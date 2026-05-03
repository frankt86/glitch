//! User-editable settings persisted to `%APPDATA%\Glitch\settings.json`.
//! Lives outside the vault per the agent-config-not-in-vault rule.

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;

// ─── Note types ─────────────────────────────────────────────────────────────

/// A single field shown in the Detail tab for a note type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDef {
    /// Human-readable column header.
    pub label: String,
    /// YAML frontmatter key to read/write.
    pub key: String,
    /// Input hint: "text" | "url" | "textarea" | "tags" | "date"
    #[serde(default = "default_hint")]
    pub hint: String,
}

fn default_hint() -> String {
    "text".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteTypeConfig {
    pub name: String,
    #[serde(default = "default_type_emoji")]
    pub emoji: String,
    /// Filename of the template inside `%APPDATA%\Glitch\templates\`.
    /// If empty or the file doesn't exist, the built-in blank template is used.
    #[serde(default)]
    pub template: String,
    /// Custom Detail tab fields. If empty, the built-in defaults are used.
    #[serde(default)]
    pub fields: Vec<FieldDef>,
}

fn default_type_emoji() -> String {
    "📄".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TypesFile {
    #[serde(rename = "type", default)]
    types: Vec<NoteTypeConfig>,
}

pub fn types_config_path() -> io::Result<PathBuf> {
    let mut p = appdata_glitch_dir()?;
    p.push("types.toml");
    Ok(p)
}

pub fn templates_dir() -> io::Result<PathBuf> {
    let mut p = appdata_glitch_dir()?;
    p.push("templates");
    Ok(p)
}

/// Load note types from `%APPDATA%\Glitch\types.toml`.
/// Returns a default set if the file doesn't exist or can't be parsed.
pub fn load_types() -> Vec<NoteTypeConfig> {
    let path = match types_config_path() {
        Ok(p) => p,
        Err(_) => return default_note_types(),
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return default_note_types(),
    };
    toml::from_str::<TypesFile>(&contents)
        .map(|f| f.types)
        .unwrap_or_else(|_| default_note_types())
}

fn default_note_types() -> Vec<NoteTypeConfig> {
    let mk = |name: &str, emoji: &str, tmpl: &str| NoteTypeConfig {
        name: name.into(), emoji: emoji.into(), template: tmpl.into(), fields: vec![],
    };
    vec![
        mk("note",     "📝",  "note.md"),
        mk("task",     "✅",  "task.md"),
        mk("meeting",  "🗓",  "meeting.md"),
        mk("book",     "📚",  "book.md"),
        mk("person",   "👤",  "person.md"),
        mk("project",  "🚀",  "project.md"),
        mk("bible",    "📖",  "bible.md"),
        mk("sermon",   "🎙️", "sermon.md"),
        mk("prayer",   "🙏",  "prayer.md"),
        mk("journal",  "📓",  "journal.md"),
        mk("recipe",   "🍳",  "recipe.md"),
        mk("research", "🔬",  "research.md"),
        mk("goal",     "🎯",  "goal.md"),
        mk("quote",    "💬",  "quote.md"),
    ]
}

/// Render a template for a given type. Replaces `{{title}}`, `{{date}}`,
/// `{{slug}}` placeholders. Falls back to the blank stub if no template file.
pub fn render_template(note_type: &str, title: &str) -> String {
    let slug = slugify_title(title);
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();

    // Try to read from the templates directory first.
    if let Ok(tmpl_dir) = templates_dir() {
        let types = load_types();
        if let Some(cfg) = types.iter().find(|t| t.name.eq_ignore_ascii_case(note_type)) {
            if !cfg.template.is_empty() {
                let tmpl_path = tmpl_dir.join(&cfg.template);
                if let Ok(raw) = std::fs::read_to_string(&tmpl_path) {
                    return raw
                        .replace("{{title}}", title)
                        .replace("{{date}}", &today)
                        .replace("{{slug}}", &slug)
                        .replace("{{type}}", note_type);
                }
            }
        }
    }

    // Fallback: minimal frontmatter stub with the requested type.
    format!(
        "---\ntitle: \"{title}\"\ntype: {note_type}\ncreated: {today}\ntags: []\n---\n\n# {title}\n\n"
    )
}

fn slugify_title(s: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Seed `%APPDATA%\Glitch\types.toml` and default template files if they don't exist.
pub fn ensure_default_types() -> io::Result<()> {
    let config_path = types_config_path()?;
    let tmpl_dir = templates_dir()?;
    std::fs::create_dir_all(&tmpl_dir)?;

    if !config_path.exists() {
        let toml_content = r#"# Glitch note types
# Each [[type]] entry registers a note type with an emoji and template file.
# Templates live in %APPDATA%\Glitch\templates\ and support placeholders:
#   {{title}}, {{date}}, {{slug}}, {{type}}
#
# Add optional [[type.fields]] to customise the Detail tab for that type.
# hint values: "text" | "url" | "textarea" | "tags" | "date"
# Example:
#   [[type.fields]]
#   label = "Date"
#   key = "date"
#   hint = "text"

[[type]]
name = "meeting"
emoji = "🗓"
template = "meeting.md"

[[type.fields]]
label = "Date"
key = "date"
hint = "text"

[[type.fields]]
label = "Attendees"
key = "attendees"
hint = "text"

[[type.fields]]
label = "Tags"
key = "tags"
hint = "tags"

[[type]]
name = "book"
emoji = "📚"
template = "book.md"

[[type]]
name = "person"
emoji = "👤"
template = "person.md"

[[type]]
name = "project"
emoji = "🚀"
template = "project.md"
"#;
        std::fs::write(&config_path, toml_content)?;
    }

    // Append any new types that don't yet appear in the existing file.
    // This lets existing installs pick up new types without overwriting customisations.
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    let new_types = [
        ("note",     "📝",  "note.md"),
        ("task",     "✅",  "task.md"),
        ("bible",    "📖",  "bible.md"),
        ("sermon",   "🎙️", "sermon.md"),
        ("prayer",   "🙏",  "prayer.md"),
        ("journal",  "📓",  "journal.md"),
        ("recipe",   "🍳",  "recipe.md"),
        ("research", "🔬",  "research.md"),
        ("goal",     "🎯",  "goal.md"),
        ("quote",    "💬",  "quote.md"),
    ];
    let mut appended = existing.clone();
    for (name, emoji, tmpl) in &new_types {
        if !existing.contains(&format!("name = \"{name}\"")) {
            appended.push_str(&format!(
                "\n[[type]]\nname = \"{name}\"\nemoji = \"{emoji}\"\ntemplate = \"{tmpl}\"\n"
            ));
        }
    }
    if appended != existing {
        std::fs::write(&config_path, appended)?;
    }

    let templates: &[(&str, &str)] = &[
        ("note.md",    "---\ntitle: \"{{title}}\"\ntype: note\ncreated: {{date}}\ntags: []\n---\n\n# {{title}}\n\n"),
        ("task.md",    "---\ntitle: \"{{title}}\"\ntype: task\ncreated: {{date}}\ndue: \nstatus: open\npriority: medium\ntags: []\n---\n\n# {{title}}\n\n**Status:** open &nbsp; **Due:** &nbsp; **Priority:** medium\n\n## Description\n\n## Steps\n\n- [ ] \n\n## Notes\n\n"),
        ("meeting.md", "---\ntitle: \"{{title}}\"\ntype: meeting\ncreated: {{date}}\ntags: []\n---\n\n# {{title}}\n\n**Date:** {{date}}  \n**Attendees:**  \n\n## Agenda\n\n## Notes\n\n## Action items\n\n- [ ] \n"),
        ("book.md",    "---\ntitle: \"{{title}}\"\ntype: book\ncreated: {{date}}\ntags: []\n---\n\n# {{title}}\n\n**Author:**  \n**Started:** {{date}}  \n**Finished:**  \n\n## Summary\n\n## Key ideas\n\n## Quotes\n\n## My take\n\n"),
        ("person.md",  "---\ntitle: \"{{title}}\"\ntype: person\ncreated: {{date}}\ntags: []\n---\n\n# {{title}}\n\n**Role:**  \n**Contact:**  \n\n## Notes\n\n## Meetings\n\n"),
        ("project.md", "---\ntitle: \"{{title}}\"\ntype: project\ncreated: {{date}}\ntags: []\n---\n\n# {{title}}\n\n**Status:** active  \n**Started:** {{date}}  \n\n## Goal\n\n## Tasks\n\n- [ ] \n\n## Notes\n\n"),
        ("bible.md",   "---\ntitle: \"{{title}}\"\ntype: bible\nbook: \nchapter: \nverses: \ndate: {{date}}\ntags: []\n---\n\n# {{title}}\n\n**Book:** &nbsp; **Chapter:** &nbsp; **Verses:**\n\n## Scripture\n\n> (Paste the verse(s) here)\n\n## Observation\n*What does the text say? (facts, repeated words, who/what/when/where)*\n\n## Application\n*How does this apply to my life today?*\n\n## Prayer\n\n"),
        ("sermon.md",  "---\ntitle: \"{{title}}\"\ntype: sermon\nspeaker: \nseries: \nscripture: \ndate: {{date}}\ntags: []\n---\n\n# {{title}}\n\n**Speaker:** &nbsp; **Date:** {{date}}  \n**Series:** &nbsp; **Scripture:**\n\n## Main Point\n\n## Outline\n\n1. \n2. \n3. \n\n## Key Quotes\n\n## Application\n\n- [ ] \n\n## Notes\n\n"),
        ("prayer.md",  "---\ntitle: \"{{title}}\"\ntype: prayer\ndate: {{date}}\nstatus: active\ntags: []\n---\n\n# {{title}}\n\n**Date:** {{date}} &nbsp; **Status:** active\n\n## Request\n*What are you bringing before God?*\n\n## Scripture\n*A verse to stand on:*\n\n## How God Answered\n\n**Date answered:**\n\n## Praise & Thanksgiving\n\n"),
        ("journal.md", "---\ntitle: \"{{title}}\"\ntype: journal\ndate: {{date}}\nmood: \ntags: []\n---\n\n# {{title}}\n\n**Date:** {{date}} &nbsp; **Mood:**\n\n## Today's Highlight\n\n## Gratitude\n\n1. \n2. \n3. \n\n## Reflection\n*What's on my mind?*\n\n## Tomorrow's Focus\n\n- [ ] \n\n"),
        ("recipe.md",  "---\ntitle: \"{{title}}\"\ntype: recipe\nservings: \nprep_time: \ncook_time: \nrating: \ndate: {{date}}\ntags: []\n---\n\n# {{title}}\n\n**Servings:** &nbsp; **Prep:** &nbsp; **Cook:** &nbsp; **Rating:** /5\n\n## Ingredients\n\n- \n\n## Instructions\n\n1. \n\n## Notes & Variations\n\n## Source\n\n"),
        ("research.md","---\ntitle: \"{{title}}\"\ntype: research\ndate: {{date}}\nstatus: in-progress\ntags: []\n---\n\n# {{title}}\n\n**Status:** in-progress &nbsp; **Started:** {{date}}\n\n## Question / Hypothesis\n\n## Sources\n\n- \n\n## Key Findings\n\n## Summary\n\n## Open Questions\n\n- \n\n"),
        ("goal.md",    "---\ntitle: \"{{title}}\"\ntype: goal\ndate: {{date}}\ntarget_date: \nstatus: active\ntags: []\n---\n\n# {{title}}\n\n**Status:** active &nbsp; **Started:** {{date}} &nbsp; **Target:**\n\n## Why This Matters\n\n## Success Criteria\n\n- [ ] \n\n## Milestones\n\n- [ ] \n\n## Progress Log\n\n"),
        ("quote.md",   "---\ntitle: \"{{title}}\"\ntype: quote\nauthor: \nsource: \ndate: {{date}}\ntags: []\n---\n\n# {{title}}\n\n> (The quote)\n\n— **Author:** &nbsp; **Source:**\n\n## Context\n\n## Why It Resonates\n\n## Related Notes\n\n"),
    ];
    for (filename, content) in templates {
        let path = tmpl_dir.join(filename);
        if !path.exists() {
            std::fs::write(&path, content)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    /// Path to the `claude` CLI. Use just `"claude"` to resolve from PATH.
    #[serde(default = "default_claude_binary")]
    pub claude_binary: String,
    /// Tools that auto-approve without showing a modal. Comma-separated.
    /// Anything not on this list triggers the permission prompt.
    #[serde(default = "default_allowed_tools")]
    pub allowed_tools_silent: String,
    /// Auto-sync the vault to GitHub on a timer (off by default).
    #[serde(default)]
    pub auto_sync: bool,
    /// Minutes between auto-sync attempts.
    #[serde(default = "default_sync_interval")]
    pub auto_sync_interval_minutes: u32,
    /// If true, commit `.glitch/chats/` and `.glitch/embeddings.bin` to git.
    /// Off by default — these are machine-specific.
    #[serde(default)]
    pub commit_chats_to_git: bool,
    #[serde(default)]
    pub commit_embeddings_to_git: bool,
    /// Where the user-editable agent instructions live (system prompt overrides
    /// for Claude). NOT in the vault — this is agent config.
    #[serde(default = "default_agent_instructions")]
    pub agent_instructions_path: Utf8PathBuf,
    /// Last vault opened — auto-reopened on next launch.
    #[serde(default)]
    pub last_vault: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            claude_binary: default_claude_binary(),
            allowed_tools_silent: default_allowed_tools(),
            auto_sync: false,
            auto_sync_interval_minutes: default_sync_interval(),
            commit_chats_to_git: false,
            commit_embeddings_to_git: false,
            agent_instructions_path: default_agent_instructions(),
            last_vault: None,
        }
    }
}

fn default_claude_binary() -> String {
    "claude".into()
}
fn default_allowed_tools() -> String {
    "Read,Glob,Grep,LS,TodoWrite".into()
}
fn default_sync_interval() -> u32 {
    15
}
fn default_agent_instructions() -> Utf8PathBuf {
    appdata_glitch_dir()
        .map(|p| Utf8PathBuf::from_path_buf(p.join("agent.md")).unwrap_or_default())
        .unwrap_or_else(|_| Utf8PathBuf::from("agent.md"))
}

pub fn appdata_glitch_dir() -> io::Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "APPDATA env var missing")
    })?;
    let mut path = PathBuf::from(appdata);
    path.push("Glitch");
    Ok(path)
}

