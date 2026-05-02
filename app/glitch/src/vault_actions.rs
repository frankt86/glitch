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

/// Create a new markdown note under `vault_root` (or a subfolder) with a
/// minimal frontmatter stub. `folder` is a vault-relative path like
/// `"projects/glitch"` or `""` for the root.
pub fn create_note(vault_root: &Utf8Path, folder: &str, title: &str) -> io::Result<CreatedNote> {
    let title = title.trim();
    let display_title = if title.is_empty() { "Untitled" } else { title };
    let slug = slugify(display_title);

    let now = Timestamp::now().strftime("%Y-%m-%d").to_string();
    let title_yaml = yaml_scalar(display_title);
    let body = format!(
        "---\ntitle: {title_yaml}\ncreated: {now}\ntags: []\n---\n\n# {display_title}\n\n",
    );
    let dir = target_dir(vault_root, folder)?;
    write_with_dedup(&dir, &slug, &body)
}

/// Create a note from a pre-rendered template body inside an optional folder.
pub fn create_note_from_template(
    vault_root: &Utf8Path,
    folder: &str,
    title: &str,
    body: &str,
) -> io::Result<CreatedNote> {
    let title = title.trim();
    let display_title = if title.is_empty() { "Untitled" } else { title };
    let slug = slugify(display_title);
    let dir = target_dir(vault_root, folder)?;
    write_with_dedup(&dir, &slug, body)
}

/// Create a folder (and any missing parents) inside the vault.
pub fn create_folder(vault_root: &Utf8Path, rel_path: &str) -> io::Result<()> {
    let rel = rel_path.trim().trim_matches(['/', '\\']);
    if rel.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty folder name"));
    }
    std::fs::create_dir_all(vault_root.join(rel))
}

/// Move a note to a different folder inside the vault by renaming the file.
/// `note_rel` and `target_folder_rel` are both vault-relative paths.
pub fn move_note(vault_root: &Utf8Path, note_rel: &str, target_folder_rel: &str) -> io::Result<()> {
    let src = vault_root.join(note_rel);
    let filename = src.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "note path has no filename")
    })?;
    let dest_dir = if target_folder_rel.is_empty() {
        vault_root.to_path_buf()
    } else {
        vault_root.join(target_folder_rel)
    };
    std::fs::create_dir_all(&dest_dir)?;
    std::fs::rename(&src, dest_dir.join(filename))
}

/// Delete a folder by moving all `.md` notes inside it (recursively) to its
/// parent directory, then removing the (now-empty) folder tree.
pub fn delete_folder(vault_root: &Utf8Path, folder_rel: &str) -> io::Result<()> {
    let folder_rel = folder_rel.trim().trim_matches(['/', '\\']);
    if folder_rel.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "cannot delete vault root"));
    }
    let folder_abs = vault_root.join(folder_rel);
    let parent_abs = folder_abs
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| vault_root.to_path_buf());
    move_md_files_to(&folder_abs, &parent_abs)?;
    std::fs::remove_dir_all(folder_abs.as_std_path())
        .or_else(|e| if e.kind() == io::ErrorKind::NotFound { Ok(()) } else { Err(e) })
}

fn move_md_files_to(src_dir: &Utf8Path, dest_dir: &Utf8Path) -> io::Result<()> {
    for entry in std::fs::read_dir(src_dir.as_std_path())? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(sub) = Utf8Path::from_path(&path) {
                move_md_files_to(sub, dest_dir)?;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let fname = path.file_name().unwrap();
            std::fs::rename(&path, dest_dir.as_std_path().join(fname))?;
        }
    }
    Ok(())
}

fn target_dir(vault_root: &Utf8Path, folder: &str) -> io::Result<Utf8PathBuf> {
    let folder = folder.trim().trim_matches(['/', '\\']);
    if folder.is_empty() {
        Ok(vault_root.to_path_buf())
    } else {
        let dir = vault_root.join(folder);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
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
