//! Git helper — wraps `std::process::Command` for git operations.
//!
//! This is the sole module that executes git. All other modules that need
//! git functionality must go through these functions. This keeps the git
//! dependency isolated and testable.

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Output from a git command.
#[derive(Debug)]
pub struct GitOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

/// Run a git command and capture output.
///
/// Returns `Err(Error::GitNotFound)` if git is not on PATH.
/// Returns `Err(Error::Git { .. })` if the command exits non-zero.
fn run_git(args: &[&str], cwd: Option<&Path>) -> Result<GitOutput> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    // Suppress interactive prompts
    cmd.env("GIT_TERMINAL_PROMPT", "0");

    let output = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::GitNotFound
        } else {
            Error::Git {
                args: args.join(" "),
                stderr: e.to_string(),
            }
        }
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    Ok(GitOutput {
        stdout,
        stderr,
        success: output.status.success(),
    })
}

/// Run a git command and require success. Returns stdout on success.
fn run_git_ok(args: &[&str], cwd: Option<&Path>) -> Result<String> {
    let output = run_git(args, cwd)?;
    if output.success {
        Ok(output.stdout)
    } else {
        Err(Error::Git {
            args: args.join(" "),
            stderr: output.stderr,
        })
    }
}

/// Git operations used by AKM.
///
/// All methods are stateless functions that shell out to `git`.
/// The struct exists to allow future dependency injection / trait extraction.
/// A `GitOps` trait will be extracted at the start of Task 3 (Registry + Sync)
/// when the `RegistrySource` trait needs to mock Git for testing.
pub struct Git;

impl Git {
    /// Check if the current (or given) directory is inside a git work tree.
    ///
    /// Bash: `_akm_in_git_repo()` / `git rev-parse --is-inside-work-tree`
    pub fn is_inside_work_tree(cwd: Option<&Path>) -> bool {
        run_git(&["rev-parse", "--is-inside-work-tree"], cwd)
            .map(|o| o.success && o.stdout == "true")
            .unwrap_or(false)
    }

    /// Get the repository root (toplevel) directory.
    ///
    /// Bash: `_akm_project_root()` / `git rev-parse --show-toplevel`
    pub fn toplevel(cwd: Option<&Path>) -> Result<PathBuf> {
        let stdout = run_git_ok(&["rev-parse", "--show-toplevel"], cwd)?;
        if stdout.is_empty() {
            Err(Error::NotInGitRepo)
        } else {
            Ok(PathBuf::from(stdout))
        }
    }

    /// Get the repository name (basename of toplevel).
    ///
    /// Bash: `_akm_repo_name()` / `basename "$(git rev-parse --show-toplevel)"`
    pub fn repo_name(cwd: Option<&Path>) -> Result<String> {
        let toplevel = Self::toplevel(cwd)?;
        toplevel
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or(Error::NotInGitRepo)
    }

    /// Clone a repository.
    ///
    /// Bash: `git clone --quiet "$url" "$dest"`
    pub fn clone(url: &str, dest: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Io {
                context: format!("Creating parent dir for clone: {}", parent.display()),
                source: e,
            })?;
        }

        run_git_ok(&["clone", "--quiet", url, &dest.to_string_lossy()], None)?;
        Ok(())
    }

    /// Pull with rebase and autostash (used for registry + artifacts sync).
    ///
    /// Bash: `git -C "$dir" pull --rebase --autostash --quiet`
    pub fn pull(repo_dir: &Path) -> Result<()> {
        run_git_ok(
            &["pull", "--rebase", "--autostash", "--quiet"],
            Some(repo_dir),
        )?;
        Ok(())
    }

    /// Push (used for artifacts auto-push + publish).
    ///
    /// Bash: `git -C "$dir" push --quiet`
    pub fn push(repo_dir: &Path) -> Result<()> {
        run_git_ok(&["push", "--quiet"], Some(repo_dir))?;
        Ok(())
    }

    /// Stage all changes.
    ///
    /// Bash: `git -C "$dir" add -A`
    pub fn add_all(repo_dir: &Path) -> Result<()> {
        run_git_ok(&["add", "-A"], Some(repo_dir))?;
        Ok(())
    }

    /// Commit with a message.
    ///
    /// Bash: `git -C "$dir" commit -m "$msg" --quiet`
    pub fn commit(repo_dir: &Path, message: &str) -> Result<()> {
        run_git_ok(&["commit", "-m", message, "--quiet"], Some(repo_dir))?;
        Ok(())
    }

    /// Check if the repo has uncommitted changes or untracked files.
    ///
    /// Bash equivalent (from _akm_artifacts_commit_and_push):
    /// ```bash
    /// git -C "$dir" diff --quiet
    /// git -C "$dir" ls-files --others --exclude-standard
    /// ```
    ///
    /// Also detects staged-but-uncommitted changes (`git diff --cached`).
    pub fn has_changes(repo_dir: &Path) -> Result<bool> {
        // Working tree changes
        let diff = run_git(&["diff", "--quiet"], Some(repo_dir))?;
        if !diff.success {
            return Ok(true);
        }
        // Staged changes
        let staged = run_git(&["diff", "--cached", "--quiet"], Some(repo_dir))?;
        if !staged.success {
            return Ok(true);
        }
        // Untracked files
        let untracked = run_git(
            &["ls-files", "--others", "--exclude-standard"],
            Some(repo_dir),
        )?;
        Ok(!untracked.stdout.is_empty())
    }

    /// Check if a directory is a git repository (has .git dir).
    ///
    /// Bash: `[[ -d "$dir/.git" ]]`
    pub fn is_repo(dir: &Path) -> bool {
        dir.join(".git").is_dir()
    }

    /// Get the remote URL for the given remote name.
    ///
    /// Bash: `git -C "$repo" remote get-url origin`
    pub fn remote_url(repo_dir: &Path, remote: &str) -> Result<String> {
        run_git_ok(&["remote", "get-url", remote], Some(repo_dir))
    }

    /// Pull with ff-only (used for self-update).
    ///
    /// Bash: `git -C "$repo" pull --ff-only`
    pub fn pull_ff_only(repo_dir: &Path) -> Result<()> {
        run_git_ok(&["pull", "--ff-only"], Some(repo_dir))?;
        Ok(())
    }

    /// Count how many local commits are ahead of the upstream tracking branch.
    ///
    /// Returns 0 if there is no upstream configured or if the count cannot
    /// be determined (matches Bash `|| echo "0"` fallback).
    ///
    /// Bash: `git -C "$dir" rev-list --count @{upstream}..HEAD 2>/dev/null || echo "0"`
    pub fn commits_ahead(repo_dir: &Path) -> Result<u32> {
        let result = run_git(
            &["rev-list", "--count", "@{upstream}..HEAD"],
            Some(repo_dir),
        )?;
        if result.success {
            result.stdout.trim().parse::<u32>().map_err(|_| Error::Git {
                args: "rev-list --count @{upstream}..HEAD".into(),
                stderr: format!("Could not parse commit count: '{}'", result.stdout.trim()),
            })
        } else {
            // No upstream or other error — treat as 0 ahead (Bash: `|| echo "0"`)
            Ok(0)
        }
    }
}
