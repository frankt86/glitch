use camino::Utf8PathBuf;
use glitch_ai::{SessionConfig, StreamEvent};
use glitch_core::{Note, NoteId, Vault};
use glitch_sync::SyncStatus;

#[derive(Debug, Clone, Default)]
pub struct AppState {
    pub vault: Option<Vault>,
    pub current_note: Option<NoteId>,
    pub editor_content: String,
    pub editor_dirty: bool,
}

impl AppState {
    pub fn current_note(&self) -> Option<&Note> {
        let id = self.current_note.as_ref()?;
        self.vault.as_ref()?.notes.iter().find(|n| n.id == *id)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatEntry {
    UserPrompt(String),
    LocalReply { command: String, body: String },
    Stream(StreamEvent),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeStatus {
    Unknown,
    Available,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    Idle,
    Starting,
    Ready,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum ChatCommand {
    StartSession {
        root: Utf8PathBuf,
        config: SessionConfig,
    },
    Send(String),
    Interrupt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncState {
    Unknown,
    NotARepo,
    Clean,
    Dirty(SyncStatus),
    Conflicts(SyncStatus),
    Syncing,
    Error(String),
}

impl Default for SyncState {
    fn default() -> Self {
        SyncState::Unknown
    }
}

impl SyncState {
    pub fn label(&self) -> &'static str {
        match self {
            SyncState::Unknown => "checking…",
            SyncState::NotARepo => "not a git repo",
            SyncState::Clean => "synced",
            SyncState::Dirty(_) => "uncommitted",
            SyncState::Conflicts(_) => "conflicts",
            SyncState::Syncing => "syncing…",
            SyncState::Error(_) => "sync error",
        }
    }
    pub fn css_class(&self) -> &'static str {
        match self {
            SyncState::Unknown => "badge badge-neutral",
            SyncState::NotARepo => "badge badge-warn",
            SyncState::Clean => "badge badge-ok",
            SyncState::Dirty(_) => "badge badge-warn",
            SyncState::Conflicts(_) | SyncState::Error(_) => "badge badge-error",
            SyncState::Syncing => "badge badge-warn",
        }
    }
}

#[derive(Debug, Clone)]
pub enum SyncCommand {
    CheckStatus,
    Sync,
}
