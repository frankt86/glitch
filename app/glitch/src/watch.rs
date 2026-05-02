//! Vault watcher coroutine: keeps a `notify` watcher alive for the current
//! vault root and reloads the in-memory `Vault` (debounced) whenever
//! markdown files appear, change, or disappear.

use crate::state::AppState;
use camino::Utf8PathBuf;
use dioxus::prelude::*;
use futures::StreamExt;
use glitch_core::{Vault, VaultEvent};
use notify::RecommendedWatcher;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver as TokioUnbounded;

const DEBOUNCE: Duration = Duration::from_millis(200);

/// `commands` is dioxus's futures-channel receiver (from `use_coroutine`).
/// Internally we hold a tokio receiver from `glitch_core::watch_vault`, since
/// `notify`'s callback writes to it synchronously.
#[allow(unused_assignments, unused_variables)]
pub async fn watch_coroutine(
    mut commands: UnboundedReceiver<Utf8PathBuf>,
    mut app_state: Signal<AppState>,
) {
    let mut watcher: Option<RecommendedWatcher> = None;
    let mut events: Option<TokioUnbounded<VaultEvent>> = None;
    let mut root: Option<Utf8PathBuf> = None;

    loop {
        tokio::select! {
            biased;
            cmd = commands.next() => {
                let Some(new_root) = cmd else { return; };
                watcher = None;
                events = None;
                root = None;
                match glitch_core::watch_vault(&new_root) {
                    Ok((w, e)) => {
                        watcher = Some(w);
                        events = Some(e);
                        root = Some(new_root);
                    }
                    Err(err) => tracing::error!("watcher failed to start: {err}"),
                }
            }
            evt = next_event(&mut events) => {
                if evt.is_none() {
                    continue;
                }
                tokio::time::sleep(DEBOUNCE).await;
                if let Some(e) = events.as_mut() {
                    while e.try_recv().is_ok() {}
                }
                if let Some(r) = root.as_ref() {
                    let r = r.clone();
                    match tokio::task::spawn_blocking(move || Vault::load(&r)).await {
                        Ok(Ok(v)) => {
                            app_state.write().vault = Some(v);
                        }
                        Ok(Err(err)) => tracing::warn!("vault reload failed: {err}"),
                        Err(err) => tracing::warn!("vault reload panicked: {err}"),
                    }
                }
            }
        }
    }
    // unreachable: prevents "watcher unused" warnings
    #[allow(unreachable_code)]
    drop((watcher, events, root));
}

async fn next_event(opt: &mut Option<TokioUnbounded<VaultEvent>>) -> Option<VaultEvent> {
    match opt.as_mut() {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}
