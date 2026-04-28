//! Local (no-AI) vault mutations triggered from the UI: new note, rename,
//! delete. Runs entirely in-process; the watcher picks up the file changes
//! and refreshes the sidebar.

use camino::{Utf8Path, Utf8PathBuf};
use jiff::Timestamp;
use std::io;

const MAX_SLUG_CHARS: usize = 60;

#[derive(Debug)]
pub struct CreatedNote {
    pub absolute_path: Utf8PathBuf,
    pub relative_path: Utf8PathBuf,
}

/// Create a new markdown note under `vault_root` with a minimal frontmatter
/// stub. If a file with the same slug already exists, append `-2`, `-3`, etc.
pub fn create_note(vault_root: &Utf8Path, title: &str) -> io::Result<CreatedNote> {
    let title = title.trim();
    let display_title = if title.is_empty() { "Untitled" } else { title };
    let slug = slugify(display_title);

    let now = Timestamp::now().strftime("%Y-%m-%d").to_string();
    let title_yaml = yaml_scalar(display_title);
    let body = format!(
        "---\ntitle: {title_yaml}\ncreated: {now}\ntags: []\n---\n\n# {display_title}\n\n",
    );
    write_with_dedup(vault_root, &slug, &body)
}

/// Create a note from a pre-rendered template body. The slug is derived from
/// `title`; duplicates get a numeric suffix.
pub fn create_note_from_template(
    vault_root: &Utf8Path,
    title: &str,
    body: &str,
) -> io::Result<CreatedNote> {
    let title = title.trim();
    let display_title = if title.is_empty() { "Untitled" } else { title };
    let slug = slugify(display_title);
    write_with_dedup(vault_root, &slug, body)
}

/// Write the historical content of a note into `history/<stem>-<sha>.md`,
/// avoiding overwrites. Returns the created file so the caller can open it.
pub fn restore_note(
    vault_root: &Utf8Path,
    original_rel: &str,
    sha: &str,
    content: &str,
) -> io::Result<CreatedNote> {
    let stem = std::path::Path::new(original_rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("note");
    let history_dir = vault_root.join("history");
    std::fs::create_dir_all(history_dir.as_std_path())?;
    let slug = format!("{stem}-{sha}");
    write_with_dedup(&history_dir, &slug, content)
}

fn write_with_dedup(vault_root: &Utf8Path, slug: &str, body: &str) -> io::Result<CreatedNote> {
    let mut path = vault_root.join(format!("{slug}.md"));
    let mut suffix = 1;
    while path.exists() {
        suffix += 1;
        path = vault_root.join(format!("{slug}-{suffix}.md"));
    }
    std::fs::write(path.as_std_path(), body)?;
    let relative = path
        .strip_prefix(vault_root)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| path.clone());
    Ok(CreatedNote { absolute_path: path, relative_path: relative })
}

fn yaml_scalar(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

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
