//! Thin async wrapper around the system `git` CLI.
//!
//! We deliberately do not use `git2` or `gix` — the hard part of "GitHub sync"
//! is authentication (PATs, SSH keys, Windows Credential Manager, `gh` CLI),
//! and the system git already solves it via credential helpers. Reimplementing
//! that surface in Rust is a tar pit.

use camino::Utf8Path;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use thiserror::Error;
use tokio::process::Command;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("`git` CLI not found on PATH")]
    GitNotInstalled,
    #[error("git command failed ({code}): {stderr}")]
    GitFailed { code: i32, stderr: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub dirty_files: Vec<DirtyEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirtyEntry {
    pub path: String,
    /// Two-letter porcelain code: e.g. " M", "M ", "??", "UU".
    pub code: String,
}

impl SyncStatus {
    pub fn is_clean(&self) -> bool {
        self.dirty_files.is_empty()
    }
    pub fn has_conflicts(&self) -> bool {
        self.dirty_files.iter().any(|e| e.code.contains('U'))
    }
}

/// Returns true if `git --version` runs.
pub async fn is_git_available() -> bool {
    let mut cmd = Command::new("git");
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

pub async fn is_repo(path: &Utf8Path) -> bool {
    run_git(path, &["rev-parse", "--git-dir"]).await.is_ok()
}

pub async fn init(path: &Utf8Path) -> Result<(), SyncError> {
    run_git(path, &["init", "-b", "main"]).await?;
    Ok(())
}

pub async fn connect_remote(path: &Utf8Path, remote_url: &str) -> Result<(), SyncError> {
    if run_git(path, &["remote", "get-url", "origin"]).await.is_ok() {
        run_git(path, &["remote", "set-url", "origin", remote_url]).await?;
    } else {
        run_git(path, &["remote", "add", "origin", remote_url]).await?;
    }
    Ok(())
}

pub async fn status(path: &Utf8Path) -> Result<SyncStatus, SyncError> {
    let out = run_git(path, &["status", "--porcelain=v1", "-b"]).await?;
    Ok(parse_porcelain(&out))
}

pub async fn pull(path: &Utf8Path) -> Result<(), SyncError> {
    run_git(path, &["pull", "--rebase", "--autostash"]).await?;
    Ok(())
}

pub async fn commit_all(path: &Utf8Path, message: &str) -> Result<(), SyncError> {
    run_git(path, &["add", "-A"]).await?;
    // Skip commit when there is nothing staged.
    let staged = run_git(path, &["diff", "--cached", "--quiet"]).await;
    if staged.is_ok() {
        return Ok(());
    }
    run_git(path, &["commit", "-m", message]).await?;
    Ok(())
}

pub async fn push(path: &Utf8Path) -> Result<(), SyncError> {
    // First push of a new branch needs --set-upstream; do that conditionally.
    let upstream = run_git(path, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).await;
    if upstream.is_ok() {
        run_git(path, &["push"]).await?;
    } else {
        run_git(path, &["push", "-u", "origin", "HEAD"]).await?;
    }
    Ok(())
}

/// Convenience: pull → commit_all (if dirty) → push.
pub async fn sync(path: &Utf8Path, commit_message: &str) -> Result<SyncStatus, SyncError> {
    pull(path).await?;
    let st = status(path).await?;
    if !st.is_clean() && !st.has_conflicts() {
        commit_all(path, commit_message).await?;
    }
    if !st.has_conflicts() {
        push(path).await?;
    }
    status(path).await
}

async fn run_git(cwd: &Utf8Path, args: &[&str]) -> Result<String, SyncError> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(cwd.as_std_path())
        .stdin(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd
        .output()
        .await
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => SyncError::GitNotInstalled,
            _ => SyncError::Io(e),
        })?;
    if !output.status.success() {
        return Err(SyncError::GitFailed {
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_porcelain(out: &str) -> SyncStatus {
    let mut branch = None;
    let mut upstream = None;
    let mut ahead = 0u32;
    let mut behind = 0u32;
    let mut dirty_files = Vec::new();

    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            // ## main...origin/main [ahead 1, behind 2]
            let (b, suffix) = match rest.split_once(' ') {
                Some((b, s)) => (b, Some(s)),
                None => (rest, None),
            };
            let (local, up) = match b.split_once("...") {
                Some((l, u)) => (l, Some(u.to_string())),
                None => (b, None),
            };
            branch = Some(local.to_string());
            upstream = up;
            if let Some(s) = suffix {
                if let Some(open) = s.find('[') {
                    let inside = &s[open + 1..s.find(']').unwrap_or(s.len())];
                    for token in inside.split(", ") {
                        let mut parts = token.splitn(2, ' ');
                        match (parts.next(), parts.next()) {
                            (Some("ahead"), Some(n)) => ahead = n.parse().unwrap_or(0),
                            (Some("behind"), Some(n)) => behind = n.parse().unwrap_or(0),
                            _ => {}
                        }
                    }
                }
            }
        } else if line.len() >= 3 {
            let code = line[..2].to_string();
            let path = line[3..].to_string();
            dirty_files.push(DirtyEntry { code, path });
        }
    }

    SyncStatus {
        branch,
        upstream,
        ahead,
        behind,
        dirty_files,
    }
}

// ─── Per-file history ───────────────────────────────────────────────────────

/// A single commit in a file's git history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitInfo {
    /// Short SHA (7 chars).
    pub sha: String,
    pub author: String,
    /// Short date (YYYY-MM-DD).
    pub date: String,
    /// First line of the commit message.
    pub message: String,
}

/// Return the commits that touched `rel_path` (relative to `vault_root`).
/// Returns an empty vec — not an error — when the file has no history or the
/// vault is not a git repository.
pub async fn file_history(
    vault_root: &Utf8Path,
    rel_path: &str,
) -> Result<Vec<CommitInfo>, SyncError> {
    // \x1f = ASCII Unit Separator — safe delimiter inside git format strings.
    let fmt = "--format=%h\x1f%an\x1f%cs\x1f%s";
    let git_path = rel_path.replace('\\', "/");
    let out = match run_git(vault_root, &["log", "--follow", fmt, "--", &git_path]).await {
        Ok(o) => o,
        // Not a git repo or git isn't installed → treat as no history.
        Err(SyncError::GitNotInstalled) | Err(SyncError::GitFailed { .. }) => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    Ok(parse_history_log(&out))
}

fn parse_history_log(out: &str) -> Vec<CommitInfo> {
    out.lines()
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\x1f');
            Some(CommitInfo {
                sha: parts.next()?.to_string(),
                author: parts.next()?.to_string(),
                date: parts.next()?.to_string(),
                message: parts.next().unwrap_or("").to_string(),
            })
        })
        .collect()
}

