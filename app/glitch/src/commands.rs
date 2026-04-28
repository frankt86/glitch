//! Slash commands. Parsed from chat input and dispatched either as local
//! replies (e.g. `/help`) or as templated prompts to the active Claude session.
//!
//! Per the project rule, command definitions and instructions live in the
//! application — never in the synced vault.

use camino::Utf8PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum SlashCommand {
    Help,
    NewNote { title: String, note_type: Option<String> },
    Explain,
    Extract { url: String },
    Connect,
}

#[derive(Debug, Clone)]
pub struct CommandContext {
    pub vault_root: Option<Utf8PathBuf>,
    pub current_note_relative: Option<String>,
    pub current_note_content: Option<String>,
}

#[derive(Debug, Clone)]
pub enum CommandOutcome {
    LocalReply(String),
    Prompt(String),
    Error(String),
    /// Fetch and save an article into the vault.
    Extract {
        url: String,
        vault_root: camino::Utf8PathBuf,
    },
    /// Create a note from a local template (no AI involved).
    LocalCreate {
        title: String,
        note_type: String,
        vault_root: camino::Utf8PathBuf,
    },
}

impl SlashCommand {
    /// `input` is everything after the leading `/`. Returns `Err` with a
    /// user-facing message on bad usage.
    pub fn parse(input: &str) -> Result<Self, String> {
        let trimmed = input.trim();
        let (name, args) = trimmed
            .split_once(char::is_whitespace)
            .map(|(n, a)| (n, a.trim()))
            .unwrap_or((trimmed, ""));

        match name {
            "" => Err("usage: /<command>. try /help".into()),
            "help" | "h" | "?" => Ok(SlashCommand::Help),
            "note" | "n" | "new" => {
                if args.is_empty() {
                    Err("usage: /note <title> [--type <type>]".into())
                } else {
                    let (title, note_type) = parse_note_args(args);
                    if title.is_empty() {
                        Err("usage: /note <title> [--type <type>]".into())
                    } else {
                        Ok(SlashCommand::NewNote { title, note_type })
                    }
                }
            }
            "explain" | "e" => Ok(SlashCommand::Explain),
            "extract" | "fetch" => {
                if args.is_empty() {
                    Err("usage: /extract <url>".into())
                } else {
                    Ok(SlashCommand::Extract {
                        url: args.to_string(),
                    })
                }
            }
            "connect" | "related" => Ok(SlashCommand::Connect),
            other => Err(format!(
                "unknown command: /{other}. try /help for the list"
            )),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            SlashCommand::Help => "help",
            SlashCommand::NewNote { .. } => "note",
            SlashCommand::Explain => "explain",
            SlashCommand::Extract { .. } => "extract",
            SlashCommand::Connect => "connect",
        }
    }

    pub fn dispatch(self, ctx: &CommandContext) -> CommandOutcome {
        match self {
            SlashCommand::Help => CommandOutcome::LocalReply(HELP_TEXT.into()),
            SlashCommand::NewNote { title, note_type } => {
                let Some(root) = ctx.vault_root.as_ref() else {
                    return CommandOutcome::Error("open a vault first".into());
                };
                if let Some(t) = note_type {
                    // Local template path — no AI involved.
                    CommandOutcome::LocalCreate {
                        title,
                        note_type: t,
                        vault_root: root.clone(),
                    }
                } else {
                    // No type specified: let Claude create the note with its own content.
                    CommandOutcome::Prompt(new_note_prompt(root, &title))
                }
            }
            SlashCommand::Explain => match (&ctx.current_note_relative, &ctx.current_note_content) {
                (Some(path), Some(body)) => CommandOutcome::Prompt(explain_prompt(path, body)),
                _ => CommandOutcome::Error("open a note first".into()),
            },
            SlashCommand::Extract { url } => {
                let Some(root) = ctx.vault_root.as_ref() else {
                    return CommandOutcome::Error("open a vault first".into());
                };
                if !(url.starts_with("http://") || url.starts_with("https://")) {
                    return CommandOutcome::Error(format!(
                        "invalid url: {url} — must start with http:// or https://"
                    ));
                }
                CommandOutcome::Extract {
                    url,
                    vault_root: root.clone(),
                }
            }
            SlashCommand::Connect => match (&ctx.current_note_relative, &ctx.current_note_content) {
                (Some(path), Some(body)) => CommandOutcome::Prompt(connect_prompt(path, body)),
                _ => CommandOutcome::Error("open a note first".into()),
            },
        }
    }
}

/// Parse `/note <title> [--type <name>]` args string into `(title, note_type)`.
fn parse_note_args(args: &str) -> (String, Option<String>) {
    // Look for --type flag anywhere in the string.
    if let Some(pos) = args.find("--type") {
        let before = args[..pos].trim().to_string();
        let after = args[pos + 6..].trim(); // skip "--type"
        let type_name = after
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let note_type = if type_name.is_empty() { None } else { Some(type_name) };
        (before, note_type)
    } else {
        (args.trim().to_string(), None)
    }
}

/// Try to parse `input` as a slash command. `None` means free-form chat.
pub fn try_parse(input: &str) -> Option<Result<SlashCommand, String>> {
    let trimmed = input.trim_start();
    let stripped = trimmed.strip_prefix('/')?;
    Some(SlashCommand::parse(stripped))
}

const HELP_TEXT: &str = "Slash commands:
  /note <title>               ask Claude to create a new note in the vault root
  /note <title> --type <type> create a note from a local template (no AI)
                              types: meeting, book, person, project (or custom)
  /extract <url>              fetch an article and save it as a note (no AI)
  /explain                    summarise the currently open note
  /connect                    find related notes for the current note
  /help                       this message