pub fn settings_path() -> io::Result<PathBuf> {
    let mut p = appdata_glitch_dir()?;
    p.push("settings.json");
    Ok(p)
}

pub fn load() -> AppSettings {
    let Ok(path) = settings_path() else {
        return AppSettings::default();
    };
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return AppSettings::default();
    };
    serde_json::from_str(&contents).unwrap_or_default()
}

pub fn save(settings: &AppSettings) -> io::Result<()> {
    let path = settings_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(settings)?;
    std::fs::write(&path, json)?;
    Ok(())
}

/// Persist `last_vault` without clobbering other settings.
pub fn save_last_vault(path: &str) {
    let mut settings = load();
    settings.last_vault = Some(path.to_string());
    if let Err(err) = save(&settings) {
        tracing::warn!("failed to save last_vault: {err}");
    }
}

/// Ensure the agent instructions file exists with a default starter template.
/// Returns the path it lives at. Safe to call repeatedly (no clobber).
pub fn ensure_agent_instructions(path: &Utf8PathBuf) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent.as_std_path())?;
    }
    let starter = r#"# Glitch agent instructions

These instructions are prepended to every Claude session in Glitch. They live
in `%APPDATA%\Glitch\agent.md`, not in your vault, so they don't sync to
GitHub or leak between machines.

## Conventions

- New notes go in the vault root unless the user specifies a folder.
- Frontmatter on every note: `title`, `created`, `tags`.
- Tags MUST use inline YAML array format: `tags: [tag1, tag2, tag3]`
  Never use the YAML block list format with dashes.
- Use `[[wikilinks]]` to reference other notes by their relative path.

## Your tools

- Read/Glob/Grep/LS are pre-approved.
- Write/Edit/MultiEdit prompt the user via Glitch's permission modal.
- Bash/WebFetch/WebSearch require explicit approval each time.
"#;
    std::fs::write(path.as_std_path(), starter)
}
