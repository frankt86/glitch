use crate::events::{StreamEvent, UserInput};
use camino::Utf8Path;
use futures::StreamExt;
use std::process::Stdio;
use thiserror::Error;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio_util::codec::{FramedRead, LinesCodec};
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Error)]
pub enum ClaudeError {
    #[error("`claude` CLI not found on PATH; install Claude Code from https://docs.claude.com/claude-code")]
    NotInstalled,
    #[error("failed to spawn claude: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("session has been closed")]
    Closed,
}

#[derive(Debug, Clone)]
pub struct ClaudeClient {
    /// Override the binary name (defaults to "claude"). Useful for tests / Windows shims.
    pub binary: String,
}

impl Default for ClaudeClient {
    fn default() -> Self {
        Self {
            binary: "claude".into(),
        }
    }
}

/// Optional Claude Code session args. When `mcp_config` and `permission_prompt_tool`
/// are both set, tools NOT in `allowed_tools` will trigger the permission prompt
/// in the UI instead of being silently denied.
#[derive(Debug, Clone, Default)]
pub struct SessionConfig {
    /// Comma-separated list. Empty = use Claude's defaults.
    pub allowed_tools: Option<String>,
    pub disallowed_tools: Option<String>,
    /// Path to an `.mcp.json`-style config file.
    pub mcp_config: Option<String>,
    /// Tool name like `mcp__glitch_permissions__approve`.
    pub permission_prompt_tool: Option<String>,
    /// Text appended to Claude's system prompt (agent instructions).
    pub system_prompt_append: Option<String>,
}

impl ClaudeClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Quick check: is `claude --version` runnable? Used at startup to decide
    /// whether to show the setup screen.
    pub async fn is_available(&self) -> bool {
        let mut cmd = Command::new(&self.binary);
        cmd.arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Start a long-lived `claude -p` session in stream-json mode anchored at
    /// `working_dir`. The session keeps the process alive across multiple
    /// prompts so the agent loop can run uninterrupted.
    pub fn start_session(
        &self,
        working_dir: &Utf8Path,
        config: &SessionConfig,
    ) -> Result<SessionHandle, ClaudeError> {
        let mut cmd = Command::new(&self.binary);
        cmd.args([
            "-p",
            "--output-format",
            "stream-json",
            "--input-format",
            "stream-json",
            "--verbose",
        ]);

        // Always pre-approve the safe read-only toolset so silent ops don't
        // pop a modal for every Glob/Read.
        let allowed = config
            .allowed_tools
            .clone()
            .unwrap_or_else(|| "Read,Glob,Grep,LS,TodoWrite".into());
        cmd.args(["--allowed-tools", &allowed]);

        if let Some(disallowed) = &config.disallowed_tools {
            cmd.args(["--disallowed-tools", disallowed]);
        }
        if let Some(mcp_config) = &config.mcp_config {
            cmd.args(["--mcp-config", mcp_config]);
        }
        if let Some(tool) = &config.permission_prompt_tool {
            cmd.args(["--permission-prompt-tool", tool]);
        }
        if let Some(sp) = &config.system_prompt_append {
            cmd.args(["--append-system-prompt", sp]);
        }

        cmd.current_dir(working_dir.as_std_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let mut child = cmd.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ClaudeError::NotInstalled,
            _ => ClaudeError::Spawn(e),
        })?;

        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        let (prompt_tx, prompt_rx) = mpsc::channel::<String>(16);
        let (event_tx, event_rx) = mpsc::channel::<StreamEvent>(128);

        // Writer: receives prompts from UI, writes JSON lines to stdin.
        tokio::spawn(write_loop(stdin, prompt_rx));

        // Reader: parses stdout NDJSON into StreamEvents.
        tokio::spawn(read_loop(stdout, event_tx));

        // Stderr drain → tracing.
        tokio::spawn(stderr_loop(stderr));

        let session = Session::new(child);

        Ok(SessionHandle {
            session,
            prompt_tx,
            event_rx,
        })
    }
}

/// Owns the spawned child process. Killing the session kills the process.
#[derive(Debug)]
pub struct Session {
    child: Child,
}

impl Session {
    fn new(child: Child) -> Self {
        Self { child }
    }

    pub async fn kill(&mut self) -> std::io::Result<()> {
        self.child.start_kill()?;
        let _ = self.child.wait().await;
        Ok(())
    }
}

/// User-facing handle: send prompts, receive events.
#[derive(Debug)]
pub struct SessionHandle {
    pub session: Session,
    prompt_tx: mpsc::Sender<String>,
    event_rx: mpsc::Receiver<StreamEvent>,
}

impl SessionHandle {
    /// Queue a user prompt. Backpressure: bounded channel of 16.
    pub async fn send(&self, prompt: impl Into<String>) -> Result<(), ClaudeError> {
        self.prompt_tx
            .send(prompt.into())
            .await
            .map_err(|_| ClaudeError::Closed)
    }

    /// Receive the next stream event. Returns None once the session ends.
    pub async fn recv(&mut self) -> Option<StreamEvent> {
        self.event_rx.recv().await
    }
}

async fn write_loop(stdin: tokio::process::ChildStdin, mut prompts: mpsc::Receiver<String>) {
    let mut writer = BufWriter::new(stdin);
    while let Some(prompt) = prompts.recv().await {
        let payload = match serde_json::to_string(&UserInput::user(&prompt)) {
            Ok(s) => s,
            Err(err) => {
                tracing::error!("failed to serialize user input: {err}");
                continue;
            }
        };
        if let Err(err) = writer.write_all(payload.as_bytes()).await {
            tracing::warn!("claude stdin closed: {err}");
            return;
        }
        if let Err(err) = writer.write_all(b"\n").await {
            tracing::warn!("claude stdin closed: {err}");
            return;
        }
        if let Err(err) = writer.flush().await {
            tracing::warn!("claude stdin flush failed: {err}");
            return;
        }
    }
    // Channel closed → drop stdin → claude sees EOF and exits.
    let _ = writer.shutdown().await;
}

async fn read_loop(stdout: tokio::process::ChildStdout, events: mpsc::Sender<StreamEvent>) {
    let mut frames = FramedRead::new(stdout, LinesCodec::new());
    while let Some(line) = frames.next().await {
        let line = match line {
            Ok(l) => l,
            Err(err) => {
                tracing::warn!("stream-json line error: {err}");
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let event: StreamEvent = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!(line = %line, "failed to parse stream-json line: {err}");
                StreamEvent::Unknown
            }
        };
        if events.send(event).await.is_err() {
            return;
        }
    }
}

async fn stderr_loop(stderr: tokio::process::ChildStderr) {
    let mut frames = FramedRead::new(stderr, LinesCodec::new());
    while let Some(line) = frames.next().await {
        match line {
            Ok(l) if !l.trim().is_empty() => tracing::debug!(target: "claude.stderr", "{l}"),
            Ok(_) => {}
            Err(err) => tracing::warn!("claude stderr error: {err}"),
        }
    }
}
