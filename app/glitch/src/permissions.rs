//! Named-pipe server hosted by the main Glitch app process. Receives
//! permission requests forwarded from the `--mcp-permission-server`
//! subprocesses Claude Code spawns, and routes them to the UI for
//! Allow/Deny decisions.

use camino::Utf8PathBuf;
use glitch_mcp::pipe::{ApprovalDecision, PipeMessage};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write as _;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::{mpsc, oneshot};

/// Plain-data summary of a pending approval — safe to drop into a Dioxus signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingApproval {
    pub id: String,
    pub tool_name: String,
    pub summary: String,
    pub input_pretty: String,
}

#[derive(Debug)]
pub enum PermissionEvent {
    New(PendingApproval),
    /// Sent when the connection drops before a decision was made (for UI cleanup).
    Cancelled(String),
}

pub struct PermissionHandle {
    pub pipe_name: String,
    pub events: mpsc::UnboundedReceiver<PermissionEvent>,
    pub resolver: mpsc::UnboundedSender<(String, ApprovalDecision)>,
}

/// Start the permission server. Returns a handle whose:
/// - `pipe_name` goes into the MCP config so Claude's subprocess knows where to dial
/// - `events` is consumed by a UI coroutine to populate the pending list
/// - `resolver` is the back-channel from UI button handlers to the pipe loop
pub fn start() -> std::io::Result<PermissionHandle> {
    let pipe_name = make_pipe_name();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<PermissionEvent>();
    let (resolver_tx, resolver_rx) = mpsc::unbounded_channel::<(String, ApprovalDecision)>();

    let waiters: Arc<Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>> =
        Arc::default();

    // Resolver task — routes UI decisions to the corresponding oneshot.
    {
        let waiters = waiters.clone();
        let event_tx = event_tx.clone();
        tokio::spawn(async move {
            let mut rx = resolver_rx;
            while let Some((id, decision)) = rx.recv().await {
                let entry = waiters.lock().unwrap().remove(&id);
                if let Some(tx) = entry {
                    let _ = tx.send(decision);
                }
                let _ = event_tx.send(PermissionEvent::Cancelled(id));
            }
        });
    }

    // Accept loop — keeps a server instance listening at all times.
    {
        let pipe_name = pipe_name.clone();
        let waiters = waiters.clone();
        let event_tx = event_tx.clone();
        tokio::spawn(async move {
            if let Err(err) = accept_loop(pipe_name, waiters, event_tx).await {
                tracing::error!("permission pipe accept loop exited: {err}");
            }
        });
    }

    Ok(PermissionHandle {
        pipe_name,
        events: event_rx,
        resolver: resolver_tx,
    })
}

fn make_pipe_name() -> String {
    let pid = std::process::id();
    let suffix = ulid::Ulid::new();
    format!(r"\\.\pipe\glitch-perm-{pid}-{suffix}")
}

async fn accept_loop(
    pipe_name: String,
    waiters: Arc<Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>>,
    event_tx: mpsc::UnboundedSender<PermissionEvent>,
) -> std::io::Result<()> {
    // Hold one waiting instance at all times; the moment a client connects,
    // hand off and create the next.
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_name)?;

    loop {
        server.connect().await?;
        let next = ServerOptions::new().create(&pipe_name)?;
        let connected = std::mem::replace(&mut server, next);

        let waiters = waiters.clone();
        let event_tx = event_tx.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_connection(connected, waiters, event_tx).await {
                tracing::warn!("permission connection ended: {err}");
            }
        });

        // Tiny breather to avoid hot-spinning if connect fails.
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

