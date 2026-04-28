#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod chat;
mod commands;
mod components;
mod extract;
mod permissions;
mod render;
mod settings;
mod state;
mod sync;
mod types;
mod vault_actions;
mod watch;

use components::App;
use dioxus::desktop::{Config, WindowBuilder};
use dioxus::prelude::*;

const STYLES: &str = include_str!("../assets/styles.css");

fn main() {
    // When Claude Code re-invokes us as the MCP permission server, divert
    // BEFORE doing anything Dioxus-related. stdio is reserved for JSON-RPC.
    let mut args = std::env::args().skip(1);
    if let Some(first) = args.next() {
        if first == "--mcp-permission-server" {
            let pipe_name = args.next().unwrap_or_else(|| {
                eprintln!("--mcp-permission-server requires <pipe-name>");
                std::process::exit(2);
            });
            // Send tracing to stderr only — stdout is reserved for MCP framing.
            let _ = tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
                )
                .try_init();
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio rt");
            let result = rt.block_on(glitch_mcp::run_permission_stdio(&pipe_name));
            if let Err(err) = result {
                tracing::error!("permission server exited with error: {err}");
                std::process::exit(1);
            }
            return;
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,glitch=debug")),
        )
        .init();

    let window = WindowBuilder::new()
        .with_title("Glitch")
        .with_inner_size(dioxus::desktop::LogicalSize::new(1280.0, 800.0));

    LaunchBuilder::desktop()
        .with_cfg(Config::new().with_window(window).with_custom_head(format!(
            "<style>{STYLES}</style>"
        )))
        .launch(App);
}
