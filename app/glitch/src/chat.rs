use crate::state::{ChatCommand, ChatEntry, SessionStatus};
use camino::Utf8PathBuf;
use dioxus::prelude::*;
use futures::StreamExt;
use glitch_ai::{ClaudeClient, SessionHandle};

/// The chat coroutine: owns the long-lived `claude` subprocess across the
/// lifetime of the app, accepts commands from the UI, and pushes stream
/// events into the chat history signal.
pub async fn chat_coroutine(
    mut commands: UnboundedReceiver<ChatCommand>,
    mut history: Signal<Vec<ChatEntry>>,
    mut status: Signal<SessionStatus>,
) {
    let client = ClaudeClient::new();
    let mut session: Option<SessionHandle> = None;

    loop {
        tokio::select! {
            biased;
            cmd = commands.next() => {
                let Some(cmd) = cmd else { break };
                match cmd {
                    ChatCommand::StartSession { root, config } => {
                        if let Some(mut s) = session.take() {
                            let _ = s.session.kill().await;
                        }
                        status.set(SessionStatus::Starting);
                        match client.start_session(&root, &config) {
                            Ok(s) => {
                                session = Some(s);
                                status.set(SessionStatus::Ready);
                            }
                            Err(err) => {
                                tracing::error!("failed to start claude session: {err}");
                                status.set(SessionStatus::Error(err.to_string()));
                                history.write().push(ChatEntry::Error(err.to_string()));
                            }
                        }
                    }
                    ChatCommand::Send(text) => {
                        let Some(s) = session.as_mut() else {
                            history.write().push(ChatEntry::Error(
                                "no active session — pick a vault first".into(),
                            ));
                            continue;
                        };
                        history.write().push(ChatEntry::UserPrompt(text.clone()));
                        if let Err(err) = s.send(text).await {
                            history.write().push(ChatEntry::Error(err.to_string()));
                        }
                    }
                    ChatCommand::Interrupt => {
                        if let Some(mut s) = session.take() {
                            let _ = s.session.kill().await;
                            status.set(SessionStatus::Idle);
                        }
                    }
                }
            }
            event = next_event(session.as_mut()) => {
                match event {
                    Some(event) => history.write().push(ChatEntry::Stream(event)),
                    None => {
                        // Session ended.
                        if session.take().is_some() {
                            status.set(SessionStatus::Idle);
                            history.write().push(ChatEntry::Error("claude session ended".into()));
                        }
                    }
                }
            }
        }
    }

    if let Some(mut s) = session.take() {
        let _ = s.session.kill().await;
    }
}

async fn next_event(session: Option<&mut SessionHandle>) -> Option<glitch_ai::StreamEvent> {
    match session {
        Some(s) => s.recv().await,
        None => std::future::pending().await,
    }
}

pub async fn check_claude_available() -> bool {
    ClaudeClient::new().is_available().await
}

/// Convenience: spawn a directory picker and return a UTF-8 path on success.
pub async fn pick_vault_dir() -> Option<Utf8PathBuf> {
    let folder = rfd::AsyncFileDialog::new()
        .set_title("Pick a vault directory")
        .pick_folder()
        .await?;
    Utf8PathBuf::from_path_buf(folder.path().to_path_buf()).ok()
}
