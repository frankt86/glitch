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
use crate::menu as app_menu;
use crate::permissions::{self, PendingApproval, PermissionEvent};
use crate::settings::{self, AppSettings};
use crate::state::{
    AppState, ChatCommand, ChatEntry, ClaudeStatus, SessionStatus, SyncCommand, SyncState,
};
use crate::sync::sync_coroutine;
use crate::vault_actions;
use crate::watch::watch_coroutine;
use camino::Utf8PathBuf;
use dioxus::desktop::use_muda_event_handler;
use dioxus::prelude::*;
use glitch_ai::SessionConfig;
use glitch_core::{NoteId, Vault};
use jiff::Timestamp;
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
    let mut sidebar_width = use_signal(|| 360.0f32);
    let mut is_resizing = use_signal(|| false);
    let mut sidebar_collapsed = use_signal(|| false);
    let mut chat_collapsed = use_signal(|| false);
    let mut sync_error_dismissed = use_signal(|| false);

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

    // Auto-open the last used vault on startup.
    use_future({
        let mut app_state = app_state;
        let chat_tx = chat_tx.clone();
        let sync_tx = sync_tx.clone();
        let watch_tx = watch_tx.clone();
        let runtime_sig = permission_runtime;
        move || async move {
            let last_vault = app_settings.read().last_vault.clone();
            if let Some(path) = last_vault {
                let root = Utf8PathBuf::from(path);
                if root.exists() {
                    let root_clone = root.clone();
                    let load_result = tokio::task::spawn_blocking(move || Vault::load(&root_clone)).await;
                    match load_result {
                        Ok(Ok(vault)) => {
                            let root_path = vault.root.clone();
                            app_state.write().vault = Some(vault);
                            app_state.write().current_note = None;
                            app_state.write().editor_content.clear();
                            app_state.write().editor_dirty = false;
                            let config = build_session_config(&runtime_sig, app_settings);
                            chat_tx.send(ChatCommand::StartSession {
                                root: root_path.clone(),
                                config,
                            });
                            sync_tx.send((root_path.clone(), SyncCommand::CheckStatus));
                            watch_tx.send(root_path);
                        }
                        Ok(Err(err)) => {
                            tracing::warn!("failed to auto-open last vault: {err}");
                        }
                        Err(err) => {
                            tracing::warn!("vault load panicked: {err}");
                        }
                    }
                }
            }
        }
    });

    // Native menu event handler — routes File/View/Sync menu items to signals.
    use_muda_event_handler({
        let mut settings_visible = settings_visible;
        let mut graph_visible = graph_visible;
        let mut extractor_visible = extractor_visible;
        let mut sidebar_collapsed = sidebar_collapsed;
        let mut chat_collapsed = chat_collapsed;
        let mut app_state_m = app_state;
        let sync_tx_m = sync_tx.clone();
        let chat_tx_m = chat_tx.clone();
        let watch_tx_m = watch_tx.clone();
        let runtime_sig_m = permission_runtime;
        move |event: &dioxus::desktop::muda::MenuEvent| {
            match event.id().0.as_str() {
                app_menu::OPEN_VAULT => {
                    let chat_tx = chat_tx_m.clone();
                    let sync_tx = sync_tx_m.clone();
                    let watch_tx = watch_tx_m.clone();
                    let mut app_state = app_state_m;
                    let runtime_sig = runtime_sig_m;
                    spawn(async move {
                        let Some(root) = pick_vault_dir().await else { return };
                        match Vault::load(&root) {
                            Ok(vault) => {
                                let root_path = vault.root.clone();
                                settings::save_last_vault(root_path.as_str());
                                app_state.write().vault = Some(vault);
                                app_state.write().current_note = None;
                                app_state.write().editor_content.clear();
                                app_state.write().editor_dirty = false;
                                let config = build_session_config(&runtime_sig, app_settings);
                                chat_tx.send(ChatCommand::StartSession {
                                    root: root_path.clone(),
                                    config,
                                });
                                sync_tx.send((root_path.clone(), SyncCommand::CheckStatus));
                                watch_tx.send(root_path);
                            }
                            Err(err) => tracing::error!("failed to load vault: {err}"),
                        }
                    });
                }
                app_menu::NEW_VAULT => {
                    let chat_tx = chat_tx_m.clone();
                    let sync_tx = sync_tx_m.clone();
                    let watch_tx = watch_tx_m.clone();
                    let mut app_state = app_state_m;
                    let runtime_sig = runtime_sig_m;
                    spawn(async move {
                        let Some(root) = pick_vault_dir().await else { return };
                        if let Err(err) = vault_actions::create_vault(&root) {
                            tracing::error!("failed to create vault: {err}");
                            return;
                        }
                        match Vault::load(&root) {
                            Ok(vault) => {
                                let root_path = vault.root.clone();
                                settings::save_last_vault(root_path.as_str());
                                app_state.write().vault = Some(vault);
                                app_state.write().current_note = None;
                                app_state.write().editor_content.clear();
                                app_state.write().editor_dirty = false;
                                let config = build_session_config(&runtime_sig, app_settings);
                                chat_tx.send(ChatCommand::StartSession {
                                    root: root_path.clone(),
                                    config,
                                });
                                sync_tx.send((root_path.clone(), SyncCommand::CheckStatus));
                                watch_tx.send(root_path);
                            }
                            Err(err) => tracing::error!("failed to create vault: {err}"),
                        }
                    });
                }
                app_menu::SAVE => {
                    crate::components::editor::save_current(&mut app_state_m);
                }
                app_menu::EXTRACT_URL => extractor_visible.set(true),
                app_menu::SETTINGS => settings_visible.set(true),
                app_menu::NOTES_PANEL => {
                    // muda auto-toggles the check mark; mirror it in the signal
                    let c = !*sidebar_collapsed.peek();
                    sidebar_collapsed.set(c);
                }
                app_menu::CLAUDE_PANEL => {
                    let c = !*chat_collapsed.peek();
                    chat_collapsed.set(c);
                }
                app_menu::DAILY_NOTE => {
                    open_or_create_daily(&mut app_state_m);
                }
                app_menu::GRAPH => graph_visible.set(true),
                app_menu::SYNC_NOW => {
                    if let Some(root) = app_state_m.peek().vault.as_ref().map(|v| v.root.clone()) {
                        sync_tx_m.send((root, SyncCommand::Sync));
                    }
                }
                _ => {}
            }
        }
    });

    // Sidebar "+ New" button → create a note (with optional type/template/folder).
    let create_new_note = {
        let mut app_state = app_state;
        let mut history = chat_history;
        move |(title, note_type, folder): (String, String, String)| {
            let Some(root) = app_state.read().vault.as_ref().map(|v| v.root.clone()) else {
                history
                    .write()
                    .push(ChatEntry::Error("open a vault first".into()));
                return;
            };
            let result = if note_type.is_empty() {
                vault_actions::create_note(&root, &folder, &title)
            } else {
                let body = settings::render_template(&note_type, &title);
                vault_actions::create_note_from_template(&root, &folder, &title, &body)
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

    let on_create_folder = {
        let app_state = app_state;
        let mut history = chat_history;
        move |name: String| {
            if let Some(root) = app_state.read().vault.as_ref().map(|v| v.root.clone()) {
                if let Err(err) = vault_actions::create_folder(&root, &name) {
                    history.write().push(ChatEntry::Error(format!("failed to create folder: {err}")));
                }
            }
        }
    };

    let on_move_note = {
        let mut app_state = app_state;
        let mut history = chat_history;
        move |(note_rel, target_folder): (String, String)| {
            let Some(root) = app_state.read().vault.as_ref().map(|v| v.root.clone()) else { return };
            match vault_actions::move_note(&root, &note_rel, &target_folder) {
                Ok(()) => {
                    let mut s = app_state.write();
                    if s.current_note.as_ref().map(|c| c.as_str()) == Some(note_rel.as_str()) {
                        let filename = std::path::Path::new(&note_rel)
                            .file_name()
                            .and_then(|f| f.to_str())
                            .unwrap_or("note.md")
                            .to_string();
                        let new_rel = if target_folder.is_empty() {
                            filename
                        } else {
                            format!("{target_folder}/{filename}")
                        };
                        s.current_note = Some(glitch_core::NoteId::from_relative(new_rel));
                    }
                }
                Err(err) => {
                    history.write().push(ChatEntry::Error(format!("failed to move note: {err}")));
                }
            }
        }
    };

    let on_delete_folder = {
        let app_state = app_state;
        let mut history = chat_history;
        move |folder_rel: String| {
            let Some(root) = app_state.read().vault.as_ref().map(|v| v.root.clone()) else { return };
            if let Err(err) = vault_actions::delete_folder(&root, &folder_rel) {
                history.write().push(ChatEntry::Error(format!("failed to delete folder: {err}")));
            }
        }
    };

    let on_reparent = {
        let app_state = app_state;
        let mut history = chat_history;
        move |(note_rel, parent_rel): (String, Option<String>)| {
            let Some(root) = app_state.read().vault.as_ref().map(|v| v.root.clone()) else { return };
            if let Err(err) = vault_actions::set_note_parent(&root, &note_rel, parent_rel.as_deref()) {
                history.write().push(ChatEntry::Error(format!("failed to set parent: {err}")));
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

    let sidebar_w = if *sidebar_collapsed.read() { 0.0f32 } else { *sidebar_width.read() };
    let resize_w = if *sidebar_collapsed.read() { 0 } else { 5 };
    let chat_w: u32 = if *chat_collapsed.read() { 0 } else { 380 };

    rsx! {
        div {
            class: "app",
            onmousemove: move |evt| {
                if !*is_resizing.read() { return; }
                let x = evt.data().client_coordinates().x as f32;
                sidebar_width.set(x.clamp(160.0, 520.0));
            },
            onmouseup: move |_| is_resizing.set(false),
            onmouseleave: move |_| is_resizing.set(false),

            header { class: "topbar",
                // Sidebar collapse
                button {
                    class: "btn-icon",
                    title: if *sidebar_collapsed.read() { "Show notes" } else { "Hide notes" },
                    onclick: move |_| {
                        let c = !*sidebar_collapsed.read();
                        sidebar_collapsed.set(c);
                        app_menu::set_notes_panel_checked(!c);
                    },
                    if *sidebar_collapsed.read() { "⊢" } else { "⊣" }
                }

                // Vault path (flex: 1, centered)
                div { class: "vault-path", "{vault_path_label}" }

                // Sync status
                SyncBadge { state: sync_state }

                // Claude status badge
                ClaudeBadge { status: claude_status, session: session_status }

                // Claude panel collapse
                button {
                    class: "btn-icon",
                    title: if *chat_collapsed.read() { "Show Claude" } else { "Hide Claude" },
                    onclick: move |_| {
                        let c = !*chat_collapsed.read();
                        chat_collapsed.set(c);
                        app_menu::set_claude_panel_checked(!c);
                    },
                    if *chat_collapsed.read() { "⊣" } else { "⊢" }
                }
            }
            // Sync error banner — shows full error text, dismissible.
            {
                let ss = sync_state.read();
                if let SyncState::Error(ref msg) = *ss {
                    if !*sync_error_dismissed.read() {
                        let msg = msg.clone();
                        rsx! {
                            div { class: "sync-error-banner",
                                span { class: "sync-error-text", "Sync error: {msg}" }
                                button {
                                    class: "sync-error-dismiss",
                                    onclick: move |_| sync_error_dismissed.set(true),
                                    "×"
                                }
                            }
                        }
                    } else { rsx! {} }
                } else {
                    // Reset dismiss when error clears so the next error is visible.
                    if *sync_error_dismissed.read() {
                        sync_error_dismissed.set(false);
                    }
                    rsx! {}
                }
            }

            if app_state.read().vault.is_none() {
                div { class: "empty-state",
                    div { class: "empty-state-card",
                        h1 { class: "empty-state-title", "Glitch" }
                        p { class: "empty-state-sub", "AI-native knowledge base" }
                        div { class: "empty-state-actions",
                            button {
                                class: "btn btn-primary empty-state-btn",
                                onclick: move |_| {
                                    let mut app_state = app_state;
                                    let chat_tx = chat_tx.clone();
                                    let sync_tx = sync_tx.clone();
                                    let watch_tx = watch_tx.clone();
                                    let runtime_sig = permission_runtime;
                                    spawn(async move {
                                        let Some(root) = pick_vault_dir().await else { return };
                                        match Vault::load(&root) {
                                            Ok(vault) => {
                                                let root_path = vault.root.clone();
                                                settings::save_last_vault(root_path.as_str());
                                                app_state.write().vault = Some(vault);
                                                app_state.write().current_note = None;
                                                app_state.write().editor_content.clear();
                                                app_state.write().editor_dirty = false;
                                                let config = build_session_config(&runtime_sig, app_settings);
                                                chat_tx.send(ChatCommand::StartSession { root: root_path.clone(), config });
                                                sync_tx.send((root_path.clone(), SyncCommand::CheckStatus));
                                                watch_tx.send(root_path);
                                            }
                                            Err(err) => tracing::error!("failed to load vault: {err}"),
                                        }
                                    });
                                },
                                "Open Vault"
                                kbd { class: "empty-state-kbd", "Ctrl+O" }
                            }
                            button {
                                class: "btn empty-state-btn",
                                onclick: move |_| {
                                    let mut app_state = app_state;
                                    let chat_tx = chat_tx.clone();
                                    let sync_tx = sync_tx.clone();
                                    let watch_tx = watch_tx.clone();
                                    let runtime_sig = permission_runtime;
                                    spawn(async move {
                                        let Some(root) = pick_vault_dir().await else { return };
                                        if let Err(err) = vault_actions::create_vault(&root) {
                                            tracing::error!("failed to create vault: {err}");
                                            return;
                                        }
                                        match Vault::load(&root) {
                                            Ok(vault) => {
                                                let root_path = vault.root.clone();
                                                settings::save_last_vault(root_path.as_str());
                                                app_state.write().vault = Some(vault);
                                                app_state.write().current_note = None;
                                                app_state.write().editor_content.clear();
                                                app_state.write().editor_dirty = false;
                                                let config = build_session_config(&runtime_sig, app_settings);
                                                chat_tx.send(ChatCommand::StartSession { root: root_path.clone(), config });
                                                sync_tx.send((root_path.clone(), SyncCommand::CheckStatus));
                                                watch_tx.send(root_path);
                                            }
                                            Err(err) => tracing::error!("failed to init vault: {err}"),
                                        }
                                    });
                                },
                                "New Vault"
                            }
                        }
                    }
                }
            } else {
                main {
                    class: "workspace",
                    style: "grid-template-columns: {sidebar_w}px {resize_w}px 1fr {chat_w}px",
                    Sidebar {
                        state: app_state,
                        on_create_note: create_new_note,
                        on_create_folder,
                        on_move_note,
                        on_delete_folder,
                        on_reparent,
                    }
                    div {
                        class: "sidebar-resize-handle",
                        onmousedown: move |evt| {
                            evt.prevent_default();
                            is_resizing.set(true);
                        },
                    }
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
            }
            PermissionModal { pending: pending_approvals, on_decision }
            SettingsPanel { visible: settings_visible, settings: app_settings }
            GraphView { visible: graph_visible, state: app_state }
            ExtractorDialog { visible: extractor_visible, state: app_state }
        }
    }
}

fn build_session_config(
    runtime: &Signal<Option<PermissionRuntime>>,
    settings: Signal<AppSettings>,
) -> SessionConfig {
    let read = runtime.read();
    let s = settings.read();
    let system_prompt =
        std::fs::read_to_string(s.agent_instructions_path.as_std_path()).ok();
    let mut cfg = SessionConfig {
        allowed_tools: Some(s.allowed_tools_silent.clone()),
        system_prompt_append: system_prompt,
        ..Default::default()
    };
    drop(s);
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
                    match vault_actions::create_note_from_template(&vault_root, "", &title, &body) {
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
                CommandOutcome::DailyNote => {
                    open_or_create_daily(app_state);
                    history.write().push(ChatEntry::LocalReply {
                        command: display,
                        body: format!("opened daily/{}", Timestamp::now().strftime("%Y-%m-%d")),
                    });
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

/// Open today's daily note, creating it (in `daily/YYYY-MM-DD.md`) if it does
/// not exist yet. No-op when no vault is loaded.
fn open_or_create_daily(app_state: &mut Signal<AppState>) {
    let vault_root = match app_state.read().vault.as_ref().map(|v| v.root.clone()) {
        Some(r) => r,
        None => return,
    };
    let date = Timestamp::now().strftime("%Y-%m-%d").to_string();
    let rel_path = format!("daily/{date}.md");
    let abs_path = vault_root.join(&rel_path);

    if abs_path.exists() {
        match std::fs::read_to_string(abs_path.as_std_path()) {
            Ok(content) => {
                let mut s = app_state.write();
                s.current_note = Some(NoteId::from_relative(rel_path));
                s.editor_content = content;
                s.editor_dirty = false;
            }
            Err(err) => tracing::error!("failed to read daily note: {err}"),
        }
    } else {
        let body = format!(
            "---\ntitle: \"{date}\"\ncreated: {date}\ntags: []\ntype: daily\n---\n\n\
            # {date}\n\n## Today\n\n\n\n## Notes\n\n\n\n## Grateful for\n\n"
        );
        match vault_actions::create_note_from_template(&vault_root, "daily", &date, &body) {
            Ok(created) => {
                let content =
                    std::fs::read_to_string(created.absolute_path.as_std_path()).unwrap_or(body);
                let mut s = app_state.write();
                s.current_note = Some(NoteId(created.relative_path));
                s.editor_content = content;
                s.editor_dirty = false;
            }
            Err(err) => tracing::error!("failed to create daily note: {err}"),
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
fn SyncBadge(state: Signal<SyncState>) -> Element {
    let s = state.read();
    rsx! { div { class: "{s.css_class()}", "{s.label()}" } }
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