/// Return the raw content of `rel_path` as it existed at commit `sha`.
pub async fn file_at_rev(
    vault_root: &Utf8Path,
    rel_path: &str,
    sha: &str,
) -> Result<String, SyncError> {
    let git_path = rel_path.replace('\\', "/");
    run_git(vault_root, &["show", &format!("{sha}:{git_path}")]).await
}

/// Auto-generated commit message: "notes: update N files (a.md, b.md, …)".
pub fn auto_commit_message(status: &SyncStatus) -> String {
    let n = status.dirty_files.len();
    if n == 0 {
        return "notes: no changes".into();
    }
    let preview: Vec<&str> = status
        .dirty_files
        .iter()
        .take(3)
        .map(|e| e.path.as_str())
        .collect();
    let suffix = if n > 3 {
        format!(", +{} more", n - 3)
    } else {
        String::new()
    };
    format!("notes: update {n} files ({}{suffix})", preview.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_branch_with_upstream() {
        let out = "## main...origin/main\n";
        let st = parse_porcelain(out);
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert_eq!(st.upstream.as_deref(), Some("origin/main"));
        assert_eq!(st.ahead, 0);
        assert_eq!(st.behind, 0);
        assert!(st.is_clean());
    }

    #[test]
    fn parses_dirty_with_ahead_behind() {
        let out = "## feature...origin/feature [ahead 2, behind 1]\n M notes/a.md\n?? notes/b.md\nUU notes/c.md\n";
        let st = parse_porcelain(out);
        assert_eq!(st.ahead, 2);
        assert_eq!(st.behind, 1);
        assert_eq!(st.dirty_files.len(), 3);
        assert!(st.has_conflicts());
    }

    #[test]
    fn auto_message_truncates() {
        let st = SyncStatus {
            branch: None,
            upstream: None,
            ahead: 0,
            behind: 0,
            dirty_files: (0..5)
                .map(|i| DirtyEntry {
                    code: " M".into(),
                    path: format!("note-{i}.md"),
                })
                .collect(),
        };
        let msg = auto_commit_message(&st);
        assert!(msg.starts_with("notes: update 5 files"));
        assert!(msg.contains("+2 more"));
    }
}
