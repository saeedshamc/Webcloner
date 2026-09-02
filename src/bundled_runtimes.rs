use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Search paths for bundled PHP / .NET runtimes (inside the app bundle).
#[derive(Debug, Clone, Default)]
pub struct RuntimesLocator {
    search_roots: Vec<PathBuf>,
}

impl RuntimesLocator {
    pub fn new() -> Self {
        Self {
            search_roots: Vec::new(),
        }
    }

    pub fn add_search_root(&mut self, path: PathBuf) {
        if !self.search_roots.iter().any(|p| p == &path) {
            self.search_roots.push(path);
        }
    }

    pub fn php_available(&self) -> bool {
        self.resolve_php().is_some()
    }

    pub fn dotnet_available(&self) -> bool {
        self.resolve_dotnet().is_some()
    }

    pub fn resolve_php(&self) -> Option<PathBuf> {
        for root in &self.search_roots {
            #[cfg(windows)]
            let candidates = [
                root.join("php").join("php.exe"),
                root.join("php-win").join("php.exe"),
            ];
            #[cfg(not(windows))]
            let candidates = [root.join("php").join("php"), root.join("php").join("bin").join("php")];

            for candidate in candidates {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }

        if command_on_path("php") {
            return Some(PathBuf::from("php"));
        }
        None
    }

    pub fn resolve_dotnet(&self) -> Option<PathBuf> {
        for root in &self.search_roots {
            #[cfg(windows)]
            let candidates = [
                root.join("dotnet").join("dotnet.exe"),
                root.join("dotnet-win").join("dotnet.exe"),
            ];
            #[cfg(not(windows))]
            let candidates = [root.join("dotnet").join("dotnet")];

            for candidate in candidates {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }

        if command_on_path("dotnet") {
            return Some(PathBuf::from("dotnet"));
        }
        None
    }

    pub fn bundled_php_note(&self) -> String {
        if self.uses_bundled_php() {
            "PHP داخلی برنامه (بدون نیاز به PATH)".to_string()
        } else if self.php_available() {
            "PHP از PATH سیستم".to_string()
        } else {
            "PHP داخلی پیدا نشد — یک‌بار scripts/setup-runtimes.ps1 را اجرا کنید".to_string()
        }
    }

    pub fn bundled_dotnet_note(&self) -> String {
        if self.uses_bundled_dotnet() {
            "ASP.NET با .NET داخلی برنامه".to_string()
        } else if self.dotnet_available() {
            ".NET از PATH سیستم".to_string()
        } else {
            ".NET داخلی پیدا نشد — scripts/setup-runtimes.ps1 را اجرا کنید".to_string()
        }
    }

    fn uses_bundled_php(&self) -> bool {
        self.resolve_php()
            .map(|p| !is_bare_command_name(&p))
            .unwrap_or(false)
    }

    fn uses_bundled_dotnet(&self) -> bool {
        self.resolve_dotnet()
            .map(|p| !is_bare_command_name(&p))
            .unwrap_or(false)
    }
}

fn is_bare_command_name(path: &Path) -> bool {
    path.components().count() <= 1
}

fn command_on_path(name: &str) -> bool {
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
