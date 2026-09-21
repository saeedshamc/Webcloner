use crate::broken_links;
use crate::rewriter;
use crate::utils;
use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use url::Url;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub phase: String,
    pub message: String,
    pub pages_done: usize,
    pub pages_total: usize,
    pub assets_done: usize,
    pub assets_known: usize,
}

pub struct CrawlOptions {
    pub start_url: String,
    pub out_dir: PathBuf,
    pub max_pages: usize,
    pub max_depth: usize,
    pub include_external_assets: bool,
    pub follow_external_pages: bool,
    pub concurrency: usize,
    pub timeout_secs: u64,
    pub user_agent: String,
    /// Skip known analytics/ads hosts when true.
    pub block_tracking: bool,
    /// Write broken-links.txt after clone when true.
    pub report_broken_links: bool,
    /// Optional callback for live progress messages (used by the GUI).
    pub on_progress: Option<Arc<dyn Fn(String) + Send + Sync>>,
    /// Structured progress for GUI progress bars.
    pub on_progress_event: Option<Arc<dyn Fn(ProgressEvent) + Send + Sync>>,
    /// Set to true to request cooperative cancellation.
    pub cancel_flag: Option<Arc<AtomicBool>>,
}

impl CrawlOptions {
    fn cancelled(&self) -> bool {
        self.cancel_flag
            .as_ref()
            .map(|f| f.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    fn log(&self, message: impl Into<String>) {
        let message = message.into();
        println!("{message}");
        if let Some(cb) = &self.on_progress {
            cb(message.clone());
        }
    }

    fn emit(
        &self,
        phase: &str,
        message: impl Into<String>,
        pages_done: usize,
        pages_total: usize,
        assets_done: usize,
        assets_known: usize,
    ) {
        let message = message.into();
        self.log(&message);
        if let Some(cb) = &self.on_progress_event {
            cb(ProgressEvent {
                phase: phase.to_string(),
                message,
                pages_done,
                pages_total,
                assets_done,
                assets_known,
            });
        }
    }
}

struct PageRecord {
    url: Url,
    rel_path: PathBuf,
    raw_html: String,
}

struct CssRecord {
    url: Url,
    rel_path: PathBuf,
    raw_css: String,
}

pub fn run(opts: CrawlOptions) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("ساخت runtime ناموفق بود.")?;
    rt.block_on(run_async(opts))
}

pub async fn run_async(opts: CrawlOptions) -> Result<()> {
    let start_url = Url::parse(&opts.start_url)
        .with_context(|| format!("آدرس «{}» معتبر نیست.", opts.start_url))?;
    let site_host = start_url
        .host_str()
        .context("آدرس شروع میزبان (host) ندارد.")?
        .to_string();

    std::fs::create_dir_all(&opts.out_dir)?;

    let client = Client::builder()
        .user_agent(opts.user_agent.clone())
        .timeout(Duration::from_secs(opts.timeout_secs))
        .connect_timeout(Duration::from_secs(opts.timeout_secs.min(15)))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("ساخت کلاینت HTTP ناموفق بود.")?;

    let url_map: Arc<Mutex<HashMap<String, PathBuf>>> = Arc::new(Mutex::new(HashMap::new()));
    let semaphore = Arc::new(tokio::sync::Semaphore::new(opts.concurrency.max(1)));

    let mut pages: Vec<PageRecord> = Vec::new();
    let mut css_records: Vec<CssRecord> = Vec::new();
    let mut fetch_errors: Vec<String> = Vec::new();

    let mut visited_pages: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(Url, usize)> = VecDeque::new();
    queue.push_back((start_url.clone(), 0));
    visited_pages.insert(normalize(&start_url));

    opts.emit(
        "pages",
        format!("در حال کلون {start_url} → {}", opts.out_dir.display()),
        0,
        opts.max_pages,
        0,
        0,
    );

    let mut pending_assets: HashSet<String> = HashSet::new();
    let mut asset_download_queue: VecDeque<Url> = VecDeque::new();
    let mut cancelled = false;

    while let Some((page_url, depth)) = queue.pop_front() {
        if opts.cancelled() {
            cancelled = true;
            break;
        }
        if pages.len() >= opts.max_pages {
            break;
        }

        let resp = match client.get(page_url.clone()).send().await {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("رد صفحه {page_url} ({e})");
                opts.log(&msg);
                fetch_errors.push(msg);
                continue;
            }
        };
        if !resp.status().is_success() {
            let msg = format!("رد صفحه {page_url} (HTTP {})", resp.status());
            opts.log(&msg);
            fetch_errors.push(msg);
            continue;
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                let msg = format!("رد صفحه {page_url} (خواندن بدنه: {e})");
                opts.log(&msg);
                fetch_errors.push(msg);
                continue;
            }
        };

