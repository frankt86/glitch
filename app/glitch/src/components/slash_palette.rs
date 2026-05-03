//! Floating autocomplete dropdown for slash commands. Triggered by `/` at the
//! start of an input. Matching commands filter as the user types.

use dioxus::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandHint {
    pub name: &'static str,
    pub args: &'static str,
    pub description: &'static str,
    /// What to insert into the input when chosen.
    pub insertion: &'static str,
    /// If Some, the command maps to a TipTap formatting action rather than
    /// going to the AI. The value is the format identifier sent to TipTap.
    pub tiptap_cmd: Option<&'static str>,
}

pub const COMMANDS: &[CommandHint] = &[
    // ── Headings ──────────────────────────────────────────────────────────────
    CommandHint { name: "h1",       args: "", description: "Heading 1",         insertion: "/h1",       tiptap_cmd: Some("h1") },
    CommandHint { name: "h2",       args: "", description: "Heading 2",         insertion: "/h2",       tiptap_cmd: Some("h2") },
    CommandHint { name: "h3",       args: "", description: "Heading 3",         insertion: "/h3",       tiptap_cmd: Some("h3") },
    CommandHint { name: "h4",       args: "", description: "Heading 4",         insertion: "/h4",       tiptap_cmd: Some("h4") },
    CommandHint { name: "h5",       args: "", description: "Heading 5",         insertion: "/h5",       tiptap_cmd: Some("h5") },
    CommandHint { name: "h6",       args: "", description: "Heading 6",         insertion: "/h6",       tiptap_cmd: Some("h6") },
    // ── Inline formatting ─────────────────────────────────────────────────────
    CommandHint { name: "bold",     args: "", description: "Bold",              insertion: "/bold",     tiptap_cmd: Some("bold") },
    CommandHint { name: "italic",   args: "", description: "Italic",            insertion: "/italic",   tiptap_cmd: Some("italic") },
    CommandHint { name: "strike",   args: "", description: "Strikethrough",     insertion: "/strike",   tiptap_cmd: Some("strike") },
    CommandHint { name: "code",     args: "", description: "Inline code",       insertion: "/code",     tiptap_cmd: Some("code") },
    // ── Block elements ────────────────────────────────────────────────────────
    CommandHint { name: "codeblock",args: "", description: "Code block",        insertion: "/codeblock",tiptap_cmd: Some("codeblock") },
    CommandHint { name: "quote",    args: "", description: "Block quote",       insertion: "/quote",    tiptap_cmd: Some("quote") },
    CommandHint { name: "bullet",   args: "", description: "Bullet list",       insertion: "/bullet",   tiptap_cmd: Some("bullet") },
    CommandHint { name: "numbered", args: "", description: "Numbered list",     insertion: "/numbered", tiptap_cmd: Some("numbered") },
    CommandHint { name: "divider",  args: "", description: "Horizontal divider",insertion: "/divider",  tiptap_cmd: Some("divider") },
    // ── Data ──────────────────────────────────────────────────────────────────
    CommandHint { name: "table",    args: "", description: "Insert an interactive data table", insertion: "/table", tiptap_cmd: None },
    // ── Actions ───────────────────────────────────────────────────────────────
    CommandHint { name: "note",     args: "<title>", description: "Create a new note",           insertion: "/note ",    tiptap_cmd: None },
    CommandHint { name: "daily",   args: "",         description: "Open today's daily note",     insertion: "/daily",   tiptap_cmd: None },
    CommandHint { name: "extract",  args: "<url>",   description: "Fetch an article as a note",  insertion: "/extract ", tiptap_cmd: None },
    CommandHint { name: "explain",  args: "",         description: "Summarise the current note",  insertion: "/explain",  tiptap_cmd: None },
    CommandHint { name: "connect",  args: "",         description: "Find related notes",          insertion: "/connect",  tiptap_cmd: None },
    CommandHint { name: "help",     args: "",         description: "Show available commands",     insertion: "/help",     tiptap_cmd: None },
];

/// Return the TipTap format command string for a given insertion token, if any.
pub fn tiptap_cmd_for(insertion: &str) -> Option<&'static str> {
    COMMANDS
        .iter()
        .find(|c| c.insertion == insertion)
        .and_then(|c| c.tiptap_cmd)
}

/// Returns the slash query (text after `/`) if `text` is a slash-command-in-progress.
pub fn slash_query(text: &str) -> Option<&str> {
    let t = text.trim_start();
    let stripped = t.strip_prefix('/')?;
    if stripped.contains(char::is_whitespace) {
        return None;
    }
    Some(stripped)
}

pub fn matches(query: &str) -> Vec<CommandHint> {
    let q = query.to_ascii_lowercase();
    COMMANDS
        .iter()
        .copied()
        .filter(|c| c.name.starts_with(&q))
        .collect()
}

#[component]
pub fn SlashPalette(
    text: Signal<String>,
    selected: Signal<usize>,
    on_select: EventHandler<&'static str>,
) -> Element {
    let body = text.read().clone();
    let Some(q) = slash_query(&body) else {
        return rsx! { Fragment {} };
    };
    let matched = matches(q);
    if matched.is_empty() {
        return rsx! { Fragment {} };
    }
    let active = (*selected.read()).min(matched.len().saturating_sub(1));

    rsx! {
        div { class: "slash-palette",
            for (i, hint) in matched.iter().enumerate() {
                {
                    let h = *hint;
                    let class = if i == active { "slash-row active" } else { "slash-row" };
                    let badge = if h.tiptap_cmd.is_some() { "format" } else { "action" };
                    rsx! {
                        button {
                            class: "{class}",
                            onclick: move |_| on_select.call(h.insertion),
                            span { class: "slash-name", "/{h.name}" }
                            span { class: "slash-args", "{h.args}" }
                            span { class: "slash-desc", "{h.description}" }
                            span { class: "slash-badge slash-badge-{badge}" }
                        }
                    }
                }
            }
        }
    }
}
