use regex::Regex;
use scraper::{Html, Selector};
use std::sync::OnceLock;
use url::Url;

/// One reference to an external resource found inside an HTML or CSS document,
/// expressed as the *exact literal text* that appeared in the source (so we can
/// later find-and-replace it verbatim) plus the URL it resolves to.
#[derive(Debug, Clone)]
pub struct Reference {
    pub literal: String,
    pub resolved: Url,
}

fn selector(css: &str) -> Selector {
    Selector::parse(css).expect("valid selector")
}

/// Extract every asset reference (css, js, images, fonts, media, favicons, srcset entries,
/// inline style url()s and inline <style> blocks) from an HTML document.
pub fn extract_html_asset_refs(html: &str, base: &Url) -> Vec<Reference> {
    let doc = Html::parse_document(html);
    let mut refs = Vec::new();

    let tag_attrs: &[(&str, &str)] = &[
        ("link[href]", "href"),
        ("script[src]", "src"),
        ("img[src]", "src"),
        ("source[src]", "src"),
        ("video[src]", "src"),
        ("video[poster]", "poster"),
        ("audio[src]", "src"),
        ("embed[src]", "src"),
        ("object[data]", "data"),
        ("iframe[src]", "src"),
    ];

    for (sel, attr) in tag_attrs {
        for el in doc.select(&selector(sel)) {
            if let Some(val) = el.value().attr(attr) {
                push_ref(&mut refs, val, base);
            }
        }
    }

    // srcset: comma-separated "url descriptor" pairs
    for sel in &["img[srcset]", "source[srcset]"] {
        for el in doc.select(&selector(sel)) {
            if let Some(val) = el.value().attr("srcset") {
                for candidate in parse_srcset(val) {
                    push_ref(&mut refs, &candidate, base);
                }
            }
        }
    }

    // inline style="...url(...)..."
    for el in doc.select(&selector("[style]")) {
        if let Some(val) = el.value().attr("style") {
            for lit in extract_css_url_literals(val) {
                push_ref(&mut refs, &lit, base);
            }
        }
    }

    // <style>...</style> blocks (may contain url()/@import)
    for el in doc.select(&selector("style")) {
        let css_text = el.text().collect::<String>();
        for lit in extract_css_url_literals(&css_text) {
            push_ref(&mut refs, &lit, base);
        }
    }

    refs
}

/// Extract <a href> page links (used for crawling AND for rewriting internal
/// navigation once cloned pages are known). Keeps the exact literal text of
/// each href so it can be found-and-replaced verbatim later.
pub fn extract_page_links(html: &str, base: &Url) -> Vec<Reference> {
    let doc = Html::parse_document(html);
    let mut out = Vec::new();
    for el in doc.select(&selector("a[href]")) {
        if let Some(href) = el.value().attr("href") {
            let trimmed = href.trim();
            if trimmed.is_empty()
                || trimmed.starts_with('#')
                || trimmed.starts_with("mailto:")
                || trimmed.starts_with("tel:")
                || trimmed.starts_with("javascript:")
            {
                continue;
            }
            if let Ok(mut resolved) = base.join(trimmed) {
                resolved.set_fragment(None);
                out.push(Reference {
                    literal: href.to_string(),
                    resolved,
                });
            }
        }
    }
    out
}

/// Extract url(...) and @import references from a CSS document.
pub fn extract_css_asset_refs(css: &str, base: &Url) -> Vec<Reference> {
    let mut refs = Vec::new();
    for lit in extract_css_url_literals(css) {
        push_ref(&mut refs, &lit, base);
    }
    refs
}

fn push_ref(refs: &mut Vec<Reference>, literal: &str, base: &Url) {
    let trimmed = literal.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("data:")
        || trimmed.starts_with('#')
        || trimmed.starts_with("javascript:")
    {
        return;
    }
    if let Ok(resolved) = base.join(trimmed) {
        refs.push(Reference {
            literal: literal.to_string(),
            resolved,
        });
    }
}

fn parse_srcset(val: &str) -> Vec<String> {
    val.split(',')
        .filter_map(|part| part.trim().split_whitespace().next())
        .map(|s| s.to_string())
        .collect()
}

fn css_url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"url\(\s*['"]?([^'")]+)['"]?\s*\)"#).expect("valid regex")
    })
}

fn css_import_quoted_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"@import\s+['"]([^'"]+)['"]"#).expect("valid regex"))
}

fn css_import_url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"@import\s+url\(\s*['"]?([^'")]+)['"]?\s*\)"#).expect("valid regex")
    })
}

fn extract_css_url_literals(css: &str) -> Vec<String> {
    let mut out = Vec::new();
    for cap in css_url_regex().captures_iter(css) {
        out.push(cap[1].to_string());
    }
    for cap in css_import_quoted_regex().captures_iter(css) {
        out.push(cap[1].to_string());
    }
    for cap in css_import_url_regex().captures_iter(css) {
        out.push(cap[1].to_string());
    }
    out
}

/// Rewrite every literal reference in `content` (HTML or CSS text) to the
/// relative path resolved by `resolve`. `resolve` maps an absolute URL to a
/// relative path string (already computed by the caller) or `None` if the
/// resource wasn't downloaded (in which case the original literal is left as-is).
pub fn rewrite_references<F>(content: &str, refs: &[Reference], mut resolve: F) -> String
where
    F: FnMut(&Url) -> Option<String>,
{
    let mut out = content.to_string();
    // Replace longer literals first to avoid partial-match issues when one
    // literal is a substring of another.
    let mut sorted: Vec<&Reference> = refs.iter().collect();
    sorted.sort_by_key(|r| std::cmp::Reverse(r.literal.len()));

    for r in sorted {
        let Some(new_path) = resolve(&r.resolved) else {
            continue;
        };
        out = replace_one_reference(&out, &r.literal, &new_path);
    }
    out
}

/// Replace a single literal reference, but only in the contexts where it can
/// safely be identified: a quoted attribute value (`"lit"` / `'lit'`) or inside
/// a CSS `url(...)` (quoted or not). This avoids a blind substring replace that
/// could corrupt unrelated text when the literal is short/common (e.g. `/`).
fn replace_one_reference(content: &str, literal: &str, new_path: &str) -> String {
    let mut out = content.to_string();
    let mut hit = false;

    for (needle, replacement) in [
        (format!("\"{literal}\""), format!("\"{new_path}\"")),
        (format!("'{literal}'"), format!("'{new_path}'")),
        (format!("url({literal})"), format!("url({new_path})")),
        (
            format!("url(\"{literal}\")"),
            format!("url(\"{new_path}\")"),
        ),
        (
            format!("url('{literal}')"),
            format!("url('{new_path}')"),
        ),
    ] {
        if out.contains(&needle) {
            out = out.replace(&needle, &replacement);
            hit = true;
        }
    }

    // Fallback for unquoted contexts (e.g. srcset entries) — only for literals
    // long/specific enough that a blind replace is safe.
    if !hit && literal.len() > 3 {
        out = out.replace(literal, new_path);
    }

    out
}
