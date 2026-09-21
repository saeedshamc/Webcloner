use crate::broken_links;
use crate::job_state::{JobOptionsSnapshot, JobState};
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredPage {
    pub url: String,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverResult {
    pub start_url: String,
    pub pages: Vec<DiscoveredPage>,
    pub truncated: bool,
    pub message: String,
}

pub struct DiscoverOptions {
    pub start_url: String,
    /// Safety cap so discovery cannot run forever.
    pub max_pages_cap: usize,
    pub max_depth: usize,
    pub follow_external_pages: bool,
    pub block_tracking: bool,
    pub timeout_secs: u64,
    pub user_agent: String,
    pub on_progress: Option<Arc<dyn Fn(String) + Send + Sync>>,
    pub cancel_flag: Option<Arc<AtomicBool>>,
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
    pub block_tracking: bool,
    pub report_broken_links: bool,
    /// If set, only these pages are downloaded (user-confirmed discovery list).
    pub planned_pages: Option<Vec<String>>,
    /// Continue from `.webcloner-job.json` in out_dir.
    pub resume: bool,
    pub on_progress: Option<Arc<dyn Fn(String) + Send + Sync>>,
    pub on_progress_event: Option<Arc<dyn Fn(ProgressEvent) + Send + Sync>>,
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

pub async fn discover_async(opts: DiscoverOptions) -> Result<DiscoverResult> {
    let start_url = Url::parse(&opts.start_url)
        .with_context(|| format!("آدرس «{}» معتبر نیست.", opts.start_url))?;
    let site_host = start_url
        .host_str()
        .context("آدرس شروع میزبان (host) ندارد.")?
        .to_string();

    let client = Client::builder()
        .user_agent(opts.user_agent.clone())
        .timeout(Duration::from_secs(opts.timeout_secs))
        .connect_timeout(Duration::from_secs(opts.timeout_secs.min(15)))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?;

    let cap = opts.max_pages_cap.max(1);
    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(Url, usize)> = VecDeque::new();
    let mut pages: Vec<DiscoveredPage> = Vec::new();
    queue.push_back((start_url.clone(), 0));
    visited.insert(normalize(&start_url));

    let log = |m: String| {
        println!("{m}");
        if let Some(cb) = &opts.on_progress {
            cb(m);
        }
    };

    log(format!("کشف صفحات از {start_url} ..."));
    let mut truncated = false;

    while let Some((page_url, depth)) = queue.pop_front() {
        if opts
            .cancel_flag
            .as_ref()
            .map(|f| f.load(Ordering::SeqCst))
            .unwrap_or(false)
        {
            bail!("کشف آدرس‌ها لغو شد.");
        }
        if pages.len() >= cap {
            truncated = true;
            break;
        }

        let resp = match client.get(page_url.clone()).send().await {
            Ok(r) => r,
            Err(_) => continue,
        };
        if !resp.status().is_success() {
            continue;
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(_) => continue,
        };
        if !utils::is_probably_html(content_type.as_deref(), &bytes) {
            continue;
        }
        let html = String::from_utf8_lossy(&bytes).to_string();
        pages.push(DiscoveredPage {
            url: page_url.to_string(),
            depth,
        });
        if pages.len() % 10 == 0 {
            log(format!("کشف شد: {} صفحه...", pages.len()));
        }

        if depth >= opts.max_depth {
            continue;
        }
        for link in rewriter::extract_page_links(&html, &page_url) {
            let link = link.resolved;
            if opts.block_tracking && is_tracking_url(&link) {
                continue;
            }
            let same_host = link.host_str() == Some(site_host.as_str());
            if !same_host && !opts.follow_external_pages {
                continue;
            }
            if !matches!(link.scheme(), "http" | "https") {
                continue;
            }
            let key = normalize(&link);
            if visited.insert(key) {
                queue.push_back((link, depth + 1));
            }
        }
    }

    let message = if truncated {
        format!(
            "{} آدرس پیدا شد (به سقف ایمنی {} رسید — لیست ناقص است).",
            pages.len(),
            cap
        )
    } else {
        format!("{} آدرس صفحه برای کلون پیدا شد.", pages.len())
    };
    log(message.clone());

    Ok(DiscoverResult {
        start_url: start_url.to_string(),
        pages,
        truncated,
        message,
    })
}

pub async fn run_async(opts: CrawlOptions) -> Result<()> {
    let start_url = Url::parse(&opts.start_url)
        .with_context(|| format!("آدرس «{}» معتبر نیست.", opts.start_url))?;
    let site_host = start_url
        .host_str()
        .context("آدرس شروع میزبان (host) ندارد.")?
        .to_string();

    std::fs::create_dir_all(&opts.out_dir)?;

    let mut done_pages: HashSet<String> = HashSet::new();
    let mut done_assets: HashSet<String> = HashSet::new();
    let mut resumed_map: HashMap<String, PathBuf> = HashMap::new();
    let mut planned_from_resume: Option<Vec<String>> = None;

    if opts.resume {
        if let Some(state) = JobState::load(&opts.out_dir)? {
            opts.log(format!(
                "ادامه از وضعیت قبلی: {} صفحه انجام‌شده از {}",
                state.done_pages.len(),
                state.planned_pages.len()
            ));
            done_pages = state.done_pages.into_iter().collect();
            done_assets = state.done_assets.into_iter().collect();
            resumed_map = state
                .url_map
                .into_iter()
                .map(|(k, v)| (k, PathBuf::from(v)))
                .collect();
            planned_from_resume = Some(state.planned_pages);
        } else {
            bail!("فایل ادامه دانلود (.webcloner-job.json) پیدا نشد.");
        }
    }

    let client = Client::builder()
        .user_agent(opts.user_agent.clone())
        .timeout(Duration::from_secs(opts.timeout_secs))
        .connect_timeout(Duration::from_secs(opts.timeout_secs.min(15)))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("ساخت کلاینت HTTP ناموفق بود.")?;

    let url_map: Arc<Mutex<HashMap<String, PathBuf>>> = Arc::new(Mutex::new(resumed_map));
    let semaphore = Arc::new(tokio::sync::Semaphore::new(opts.concurrency.max(1)));

    let mut pages: Vec<PageRecord> = Vec::new();
    let mut css_records: Vec<CssRecord> = Vec::new();
    let mut fetch_errors: Vec<String> = Vec::new();
    let mut pending_assets: HashSet<String> = HashSet::new();
    let mut asset_download_queue: VecDeque<Url> = VecDeque::new();

    // Restore planned assets not yet done when resuming.
    if opts.resume {
        if let Some(state) = JobState::load(&opts.out_dir)? {
            for a in state.planned_assets {
                if !done_assets.contains(&a) {
                    if let Ok(u) = Url::parse(&a) {
                        pending_assets.insert(a.clone());
                        asset_download_queue.push_back(u);
                    }
                }
            }
        }
    }

    let planned_list: Option<Vec<Url>> = if let Some(list) = planned_from_resume.or(opts.planned_pages.clone())
    {
        let mut urls = Vec::new();
        for s in list {
            match Url::parse(&s) {
                Ok(u) => urls.push(u),
                Err(_) => fetch_errors.push(format!("آدرس نامعتبر در لیست: {s}")),
            }
        }
        Some(urls)
    } else {
        None
    };

    let mut cancelled = false;

    if let Some(planned) = planned_list {
        let total = planned.len();
        opts.emit(
            "pages",
            format!("دانلود {} صفحه تأییدشده...", total),
            done_pages.len(),
            total,
            done_assets.len(),
            pending_assets.len(),
        );

        for page_url in planned {
            if opts.cancelled() {
                cancelled = true;
                break;
            }
            let key = normalize(&page_url);
            if done_pages.contains(&key) {
                // Reload HTML from disk if present for rewrite phase.
                if let Some(rel) = url_map.lock().unwrap().get(&key).cloned() {
                    let full = opts.out_dir.join(&rel);
                    if let Ok(raw) = std::fs::read_to_string(&full) {
                        pages.push(PageRecord {
                            url: page_url,
                            rel_path: rel,
                            raw_html: raw,
                        });
                    }
                }
                continue;
            }

            match fetch_html_page(&client, &page_url).await {
                Ok((html, rel_path)) => {
                    opts.emit(
                        "pages",
                        format!("صفحه [{}] {page_url}", done_pages.len() + 1),
                        done_pages.len() + 1,
                        total,
                        done_assets.len(),
                        pending_assets.len(),
                    );
                    url_map
                        .lock()
                        .unwrap()
                        .insert(key.clone(), rel_path.clone());
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
                    pages.push(PageRecord {
                        url: page_url,
                        rel_path,
                        raw_html: html,
                    });
                    done_pages.insert(key);
                    save_job_checkpoint(&opts, &site_host, &done_pages, &done_assets, &url_map, &pending_assets)?;
                }
                Err(msg) => {
                    opts.log(&msg);
                    fetch_errors.push(msg);
                }
            }
        }

        // Persist full planned list for resume.
        let planned_keys: Vec<String> = {
            // Include already done + current pages we intended — from job or opts
            if let Some(p) = &opts.planned_pages {
                p.clone()
            } else if let Ok(Some(st)) = JobState::load(&opts.out_dir) {
                st.planned_pages
            } else {
                pages.iter().map(|p| p.url.to_string()).collect()
            }
        };
        write_full_plan_checkpoint(
            &opts,
            &site_host,
            &planned_keys,
            &done_pages,
            &done_assets,
            &url_map,
            &pending_assets,
        )?;
    } else {
        // Classic crawl mode with max_pages / max_depth.
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

        while let Some((page_url, depth)) = queue.pop_front() {
            if opts.cancelled() {
                cancelled = true;
                break;
            }
            if pages.len() >= opts.max_pages {
                break;
            }
            let key = normalize(&page_url);
            if done_pages.contains(&key) {
                continue;
            }

            match fetch_html_page(&client, &page_url).await {
                Ok((html, rel_path)) => {
                    opts.emit(
                        "pages",
                        format!("صفحه [{}] {page_url}", pages.len() + 1),
                        pages.len() + 1,
                        opts.max_pages,
                        0,
                        asset_download_queue.len() + pending_assets.len(),
                    );
                    url_map
                        .lock()
                        .unwrap()
                        .insert(key.clone(), rel_path.clone());
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
                                let k = normalize(&link);
                                if visited_pages.insert(k) {
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
                    done_pages.insert(key);
                    let planned: Vec<String> = pages.iter().map(|p| p.url.to_string()).collect();
                    write_full_plan_checkpoint(
                        &opts,
                        &site_host,
                        &planned,
                        &done_pages,
                        &done_assets,
                        &url_map,
                        &pending_assets,
                    )?;
                }
                Err(msg) => {
                    opts.log(&msg);
                    fetch_errors.push(msg);
                }
            }
        }
    }

    if cancelled {
        opts.emit(
            "cancelled",
            format!(
                "دانلود لغو شد. برای ادامه، «ادامه دانلود» را بزنید.\nپوشه: {}",
                opts.out_dir.display()
            ),
            done_pages.len(),
            opts.max_pages.max(done_pages.len()),
            done_assets.len(),
            pending_assets.len(),
        );
        bail!("دانلود توسط کاربر لغو شد.");
    }

    if pages.is_empty() && done_pages.is_empty() {
        let details = if fetch_errors.is_empty() {
            "اتصال به اینترنت، فایروال، یا آدرس سایت را بررسی کنید.".to_string()
        } else {
            fetch_errors.join("\n")
        };
        bail!("هیچ صفحه‌ای دانلود نشد.\n{details}");
    }

    // Filter asset queue against already-done assets.
    {
        let mut filtered = VecDeque::new();
        while let Some(u) = asset_download_queue.pop_front() {
            let k = normalize(&u);
            if !done_assets.contains(&k) {
                filtered.push_back(u);
            }
        }
        asset_download_queue = filtered;
    }

    let assets_known = asset_download_queue.len() + done_assets.len();
    opts.emit(
        "assets",
        format!(
            "{} صفحه آماده. در حال دانلود {} فایل...",
            pages.len().max(done_pages.len()),
            asset_download_queue.len()
        ),
        pages.len().max(done_pages.len()),
        pages.len().max(done_pages.len()),
        done_assets.len(),
        assets_known.max(1),
    );

    let mut assets_done = done_assets.len();
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
                        let key = normalize(&asset_url);
                        done_assets.insert(key.clone());
                        url_map
                            .lock()
                            .unwrap()
                            .insert(key, rel_path.clone());

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

        save_job_checkpoint(
            &opts,
            &site_host,
            &done_pages,
            &done_assets,
            &url_map,
            &pending_assets,
        )?;

        opts.emit(
            "assets",
            format!("asset: {assets_done} دریافت شد"),
            pages.len().max(done_pages.len()),
            pages.len().max(done_pages.len()),
            assets_done,
            pending_assets.len().max(assets_done),
        );
    }

    if cancelled || opts.cancelled() {
        opts.emit(
            "cancelled",
            format!(
                "دانلود لغو شد. پیشرفت ذخیره شد — می‌توانید ادامه دهید.\n{}",
                opts.out_dir.display()
            ),
            pages.len().max(done_pages.len()),
            pages.len().max(done_pages.len()),
            assets_done,
            pending_assets.len(),
        );
        bail!("دانلود توسط کاربر لغو شد.");
    }

    opts.emit(
        "rewrite",
        "بازنویسی لینک‌ها برای استفاده آفلاین...",
        pages.len().max(done_pages.len()),
        pages.len().max(done_pages.len()),
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
            }
            Ok(_) => {
                broken_note = "\nلینک شکستهٔ محلی پیدا نشد.".into();
            }
            Err(e) => {
                opts.log(format!("اسکن لینک شکسته ناموفق: {e}"));
            }
        }
    }

    let _ = JobState::clear(&opts.out_dir);

    opts.emit(
        "done",
        format!(
            "تمام شد. سایت آفلاین در: {}{}",
            opts.out_dir.display(),
            broken_note
        ),
        pages.len().max(done_pages.len()),
        pages.len().max(done_pages.len()),
        assets_done,
        assets_done,
    );

    Ok(())
}

