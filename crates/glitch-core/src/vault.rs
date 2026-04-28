use crate::note::Note;
use camino::{Utf8Path, Utf8PathBuf};
use std::io;
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("vault path does not exist: {0}")]
    NotFound(Utf8PathBuf),
    #[error("vault path is not a directory: {0}")]
    NotADirectory(Utf8PathBuf),
    #[error("path is not valid UTF-8")]
    NonUtf8Path,
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Debug, Clone)]
pub struct Vault {
    pub root: Utf8PathBuf,
    pub notes: Vec<Note>,
}

impl Vault {
    pub fn load(root: impl AsRef<Utf8Path>) -> Result<Self, VaultError> {
        let root = root.as_ref();
        if !root.exists() {
            return Err(VaultError::NotFound(root.to_path_buf()));
        }
        if !root.is_dir() {
            return Err(VaultError::NotADirectory(root.to_path_buf()));
        }

        let mut notes = Vec::new();
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_hidden(e.path()))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(err) => {
                    tracing::warn!("walkdir error: {err}");
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let Some(path) = Utf8Path::from_path(path) else {
                tracing::warn!(?path, "skipping non-UTF-8 path");
                continue;
            };
            if path.extension() != Some("md") {
                continue;
            }
            match Note::from_path(root, path) {
                Ok(note) => notes.push(note),
                Err(err) => tracing::warn!("failed to read note {path}: {err}"),
            }
        }

        notes.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

        Ok(Self {
            root: root.to_path_buf(),
            notes,
        })
    }
}

fn is_hidden(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with('.') && n != "." && n != "..")
        .unwrap_or(false)
}
