use crate::bundled_runtimes::RuntimesLocator;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tower_http::services::ServeDir;
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServerBackend {
    Static,
    Php,
    AspNet,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedBackend {
    pub backend: ServerBackend,
    pub available: bool,
    pub label: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectScan {
    pub has_html: bool,
    pub has_php: bool,
    pub has_asp: bool,
    pub has_aspx: bool,
    pub has_csproj: bool,
    pub recommended: ServerBackend,
    pub backends: Vec<DetectedBackend>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub running: bool,
    pub url: Option<String>,
    pub project_dir: Option<String>,
    pub backend: Option<ServerBackend>,
    pub port: Option<u16>,
    pub busy: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub php_available: bool,
    pub php_path: Option<String>,
    pub php_bundled: bool,
    pub php_note: String,
    pub dotnet_available: bool,
    pub dotnet_path: Option<String>,
    pub dotnet_bundled: bool,
    pub dotnet_note: String,
}

enum RunningServer {
    Static {
        shutdown_tx: oneshot::Sender<()>,
        handle: JoinHandle<()>,
    },
    External {
        child: Child,
    },
}

#[derive(Default)]
struct ServerMeta {
    url: Option<String>,
    dir: Option<PathBuf>,
    backend: Option<ServerBackend>,
    port: Option<u16>,
}

struct ServerInner {
    running: Option<RunningServer>,
    meta: ServerMeta,
    busy: bool,
}

pub struct LocalServer {
    inner: AsyncMutex<ServerInner>,
    runtimes: Arc<RwLock<RuntimesLocator>>,
}

impl LocalServer {
    pub fn new() -> Self {
        Self {
            inner: AsyncMutex::new(ServerInner {
                running: None,
                meta: ServerMeta::default(),
                busy: false,
            }),
            runtimes: Arc::new(RwLock::new(RuntimesLocator::new())),
        }
    }

    pub fn add_runtimes_root(&self, path: PathBuf) {
        if let Ok(mut guard) = self.runtimes.write() {
            guard.add_search_root(path);
        }
    }

    pub async fn scan_project_async(&self, dir: PathBuf) -> Result<ProjectScan> {
        validate_dir(&dir)?;
        let runtimes = self
            .runtimes
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();
        tokio::task::spawn_blocking(move || build_project_scan(&dir, &runtimes))
            .await
            .context("project scan task failed")?
    }

    pub async fn start_async(
        &self,
        dir: PathBuf,
        port: u16,
        backend: ServerBackend,
        auto_port: bool,
    ) -> Result<(String, u16)> {
        {
            let mut guard = self.inner.lock().await;
            if guard.busy {
                bail!("عملیات سرور در حال انجام است — لطفاً چند ثانیه صبر کنید.");
            }
            guard.busy = true;
        }

        let result = self.start_async_inner(dir, port, backend, auto_port).await;

        {
            let mut guard = self.inner.lock().await;
            guard.busy = false;
        }

        result
    }

    async fn start_async_inner(
        &self,
        dir: PathBuf,
        port: u16,
        backend: ServerBackend,
        auto_port: bool,
    ) -> Result<(String, u16)> {
        self.stop_running().await?;
        validate_dir(&dir)?;

        let requested = port.clamp(1024, 65535);
        let port = if auto_port {
            crate::net_util::find_free_port(requested)?
        } else if !crate::net_util::port_is_free(requested) {
            let alt = crate::net_util::find_free_port(requested.saturating_add(1).max(1024))?;
            bail!(
                "پورت {requested} اشغال است. پورت آزاد پیشنهادی: {alt} — گزینه «پورت خودکار» را روشن کنید یا پورت را عوض کنید."
            );
        } else {
            requested
        };
        let url = format!("http://127.0.0.1:{port}");

        let running = match backend {
            ServerBackend::Static => start_static_server(dir.clone(), port).await?,
            ServerBackend::Php => {
                let php = self
                    .runtimes
                    .read()
                    .map_err(|_| anyhow::anyhow!("قفل runtime خراب شده است."))?
                    .resolve_php()
                    .context(
                        "PHP داخلی پیدا نشد. یک‌بار از ریشه پروژه scripts/setup-runtimes.ps1 را اجرا کنید.",
                    )?;
                let child = tokio::task::spawn_blocking({
                    let php = php.clone();
                    let dir = dir.clone();
                    move || spawn_php_server(&php, &dir, port)
                })
                .await
                .context("راه‌اندازی PHP ناموفق بود.")??;
                RunningServer::External { child }
            }
            ServerBackend::AspNet => {
                let dotnet = self
                    .runtimes
                    .read()
                    .map_err(|_| anyhow::anyhow!("قفل runtime خراب شده است."))?
                    .resolve_dotnet()
                    .context(
                        ".NET داخلی پیدا نشد. یک‌بار scripts/setup-runtimes.ps1 را اجرا کنید.",
                    )?;
                let csproj = find_csproj(&dir).context("فایل .csproj در پروژه پیدا نشد.")?;
                let project_dir = csproj
                    .parent()
                    .context("مسیر پروژه ASP.NET نامعتبر است.")?
                    .to_path_buf();
                let child = tokio::task::spawn_blocking({
                    let dotnet = dotnet.clone();
                    move || spawn_dotnet_server(&dotnet, &project_dir, port)
                })
                .await
                .context("راه‌اندازی ASP.NET ناموفق بود.")??;
                RunningServer::External { child }
            }
        };

        let mut guard = self.inner.lock().await;
        guard.running = Some(running);
        guard.meta.url = Some(url.clone());
        guard.meta.dir = Some(dir);
        guard.meta.backend = Some(backend);
        guard.meta.port = Some(port);

        Ok((url, port))
    }

    pub fn runtime_status(&self) -> RuntimeStatus {
        let runtimes = self.runtimes.read().ok();
        let (php_path, php_bundled, php_note) = match &runtimes {
            Some(r) => (
                r.resolve_php().map(|p| p.display().to_string()),
                r.resolve_php()
                    .map(|p| p.components().count() > 1)
                    .unwrap_or(false),
                r.bundled_php_note(),
            ),
            None => (None, false, "وضعیت PHP نامشخص".into()),
        };
        let (dotnet_path, dotnet_bundled, dotnet_note) = match &runtimes {
            Some(r) => (
                r.resolve_dotnet().map(|p| p.display().to_string()),
                r.resolve_dotnet()
                    .map(|p| p.components().count() > 1)
                    .unwrap_or(false),
                r.bundled_dotnet_note(),
            ),
            None => (None, false, "وضعیت .NET نامشخص".into()),
        };
        RuntimeStatus {
            php_available: php_path.is_some(),
            php_path,
            php_bundled,
            php_note,
            dotnet_available: dotnet_path.is_some(),
            dotnet_path,
            dotnet_bundled,
            dotnet_note,
        }
    }

    pub async fn stop_async(&self) -> Result<()> {
        {
            let mut guard = self.inner.lock().await;
            if guard.busy {
                bail!("عملیات سرور در حال انجام است — لطفاً چند ثانیه صبر کنید.");
            }
            guard.busy = true;
        }

        let result = self.stop_running().await;

        {
            let mut guard = self.inner.lock().await;
            guard.busy = false;
        }

        result
    }

    async fn stop_running(&self) -> Result<()> {
        let server = {
            let mut guard = self.inner.lock().await;
            guard.running.take()
        };

        if let Some(server) = server {
            shutdown_server(server).await;
        }

        let mut guard = self.inner.lock().await;
        guard.meta = ServerMeta::default();
        Ok(())
    }

    pub async fn status_async(&self) -> ServerStatus {
        let guard = self.inner.lock().await;
        ServerStatus {
            running: guard.running.is_some(),
            url: guard.meta.url.clone(),
            project_dir: guard.meta.dir.as_ref().map(|p| p.display().to_string()),
            backend: guard.meta.backend,
            port: guard.meta.port,
            busy: guard.busy,
        }
    }
}

impl Default for LocalServer {
    fn default() -> Self {
        Self::new()
    }
}

async fn shutdown_server(server: RunningServer) {
    match server {
        RunningServer::Static {
            shutdown_tx,
            handle,
        } => {
            let _ = shutdown_tx.send(());
            let _ = tokio::time::timeout(Duration::from_secs(3), handle).await;
        }
        RunningServer::External { mut child } => {
            let _ = tokio::task::spawn_blocking(move || kill_child_process(&mut child)).await;
        }
    }
}

fn build_project_scan(dir: &Path, runtimes: &RuntimesLocator) -> Result<ProjectScan> {
    let mut has_html = false;
    let mut has_php = false;
    let mut has_asp = false;
    let mut has_aspx = false;
    let mut has_csproj = false;

    for entry in WalkDir::new(dir)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) == Some("index.html") {
            has_html = true;
        }
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
        {
            Some(ext) if ext == "html" || ext == "htm" => has_html = true,
            Some(ext) if ext == "php" => has_php = true,
            Some(ext) if ext == "asp" => has_asp = true,
            Some(ext) if ext == "aspx" => has_aspx = true,
            Some(ext) if ext == "csproj" => has_csproj = true,
            _ => {}
        }
    }

    let php_available = runtimes.php_available();
    let dotnet_available = runtimes.dotnet_available();

    let asp_note = if has_asp && !has_csproj {
        "ASP کلاسیک (.asp) فقط به IIS نیاز دارد — از حالت استاتیک یا PHP استفاده کنید".to_string()
    } else {
        String::new()
    };

    let mut backends = vec![
        DetectedBackend {
            backend: ServerBackend::Static,
            available: true,
            label: "استاتیک (HTML/CSS/JS)".to_string(),
            note: "برای سایت‌های کلون‌شده و فایل‌های front-end".to_string(),
        },
        DetectedBackend {
            backend: ServerBackend::Php,
            available: php_available,
            label: "PHP (داخلی)".to_string(),
            note: runtimes.bundled_php_note(),
        },
        DetectedBackend {
            backend: ServerBackend::AspNet,
            available: dotnet_available && has_csproj,
            label: "ASP.NET (داخلی)".to_string(),
            note: if asp_note.is_empty() {
                runtimes.bundled_dotnet_note()
            } else {
                format!("{}. {}", runtimes.bundled_dotnet_note(), asp_note)
            },
        },
    ];

    if has_asp && !has_csproj {
        backends.push(DetectedBackend {
            backend: ServerBackend::Static,
            available: true,
            label: "ASP کلاسیک (فقط مشاهده فایل)".to_string(),
            note: "فایل .asp اجرا نمی‌شود؛ فقط محتوای خام سرو می‌شود".to_string(),
        });
    }

    let recommended = if has_csproj && dotnet_available {
        ServerBackend::AspNet
    } else if has_php && php_available {
        ServerBackend::Php
    } else {
        ServerBackend::Static
    };

    Ok(ProjectScan {
        has_html,
        has_php,
        has_asp,
        has_aspx,
        has_csproj,
        recommended,
        backends,
    })
}

