use crate::settings::{self, AppSettings, NoteTypeConfig, PermissionProfile};
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
                    PermissionProfilesSection { draft }
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
                    NoteTypesSection {}
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
fn NoteTypesSection() -> Element {
    let types = settings::load_types();
    let types_path = settings::types_config_path()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "(unknown)".into());
    let tmpl_dir = settings::templates_dir()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "(unknown)".into());

    rsx! {
        section { class: "settings-section",
            h3 { class: "settings-section-title", "Note types" }
            div { class: "settings-field",
                label { class: "settings-label", "Registered types" }
                div { class: "settings-control",
                    div { class: "note-types-list",
                        for t in types {
                            NoteTypeRow { cfg: t }
                        }
                    }
                    div { class: "settings-hint",
                        "Edit "
                        span { class: "mono", "{types_path}" }
                        " to add or rename types. Templates live in "
                        span { class: "mono", "{tmpl_dir}" }
                        ". Use "
                        span { class: "mono", "/note <title> --type <name>" }
                        " to create from a template."
                    }
                }
            }
        }
    }
}

#[component]
fn NoteTypeRow(cfg: NoteTypeConfig) -> Element {
    rsx! {
        div { class: "note-type-row",
            span { class: "note-type-emoji", "{cfg.emoji}" }
            span { class: "note-type-name", "{cfg.name}" }
            if !cfg.template.is_empty() {
                span { class: "note-type-template", "{cfg.template}" }
            }
        }
    }
}

static BUILTIN_NAMES: &[&str] = &["Read-only", "Standard", "Power"];

#[component]
fn PermissionProfilesSection(draft: Signal<AppSettings>) -> Element {
    let mut new_name = use_signal(String::new);
    let mut new_tools = use_signal(String::new);
    let mut add_open = use_signal(|| false);

    let active = draft.read().active_profile.clone();
    let profiles = draft.read().profiles.clone();

    rsx! {
        section { class: "settings-section",
            h3 { class: "settings-section-title", "Permission profiles" }
            div { class: "settings-field",
                label { class: "settings-label", "Active profile" }
                div { class: "settings-control",
                    div { class: "perm-profile-grid",
                        for profile in profiles.iter() {
                            {
                                let pname = profile.name.clone();
                                let pname2 = pname.clone();
                                let tools_preview: String = profile.allowed_tools
                                    .split(',')
                                    .map(str::trim)
                                    .take(4)
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                let is_active = active == pname;
                                let is_builtin = BUILTIN_NAMES.contains(&pname.as_str());
                                rsx! {
                                    div {
                                        key: "{pname}",
                                        class: if is_active { "perm-profile-card active" } else { "perm-profile-card" },
                                        onclick: move |_| draft.write().active_profile = pname2.clone(),
                                        div { class: "perm-profile-name", "{pname}" }
                                        div { class: "perm-profile-tools", "{tools_preview}…" }
                                        if !is_builtin {
                                            button {
                                                class: "perm-profile-del",
                                                title: "Delete profile",
                                                onclick: {
                                                    let pname = pname.clone();
                                                    move |evt: MouseEvent| {
                                                        evt.stop_propagation();
                                                        let mut d = draft.write();
                                                        d.profiles.retain(|p| p.name != pname);
                                                        if d.active_profile == pname {
                                                            d.active_profile = "Standard".into();
                                                        }
                                                    }
                                                },
                                                "×"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        button {
                            class: "perm-profile-add-btn",
                            onclick: move |_| add_open.set(true),
                            "+ Custom"
                        }
                    }
                    if *add_open.read() {
                        div { class: "perm-profile-add-form",
                            input {
                                class: "settings-input settings-input-narrow",
                                placeholder: "Profile name",
                                value: "{new_name.read()}",
                                oninput: move |e: FormEvent| new_name.set(e.value()),
                            }
                            input {
                                class: "settings-input",
                                placeholder: "Allowed tools (comma-separated)",
                                value: "{new_tools.read()}",
                                oninput: move |e: FormEvent| new_tools.set(e.value()),
                            }
                            div { class: "perm-profile-add-actions",
                                button {
                                    class: "btn btn-primary",
                                    onclick: move |_| {
                                        let name = new_name.read().trim().to_string();
                                        let tools = new_tools.read().trim().to_string();
                                        if !name.is_empty() && !tools.is_empty() {
                                            draft.write().profiles.push(PermissionProfile {
                                                name: name.clone(),
                                                allowed_tools: tools,
                                                disallowed_tools: String::new(),
                                            });
                                            draft.write().active_profile = name;
                                            new_name.set(String::new());
                                            new_tools.set(String::new());
                                            add_open.set(false);
                                        }
                                    },
                                    "Add"
                                }
                                button {
                                    class: "btn",
                                    onclick: move |_| {
                                        new_name.set(String::new());
                                        new_tools.set(String::new());
                                        add_open.set(false);
                                    },
                                    "Cancel"
                                }
                            }
                        }
                    }
                    div { class: "settings-hint",
                        "Profiles set which tools auto-approve vs. show a permission modal. "
                        "Changes take effect on the next session restart."
                    }
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
