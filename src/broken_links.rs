use anyhow::Result;
use regex::Regex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use walkdir::WalkDir;

/// Scan HTML/CSS under `root` for local relative refs that do not exist on disk.
pub fn scan_broken_links(root: &Path) -> Result<Vec<String>> {
    let mut broken = Vec::new();
    let mut seen = HashSet::new();

    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(ext.as_str(), "html" | "htm" | "css") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let base_dir = path.parent().unwrap_or(root);
        for raw in extract_local_refs(&text, ext == "css") {
            if raw.starts_with("http://")
                || raw.starts_with("https://")
                || raw.starts_with("data:")
                || raw.starts_with("mailto:")
                || raw.starts_with("javascript:")
                || raw.starts_with('#')
            {
                continue;
            }
            let cleaned = raw.split(['?', '#']).next().unwrap_or(&raw);
            if cleaned.is_empty() || cleaned == "/" {
                continue;
            }
            let target = resolve_local(base_dir, root, cleaned);
            if !target.exists() {
                let key = format!("{} -> {}", path.strip_prefix(root).unwrap_or(path).display(), cleaned);
                if seen.insert(key.clone()) {
                    broken.push(key);
                }
            }
        }
    }

    Ok(broken)
}

fn resolve_local(base_dir: &Path, root: &Path, rel: &str) -> PathBuf {
    let trimmed = rel.trim_start_matches('/');
    if rel.starts_with('/') {
        root.join(trimmed)
    } else {
        base_dir.join(trimmed)
    }
}

fn extract_local_refs(text: &str, css: bool) -> Vec<String> {
    let mut out = Vec::new();
    if css {
        for cap in css_url_re().captures_iter(text) {
            if let Some(m) = cap.get(1) {
                out.push(m.as_str().to_string());
            }
        }
    } else {
        for cap in html_attr_re().captures_iter(text) {
            if let Some(m) = cap.get(1) {
                out.push(m.as_str().to_string());
            }
        }
        for cap in css_url_re().captures_iter(text) {
            if let Some(m) = cap.get(1) {
                out.push(m.as_str().to_string());
            }
        }
    }
    out
}

fn html_attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)(?:href|src|data-src|poster)\s*=\s*["']([^"']+)["']"#).expect("regex")
    })
}

fn css_url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"url\(\s*['"]?([^'")]+)['"]?\s*\)"#).expect("regex"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detects_missing_local_href() {
        let dir = std::env::temp_dir().join(format!("wc-broken-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("index.html"),
            r#"<a href="missing.css">x</a><img src="ok.png">"#,
        )
        .unwrap();
        fs::write(dir.join("ok.png"), b"x").unwrap();
        let broken = scan_broken_links(&dir).unwrap();
        assert!(broken.iter().any(|b| b.contains("missing.css")));
        assert!(!broken.iter().any(|b| b.contains("ok.png")));
        let _ = fs::remove_dir_all(&dir);
    }
}
