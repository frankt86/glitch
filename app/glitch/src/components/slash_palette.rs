//! Floating autocomplete dropdown for slash commands. Triggered by `/` at the
//! start of an input. Matching commands filter as the user types.

use dioxus::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandHint {
    pub name: &'static str,
    pub args: &'static str,
    pub description: &'static str,
    /// What to insert into the input when chosen — usually `/name ` with a
    /// trailing space for arguments, or `/name` for argless commands.
    pub insertion: &'static str,
}

pub const COMMANDS: &[CommandHint] = &[
    CommandHint {
        name: "note",
        args: "<title>",
        description: "ask Claude to create a new note in the vault root",
        insertion: "/note ",
    },
    CommandHint {
        name: "extract",
        args: "<url>",
        description: "fetch an article and save as a note (runs in Glitch, no AI)",
        insertion: "/extract ",
    },
    CommandHint {
        name: "explain",
        args: "",
        description: "summarise the currently open note",
        insertion: "/explain",
    },
    CommandHint {
        name: "connect",
        args: "",
        description: "find related notes (placeholder)",
        insertion: "/connect",
    },
    CommandHint {
        name: "help",
        args: "",
        description: "show available commands",
        insertion: "/help",
    },
];

/// Returns the slash query (text after `/`) if `text` is a slash-command-in-progress.
/// `None` means: not a slash command, hide the palette.
pub fn slash_query(text: &str) -> Option<&str> {
    let t = text.trim_start();
    let stripped = t.strip_prefix('/')?;
    // If the user has already typed past the command name (a space followed by
    // arguments), hide the palette — they're typing args, not picking a command.
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
                    rsx! {
                        button {
                            class: "{class}",
                            onclick: move |_| on_select.call(h.insertion),
                            span { class: "slash-name", "/{h.name}" }
                            span { class: "slash-args", "{h.args}" }
                            span { class: "slash-desc", "{h.description}" }
                        }
                    }
                }
            }
        }
    }
}