Anything not starting with `/` is sent to Claude as free-form chat.";

fn new_note_prompt(vault_root: &Utf8PathBuf, title: &str) -> String {
    let slug = slugify(title);
    let path = vault_root.join(format!("{slug}.md"));
    format!(
        "Create a new note titled \"{title}\".

Use the Write tool to create a single new markdown file at exactly this path:
{path}

Format:
- YAML frontmatter with `title`, `created` (today's ISO date), and an empty `tags: []` list.
- An H1 with the title.
- 3-6 short paragraphs of useful starter content for someone exploring this topic.
- Do not invent facts; if you are unsure, ask clarifying questions in the body as a TODO list.

After writing the file, reply with the path of the created note and a 1-line summary. Do not write any other files.",
    )
}

fn explain_prompt(note_path: &str, note_body: &str) -> String {
    format!(
        "Explain the following note in clear, plain language. Highlight the key claims, any open questions, and how it might connect to other notes in the vault.

Note path: {note_path}

--- BEGIN NOTE ---
{note_body}
--- END NOTE ---

Reply with:
1. A 2-3 sentence summary.
2. Up to 5 bullet points of key claims or facts.
3. Any TODOs or unanswered questions you spot.

Do not edit any files.",
    )
}


fn connect_prompt(note_path: &str, note_body: &str) -> String {
    format!(
        "Find notes that might be related to the following note. Use the Glob and Read tools to scan the vault, then list up to 5 candidates with a 1-line reason each.

Note path: {note_path}

--- BEGIN NOTE ---
{note_body}
--- END NOTE ---

Reply only with the candidate list. Do not edit any files.",
    )
}

const MAX_SLUG_CHARS: usize = 60;

/// Filesystem-safe slug capped at [`MAX_SLUG_CHARS`]. Stops at the first colon
/// or URL-ish boundary so a stray "https://…" doesn't become the filename.
fn slugify(s: &str) -> String {
    // Truncate at first ':' (titles like "topic: subtopic" keep "topic") or
    // an http(s):// boundary.
    let head = s
        .split_once(':')
        .map(|(h, _)| h)
        .unwrap_or(s)
        .split_whitespace()
        // Stop at the first word that looks URL-ish ("http", "https", "www.")
        // — keeps long titles with embedded URLs from polluting filenames.
        .take_while(|w| {
            let lw = w.to_ascii_lowercase();
            !(lw.starts_with("http") || lw.starts_with("www."))
        })
        .collect::<Vec<_>>()
        .join(" ");
    let head = if head.trim().is_empty() { s } else { &head };

    let mut out = String::with_capacity(head.len().min(MAX_SLUG_CHARS));
    let mut last_dash = false;
    for ch in head.chars() {
        if out.len() >= MAX_SLUG_CHARS {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "untitled".into()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_help_aliases() {
        assert_eq!(SlashCommand::parse("help").unwrap(), SlashCommand::Help);
        assert_eq!(SlashCommand::parse("h").unwrap(), SlashCommand::Help);
        assert_eq!(SlashCommand::parse("?").unwrap(), SlashCommand::Help);
    }

    #[test]
    fn parses_note_with_title() {
        assert_eq!(
            SlashCommand::parse("note On Glitches").unwrap(),
            SlashCommand::NewNote { title: "On Glitches".into(), note_type: None }
        );
        assert_eq!(
            SlashCommand::parse("n   spaced  title").unwrap(),
            SlashCommand::NewNote { title: "spaced  title".into(), note_type: None }
        );
    }

    #[test]
    fn parses_note_with_type() {
        assert_eq!(
            SlashCommand::parse("note My Meeting --type meeting").unwrap(),
            SlashCommand::NewNote { title: "My Meeting".into(), note_type: Some("meeting".into()) }
        );
        assert_eq!(
            SlashCommand::parse("note Reading Notes --type Book").unwrap(),
            SlashCommand::NewNote { title: "Reading Notes".into(), note_type: Some("book".into()) }
        );
    }

    #[test]
    fn note_without_args_is_error() {
        assert!(SlashCommand::parse("note").is_err());
        assert!(SlashCommand::parse("note   ").is_err());
    }

    #[test]
    fn try_parse_distinguishes_freeform() {
        assert!(try_parse("hello world").is_none());
        assert!(try_parse("  /help").is_some());
    }

    #[test]
    fn slug_is_filesystem_safe() {
        assert_eq!(slugify("On Glitches & Such"), "on-glitches-such");
        assert_eq!(slugify("  ?? hi !!  "), "hi");
    }

    #[test]
    fn slug_truncates_long_input() {
        let huge = "this is a really really really really really really really really really long title";
        let s = slugify(huge);
        assert!(s.len() <= MAX_SLUG_CHARS, "got len {} ({s:?})", s.len());
        assert!(s.starts_with("this-is-a-really"));
    }

    #[test]
    fn slug_drops_trailing_url() {
        let bad = "turn this site into a note: https://pycon.blogspot.com/2026/04/asking";
        let s = slugify(bad);
        assert_eq!(s, "turn-this-site-into-a-note");
    }

    #[test]
    fn slug_drops_inline_url() {
        let bad = "save https://example.com/article-here for later";
        let s = slugify(bad);
        assert_eq!(s, "save");
    }

    #[test]
    fn slug_falls_back_for_non_alphanumeric() {
        assert_eq!(slugify("???"), "untitled");
    }

    #[test]
    fn unknown_command_reports_useful_error() {
        let err = SlashCommand::parse("nonsense").unwrap_err();
        assert!(err.contains("/nonsense"));
        assert!(err.contains("/help"));
    }
}
