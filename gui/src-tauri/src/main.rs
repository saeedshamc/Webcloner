#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use webcloner::{downloader, zipper};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadOptions {
    url: String,
    out: String,
    max_pages: usize,
    max_depth: usize,
    concurrency: usize,
    include_external_assets: bool,
    follow_external_pages: bool,
    zip: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadResult {
    out_dir: String,
    message: String,
}

#[tauri::command]
fn download_site(options: DownloadOptions) -> Result<DownloadResult, String> {
    let out_dir = PathBuf::from(&options.out);

    let opts = downloader::CrawlOptions {
        start_url: options.url,
        out_dir: out_dir.clone(),
        max_pages: options.max_pages,
        max_depth: options.max_depth,
        include_external_assets: options.include_external_assets,
        follow_external_pages: options.follow_external_pages,
        concurrency: options.concurrency,
        timeout_secs: 20,
        user_agent: "webcloner-gui/1.0 (+offline mirror tool)".to_string(),
    };

    downloader::run(opts).map_err(|e| e.to_string())?;

    let mut message = format!(
        "دانلود با موفقیت انجام شد.\nپوشه: {}",
        out_dir.display()
    );

    if options.zip {
        let zip_path = out_dir.with_extension("zip");
        zipper::zip_dir(&out_dir, &zip_path).map_err(|e| e.to_string())?;
        message.push_str(&format!("\nفایل ZIP: {}", zip_path.display()));
    }

    Ok(DownloadResult {
        out_dir: out_dir.display().to_string(),
        message,
    })
}

#[tauri::command]
fn open_folder(path: String) -> Result<(), String> {
    tauri::api::shell::open(&path).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![download_site, open_folder])
        .run(tauri::generate_context!())
        .expect("error while running webcloner GUI");
}
