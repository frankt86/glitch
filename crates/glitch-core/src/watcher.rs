use camino::{Utf8Path, Utf8PathBuf};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum VaultEvent {
    Created(Utf8PathBuf),
    Modified(Utf8PathBuf),
    Removed(Utf8PathBuf),
    Renamed { from: Utf8PathBuf, to: Utf8PathBuf },
}

#[derive(Debug, Error)]
pub enum WatchError {
    #[error(transparent)]
    Notify(#[from] notify::Error),
}

/// Spawns a recursive filesystem watcher over the vault root and forwards
/// markdown-relevant events into the returned tokio receiver. The returned
/// `RecommendedWatcher` must be kept alive for as long as you want events.
pub fn watch_vault(
    root: &Utf8Path,
) -> Result<(RecommendedWatcher, mpsc::UnboundedReceiver<VaultEvent>), WatchError> {
    let (tx, rx) = mpsc::unbounded_channel();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        match res {
            Ok(event) => {
                for vault_event in convert(event) {
                    let _ = tx.send(vault_event);
                }
            }
            Err(err) => tracing::warn!("watcher error: {err}"),
        }
    })?;

    watcher.watch(root.as_std_path(), RecursiveMode::Recursive)?;
    Ok((watcher, rx))
}

fn convert(event: notify::Event) -> Vec<VaultEvent> {
    let paths: Vec<Utf8PathBuf> = event
        .paths
        .iter()
        .filter_map(|p| Utf8Path::from_path(p).map(|u| u.to_path_buf()))
        .filter(|p| p.extension() == Some("md"))
        .collect();

    if paths.is_empty() {
        return Vec::new();
    }

    match event.kind {
        EventKind::Create(_) => paths.into_iter().map(VaultEvent::Created).collect(),
        EventKind::Modify(notify::event::ModifyKind::Name(_)) if paths.len() == 2 => {
            vec![VaultEvent::Renamed {
                from: paths[0].clone(),
                to: paths[1].clone(),
            }]
        }
        EventKind::Modify(_) => paths.into_iter().map(VaultEvent::Modified).collect(),
        EventKind::Remove(_) => paths.into_iter().map(VaultEvent::Removed).collect(),
        _ => Vec::new(),
    }
}
