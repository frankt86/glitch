//! Stub for the graph view (M7). Counts wikilinks in the loaded vault and
//! shows a placeholder explaining what the real view will deliver.

use crate::state::AppState;
use dioxus::prelude::*;
use glitch_core::Vault;

#[component]
pub fn GraphView(visible: Signal<bool>, state: Signal<AppState>) -> Element {
    if !*visible.read() {
        return rsx! { Fragment {} };
    }
    let close = move |_| visible.set(false);

    let stats = state
        .read()
        .vault
        .as_ref()
        .map(compute_stats)
        .unwrap_or_default();

    rsx! {
        div { class: "modal-overlay", onclick: close,
            div { class: "graph-card", onclick: move |e| e.stop_propagation(),
                header { class: "settings-header",
                    h2 { "Graph view" }
                    button { class: "btn-link", onclick: close, "Close" }
                }
                div { class: "graph-body",
                    div { class: "graph-stat",
                        span { class: "graph-stat-num", "{stats.notes}" }
                        span { class: "graph-stat-label", "notes" }
                    }
                    div { class: "graph-stat",
                        span { class: "graph-stat-num", "{stats.wikilinks}" }
                        span { class: "graph-stat-label", "wikilinks" }
                    }
                    div { class: "graph-stat",
                        span { class: "graph-stat-num", "{stats.frontmatter_relations}" }
                        span { class: "graph-stat-label", "frontmatter relations" }
                    }
                    div { class: "graph-stat",
                        span { class: "graph-stat-num", "{stats.tagged}" }
                        span { class: "graph-stat-label", "tagged notes" }
                    }
                }
                div { class: "graph-coming",
                    h3 { "Real graph rendering lands in M7" }
                    p {
                        "The full graph view will use `petgraph` + `fdg` (force-directed layout) "
                        "and SVG with pan/zoom. Edge kinds:"
                    }
                    ul {
                        li { "solid (accent) — explicit `[[wikilinks]]`" }
                        li { "solid (dotted) — frontmatter `related:` arrays" }
                        li { "thin neutral — shared parent folder (hierarchy)" }
                        li { "dashed muted — shared keywords (Jaccard ≥ threshold)" }
                        li { "dashed warm — AI-suggested (embeddings + Claude judgement)" }
                    }
                    p { "Toolbar chips will toggle each edge kind." }
                }
            }
        }
    }
}

#[derive(Default)]
struct Stats {
    notes: usize,
    wikilinks: usize,
    frontmatter_relations: usize,
    tagged: usize,
}

fn compute_stats(vault: &Vault) -> Stats {
    let mut wikilinks = 0usize;
    let mut frontmatter_relations = 0usize;
    let mut tagged = 0usize;
    for note in &vault.notes {
        frontmatter_relations += note.frontmatter.related.len();
        if !note.keywords.is_empty() {
            tagged += 1;
        }
        if let Ok(content) = note.read_content() {
            wikilinks += count_wikilinks(&content);
        }
    }
    Stats {
        notes: vault.notes.len(),
        wikilinks,
        frontmatter_relations,
        tagged,
    }
}

fn count_wikilinks(content: &str) -> usize {
    let mut n = 0usize;
    let bytes = content.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            // find matching ]]
            let mut j = i + 2;
            while j + 1 < bytes.len() && !(bytes[j] == b']' && bytes[j + 1] == b']') {
                j += 1;
            }
            if j + 1 < bytes.len() {
                n += 1;
                i = j + 2;
                continue;
            }
        }
        i += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_wikilinks_simple() {
        assert_eq!(count_wikilinks("see [[note-a]] and [[note-b]]"), 2);
        assert_eq!(count_wikilinks("no links here"), 0);
        assert_eq!(count_wikilinks("[[unclosed"), 0);
        assert_eq!(count_wikilinks("[[a]] [[b]] [[c]]"), 3);
    }
}