        if !utils::is_probably_html(content_type.as_deref(), &bytes) {
            let msg = format!("رد صفحه غیر HTML: {page_url}");
            opts.log(&msg);
            fetch_errors.push(msg);
            continue;
        }
        let html = String::from_utf8_lossy(&bytes).to_string();
        opts.emit(
            "pages",
            format!("صفحه [{}] {page_url}", pages.len() + 1),
            pages.len() + 1,
            opts.max_pages,
            0,
            asset_download_queue.len() + pending_assets.len(),
        );

        let rel_path = utils::url_to_local_path(&page_url, &site_host);
        url_map
            .lock()
            .unwrap()
            .insert(normalize(&page_url), rel_path.clone());

        for reference in rewriter::extract_html_asset_refs(&html, &page_url) {
            if opts.block_tracking && is_tracking_url(&reference.resolved) {
                continue;
            }
            queue_asset(
                &reference.resolved,
                &site_host,
                opts.include_external_assets,
                &mut pending_assets,
                &mut asset_download_queue,
            );
        }

        if depth < opts.max_depth {
            for link in rewriter::extract_page_links(&html, &page_url) {
                let link = link.resolved;
                if opts.block_tracking && is_tracking_url(&link) {
                    continue;
                }
                let same_host = link.host_str() == Some(site_host.as_str());
                if !same_host && !opts.follow_external_pages {
                    continue;
                }
                if matches!(link.scheme(), "http" | "https") {
                    let key = normalize(&link);
                    if visited_pages.insert(key) {
                        queue.push_back((link, depth + 1));
                    }
                }
            }
        }

