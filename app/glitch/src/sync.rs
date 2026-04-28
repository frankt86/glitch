use crate::state::{SyncCommand, SyncState};
use camino::{Utf8Path, Utf8PathBuf};
use dioxus::prelude::*;
use futures::StreamExt;
use glitch_sync::{auto_commit_message, is_repo, status, sync, SyncStatus};

pub async fn sync_coroutine(
    mut commands: UnboundedReceiver<(Utf8PathBuf, SyncCommand)>,
    mut state: Signal<SyncState>,
) {
    while let Some((root, cmd)) = commands.next().await {
        match cmd {
            SyncCommand::CheckStatus => {
                state.set(check_status(&root).await);
            }
            SyncCommand::Sync => {
                state.set(SyncState::Syncing);
                let result = run_sync(&root).await;
                state.set(result);
            }
        }
    }
}

async fn check_status(root: &Utf8Path) -> SyncState {
    if !is_repo(root).await {
        return SyncState::NotARepo;
    }
    match status(root).await {
        Ok(st) => classify(st),
        Err(err) => SyncState::Error(err.to_string()),
    }
}

async fn run_sync(root: &Utf8Path) -> SyncState {
    if !is_repo(root).await {
        return SyncState::NotARepo;
    }
    let pre = match status(root).await {
        Ok(s) => s,
        Err(err) => return SyncState::Error(err.to_string()),
    };
    let msg = auto_commit_message(&pre);
    match sync(root, &msg).await {
        Ok(post) => classify(post),
        Err(err) => SyncState::Error(err.to_string()),
    }
}

fn classify(st: SyncStatus) -> SyncState {
    if st.has_conflicts() {
        SyncState::Conflicts(st)
    } else if st.is_clean() {
        SyncState::Clean
    } else {
        SyncState::Dirty(st)
    }
}
