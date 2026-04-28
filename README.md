# Glitch

Pure-Rust AI-native markdown knowledge base. Named for [the keytool from ReBoot](https://reboot.fandom.com/wiki/Glitch) — a tool that shapeshifts into whatever you need.

## Status

Pre-alpha.

## Architecture

- **UI**: Dioxus 0.6 desktop (WebView2 on Windows)
- **AI**: shells out to the [Claude Code CLI](https://docs.claude.com/claude-code) in `--output-format stream-json` mode for the full agent loop
- **Storage**: plain markdown files in a vault directory; sortable typed tables embedded as ` ```glitch-table ` fenced blocks
- **Sync**: GitHub via the system `git` CLI
- **Packaging**: Microsoft `winappCli` → MSIX

## Build

```sh
cargo run -p glitch
```

Requires the `claude` CLI on PATH for the AI features and `git` on PATH for vault sync.
