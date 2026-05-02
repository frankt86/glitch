//! Graph view (M7) — force-directed layout with typed edges.
//!
//! Edge kinds (all togglable via filter chips):
//!   Wikilink   — explicit `[[target]]` in note body  (solid accent)
//!   Related    — frontmatter `related:` array        (dashed accent)
//!   Hierarchy  — shared parent folder               (solid muted, thin)
//!   Keyword    — shared keywords, Jaccard ≥ 0.3     (dashed muted)

use crate::state::AppState;
use dioxus::prelude::*;
use glitch_core::{Note, Vault};
use std::collections::{HashMap, HashSet};

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum EdgeKind {
    Wikilink,
    Related,
    Hierarchy,
    Keyword,
}

impl EdgeKind {
    fn stroke(self) -> &'static str {
        match self {
            Self::Wikilink  => "#7aa2f7",
            Self::Related   => "#7aa2f7",
            Self::Hierarchy => "#4a5068",
            Self::Keyword   => "#4a5068",
        }
    }
    fn stroke_width(self) -> &'static str {
        match self {
            Self::Hierarchy => "0.5",
            _ => "1",
        }
    }
    fn stroke_dasharray(self) -> &'static str {
        match self {
            Self::Related  => "4 2",
            Self::Keyword  => "2 3",
            _ => "none",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct GNode {
    id: String,    // vault-relative path
    title: String,
}

#[derive(Clone, Debug, PartialEq)]
struct GEdge {
    from: usize,
    to: usize,
    kind: EdgeKind,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct GraphData {
    nodes: Vec<GNode>,
    edges: Vec<GEdge>,
}

// ── Graph construction ────────────────────────────────────────────────────────

fn build_graph(vault: &Vault) -> GraphData {
    let notes = &vault.notes;
    let nodes: Vec<GNode> = notes
        .iter()
        .map(|n| GNode { id: n.id.as_str().to_string(), title: n.title.clone() })
        .collect();

    let idx: HashMap<&str, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let mut edges: Vec<GEdge> = Vec::new();
    let mut seen: HashSet<(usize, usize, u8)> = HashSet::new();

    let mut push = |edges: &mut Vec<GEdge>, from: usize, to: usize, kind: EdgeKind| {
        let key = (from.min(to), from.max(to), kind as u8);
        if seen.insert(key) {
            edges.push(GEdge { from, to, kind });
        }
    };

    for (i, note) in notes.iter().enumerate() {
        // ── Wikilinks ────────────────────────────────────────────────────────
        if let Ok(content) = note.read_content() {
            for target in extract_wikilinks(&content) {
                if let Some(&j) = find_by_name(&idx, notes, &target) {
                    if j != i { push(&mut edges, i, j, EdgeKind::Wikilink); }
                }
            }
        }

        // ── Frontmatter `related:` ────────────────────────────────────────
        for rel in &note.frontmatter.related {
            if let Some(&j) = idx.get(rel.as_str())
                .or_else(|| idx.get(format!("{rel}.md").as_str()))
            {
                if j != i { push(&mut edges, i, j, EdgeKind::Related); }
            }
        }

        // ── Hierarchy (shared folder) ─────────────────────────────────────
        let par = note.id.0.parent().and_then(|p| {
            let s = p.as_str();
            if s.is_empty() { None } else { Some(s.to_string()) }
        });
        if let Some(ref par_str) = par {
            for (j, other) in notes.iter().enumerate() {
                if j <= i { continue; }
                let other_par = other.id.0.parent().map(|p| p.as_str().to_string());
                if other_par.as_deref() == Some(par_str.as_str()) {
                    push(&mut edges, i, j, EdgeKind::Hierarchy);
                }
            }
        }

        // ── Keyword (Jaccard ≥ 0.3) ───────────────────────────────────────
        if !note.keywords.is_empty() {
            for (j, other) in notes.iter().enumerate() {
                if j <= i || other.keywords.is_empty() { continue; }
                let inter = note.keywords.iter().filter(|k| other.keywords.contains(k)).count();
                let union = note.keywords.len() + other.keywords.len() - inter;
                if inter > 0 && inter as f32 / union as f32 >= 0.3 {
                    push(&mut edges, i, j, EdgeKind::Keyword);
                }
            }
        }
    }

    GraphData { nodes, edges }
}

fn extract_wikilinks(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = content.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let start = i + 2;
            let mut j = start;
            while j + 1 < bytes.len() && !(bytes[j] == b']' && bytes[j + 1] == b']') {
                j += 1;
            }
            if j + 1 < bytes.len() {
                let inner = &content[start..j];
                let name = inner.split('|').next().unwrap_or(inner).trim().to_string();
                if !name.is_empty() { out.push(name); }
                i = j + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn find_by_name<'a>(
    idx: &'a HashMap<&str, usize>,
    notes: &[Note],
    name: &str,
) -> Option<&'a usize> {
    let name_lower = name.to_lowercase();
    idx.get(name)
        .or_else(|| idx.get(format!("{name}.md").as_str()))
        .or_else(|| {
            // file-stem match (case-insensitive)
            let pos = notes.iter().position(|n| {
                n.id.0.file_stem().map(|s| s.to_lowercase()) == Some(name_lower.clone())
            })?;
            // SAFETY: position came from iterating notes, idx has all notes
            idx.get(notes[pos].id.as_str())
        })
}

// ── Force-directed layout (Fruchterman–Reingold) ──────────────────────────────

fn layout(n: usize, edges: &[(usize, usize)], iterations: u32) -> Vec<(f32, f32)> {
    use std::f32::consts::PI;
    if n == 0 { return vec![]; }
    if n == 1 { return vec![(0.0, 0.0)]; }

    // k: ideal node spacing; init_r: circle radius so adjacent nodes start ~k apart
    let k = (1200.0f32 / n as f32).sqrt();
    let init_r = k * (n as f32 / (2.0 * PI)).sqrt();
    let t_max = k * (n as f32).sqrt() * 0.5;

    let mut pos: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let a = i as f32 * 2.0 * PI / n as f32;
            (a.cos() * init_r, a.sin() * init_r)
        })
        .collect();

    for iter in 0..iterations {
        let t = t_max * (1.0 - iter as f32 / iterations as f32).max(0.01);
        let mut disp = vec![(0.0f32, 0.0f32); n];

        // Repulsion
        for i in 0..n {
            for j in 0..n {
                if i == j { continue; }
                let dx = pos[i].0 - pos[j].0;
                let dy = pos[i].1 - pos[j].1;
                let d = (dx * dx + dy * dy).sqrt().max(0.001);
                let f = k * k / d;
                disp[i].0 += dx / d * f;
                disp[i].1 += dy / d * f;
            }
        }
        // Attraction (linear spring — keeps linked notes near each other
        // without collapsing them into a single point)
        for &(u, v) in edges {
            let dx = pos[u].0 - pos[v].0;
            let dy = pos[u].1 - pos[v].1;
            let d = (dx * dx + dy * dy).sqrt().max(0.001);
            let f = d / k;
            let (fx, fy) = (dx / d * f, dy / d * f);
            disp[u].0 -= fx; disp[u].1 -= fy;
            disp[v].0 += fx; disp[v].1 += fy;
        }
        // Apply with cooling
        for i in 0..n {
            let d = (disp[i].0 * disp[i].0 + disp[i].1 * disp[i].1).sqrt().max(0.001);
            let clamped = d.min(t);
            pos[i].0 += disp[i].0 / d * clamped;
            pos[i].1 += disp[i].1 / d * clamped;
        }
    }

    // Normalise: centre around zero
    let cx = pos.iter().map(|p| p.0).sum::<f32>() / n as f32;
    let cy = pos.iter().map(|p| p.1).sum::<f32>() / n as f32;
    pos.iter().map(|&(x, y)| (x - cx, y - cy)).collect()
}

