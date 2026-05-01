use crate::extract;
use crate::state::AppState;
use dioxus::prelude::*;
use glitch_core::NoteId;

#[derive(Clone, PartialEq)]
enum FetchState {
    Idle,
    Fetching,
    Done { title: String, rel_path: String },
    Failed(String),
}

#[component]
pub fn ExtractorDialog(
    visible: Signal<bool>,
    state: Signal<AppState>,
) -> Element {
    if !*visible.read() {
        return rsx! { Fragment {} };
    }

    let mut url_input = use_signal(String::new);
    let mut fetch_state = use_signal(|| FetchState::Idle);

    use_effect(move || {
        if *visible.read() {
            url_input.set(String::new());
            fetch_state.set(FetchState::Idle);
        }
    });

    let close = move |_| visible.set(false);

    // Shared extraction logic — called by both button click and Enter key.
    let mut do_fetch = move || {
        let url = url_input.read().trim().to_string();
        if url.is_empty() {
            return;
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            fetch_state.set(FetchState::Failed(
                "URL must start with http:// or https://".into(),
            ));
            return;
        }
        let Some(vault_root) = state.read().vault.as_ref().map(|v| v.root.clone()) else {
            fetch_state.set(FetchState::Failed("open a vault first".into()));
            return;
        };
        fetch_state.set(FetchState::Fetching);
        let mut fetch_state = fetch_state;
        let mut state = state;
        spawn(async move {
            match extract::extract_to_vault(&url, &vault_root).await {
                Ok(note) => {
                    let rel = note.relative_path.to_string();
                    let title = note.title.clone();
                    let content =
                        match tokio::fs::read_to_string(note.absolute_path.as_std_path()).await {
                            Ok(c) => c,
                            Err(_) => String::new(),
                        };
                    let id = NoteId(note.relative_path);
                    let mut s = state.write();
                    s.current_note = Some(id);
                    s.editor_content = content;
                    s.editor_dirty = false;
                    drop(s);
                    fetch_state.set(FetchState::Done { title, rel_path: rel });
                }
                Err(err) => {
                    fetch_state.set(FetchState::Failed(err.to_string()));
                }
            }
        });
    };

    let busy = matches!(*fetch_state.read(), FetchState::Fetching);
    let done = matches!(*fetch_state.read(), FetchState::Done { .. });

    rsx! {
        div { class: "modal-overlay", onclick: close,
            div { class: "extractor-card", onclick: move |e| e.stop_propagation(),
                header { class: "extractor-header",
                    h2 { "Extract article" }
                    button { class: "btn-link", onclick: close, "Close" }
                }
                div { class: "extractor-body",
                    p { class: "extractor-hint",
                        "Paste any article URL — Glitch fetches the readable content and saves it as a note in "
                        span { class: "mono", "articles/" }
                        "."
                    }
                    div { class: "extractor-row",
                        input {
                            class: "extractor-url-input",
                            r#type: "url",
                            placeholder: "https://example.com/article",
                            value: "{url_input.read()}",
                            disabled: busy,
                            oninput: move |e: FormEvent| url_input.set(e.value()),
                            onkeydown: {
                                let mut do_fetch = do_fetch;
                                move |e: KeyboardEvent| {
                                    if e.key() == Key::Enter {
                                        do_fetch();
                                    }
                                }
                            },
                        }
                        button {
                            class: if busy { "btn btn-primary btn-busy" } else { "btn btn-primary" },
                            disabled: busy,
                            onclick: move |_| do_fetch(),
                            if busy { "Fetching…" } else { "Fetch" }
                        }
                    }
                    match &*fetch_state.read() {
                        FetchState::Idle => rsx! { Fragment {} },
                        FetchState::Fetching => rsx! {
                            div { class: "extractor-status extractor-busy",
                                span { class: "extractor-spinner" }
                                "Fetching article…"
                            }
                        },
                        FetchState::Done { title, rel_path } => rsx! {
                            div { class: "extractor-status extractor-ok",
                                span { class: "extractor-ok-icon", "✓" }
                                div {
                                    div { class: "extractor-ok-title", "{title}" }
                                    div { class: "extractor-ok-path mono", "{rel_path}" }
                                }
                            }
                        },
                        FetchState::Failed(msg) => rsx! {
                            div { class: "extractor-status extractor-err",
                                span { class: "extractor-err-icon", "✗" }
                                "{msg}"
                            }
                        },
                    }
                }
                footer { class: "extractor-footer",
                    if done {
                        button { class: "btn btn-primary", onclick: close, "Open note" }
                    } else {
                        button { class: "btn", disabled: busy, onclick: close, "Cancel" }
                    }
                }
            }
        }
    }
}
