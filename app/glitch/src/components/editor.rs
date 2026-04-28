use crate::components::slash_palette::{matches as palette_matches, slash_query, SlashPalette};
use crate::state::AppState;
use dioxus::prelude::*;

/// Returns the slash query if the editor's last line is a slash-command-in-progress.
fn editor_slash_query(content: &str) -> Option<String> {
    let last_line = content.rsplit('\n').next().unwrap_or(content);
    slash_query(last_line).map(|s| s.to_string())
}

/// Replace the editor's last line (which must be a slash command) with empty.
/// Preserves the trailing newline if present so the cursor stays on its own line.
fn strip_slash_line(content: &str) -> String {
    match content.rfind('\n') {
        Some(idx) => content[..=idx].to_string(),
        None => String::new(),
    }
}

#[component]
pub fn Editor(state: Signal<AppState>, on_command: EventHandler<String>) -> Element {
    let mut palette_index = use_signal(|| 0usize);

    let title = state
        .read()
        .current_note()
        .map(|n| n.id.as_str().to_string())
        .unwrap_or_else(|| "no note selected".into());
    let content = state.read().editor_content.clone();
    let dirty = state.read().editor_dirty;
    let has_note = state.read().current_note.is_some();

    // Mirror the editor content into a Signal<String> for the palette renderer.
    let mut palette_text = use_signal(String::new);
    let last_line = content.rsplit('\n').next().unwrap_or(&content).to_string();
    if *palette_text.read() != last_line {
        palette_text.set(last_line.clone());
    }

    rsx! {
        section { class: "editor",
            header { class: "editor-header",
                span { class: "editor-title", "{title}" }
                if dirty {
                    span { class: "editor-dirty-pip", title: "unsaved changes", "●" }
                }
                button {
                    class: "btn",
                    disabled: !dirty || !has_note,
                    onclick: {
                        let mut state = state;
                        move |_| save_current(&mut state)
                    },
                    "Save"
                }
            }
            div { class: "editor-single",
                div { class: "editor-pane",
                    SlashPalette {
                        text: palette_text,
                        selected: palette_index,
                        on_select: {
                            let mut state = state;
                            let on_command = on_command;
                            move |insertion: &'static str| {
                                // Click in palette: dispatch the command and strip the slash line.
                                on_command.call(insertion.trim().to_string());
                                let mut s = state.write();
                                s.editor_content = strip_slash_line(&s.editor_content);
                                s.editor_dirty = true;
                                palette_index.set(0);
                            }
                        }
                    }
                    textarea {
                        class: "editor-textarea",
                        spellcheck: "false",
                        placeholder: "select a note from the sidebar… or type / for commands",
                        value: "{content}",
                        oninput: {
                            let mut state = state;
                            move |evt: FormEvent| {
                                let mut s = state.write();
                                s.editor_content = evt.value();
                                s.editor_dirty = true;
                                palette_index.set(0);
                            }
                        },
                        onkeydown: {
                            let mut state = state;
                            let on_command = on_command;
                            move |evt: KeyboardEvent| {
                                let body = state.read().editor_content.clone();
                                let q = editor_slash_query(&body);
                                let palette_open = q.is_some();
                                let items = if palette_open {
                                    palette_matches(q.as_deref().unwrap_or(""))
                                } else {
                                    Vec::new()
                                };
                                if !palette_open || items.is_empty() {
                                    return;
                                }
                                match evt.key() {
                                    Key::ArrowDown => {
                                        evt.prevent_default();
                                        let len = items.len();
                                        let mut i = palette_index.write();
                                        *i = (*i + 1) % len;
                                    }
                                    Key::ArrowUp => {
                                        evt.prevent_default();
                                        let len = items.len();
                                        let mut i = palette_index.write();
                                        *i = if *i == 0 { len - 1 } else { *i - 1 };
                                    }
                                    Key::Enter if !evt.modifiers().shift() => {
                                        evt.prevent_default();
                                        let i = (*palette_index.read()).min(items.len() - 1);
                                        let chosen = items[i];
                                        on_command.call(chosen.insertion.trim().to_string());
                                        let mut s = state.write();
                                        s.editor_content = strip_slash_line(&s.editor_content);
                                        s.editor_dirty = true;
                                        palette_index.set(0);
                                    }
                                    Key::Escape => {
                                        evt.prevent_default();
                                        // Drop the palette-open status by removing the trailing slash text.
                                        let mut s = state.write();
                                        s.editor_content = strip_slash_line(&s.editor_content);
                                        s.editor_dirty = true;
                                        palette_index.set(0);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn save_current(state: &mut Signal<AppState>) {
    let snapshot = state.read();
    let Some(note) = snapshot.current_note() else {
        return;
    };
    let path = note.absolute_path.clone();
    let content = snapshot.editor_content.clone();
    drop(snapshot);

    if let Err(err) = std::fs::write(&path, &content) {
        tracing::error!("failed to save {path}: {err}");
        return;
    }
    state.write().editor_dirty = false;
    tracing::info!("saved {path}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_slash_on_last_line() {
        assert_eq!(editor_slash_query("# title\n\n/no").as_deref(), Some("no"));
        assert_eq!(editor_slash_query("/help").as_deref(), Some("help"));
    }

    #[test]
    fn no_palette_for_inline_slashes() {
        assert!(editor_slash_query("a /path/somewhere").is_none());
        assert!(editor_slash_query("# heading\nplain text").is_none());
    }

    #[test]
    fn strip_keeps_prior_lines() {
        assert_eq!(
            strip_slash_line("# title\n\n/note"),
            "# title\n\n".to_string()
        );
        assert_eq!(strip_slash_line("/help"), "");
    }
}
