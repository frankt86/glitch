use crate::components::slash_palette::{matches as palette_matches, slash_query, SlashPalette};
use crate::state::{ChatEntry, ClaudeStatus, SessionStatus};
use dioxus::prelude::*;
use glitch_ai::{ContentBlock, ContentField, StreamEvent};

#[component]
pub fn ChatPanel(
    history: Signal<Vec<ChatEntry>>,
    status: Signal<SessionStatus>,
    claude_status: Signal<ClaudeStatus>,
    on_send: EventHandler<String>,
    on_interrupt: EventHandler<()>,
) -> Element {
    let mut draft = use_signal(String::new);
    let mut palette_index = use_signal(|| 0usize);
    let mut is_listening = use_signal(|| false);
    let entries = history.read().clone();

    let send = move |_| {
        let text = draft.read().trim().to_string();
        if text.is_empty() {
            return;
        }
        on_send.call(text);
        draft.set(String::new());
    };

    let setup_needed = matches!(*claude_status.read(), ClaudeStatus::Missing);
    let session_idle = matches!(*status.read(), SessionStatus::Idle);

    rsx! {
        aside { class: "chat",
            header { class: "chat-header",
                span { "Claude" }
                if matches!(*status.read(), SessionStatus::Ready) {
                    button { class: "btn-link", onclick: move |_| on_interrupt.call(()), "stop" }
                }
            }
            div { class: "chat-history",
                if setup_needed {
                    SetupCard {}
                } else if entries.is_empty() && session_idle {
                    div { class: "chat-empty", "open a vault to start a session" }
                } else {
                    for (i, entry) in entries.iter().enumerate() {
                        EntryView { key: "{i}", entry: entry.clone() }
                    }
                }
            }
            footer { class: "chat-input",
                SlashPalette {
                    text: draft,
                    selected: palette_index,
                    on_select: move |insertion: &'static str| {
                        draft.set(insertion.to_string());
                        palette_index.set(0);
                    },
                }
                textarea {
                    class: "chat-textarea",
                    placeholder: "/help for commands · or ask Claude…  (Enter to send, Shift+Enter for newline)",
                    value: "{draft.read()}",
                    rows: "3",
                    oninput: move |evt: FormEvent| {
                        draft.set(evt.value());
                        palette_index.set(0);
                    },
                    onkeydown: move |evt| {
                        let body = draft.read().clone();
                        let palette_open = slash_query(&body).is_some();
                        let items = if palette_open { palette_matches(slash_query(&body).unwrap_or("")) } else { Vec::new() };

                        // Palette intercepts arrows / Enter / Esc when open.
                        if palette_open && !items.is_empty() {
                            match evt.key() {
                                Key::ArrowDown => {
                                    evt.prevent_default();
                                    let len = items.len();
                                    let mut i = palette_index.write();
                                    *i = (*i + 1) % len;
                                    return;
                                }
                                Key::ArrowUp => {
                                    evt.prevent_default();
                                    let len = items.len();
                                    let mut i = palette_index.write();
                                    *i = if *i == 0 { len - 1 } else { *i - 1 };
                                    return;
                                }
                                Key::Enter if !evt.modifiers().shift() => {
                                    evt.prevent_default();
                                    let i = (*palette_index.read()).min(items.len() - 1);
                                    let chosen = items[i];
                                    draft.set(chosen.insertion.to_string());
                                    palette_index.set(0);
                                    return;
                                }
                                Key::Escape => {
                                    evt.prevent_default();
                                    draft.set(String::new());
                                    palette_index.set(0);
                                    return;
                                }
                                _ => {}
                            }
                        }

                        // Submit on Enter (or Ctrl+Enter as alias). Shift+Enter inserts a newline.
                        if evt.key() == Key::Enter && !evt.modifiers().shift() {
                            evt.prevent_default();
                            let text = draft.read().trim().to_string();
                            if !text.is_empty() {
                                on_send.call(text);
                                draft.set(String::new());
                                palette_index.set(0);
                            }
                        }
                    }
                }
                div { class: "chat-actions",
                    button {
                        class: if *is_listening.read() { "btn btn-mic listening" } else { "btn btn-mic" },
                        title: "Voice input",
                        onclick: move |_| {
                            if *is_listening.read() { return; }
                            is_listening.set(true);
                            spawn(async move {
                                let script = r#"
                                    (async function() {
                                        var diag = {
                                            hasMD: !!(navigator.mediaDevices),
                                            hasGUM: !!(navigator.mediaDevices && navigator.mediaDevices.getUserMedia),
                                            hasSR: !!(window.SpeechRecognition || window.webkitSpeechRecognition),
                                            protocol: location.protocol,
                                            href: location.href
                                        };
                                        console.log('[glitch] STT diag', JSON.stringify(diag));
                                        if (!diag.hasSR) {
                                            dioxus.send('[MIC ERROR] SpeechRecognition not available. protocol=' + diag.protocol + ' href=' + diag.href);
                                            return;
                                        }
                                        if (diag.hasGUM) {
                                            try {
                                                const s = await navigator.mediaDevices.getUserMedia({audio:true});
                                                s.getTracks().forEach(t => t.stop());
                                            } catch(e) {
                                                console.log('[glitch] getUserMedia failed:', e.name, e.message);
                                            }
                                        }
                                        const R = window.SpeechRecognition || window.webkitSpeechRecognition;
                                        const r = new R();
                                        r.lang = 'en-US';
                                        r.interimResults = false;
                                        r.maxAlternatives = 1;
                                        r.onresult = e => dioxus.send(e.results[0][0].transcript);
                                        r.onerror = e => dioxus.send('[MIC ERROR] ' + (e.error||'unknown') + ' diag=' + JSON.stringify(diag));
                                        r.start();
                                    })();
                                "#;
                                let mut eval = document::eval(script);
                                if let Ok(t) = eval.recv::<String>().await {
                                    if !t.starts_with("[MIC ERROR]") && !t.is_empty() {
                                        let existing = draft.peek().clone();
                                        if existing.is_empty() {
                                            draft.set(t);
                                        } else {
                                            draft.set(format!("{} {}", existing.trim_end(), t));
                                        }
                                    }
                                }
                                is_listening.set(false);
                            });
                        },
                        "🎤"
                    }
                    button { class: "btn btn-primary", onclick: send, "Send (Enter)" }
                }
            }
        }
    }
}

