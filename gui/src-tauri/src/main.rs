#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::Manager;
use url::Url;
use webcloner::{
    downloader, local_server::LocalServer, local_server::ProjectScan, local_server::ServerBackend,
    local_server::ServerStatus, zipper,
};

struct AppState {
    local_server: Arc<LocalServer>,
    download_active: Arc<AtomicBool>,
}

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadStatus {
    active: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartServerOptions {
    project_dir: String,
    port: u16,
    backend: ServerBackend,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartServerResult {
    url: String,
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
    if !is_absolute_path(&base) {
        return Err("مسیر ذخیره‌سازی باید کامل باشد. دوباره پوشه را انتخاب کنید.".into());
    }
    if !base.exists() {
        std::fs::create_dir_all(&base).map_err(|e| {
            format!("ساخت پوشه ذخیره‌سازی ممکن نشد ({}): {e}", base.display())
        })?;
    }
    if !base.is_dir() {
        return Err(format!("مسیر انتخاب‌شده پوشه نیست: {}", base.display()));
    }

    Ok(base.join(sanitize_folder_name(out_name)))
}

fn is_absolute_path(path: &Path) -> bool {
    if path.is_absolute() {
        return true;
    }
    let s = path.to_string_lossy();
    s.len() >= 3 && s.as_bytes()[1] == b':' && matches!(s.as_bytes()[2], b'\\' | b'/')
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

fn validate_project_dir(project_dir: &str) -> Result<PathBuf, String> {
    let dir = PathBuf::from(project_dir.trim());
    if dir.as_os_str().is_empty() {
        return Err("پوشه پروژه را انتخاب کنید.".into());
    }
    if !dir.is_absolute() {
        return Err("مسیر پروژه باید کامل باشد.".into());
    }
    if !dir.exists() {
        return Err(format!("پوشه وجود ندارد: {}", dir.display()));
    }
    if !dir.is_dir() {
        return Err(format!("مسیر انتخاب‌شده پوشه نیست: {}", dir.display()));
    }
    Ok(dir)
}

#[tauri::command]
fn get_default_save_dir() -> String {
    default_save_dir()
}

#[tauri::command]
async fn pick_output_folder() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let picked = tauri::api::dialog::blocking::FileDialogBuilder::new()
            .set_title("انتخاب پوشه ذخیره‌سازی")
            .pick_folder();
        Ok(picked.map(|p| p.display().to_string()))
    })
    .await
    .map_err(|e| format!("dialog task failed: {e}"))?
}

#[tauri::command]
async fn pick_project_folder() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let picked = tauri::api::dialog::blocking::FileDialogBuilder::new()
            .set_title("انتخاب پوشه پروژه")
            .pick_folder();
        Ok(picked.map(|p| p.display().to_string()))
    })
    .await
    .map_err(|e| format!("dialog task failed: {e}"))?
}

#[tauri::command]
fn resolve_clone_output_path(save_dir: String, out_name: String) -> Result<String, String> {
    Ok(resolve_output_dir(&save_dir, &out_name)?
        .display()
        .to_string())
}

