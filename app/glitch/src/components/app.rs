use crate::chat::{chat_coroutine, check_claude_available, pick_vault_dir};
use crate::commands::{try_parse, CommandContext, CommandOutcome};
use crate::components::chat_panel::ChatPanel;
use crate::components::editor::Editor;
use crate::components::extractor::ExtractorDialog;
use crate::components::graph_view::GraphView;
use crate::components::permission_modal::PermissionModal;
use crate::components::settings_panel::SettingsPanel;
use crate::components::sidebar::Sidebar;
use crate::extract;
use crate::permissions::{self, PendingApproval, PermissionEvent};
use crate::settings;
use crate::state::{
    AppState, ChatCommand, ChatEntry, ClaudeStatus, SessionStatus, SyncCommand, SyncState,
};
use crate::sync::sync_coroutine;
use crate::vault_actions;
use crate::watch::watch_coroutine;
use camino::Utf8PathBuf;
use dioxus::prelude::*;
use glitch_ai::SessionConfig;
use glitch_core::{NoteId, Vault};
use glitch_mcp::pipe::ApprovalDecision;
use tokio::sync::mpsc::UnboundedSender;

/// Plain-data view of the running permission server, suitable for a Signal.
#[derive(Clone)]
struct PermissionRuntime {
    pipe_name: String,
    mcp_config_path: Option<Utf8PathBuf>,
    resolver: UnboundedSender<(String, ApprovalDecision)>,
}

impl PartialEq for PermissionRuntime {
    // The pipe name is unique per process boot, so it's a sufficient identity.
    fn eq(&self, other: &Self) -> bool {
        self.pipe_name == other.pipe_name
    }
}

