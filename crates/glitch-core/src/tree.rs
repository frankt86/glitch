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
}

#[derive(Debug, Clone, PartialEq)]
pub struct NoteRef {
    pub id: NoteId,
    pub title: String,
    pub icon: String,
    pub keywords: Vec<String>,
}

impl TreeFolder {
    /// Build the folder tree and the parent→children map from a flat note list.
    ///
    /// Returns `(root_folder, child_map)` where `child_map` maps a parent note's
    /// ID string to the NoteRefs of its sub-notes (from `parent:` frontmatter).
    pub fn build<F>(notes: &[Note], type_emoji: F) -> (Self, HashMap<String, Vec<NoteRef>>)
    where
        F: Fn(&str) -> Option<&'static str> + Copy,
    {
        let mut root_folders: BTreeMap<String, TreeFolder> = BTreeMap::new();
        let mut root_notes: Vec<NoteRef> = Vec::new();
        let mut all_refs: Vec<NoteRef> = Vec::new();

        for note in notes {
            let note_ref = NoteRef {
                id: note.id.clone(),
                title: note.title.clone(),
                icon: note.icon(type_emoji),
                keywords: note.keywords.clone(),
            };
            all_refs.push(note_ref.clone());
            let parents = note.id.parent_components();
            if parents.is_empty() {
                root_notes.push(note_ref);
            } else {
                insert(&mut root_folders, &parents, "", note_ref);
            }
        }

        // Build child_map: resolve each note's `parent:` frontmatter field
        // using priority-based matching to avoid ambiguity (PC-3).
        let mut child_map: HashMap<String, Vec<NoteRef>> = HashMap::new();
        for note in notes {
            let Some(ref raw_parent) = note.frontmatter.parent else { continue };
            let raw_lower = raw_parent.to_lowercase();

            // 1. Exact ID match
            let parent_ref = all_refs.iter()
                .find(|n| n.id.as_str() == raw_parent.as_str())
                // 2. ID + ".md" match
                .or_else(|| all_refs.iter().find(|n| n.id.as_str() == format!("{raw_parent}.md")))
                // 3. File-stem match — only if unambiguous
                .or_else(|| {
                    let hits: Vec<_> = all_refs.iter()
                        .filter(|n| {
                            n.id.0.file_stem()
                                .map(|s| s.to_lowercase())
                                .as_deref()
                                == Some(raw_lower.as_str())
                        })
                        .collect();
                    if hits.len() == 1 { hits.into_iter().next() } else { None }
                })
                // 4. Title match — only if unambiguous
                .or_else(|| {
                    let hits: Vec<_> = all_refs.iter()
                        .filter(|n| n.title.to_lowercase() == raw_lower)
                        .collect();
                    if hits.len() == 1 { hits.into_iter().next() } else { None }
                });

            if let Some(parent) = parent_ref {
                let child_ref = all_refs.iter()
                    .find(|n| n.id == note.id)
                    .cloned();
                if let Some(child) = child_ref {
                    child_map.entry(parent.id.as_str().to_string()).or_default().push(child);
                }
            }
        }

        // Remove notes already rendered as children so they don't appear twice.
        let child_ids: std::collections::HashSet<String> = child_map
            .values()
            .flat_map(|v| v.iter().map(|n| n.id.as_str().to_string()))
            .collect();
        root_notes.retain(|n| !child_ids.contains(n.id.as_str()));
        let mut folders: Vec<TreeFolder> = root_folders.into_values().collect();
        for f in &mut folders {
            remove_children_recursive(f, &child_ids);
        }

        for f in &mut folders {
            sort_recursive(f);
        }
        folders.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        root_notes.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

        let tree = TreeFolder {
            name: String::new(),
            path: String::new(),
            folders,
            notes: root_notes,
        };
        (tree, child_map)
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

fn remove_children_recursive(
    folder: &mut TreeFolder,
    child_ids: &std::collections::HashSet<String>,
) {
    folder.notes.retain(|n| !child_ids.contains(n.id.as_str()));
    for sub in &mut folder.folders {
        remove_children_recursive(sub, child_ids);
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
        let (tree, _cm) = TreeFolder::build(&notes, |_| None);
        assert_eq!(tree.notes.len(), 1); // README.md
        assert_eq!(tree.folders.len(), 2); // people, projects
        let projects = tree.folders.iter().find(|f| f.name == "projects").unwrap();
        assert_eq!(projects.folders.len(), 2);
        assert_eq!(projects.note_count(), 3);
    }

    #[test]
    fn folders_sort_alphabetically_case_insensitive() {
        let notes = vec![note("Zoo/a.md"), note("apple/b.md"), note("Banana/c.md")];
        let (tree, _) = TreeFolder::build(&notes, |_| None);
        let names: Vec<&str> = tree.folders.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["apple", "Banana", "Zoo"]);
    }
}
