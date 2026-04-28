use crate::settings::{self, AppSettings};
use dioxus::prelude::*;

#[component]
pub fn SettingsPanel(
    visible: Signal<bool>,
    settings: Signal<AppSettings>,
) -> Element {
    if !*visible.read() {
        return rsx! { Fragment {} };
    }

    let mut draft = use_signal(|| settings.read().clone());
    use_effect(move || {
        // Resync when re-opened with newer settings.
        if *visible.read() {
            draft.set(settings.read().clone());
        }
    });

    let close = move |_| visible.set(false);

    let save_and_close = {
        let mut visible = visible;
        let mut settings = settings;
        move |_| {
            let new_settings = draft.read().clone();
            if let Err(err) = settings::save(&new_settings) {
                tracing::error!("failed to save settings: {err}");
            }
            settings.set(new_settings);
            visible.set(false);
        }
    };

    let settings_path = settings::settings_path()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "(unknown)".into());

    rsx! {
        div { class: "modal-overlay", onclick: close,
            div { class: "settings-card", onclick: move |e| e.stop_propagation(),
                header { class: "settings-header",
                    h2 { "Settings" }
                    button { class: "btn-link", onclick: close, "Close" }
                }
                div { class: "settings-body",
                    Section { title: "Claude integration",
                        Field { label: "Claude binary",
                            input {
                                class: "settings-input",
                                value: "{draft.read().claude_binary}",
                                oninput: move |e: FormEvent| draft.write().claude_binary = e.value(),
                            }
                            div { class: "settings-hint",
                                "Path to the `claude` executable, or just `claude` to resolve from PATH."
                            }
                        }
                        Field { label: "Auto-approved tools",
                            input {
                                class: "settings-input",
                                value: "{draft.read().allowed_tools_silent}",
                                oninput: move |e: FormEvent| draft.write().allowed_tools_silent = e.value(),
                            }
                            div { class: "settings-hint",
                                "Comma-separated list of tools that bypass the approval modal. Default: Read,Glob,Grep,LS,TodoWrite."
                            }
                        }
                    }
                    Section { title: "Git sync",
                        Field { label: "Auto-sync vault to GitHub",
                            label { class: "settings-toggle",
                                input {
                                    r#type: "checkbox",
                                    checked: "{draft.read().auto_sync}",
                                    oninput: move |e: FormEvent| draft.write().auto_sync = e.value() == "true",
                                }
                                span { "Pull, commit, and push every {draft.read().auto_sync_interval_minutes} minutes" }
                            }
                        }
                        Field { label: "Sync interval (minutes)",
                            input {
                                class: "settings-input settings-input-narrow",
                                r#type: "number",
                                min: "1",
                                value: "{draft.read().auto_sync_interval_minutes}",
                                oninput: move |e: FormEvent| {
                                    if let Ok(n) = e.value().parse::<u32>() {
                                        draft.write().auto_sync_interval_minutes = n.max(1);
                                    }
                                },
                            }
                        }
                        Field { label: "Include in git",
                            label { class: "settings-toggle",
                                input {
                                    r#type: "checkbox",
                                    checked: "{draft.read().commit_chats_to_git}",
                                    oninput: move |e: FormEvent| draft.write().commit_chats_to_git = e.value() == "true",
                                }
                                span { "Chat history (`.glitch/chats/`) — useful for paired devices, leaks transcripts otherwise" }
                            }
                            label { class: "settings-toggle",
                                input {
                                    r#type: "checkbox",
                                    checked: "{draft.read().commit_embeddings_to_git}",
                                    oninput: move |e: FormEvent| draft.write().commit_embeddings_to_git = e.value() == "true",
                                }
                                span { "Embeddings cache (`.glitch/embeddings.bin`) — large; usually keep machine-local" }
                            }
                        }
                    }
                    Section { title: "Agent instructions",
                        Field { label: "Instructions file",
                            input {
                                class: "settings-input",
                                value: "{draft.read().agent_instructions_path}",
                                oninput: move |e: FormEvent| {
                                    draft.write().agent_instructions_path = e.value().into();
                                },
                            }
                            div { class: "settings-hint",
                                "Path to a markdown file prepended to every Claude session. Stored in app config, not the vault, so it never syncs to GitHub."
                            }
                        }
                    }
                    Section { title: "Note types",
                        Field { label: "Type registry",
                            div { class: "settings-readonly",
                                "Built-in types: meeting 🗓 · person 👤 · book 📚 · project 🚧 · article 📰 · idea 💡 · todo ✅ · log 📋 · code 💻 · place 📍 · event 🎉 · question ❓"
                            }
                            div { class: "settings-hint",
                                "Customisable types + templates land in M2.75-tail (next iteration) at `%APPDATA%\\Glitch\\types.toml` and `templates\\<type>.md`."
                            }
                        }
                    }
                    Section { title: "About",
                        Field { label: "Version",
                            div { class: "settings-readonly", "Glitch v{env!(\"CARGO_PKG_VERSION\")} · pure-Rust AI-native vault" }
                        }
                        Field { label: "Settings file",
                            div { class: "settings-readonly mono", "{settings_path}" }
                        }
                    }
                }
                footer { class: "settings-footer",
                    button { class: "btn", onclick: close, "Cancel" }
                    button { class: "btn btn-primary", onclick: save_and_close, "Save" }
                }
            }
        }
    }
}

#[component]
fn Section(title: &'static str, children: Element) -> Element {
    rsx! {
        section { class: "settings-section",
            h3 { class: "settings-section-title", "{title}" }
            {children}
        }
    }
}

#[component]
fn Field(label: &'static str, children: Element) -> Element {
    rsx! {
        div { class: "settings-field",
            label { class: "settings-label", "{label}" }
            div { class: "settings-control", {children} }
        }
    }
}