fn validate_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        bail!("پوشه وجود ندارد: {}", dir.display());
    }
    if !dir.is_dir() {
        bail!("مسیر انتخاب‌شده پوشه نیست: {}", dir.display());
    }
    Ok(())
}

async fn start_static_server(dir: PathBuf, port: u16) -> Result<RunningServer> {
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let listener = crate::net_util::bind_or_explain(port)?;
    listener.set_nonblocking(true)?;

    let handle = tokio::spawn(async move {
        let serve_dir = ServeDir::new(&dir).append_index_html_on_directories(true);
        let app = axum::Router::new().nest_service("/", serve_dir);
        match axum::Server::from_tcp(listener) {
            Ok(server) => {
                let graceful = server
                    .serve(app.into_make_service())
                    .with_graceful_shutdown(async {
                        let _ = shutdown_rx.await;
                    });
                let _ = graceful.await;
            }
            Err(_) => {}
        }
    });

    tokio::time::sleep(Duration::from_millis(80)).await;
    if handle.is_finished() {
        bail!("سرور استاتیک روی پورت {port} بالا نیامد.");
    }

    Ok(RunningServer::Static {
        shutdown_tx,
        handle,
    })
}

fn spawn_php_server(php: &Path, dir: &Path, port: u16) -> Result<Child> {
    Command::new(php)
        .arg("-S")
        .arg(format!("127.0.0.1:{port}"))
        .arg("-t")
        .arg(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("اجرای PHP با {} ناموفق بود.", php.display()))
}

fn spawn_dotnet_server(dotnet: &Path, project_dir: &Path, port: u16) -> Result<Child> {
    Command::new(dotnet)
        .arg("run")
        .arg("--urls")
        .arg(format!("http://127.0.0.1:{port}"))
        .current_dir(project_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("اجرای ASP.NET با {} ناموفق بود.", dotnet.display()))
}

fn kill_child_process(child: &mut Child) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn find_csproj(dir: &Path) -> Option<PathBuf> {
    WalkDir::new(dir)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
        .find(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("csproj"))
                    .unwrap_or(false)
        })
        .map(|e| e.path().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn scans_html_project() {
        let dir = std::env::temp_dir().join(format!("wc-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("index.html"), b"<html></html>").unwrap();
        let server = LocalServer::new();
        let scan = server.scan_project_async(dir.clone()).await.unwrap();
        assert!(scan.has_html);
        assert!(matches!(scan.recommended, ServerBackend::Static));
        let _ = fs::remove_dir_all(&dir);
    }
}
