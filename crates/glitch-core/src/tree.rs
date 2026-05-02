//! Build a folder tree out of a flat list of notes — used by the sidebar.

use crate::note::{Note, NoteId};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq)]
pub struct TreeFolder {
    /// Display name (last path component). Root is "".
    pub name: String,
    /// Path relative to vault root, e.g. "projects/glitch". Root is "".
    pub path: String,
    /// Sub-folders sorted alphabetically.
    pub folders: Vec<TreeFolder>,
    /// Notes in this folder (not in sub-folders), sorted by title.
    pub notes: Vec<NoteRef>,
    /// Maps parent note ID → Vec of child NoteRefs (from `parent:` frontmatter).
    /// Only populated on the root TreeFolder; empty on sub-folders.
    pub child_map: HashMap<String, Vec<NoteRef>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NoteRef {
    pub id: NoteId,
    pub title: String,
    pub icon: String,
    pub keywords: Vec<String>,
    /// Raw `parent:` frontmatter value, if set.
    pub parent: Option<String>,
}

impl TreeFolder {
    pub fn build<F>(notes: &[Note], type_emoji: F) -> Self
    where
        F: Fn(&str) -> Option<&'static str> + Copy,
    {
        // BTreeMap so children come out alphabetically.
        let mut root_folders: BTreeMap<String, TreeFolder> = BTreeMap::new();
        let mut root_notes: Vec<NoteRef> = Vec::new();

        let mut all_refs: Vec<NoteRef> = Vec::new();

        for note in notes {
            let icon = note.icon(type_emoji);
            let note_ref = NoteRef {
                id: note.id.clone(),
                title: note.title.clone(),
                icon,
                keywords: note.keywords.clone(),
                parent: note.frontmatter.parent.clone(),
            };

            all_refs.push(note_ref.clone());
            let parents = note.id.parent_components();
            if parents.is_empty() {
                root_notes.push(note_ref);
                continue;
            }
            insert(&mut root_folders, &parents, "", note_ref);
        }

        // Build child_map: resolve each note's `parent:` field to a NoteId string.
        let mut child_map: HashMap<String, Vec<NoteRef>> = HashMap::new();
        for note_ref in &all_refs {
            let Some(ref raw_parent) = note_ref.parent else { continue };
            let raw_lower = raw_parent.to_lowercase();
            // Resolve: exact ID → {parent}.md → file-stem → title (case-insensitive)
            let parent_id = all_refs.iter().find(|n| {
                n.id.as_str() == raw_parent.as_str()
                    || n.id.as_str() == format!("{raw_parent}.md")
                    || n.id.0.file_stem().map(|s| s.to_lowercase()).as_deref() == Some(&raw_lower)
                    || n.title.to_lowercase() == raw_lower
            });
            if let Some(p) = parent_id {
                child_map
                    .entry(p.id.as_str().to_string())
                    .or_default()
                    .push(note_ref.clone());
            }
        }

        let mut folders: Vec<TreeFolder> = root_folders.into_values().collect();
        folders.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        for f in &mut folders {
            sort_recursive(f);
        }
        root_notes.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

        TreeFolder {
            name: String::new(),
            path: String::new(),
            folders,
            notes: root_notes,
            child_map,
        }
    }

    pub fn note_count(&self) -> usize {
        self.notes.len() + self.folders.iter().map(|f| f.note_count()).sum::<usize>()
    }
}

fn insert(
    folders: &mut BTreeMap<String, TreeFolder>,
    components: &[&str],
    parent_path: &str,
    note_ref: NoteRef,
) {
    let (head, tail) = components.split_first().expect("non-empty path");
    let path = if parent_path.is_empty() {
        (*head).to_string()
    } else {
        format!("{parent_path}/{head}")
    };
    let folder = folders
        .entry((*head).to_string())
        .or_insert_with(|| TreeFolder {
            name: (*head).to_string(),
            path: path.clone(),
            folders: Vec::new(),
            notes: Vec::new(),
            child_map: HashMap::new(),
        });

    if tail.is_empty() {
        folder.notes.push(note_ref);
    } else {
        let mut child_map: BTreeMap<String, TreeFolder> = std::mem::take(&mut folder.folders)
            .into_iter()
            .map(|f| (f.name.clone(), f))
            .collect();
        insert(&mut child_map, tail, &path, note_ref);
        folder.folders = child_map.into_values().collect();
    }
}

fn sort_recursive(folder: &mut TreeFolder) {
    folder.folders.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    folder
        .notes
        .sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    for child in &mut folder.folders {
        sort_recursive(child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::Frontmatter;
    use camino::Utf8PathBuf;
    use jiff::Timestamp;

    fn note(rel: &str) -> Note {
        Note {
            id: NoteId(Utf8PathBuf::from(rel)),
            absolute_path: Utf8PathBuf::from(format!("/vault/{rel}")),
            title: rel.rsplit('/').next().unwrap_or(rel).to_string(),
            frontmatter: Frontmatter::default(),
            keywords: Vec::new(),
            modified: Timestamp::UNIX_EPOCH,
            size_bytes: 0,
        }
    }

    #[test]
    fn builds_nested_tree() {
        let notes = vec![
            note("README.md"),
            note("projects/glitch/architecture.md"),
            note("projects/glitch/notes.md"),
            note("projects/tolaria/intro.md"),
            note("people/alice.md"),
        ];
        let tree = TreeFolder::build(&notes, |_| None);
        assert_eq!(tree.notes.len(), 1); // README.md
        assert_eq!(tree.folders.len(), 2); // people, projects
        let projects = tree.folders.iter().find(|f| f.name == "projects").unwrap();
        assert_eq!(projects.folders.len(), 2);
        assert_eq!(projects.note_count(), 3);
    }

    #[test]
    fn folders_sort_alphabetically_case_insensitive() {
        let notes = vec![note("Zoo/a.md"), note("apple/b.md"), note("Banana/c.md")];
        let tree = TreeFolder::build(&notes, |_| None);
        let names: Vec<&str> = tree.folders.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["apple", "Banana", "Zoo"]);
    }
}
