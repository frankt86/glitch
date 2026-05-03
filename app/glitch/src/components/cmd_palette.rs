use crate::state::AppState;
use crate::types::default_emoji;
use dioxus::prelude::*;
use glitch_core::NoteId;

#[derive(Clone, PartialEq)]
enum PaletteItem {
    Note { id: NoteId, title: String, icon: String },
    Command { name: &'static str, description: &'static str },
}

const COMMANDS: &[(&str, &str)] = &[
    ("/note",    "Create a new note"),
    ("/daily",   "Open today's daily note"),
    ("/extract", "Extract article from URL"),
    ("/search",  "Full-text search"),
    ("/table",   "Insert a table"),
];

#[component]
pub fn CommandPalette(
    state: Signal<AppState>,
    open: Signal<bool>,
    on_open_note: EventHandler<NoteId>,
    on_run_command: EventHandler<String>,
) -> Element {
    if !*open.read() {
        return rsx! {};
    }

    let mut query = use_signal(String::new);
    let mut selected: Signal<usize> = use_signal(|| 0);

    let q = query.read().trim().to_ascii_lowercase();

    let note_items: Vec<PaletteItem> = {
        let snap = state.read();
        if let Some(vault) = snap.vault.as_ref() {
            vault.notes.iter()
                .filter(|n| q.is_empty() || n.title.to_ascii_lowercase().contains(&q))
                .take(20)
                .map(|n| PaletteItem::Note {
                    id: n.id.clone(),
                    title: n.title.clone(),
                    icon: n.icon(default_emoji),
                })
                .collect()
        } else {
            vec![]
        }
    };

    let cmd_items: Vec<PaletteItem> = COMMANDS.iter()
        .filter(|(name, desc)| {
            q.is_empty()
                || name.to_ascii_lowercase().contains(&q)
                || desc.to_ascii_lowercase().contains(&q)
        })
        .map(|(name, desc)| PaletteItem::Command { name, description: desc })
        .collect();

    let items: Vec<PaletteItem> = note_items.into_iter().chain(cmd_items).collect();
    let total = items.len();

    // Clamp selection to valid range.
    let sel = (*selected.read()).min(total.saturating_sub(1));

    let dismiss = move |_| {
        open.set(false);
        query.set(String::new());
        selected.set(0);
    };

    let accept = {
        let items = items.clone();
        move || {
            let item = items.get(sel).cloned();
            open.set(false);
            query.set(String::new());
            selected.set(0);
            match item {
                Some(PaletteItem::Note { id, .. }) => on_open_note.call(id),
                Some(PaletteItem::Command { name, .. }) => on_run_command.call(name.to_string()),
                None => {}
            }
        }
    };

    rsx! {
        div {
            class: "cmd-overlay",
            onclick: dismiss,
            div {
                class: "cmd-palette",
                onclick: move |evt| evt.stop_propagation(),
                input {
                    class: "cmd-input",
                    autofocus: true,
                    placeholder: "Go to note or run command…",
                    value: "{query.read()}",
                    oninput: move |evt: FormEvent| {
                        query.set(evt.value());
                        selected.set(0);
                    },
                    onkeydown: {
                        let mut accept = accept.clone();
                        move |evt: KeyboardEvent| {
                            match evt.key() {
                                Key::Escape => {
                                    evt.prevent_default();
                                    open.set(false);
                                    query.set(String::new());
                                    selected.set(0);
                                }
                                Key::ArrowDown => {
                                    evt.prevent_default();
                                    if total > 0 {
                                        selected.set((sel + 1) % total);
                                    }
                                }
                                Key::ArrowUp => {
                                    evt.prevent_default();
                                    if total > 0 {
                                        selected.set(if sel == 0 { total - 1 } else { sel - 1 });
                                    }
                                }
                                Key::Enter => {
                                    evt.prevent_default();
                                    accept();
                                }
                                _ => {}
                            }
                        }
                    }
                }
                div { class: "cmd-list",
                    if items.is_empty() {
                        div { class: "cmd-empty", "No results" }
                    }
                    for (idx, item) in items.iter().enumerate() {
                        {
                            let item = item.clone();
                            let item2 = item.clone();
                            let is_sel = idx == sel;
                            let mut accept2 = accept.clone();
                            rsx! {
                                div {
                                    key: "{idx}",
                                    class: if is_sel { "cmd-row cmd-row-active" } else { "cmd-row" },
                                    onclick: move |_| {
                                        selected.set(idx);
                                        accept2();
                                    },
                                    onmouseenter: move |_| selected.set(idx),
                                    match &item2 {
                                        PaletteItem::Note { icon, title, .. } => rsx! {
                                            span { class: "cmd-icon", "{icon}" }
                                            span { class: "cmd-title", "{title}" }
                                        },
                                        PaletteItem::Command { name, description } => rsx! {
                                            span { class: "cmd-icon cmd-cmd-icon", ">" }
                                            span { class: "cmd-title", "{name}" }
                                            span { class: "cmd-desc", "{description}" }
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
