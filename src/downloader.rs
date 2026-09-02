use crate::rewriter;
use crate::utils;
use anyhow::{Context, Result, bail};
use reqwest::Client;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use url::Url;

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
    /// Optional callback for live progress messages (used by the GUI).
    pub on_progress: Option<Arc<dyn Fn(String) + Send + Sync>>,
}

impl CrawlOptions {
    fn log(&self, message: impl Into<String>) {
        let message = message.into();
        println!("{message}");
        if let Some(cb) = &self.on_progress {
            cb(message);
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
        .context("failed to start async runtime")?;
    rt.block_on(run_async(opts))
}

pub async fn run_async(opts: CrawlOptions) -> Result<()> {
    let start_url = Url::parse(&opts.start_url)
        .with_context(|| format!("'{}' is not a valid URL", opts.start_url))?;
    let site_host = start_url
        .host_str()
        .context("start URL has no host")?
        .to_string();

    std::fs::create_dir_all(&opts.out_dir)?;

    let client = Client::builder()
        .user_agent(opts.user_agent.clone())
        .timeout(Duration::from_secs(opts.timeout_secs))
        .connect_timeout(Duration::from_secs(opts.timeout_secs.min(15)))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("failed to build HTTP client")?;

    // --- Global state shared across the whole crawl ---
    let url_map: Arc<Mutex<HashMap<String, PathBuf>>> = Arc::new(Mutex::new(HashMap::new()));
    let semaphore = Arc::new(tokio::sync::Semaphore::new(opts.concurrency.max(1)));

    let mut pages: Vec<PageRecord> = Vec::new();
    let mut css_records: Vec<CssRecord> = Vec::new();
    let mut fetch_errors: Vec<String> = Vec::new();

    let mut visited_pages: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(Url, usize)> = VecDeque::new();
    queue.push_back((start_url.clone(), 0));
    visited_pages.insert(normalize(&start_url));

    opts.log(format!(
        "🌐 Cloning {start_url} → {}",
        opts.out_dir.display()
    ));

    let mut pending_assets: HashSet<String> = HashSet::new();
    let mut asset_download_queue: VecDeque<Url> = VecDeque::new();

    while let Some((page_url, depth)) = queue.pop_front() {
        if pages.len() >= opts.max_pages {
            break;
        }

        let resp = match client.get(page_url.clone()).send().await {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("⚠ skip page {page_url} ({e})");
                opts.log(&msg);
                fetch_errors.push(msg);
                continue;
            }
        };
        if !resp.status().is_success() {
            let msg = format!("⚠ skip page {page_url} (HTTP {})", resp.status());
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
                let msg = format!("⚠ skip page {page_url} (read body: {e})");
                opts.log(&msg);
                fetch_errors.push(msg);
                continue;
            }
        };

        if !utils::is_probably_html(content_type.as_deref(), &bytes) {
            let msg = format!("⚠ skip non-HTML page {page_url}");
            opts.log(&msg);
            fetch_errors.push(msg);
            continue;
        }
        let html = String::from_utf8_lossy(&bytes).to_string();
        opts.log(format!("📄 [{}] {page_url}", pages.len() + 1));

        let rel_path = utils::url_to_local_path(&page_url, &site_host);
        url_map
            .lock()
            .unwrap()
            .insert(normalize(&page_url), rel_path.clone());

        // Discover assets referenced by this page.
        for reference in rewriter::extract_html_asset_refs(&html, &page_url) {
            queue_asset(
                &reference.resolved,
                &site_host,
                opts.include_external_assets,
                &mut pending_assets,
                &mut asset_download_queue,
            );
        }

        // Discover further pages to crawl (same-domain by default).
        if depth < opts.max_depth {
            for link in rewriter::extract_page_links(&html, &page_url) {
                let link = link.resolved;
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

    if pages.is_empty() {
        let details = if fetch_errors.is_empty() {
            "اتصال به اینترنت، فایروال، یا آدرس سایت را بررسی کنید.".to_string()
        } else {
            fetch_errors.join("\n")
        };
        bail!("هیچ صفحه‌ای دانلود نشد.\n{details}");
    }

    opts.log(format!(
        "✅ Crawled {} page(s). Downloading {} asset(s)...",
        pages.len(),
        asset_download_queue.len()
    ));

    // ---------- Phase 2: download assets (recursively resolving CSS-referenced assets) ----------
    while !asset_download_queue.is_empty() {
        let batch: Vec<Url> = asset_download_queue.drain(..).collect();
        let mut handles = Vec::new();

        for asset_url in batch {
            let client = client.clone();
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let site_host = site_host.clone();
            let out_dir = opts.out_dir.clone();

            handles.push(tokio::spawn(async move {
                let _permit = permit;
                let result = download_asset(&client, &asset_url, &site_host, &out_dir).await;
                (asset_url, result)
            }));
        }

        for handle in handles {
            if let Ok((asset_url, result)) = handle.await {
                match result {
                    Ok(DownloadedAsset { rel_path, css_text }) => {
                        url_map
                            .lock()
                            .unwrap()
                            .insert(normalize(&asset_url), rel_path.clone());

                        if let Some(css_text) = css_text {
                            // Discover assets referenced from inside this CSS file
                            // (fonts, background images, @import chains) and queue
                            // anything new for the next batch.
                            for reference in
                                rewriter::extract_css_asset_refs(&css_text, &asset_url)
                            {
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
                        opts.log(format!("⚠ asset failed {asset_url} ({e})"));
                    }
                }
            }
        }
    }

    opts.log(format!(
        "✅ Downloaded {} asset(s).",
        url_map.lock().unwrap().len()
    ));
    opts.log("✍️  Rewriting references for offline use...");

    // ---------- Phase 3: rewrite + write everything to disk ----------
    let map_snapshot = url_map.lock().unwrap().clone();

    for page in &pages {
        let refs = rewriter::extract_html_asset_refs(&page.raw_html, &page.url);
        let page_links = rewriter::extract_page_links(&page.raw_html, &page.url);

        // Rewrite asset refs (href/src/etc.) to local relative paths.
        let rewritten = rewriter::rewrite_references(&page.raw_html, &refs, |u| {
            map_snapshot
                .get(&normalize(u))
                .map(|target| utils::relative_path(&page.rel_path, target))
        });

        // Also rewrite <a href> links that point to pages we actually cloned,
        // so internal navigation works offline too. Links we didn't crawl are
        // left pointing at the live site.
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

    // Make sure there's an index.html at the root pointing at the start page,
    // so double-clicking the folder / opening file:// just works.
    ensure_root_index(&opts.out_dir, &pages, &site_host)?;

    opts.log(format!(
        "🎉 Done. Offline site saved to: {}\n   Open {}/index.html in a browser, or run:\n   webcloner serve {}",
        opts.out_dir.display(),
        opts.out_dir.display(),
        opts.out_dir.display()
    ));

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
    /// Set for CSS files so the caller can scan them for nested url()/@import refs.
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
