use crate::settings;
use crate::state::AppState;
use crate::types::default_emoji;
use dioxus::prelude::*;
use glitch_core::{NoteId, NoteRef, TreeFolder};
use std::collections::{HashMap, HashSet};

/// (title, note_type, folder) — folder is vault-relative, empty for root
#[component]
pub fn Sidebar(
    state: Signal<AppState>,
    on_create_note: EventHandler<(String, String, String)>,
    on_create_folder: EventHandler<String>,
    on_move_note: EventHandler<(String, String)>,
    on_delete_folder: EventHandler<String>,
    on_reparent: EventHandler<(String, Option<String>)>,
) -> Element {
    let tree_memo = use_memo(move || {
        state.read().vault.as_ref().map(|v| TreeFolder::build(&v.notes, default_emoji))
    });
    let tree = tree_memo.read().clone();
    let total = state.read().vault.as_ref().map(|v| v.notes.len()).unwrap_or(0);

    let child_map = use_memo(move || {
        tree_memo.read().as_ref().map(|t| t.child_map.clone()).unwrap_or_default()
    });

    let expanded = use_signal(|| {
        let mut set = HashSet::new();
        if let Some(t) = &tree {
            for f in &t.folders {
                set.insert(f.path.clone());
            }
        }
        set
    });
    let expanded_notes: Signal<HashSet<String>> = use_signal(HashSet::new);

    let mut new_note_open = use_signal(|| false);
    let mut new_note_title = use_signal(String::new);
    let mut new_note_type = use_signal(String::new);
    let mut new_folder_open = use_signal(|| false);
    let mut new_folder_name = use_signal(String::new);
    let mut search_query = use_signal(String::new);
    let mut dragging_note: Signal<Option<String>> = use_signal(|| None);
    let mut is_root_drag_over: Signal<bool> = use_signal(|| false);
    let mut is_unparent_drag_over: Signal<bool> = use_signal(|| false);
    let has_vault = state.read().vault.is_some();

    let current = state.read().current_note.clone();

    let available_types = settings::load_types();

    let submit = move |_evt| {
        let title = new_note_title.read().trim().to_string();
        if title.is_empty() {
            return;
        }
        let note_type = new_note_type.read().clone();
        on_create_note.call((title, note_type, String::new()));
        new_note_title.set(String::new());
        new_note_type.set(String::new());
        new_note_open.set(false);
    };

    let query = search_query.read().trim().to_lowercase();
    let searching = !query.is_empty();

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
                        new_folder_open.set(false);
                    },
                    "+ 📄"
                }
                button {
                    class: "sidebar-newbtn",
                    disabled: !has_vault,
                    title: "New folder",
                    onclick: move |_| {
                        let next = !*new_folder_open.read();
                        new_folder_open.set(next);
                        if !next { new_folder_name.set(String::new()); }
                        new_note_open.set(false);
                    },
                    "+ 📁"
                }
            }
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
            if *new_folder_open.read() {
                div { class: "new-note-form",
                    input {
                        class: "new-note-input",
                        autofocus: true,
                        placeholder: "folder name…",
                        value: "{new_folder_name.read()}",
                        oninput: move |evt: FormEvent| new_folder_name.set(evt.value()),
                        onkeydown: move |evt: KeyboardEvent| {
                            if evt.key() == Key::Enter {
                                evt.prevent_default();
                                let name = new_folder_name.read().trim().to_string();
                                if !name.is_empty() {
                                    on_create_folder.call(name);
                                    new_folder_name.set(String::new());
                                    new_folder_open.set(false);
                                }
                            } else if evt.key() == Key::Escape {
                                evt.prevent_default();
                                new_folder_name.set(String::new());
                                new_folder_open.set(false);
                            }
                        }
                    }
                    button {
                        class: "btn btn-primary",
                        onclick: move |_| {
                            let name = new_folder_name.read().trim().to_string();
                            if !name.is_empty() {
                                on_create_folder.call(name);
                                new_folder_name.set(String::new());
                                new_folder_open.set(false);
                            }
                        },
                        "Create folder"
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
                                        on_create_note.call((title, note_type, String::new()));
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
                            dragging_note,
                            child_map,
                            expanded_notes,
                            on_reparent,
                        }
                    }
                } else if let Some(t) = tree {
                    for note in t.notes.iter() {
                        NoteRow {
                            key: "{note.id.as_str()}",
                            note: note.clone(),
                            depth: 0u32,
                            current: current.clone(),
                            state,
                            dragging_note,
                            child_map,
                            expanded_notes,
                            on_reparent,
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
                            dragging_note,
                            on_move_note,
                            on_delete_folder,
                            on_reparent,
                            child_map,
                            expanded_notes,
                        }
                    }
                }
                if has_vault {
                    div {
                        class: if *is_root_drag_over.read() { "root-drop-zone drag-over" } else { "root-drop-zone" },
                        ondragover: move |evt| evt.prevent_default(),
                        ondragenter: move |_| is_root_drag_over.set(true),
                        ondragleave: move |_| is_root_drag_over.set(false),
                        ondrop: move |_| {
                            is_root_drag_over.set(false);
                            spawn(async move {
                                let mut ev = document::eval("dioxus.send(window.__glitch_drop_id||'')");
                                let js_id = ev.recv::<String>().await.ok().filter(|s| !s.is_empty());
                                let note_id = js_id.or_else(|| dragging_note.read().clone());
                                if let Some(id) = note_id {
                                    dragging_note.set(None);
                                    on_move_note.call((id, String::new()));
                                }
                            });
                        },
                        "↑ move to root"
                    }
                    if dragging_note.read().is_some() {
                        div {
                            class: if *is_unparent_drag_over.read() { "root-drop-zone drag-over" } else { "root-drop-zone" },
                            ondragover: move |evt| evt.prevent_default(),
                            ondragenter: move |_| is_unparent_drag_over.set(true),
                            ondragleave: move |_| is_unparent_drag_over.set(false),
                            ondrop: move |_| {
                                is_unparent_drag_over.set(false);
                                spawn(async move {
                                    let mut ev = document::eval("dioxus.send(window.__glitch_drop_id||'')");
                                    let js_id = ev.recv::<String>().await.ok().filter(|s| !s.is_empty());
                                    let note_id = js_id.or_else(|| dragging_note.read().clone());
                                    if let Some(id) = note_id {
                                        dragging_note.set(None);
                                        on_reparent.call((id, None));
                                    }
                                });
                            },
                            "✂ remove parent"
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
    dragging_note: Signal<Option<String>>,
    on_move_note: EventHandler<(String, String)>,
    on_delete_folder: EventHandler<String>,
    on_reparent: EventHandler<(String, Option<String>)>,
    child_map: ReadOnlySignal<HashMap<String, Vec<NoteRef>>>,
    expanded_notes: Signal<HashSet<String>>,
) -> Element {
    let is_open = expanded.read().contains(&folder.path);
    let chevron = if is_open { "▾" } else { "▸" };
    let indent = format!("padding-left: {}px", depth * 12 + 6);
    let path = folder.path.clone();
    let mut is_drag_over = use_signal(|| false);

    let folder_path_drop = folder.path.clone();
    let folder_path_delete = folder.path.clone();

    rsx! {
        div {
            class: if *is_drag_over.read() { "tree-row tree-folder drag-over" } else { "tree-row tree-folder" },
            style: "{indent}",
            onclick: move |_| {
                let mut e = expanded;
                let mut set = e.write();
                if !set.insert(path.clone()) {
                    set.remove(&path);
                }
            },
            ondragover: move |evt| {
                evt.prevent_default();
            },
            ondragenter: move |_| is_drag_over.set(true),
            ondragleave: move |_| is_drag_over.set(false),
            ondrop: {
                let folder_path_drop = folder_path_drop.clone();
                move |_| {
                    is_drag_over.set(false);
                    let fp = folder_path_drop.clone();
                    spawn(async move {
                        let mut ev = document::eval("dioxus.send(window.__glitch_drop_id||'')");
                        let js_id = ev.recv::<String>().await.ok().filter(|s| !s.is_empty());
                        let note_id = js_id.or_else(|| dragging_note.read().clone());
                        if let Some(id) = note_id {
                            dragging_note.set(None);
                            on_move_note.call((id, fp));
                        }
                    });
                }
            },
            span { class: "tree-chevron", "{chevron}" }
            span { class: "tree-icon", "📁" }
            span { class: "tree-name", "{folder.name}" }
            span { class: "tree-count", "{folder.note_count()}" }
            button {
                class: "folder-delete-btn",
                title: "Delete folder (moves notes to parent)",
                onclick: {
                    let fp = folder_path_delete.clone();
                    move |evt: MouseEvent| {
                        evt.stop_propagation();
                        on_delete_folder.call(fp.clone());
                    }
                },
                "🗑"
            }
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
                    dragging_note,
                    on_move_note,
                    on_delete_folder,
                    on_reparent,
                    child_map,
                    expanded_notes,
                }
            }
            for note in folder.notes.iter() {
                NoteRow {
                    key: "{note.id.as_str()}",
                    note: note.clone(),
                    depth: depth + 1,
                    current: current.clone(),
                    state,
                    dragging_note,
                    child_map,
                    expanded_notes,
                    on_reparent,
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
    dragging_note: Signal<Option<String>>,
    child_map: ReadOnlySignal<HashMap<String, Vec<NoteRef>>>,
    expanded_notes: Signal<HashSet<String>>,
    on_reparent: EventHandler<(String, Option<String>)>,
) -> Element {
    let children: Vec<NoteRef> =
        child_map.read().get(note.id.as_str()).cloned().unwrap_or_default();
    let has_children = !children.is_empty();
    let note_id_str = note.id.as_str().to_string();
    let is_expanded = expanded_notes.read().contains(&note_id_str);

    let active = current.as_ref() == Some(&note.id);
    let mut is_drop_target = use_signal(|| false);
    let drop_cls = if *is_drop_target.read() { " drag-over" } else { "" };
    let active_cls = if active { " active" } else { "" };
    let class = format!("tree-row tree-note{active_cls}{drop_cls}");
    let indent = format!("padding-left: {}px", depth * 12 + 22);
    let id = note.id.clone();
    let kw_count = note.keywords.len();
    let note_rel = note.id.as_str().to_string();

    rsx! {
        div {
            class: "{class}",
            style: "{indent}",
            draggable: "true",
            "data-note-id": "{note_rel}",
            title: if kw_count > 0 {
                format!("{} keyword(s): {}", kw_count, note.keywords.join(", "))
            } else {
                String::new()
            },
            ondragstart: {
                let note_rel = note_rel.clone();
                move |_| {
                    is_drop_target.set(false);
                    dragging_note.set(Some(note_rel.clone()));
                }
            },
            ondragend: move |_| {
                is_drop_target.set(false);
                dragging_note.set(None);
            },
            ondragover: move |evt| evt.prevent_default(),
            ondragenter: {
                let note_rel = note_rel.clone();
                move |_| {
                    if dragging_note.read().as_deref() != Some(note_rel.as_str()) {
                        is_drop_target.set(true);
                    }
                }
            },
            ondragleave: move |_| is_drop_target.set(false),
            ondrop: {
                let tid = note_rel.clone();
                move |_| {
                    is_drop_target.set(false);
                    let tid = tid.clone();
                    spawn(async move {
                        let mut ev = document::eval("dioxus.send(window.__glitch_drop_id||'')");
                        let js_id = ev.recv::<String>().await.ok().filter(|s| !s.is_empty());
                        let did = js_id.or_else(|| dragging_note.read().clone());
                        if let Some(did) = did {
                            dragging_note.set(None);
                            if did != tid && !is_descendant(&*child_map.read(), &did, &tid) {
                                on_reparent.call((did, Some(tid.clone())));
                            }
                        }
                    });
                }
            },
            onclick: {
                let id = id.clone();
                let mut state = state;
                let note_id_str = note_id_str.clone();
                move |_| {
                    if has_children {
                        let mut set = expanded_notes.write();
                        if !set.remove(&note_id_str) {
                            set.insert(note_id_str.clone());
                        }
                    }
                    load_note(&mut state, id.clone())
                }
            },
            if has_children {
                span { class: "tree-chevron", if is_expanded { "▾" } else { "▸" } }
            }
            span { class: "tree-icon", "{note.icon}" }
            span { class: "tree-name", "{note.title}" }
            if kw_count > 0 {
                span { class: "tree-count", "{kw_count}" }
            }
        }
        if is_expanded {
            for child in children.iter() {
                NoteRow {
                    key: "{child.id.as_str()}",
                    note: child.clone(),
                    depth: depth + 1,
                    current: current.clone(),
                    state,
                    dragging_note,
                    child_map,
                    expanded_notes,
                    on_reparent,
                }
            }
        }
    }
}

/// Returns true if `needle` is anywhere in the subtree rooted at `ancestor`.
/// Used to prevent drag-drop cycles (e.g. dropping a parent onto its own child).
fn is_descendant(child_map: &HashMap<String, Vec<NoteRef>>, ancestor: &str, needle: &str) -> bool {
    let mut visited = HashSet::new();
    is_descendant_inner(child_map, ancestor, needle, &mut visited)
}

fn is_descendant_inner(
    child_map: &HashMap<String, Vec<NoteRef>>,
    current: &str,
    needle: &str,
    visited: &mut HashSet<String>,
) -> bool {
    if !visited.insert(current.to_string()) {
        return false;
    }
    if let Some(children) = child_map.get(current) {
        for child in children {
            let cid = child.id.as_str();
            if cid == needle || is_descendant_inner(child_map, cid, needle, visited) {
                return true;
            }
        }
    }
    false
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
