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
const SELF_SAVE_WINDOW: Duration = Duration::from_secs(2);

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
                let Some(first) = evt else { continue };
                tokio::time::sleep(DEBOUNCE).await;
                // Collect all pending events including the first.
                let mut changed: Vec<Utf8PathBuf> = vec![event_path(&first)];
                if let Some(e) = events.as_mut() {
                    while let Ok(extra) = e.try_recv() {
                        changed.push(event_path(&extra));
                    }
                }
                // Skip the vault reload if every changed file was written by
                // Glitch itself within the last SELF_SAVE_WINDOW seconds.
                {
                    let snap = app_state.read();
                    if let Some((saved_path, saved_at)) = &snap.last_self_save {
                        if saved_at.elapsed() < SELF_SAVE_WINDOW
                            && changed.iter().all(|p| p == saved_path)
                        {
                            continue;
                        }
                    }
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

fn event_path(evt: &VaultEvent) -> Utf8PathBuf {
    match evt {
        VaultEvent::Created(p) | VaultEvent::Modified(p) | VaultEvent::Removed(p) => p.clone(),
        VaultEvent::Renamed { to, .. } => to.clone(),
    }
}

async fn next_event(opt: &mut Option<TokioUnbounded<VaultEvent>>) -> Option<VaultEvent> {
    match opt.as_mut() {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}
