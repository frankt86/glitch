# Glitch

Pure-Rust AI-native markdown knowledge base. Named for [the keytool from ReBoot](https://reboot.fandom.com/wiki/Glitch) — a tool that shapeshifts into whatever you need.

## Status

Pre-alpha.

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

## Features

- **WYSIWYG markdown editor** powered by TipTap v2 — headings, bold, italic, strike, code, blockquote, lists, dividers
- **Slash commands** — type `/` anywhere in the editor or chat to get an autocomplete palette; formatting commands apply instantly, action commands (note, extract, explain, connect) go to the AI
- **Inline data tables** — `glitch-table` fenced blocks render as interactive grids with sortable columns, typed cells (text, number, date, checkbox, select, formula), and inline add/delete; gap-cursor lets you click above or below a table to place the cursor there
- **Streaming AI chat** — Claude Code CLI runs as a subprocess in `stream-json` mode; tool-use blocks surface as expandable cards; the AI always sees which note is open
- **Frontmatter detail tab** — structured fields per note type (article, meeting, book, person, project); editable title pinned above the editor
- **Note types & templates** — built-in types: note, task, meeting, book, person, project, bible (SOAP method), sermon, prayer, daily journal, recipe, research, goal, quote; register custom types in `%APPDATA%\Glitch\types.toml`; `/note <title> --type meeting` materialises a template
- **Sidebar search** — live title search above the note tree; filters across all folders in real time; clear button to restore the tree
- **Resizable sidebar** — drag the divider between the note list and editor to set any width from 160 px to 520 px
- **Per-note git history** — commit list with side-by-side diff view; read-only restore to a new note
- **GitHub sync** — auto-commit on inactivity, manual sync button, conflict surface
- **Graph view** — full-screen force-directed layout of all notes; typed edges (wikilink, frontmatter related, hierarchy, shared keyword); filter chips; pan and zoom; click any node to open that note in the editor
- **Article extractor** — paste any URL via "Extract URL…" toolbar button or `/extract <url>`; fetches readable content via `dom_smoothie` + `htmd`, saves as a note with frontmatter (`source`, `author`, `fetched`); images embedded as base64 so notes are fully self-contained
- **CI/CD** — GitHub Actions builds and uploads `glitch.exe` on every push to `main`; attaches the binary to GitHub Releases automatically