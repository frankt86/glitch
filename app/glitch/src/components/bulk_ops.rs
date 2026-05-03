use crate::settings::AppSettings;
use crate::state::AppState;
use dioxus::prelude::*;
use glitch_core::frontmatter as fm;
use std::process::Stdio;

#[derive(Clone, PartialEq)]
enum BulkOp {
    Summarize,
    Tags,
}

#[derive(Clone, PartialEq)]
enum BulkState {
    Idle,
    Running { current: String, done: usize, total: usize },
    Done { done: usize, errors: usize },
}

#[component]
pub fn BulkOpsDialog(
    visible: Signal<bool>,
    state: Signal<AppState>,
    settings: Signal<AppSettings>,
) -> Element {
    if !*visible.read() {
        return rsx! { Fragment {} };
    }

    let mut op = use_signal(|| BulkOp::Summarize);
    let mut bulk_state = use_signal(|| BulkState::Idle);
    let mut skip_existing = use_signal(|| true);

    use_effect(move || {
        if *visible.read() {
            bulk_state.set(BulkState::Idle);
        }
    });

    let close = move |_| visible.set(false);
    let running = matches!(*bulk_state.read(), BulkState::Running { .. });

    let state_snap = bulk_state.read().clone();
    let (progress_current, progress_done, progress_total) = match &state_snap {
        BulkState::Running { current, done, total } => (current.clone(), *done, *total),
        _ => (String::new(), 0, 0),
    };
    let pct = if progress_total > 0 { progress_done * 100 / progress_total } else { 0 };
    let op_label = match *op.read() {
        BulkOp::Summarize => "summary",
        BulkOp::Tags => "tags",
    };
    let op_hint = match *op.read() {
        BulkOp::Summarize => "Writes a `summary:` frontmatter field to each note via Claude.",
        BulkOp::Tags => "Suggests tags and writes a `tags:` frontmatter field to each note via Claude.",
    };

    rsx! {
        div { class: "modal-overlay", onclick: close,
            div { class: "bulk-ops-card", onclick: move |e| e.stop_propagation(),
                header { class: "settings-header",
                    h2 { "Bulk AI Operations" }
                    button { class: "btn-link", onclick: close, "Close" }
                }
                div { class: "bulk-ops-body",
                    div { class: "bulk-op-section",
                        div { class: "bulk-op-label", "Operation" }
                        div { class: "bulk-op-btns",
                            button {
                                class: if *op.read() == BulkOp::Summarize { "btn bulk-op-btn active" } else { "btn bulk-op-btn" },
                                disabled: running,
                                onclick: move |_| op.set(BulkOp::Summarize),
                                "📝 Summarize"
                            }
                            button {
                                class: if *op.read() == BulkOp::Tags { "btn bulk-op-btn active" } else { "btn bulk-op-btn" },
                                disabled: running,
                                onclick: move |_| op.set(BulkOp::Tags),
                                "🏷 Add Tags"
                            }
                        }
                        div { class: "bulk-op-hint", "{op_hint}" }
                    }

                    label { class: "settings-toggle",
                        input {
                            r#type: "checkbox",
                            checked: "{skip_existing.read()}",
                            disabled: running,
                            oninput: move |e: FormEvent| skip_existing.set(e.value() == "true"),
                        }
                        span { "Skip notes that already have `{op_label}:` set" }
                    }

                    div { class: "bulk-status-area",
                        if matches!(state_snap, BulkState::Idle) {
                            div { class: "bulk-status-idle",
                                "Ready — all notes in the open vault will be processed by Claude."
                            }
                        } else if matches!(state_snap, BulkState::Running { .. }) {
                            div { class: "bulk-progress-wrap",
                                div { class: "bulk-progress-bar",
                                    div { class: "bulk-progress-fill", style: "width:{pct}%" }
                                }
                                div { class: "bulk-progress-nums", "{progress_done} / {progress_total}" }
                                div { class: "bulk-progress-note", "{progress_current}" }
                            }
                        } else if let BulkState::Done { done, errors } = state_snap {
                            div { class: "bulk-status-done",
                                if errors == 0 {
                                    "✓ Done — {done} notes updated."
                                } else {
                                    "✓ Done — {done} updated, {errors} failed (check logs)."
                                }
                            }
                        }
                    }
                }
                footer { class: "settings-footer",
                    button { class: "btn", onclick: close, "Close" }
                    button {
                        class: "btn btn-primary",
                        disabled: running || state.read().vault.is_none(),
                        onclick: move |_| {
                            let Some(vault) = state.read().vault.clone() else { return };
                            let claude_binary = settings.read().claude_binary.clone();
                            let op_val = op.read().clone();
                            let skip = *skip_existing.read();

                            // Collect (title, absolute_path) for notes to process.
                            let notes: Vec<(String, std::path::PathBuf)> = vault
                                .notes
                                .iter()
                                .filter_map(|n| {
                                    let path = n.absolute_path.as_std_path().to_path_buf();
                                    if skip {
                                        let content =
                                            std::fs::read_to_string(&path).unwrap_or_default();
                                        let (yaml, _) = fm::split_raw(&content);
                                        let existing = match &op_val {
                                            BulkOp::Summarize => fm::get_field(&yaml, "summary"),
                                            BulkOp::Tags => {
                                                let t = fm::get_field(&yaml, "tags");
                                                if t == "[]" { String::new() } else { t }
                                            }
                                        };
                                        if !existing.is_empty() {
                                            return None;
                                        }
                                    }
                                    Some((n.title.clone(), path))
                                })
                                .collect();

                            let total = notes.len();
                            if total == 0 {
                                bulk_state.set(BulkState::Done { done: 0, errors: 0 });
                                return;
                            }
                            bulk_state.set(BulkState::Running {
                                current: String::new(),
                                done: 0,
                                total,
                            });

                            spawn(async move {
                                let mut done = 0usize;
                                let mut errors = 0usize;

                                for (title, path) in notes {
                                    bulk_state.set(BulkState::Running {
                                        current: title.clone(),
                                        done,
                                        total,
                                    });

                                    let binary = claude_binary.clone();
                                    let title_c = title.clone();
                                    let path_c = path.clone();
                                    let op_c = op_val.clone();

                                    let result = tokio::task::spawn_blocking(
                                        move || -> anyhow::Result<()> {
                                            let content =
                                                std::fs::read_to_string(&path_c)?;
                                            let (_, body) = fm::split_raw(&content);
                                            let snippet: String =
                                                body.chars().take(1500).collect();

                                            let prompt = match &op_c {
                                                BulkOp::Summarize => format!(
                                                    "Summarize the following note titled \"{title_c}\" in 1-2 sentences. Return ONLY the summary text, no labels or extra formatting.\n\n{snippet}"
                                                ),
                                                BulkOp::Tags => format!(
                                                    "Suggest 3-5 tags for the note titled \"{title_c}\". Return ONLY a comma-separated list of lowercase tags, nothing else.\n\n{snippet}"
                                                ),
                                            };
                                            let field = match &op_c {
                                                BulkOp::Summarize => "summary",
                                                BulkOp::Tags => "tags",
                                            };

                                            let mut cmd =
                                                std::process::Command::new(&binary);
                                            cmd.args(["-p", &prompt])
                                                .stdout(Stdio::piped())
                                                .stderr(Stdio::null());
                                            #[cfg(windows)]
                                            {
                                                use std::os::windows::process::CommandExt;
                                                cmd.creation_flags(0x08000000);
                                            }
                                            let out = cmd.output()?;
                                            let response = String::from_utf8_lossy(
                                                &out.stdout,
                                            )
                                            .trim()
                                            .to_string();
                                            if response.is_empty() {
                                                return Err(anyhow::anyhow!(
                                                    "empty response from claude"
                                                ));
                                            }

                                            let updated_content =
                                                fm::update_field(&content, field, &response);
                                            std::fs::write(&path_c, updated_content.as_bytes())?;
                                            Ok(())
                                        },
                                    )
                                    .await;

                                    match result {
                                        Ok(Ok(())) => done += 1,
                                        Ok(Err(e)) => {
                                            tracing::warn!("bulk op failed for {title}: {e}");
                                            errors += 1;
                                        }
                                        Err(e) => {
                                            tracing::warn!("bulk op task panic for {title}: {e}");
                                            errors += 1;
                                        }
                                    }
                                }

                                bulk_state.set(BulkState::Done { done, errors });
                            });
                        },
                        if running { "Running…" } else { "Run" }
                    }
                }
            }
        }
    }
}