#[component]
fn SetupCard() -> Element {
    rsx! {
        div { class: "setup-card",
            h3 { "Claude Code is not on PATH" }
            p {
                "Glitch drives the Claude Code CLI as an agent backend. Install it from "
                a {
                    href: "https://docs.claude.com/claude-code",
                    "docs.claude.com/claude-code"
                }
                " and restart Glitch."
            }
        }
    }
}

#[component]
fn EntryView(entry: ChatEntry) -> Element {
    match entry {
        ChatEntry::UserPrompt(text) => rsx! {
            div { class: "entry user",
                div { class: "entry-role", "you" }
                pre { class: "entry-text", "{text}" }
            }
        },
        ChatEntry::LocalReply { command, body } => rsx! {
            div { class: "entry local-reply",
                div { class: "entry-role", "{command}" }
                pre { class: "entry-text", "{body}" }
            }
        },
        ChatEntry::Error(text) => rsx! {
            div { class: "entry error",
                div { class: "entry-role", "error" }
                pre { class: "entry-text", "{text}" }
            }
        },
        ChatEntry::Stream(event) => render_stream_event(event),
    }
}

fn render_stream_event(event: StreamEvent) -> Element {
    match event {
        StreamEvent::System {
            subtype,
            session_id,
            model,
            ..
        } => {
            let label = format!(
                "system · {} · {} · {}",
                subtype.unwrap_or_else(|| "init".into()),
                model.unwrap_or_else(|| "?".into()),
                session_id.as_deref().unwrap_or("?")
            );
            rsx! { div { class: "entry system", "{label}" } }
        }
        StreamEvent::Assistant { message, .. } => rsx! {
            div { class: "entry assistant",
                div { class: "entry-role", "claude" }
                for block in message.content.iter() {
                    AssistantBlockView { block: block.clone() }
                }
            }
        },
        StreamEvent::User { message, .. } => match message.content {
            ContentField::Text(text) if !text.is_empty() => rsx! {
                div { class: "entry user-echo", pre { "{text}" } }
            },
            ContentField::Blocks(blocks) => rsx! {
                div { class: "entry tool-results",
                    for block in blocks.into_iter() {
                        ToolResultBlockView { block }
                    }
                }
            },
            ContentField::Text(_) => rsx! { Fragment {} },
        },
        StreamEvent::Result {
            subtype,
            is_error,
            result,
            total_cost_usd,
            duration_ms,
            ..
        } => {
            let cost = total_cost_usd
                .map(|c| format!("${c:.4}"))
                .unwrap_or_else(|| "—".into());
            let dur = duration_ms
                .map(|d| format!("{d}ms"))
                .unwrap_or_else(|| "—".into());
            let class = if is_error { "entry result error" } else { "entry result" };
            let summary = result.unwrap_or_default();
            let header = format!(
                "result · {} · {} · {}",
                subtype.unwrap_or_else(|| "?".into()),
                cost,
                dur
            );
            rsx! {
                div { class: "{class}",
                    div { class: "entry-role", "{header}" }
                    if !summary.is_empty() {
                        pre { class: "entry-text", "{summary}" }
                    }
                }
            }
        }
        StreamEvent::Unknown => rsx! { Fragment {} },
    }
}

