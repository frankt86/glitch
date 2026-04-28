# Glitch

Pure-Rust AI-native markdown knowledge base. Named for [the keytool from ReBoot](https://reboot.fandom.com/wiki/Glitch) — a tool that shapeshifts into whatever you need.

## Status

Active development. Core editing, tables, git sync, slash commands, and git history are working.

## Features

- **WYSIWYG editor** — TipTap (ProseMirror) embedded in WebView2; tables render inline in the document like Notion, not in a separate pane
- **Glitch tables** — sortable/filterable typed data tables (text, number, date, checkbox, select, formula) embedded as ` ```glitch-table ``` ` fenced blocks in plain markdown; add columns via dropdown, add/delete rows inline
- **AI chat** — streams from the [Claude Code CLI](https://docs.claude.com/claude-code) in `--output-format stream-json` mode; full agent loop with tool-use cards, thinking blocks, and interrupt support
- **Slash commands** — `/note`, `/explain`, `/extract`, `/connect`, `/table`, `/help` available in both the editor and the chat input
- **GitHub sync** — auto-commit, push/pull, conflict surface via the system `git` CLI
- **Git history** — per-note commit list with inline side-by-side diff and read-only restore
- **Collapsible vault sidebar** — folder tree with frontmatter icons and note types
- **Source view** — raw markdown textarea toggle alongside the WYSIWYG editor
- **Note types & templates** — type registry in `%APPDATA%\Glitch\types.toml` with `{{title}}` / `{{date}}` placeholders

## Architecture

| Layer | Choice |
|---|---|
| UI framework | Dioxus 0.6 desktop (WebView2 on Windows) |
| Editor widget | TipTap v2 in an iframe via custom protocol |
| AI subprocess | Claude Code CLI · `stream-json` NDJSON over stdout |
| Markdown | `pulldown-cmark` (render) · `tiptap-markdown` (editor) |
| Table formulas | `rhai` scripting engine |
| Git sync | system `git` CLI subprocess |
| State / DB | `rusqlite` (app state) · plain `.md` files (vault) |

The vault is plain markdown so it stays portable — VS Code and Obsidian can open it. Glitch tables degrade gracefully to fenced code blocks in other editors.

## Build

```sh
cargo run -p glitch
```

Requires:
- **Rust stable** (see `rust-toolchain.toml`)
- **`claude` CLI** on PATH for AI features (`npm install -g @anthropic-ai/claude-code`)
- **`git`** on PATH for vault sync

## Vault format

Notes are plain markdown with optional YAML frontmatter:

```yaml
---
title: My note
type: meeting
tags: [rust, dioxus]
related: [other-note.md]
---
```

Tables are stored inline as fenced blocks that degrade to readable JSON in other editors:

````
```glitch-table
{
  "schema": {
    "columns": [
      { "name": "Task", "type": "text" },
      { "name": "Done", "type": "checkbox" },
      { "name": "Hours", "type": "number" }
    ]
  },
  "rows": [
    ["Write docs", true, 2]
  ]
}
```
````

## Roadmap

- [x] M1 — Vault loader, Dioxus shell, Claude streaming chat
- [x] M2 — GitHub sync (git CLI wrapper)
- [x] M2.5 — Slash commands, folder tree, frontmatter
- [x] M2.75 — Note types, templates, tool approval modal
- [x] M2.8 — Per-note git history with diff view
- [x] M2.85 — Universal slash-command palette (editor + chat)
- [x] M3 — TipTap WYSIWYG editor with inline glitch-table NodeViews
- [x] M4 — Interactive table grid (sort, filter, typed columns, formulas)
- [ ] M5 — Embeddings + AI-suggested connections (`fastembed`)
- [ ] M6 — Graph view (petgraph + fdg, typed edges)
- [ ] M7 — Article extractor (`dom_smoothie` + `htmd`)
- [ ] M8 — MSIX packaging (`winappCli`)