/// Scale factor so the layout fits inside a viewport of `vw × vh` with `margin` padding.
fn fit_scale(positions: &[(f32, f32)], vw: f32, vh: f32, margin: f32) -> f32 {
    if positions.len() < 2 { return 20.0; }
    let (min_x, max_x) = positions.iter().fold((f32::MAX, f32::MIN), |(mn, mx), &(x, _)| (mn.min(x), mx.max(x)));
    let (min_y, max_y) = positions.iter().fold((f32::MAX, f32::MIN), |(mn, mx), &(_, y)| (mn.min(y), mx.max(y)));
    let span_x = (max_x - min_x).max(0.001);
    let span_y = (max_y - min_y).max(0.001);
    ((vw - 2.0 * margin) / span_x).min((vh - 2.0 * margin) / span_y)
}

// ── Component ─────────────────────────────────────────────────────────────────

const SVG_W: f32 = 1300.0;
const SVG_H: f32 = 700.0;
const MARGIN: f32 = 80.0;
const NODE_R: f32 = 7.0;

#[component]
pub fn GraphView(visible: Signal<bool>, state: Signal<AppState>) -> Element {
    if !*visible.read() {
        return rsx! { Fragment {} };
    }

    // ── Filter chips ─────────────────────────────────────────────────────────
    let mut show_wikilink  = use_signal(|| true);
    let mut show_related   = use_signal(|| true);
    let mut show_hierarchy = use_signal(|| false);
    let mut show_keyword   = use_signal(|| false);

    // ── Pan / zoom state ─────────────────────────────────────────────────────
    let mut pan_x      = use_signal(|| 0.0f32);
    let mut pan_y      = use_signal(|| 0.0f32);
    let mut zoom       = use_signal(|| 1.0f32);
    let mut dragging   = use_signal(|| false);
    let mut last_mx    = use_signal(|| 0.0f32);
    let mut last_my    = use_signal(|| 0.0f32);
    let mut down_mx    = use_signal(|| 0.0f32);  // mousedown origin for drag threshold
    let mut down_my    = use_signal(|| 0.0f32);

    // ── Build graph (memoised — recomputes only when vault changes) ───────────
    let graph = use_memo(move || {
        state.read().vault.as_ref().map(|v| build_graph(v)).unwrap_or_default()
    });

    // ── Layout (runs once per graph, synchronously; fast for < 300 notes) ────
    // Only use intentional link edges (Wikilink/Related) for positioning.
    // Hierarchy edges are O(n²) in dense folders and collapse everything together.
    let positions = use_memo(move || {
        let g = graph.read();
        let edge_pairs: Vec<(usize, usize)> = g.edges.iter()
            .filter(|e| matches!(e.kind, EdgeKind::Wikilink | EdgeKind::Related))
            .map(|e| (e.from, e.to))
            .collect();
        layout(g.nodes.len(), &edge_pairs, 300)
    });

    let g = graph.read();
    let pos = positions.read();
    let base_scale = fit_scale(&pos, SVG_W, SVG_H, MARGIN);
    let scale = base_scale * *zoom.read();
    let cx = SVG_W / 2.0 + *pan_x.read();
    let cy = SVG_H / 2.0 + *pan_y.read();
    let transform_str = format!("translate({cx},{cy}) scale({scale})");

    // ── Pre-compute render lists (avoids complex exprs inside rsx!) ──────────
    let active_kinds: HashSet<EdgeKind> = {
        let mut s = HashSet::new();
        if *show_wikilink.read()  { s.insert(EdgeKind::Wikilink); }
        if *show_related.read()   { s.insert(EdgeKind::Related); }
        if *show_hierarchy.read() { s.insert(EdgeKind::Hierarchy); }
        if *show_keyword.read()   { s.insert(EdgeKind::Keyword); }
        s
    };

    // Edge render list: (x1, y1, x2, y2, stroke, stroke_width, dasharray)
    let render_edges: Vec<(f32, f32, f32, f32, &'static str, &'static str, &'static str)> =
        g.edges.iter()
            .filter(|e| active_kinds.contains(&e.kind))
            .filter_map(|e| {
                let &(x1, y1) = pos.get(e.from)?;
                let &(x2, y2) = pos.get(e.to)?;
                let k = e.kind;
                Some((x1, y1, x2, y2, k.stroke(), k.stroke_width(), k.stroke_dasharray()))
            })
            .collect();

    // Node render list: (cx, cy, label, rel_path, label_y, font_size, node_r)
    let nr = NODE_R / scale;
    let nfont = (10.0f32 / scale).max(0.5);
    let render_nodes: Vec<(f32, f32, String, String)> = g.nodes.iter().enumerate()
        .filter_map(|(i, node)| {
            let &(nx, ny) = pos.get(i)?;
            let label = if node.title.chars().count() > 18 {
                let s: String = node.title.chars().take(17).collect();
                format!("{s}…")
            } else {
                node.title.clone()
            };
            Some((nx, ny, label, node.id.clone()))
        })
        .collect();

    let cursor_style = if *dragging.read() { "display:block;cursor:grabbing" } else { "display:block;cursor:grab" };
    let node_count = g.nodes.len();
    let is_empty = node_count == 0;
    let close = move |_| visible.set(false);

    rsx! {
        div { class: "graph-card",

            // ── Header ───────────────────────────────────────────────────
            header { class: "graph-header",
                    h2 { class: "graph-title", "Graph" }
                    div { class: "graph-filters",
                        // Each chip is a toggle button
                        button {
                            class: if *show_wikilink.read() { "graph-chip active" } else { "graph-chip" },
                            onclick: move |_| { let v = *show_wikilink.read(); show_wikilink.set(!v); },
                            span { class: "chip-dot chip-wikilink" }
                            "Wikilinks"
                        }
                        button {
                            class: if *show_related.read() { "graph-chip active" } else { "graph-chip" },
                            onclick: move |_| { let v = *show_related.read(); show_related.set(!v); },
                            span { class: "chip-dot chip-related" }
                            "Related"
                        }
                        button {
                            class: if *show_hierarchy.read() { "graph-chip active" } else { "graph-chip" },
                            onclick: move |_| { let v = *show_hierarchy.read(); show_hierarchy.set(!v); },
                            span { class: "chip-dot chip-hierarchy" }
                            "Hierarchy"
                        }
                        button {
                            class: if *show_keyword.read() { "graph-chip active" } else { "graph-chip" },
                            onclick: move |_| { let v = *show_keyword.read(); show_keyword.set(!v); },
                            span { class: "chip-dot chip-keyword" }
                            "Keywords"
                        }
                        span { class: "graph-count", "{g.nodes.len()} notes" }
                    }
                    button { class: "btn-link graph-close", onclick: close, "×" }
                }

            // ── SVG ──────────────────────────────────────────────────────
            svg {
                class: "graph-svg",
                view_box: "0 0 {SVG_W} {SVG_H}",
                style: "{cursor_style}",

                // Pan/zoom event capture rect
                rect {
                    x: "0", y: "0",
                    width: "100%", height: "100%",
                        fill: "transparent",
                        onmousedown: move |evt| {
                            let c = evt.data().client_coordinates();
                            let mx = c.x as f32;
                            let my = c.y as f32;
                            last_mx.set(mx);
                            last_my.set(my);
                            down_mx.set(mx);
                            down_my.set(my);
                            dragging.set(true);
                        },
                        onmousemove: move |evt| {
                            if !*dragging.read() { return; }
                            let c = evt.data().client_coordinates();
                            let mx = c.x as f32;
                            let my = c.y as f32;
                            // Only pan if moved more than 4px from the mousedown origin
                            let dx = mx - *down_mx.read();
                            let dy = my - *down_my.read();
                            if dx * dx + dy * dy < 16.0 { return; }
                            let old_px = *pan_x.read();
                            let old_py = *pan_y.read();
                            let old_lx = *last_mx.read();
                            let old_ly = *last_my.read();
                            pan_x.set(old_px + mx - old_lx);
                            pan_y.set(old_py + my - old_ly);
                            last_mx.set(mx);
                            last_my.set(my);
                        },
                        onmouseup:    move |_| dragging.set(false),
                        onmouseleave: move |_| dragging.set(false),
                        onwheel: move |evt| {
                            let delta = evt.data().delta().strip_units();
                            let factor = if delta.y < 0.0 { 1.1f32 } else { 0.9f32 };
                            let old_zoom = *zoom.read();
                            zoom.set((old_zoom * factor).clamp(0.1, 10.0));
                        },
                    }

                    // Content group
                    g { transform: "{transform_str}",
                        // Edges (pre-computed, simple tuple iteration)
                        for (x1, y1, x2, y2, stroke, sw, dasharray) in render_edges.iter() {
                            line {
                                x1: "{x1}", y1: "{y1}",
                                x2: "{x2}", y2: "{y2}",
                                stroke: "{stroke}",
                                stroke_width: "{sw}",
                                stroke_dasharray: "{dasharray}",
                                opacity: "0.6",
                            }
                        }
                        // Nodes (pre-computed, simple tuple iteration)
                        for (nx, ny, label, rel) in render_nodes.iter() {
                            {
                                let rel2 = rel.clone();
                                let label2 = label.clone();
                                let nx2 = *nx;
                                let ny2 = *ny;
                                let ly = ny2 + nr + nfont;
                                let mut app_state = state;
                                rsx! {
                                    g {
                                        style: "cursor:pointer",
                                        onclick: move |evt| {
                                            evt.stop_propagation();
                                            let vault = app_state.read().vault.as_ref().map(|v| v.root.clone());
                                            if let Some(root) = vault {
                                                let abs = root.join(&rel2);
                                                if let Ok(content) = std::fs::read_to_string(&abs) {
                                                    let mut s = app_state.write();
                                                    s.current_note = Some(glitch_core::NoteId::from_relative(rel2.clone()));
                                                    s.editor_content = content;
                                                    s.editor_dirty = false;
                                                }
                                            }
                                            visible.set(false);
                                        },
                                        circle {
                                            cx: "{nx2}", cy: "{ny2}",
                                            r: "{nr}",
                                            fill: "#7aa2f7",
                                            stroke: "#5a7fe0",
                                            stroke_width: "{0.5 / scale}",
                                        }
                                        text {
                                            x: "{nx2}", y: "{ly}",
                                            font_size: "{nfont}",
                                            fill: "#c0c8dd",
                                            text_anchor: "middle",
                                            pointer_events: "none",
                                            "{label2}"
                                        }
                                    }
                                }
                            }
                        }
                    }

                // Empty state overlay
                if is_empty {
                    text {
                        x: "{SVG_W / 2.0}", y: "{SVG_H / 2.0}",
                        text_anchor: "middle", fill: "#4a5068", font_size: "14",
                        "No notes in vault"
                    }
                }
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_wikilinks_basic() {
        assert_eq!(extract_wikilinks("see [[note-a]] and [[note-b]]"), vec!["note-a", "note-b"]);
        assert_eq!(extract_wikilinks("[[alias|target]]"), vec!["alias"]);
        assert_eq!(extract_wikilinks("no links"), Vec::<String>::new());
    }

    #[test]
    fn layout_smoke() {
        let pos = layout(5, &[(0,1),(1,2),(2,3),(3,4)], 10);
        assert_eq!(pos.len(), 5);
        for &(x, y) in &pos {
            assert!(x.is_finite() && y.is_finite());
        }
    }
}
