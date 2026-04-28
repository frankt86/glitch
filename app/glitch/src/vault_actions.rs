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

    let mut path = vault_root.join(format!("{slug}.md"));
    let mut suffix = 1;
    while path.exists() {
        suffix += 1;
        path = vault_root.join(format!("{slug}-{suffix}.md"));
    }

    let now = Timestamp::now().strftime("%Y-%m-%d").to_string();
    let title_yaml = yaml_scalar(display_title);
    let body = format!(
        "---\ntitle: {title_yaml}\ncreated: {now}\ntags: []\n---\n\n# {display_title}\n\n",
    );
    std::fs::write(path.as_std_path(), body)?;

    let relative = path
        .strip_prefix(vault_root)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| path.clone());
    Ok(CreatedNote {
        absolute_path: path,
        relative_path: relative,
    })
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