#[component]
pub fn App() -> Element {
    let app_state = use_signal(AppState::default);
    let chat_history = use_signal(Vec::<ChatEntry>::new);
    let session_status = use_signal(|| SessionStatus::Idle);
    let claude_status = use_signal(|| ClaudeStatus::Unknown);
    let sync_state = use_signal(SyncState::default);
    let pending_approvals = use_signal(Vec::<PendingApproval>::new);
    let permission_runtime = use_signal(|| Option::<PermissionRuntime>::None);
    let app_settings = use_signal(settings::load);
    let settings_visible = use_signal(|| false);
    let graph_visible = use_signal(|| false);
    let extractor_visible = use_signal(|| false);

    // Ensure agent instructions + note type templates exist on first run.
    use_future({
        let app_settings = app_settings;
        move || async move {
            let path = app_settings.read().agent_instructions_path.clone();
            if let Err(err) = settings::ensure_agent_instructions(&path) {
                tracing::warn!("failed to seed agent instructions: {err}");
            }
            if let Err(err) = settings::ensure_default_types() {
                tracing::warn!("failed to seed note types: {err}");
            }
        }
    });

    use_future({
        let mut claude_status = claude_status;
        move || async move {
            let available = check_claude_available().await;
            claude_status.set(if available {
                ClaudeStatus::Available
            } else {
                ClaudeStatus::Missing
            });
        }
    });

    // Permission server bootstrap + event drain. Starts on app boot.
    use_coroutine({
        let mut pending = pending_approvals;
        let mut runtime_sig = permission_runtime;
        move |_: UnboundedReceiver<()>| async move {
            let handle = match permissions::start() {
                Ok(h) => h,
                Err(err) => {
                    tracing::error!("permission server failed to start: {err}");
                    return;
                }
            };
            let path = match permissions::write_mcp_config(&handle.pipe_name) {
                Ok(p) => Some(p),
                Err(err) => {
                    tracing::warn!("failed to write MCP config: {err}");
                    None
                }
            };
            runtime_sig.set(Some(PermissionRuntime {
                pipe_name: handle.pipe_name,
                mcp_config_path: path,
                resolver: handle.resolver,
            }));
            let mut events = handle.events;
            while let Some(evt) = events.recv().await {
                match evt {
                    PermissionEvent::New(approval) => pending.write().push(approval),
                    PermissionEvent::Cancelled(id) => pending.write().retain(|a| a.id != id),
                }
            }
        }
    });

    let chat_tx = use_coroutine(move |rx: UnboundedReceiver<ChatCommand>| async move {
        chat_coroutine(rx, chat_history, session_status).await;
    });

    let sync_tx = use_coroutine(
        move |rx: UnboundedReceiver<(Utf8PathBuf, SyncCommand)>| async move {
            sync_coroutine(rx, sync_state).await;
        },
    );

    let watch_tx = use_coroutine(move |rx: UnboundedReceiver<Utf8PathBuf>| async move {
        watch_coroutine(rx, app_state).await;
    });

    let open_vault = {
        let mut app_state = app_state;
        let chat_tx = chat_tx.clone();
        let sync_tx = sync_tx.clone();
        let watch_tx = watch_tx.clone();
        let runtime_sig = permission_runtime;
        move |_| {
            let chat_tx = chat_tx.clone();
            let sync_tx = sync_tx.clone();
            let watch_tx = watch_tx.clone();
            let runtime_sig = runtime_sig;
            spawn(async move {
                let Some(root) = pick_vault_dir().await else {
                    return;
                };
                match Vault::load(&root) {
                    Ok(vault) => {
                        let root_path = vault.root.clone();
                        app_state.write().vault = Some(vault);
                        app_state.write().current_note = None;
                        app_state.write().editor_content.clear();
                        app_state.write().editor_dirty = false;

                        let config = build_session_config(&runtime_sig);
                        chat_tx.send(ChatCommand::StartSession {
                            root: root_path.clone(),
                            config,
                        });
                        sync_tx.send((root_path.clone(), SyncCommand::CheckStatus));
                        watch_tx.send(root_path);
                    }
                    Err(err) => {
                        tracing::error!("failed to load vault: {err}");
                    }
                }
            });
        }
    };

    // Sidebar "+ New" button → create a note (with optional type/template).
    let create_new_note = {
        let mut app_state = app_state;
        let mut history = chat_history;
        move |(title, note_type): (String, String)| {
            let Some(root) = app_state.read().vault.as_ref().map(|v| v.root.clone()) else {
                history
                    .write()
                    .push(ChatEntry::Error("open a vault first".into()));
                return;
            };
            let result = if note_type.is_empty() {
                vault_actions::create_note(&root, &title)
            } else {
                let body = settings::render_template(&note_type, &title);
                vault_actions::create_note_from_template(&root, &title, &body)
            };
            match result {
                Ok(created) => {
                    let id = NoteId(created.relative_path.clone());
                    let content =
                        std::fs::read_to_string(&created.absolute_path).unwrap_or_default();
                    let mut s = app_state.write();
                    s.current_note = Some(id);
                    s.editor_content = content;
                    s.editor_dirty = false;
                    drop(s);
                    let type_label = if note_type.is_empty() {
                        String::new()
                    } else {
                        format!(" (type: {note_type})")
                    };
                    history.write().push(ChatEntry::LocalReply {
                        command: "/note".into(),
                        body: format!("created {}{type_label}", created.relative_path),
                    });
                }
                Err(err) => {
                    history
                        .write()
                        .push(ChatEntry::Error(format!("failed to create note: {err}")));
                }
            }
        }
    };

    let trigger_sync = {
        let app_state = app_state;
        let sync_tx = sync_tx.clone();
        move |_| {
            if let Some(root) = app_state.read().vault.as_ref().map(|v| v.root.clone()) {
                sync_tx.send((root, SyncCommand::Sync));
            }
        }
    };

    let on_decision = {
        let mut pending = pending_approvals;
        let runtime_sig = permission_runtime;
        move |(id, decision): (String, ApprovalDecision)| {
            // Optimistically remove from queue; the resolver will also fire a
            // Cancelled event, which is a no-op against an empty match.
            pending.write().retain(|a| a.id != id);
            if let Some(rt) = runtime_sig.read().as_ref() {
                if let Err(err) = rt.resolver.send((id, decision)) {
                    tracing::warn!("failed to send approval decision: {err}");
                }
            }
        }
    };

    let vault_path_label = app_state
        .read()
        .vault
        .as_ref()
        .map(|v| v.root.to_string())
        .unwrap_or_else(|| "no vault".into());

    rsx! {
        div { class: "app",
            header { class: "topbar",
                div { class: "brand", "Glitch" }
                button { class: "btn", onclick: open_vault, "Open vault…" }
                div { class: "vault-path", "{vault_path_label}" }
                button {
                    class: "btn",
                    onclick: {
                        let mut extractor_visible = extractor_visible;
                        move |_| extractor_visible.set(true)
                    },
                    "Extract URL…"
                }
                button {
                    class: "btn",
                    onclick: {
                        let mut graph_visible = graph_visible;
                        move |_| graph_visible.set(true)
                    },
                    "Graph"
                }
                button {
                    class: "btn",
                    onclick: {
                        let mut settings_visible = settings_visible;
                        move |_| settings_visible.set(true)
                    },
                    "Settings"
                }
                SyncBadge { state: sync_state, on_sync: trigger_sync }
                ClaudeBadge { status: claude_status, session: session_status }
            }
            main { class: "workspace",
                Sidebar { state: app_state, on_create_note: create_new_note }
                Editor {
                    state: app_state,
                    on_command: {
                        let mut history = chat_history;
                        let mut app_state = app_state;
                        let chat_tx = chat_tx.clone();
                        move |text: String| {
                            handle_send(text, &mut app_state, &mut history, &chat_tx);
                        }
                    },
                }
                ChatPanel {
                    history: chat_history,
                    status: session_status,
                    claude_status,
                    on_send: {
                        let mut history = chat_history;
                        let mut app_state = app_state;
                        let chat_tx = chat_tx.clone();
                        move |text: String| {
                            handle_send(text, &mut app_state, &mut history, &chat_tx);
                        }
                    },
                    on_interrupt: move |_| chat_tx.send(ChatCommand::Interrupt),
                }
            }
            PermissionModal { pending: pending_approvals, on_decision }
            SettingsPanel { visible: settings_visible, settings: app_settings }
            GraphView { visible: graph_visible, state: app_state }
            ExtractorDialog { visible: extractor_visible, state: app_state }
        }
    }
}

