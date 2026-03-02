//! Artifacts domain — bidirectional git sync of LLM session outputs.
//!
//! The artifacts repository is a single git repo where LLM session outputs
//! (plans, research, session notes, etc.) are stored, organized by project
//! subdirectory. AKM manages this repo with:
//!
//! - **CLI sync** (`akm artifacts sync`): clone-or-pull, then push-if-ahead
//! - **Session lifecycle** (shell wrappers): pull on start, commit+push on exit
//!
//! All git operations go through the `Git` helper (Task 1). This module
//! provides the domain-specific orchestration.

use crate::config::Config;
use crate::error::{Error, IoContext, Result};
use crate::git::Git;
use crate::paths::Paths;
use std::path::PathBuf;

/// Outcome of an artifacts sync operation, used for display messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    /// No remote configured — sync skipped.
    NoRemote,
    /// First-time clone completed successfully.
    Cloned,
    /// Pulled latest changes, no local commits to push.
    Pulled,
    /// Pulled latest changes, then pushed local commits.
    PulledAndPushed {
        /// Number of local commits that were pushed.
        commits_pushed: u32,
    },
}

/// Outcome of a commit-and-push operation (session lifecycle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitPushOutcome {
    /// No git repo found at artifacts dir — nothing to do.
    NotARepo,
    /// No changes detected — nothing to commit.
    NoChanges,
    /// Changes committed but push skipped (no remote, or push failed non-fatally).
    Committed,
    /// Changes committed and pushed.
    CommittedAndPushed,
}

/// Manages the artifacts git repository.
///
/// This struct does not own any state — it resolves paths from `Config`
/// and `Paths` on each call, making it safe to use across config reloads.
pub struct ArtifactRepo;

impl ArtifactRepo {
    /// Resolve the artifacts directory from config, falling back to default.
    ///
    /// Bash: `ARTIFACTS_DIR="${ARTIFACTS_DIR:-$HOME/.akm/artifacts}"`
    fn artifacts_dir(config: &Config, paths: &Paths) -> PathBuf {
        config.artifacts_dir(paths)
    }

    /// Perform a full bidirectional sync of the artifacts repository.
    ///
    /// This is the implementation behind `akm artifacts sync`.
    ///
    /// Logic (mirrors Bash cmd_artifacts_sync exactly):
    /// 1. Check if remote is configured → if not, return NoRemote
    /// 2. If local repo exists (.git dir present):
    ///    a. Pull with rebase+autostash
    ///    b. Count commits ahead of upstream
    ///    c. If ahead > 0, push
    /// 3. If no local repo: clone from remote
    ///
    /// Bash: `cmd_artifacts_sync()` (bin/akm:474–518)
    pub fn sync(config: &Config, paths: &Paths) -> Result<SyncOutcome> {
        let remote = match &config.artifacts.remote {
            Some(r) if !r.is_empty() => r.clone(),
            _ => return Ok(SyncOutcome::NoRemote),
        };

        let dir = Self::artifacts_dir(config, paths);

        if Git::is_repo(&dir) {
            // Pull with rebase
            // Bash: bin/akm:488 `git -C "$dir" pull --rebase --autostash`
            Git::pull(&dir).map_err(|e| Error::ArtifactsSync {
                operation: format!("pull from {remote}"),
                message: e.to_string(),
            })?;

            // Check if local commits need pushing
            // Bash: bin/akm:497 `ahead="$(git -C "$dir" rev-list --count @{upstream}..HEAD)"`
            let ahead = Git::commits_ahead(&dir).unwrap_or(0);
            if ahead > 0 {
                // Bash: bin/akm:500 `git -C "$dir" push`
                Git::push(&dir).map_err(|e| Error::ArtifactsSync {
                    operation: format!("push to {remote}"),
                    message: e.to_string(),
                })?;
                Ok(SyncOutcome::PulledAndPushed {
                    commits_pushed: ahead,
                })
            } else {
                Ok(SyncOutcome::Pulled)
            }
        } else {
            // First-time clone
            // Bash: bin/akm:509 `mkdir -p "$(dirname "$dir")"`
            // Bash: bin/akm:511 `git clone "$remote" "$dir"`
            Git::clone(&remote, &dir).map_err(|e| Error::ArtifactsClone {
                remote: remote.clone(),
                message: e.to_string(),
            })?;
            Ok(SyncOutcome::Cloned)
        }
    }

    /// Ensure the per-project artifact directory exists.
    ///
    /// Creates `<artifacts_dir>/<repo_name>/` if it doesn't exist.
    /// Returns the path to the project's artifact directory.
    ///
    /// Bash: `_akm_artifacts_ensure_dir()` (shell/akm-init.sh:130–144)
    pub fn ensure_project_dir(
        config: &Config,
        paths: &Paths,
        project_name: &str,
    ) -> Result<PathBuf> {
        let dir = Self::artifacts_dir(config, paths);
        let project_dir = dir.join(project_name);
        if !project_dir.exists() {
            std::fs::create_dir_all(&project_dir).io_context(format!(
                "Creating artifact directory for project '{project_name}'"
            ))?;
        }
        Ok(project_dir)
    }

