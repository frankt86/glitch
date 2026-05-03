use crate::components::slash_palette::{
    matches as palette_matches, slash_query, tiptap_cmd_for, SlashPalette,
};
use crate::components::table::GlitchTableView;
use crate::settings;
use crate::state::AppState;
use crate::vault_actions;
use camino::Utf8PathBuf;
use dioxus::prelude::*;
use glitch_core::{frontmatter as fm, parse_all_tables, replace_table_block, GlitchTable, NoteId};
use glitch_embed::SimilarNote;
use glitch_sync::CommitInfo;

// ---------------------------------------------------------------------------
// TipTap helpers
// ---------------------------------------------------------------------------

#[cfg(windows)]
const TIPTAP_SRC: &str = "http://glitch-editor.localhost/";
#[cfg(not(windows))]
const TIPTAP_SRC: &str = "glitch-editor://localhost/";

/// Push raw markdown to the TipTap iframe.
/// Strips frontmatter, then also strips any leading `# Heading` line —
/// that heading is shown in the editor-title-h1 input instead.
fn push_to_tiptap(content: &str) {
    let (_, body) = fm::split_raw(content);
    let body_to_show = strip_leading_h1(&body);
    let json = serde_json::to_string(&body_to_show).unwrap_or_else(|_| "\"\"".into());
    document::eval(&format!(
        "var f=document.getElementById('glitch-tiptap');\
         if(f&&f.contentWindow)f.contentWindow.postMessage({{type:'set-content',content:{json}}},'*');"
    ));
}

/// Strip a leading `# Heading` line from the body (any H1, not just matching the title).
fn strip_leading_h1(body: &str) -> String {
    let trimmed = body.trim_start_matches('\n');
    if trimmed.starts_with("# ") {
        let after = trimmed.find('\n').map(|i| &trimmed[i..]).unwrap_or("");
        return after.trim_start_matches(['\r', '\n']).to_string();
    }
    body.to_string()
}

/// Extract the text of a leading `# Heading` line, if present.
fn extract_leading_h1(body: &str) -> Option<String> {
    let trimmed = body.trim_start_matches('\n');
    let rest = trimmed.strip_prefix("# ")?;
    let heading = rest.lines().next()?.trim().to_string();
    if heading.is_empty() { None } else { Some(heading) }
}

/// True if the body (after frontmatter) starts with any `# Heading`.
fn body_has_leading_h1(content: &str) -> bool {
    let (_, body) = fm::split_raw(content);
    body.trim_start_matches('\n').starts_with("# ")
}

fn insert_table_in_tiptap() {
    document::eval(
        "var f=document.getElementById('glitch-tiptap');\
         if(f&&f.contentWindow){\
           f.contentWindow.postMessage({type:'clear-slash'},'*');\
           f.contentWindow.postMessage({type:'insert-table'},'*');\
         }",
    );
}

fn clear_tiptap_slash() {
    document::eval(
        "var f=document.getElementById('glitch-tiptap');\
         if(f&&f.contentWindow)f.contentWindow.postMessage({type:'clear-slash'},'*');",
    );
}

/// Send a formatting command to TipTap. The iframe clears the /command text
/// then applies the format in a single message handler.
fn send_format_to_tiptap(cmd: &str) {
    let cmd_json = serde_json::to_string(cmd).unwrap_or_else(|_| "\"\"".into());
    document::eval(&format!(
        "var f=document.getElementById('glitch-tiptap');\
         if(f&&f.contentWindow)\
           f.contentWindow.postMessage({{type:'format-command',command:{cmd_json}}},'*');"
    ));
}

/// Map a TipTap format command to the raw markdown prefix/delimiter for the Source tab.
fn format_cmd_to_markdown(cmd: &str) -> &'static str {
    match cmd {
        "h1"        => "# ",
        "h2"        => "## ",
        "h3"        => "### ",
        "h4"        => "#### ",
        "h5"        => "##### ",
        "h6"        => "###### ",
        "quote"     => "> ",
        "bullet"    => "- ",
        "numbered"  => "1. ",
        "divider"   => "---",
        "bold"      => "**",
        "italic"    => "_",
        "strike"    => "~~",
        "code"      => "`",
        "codeblock" => "```",
        _           => "",
    }
}

fn make_default_table_block() -> String {
    "```glitch-table\n{\n  \"schema\": {\n    \"columns\": [\n      { \"name\": \"Name\", \"type\": \"text\" }\n    ]\n  },\n  \"rows\": []\n}\n```"
        .to_string()
}

// ---------------------------------------------------------------------------
// Frontmatter helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Detail-tab field definitions per note type
// ---------------------------------------------------------------------------