fn snapshot_options(opts: &CrawlOptions) -> JobOptionsSnapshot {
    JobOptionsSnapshot {
        include_external_assets: opts.include_external_assets,
        follow_external_pages: opts.follow_external_pages,
        concurrency: opts.concurrency,
        timeout_secs: opts.timeout_secs,
        user_agent: opts.user_agent.clone(),
        block_tracking: opts.block_tracking,
        report_broken_links: opts.report_broken_links,
    }
}

fn map_to_strings(url_map: &Mutex<HashMap<String, PathBuf>>) -> HashMap<String, String> {
    url_map
        .lock()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.display().to_string()))
        .collect()
}

fn save_job_checkpoint(
    opts: &CrawlOptions,
    site_host: &str,
    done_pages: &HashSet<String>,
    done_assets: &HashSet<String>,
    url_map: &Mutex<HashMap<String, PathBuf>>,
    pending_assets: &HashSet<String>,
) -> Result<()> {
    let planned_pages = if let Ok(Some(st)) = JobState::load(&opts.out_dir) {
        st.planned_pages
    } else {
        done_pages.iter().cloned().collect()
    };
    write_full_plan_checkpoint(
        opts,
        site_host,
        &planned_pages,
        done_pages,
        done_assets,
        url_map,
        pending_assets,
    )
}