fn build_session_config(runtime: &Signal<Option<PermissionRuntime>>) -> SessionConfig {
    let read = runtime.read();
    let mut cfg = SessionConfig {
        // Read-only safe tools auto-approve. Anything else triggers the modal.
        allowed_tools: Some("Read,Glob,Grep,LS,TodoWrite".into()),
        ..Default::default()
    };
    if let Some(rt) = read.as_ref() {
        if let Some(path) = &rt.mcp_config_path {
            cfg.mcp_config = Some(path.to_string());
            cfg.permission_prompt_tool = Some(glitch_mcp::PERMISSION_TOOL_NAME.into());
        }
    }
    cfg
}

fn handle_send(
    text: String,
    app_state: &mut Signal<AppState>,
    history: &mut Signal<Vec<ChatEntry>>,
    chat_tx: &Coroutine<ChatCommand>,
) {
    match try_parse(&text) {
        Some(Ok(cmd)) => {
            let display = format!("/{}", cmd.name());
            history.write().push(ChatEntry::UserPrompt(display.clone()));
            let ctx = build_context(*app_state);
            match cmd.dispatch(&ctx) {
                CommandOutcome::LocalReply(body) => {
                    history.write().push(ChatEntry::LocalReply {
                        command: display,
                        body,
                    });
                }
                CommandOutcome::Prompt(prompt) => {
                    chat_tx.send(ChatCommand::Send(prompt));
                }
                CommandOutcome::Error(err) => {
                    history.write().push(ChatEntry::Error(err));
                }
                CommandOutcome::Extract { url, vault_root } => {
                    let mut history = *history;
                    spawn(async move {
                        match extract::extract_to_vault(&url, &vault_root).await {
                            Ok(note) => {
                                history.write().push(ChatEntry::LocalReply {
                                    command: "/extract".into(),
                                    body: format!(
                                        "saved {} ({})",
                                        note.relative_path, note.title
                                    ),
                                });
                            }
                            Err(err) => {
                                history
                                    .write()
                                    .push(ChatEntry::Error(format!("/extract failed: {err}")));
                            }
                        }
                    });
                }
                CommandOutcome::LocalCreate { title, note_type, vault_root } => {
                    let body = settings::render_template(&note_type, &title);
                    match vault_actions::create_note_from_template(&vault_root, &title, &body) {
                        Ok(created) => {
                            let id = glitch_core::NoteId(created.relative_path.clone());
                            let content = std::fs::read_to_string(&created.absolute_path)
                                .unwrap_or_default();
                            let mut s = app_state.write();
                            s.current_note = Some(id);
                            s.editor_content = content;
                            s.editor_dirty = false;
                            drop(s);
                            history.write().push(ChatEntry::LocalReply {
                                command: display,
                                body: format!(
                                    "created {} (type: {note_type})",
                                    created.relative_path
                                ),
                            });
                        }
                        Err(err) => {
                            history.write().push(ChatEntry::Error(format!(
                                "/note failed: {err}"
                            )));
                        }
                    }
                }
            }
        }
        Some(Err(err)) => {
            history.write().push(ChatEntry::Error(err));
        }
        None => {
            // Inject current-note context so Claude knows what the user is looking at.
            let ctx = build_context(*app_state);
            let enriched = match &ctx.current_note_relative {
                Some(rel) => format!("[Open note: {rel}]\n\n{text}"),
                None => text,
            };
            chat_tx.send(ChatCommand::Send(enriched));
        }
    }
}