/// Returns (display_label, yaml_key, input_hint) for the Detail tab.
/// hint: "text" | "url" | "textarea" | "tags" | "type-select"
fn fields_for_type(note_type: &str) -> Vec<(&'static str, &'static str, &'static str)> {
    match note_type {
        "article" => vec![
            ("Type", "type", "type-select"),
            ("Source URL", "source", "url"),
            ("Author", "author", "text"),
            ("Fetched", "fetched", "text"),
            ("Excerpt", "excerpt", "textarea"),
            ("Tags", "tags", "tags"),
        ],
        "meeting" => vec![
            ("Type", "type", "type-select"),
            ("Date", "date", "text"),
            ("Attendees", "attendees", "text"),
            ("Tags", "tags", "tags"),
        ],
        "book" => vec![
            ("Type", "type", "type-select"),
            ("Author", "author", "text"),
            ("Started", "started", "text"),
            ("Finished", "finished", "text"),
            ("Tags", "tags", "tags"),
        ],
        "person" => vec![
            ("Type", "type", "type-select"),
            ("Role", "role", "text"),
            ("Contact", "contact", "text"),
            ("Tags", "tags", "tags"),
        ],
        "project" => vec![
            ("Type", "type", "type-select"),
            ("Status", "status", "text"),
            ("Started", "started", "text"),
            ("Tags", "tags", "tags"),
        ],
        _ => vec![
            ("Type", "type", "type-select"),
            ("Tags", "tags", "tags"),
        ],
    }
}

// ---------------------------------------------------------------------------
// Source-tab slash helpers
// ---------------------------------------------------------------------------

fn editor_slash_query(content: &str) -> Option<String> {
    let last_line = content.rsplit('\n').next().unwrap_or(content);
    slash_query(last_line).map(|s| s.to_string())
}

