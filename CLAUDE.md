# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo run -p glitch              # dev run
cargo build --release -p glitch  # release binary → target/release/glitch.exe
cargo test --workspace           # all tests
cargo clippy                     # lint
cargo fmt                        # format
```

No Makefile or justfile — all tasks are plain Cargo.

## Crate Structure

```
app/glitch/         # Dioxus 0.6 desktop app (entry point)
crates/
  glitch-core       # Vault + Note model, markdown parsing, glitch-table format, file watcher
  glitch-ai         # Spawns and manages the `claude` CLI subprocess (NDJSON stdio)
  glitch-sync       # Async wrapper around system `git` CLI
  glitch-mcp        # MCP permission-prompt server via Windows named pipe
  glitch-embed      # BGE-small-en-v1.5 embeddings (~130 MB, run once), cosine similarity search
```

## Key Architecture

**App entry** (`app/glitch/src/main.rs`): Detects `--mcp-permission-server <pipe>` early to run as MCP subprocess, otherwise launches the Dioxus window. WebView2 data dir is set to `%LOCALAPPDATA%\Glitch\WebView2` so installed builds can write to it.

**Root component** (`app/glitch/src/components/app.rs`): Sets up all Dioxus signals (vault state, chat history, sync state, pending permission approvals) and spawns four long-lived coroutines: `chat_coroutine`, `sync_coroutine`, `watch_coroutine`, and the permission server.

**AI subprocess** (`crates/glitch-ai/src/client.rs`): Spawns `claude -p --output-format stream-json --input-format stream-json` with piped stdio. Prompts are written as JSON lines to stdin; responses are parsed NDJSON `StreamEvent` variants. The currently open note is included in each prompt.

**Git sync** (`crates/glitch-sync/src/lib.rs`): Shells out to the system `git` binary — intentionally, so credential helpers (PAT, SSH, Windows Credential Manager, `gh` CLI) handle auth. `sync()` does pull → commit → push. Parses `git status --porcelain=v1 -b` into a `SyncStatus` struct.

**Permission server** (`crates/glitch-mcp` + `app/glitch/src/permissions.rs`): Claude is invoked with `--permission-prompt-tool mcp__glitch_permissions__approve`. Glitch spawns itself as a subprocess to host the MCP tool over a Windows named pipe (`\\.\pipe\glitch-perm-{pid}-{ulid}`). Each tool-call approval request is forwarded to the main app, which shows a modal; the user's Allow/Deny is sent back.

**Table engine** (`crates/glitch-core/src/table.rs`): `glitch-table` fenced blocks contain inline JSON with `schema` (typed columns) and `rows`. Formulas are evaluated by a `rhai::Engine`. Column types: Text, Number (plain/money/percent), Date, Checkbox, Select, Formula.

**Embeddings** (`crates/glitch-embed/src/lib.rs`): Synchronous API — call via `tokio::task::spawn_blocking`. Stores embeddings in `.glitch/embeddings.json` (HashMap of rel_path → `Vec<f32>`).

## Vault Format

Plain `.md` files with optional YAML frontmatter. `glitch-table` fenced blocks degrade to readable JSON in any other editor. Agent/MCP/permission config lives in `%APPDATA%\Glitch\`, not in the synced vault.

## Packaging

- `packaging/inno/glitch.iss` → Inno Setup installer (`build/GlitchSetup.exe`)
- `packaging/msix/build-msix.ps1` → MSIX package (`build/glitch.msix`)
- CI (`.github/workflows/build.yml`): test → build → package → attach to GitHub Release on tag push
