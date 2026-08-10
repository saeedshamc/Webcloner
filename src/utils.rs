use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use url::Url;

/// Turn a URL into a safe, deterministic relative file path under the output directory.
///
/// Rules:
/// - Same-host URLs are mirrored under their path (e.g. /css/app.css -> css/app.css).
/// - Cross-host URLs (CDNs, Google Fonts, ...) go under `_external/<host>/<path>`.
/// - Paths ending in `/` (or empty) become `index.html`.
/// - Query strings are folded into the filename via a short hash so that
///   `style.css?v=2` and `style.css?v=3` don't collide or get silently overwritten.
pub fn url_to_local_path(url: &Url, site_host: &str) -> PathBuf {
    let host = url.host_str().unwrap_or("unknown-host");
    let mut path = url.path().to_string();

    if path.is_empty() || path.ends_with('/') {
        path.push_str("index.html");
    }

    // Strip leading slash so we can join it as a relative path.
    let path = path.trim_start_matches('/');
    let mut rel = PathBuf::new();

    if host != site_host {
        rel.push("_external");
        rel.push(sanitize_component(host));
    }

    for segment in path.split('/') {
        if segment.is_empty() {
            continue;
        }
        rel.push(sanitize_component(segment));
    }

    if rel.as_os_str().is_empty() {
        rel.push("index.html");
    }

    // If the URL has a query string, disambiguate the filename with a short hash
    // of the full URL so distinct query variants don't collide on disk.
    if url.query().is_some() {
        let hash = short_hash(url.as_str());
        let file_name = rel
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "index.html".to_string());
        let (stem, ext) = split_ext(&file_name);
        let new_name = match ext {
            Some(ext) => format!("{stem}.{hash}.{ext}"),
            None => format!("{stem}.{hash}"),
        };
        rel.set_file_name(new_name);
    }

    rel
}

fn split_ext(file_name: &str) -> (&str, Option<&str>) {
    match file_name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (file_name, None),
    }
}

fn short_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    hex_prefix(&result, 8)
}

fn hex_prefix(bytes: &[u8], n: usize) -> String {
    bytes.iter().take(n).map(|b| format!("{b:02x}")).collect()
}

/// Replace filesystem-unfriendly characters in a single path segment.
fn sanitize_component(s: &str) -> String {
    let decoded = percent_decode(s);
    decoded
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '\\' | '|' | '?' | '*' => '_',
            c => c,
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .to_string()
}

/// Compute a relative path `from` -> `to`, both given as paths relative to the same root.
/// Used to rewrite `href`/`src`/`url()` references so the site works from `file://` too.
pub fn relative_path(from_file: &Path, to_file: &Path) -> String {
    let from_dir = from_file.parent().unwrap_or_else(|| Path::new(""));
    let from_components: Vec<_> = from_dir
        .components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect();
    let to_components: Vec<_> = to_file
        .components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect();

    let mut common = 0;
    while common < from_components.len()
        && common < to_components.len()
        && from_components[common] == to_components[common]
    {
        common += 1;
    }

    let mut result = PathBuf::new();
    for _ in common..from_components.len() {
        result.push("..");
    }
    for comp in &to_components[common..] {
        result.push(comp.as_os_str());
    }

    let s = result.to_string_lossy().replace('\\', "/");
    if s.is_empty() {
        ".".to_string()
    } else {
        s
    }
}

pub fn is_probably_html(content_type: Option<&str>, bytes: &[u8]) -> bool {
    if let Some(ct) = content_type {
        if ct.contains("text/html") || ct.contains("application/xhtml") {
            return true;
        }
        if ct.starts_with("text/") == false && !ct.contains("html") {
            return false;
        }
    }
    let head = &bytes[..bytes.len().min(512)];
    let text = String::from_utf8_lossy(head).to_lowercase();
    text.contains("<html") || text.contains("<!doctype html")
}

pub fn is_probably_css(content_type: Option<&str>, path: &str) -> bool {
    if let Some(ct) = content_type {
        if ct.contains("text/css") {
            return true;
        }
    }
    path.ends_with(".css")
}