fn strip_slash_line(content: &str) -> String {
    match content.rfind('\n') {
        Some(idx) => content[..=idx].to_string(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Editor component
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum EditorTab {
    Edit,
    Detail,
    Source,
    History,
    Tables,
    Related,
}

#[derive(Clone, PartialEq)]
enum RelatedState {
    Idle,
    Loading,
    /// Loaded for note at `key` (vault-relative path).
    Results { key: String, notes: Vec<SimilarNote> },
    Error(String),
}

#[derive(Clone, PartialEq)]
enum HistoryState {
    Idle,
    Loading,
    Commits(Vec<CommitInfo>),
    LoadingDiff(CommitInfo),
    Diff { commit: CommitInfo, historical: String },
    Empty,
    Error(String),
}

#[component]
pub fn Editor(state: Signal<AppState>, on_command: EventHandler<String>) -> Element {
    let mut palette_index = use_signal(|| 0usize);
    let mut tab = use_signal(|| EditorTab::Edit);
    let mut hist_state = use_signal(|| HistoryState::Idle);
    let mut hist_note = use_signal(|| Option::<String>::None);
    let mut last_pushed_note: Signal<Option<String>> = use_signal(|| None);
    let mut tiptap_slash_text = use_signal(String::new);
    let mut tiptap_ready = use_signal(|| false);
    let mut delete_pending = use_signal(|| false);
    // Tracks whether the current note's body starts with "# {title}" so we can
    // reconstruct that heading when TipTap content-changed fires (we strip it
    // on push to avoid showing the title twice alongside editor-title-h1).
    let mut note_has_leading_h1 = use_signal(|| false);
    let mut related_state = use_signal(|| RelatedState::Idle);

    // Debounced auto-save: any dirty change triggers a 1.5s coalesced write.
    let save_tx = use_coroutine(move |mut rx: UnboundedReceiver<()>| async move {
        while rx.recv().await.is_ok() {
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            while rx.try_recv().is_ok() {}
            save_current(&mut state);
        }
    });

    let title = state
        .read()
        .current_note()
        .map(|n| n.id.as_str().to_string())
        .unwrap_or_else(|| "no note selected".into());
    let content = state.read().editor_content.clone();
    let dirty = state.read().editor_dirty;
    let has_note = state.read().current_note.is_some();

    let vault_root: Option<Utf8PathBuf> = state.read().vault.as_ref().map(|v| v.root.clone());
    let current_rel: Option<String> = state
        .read()
        .current_note
        .clone()
        .map(|id| id.as_str().to_string());

    // Reset history and delete-confirm when note changes.
    {
        let note_key = current_rel.clone();
        if *hist_note.read() != note_key {
            hist_note.set(note_key);
            hist_state.set(HistoryState::Idle);
            delete_pending.set(false);
        }
    }

    // Push content to TipTap when the active note changes.
    {
        let note_key = current_rel.clone();
        if *last_pushed_note.read() != note_key {
            last_pushed_note.set(note_key);
            note_has_leading_h1.set(body_has_leading_h1(&content));
            push_to_tiptap(&content);
        }
    }

    if *tiptap_ready.read() {
        tiptap_ready.set(false);
        push_to_tiptap(&content);
    }

    let mut palette_text = use_signal(String::new);
    let last_line = content.rsplit('\n').next().unwrap_or(&content).to_string();
    if *palette_text.read() != last_line {
        palette_text.set(last_line);
    }

    // Bridge: listen for postMessages from the TipTap iframe.
    use_effect(move || {
        spawn(async move {
            let mut eval = document::eval(
                "if (!window.__glitchBridge) {\
                   window.__glitchBridge = true;\
                   window.addEventListener('message', function(e) {\
                     if (e.data && e.data.type) dioxus.send(e.data);\
                   });\
                 }",
            );
            loop {
                match eval.recv::<serde_json::Value>().await {
                    Ok(val) => match val.get("type").and_then(|t| t.as_str()) {
                        Some("ctrl-s") => {
                            save_current(&mut state);
                        }
                        Some("content-changed") => {
                            if let Some(md) = val.get("content").and_then(|c| c.as_str()) {
                                let mut s = state.write();
                                let (yaml, _) = fm::split_raw(&s.editor_content);
                                // If the note originally had "# Title" as its first body
                                // line, put it back (we stripped it on push to TipTap).
                                let body = if *note_has_leading_h1.read() {
                                    let title = fm::get_field(&yaml, "title");
                                    if title.is_empty() {
                                        md.to_string()
                                    } else {
                                        format!("# {title}\n\n{md}")
                                    }
                                } else {
                                    md.to_string()
                                };
                                s.editor_content = fm::join_raw(&yaml, &body);
                                s.editor_dirty = true;
                                drop(s);
                                save_tx.send(());
                            }
                        }
                        Some("tiptap-ready") => {
                            tiptap_ready.set(true);
                        }
                        Some("slash-changed") => {
                            let is_null = val.get("query").map_or(true, |q| q.is_null());
                            if is_null {
                                tiptap_slash_text.set(String::new());
                            } else if let Some(q) = val.get("query").and_then(|q| q.as_str()) {
                                tiptap_slash_text.set(format!("/{q}"));
                            }
                        }
                        _ => {}
                    },
                    Err(_) => break,
                }
            }
        });
    });

    let on_history_tab = {
        let vault_root = vault_root.clone();
        let current_rel = current_rel.clone();
        move |_| {
            tab.set(EditorTab::History);
            let already_loaded = matches!(
                &*hist_state.read(),
                HistoryState::Commits(_)
                    | HistoryState::Diff { .. }
                    | HistoryState::LoadingDiff(_)
            );
            if already_loaded {
                return;
            }
            if let (Some(root), Some(rel)) = (&vault_root, &current_rel) {
                let root = root.clone();
                let rel = rel.clone();
                hist_state.set(HistoryState::Loading);
                spawn(async move {
                    match glitch_sync::file_history(&root, &rel).await {
                        Ok(commits) if commits.is_empty() => hist_state.set(HistoryState::Empty),
                        Ok(commits) => hist_state.set(HistoryState::Commits(commits)),
                        Err(e) => hist_state.set(HistoryState::Error(e.to_string())),
                    }
                });
            }
        }
    };

    // Shared embed trigger — called by both the tab click and the Recalculate button.
    let mut run_related = {
        let vault_root = vault_root.clone();
        let current_rel = current_rel.clone();
        let content = content.clone();
        move || {
            let (Some(root), Some(rel)) = (&vault_root, &current_rel) else { return };
            let root = root.clone();
            let rel = rel.clone();
            let content = content.clone();
            related_state.set(RelatedState::Loading);
            spawn(async move {
                let root2 = root.clone();
                let rel2 = rel.clone();
                let result = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<SimilarNote>> {
                    let cache_dir = std::env::var("LOCALAPPDATA")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|_| std::env::temp_dir())
                        .join("Glitch").join("models");
                    glitch_embed::embed_note(&root2, &rel2, &content, &cache_dir)?;
                    glitch_embed::find_similar(&root2, &rel2, 5)
                }).await;
                match result {
                    Ok(Ok(notes)) => related_state.set(RelatedState::Results { key: rel, notes }),
                    Ok(Err(e)) => related_state.set(RelatedState::Error(e.to_string())),
                    Err(e) => related_state.set(RelatedState::Error(e.to_string())),
                }
            });
        }
    };

    let on_related_tab = {
        let mut run_related = run_related.clone();
        let current_rel = current_rel.clone();
        move |_| {
            tab.set(EditorTab::Related);
            // Don't re-compute if we already have results for this exact note.
            if let RelatedState::Results { key, .. } = &*related_state.read() {
                if Some(key.as_str()) == current_rel.as_deref() {
                    return;
                }
            }
            run_related();
        }
    };

    let current_tab = tab.read().clone();
    let show_save = matches!(
        current_tab,
        EditorTab::Edit | EditorTab::Detail | EditorTab::Source | EditorTab::Tables
    );
    let tables = parse_all_tables(&content);
    let has_tables = !tables.is_empty();

    // Derive frontmatter values for title H1 and Detail tab.
    let (yaml, body) = fm::split_raw(&content);
    // Use frontmatter title if set; fall back to the leading # heading in the body.
    let fm_title = {
        let t = fm::get_field(&yaml, "title");
        if t.is_empty() {
            extract_leading_h1(&body).unwrap_or_default()
        } else {
            t
        }
    };
    let note_type = fm::get_field(&yaml, "type");

    rsx! {
        section { class: "editor",
            header { class: "editor-header",
                div { class: "editor-tabs",
                    button {
                        class: if current_tab == EditorTab::Edit { "editor-tab active" } else { "editor-tab" },
                        onclick: move |_| {
                            if *tab.read() != EditorTab::Edit {
                                tab.set(EditorTab::Edit);
                                push_to_tiptap(&state.read().editor_content);
                            }
                        },
                        "Edit"
                    }
                    if has_note {
                        button {
                            class: if current_tab == EditorTab::Detail { "editor-tab active" } else { "editor-tab" },
                            onclick: move |_| tab.set(EditorTab::Detail),
                            "Detail"
                        }
                    }
                    button {
                        class: if current_tab == EditorTab::Source { "editor-tab active" } else { "editor-tab" },
                        onclick: move |_| tab.set(EditorTab::Source),
                        "Source"
                    }
                    button {
                        class: if current_tab == EditorTab::History { "editor-tab active" } else { "editor-tab" },
                        disabled: !has_note,
                        onclick: on_history_tab,
                        "History"
                    }
                    if has_note {
                        button {
                            class: if current_tab == EditorTab::Tables { "editor-tab active" } else { "editor-tab" },
                            onclick: move |_| tab.set(EditorTab::Tables),
                            "Tables"
                        }
                    }
                    if has_note {
                        button {
                            class: if current_tab == EditorTab::Related { "editor-tab active" } else { "editor-tab" },
                            onclick: on_related_tab,
                            "Related"
                        }
                    }
                }
                span { class: "editor-title", "{title}" }
                if dirty && show_save {
                    span { class: "editor-dirty-pip", title: "unsaved changes", "●" }
                }
                if show_save {
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
                // Delete note button with inline confirmation.
                if has_note {
                    if *delete_pending.read() {
                        span { class: "editor-delete-confirm",
                            span { "Delete this note?" }
                            button {
                                class: "btn btn-danger",
                                onclick: {
                                    let mut state = state;
                                    move |_| {
                                        delete_pending.set(false);
                                        delete_current(&mut state);
                                    }
                                },
                                "Yes, delete"
                            }
                            button {
                                class: "btn",
                                onclick: move |_| delete_pending.set(false),
                                "Cancel"
                            }
                        }
                    } else {
                        button {
                            class: "btn editor-delete-btn",
                            title: "Delete this note",
                            onclick: move |_| delete_pending.set(true),
                            "🗑"
                        }
                    }
                }
            }

            if current_tab == EditorTab::Edit {
                // Title H1 input above the TipTap iframe.
                div { class: "editor-title-area",
                    input {
                        class: "editor-title-h1",
                        r#type: "text",
                        placeholder: "Untitled",
                        value: "{fm_title}",
                        oninput: {
                            let mut state = state;
                            move |evt: FormEvent| {
                                let new_title = evt.value();
                                let mut s = state.write();
                                let (yaml, body) = fm::split_raw(&s.editor_content);
                                let new_yaml = fm::set_field(&yaml, "title", &new_title);
                                // Keep the body # heading in sync if the note has one.
                                let new_body = if *note_has_leading_h1.read() {
                                    let trimmed = body.trim_start_matches('\n');
                                    let after_h1 = trimmed
                                        .find('\n')
                                        .map(|i| trimmed[i..].trim_start_matches('\n'))
                                        .unwrap_or("");
                                    format!("# {new_title}\n\n{after_h1}")
                                } else {
                                    body
                                };
                                s.editor_content = fm::join_raw(&new_yaml, &new_body);
                                s.editor_dirty = true;
                                drop(s);
                                save_tx.send(());
                            }
                        }
                    }
                }
                div { class: "editor-single",
                    div { class: "editor-pane",
                        SlashPalette {
                            text: tiptap_slash_text,
                            selected: palette_index,
                            on_select: {
                                let on_command = on_command;
                                move |insertion: &'static str| {
                                    let cmd = insertion.trim();
                                    if cmd == "/table" {
                                        insert_table_in_tiptap();
                                    } else if let Some(fmt) = tiptap_cmd_for(cmd) {
                                        send_format_to_tiptap(fmt);
                                    } else {
                                        on_command.call(cmd.to_string());
                                        clear_tiptap_slash();
                                    }
                                    palette_index.set(0);
                                    tiptap_slash_text.set(String::new());
                                }
                            }
                        }
                        iframe {
                            id: "glitch-tiptap",
                            class: "tiptap-host",
                            src: TIPTAP_SRC,
                        }
                    }
                }
            } else if current_tab == EditorTab::Detail {
                div { class: "editor-single",
                    div { class: "detail-pane",
                        div { class: "detail-field",
                            label { class: "detail-label", "Title" }
                            input {
                                class: "detail-input",
                                r#type: "text",
                                placeholder: "Untitled",
                                value: "{fm_title}",
                                oninput: {
                                    let mut state = state;
                                    move |evt: FormEvent| {
                                        let mut s = state.write();
                                        s.editor_content = fm::update_field(
                                            &s.editor_content,
                                            "title",
                                            &evt.value(),
                                        );
                                        s.editor_dirty = true;
                                    }
                                }
                            }
                        }
                        for (label, key, hint) in fields_for_type(&note_type) {
                            {
                                let raw_val = fm::get_field(&yaml, key);
                                let display_val =
                                    if hint == "tags" { fm::tags_to_str(&raw_val) } else { raw_val };
                                if hint == "type-select" {
                                    let available_types = settings::load_types();
                                    let cur_type = note_type.clone();
                                    rsx! {
                                        div { class: "detail-field", key: "f-{key}",
                                            label { class: "detail-label", "{label}" }
                                            select {
                                                class: "detail-input detail-select",
                                                onchange: {
                                                    let mut state = state;
                                                    move |evt: FormEvent| {
                                                        let mut s = state.write();
                                                        s.editor_content = fm::update_field(
                                                            &s.editor_content,
                                                            "type",
                                                            &evt.value(),
                                                        );
                                                        s.editor_dirty = true;
                                                    }
                                                },
                                                option {
                                                    value: "",
                                                    selected: cur_type.is_empty(),
                                                    "— none —"
                                                }
                                                for t in available_types {
                                                    {
                                                        let tname = t.name.clone();
                                                        let temoji = t.emoji.clone();
                                                        let is_sel = tname == cur_type;
                                                        rsx! {
                                                            option {
                                                                value: "{tname}",
                                                                selected: is_sel,
                                                                "{temoji} {tname}"
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else if hint == "textarea" {
                                    rsx! {
                                        div { class: "detail-field", key: "f-{key}",
                                            label { class: "detail-label", "{label}" }
                                            textarea {
                                                class: "detail-input detail-textarea",
                                                value: "{display_val}",
                                                oninput: {
                                                    let mut state = state;
                                                    move |evt: FormEvent| {
                                                        let mut s = state.write();
                                                        s.editor_content = fm::update_field(
                                                            &s.editor_content,
                                                            key,
                                                            &evt.value(),
                                                        );
                                                        s.editor_dirty = true;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    // text, url, tags
                                    let itype = if hint == "url" { "url" } else { "text" };
                                    rsx! {
                                        div { class: "detail-field", key: "f-{key}",
                                            label { class: "detail-label", "{label}" }
                                            input {
                                                class: "detail-input",
                                                r#type: "{itype}",
                                                value: "{display_val}",
                                                oninput: {
                                                    let mut state = state;
                                                    move |evt: FormEvent| {
                                                        let raw = evt.value();
                                                        let write_val = if hint == "tags" {
                                                            fm::str_to_tags(&raw)
                                                        } else {
                                                            raw
                                                        };
                                                        let mut s = state.write();
                                                        s.editor_content = fm::update_field(
                                                            &s.editor_content,
                                                            key,
                                                            &write_val,
                                                        );
                                                        s.editor_dirty = true;
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
            } else if current_tab == EditorTab::Source {
                div { class: "editor-single",
                    div { class: "editor-pane",
                        SlashPalette {
                            text: palette_text,
                            selected: palette_index,
                            on_select: {
                                let mut state = state;
                                let on_command = on_command;
                                move |insertion: &'static str| {
                                    let cmd = insertion.trim();
                                    if cmd == "/table" {
                                        let block = make_default_table_block();
                                        let mut s = state.write();
                                        let base = strip_slash_line(&s.editor_content)
                                            .trim_end()
                                            .to_string();
                                        s.editor_content = format!("{}\n\n{}", base, block);
                                        s.editor_dirty = true;
                                        drop(s);
                                        tab.set(EditorTab::Tables);
                                    } else if let Some(fmt) = tiptap_cmd_for(cmd) {
                                        let prefix = format_cmd_to_markdown(fmt);
                                        let mut s = state.write();
                                        let base = strip_slash_line(&s.editor_content)
                                            .trim_end()
                                            .to_string();
                                        s.editor_content = if base.is_empty() {
                                            prefix.to_string()
                                        } else {
                                            format!("{}\n{}", base, prefix)
                                        };
                                        s.editor_dirty = true;
                                    } else {
                                        on_command.call(cmd.to_string());
                                        let mut s = state.write();
                                        s.editor_content = strip_slash_line(&s.editor_content);
                                        s.editor_dirty = true;
                                    }
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
                                    drop(s);
                                    save_tx.send(());
                                    palette_index.set(0);
                                }
                            },
                            onkeydown: {
                                let mut state = state;
                                let on_command = on_command;
                                move |evt: KeyboardEvent| {
                                    // Ctrl+S saves the note.
                                    if evt.modifiers().ctrl() && evt.key() == Key::Character("s".to_string()) {
                                        evt.prevent_default();
                                        save_current(&mut state);
                                        return;
                                    }
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
                                            let i =
                                                (*palette_index.read()).min(items.len() - 1);
                                            let chosen = items[i];
                                            let cmd = chosen.insertion.trim();
                                            if cmd == "/table" {
                                                let block = make_default_table_block();
                                                let mut s = state.write();
                                                let base = strip_slash_line(&s.editor_content)
                                                    .trim_end()
                                                    .to_string();
                                                s.editor_content =
                                                    format!("{}\n\n{}", base, block);
                                                s.editor_dirty = true;
                                                drop(s);
                                                tab.set(EditorTab::Tables);
                                            } else if let Some(fmt) = tiptap_cmd_for(cmd) {
                                                let prefix = format_cmd_to_markdown(fmt);
                                                let mut s = state.write();
                                                let base = strip_slash_line(&s.editor_content)
                                                    .trim_end()
                                                    .to_string();
                                                s.editor_content = if base.is_empty() {
                                                    prefix.to_string()
                                                } else {
                                                    format!("{}\n{}", base, prefix)
                                                };
                                                s.editor_dirty = true;
                                            } else {
                                                on_command.call(cmd.to_string());
                                                let mut s = state.write();
                                                s.editor_content =
                                                    strip_slash_line(&s.editor_content);
                                                s.editor_dirty = true;
                                            }
                                            palette_index.set(0);
                                        }
                                        Key::Escape => {
                                            evt.prevent_default();
                                            let mut s = state.write();
                                            s.editor_content =
                                                strip_slash_line(&s.editor_content);
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
            } else if current_tab == EditorTab::Tables {
                div { class: "editor-single",
                    div { class: "tables-pane",
                        if !has_tables {
                            div { class: "tables-empty",
                                p { "No tables in this note yet." }
                                button {
                                    class: "btn",
                                    onclick: {
                                        let mut state = state;
                                        move |_| {
                                            let block = make_default_table_block();
                                            let mut s = state.write();
                                            let base = s.editor_content.trim_end().to_string();
                                            s.editor_content = if base.is_empty() {
                                                block
                                            } else {
                                                format!("{}\n\n{}", base, block)
                                            };
                                            s.editor_dirty = true;
                                        }
                                    },
                                    "New Table"
                                }
                            }
                        }
                        for table in parse_all_tables(&content) {
                            {
                                let block_idx = table.block_index;
                                let mut s2 = state;
                                rsx! {
                                    GlitchTableView {
                                        key: "{block_idx}",
                                        table: table,
                                        on_change: move |updated: GlitchTable| {
                                            let md = s2.read().editor_content.clone();
                                            if let Some(new_md) =
                                                replace_table_block(&md, block_idx, &updated.to_json())
                                            {
                                                let mut sw = s2.write();
                                                sw.editor_content = new_md;
                                                sw.editor_dirty = true;
                                            }
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            } else if current_tab == EditorTab::Related {
                RelatedPanel {
                    related_state,
                    app_state: state,
                    on_recalculate: move |_| run_related(),
                }
            } else {
                HistoryPanel {
                    hist_state,
                    current_content: content,
                    vault_root,
                    current_rel,
                    app_state: state,
                    tab,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Delete helper
// ---------------------------------------------------------------------------

fn delete_current(state: &mut Signal<AppState>) {
    let snapshot = state.read();
    let Some(note) = snapshot.current_note() else { return };
    let path = note.absolute_path.clone();
    drop(snapshot);
    if let Err(err) = trash::delete(&path) {
        tracing::error!("failed to trash {path}: {err}");
        return;
    }
    let mut s = state.write();
    s.current_note = None;
    s.editor_content.clear();
    s.editor_dirty = false;
    tracing::info!("trashed {path}");
}

// ---------------------------------------------------------------------------
// Related panel
// ---------------------------------------------------------------------------

#[component]
fn RelatedPanel(
    related_state: Signal<RelatedState>,
    app_state: Signal<AppState>,
    on_recalculate: EventHandler<()>,
) -> Element {
    match related_state.read().clone() {
        RelatedState::Idle => rsx! {
            div { class: "related-pane related-idle",
                p { "Click the Related tab to find similar notes." }
            }
        },
        RelatedState::Loading => rsx! {
            div { class: "related-pane related-loading",
                p { "Indexing note…" }
                p { class: "related-hint", "First run downloads the embedding model (~130 MB)." }
            }
        },
        RelatedState::Error(msg) => rsx! {
            div { class: "related-pane related-error",
                p { class: "related-error-msg", "Error: {msg}" }
                button {
                    class: "btn",
                    onclick: move |_| on_recalculate.call(()),
                    "↺ Retry"
                }
            }
        },
        RelatedState::Results { notes, .. } => rsx! {
            div { class: "related-pane",
                div { class: "related-toolbar",
                    button {
                        class: "btn",
                        onclick: move |_| on_recalculate.call(()),
                        "↺ Recalculate"
                    }
                }
                if notes.is_empty() {
                    p { class: "related-empty", "No similar notes found yet. Add more notes to the vault to see suggestions." }
                } else {
                    for note in notes.iter() {
                        {
                            let rel = note.rel_path.clone();
                            let pct = (note.score * 100.0) as u32;
                            let title = rel
                                .rsplit('/')
                                .next()
                                .unwrap_or(&rel)
                                .trim_end_matches(".md")
                                .to_string();
                            let mut app_state = app_state;
                            rsx! {
                                button {
                                    class: "related-row",
                                    onclick: move |_| {
                                        let vault = app_state.read().vault.as_ref().map(|v| v.root.clone());
                                        if let Some(root) = vault {
                                            let abs = root.join(&rel);
                                            if let Ok(content) = std::fs::read_to_string(&abs) {
                                                let mut s = app_state.write();
                                                s.current_note = Some(glitch_core::NoteId::from_relative(rel.clone()));
                                                s.editor_content = content;
                                                s.editor_dirty = false;
                                            }
                                        }
                                    },
                                    span { class: "related-title", "{title}" }
                                    span { class: "related-score", "{pct}%" }
                                }
                            }
                        }
                    }
                }
            }
        },
    }
}

// ---------------------------------------------------------------------------
// History panel
// ---------------------------------------------------------------------------

#[component]
fn HistoryPanel(
    hist_state: Signal<HistoryState>,
    current_content: String,
    vault_root: Option<Utf8PathBuf>,
    current_rel: Option<String>,
    app_state: Signal<AppState>,
    tab: Signal<EditorTab>,
) -> Element {
    match hist_state.read().clone() {
        HistoryState::Idle | HistoryState::Loading => rsx! {
            div { class: "history-panel history-center", "Loading history…" }
        },
        HistoryState::Empty => rsx! {
            div { class: "history-panel history-center",
                p { "No git history for this note." }
                p { class: "history-hint",
                    "Commit your vault to git to start tracking changes."
                }
            }
        },
        HistoryState::Error(msg) => rsx! {
            div { class: "history-panel history-center",
                p { "Could not load history:" }
                pre { class: "history-error", "{msg}" }
            }
        },
        HistoryState::Commits(commits) => rsx! {
            div { class: "history-panel",
                div { class: "history-list",
                    for commit in commits {
                        CommitRow {
                            commit: commit.clone(),
                            vault_root: vault_root.clone(),
                            current_rel: current_rel.clone(),
                            hist_state,
                        }
                    }
                }
            }
        },
        HistoryState::LoadingDiff(commit) => rsx! {
            div { class: "history-panel history-center",
                "Loading diff for {commit.sha}…"
            }
        },
        HistoryState::Diff { commit, historical } => rsx! {
            DiffView {
                commit,
                historical,
                current_content,
                vault_root,
                current_rel,
                hist_state,
                app_state,
                tab,
            }
        },
    }
}

#[component]
fn CommitRow(
    commit: CommitInfo,
    vault_root: Option<Utf8PathBuf>,
    current_rel: Option<String>,
    hist_state: Signal<HistoryState>,
) -> Element {
    let sha = commit.sha.clone();
    let on_click = {
        let commit = commit.clone();
        move |_| {
            if let (Some(root), Some(rel)) = (&vault_root, &current_rel) {
                let root = root.clone();
                let rel = rel.clone();
                let commit_for_state = commit.clone();
                let sha_fetch = sha.clone();
                hist_state.set(HistoryState::LoadingDiff(commit_for_state.clone()));
                spawn(async move {
                    match glitch_sync::file_at_rev(&root, &rel, &sha_fetch).await {
                        Ok(historical) => hist_state.set(HistoryState::Diff {
                            commit: commit_for_state,
                            historical,
                        }),
                        Err(e) => hist_state.set(HistoryState::Error(e.to_string())),
                    }
                });
            }
        }
    };

    rsx! {
        div { class: "history-row", onclick: on_click,
            span { class: "history-sha", "{commit.sha}" }
            span { class: "history-date", "{commit.date}" }
            span { class: "history-author", "{commit.author}" }
            span { class: "history-msg", "{commit.message}" }
        }
    }
}

#[component]
fn DiffView(
    commit: CommitInfo,
    historical: String,
    current_content: String,
    vault_root: Option<Utf8PathBuf>,
    current_rel: Option<String>,
    hist_state: Signal<HistoryState>,
    app_state: Signal<AppState>,
    tab: Signal<EditorTab>,
) -> Element {
    let diff_lines = compute_diff_lines(&historical, &current_content);
    let stats = diff_stats(&diff_lines);

    let vault_root2 = vault_root.clone();
    let current_rel2 = current_rel.clone();
    let on_back = move |_| {
        if let (Some(root), Some(rel)) = (&vault_root2, &current_rel2) {
            let root = root.clone();
            let rel = rel.clone();
            hist_state.set(HistoryState::Loading);
            spawn(async move {
                match glitch_sync::file_history(&root, &rel).await {
                    Ok(commits) if commits.is_empty() => hist_state.set(HistoryState::Empty),
                    Ok(commits) => hist_state.set(HistoryState::Commits(commits)),
                    Err(e) => hist_state.set(HistoryState::Error(e.to_string())),
                }
            });
        }
    };

    let on_restore = {
        let historical = historical.clone();
        let commit = commit.clone();
        move |_| {
            let Some(root) = &vault_root else { return };
            let Some(rel) = &current_rel else { return };
            match vault_actions::restore_note(root, rel, &commit.sha, &historical) {
                Ok(created) => {
                    let id = NoteId(created.relative_path.clone());
                    let content = std::fs::read_to_string(&created.absolute_path)
                        .unwrap_or_else(|_| historical.clone());
                    let mut s = app_state.write();
                    s.current_note = Some(id);
                    s.editor_content = content;
                    s.editor_dirty = false;
                    drop(s);
                    tab.set(EditorTab::Edit);
                }
                Err(e) => tracing::error!("restore failed: {e}"),
            }
        }
    };

    rsx! {
        div { class: "history-panel diff-panel",
            div { class: "diff-toolbar",
                button { class: "btn", onclick: on_back, "← Back" }
                span { class: "diff-commit-info",
                    span { class: "history-sha", "{commit.sha}" }
                    " · {commit.date} · {commit.author}"
                }
                span { class: "diff-stats",
                    span { class: "diff-stat-add", "+{stats.added}" }
                    " / "
                    span { class: "diff-stat-del", "-{stats.removed}" }
                }
                button { class: "btn", onclick: on_restore,
                    "Save as history/{commit.sha}.md"
                }
            }
            div { class: "diff-body",
                for (tag, line) in &diff_lines {
                    DiffLine { tag: *tag, line: line.clone() }
                }
            }
        }
    }
}

#[component]
fn DiffLine(tag: char, line: String) -> Element {
    let class = match tag {
        '+' => "diff-line diff-add",
        '-' => "diff-line diff-del",
        _ => "diff-line diff-ctx",
    };
    let prefix = match tag {
        '+' => "+",
        '-' => "-",
        _ => " ",
    };
    rsx! {
        div { class: "{class}",
            span { class: "diff-prefix", "{prefix}" }
            span { class: "diff-text", "{line}" }
        }
    }
}

struct DiffStats {
    added: usize,
    removed: usize,
}

fn diff_stats(lines: &[(char, String)]) -> DiffStats {
    let added = lines.iter().filter(|(t, _)| *t == '+').count();
    let removed = lines.iter().filter(|(t, _)| *t == '-').count();
    DiffStats { added, removed }
}

fn compute_diff_lines(old: &str, new: &str) -> Vec<(char, String)> {
    let diff = similar::TextDiff::from_lines(old, new);
    diff.iter_all_changes()
        .map(|c| {
            let tag = match c.tag() {
                similar::ChangeTag::Delete => '-',
                similar::ChangeTag::Insert => '+',
                similar::ChangeTag::Equal => ' ',
            };
            let line = c.value().trim_end_matches('\n').to_string();
            (tag, line)
        })
        .collect()
}

fn save_current(state: &mut Signal<AppState>) {
    let snapshot = state.read();
    let Some(note) = snapshot.current_note() else { return };
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

    #[test]
    fn diff_lines_detect_changes() {
        let lines = compute_diff_lines("hello\nworld\n", "hello\nearth\n");
        assert!(lines.iter().any(|(t, l)| *t == '-' && l == "world"));
        assert!(lines.iter().any(|(t, l)| *t == '+' && l == "earth"));
    }


}