        pages.push(PageRecord {
            url: page_url,
            rel_path,
            raw_html: html,
        });
    }

    if cancelled {
        opts.emit(
            "cancelled",
            format!(
                "دانلود لغو شد. پوشه ناقص باقی ماند: {}",
                opts.out_dir.display()
            ),
            pages.len(),
            opts.max_pages,
            0,
            asset_download_queue.len(),
        );
        bail!("دانلود توسط کاربر لغو شد.");
    }

    if pages.is_empty() {
        let details = if fetch_errors.is_empty() {
            "اتصال به اینترنت، فایروال، یا آدرس سایت را بررسی کنید.".to_string()
        } else {
            fetch_errors.join("\n")
        };
        bail!("هیچ صفحه‌ای دانلود نشد.\n{details}");
    }

    let assets_known = asset_download_queue.len();
    opts.emit(
        "assets",
        format!(
            "{} صفحه دریافت شد. در حال دانلود {} فایل...",
            pages.len(),
            assets_known
        ),
        pages.len(),
        pages.len(),
        0,
        assets_known,
    );

    let mut assets_done = 0usize;
    while !asset_download_queue.is_empty() {
        if opts.cancelled() {
            cancelled = true;
            break;
        }
        let batch: Vec<Url> = asset_download_queue.drain(..).collect();
        let mut handles = Vec::new();

        for asset_url in batch {
            let client = client.clone();
            let permit = semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| anyhow::anyhow!("دانلود متوقف شد."))?;
            let site_host = site_host.clone();
            let out_dir = opts.out_dir.clone();
            let cancel = opts.cancel_flag.clone();

            handles.push(tokio::spawn(async move {
                let _permit = permit;
                if cancel
                    .as_ref()
                    .map(|f| f.load(Ordering::SeqCst))
                    .unwrap_or(false)
                {
                    return (asset_url, Err(anyhow::anyhow!("لغو شد")));
                }
                let result = download_asset(&client, &asset_url, &site_host, &out_dir).await;
                (asset_url, result)
            }));
        }

        for handle in handles {
            if let Ok((asset_url, result)) = handle.await {
                match result {
                    Ok(DownloadedAsset { rel_path, css_text }) => {
                        assets_done += 1;
                        url_map
                            .lock()
                            .unwrap()
                            .insert(normalize(&asset_url), rel_path.clone());

                        if let Some(css_text) = css_text {
                            for reference in
                                rewriter::extract_css_asset_refs(&css_text, &asset_url)
                            {
                                if opts.block_tracking && is_tracking_url(&reference.resolved) {
                                    continue;
                                }
                                queue_asset(
                                    &reference.resolved,
                                    &site_host,
                                    opts.include_external_assets,
                                    &mut pending_assets,
                                    &mut asset_download_queue,
                                );
                            }
                            css_records.push(CssRecord {
                                url: asset_url,
                                rel_path,
                                raw_css: css_text,
                            });
                        }
                    }
                    Err(e) => {
                        opts.log(format!("خطا در asset {asset_url} ({e})"));
                    }
                }
            }
        }

        opts.emit(
            "assets",
            format!("asset: {assets_done} دریافت شد"),
            pages.len(),
            pages.len(),
            assets_done,
            pending_assets.len().max(assets_done),
        );
    }

    if cancelled || opts.cancelled() {
        opts.emit(
            "cancelled",
            format!(
                "دانلود لغو شد. پوشه ناقص باقی ماند: {}",
                opts.out_dir.display()
            ),
            pages.len(),
            pages.len(),
            assets_done,
            pending_assets.len(),
        );
        bail!("دانلود توسط کاربر لغو شد.");
    }

    opts.emit(
        "rewrite",
        "بازنویسی لینک‌ها برای استفاده آفلاین...",
        pages.len(),
        pages.len(),
        assets_done,
        assets_done,
    );

    let map_snapshot = url_map.lock().unwrap().clone();

    for page in &pages {
        if opts.cancelled() {
            bail!("دانلود توسط کاربر لغو شد.");
        }
        let refs = rewriter::extract_html_asset_refs(&page.raw_html, &page.url);
        let page_links = rewriter::extract_page_links(&page.raw_html, &page.url);

        let rewritten = rewriter::rewrite_references(&page.raw_html, &refs, |u| {
            map_snapshot
                .get(&normalize(u))
                .map(|target| utils::relative_path(&page.rel_path, target))
        });

        let rewritten = rewriter::rewrite_references(&rewritten, &page_links, |u| {
            map_snapshot
                .get(&normalize(u))
                .map(|target| utils::relative_path(&page.rel_path, target))
        });

        write_file(&opts.out_dir, &page.rel_path, rewritten.as_bytes())?;
    }

    for css in &css_records {
        let refs = rewriter::extract_css_asset_refs(&css.raw_css, &css.url);
        let rewritten = rewriter::rewrite_references(&css.raw_css, &refs, |u| {
            map_snapshot
                .get(&normalize(u))
                .map(|target| utils::relative_path(&css.rel_path, target))
        });
        write_file(&opts.out_dir, &css.rel_path, rewritten.as_bytes())?;
    }

    ensure_root_index(&opts.out_dir, &pages, &site_host)?;

    let mut broken_note = String::new();
    if opts.report_broken_links {
        match broken_links::scan_broken_links(&opts.out_dir) {
            Ok(broken) if !broken.is_empty() => {
                let report_path = opts.out_dir.join("broken-links.txt");
                let body = broken.join("\n");
                let _ = std::fs::write(&report_path, &body);
                broken_note = format!(
                    "\n{} لینک شکسته پیدا شد → {}",
                    broken.len(),
                    report_path.display()
                );
                opts.log(format!(
                    "گزارش لینک شکسته: {} مورد در {}",
                    broken.len(),
                    report_path.display()
                ));
            }
            Ok(_) => {
                broken_note = "\nلینک شکستهٔ محلی پیدا نشد.".into();
            }
            Err(e) => {
                opts.log(format!("اسکن لینک شکسته ناموفق: {e}"));
            }
        }
    }

    opts.emit(
        "done",
        format!(
            "تمام شد. سایت آفلاین در: {}{}",
            opts.out_dir.display(),
            broken_note
        ),
        pages.len(),
        pages.len(),
        assets_done,
        assets_done,
    );

    Ok(())
}

