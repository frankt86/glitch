//! User-editable settings persisted to `%APPDATA%\Glitch\settings.json`.
//! Lives outside the vault per the agent-config-not-in-vault rule.

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;

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
- Use `[[wikilinks]]` to reference other notes by their relative path.

## Your tools

- Read/Glob/Grep/LS are pre-approved.
- Write/Edit/MultiEdit prompt the user via Glitch's permission modal.
- Bash/WebFetch/WebSearch require explicit approval each time.
"#;
    std::fs::write(path.as_std_path(), starter)
}
