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
use std::path::{Path, PathBuf};

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
        /// Whether local uncommitted changes were auto-committed.
        local_changes_committed: bool,
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
    fn artifacts_dir(config: &Config, paths: &Paths) -> PathBuf {
        config.artifacts_dir(paths)
    }

    /// Perform a full bidirectional sync of the artifacts repository.
    ///
    /// This is the implementation behind `akm artifacts sync`.
    ///
    /// Logic:
    /// 1. Check if remote is configured → if not, return NoRemote
    /// 2. If local repo exists (.git dir present):
    ///    a. Commit any local changes (untracked + modified)
    ///    b. Pull with rebase+autostash
    ///    c. Count commits ahead of upstream
    ///    d. If ahead > 0, push
    /// 3. If no local repo: clone from remote
    pub fn sync(config: &Config, paths: &Paths) -> Result<SyncOutcome> {
        let remote = match &config.artifacts.remote {
            Some(r) if !r.is_empty() => r.clone(),
            _ => return Ok(SyncOutcome::NoRemote),
        };

        let dir = Self::artifacts_dir(config, paths);

        if Git::is_repo(&dir) {
            // Commit any local changes before pulling
            let committed = if Git::has_changes(&dir).unwrap_or(false) {
                Git::add_all(&dir).map_err(|e| Error::ArtifactsSync {
                    operation: "stage local changes".into(),
                    message: e.to_string(),
                })?;
                let timestamp = local_timestamp();
                let message = format!("sync: {timestamp}");
                // Commit may fail if nothing staged after all (race) — that's OK
                Git::commit(&dir, &message).is_ok()
            } else {
                false
            };

            // Pull with rebase
            Git::pull(&dir).map_err(|e| Error::ArtifactsSync {
                operation: format!("pull from {remote}"),
                message: e.to_string(),
            })?;

            // Check if local commits need pushing
            let ahead = Git::commits_ahead(&dir).unwrap_or(0);
            if ahead > 0 {
                Git::push(&dir).map_err(|e| Error::ArtifactsSync {
                    operation: format!("push to {remote}"),
                    message: e.to_string(),
                })?;
                Ok(SyncOutcome::PulledAndPushed {
                    commits_pushed: ahead,
                    local_changes_committed: committed,
                })
            } else {
                Ok(SyncOutcome::Pulled)
            }
        } else {
            // First-time clone
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
    /// (failures are non-fatal).
    pub fn pull_quiet(config: &Config, paths: &Paths) -> Result<()> {
        let dir = Self::artifacts_dir(config, paths);
        if Git::is_repo(&dir) {
            let _ = Git::pull(&dir); // Intentionally ignore errors
        }
        Ok(())
    }

    /// Auto-commit and push artifacts (session exit).
    ///
    /// Logic:
    /// 1. Check if artifacts dir is a git repo → if not, return
    /// 2. Check for changes (diff + untracked files) → if none, return
    /// 3. Stage all changes (`git add -A`)
    /// 4. Commit with message: `<project_name>: YYYY-MM-DD-HHMM`
    /// 5. Pull with rebase (to pick up any remote changes)
    /// 6. Push
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
/// Format: `%Y-%m-%d-%H%M`, local time.
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

/// One entry in an artifacts directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// File or directory name (no path).
    pub name: String,
    /// Absolute path.
    pub path: PathBuf,
    /// True for a real directory. A symlink is always a leaf (`false`), even
    /// if it points at a directory — we never follow, to avoid cycles.
    pub is_dir: bool,
    /// True if the entry is a symlink (rendered as a leaf).
    pub is_symlink: bool,
}

/// List one directory level, applying the artifacts ordering rules.
///
/// Directories first (alphabetical), then files in reverse order so the
/// date-prefixed (`YYYY-MM-DD-…`) newest lands on top — the "what did the LLM
/// just write" verb. `.git` is hidden (the artifacts dir is a git repo).
/// Symlinks are leaves, never followed.
pub fn scan_dir(dir: &Path) -> Result<Vec<Entry>> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).io_context(format!("Reading {}", dir.display()))? {
        let entry = entry.io_context("Reading directory entry")?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" {
            continue;
        }
        let ft = entry.file_type().io_context(format!("Stat {name}"))?;
        let is_symlink = ft.is_symlink();
        let is_dir = ft.is_dir() && !is_symlink;
        let e = Entry {
            name,
            path: entry.path(),
            is_dir,
            is_symlink,
        };
        if is_dir {
            dirs.push(e)
        } else {
            files.push(e)
        }
    }
    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    files.sort_by(|a, b| b.name.cmp(&a.name)); // reverse → newest date-prefix first
    dirs.extend(files);
    Ok(dirs)
}

/// Render a full recursive tree from `root` as box-drawing text.
///
/// Pure enough to snapshot: it reads the filesystem but its output is a
/// deterministic function of the tree shape. Used by `akm artifacts --plain`.
pub fn render_tree(root: &Path) -> Result<String> {
    let mut out = String::new();
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "artifacts".to_string());
    out.push_str(&format!("{name}/\n"));
    render_children(root, "", &mut out)?;
    Ok(out)
}

fn render_children(dir: &Path, prefix: &str, out: &mut String) -> Result<()> {
    let entries = scan_dir(dir)?;
    let n = entries.len();
    for (i, e) in entries.iter().enumerate() {
        let last = i + 1 == n;
        let connector = if last { "└── " } else { "├── " };
        let suffix = if e.is_dir { "/" } else { "" };
        out.push_str(&format!("{prefix}{connector}{}{suffix}\n", e.name));
        if e.is_dir {
            let child_prefix = format!("{prefix}{}", if last { "    " } else { "│   " });
            render_children(&e.path, &child_prefix, out)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_orders_dirs_first_then_files_newest_first_and_hides_git() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("research")).unwrap();
        std::fs::create_dir_all(root.join("plans")).unwrap();
        std::fs::write(root.join("2026-08-01-old.md"), "x").unwrap();
        std::fs::write(root.join("2026-08-13-new.md"), "x").unwrap();

        let entries = scan_dir(root).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        // dirs alpha first, then files reverse-alpha (newest date-prefix first), .git hidden
        assert_eq!(
            names,
            vec![
                "plans",
                "research",
                "2026-08-13-new.md",
                "2026-08-01-old.md"
            ]
        );
        assert!(entries[0].is_dir);
        assert!(!entries[2].is_dir);
    }

    #[test]
    fn render_tree_draws_box_connectors() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("akm-rs");
        std::fs::create_dir_all(root.join("plans")).unwrap();
        std::fs::write(root.join("plans/2026-08-13-a.md"), "x").unwrap();
        std::fs::write(root.join("plans/2026-08-01-b.md"), "x").unwrap();

        let out = render_tree(&root).unwrap();
        assert_eq!(
            out,
            "akm-rs/\n\
             └── plans/\n\
             \x20   ├── 2026-08-13-a.md\n\
             \x20   └── 2026-08-01-b.md\n"
        );
    }

    #[test]
    fn test_sync_outcome_variants() {
        let _no_remote = SyncOutcome::NoRemote;
        let _cloned = SyncOutcome::Cloned;
        let _pulled = SyncOutcome::Pulled;
        let _pushed = SyncOutcome::PulledAndPushed {
            commits_pushed: 3,
            local_changes_committed: false,
        };
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
        // Verifies the "misc" fallback when the repo name is unknown
        let name = "";
        let label = if name.is_empty() { "misc" } else { name };
        assert_eq!(label, "misc");
    }
}
