use crate::frontmatter::{self, Frontmatter};
use camino::{Utf8Path, Utf8PathBuf};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;

/// Stable identifier for a note within a vault — its path relative to the vault root.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NoteId(pub Utf8PathBuf);

impl NoteId {
    pub fn from_relative(path: impl AsRef<Utf8Path>) -> Self {
        Self(path.as_ref().to_path_buf())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Components except the filename, e.g. `projects/glitch` for
    /// `projects/glitch/architecture.md`.
    pub fn parent_components(&self) -> Vec<&str> {
        let path = self.0.as_str();
        let parent = match path.rfind(['/', '\\']) {
            Some(idx) => &path[..idx],
            None => return Vec::new(),
        };
        parent.split(['/', '\\']).filter(|s| !s.is_empty()).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: NoteId,
    pub absolute_path: Utf8PathBuf,
    /// Display title — frontmatter `title` if present, else filename stem.
    pub title: String,
    /// Frontmatter parsed at load time. Empty if no frontmatter.
    pub frontmatter: Frontmatter,
    /// `tags` ∪ `keywords`, lower-cased, deduped, sorted.
    pub keywords: Vec<String>,
    pub modified: Timestamp,
    pub size_bytes: u64,
}

impl Note {
    pub fn from_path(vault_root: &Utf8Path, absolute_path: &Utf8Path) -> io::Result<Self> {
        let metadata = fs::metadata(absolute_path)?;
        let modified = metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| Timestamp::from_second(d.as_secs() as i64).unwrap_or(Timestamp::UNIX_EPOCH))
            .unwrap_or(Timestamp::UNIX_EPOCH);

        let relative = absolute_path
            .strip_prefix(vault_root)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| absolute_path.to_path_buf());

        // Parse frontmatter eagerly. For very large vaults this could become
        // a bottleneck — defer to lazy + cache later.
        let raw = fs::read_to_string(absolute_path).unwrap_or_default();
        let (fm, _body) = frontmatter::split(&raw);

        let title = fm
            .title
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| title_from_path(absolute_path));
        let keywords = fm.all_keywords();

        Ok(Self {
            id: NoteId(relative),
            absolute_path: absolute_path.to_path_buf(),
            title,
            frontmatter: fm,
            keywords,
            modified,
            size_bytes: metadata.len(),
        })
    }

    pub fn read_content(&self) -> io::Result<String> {
        fs::read_to_string(&self.absolute_path)
    }

    /// Resolve the icon for this note: explicit `icon`, else type-emoji
    /// lookup, else the default note glyph.
    pub fn icon(&self, type_emoji: impl Fn(&str) -> Option<&'static str>) -> String {
        if let Some(icon) = self.frontmatter.icon.as_deref() {
            if !icon.is_empty() {
                return icon.to_string();
            }
        }
        if let Some(t) = self.frontmatter.note_type.as_deref() {
            if let Some(em) = type_emoji(t) {
                return em.to_string();
            }
        }
        "📄".to_string()
    }
}

fn title_from_path(path: &Utf8Path) -> String {
    path.file_stem().unwrap_or("untitled").to_string()
}
