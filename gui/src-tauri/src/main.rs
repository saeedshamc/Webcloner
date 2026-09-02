#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::Manager;
use url::Url;
use webcloner::{downloader, zipper};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadOptions {
    url: String,
    save_dir: String,
    out_name: String,
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

fn normalize_start_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("آدرس سایت خالی است.".into());
    }

    let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{}", trimmed.trim_start_matches('/'))
    };

    Url::parse(&with_scheme)
        .map(|u| u.to_string())
        .map_err(|_| format!("آدرس «{input}» معتبر نیست."))
}

fn sanitize_folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '\\' | '|' | '?' | '*' | '/' => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "cloned-site".to_string()
    } else {
        trimmed.to_string()
    }
}

fn resolve_output_dir(save_dir: &str, out_name: &str) -> Result<PathBuf, String> {
    let base = PathBuf::from(save_dir.trim());
    if base.as_os_str().is_empty() {
        return Err("محل ذخیره‌سازی را انتخاب کنید.".into());
    }
    if !base.is_absolute() {
        return Err("مسیر ذخیره‌سازی باید کامل باشد. دوباره پوشه را انتخاب کنید.".into());
    }
    if !base.exists() {
        return Err(format!("پوشه وجود ندارد: {}", base.display()));
    }
    if !base.is_dir() {
        return Err(format!("مسیر انتخاب‌شده پوشه نیست: {}", base.display()));
    }

    Ok(base.join(sanitize_folder_name(out_name)))
}

fn default_save_dir() -> String {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let docs = PathBuf::from(&profile).join("Documents");
        if docs.is_dir() {
            return docs.display().to_string();
        }
        return profile;
    }
    if let Ok(home) = std::env::var("HOME") {
        return home;
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .display()
        .to_string()
}

fn run_download(options: DownloadOptions) -> Result<DownloadResult, String> {
    let start_url = normalize_start_url(&options.url)?;
    let out_dir = resolve_output_dir(&options.save_dir, &options.out_name)?;

    let opts = downloader::CrawlOptions {
        start_url,
        out_dir: out_dir.clone(),
        max_pages: options.max_pages.max(1),
        max_depth: options.max_depth,
        include_external_assets: options.include_external_assets,
        follow_external_pages: options.follow_external_pages,
        concurrency: options.concurrency.max(1),
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
fn get_default_save_dir() -> String {
    default_save_dir()
}

#[tauri::command]
fn pick_output_folder() -> Result<Option<String>, String> {
    let picked = tauri::api::dialog::blocking::FileDialogBuilder::new()
        .set_title("انتخاب پوشه ذخیره‌سازی")
        .pick_folder();

    Ok(picked.map(|p| p.display().to_string()))
}

#[tauri::command]
async fn download_site(options: DownloadOptions) -> Result<DownloadResult, String> {
    tauri::async_runtime::spawn_blocking(move || run_download(options))
        .await
        .map_err(|e| format!("خطا در اجرای دانلود: {e}"))?
}

#[tauri::command]
fn open_folder(path: String, window: tauri::Window) -> Result<(), String> {
    let folder = Path::new(&path);
    if !folder.exists() {
        return Err(format!("پوشه پیدا نشد: {}", folder.display()));
    }
    tauri::api::shell::open(&window.shell_scope(), path, None).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_default_save_dir,
            pick_output_folder,
            download_site,
            open_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running webcloner GUI");
}