fn ensure_root_index(out_dir: &PathBuf, pages: &[PageRecord], _site_host: &str) -> Result<()> {
    let root_index = out_dir.join("index.html");
    if root_index.exists() {
        return Ok(());
    }
    if let Some(first) = pages.first() {
        let target = utils::relative_path(&PathBuf::from("index.html"), &first.rel_path);
        let redirect = format!(
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
<meta http-equiv=\"refresh\" content=\"0; url={target}\">\
<title>Redirecting...</title></head><body>\
<p>If you are not redirected, <a href=\"{target}\">click here</a>.</p></body></html>"
        );
        std::fs::write(root_index, redirect)?;
    }
    Ok(())
}

struct DownloadedAsset {
    rel_path: PathBuf,
    css_text: Option<String>,
}

async fn download_asset(
    client: &Client,
    url: &Url,
    site_host: &str,
    out_dir: &PathBuf,
) -> Result<DownloadedAsset> {
    let resp = client.get(url.clone()).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = resp.bytes().await?;
    let rel_path = utils::url_to_local_path(url, site_host);

    write_file(out_dir, &rel_path, &bytes)?;

    let is_css = utils::is_probably_css(content_type.as_deref(), url.path());
    let css_text = if is_css {
        Some(String::from_utf8_lossy(&bytes).to_string())
    } else {
        None
    };

    Ok(DownloadedAsset { rel_path, css_text })
}

fn queue_asset(
    url: &Url,
    site_host: &str,
    include_external: bool,
    pending: &mut HashSet<String>,
    queue: &mut VecDeque<Url>,
) {
    if !matches!(url.scheme(), "http" | "https") {
        return;
    }
    let same_host = url.host_str() == Some(site_host);
    if !same_host && !include_external {
        return;
    }
    let key = normalize(url);
    if pending.insert(key) {
        queue.push_back(url.clone());
    }
}

fn is_tracking_url(url: &Url) -> bool {
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let path = url.path().to_ascii_lowercase();
    const HOSTS: &[&str] = &[
        "www.google-analytics.com",
        "google-analytics.com",
        "www.googletagmanager.com",
        "googletagmanager.com",
        "stats.g.doubleclick.net",
        "www.googleadservices.com",
        "pagead2.googlesyndication.com",
        "connect.facebook.net",
        "www.facebook.com",
        "static.hotjar.com",
        "script.hotjar.com",
        "cdn.segment.com",
        "api.segment.io",
        "cdn.mouseflow.com",
        "snap.licdn.com",
        "bat.bing.com",
        "analytics.tiktok.com",
        "mc.yandex.ru",
        "cdn.clarity.ms",
    ];
    if HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}"))) {
        return true;
    }
    path.contains("/analytics.js")
        || path.contains("gtag/js")
        || (path.contains("facebook") && path.contains("pixel"))
}

fn normalize(url: &Url) -> String {
    let mut u = url.clone();
    u.set_fragment(None);
    u.as_str().to_string()
}

fn write_file(out_dir: &PathBuf, rel_path: &PathBuf, bytes: &[u8]) -> Result<()> {
    let full_path = out_dir.join(rel_path);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(full_path, bytes)?;
    Ok(())
}
