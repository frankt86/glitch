use crate::settings;
use crate::state::AppState;
use crate::types::default_emoji;
use dioxus::prelude::*;
use glitch_core::{NoteId, NoteRef, TreeFolder};
use std::collections::HashSet;

/// (title, note_type) — note_type is empty string for "no type / plain note"
#[component]
pub fn Sidebar(
    state: Signal<AppState>,
    on_create_note: EventHandler<(String, String)>,
) -> Element {
    let tree = state
        .read()
        .vault
        .as_ref()
        .map(|v| TreeFolder::build(&v.notes, default_emoji));
    let total = tree.as_ref().map(|t| t.note_count()).unwrap_or(0);

    let expanded = use_signal(|| {
        let mut set = HashSet::new();
        if let Some(t) = &tree {
            for f in &t.folders {
                set.insert(f.path.clone());
            }
        }
        set
    });

    let mut new_note_open = use_signal(|| false);
    let mut new_note_title = use_signal(String::new);
    let mut new_note_type = use_signal(String::new);
    let mut search_query = use_signal(String::new);
    let has_vault = state.read().vault.is_some();

    let current = state.read().current_note.clone();

    let available_types = settings::load_types();

    let submit = move |_evt| {
        let title = new_note_title.read().trim().to_string();
        if title.is_empty() {
            return;
        }
        let note_type = new_note_type.read().clone();
        on_create_note.call((title, note_type));
        new_note_title.set(String::new());
        new_note_type.set(String::new());
        new_note_open.set(false);
    };

    let query = search_query.read().trim().to_lowercase();
    let searching = !query.is_empty();

    // Flat list of matching notes for search mode (flattened from the already-built tree)
    let search_results: Vec<NoteRef> = if searching {
        tree.as_ref()
            .map(|t| {
                flatten_refs(t)
                    .into_iter()
                    .filter(|n| n.title.to_lowercase().contains(&query))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        vec![]
    };

    rsx! {
        nav { class: "sidebar",
            div { class: "sidebar-header",
                span { class: "sidebar-count", "{total} notes" }
                button {
                    class: "sidebar-newbtn",
                    disabled: !has_vault,
                    title: "New note",
                    onclick: move |_| {
                        let next = !*new_note_open.read();
                        new_note_open.set(next);
                        if !next {
                            new_note_title.set(String::new());
                            new_note_type.set(String::new());
                        }
                    },
                    "+ New"
                }
            }
            // ── Search box ───────────────────────────────────────────────────
            if has_vault {
                div { class: "sidebar-search-wrap",
                    input {
                        class: "sidebar-search",
                        placeholder: "Search notes…",
                        value: "{search_query.read()}",
                        oninput: move |evt: FormEvent| search_query.set(evt.value()),
                    }
                    if searching {
                        button {
                            class: "sidebar-search-clear",
                            onclick: move |_| search_query.set(String::new()),
                            "×"
                        }
                    }
                }
            }
            if *new_note_open.read() {
                div { class: "new-note-form",
                    input {
                        class: "new-note-input",
                        autofocus: true,
                        placeholder: "title…",
                        value: "{new_note_title.read()}",
                        oninput: move |evt: FormEvent| new_note_title.set(evt.value()),
                        onkeydown: {
                            let mut new_note_open = new_note_open;
                            let mut new_note_title = new_note_title;
                            let mut new_note_type = new_note_type;
                            let on_create_note = on_create_note;
                            move |evt: KeyboardEvent| {
                                if evt.key() == Key::Enter {
                                    evt.prevent_default();
                                    let title = new_note_title.read().trim().to_string();
                                    if !title.is_empty() {
                                        let note_type = new_note_type.read().clone();
                                        on_create_note.call((title, note_type));
                                        new_note_title.set(String::new());
                                        new_note_type.set(String::new());
                                        new_note_open.set(false);
                                    }
                                } else if evt.key() == Key::Escape {
                                    evt.prevent_default();
                                    new_note_title.set(String::new());
                                    new_note_type.set(String::new());
                                    new_note_open.set(false);
                                }
                            }
                        }
                    }
                    select {
                        class: "new-note-type-select",
                        onchange: move |evt: FormEvent| new_note_type.set(evt.value()),
                        option { value: "", "type…" }
                        for t in &available_types {
                            {
                                let tname = t.name.clone();
                                let temoji = t.emoji.clone();
                                rsx! {
                                    option { value: "{tname}", "{temoji} {tname}" }
                                }
                            }
                        }
                    }
                    button { class: "btn btn-primary", onclick: submit, "Create" }
                }
            }
            div { class: "tree",
                if searching {
                    // ── Search results (flat list) ────────────────────────────
                    if search_results.is_empty() {
                        div { class: "sidebar-search-empty", "No notes match" }
                    }
                    for note in search_results.iter() {
                        NoteRow {
                            key: "{note.id.as_str()}",
                            note: note.clone(),
                            depth: 0u32,
                            current: current.clone(),
                            state,
                        }
                    }
                } else if let Some(t) = tree {
                    // ── Normal tree ───────────────────────────────────────────
                    for note in t.notes.iter() {
                        NoteRow {
                            key: "{note.id.as_str()}",
                            note: note.clone(),
                            depth: 0u32,
                            current: current.clone(),
                            state,
                        }
                    }
                    for folder in t.folders.iter() {
                        FolderRow {
                            key: "{folder.path}",
                            folder: folder.clone(),
                            depth: 0u32,
                            current: current.clone(),
                            expanded,
                            state,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn FolderRow(
    folder: TreeFolder,
    depth: u32,
    current: Option<NoteId>,
    expanded: Signal<HashSet<String>>,
    state: Signal<AppState>,
) -> Element {
    let is_open = expanded.read().contains(&folder.path);
    let chevron = if is_open { "▾" } else { "▸" };
    let indent = format!("padding-left: {}px", depth * 12 + 6);
    let path = folder.path.clone();

    rsx! {
        div {
            class: "tree-row tree-folder",
            style: "{indent}",
            onclick: move |_| {
                let mut e = expanded;
                let mut set = e.write();
                if !set.insert(path.clone()) {
                    set.remove(&path);
                }
            },
            span { class: "tree-chevron", "{chevron}" }
            span { class: "tree-icon", "📁" }
            span { class: "tree-name", "{folder.name}" }
            span { class: "tree-count", "{folder.note_count()}" }
        }
        if is_open {
            for child in folder.folders.iter() {
                FolderRow {
                    key: "{child.path}",
                    folder: child.clone(),
                    depth: depth + 1,
                    current: current.clone(),
                    expanded,
                    state,
                }
            }
            for note in folder.notes.iter() {
                NoteRow {
                    key: "{note.id.as_str()}",
                    note: note.clone(),
                    depth: depth + 1,
                    current: current.clone(),
                    state,
                }
            }
        }
    }
}

#[component]
fn NoteRow(
    note: NoteRef,
    depth: u32,
    current: Option<NoteId>,
    state: Signal<AppState>,
) -> Element {
    let active = current.as_ref() == Some(&note.id);
    let class = if active { "tree-row tree-note active" } else { "tree-row tree-note" };
    let indent = format!("padding-left: {}px", depth * 12 + 22);
    let id = note.id.clone();
    let kw_count = note.keywords.len();

    rsx! {
        div {
            class: "{class}",
            style: "{indent}",
            title: if kw_count > 0 {
                format!("{} keyword(s): {}", kw_count, note.keywords.join(", "))
            } else {
                String::new()
            },
            onclick: {
                let id = id.clone();
                let mut state = state;
                move |_| load_note(&mut state, id.clone())
            },
            span { class: "tree-icon", "{note.icon}" }
            span { class: "tree-name", "{note.title}" }
            if kw_count > 0 {
                span { class: "tree-count", "{kw_count}" }
            }
        }
    }
}

fn flatten_refs(folder: &TreeFolder) -> Vec<NoteRef> {
    let mut out = folder.notes.clone();
    for sub in &folder.folders {
        out.extend(flatten_refs(sub));
    }
    out
}

fn load_note(state: &mut Signal<AppState>, id: NoteId) {
    let Some(vault) = state.read().vault.clone() else { return };
    let Some(note) = vault.notes.iter().find(|n| n.id == id) else { return };
    let content = match note.read_content() {
        Ok(c) => c,
        Err(err) => {
            tracing::warn!("failed to read {}: {err}", note.absolute_path);
            return;
        }
    };
    let mut s = state.write();
    s.current_note = Some(id);
    s.editor_content = content;
    s.editor_dirty = false;
}