#[tauri::command]
async fn scan_local_project(
    project_dir: String,
    state: tauri::State<'_, AppState>,
) -> Result<ProjectScan, String> {
    let dir = validate_project_dir(&project_dir)?;
    state
        .local_server
        .scan_project_async(dir)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn start_local_server(
    options: StartServerOptions,
    state: tauri::State<'_, AppState>,
) -> Result<StartServerResult, String> {
    let dir = validate_project_dir(&options.project_dir)?;
    let port = options.port.clamp(1024, 65535);
    let url = state
        .local_server
        .start_async(dir, port, options.backend)
        .await
        .map_err(|e| e.to_string())?;

    let backend_label = match options.backend {
        ServerBackend::Static => "استاتیک",
        ServerBackend::Php => "PHP",
        ServerBackend::AspNet => "ASP.NET",
    };

    Ok(StartServerResult {
        url: url.clone(),
        message: format!("سرور {backend_label} روی پورت {port} فعال شد:\n{url}"),
    })
}

#[tauri::command]
async fn stop_local_server(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .local_server
        .stop_async()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_local_server_status(state: tauri::State<'_, AppState>) -> Result<ServerStatus, String> {
    Ok(state.local_server.status_async().await)
}

#[tauri::command]
fn get_download_status(state: tauri::State<AppState>) -> DownloadStatus {
    DownloadStatus {
        active: state.download_active.load(Ordering::SeqCst),
    }
}

#[tauri::command]
async fn download_site(
    options: DownloadOptions,
    window: tauri::Window,
    state: tauri::State<'_, AppState>,
) -> Result<DownloadResult, String> {
    if state
        .download_active
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("یک دانلود دیگر در حال اجراست.".into());
    }

    struct DownloadReset {
        flag: Arc<AtomicBool>,
    }

    impl Drop for DownloadReset {
        fn drop(&mut self) {
            self.flag.store(false, Ordering::SeqCst);
        }
    }

    let _reset = DownloadReset {
        flag: state.download_active.clone(),
    };

    let start_url = normalize_start_url(&options.url)?;
    let out_dir = resolve_output_dir(&options.save_dir, &options.out_name)?;

    let window_for_progress = window.clone();
    let progress = Arc::new(move |line: String| {
        let _ = window_for_progress.emit("download-progress", line);
    });

    let crawl_opts = downloader::CrawlOptions {
        start_url,
        out_dir: out_dir.clone(),
        max_pages: options.max_pages.max(1),
        max_depth: options.max_depth,
        include_external_assets: options.include_external_assets,
        follow_external_pages: options.follow_external_pages,
        concurrency: options.concurrency.max(1),
        timeout_secs: 30,
        user_agent: "webcloner-gui/1.0 (+offline mirror tool)".to_string(),
        on_progress: Some(progress),
    };

    downloader::run_async(crawl_opts)
        .await
        .map_err(|e| e.to_string())?;

    let mut message = format!(
        "دانلود با موفقیت انجام شد.\nپوشه: {}",
        out_dir.display()
    );

    if options.zip {
        let zip_path = out_dir.with_extension("zip");
        let out_for_zip = out_dir.clone();
        let zip_display = zip_path.display().to_string();
        tauri::async_runtime::spawn_blocking(move || zipper::zip_dir(&out_for_zip, &zip_path))
            .await
            .map_err(|e| format!("خطا در ساخت ZIP: {e}"))?
            .map_err(|e| e.to_string())?;
        message.push_str(&format!("\nفایل ZIP: {zip_display}"));
    }

    Ok(DownloadResult {
        out_dir: out_dir.display().to_string(),
        message,
    })
}

#[tauri::command]
async fn open_folder(path: String, window: tauri::Window) -> Result<(), String> {
    let folder = Path::new(&path);
    if !folder.exists() {
        return Err(format!("پوشه پیدا نشد: {}", folder.display()));
    }
    tauri::async_runtime::spawn_blocking(move || {
        tauri::api::shell::open(&window.shell_scope(), path, None).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("open folder task failed: {e}"))?
}

#[tauri::command]
async fn open_url(url: String, window: tauri::Window) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        tauri::api::shell::open(&window.shell_scope(), url, None).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("open url task failed: {e}"))?
}

fn main() {
    tauri::Builder::default()
        .manage(AppState {
            local_server: Arc::new(LocalServer::new()),
            download_active: Arc::new(AtomicBool::new(false)),
        })
        .setup(|app| {
            let state = app.state::<AppState>();
            if let Some(resource_dir) = app.path_resolver().resource_dir() {
                state
                    .local_server
                    .add_runtimes_root(resource_dir.join("runtimes"));
            }
            let dev_runtimes =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../resources/runtimes");
            if dev_runtimes.is_dir() {
                state.local_server.add_runtimes_root(dev_runtimes);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_default_save_dir,
            pick_output_folder,
            pick_project_folder,
            resolve_clone_output_path,
            scan_local_project,
            start_local_server,
            stop_local_server,
            get_local_server_status,
            get_download_status,
            download_site,
            open_folder,
            open_url
        ])
        .build(tauri::generate_context!())
        .expect("error while building webcloner GUI")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                if let Some(state) = app_handle.try_state::<AppState>() {
                    let server = state.local_server.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = server.stop_async().await;
                    });
                }
            }
        });
}
