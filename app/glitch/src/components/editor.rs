use crate::components::slash_palette::{matches as palette_matches, slash_query, SlashPalette};
use crate::components::table::GlitchTableView;
use crate::state::AppState;
use crate::vault_actions;
use camino::Utf8PathBuf;
use dioxus::prelude::*;
use glitch_core::{parse_all_tables, replace_table_block, GlitchTable, NoteId};
use glitch_sync::CommitInfo;

// ---------------------------------------------------------------------------
// TipTap helpers
// ---------------------------------------------------------------------------

#[cfg(windows)]
const TIPTAP_SRC: &str = "http://glitch-editor.localhost/";
#[cfg(not(windows))]
const TIPTAP_SRC: &str = "glitch-editor://localhost/";

/// Push raw markdown to the single TipTap iframe. Tables are kept as-is;
/// TipTap renders them inline via the GlitchCodeBlock NodeView.
fn push_to_tiptap(content: &str) {
    let json = serde_json::to_string(content).unwrap_or_else(|_| "\"\"".into());
    document::eval(&format!(
        "var f=document.getElementById('glitch-tiptap');\
         if(f&&f.contentWindow)f.contentWindow.postMessage({{type:'set-content',content:{json}}},'*');"
    ));
}

/// Tell TipTap to delete the /command text at the cursor, then insert a
/// default glitch-table code block at that position.
fn insert_table_in_tiptap() {
    document::eval(
        "var f=document.getElementById('glitch-tiptap');\
         if(f&&f.contentWindow){\
           f.contentWindow.postMessage({type:'clear-slash'},'*');\
           f.contentWindow.postMessage({type:'insert-table'},'*');\
         }",
    );
}

/// Tell TipTap to delete the /command text at the cursor (used by non-table
/// slash commands selected from the Edit-tab palette).
fn clear_tiptap_slash() {
    document::eval(
        "var f=document.getElementById('glitch-tiptap');\
         if(f&&f.contentWindow)f.contentWindow.postMessage({type:'clear-slash'},'*');",
    );
}

/// Default glitch-table block for the Source tab and Tables tab.
fn make_default_table_block() -> String {
    "```glitch-table\n{\n  \"schema\": {\n    \"columns\": [\n      { \"name\": \"Name\", \"type\": \"text\" }\n    ]\n  },\n  \"rows\": []\n}\n```"
        .to_string()
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
    Source,
    History,
    Tables,
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
    // Slash text forwarded from TipTap via postMessage → drives SlashPalette.
    let mut tiptap_slash_text = use_signal(String::new);
    // True when TipTap fires tiptap-ready; consumed during the same render.
    let mut tiptap_ready = use_signal(|| false);

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

    // Reset history when note changes.
    {
        let note_key = current_rel.clone();
        if *hist_note.read() != note_key {
            hist_note.set(note_key);
            hist_state.set(HistoryState::Idle);
        }
    }

    // Push content to TipTap when the active note changes.
    {
        let note_key = current_rel.clone();
        if *last_pushed_note.read() != note_key {
            last_pushed_note.set(note_key);
            push_to_tiptap(&content);
        }
    }

    // When TipTap finishes (re)loading, push current content.
    if *tiptap_ready.read() {
        tiptap_ready.set(false);
        push_to_tiptap(&content);
    }

    // Mirror last line so the Source-tab SlashPalette stays reactive.
    let mut palette_text = use_signal(String::new);
    let last_line = content.rsplit('\n').next().unwrap_or(&content).to_string();
    if *palette_text.read() != last_line {
        palette_text.set(last_line);
    }

    // Bridge: listen for postMessages from the TipTap iframe (once on mount).
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
                        Some("content-changed") => {
                            if let Some(md) = val.get("content").and_then(|c| c.as_str()) {
                                let mut s = state.write();
                                s.editor_content = md.to_string();
                                s.editor_dirty = true;
                            }
                        }
                        Some("tiptap-ready") => {
                            tiptap_ready.set(true);
                        }
                        Some("slash-changed") => {
                            let is_null = val.get("query").map_or(true, |q| q.is_null());
                            if is_null {
                                tiptap_slash_text.set(String::new());
                            } else if let Some(q) =
                                val.get("query").and_then(|q| q.as_str())
                            {
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
                        Ok(commits) if commits.is_empty() => {
                            hist_state.set(HistoryState::Empty)
                        }
                        Ok(commits) => hist_state.set(HistoryState::Commits(commits)),
                        Err(e) => hist_state.set(HistoryState::Error(e.to_string())),
                    }
                });
            }
        }
    };

    let current_tab = tab.read().clone();
    let show_save =
        matches!(current_tab, EditorTab::Edit | EditorTab::Source | EditorTab::Tables);
    let tables = parse_all_tables(&content);
    let has_tables = !tables.is_empty();

    rsx! {
        section { class: "editor",
            header { class: "editor-header",
                div { class: "editor-tabs",
                    button {
                        class: if current_tab == EditorTab::Edit { "editor-tab active" } else { "editor-tab" },
                        onclick: move |_| {
                            if *tab.read() != EditorTab::Edit {
                                tab.set(EditorTab::Edit);
                                // Sync Source edits into TipTap when switching back.
                                push_to_tiptap(&state.read().editor_content);
                            }
                        },
                        "Edit"
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
            }

            if current_tab == EditorTab::Edit {
                div { class: "editor-single",
                    div { class: "editor-pane",
                        // Slash palette driven by TipTap's slash-changed messages.
                        SlashPalette {
                            text: tiptap_slash_text,
                            selected: palette_index,
                            on_select: {
                                let on_command = on_command;
                                move |insertion: &'static str| {
                                    if insertion.trim() == "/table" {
                                        insert_table_in_tiptap();
                                    } else {
                                        on_command.call(insertion.trim().to_string());
                                        clear_tiptap_slash();
                                    }
                                    palette_index.set(0);
                                    tiptap_slash_text.set(String::new());
                                }
                            }
                        }
                        // Single TipTap iframe — tables appear inline via NodeView.
                        iframe {
                            id: "glitch-tiptap",
                            class: "tiptap-host",
                            src: TIPTAP_SRC,
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
                                    if insertion.trim() == "/table" {
                                        let block = make_default_table_block();
                                        let mut s = state.write();
                                        let base =
                                            strip_slash_line(&s.editor_content)
                                                .trim_end()
                                                .to_string();
                                        s.editor_content = format!("{}\n\n{}", base, block);
                                        s.editor_dirty = true;
                                        drop(s);
                                        tab.set(EditorTab::Tables);
                                    } else {
                                        on_command.call(insertion.trim().to_string());
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
                                    if !palette_open || items.is_empty() { return; }
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
                                            if let Some(new_md) = replace_table_block(
                                                &md,
                                                block_idx,
                                                &updated.to_json(),
                                            ) {
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
// History panel (unchanged)
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
