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
use std::borrow::Cow;

const STYLES: &str = include_str!("../assets/styles.css");
const TIPTAP_HTML: &[u8] = include_bytes!("../assets/tiptap-editor.html");
const APP_ICON_PNG: &[u8] = include_bytes!("../assets/glitch_icon_512.png");

fn load_window_icon() -> Option<dioxus::desktop::tao::window::Icon> {
    let img = image::load_from_memory_with_format(APP_ICON_PNG, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (w, h) = img.dimensions();
    dioxus::desktop::tao::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

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
        .with_inner_size(dioxus::desktop::LogicalSize::new(1280.0, 800.0))
        .with_window_icon(load_window_icon());

    // WebView2 needs a writable user-data folder. When installed to
    // Program Files the exe directory is read-only, so point it at
    // %LOCALAPPDATA%\Glitch\WebView2 which is always user-writable.
    let webview_data_dir = dirs_next::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Glitch")
        .join("WebView2");

    LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_window(window)
                .with_data_directory(webview_data_dir)
                .with_custom_head(format!(
                    "<style>{STYLES}</style>\
                     <script>\
                     window.__glitch_drag=null;\
                     window.__glitch_drop_id=null;\
                     document.addEventListener('dragstart',function(e){{\
                         window.__glitch_drop_id=null;\
                         var el=e.target&&e.target.closest?e.target.closest('[data-note-id]'):null;\
                         window.__glitch_drag=el?el.getAttribute('data-note-id'):null;\
                     }},true);\
                     document.addEventListener('dragend',function(){{\
                         window.__glitch_drag=null;\
                     }},true);\
                     document.addEventListener('dragenter',function(e){{\
                         e.preventDefault();\
                     }},true);\
                     document.addEventListener('dragover',function(e){{\
                         e.preventDefault();\
                         if(e.dataTransfer)e.dataTransfer.dropEffect='move';\
                     }},true);\
                     document.addEventListener('drop',function(e){{\
                         e.preventDefault();\
                         window.__glitch_drop_id=window.__glitch_drag;\
                         window.__glitch_drag=null;\
                     }},true);\
                     </script>"
                ))
                .with_custom_protocol("glitch-editor", |_req| {
                    dioxus::desktop::wry::http::Response::builder()
                        .header("Content-Type", "text/html; charset=utf-8")
                        .body(Cow::Borrowed(TIPTAP_HTML))
                        .unwrap()
                }),
        )
        .launch(App);
}