fn build_context(app_state: Signal<AppState>) -> CommandContext {
    let snapshot = app_state.read();
    let vault_root = snapshot.vault.as_ref().map(|v| v.root.clone());
    let current = snapshot.current_note().map(|n| n.id.as_str().to_string());
    let body = if current.is_some() {
        Some(snapshot.editor_content.clone())
    } else {
        None
    };
    CommandContext {
        vault_root,
        current_note_relative: current,
        current_note_content: body,
    }
}

#[component]
fn SyncBadge(state: Signal<SyncState>, on_sync: EventHandler<()>) -> Element {
    let s = state.read().clone();
    let class = s.css_class().to_string();
    let label = s.label();
    let detail = match &s {
        SyncState::Dirty(st) => format!(" · {} files", st.dirty_files.len()),
        SyncState::Conflicts(st) => {
            format!(
                " · {} conflicts",
                st.dirty_files.iter().filter(|e| e.code.contains('U')).count()
            )
        }
        SyncState::Error(err) => format!(" · {err}"),
        _ => String::new(),
    };
    let can_sync = matches!(s, SyncState::Clean | SyncState::Dirty(_));
    rsx! {
        div { class: "sync-group",
            div { class: "{class}", title: "{detail}", "git: {label}" }
            if can_sync {
                button {
                    class: "btn",
                    onclick: move |_| on_sync.call(()),
                    "Sync"
                }
            }
        }
    }
}

#[component]
fn ClaudeBadge(status: Signal<ClaudeStatus>, session: Signal<SessionStatus>) -> Element {
    let (label, class) = match (&*status.read(), &*session.read()) {
        (ClaudeStatus::Unknown, _) => ("checking claude…", "badge badge-neutral"),
        (ClaudeStatus::Missing, _) => ("claude CLI not found", "badge badge-error"),
        (ClaudeStatus::Available, SessionStatus::Idle) => ("ready (no session)", "badge badge-neutral"),
        (ClaudeStatus::Available, SessionStatus::Starting) => ("starting…", "badge badge-warn"),
        (ClaudeStatus::Available, SessionStatus::Ready) => ("session live", "badge badge-ok"),
        (ClaudeStatus::Available, SessionStatus::Error(_)) => ("session error", "badge badge-error"),
    };
    rsx! { div { class: "{class}", "{label}" } }
}
