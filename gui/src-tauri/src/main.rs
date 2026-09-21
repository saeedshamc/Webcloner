#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::{Manager, WindowBuilder, WindowUrl};
use url::Url;
use webcloner::{
    downloader, job_state::JobState, local_server::LocalServer, local_server::ProjectScan,
    local_server::RuntimeStatus, local_server::ServerBackend, local_server::ServerStatus, net_util,
    zipper,
};

struct AppState {
    local_server: Arc<LocalServer>,
    download_active: Arc<AtomicBool>,
    download_cancel: Arc<AtomicBool>,
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
    block_tracking: bool,
    report_broken_links: bool,
    /// Confirmed page URLs from discovery (optional).
    planned_pages: Option<Vec<String>>,
    /// Resume interrupted job in output folder.
    resume: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiscoverOptionsIn {
    url: String,
    max_pages_cap: usize,
    max_depth: usize,
    follow_external_pages: bool,
    block_tracking: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResumeInfo {
    available: bool,
    out_dir: Option<String>,
    start_url: Option<String>,
    planned_pages: usize,
    done_pages: usize,
    done_assets: usize,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadResult {
    out_dir: String,
    message: String,
    cancelled: bool,
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
    auto_port: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartServerResult {
    url: String,
    port: u16,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    save_dir: Option<String>,
    out_name: Option<String>,
    max_pages: Option<usize>,
    max_depth: Option<usize>,
    concurrency: Option<usize>,
    include_external_assets: Option<bool>,
    follow_external_pages: Option<bool>,
    zip: Option<bool>,
    block_tracking: Option<bool>,
    report_broken_links: Option<bool>,
    port: Option<u16>,
    auto_port: Option<bool>,
    recent: Vec<RecentItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecentItem {
    path: String,
    kind: String,
    url: Option<String>,
    port: Option<u16>,
    at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopPackageResult {
    output_dir: String,
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

fn app_data_dir() -> PathBuf {
    if let Ok(profile) = std::env::var("APPDATA") {
        return PathBuf::from(profile).join("webcloner");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".webcloner");
    }
    PathBuf::from(".webcloner")
}

fn settings_path() -> PathBuf {
    app_data_dir().join("settings.json")
}

fn load_settings() -> AppSettings {
    let path = settings_path();
    if let Ok(raw) = fs::read_to_string(path) {
        serde_json::from_str(&raw).unwrap_or_default()
    } else {
        AppSettings::default()
    }
}

fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let dir = app_data_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("ساخت پوشه تنظیمات ناموفق: {e}"))?;
    let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(settings_path(), raw).map_err(|e| format!("ذخیره تنظیمات ناموفق: {e}"))
}

fn push_recent(settings: &mut AppSettings, item: RecentItem) {
    settings.recent.retain(|r| r.path != item.path || r.kind != item.kind);
    settings.recent.insert(0, item);
    if settings.recent.len() > 12 {
        settings.recent.truncate(12);
    }
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    secs.to_string()
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else {
            fs::copy(&path, &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
fn get_default_save_dir() -> String {
    default_save_dir()
}

#[tauri::command]
fn load_app_settings() -> AppSettings {
    load_settings()
}

#[tauri::command]
fn save_app_settings(settings: AppSettings) -> Result<(), String> {
    save_settings(&settings)
}

#[tauri::command]
fn suggest_free_port(preferred: u16) -> Result<u16, String> {
    net_util::find_free_port(preferred.clamp(1024, 65535)).map_err(|e| e.to_string())
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
    .map_err(|e| format!("دیالوگ پوشه ناموفق: {e}"))?
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
    .map_err(|e| format!("دیالوگ پوشه ناموفق: {e}"))?
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
fn get_runtime_status(state: tauri::State<AppState>) -> RuntimeStatus {
    state.local_server.runtime_status()
}

#[tauri::command]
async fn start_local_server(
    options: StartServerOptions,
    state: tauri::State<'_, AppState>,
) -> Result<StartServerResult, String> {
    let dir = validate_project_dir(&options.project_dir)?;
    let port = options.port.clamp(1024, 65535);
    let (url, used_port) = state
        .local_server
        .start_async(dir.clone(), port, options.backend, options.auto_port)
        .await
        .map_err(|e| e.to_string())?;

    let backend_label = match options.backend {
        ServerBackend::Static => "استاتیک",
        ServerBackend::Php => "PHP",
        ServerBackend::AspNet => "ASP.NET",
    };

    let mut settings = load_settings();
    settings.port = Some(used_port);
    settings.auto_port = Some(options.auto_port);
    push_recent(
        &mut settings,
        RecentItem {
            path: dir.display().to_string(),
            kind: "server".into(),
            url: Some(url.clone()),
            port: Some(used_port),
            at: now_iso(),
        },
    );
    let _ = save_settings(&settings);

    Ok(StartServerResult {
        url: url.clone(),
        port: used_port,
        message: format!("سرور {backend_label} روی پورت {used_port} فعال شد:\n{url}"),
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
fn cancel_download(state: tauri::State<AppState>) -> Result<(), String> {
    if !state.download_active.load(Ordering::SeqCst) {
        return Err("دانلود فعالی وجود ندارد.".into());
    }
    state.download_cancel.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
async fn discover_site(
    options: DiscoverOptionsIn,
    window: tauri::Window,
    state: tauri::State<'_, AppState>,
) -> Result<downloader::DiscoverResult, String> {
    if state
        .download_active
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("یک عملیات دانلود/کشف دیگر در حال اجراست.".into());
    }
    state.download_cancel.store(false, Ordering::SeqCst);

    struct Reset {
        active: Arc<AtomicBool>,
        cancel: Arc<AtomicBool>,
    }
    impl Drop for Reset {
        fn drop(&mut self) {
            self.active.store(false, Ordering::SeqCst);
            self.cancel.store(false, Ordering::SeqCst);
        }
    }
    let _reset = Reset {
        active: state.download_active.clone(),
        cancel: state.download_cancel.clone(),
    };

    let start_url = normalize_start_url(&options.url)?;
    let window_for_progress = window.clone();
    let progress = Arc::new(move |line: String| {
        let _ = window_for_progress.emit("download-progress", line);
    });

    let discover_opts = downloader::DiscoverOptions {
        start_url,
        max_pages_cap: options.max_pages_cap.clamp(1, 5000),
        max_depth: options.max_depth.min(100),
        follow_external_pages: options.follow_external_pages,
        block_tracking: options.block_tracking,
        timeout_secs: 25,
        user_agent: "webcloner-gui/1.0 (+offline mirror tool)".to_string(),
        on_progress: Some(progress),
        cancel_flag: Some(state.download_cancel.clone()),
    };

    downloader::discover_async(discover_opts)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn check_resume_job(save_dir: String, out_name: String) -> Result<ResumeInfo, String> {
    let out_dir = resolve_output_dir(&save_dir, &out_name)?;
    match JobState::load(&out_dir).map_err(|e| e.to_string())? {
        Some(st) => Ok(ResumeInfo {
            available: true,
            out_dir: Some(out_dir.display().to_string()),
            start_url: Some(st.start_url),
            planned_pages: st.planned_pages.len(),
            done_pages: st.done_pages.len(),
            done_assets: st.done_assets.len(),
            message: format!(
                "دانلود ناتمام پیدا شد: {}/{} صفحه و {} فایل ذخیره‌شده.",
                st.done_pages.len(),
                st.planned_pages.len(),
                st.done_assets.len()
            ),
        }),
        None => Ok(ResumeInfo {
            available: false,
            out_dir: None,
            start_url: None,
            planned_pages: 0,
            done_pages: 0,
            done_assets: 0,
            message: "وضعیت ادامه‌ای وجود ندارد.".into(),
        }),
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

    state.download_cancel.store(false, Ordering::SeqCst);

    struct DownloadReset {
        active: Arc<AtomicBool>,
        cancel: Arc<AtomicBool>,
    }
    impl Drop for DownloadReset {
        fn drop(&mut self) {
            self.active.store(false, Ordering::SeqCst);
            self.cancel.store(false, Ordering::SeqCst);
        }
    }
    let _reset = DownloadReset {
        active: state.download_active.clone(),
        cancel: state.download_cancel.clone(),
    };

    let start_url = normalize_start_url(&options.url)?;
    let out_dir = resolve_output_dir(&options.save_dir, &options.out_name)?;

    let window_for_progress = window.clone();
    let progress = Arc::new(move |line: String| {
        let _ = window_for_progress.emit("download-progress", line);
    });
    let window_for_event = window.clone();
    let progress_event = Arc::new(move |ev: downloader::ProgressEvent| {
        let _ = window_for_event.emit("download-progress-event", ev);
    });

    let crawl_opts = downloader::CrawlOptions {
        start_url: start_url.clone(),
        out_dir: out_dir.clone(),
        max_pages: options.max_pages.max(1),
        max_depth: options.max_depth,
        include_external_assets: options.include_external_assets,
        follow_external_pages: options.follow_external_pages,
        concurrency: options.concurrency.max(1),
        timeout_secs: 30,
        user_agent: "webcloner-gui/1.0 (+offline mirror tool)".to_string(),
        block_tracking: options.block_tracking,
        report_broken_links: options.report_broken_links,
        planned_pages: options.planned_pages.clone(),
        resume: options.resume,
        on_progress: Some(progress),
        on_progress_event: Some(progress_event),
        cancel_flag: Some(state.download_cancel.clone()),
    };

    let crawl_result = downloader::run_async(crawl_opts).await;
    let cancelled = state.download_cancel.load(Ordering::SeqCst);

    if let Err(e) = crawl_result {
        let msg = e.to_string();
        if cancelled || msg.contains("لغو") {
            return Ok(DownloadResult {
                out_dir: out_dir.display().to_string(),
                message: format!(
                    "دانلود لغو شد. پوشه ناقص باقی ماند:\n{}",
                    out_dir.display()
                ),
                cancelled: true,
            });
        }
        return Err(msg);
    }

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

    let mut settings = load_settings();
    settings.save_dir = Some(options.save_dir);
    settings.out_name = Some(options.out_name);
    settings.max_pages = Some(options.max_pages);
    settings.max_depth = Some(options.max_depth);
    settings.concurrency = Some(options.concurrency);
    settings.include_external_assets = Some(options.include_external_assets);
    settings.follow_external_pages = Some(options.follow_external_pages);
    settings.zip = Some(options.zip);
    settings.block_tracking = Some(options.block_tracking);
    settings.report_broken_links = Some(options.report_broken_links);
    push_recent(
        &mut settings,
        RecentItem {
            path: out_dir.display().to_string(),
            kind: "clone".into(),
            url: Some(start_url),
            port: None,
            at: now_iso(),
        },
    );
    let _ = save_settings(&settings);

    Ok(DownloadResult {
        out_dir: out_dir.display().to_string(),
        message,
        cancelled: false,
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
    .map_err(|e| format!("باز کردن پوشه ناموفق: {e}"))?
}

#[tauri::command]
async fn open_url(url: String, window: tauri::Window) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        tauri::api::shell::open(&window.shell_scope(), url, None).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("باز کردن آدرس ناموفق: {e}"))?
}

#[tauri::command]
async fn open_preview_window(app: tauri::AppHandle, url: String) -> Result<(), String> {
    if let Some(existing) = app.get_window("preview") {
        let _ = existing.close();
    }
    WindowBuilder::new(&app, "preview", WindowUrl::External(url.parse().map_err(|e| format!("{e}"))?))
        .title("پیش‌نمایش webcloner")
        .inner_size(1000.0, 720.0)
        .build()
        .map_err(|e| format!("ساخت پنجره پیش‌نمایش ناموفق: {e}"))?;
    Ok(())
}

#[tauri::command]
async fn prepare_desktop_package(project_dir: String) -> Result<DesktopPackageResult, String> {
    let src = validate_project_dir(&project_dir)?;
    let template = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../desktop-app-template");
    if !template.is_dir() {
        return Err("قالب desktop-app-template پیدا نشد.".into());
    }

    let stamp = now_iso();
    let out = app_data_dir()
        .join("desktop-packages")
        .join(format!("site-{}", stamp));
    if out.exists() {
        fs::remove_dir_all(&out).map_err(|e| e.to_string())?;
    }
    copy_dir_recursive(&template, &out)?;

    let dist = out.join("dist");
    if dist.exists() {
        fs::remove_dir_all(&dist).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&dist).map_err(|e| e.to_string())?;
    copy_dir_recursive(&src, &dist)?;

    let readme = format!(
        "بسته دسکتاپ آماده شد.\n\n1) برای ساخت نصب‌کننده:\n   cd \"{}\"\n   .\\package.ps1 \"{}\"\n\nیا فقط از همین پوشه با cargo tauri build در src-tauri بسازید.\nمحتوای سایت در dist/ کپی شده است.\n",
        out.display(),
        src.display()
    );
    fs::write(out.join("README-NEXT.txt"), readme).map_err(|e| e.to_string())?;

    Ok(DesktopPackageResult {
        output_dir: out.display().to_string(),
        message: format!(
            "پوشه آماده دسکتاپ ساخته شد:\n{}\nسایت در dist/ کپی شد. راهنما: README-NEXT.txt",
            out.display()
        ),
    })
}

fn main() {
    tauri::Builder::default()
        .manage(AppState {
            local_server: Arc::new(LocalServer::new()),
            download_active: Arc::new(AtomicBool::new(false)),
            download_cancel: Arc::new(AtomicBool::new(false)),
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
            load_app_settings,
            save_app_settings,
            suggest_free_port,
            pick_output_folder,
            pick_project_folder,
            resolve_clone_output_path,
            scan_local_project,
            get_runtime_status,
            start_local_server,
            stop_local_server,
            get_local_server_status,
            get_download_status,
            cancel_download,
            discover_site,
            check_resume_job,
            download_site,
            open_folder,
            open_url,
            open_preview_window,
            prepare_desktop_package
        ])
        .build(tauri::generate_context!())
        .expect("error while building webcloner GUI")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                if let Some(state) = app_handle.try_state::<AppState>() {
                    state.download_cancel.store(true, Ordering::SeqCst);
                    let server = state.local_server.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = server.stop_async().await;
                    });
                }
            }
        });
}