async fn handle_connection(
    pipe: NamedPipeServer,
    waiters: Arc<Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>>,
    event_tx: mpsc::UnboundedSender<PermissionEvent>,
) -> std::io::Result<()> {
    let (read_half, write_half) = tokio::io::split(pipe);
    let mut reader = BufReader::new(read_half);
    let writer = Arc::new(tokio::sync::Mutex::new(BufWriter::new(write_half)));

    let mut buf = String::new();
    loop {
        buf.clear();
        let n = reader.read_line(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        let trimmed = buf.trim();
        if trimmed.is_empty() {
            continue;
        }

        let msg: PipeMessage = match serde_json::from_str(trimmed) {
            Ok(m) => m,
            Err(err) => {
                tracing::warn!("malformed pipe message: {err}");
                continue;
            }
        };

        let PipeMessage::Request {
            id,
            tool_name,
            input,
        } = msg
        else {
            continue;
        };

        let (tx, rx) = oneshot::channel::<ApprovalDecision>();
        waiters.lock().unwrap().insert(id.clone(), tx);

        let summary = summarize_input(&tool_name, &input);
        let pretty = serde_json::to_string_pretty(&input).unwrap_or_default();
        let _ = event_tx.send(PermissionEvent::New(PendingApproval {
            id: id.clone(),
            tool_name: tool_name.clone(),
            summary,
            input_pretty: pretty,
        }));

        let writer = writer.clone();
        let event_tx = event_tx.clone();
        tokio::spawn(async move {
            let decision = rx
                .await
                .unwrap_or_else(|_| ApprovalDecision::deny("UI dropped before responding"));
            let response = PipeMessage::Response {
                id: id.clone(),
                decision,
            };
            if let Ok(serialized) = serde_json::to_string(&response) {
                let mut w = writer.lock().await;
                let _ = w.write_all(serialized.as_bytes()).await;
                let _ = w.write_all(b"\n").await;
                let _ = w.flush().await;
            }
            let _ = event_tx.send(PermissionEvent::Cancelled(id));
        });
    }
}

/// Best-effort short description for the modal heading.
fn summarize_input(tool_name: &str, input: &Value) -> String {
    match tool_name {
        "Read" | "Edit" | "Write" | "MultiEdit" => input
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| short_json(input)),
        "Bash" => input
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| short_json(input)),
        "Glob" => input
            .get("pattern")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| short_json(input)),
        "Grep" => input
            .get("pattern")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| short_json(input)),
        "WebFetch" => input
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| short_json(input)),
        _ => short_json(input),
    }
}

fn short_json(value: &Value) -> String {
    let s = serde_json::to_string(value).unwrap_or_default();
    if s.len() > 100 {
        format!("{}…", &s[..100])
    } else {
        s
    }
}

/// Path under `%APPDATA%\Glitch\mcp\<ulid>.json` (per the
/// agent-config-in-app rule). Created on demand.
pub fn write_mcp_config(pipe_name: &str) -> std::io::Result<Utf8PathBuf> {
    let app_dir = appdata_glitch_dir()?;
    let mcp_dir = app_dir.join("mcp");
    std::fs::create_dir_all(&mcp_dir)?;

    let exe = std::env::current_exe()?;
    let exe_str = exe
        .to_str()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "non-utf8 exe path"))?;

    let config = serde_json::json!({
        "mcpServers": {
            glitch_mcp::SERVER_NAME: {
                "command": exe_str,
                "args": ["--mcp-permission-server", pipe_name],
            }
        }
    });

    let path = mcp_dir.join(format!("session-{}.json", ulid::Ulid::new()));
    let mut file = std::fs::File::create(&path)?;
    file.write_all(serde_json::to_string_pretty(&config)?.as_bytes())?;
    Ok(Utf8PathBuf::from_path_buf(path).map_err(|p| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("non-utf8 path: {p:?}"))
    })?)
}

fn appdata_glitch_dir() -> std::io::Result<std::path::PathBuf> {
    let appdata = std::env::var_os("APPDATA").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "APPDATA env var missing")
    })?;
    let mut path = std::path::PathBuf::from(appdata);
    path.push("Glitch");
    Ok(path)
}