fn write_full_plan_checkpoint(
    opts: &CrawlOptions,
    site_host: &str,
    planned_pages: &[String],
    done_pages: &HashSet<String>,
    done_assets: &HashSet<String>,
    url_map: &Mutex<HashMap<String, PathBuf>>,
    pending_assets: &HashSet<String>,
) -> Result<()> {
    let state = JobState {
        version: 1,
        start_url: opts.start_url.clone(),
        site_host: site_host.to_string(),
        planned_pages: planned_pages.to_vec(),
        done_pages: done_pages.iter().cloned().collect(),
        planned_assets: pending_assets
            .iter()
            .chain(done_assets.iter())
            .cloned()
            .collect(),
        done_assets: done_assets.iter().cloned().collect(),
        url_map: map_to_strings(url_map),
        options: snapshot_options(opts),
        phase: "running".into(),
        updated_at: JobState::now_secs(),
    };
    state.save(&opts.out_dir)
}

async fn fetch_html_page(client: &Client, page_url: &Url) -> Result<(String, PathBuf), String> {
    let resp = client
        .get(page_url.clone())
        .send()
        .await
        .map_err(|e| format!("رد صفحه {page_url} ({e})"))?;
    if !resp.status().is_success() {
        return Err(format!("رد صفحه {page_url} (HTTP {})", resp.status()));
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("رد صفحه {page_url} (خواندن بدنه: {e})"))?;
    if !utils::is_probably_html(content_type.as_deref(), &bytes) {
        return Err(format!("رد صفحه غیر HTML: {page_url}"));
    }
    let host = page_url.host_str().unwrap_or("site");
    let rel_path = utils::url_to_local_path(page_url, host);
    Ok((String::from_utf8_lossy(&bytes).to_string(), rel_path))
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
    if HOSTS
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")))
    {
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
