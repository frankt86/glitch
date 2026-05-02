//! Article-to-note extractor. Runs **inside Glitch** (not via Claude) so it
//! works regardless of the local Claude environment's network sandbox.
//!
//! Pipeline: reqwest → dom_smoothie::Readability → image embed (base64) →
//! htmd → markdown with frontmatter. Images are embedded as base64 data URLs
//! so notes remain readable even if the source page disappears.

use base64::Engine as _;
use camino::{Utf8Path, Utf8PathBuf};
use jiff::Timestamp;
use std::fmt;
use std::time::Duration;

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Glitch/0.1";

#[derive(Debug)]
pub enum ExtractError {
    InvalidUrl(String),
    Http(String),
    Readability(String),
    Conversion(String),
    Io(String),
}

impl fmt::Display for ExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(s) => write!(f, "invalid url: {s}"),
            Self::Http(s) => write!(f, "http error: {s}"),
            Self::Readability(s) => write!(f, "readability failed: {s}"),
            Self::Conversion(s) => write!(f, "html→markdown conversion failed: {s}"),
            Self::Io(s) => write!(f, "io: {s}"),
        }
    }
}

impl std::error::Error for ExtractError {}

impl From<reqwest::Error> for ExtractError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e.to_string())
    }
}
impl From<std::io::Error> for ExtractError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct ExtractedNote {
    #[allow(dead_code)]
    pub absolute_path: Utf8PathBuf,
    pub relative_path: Utf8PathBuf,
    pub title: String,
}

/// Fetch `url`, extract the readable article, embed images as base64 data URLs,
/// and write it as a markdown note under `<vault_root>/articles/<slug>.md`.
pub async fn extract_to_vault(
    url: &str,
    vault_root: &Utf8Path,
) -> Result<ExtractedNote, ExtractError> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(ExtractError::InvalidUrl(url.into()));
    }

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(30))
        .build()?;

    let resp = client.get(url).send().await?.error_for_status()?;
    let html = resp.text().await?;
    let html = promote_lazy_images(html);

    let mut readable = dom_smoothie::Readability::new(html.as_str(), Some(url), None)
        .map_err(|e| ExtractError::Readability(e.to_string()))?;
    let article = readable
        .parse()
        .map_err(|e| ExtractError::Readability(e.to_string()))?;

    let title_raw = article.title.to_string();
    let title = if title_raw.trim().is_empty() {
        url.to_string()
    } else {
        title_raw
    };

    let slug = slugify(&title);
    let articles_dir = vault_root.join("articles");
    tokio::fs::create_dir_all(articles_dir.as_std_path()).await?;

    // Download images and embed as base64 data URLs so the note is self-contained.
    let content_html = article.content.to_string();
    let content_html = embed_images_base64(content_html, &client).await;

    let body_md = htmd::convert(&content_html)
        .map_err(|e| ExtractError::Conversion(e.to_string()))?;

    // Resolve note path, avoiding collisions with a numeric suffix.
    let mut path = articles_dir.join(format!("{slug}.md"));
    let mut suffix = 1;
    while path.exists() {
        suffix += 1;
        path = articles_dir.join(format!("{slug}-{suffix}.md"));
    }

    let now = Timestamp::now().strftime("%Y-%m-%d").to_string();
    let excerpt = article
        .excerpt
        .as_ref()
        .map(|s| s.to_string().trim().to_string())
        .unwrap_or_default();
    let byline = article
        .byline
        .as_ref()
        .map(|s| s.to_string().trim().to_string())
        .unwrap_or_default();

    let frontmatter = build_frontmatter(&title, url, &now, &byline, &excerpt);
    let document = format!("{frontmatter}\n# {title}\n\n{body_md}\n");
    tokio::fs::write(path.as_std_path(), document).await?;

    let relative = path
        .strip_prefix(vault_root)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| path.clone());
    Ok(ExtractedNote {
        absolute_path: path,
        relative_path: relative,
        title,
    })
}

// ─── Image embedding ──────────────────────────────────────────────────────────

/// Download every image referenced in the HTML and replace `src` attributes
/// with `data:<mime>;base64,<data>` so the note is fully self-contained.
/// Images that fail to download keep their original remote URL.
async fn embed_images_base64(html: String, client: &reqwest::Client) -> String {
    let srcs = extract_img_srcs(&html);
    if srcs.is_empty() {
        return html;
    }

    let mut result = html;
    for src in srcs {
        if src.starts_with("data:") {
            continue; // already embedded
        }
        match fetch_as_data_url(client, &src).await {
            Ok(data_url) => {
                result = result.replace(src.as_str(), data_url.as_str());
            }
            Err(err) => {
                tracing::warn!("image download failed ({src}): {err}");
            }
        }
    }
    result
}

async fn fetch_as_data_url(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;

    let mime = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .split(';')
        .next()
        .unwrap_or("image/jpeg")
        .trim()
        .to_string();

    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
}

/// Pull all `src` attribute values from `<img>` tags in the HTML.
fn extract_img_srcs(html: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut rest = html;
    while let Some(pos) = rest.find("<img") {
        rest = &rest[pos + 4..];
        let tag_end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..tag_end];
        if let Some(src) = attr_value(tag, "src") {
            if !src.is_empty() {
                urls.push(src);
            }
        }
    }
    urls
}

/// Extract the value of `name="..."` or `name='...'` from an HTML attribute string.
fn attr_value(attrs: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let pattern = format!("{name}={quote}");
        if let Some(start) = attrs.find(&pattern) {
            let after = &attrs[start + pattern.len()..];
            if let Some(end) = after.find(quote) {
                return Some(after[..end].to_string());
            }
        }
    }
    None
}

// ─── HTML pre-processing ──────────────────────────────────────────────────────

/// Promote lazy-load image attributes to standard `src` so Readability and
/// htmd see real URLs instead of empty placeholders.
fn promote_lazy_images(html: String) -> String {
    html.replace(" data-src=", " src=")
        .replace(" data-lazy-src=", " src=")
        .replace(" data-original=", " src=")
        .replace(" data-lazy=", " src=")
}

// ─── Frontmatter + slugify ────────────────────────────────────────────────────

fn build_frontmatter(title: &str, source: &str, fetched: &str, byline: &str, excerpt: &str) -> String {
    let mut s = String::from("---\n");
    s.push_str(&format!("title: {}\n", yaml_scalar(title)));
    s.push_str("type: article\n");
    s.push_str(&format!("source: {source}\n"));
    s.push_str(&format!("fetched: {fetched}\n"));
    if !byline.is_empty() {
        s.push_str(&format!("author: {}\n", yaml_scalar(byline)));
    }
    if !excerpt.is_empty() {
        s.push_str(&format!("excerpt: {}\n", yaml_scalar(excerpt)));
    }
    s.push_str("tags: []\n");
    s.push_str("---\n");
    s
}

fn yaml_scalar(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

const MAX_SLUG_CHARS: usize = 60;

fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len().min(MAX_SLUG_CHARS));
    let mut last_dash = false;
    for ch in s.chars() {
        if out.len() >= MAX_SLUG_CHARS {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "untitled".into()
    } else {
        trimmed
    }
}