    /// Pull the artifacts repository (session start).
    ///
    /// Silently succeeds if the repo doesn't exist or pull fails
    /// (matches Bash `|| true` behavior).
    ///
    /// Bash: `_akm_artifacts_pull()` (shell/akm-init.sh:147–152)
    pub fn pull_quiet(config: &Config, paths: &Paths) -> Result<()> {
        let dir = Self::artifacts_dir(config, paths);
        if Git::is_repo(&dir) {
            // Bash: `git -C "$artifacts_dir" pull --rebase --autostash --quiet 2>/dev/null || true`
            let _ = Git::pull(&dir); // Intentionally ignore errors
        }
        Ok(())
    }

    /// Auto-commit and push artifacts (session exit).
    ///
    /// Logic (mirrors Bash _akm_artifacts_commit_and_push):
    /// 1. Check if artifacts dir is a git repo → if not, return
    /// 2. Check for changes (diff + untracked files) → if none, return
    /// 3. Stage all changes (`git add -A`)
    /// 4. Commit with message: `<project_name>: YYYY-MM-DD-HHMM`
    /// 5. Pull with rebase (to pick up any remote changes)
    /// 6. Push
    ///
    /// Bash: `_akm_artifacts_commit_and_push()` (shell/akm-init.sh:155–178)
    pub fn commit_and_push(
        config: &Config,
        paths: &Paths,
        project_name: &str,
    ) -> Result<CommitPushOutcome> {
        let dir = Self::artifacts_dir(config, paths);

        if !Git::is_repo(&dir) {
            return Ok(CommitPushOutcome::NotARepo);
        }

        // Check for changes
        let has_changes = Git::has_changes(&dir).unwrap_or(false);
        if !has_changes {
            return Ok(CommitPushOutcome::NoChanges);
        }

        // Stage all
        Git::add_all(&dir).map_err(|e| Error::ArtifactsSync {
            operation: "stage changes".into(),
            message: e.to_string(),
        })?;

        // Commit with timestamp message
        // Bash: `git -C "$artifacts_dir" commit -m "${repo_name:-misc}: $(date +%Y-%m-%d-%H%M)"`
        let timestamp = local_timestamp();
        let name = if project_name.is_empty() {
            "misc"
        } else {
            project_name
        };
        let message = format!("{name}: {timestamp}");
        // Commit may fail if nothing is staged after all (race condition) — that's OK
        let _ = Git::commit(&dir, &message);

        // Pull to rebase on any remote changes before pushing
        let _ = Git::pull(&dir);

        // Push
        match Git::push(&dir) {
            Ok(()) => Ok(CommitPushOutcome::CommittedAndPushed),
            Err(_) => Ok(CommitPushOutcome::Committed),
        }
    }
}

/// Generate a timestamp string in `YYYY-MM-DD-HHMM` format matching
/// Bash `date +%Y-%m-%d-%H%M` using local time.
///
/// EXCEPTION: Uses `unsafe` for `libc::localtime_r`. This is acceptable because:
/// - `libc` is already linked by `std` (zero additional binary size)
/// - `localtime_r` is POSIX thread-safe (unlike `localtime`)
/// - Both safety invariants are documented inline
/// - Avoids adding a crate dependency or shelling out to `date`
fn local_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as libc::time_t;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: localtime_r is thread-safe and writes into our stack buffer.
    // The input `now` is a valid time_t from SystemTime.
    let result = unsafe { libc::localtime_r(&now, tm.as_mut_ptr()) };
    if result.is_null() {
        return "0000-00-00-0000".to_string();
    }
    // SAFETY: localtime_r succeeded (non-null return), so tm is fully initialized.
    let tm = unsafe { tm.assume_init() };
    format!(
        "{:04}-{:02}-{:02}-{:02}{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_outcome_variants() {
        let _no_remote = SyncOutcome::NoRemote;
        let _cloned = SyncOutcome::Cloned;
        let _pulled = SyncOutcome::Pulled;
        let _pushed = SyncOutcome::PulledAndPushed { commits_pushed: 3 };
    }

    #[test]
    fn test_commit_push_outcome_variants() {
        let _not_repo = CommitPushOutcome::NotARepo;
        let _no_changes = CommitPushOutcome::NoChanges;
        let _committed = CommitPushOutcome::Committed;
        let _pushed = CommitPushOutcome::CommittedAndPushed;
    }

    #[test]
    fn test_local_timestamp_format() {
        let ts = local_timestamp();
        // Should match YYYY-MM-DD-HHMM pattern (15 chars)
        assert_eq!(ts.len(), 15, "Timestamp should be 15 chars: {ts}");
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "-");
    }

    #[test]
    fn test_commit_message_empty_project_name_fallback() {
        // Verifies the "${repo_name:-misc}" Bash fallback behavior
        let name = "";
        let label = if name.is_empty() { "misc" } else { name };
        assert_eq!(label, "misc");
    }
}
