//! Article-to-note extractor. Runs **inside Glitch** (not via Claude) so it
//! works regardless of the local Claude environment's network sandbox.
//!
//! Pipeline: reqwest → dom_smoothie::Readability → htmd → markdown with
//! frontmatter. Robots.txt is intentionally not consulted — same posture as
//! Pocket / Reader / Obsidian Web Clipper for personal-use single-page fetches.

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

/// Fetch `url`, extract the readable article, write it as a markdown note
/// under `<vault_root>/articles/<slug>.md`, and return the new path.
pub async fn extract_to_vault(
    url: &str,
    vault_root: &Utf8Path,
) -> Result<ExtractedNote, ExtractError> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(ExtractError::InvalidUrl(url.into()));
    }

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(15))
        .build()?;
    let resp = client.get(url).send().await?.error_for_status()?;
    let html = resp.text().await?;

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
    let content_str = article.content.to_string();
    let body_md = htmd::convert(&content_str)
        .map_err(|e| ExtractError::Conversion(e.to_string()))?;

    let slug = slugify(&title);
    let articles_dir = vault_root.join("articles");
    tokio::fs::create_dir_all(articles_dir.as_std_path()).await?;

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
    // Conservative: always quote and escape backslashes/quotes.
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