#[component]
fn AssistantBlockView(block: ContentBlock) -> Element {
    match block {
        ContentBlock::Text { text } => {
            let tts_text = text.clone();
            rsx! {
                div { class: "entry-text-wrap",
                    pre { class: "entry-text", "{text}" }
                    button {
                        class: "tts-btn",
                        title: "Read aloud",
                        onclick: move |_| {
                            let script = format!(
                                "window.speechSynthesis.cancel();\
                                 window.speechSynthesis.speak(new SpeechSynthesisUtterance({}));\
                                 dioxus.send(null);",
                                serde_json::to_string(&tts_text).unwrap_or_default()
                            );
                            spawn(async move { document::eval(&script).await.ok(); });
                        },
                        "🔊"
                    }
                }
            }
        }
        ContentBlock::Thinking { thinking } => rsx! {
            details { class: "entry-thinking",
                summary { "thinking" }
                pre { class: "entry-text", "{thinking}" }
            }
        },
        ContentBlock::ToolUse { id, name, input } => {
            let pretty = serde_json::to_string_pretty(&input).unwrap_or_default();
            rsx! {
                details { class: "tool-use",
                    summary { "🔧 {name}  ", code { class: "tool-id", "{id}" } }
                    pre { class: "entry-text", "{pretty}" }
                }
            }
        }
        ContentBlock::ToolResult { .. } | ContentBlock::Unknown => rsx! { Fragment {} },
    }
}

#[component]
fn ToolResultBlockView(block: ContentBlock) -> Element {
    if let ContentBlock::ToolResult {
        tool_use_id,
        content,
        is_error,
    } = block
    {
        rsx! {
            ToolResultView {
                tool_use_id,
                content,
                is_error,
            }
        }
    } else {
        rsx! {}
    }
}

#[component]
fn ToolResultView(tool_use_id: String, content: serde_json::Value, is_error: bool) -> Element {
    let body = match content {
        serde_json::Value::String(s) => s,
        other => serde_json::to_string_pretty(&other).unwrap_or_default(),
    };
    let class = if is_error {
        "tool-result error"
    } else {
        "tool-result"
    };
    rsx! {
        details { class: "{class}",
            summary { "↳ result  ", code { class: "tool-id", "{tool_use_id}" } }
            pre { class: "entry-text", "{body}" }
        }
    }
}
