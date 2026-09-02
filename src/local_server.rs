use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tokio::sync::oneshot;
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
}

enum RunningServer {
    Static {
        shutdown_tx: oneshot::Sender<()>,
        thread: JoinHandle<()>,
    },
    External {
        child: Child,
    },
}

pub struct LocalServer {
    inner: Mutex<Option<RunningServer>>,
    active_url: Mutex<Option<String>>,
    active_dir: Mutex<Option<PathBuf>>,
    active_backend: Mutex<Option<ServerBackend>>,
}

impl LocalServer {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            active_url: Mutex::new(None),
            active_dir: Mutex::new(None),
            active_backend: Mutex::new(None),
        }
    }

    pub fn scan_project(dir: &Path) -> Result<ProjectScan> {
        validate_dir(dir)?;

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
            match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase) {
                Some(ext) if ext == "html" || ext == "htm" => has_html = true,
                Some(ext) if ext == "php" => has_php = true,
                Some(ext) if ext == "asp" => has_asp = true,
                Some(ext) if ext == "aspx" => has_aspx = true,
                Some(ext) if ext == "csproj" => has_csproj = true,
                _ => {}
            }
        }

        let php_available = command_exists("php");
        let dotnet_available = command_exists("dotnet");

        let backends = vec![
            DetectedBackend {
                backend: ServerBackend::Static,
                available: true,
                label: "استاتیک (HTML/CSS/JS)".to_string(),
                note: "برای سایت‌های کلون‌شده و فایل‌های خام front-end".to_string(),
            },
            DetectedBackend {
                backend: ServerBackend::Php,
                available: php_available,
                label: "PHP".to_string(),
                note: if php_available {
                    "از php -S داخلی PHP استفاده می‌کند".to_string()
                } else {
                    "PHP روی سیستم نصب نیست (php در PATH)".to_string()
                },
            },
            DetectedBackend {
                backend: ServerBackend::AspNet,
                available: dotnet_available && has_csproj,
                label: "ASP.NET".to_string(),
                note: if !dotnet_available {
                    ".NET SDK روی سیستم نصب نیست (dotnet در PATH)".to_string()
                } else if !has_csproj {
                    "فایل .csproj در پروژه پیدا نشد — برای ASP.NET Core".to_string()
                } else if has_asp && !has_csproj {
                    "ASP کلاسیک نیاز به IIS دارد؛ فقط ASP.NET Core پشتیبانی می‌شود".to_string()
                } else {
                    "پروژه را با dotnet run اجرا می‌کند".to_string()
                },
            },
        ];

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

    pub fn start(&self, dir: PathBuf, port: u16, backend: ServerBackend) -> Result<String> {
        self.stop()?;
        validate_dir(&dir)?;

        let url = format!("http://127.0.0.1:{port}");
        let running = match backend {
            ServerBackend::Static => start_static_server(dir.clone(), port)?,
            ServerBackend::Php => {
                if !command_exists("php") {
                    bail!("PHP روی سیستم نصب نیست.");
                }
                start_external_server(
                    Command::new("php")
                        .arg("-S")
                        .arg(format!("127.0.0.1:{port}"))
                        .arg("-t")
                        .arg(&dir)
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .context("failed to start PHP built-in server")?,
                )?
            }
            ServerBackend::AspNet => {
                if !command_exists("dotnet") {
                    bail!(".NET SDK روی سیستم نصب نیست.");
                }
                let csproj = find_csproj(&dir).context("فایل .csproj در پروژه پیدا نشد.")?;
                let project_dir = csproj
                    .parent()
                    .context("مسیر پروژه نامعتبر است.")?
                    .to_path_buf();
                start_external_server(
                    Command::new("dotnet")
                        .arg("run")
                        .arg("--urls")
                        .arg(format!("http://127.0.0.1:{port}"))
                        .current_dir(&project_dir)
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .context("failed to start ASP.NET project")?,
                )?
            }
        };

        *self.inner.lock().unwrap() = Some(running);
        *self.active_url.lock().unwrap() = Some(url.clone());
        *self.active_dir.lock().unwrap() = Some(dir);
        *self.active_backend.lock().unwrap() = Some(backend);

        Ok(url)
    }

    pub fn stop(&self) -> Result<()> {
        let mut guard = self.inner.lock().unwrap();
        if let Some(server) = guard.take() {
            match server {
                RunningServer::Static {
                    shutdown_tx,
                    thread,
                } => {
                    let _ = shutdown_tx.send(());
                    let _ = thread.join();
                }
                RunningServer::External { mut child } => {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }

        *self.active_url.lock().unwrap() = None;
        *self.active_dir.lock().unwrap() = None;
        *self.active_backend.lock().unwrap() = None;
        Ok(())
    }

    pub fn status(&self) -> ServerStatus {
        let running = self.inner.lock().unwrap().is_some();
        ServerStatus {
            running,
            url: self.active_url.lock().unwrap().clone(),
            project_dir: self
                .active_dir
                .lock()
                .unwrap()
                .as_ref()
                .map(|p| p.display().to_string()),
            backend: *self.active_backend.lock().unwrap(),
        }
    }
}

impl Default for LocalServer {
    fn default() -> Self {
        Self::new()
    }
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

fn start_static_server(dir: PathBuf, port: u16) -> Result<RunningServer> {
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let thread = thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(_) => return,
        };

        rt.block_on(async move {
            let serve_dir = ServeDir::new(&dir).append_index_html_on_directories(true);
            let app = axum::Router::new().nest_service("/", serve_dir);
            let server = axum::Server::bind(&addr).serve(app.into_make_service());
            let graceful = server.with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            });
            let _ = graceful.await;
        });
    });

    // Give the server a moment to bind before returning the URL.
    thread::sleep(Duration::from_millis(150));

    Ok(RunningServer::Static {
        shutdown_tx,
        thread,
    })
}

fn start_external_server(child: Child) -> Result<RunningServer> {
    Ok(RunningServer::External { child })
}

fn command_exists(name: &str) -> bool {
    #[cfg(windows)]
    {
        Command::new("where")
            .arg(name)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        Command::new("which")
            .arg(name)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
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
