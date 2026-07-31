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
    // Force English messages — `merge_ff_only` classifies failures by matching
    // git's stderr, which is localized otherwise.
    cmd.env("LC_ALL", "C");

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

/// Outcome of a fast-forward-only merge attempt.
///
/// Every variant leaves the working tree in a defined state: `Blocked` and
/// `NotFastForward` mean git changed nothing at all, so the caller can park,
/// clean and retry without fear of half-applied merges or conflict markers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// Already at (or ahead of) the target — nothing was applied.
    UpToDate,
    /// The working tree was fast-forwarded to the target.
    FastForwarded,
    /// Local changes to these paths would be overwritten. Tree untouched.
    Blocked { paths: Vec<String> },
    /// Histories have diverged, so no fast-forward is possible. Tree untouched.
    NotFastForward,
}

/// A single entry from `git status --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    /// Two-character status code (e.g. ` M`, `??`, `A `).
    pub code: String,
    /// Path relative to the repository root. For renames, the destination.
    pub path: String,
}

impl StatusEntry {
    /// Whether this entry is an untracked file.
    pub fn is_untracked(&self) -> bool {
        self.code == "??"
    }
}

/// Git operations used by AKM.
///
/// All methods are stateless functions that shell out to `git`.
/// The struct exists as a namespace. Registry sync uses the `RegistrySource`
/// trait for abstraction, so Git is only called through `registry::git::GitRegistry`.
pub struct Git;

impl Git {
    /// Check if the current (or given) directory is inside a git work tree.
    pub fn is_inside_work_tree(cwd: Option<&Path>) -> bool {
        run_git(&["rev-parse", "--is-inside-work-tree"], cwd)
            .map(|o| o.success && o.stdout == "true")
            .unwrap_or(false)
    }

    /// Get the repository root (toplevel) directory.
    pub fn toplevel(cwd: Option<&Path>) -> Result<PathBuf> {
        let stdout = run_git_ok(&["rev-parse", "--show-toplevel"], cwd)?;
        if stdout.is_empty() {
            Err(Error::NotInGitRepo)
        } else {
            Ok(PathBuf::from(stdout))
        }
    }

    /// Get the repository name (basename of toplevel).
    pub fn repo_name(cwd: Option<&Path>) -> Result<String> {
        let toplevel = Self::toplevel(cwd)?;
        toplevel
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or(Error::NotInGitRepo)
    }

    /// Clone a repository.
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
    pub fn pull(repo_dir: &Path) -> Result<()> {
        run_git_ok(
            &["pull", "--rebase", "--autostash", "--quiet"],
            Some(repo_dir),
        )?;
        Ok(())
    }

    /// Push (used for artifacts auto-push + publish).
    pub fn push(repo_dir: &Path) -> Result<()> {
        run_git_ok(&["push", "--quiet"], Some(repo_dir))?;
        Ok(())
    }

    /// Stage all changes.
    pub fn add_all(repo_dir: &Path) -> Result<()> {
        run_git_ok(&["add", "-A"], Some(repo_dir))?;
        Ok(())
    }

    /// Commit with a message.
    pub fn commit(repo_dir: &Path, message: &str) -> Result<()> {
        run_git_ok(&["commit", "-m", message, "--quiet"], Some(repo_dir))?;
        Ok(())
    }

    /// Check if the repo has uncommitted changes or untracked files.
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
    pub fn is_repo(dir: &Path) -> bool {
        dir.join(".git").is_dir()
    }

    /// Get the remote URL for the given remote name.
    pub fn remote_url(repo_dir: &Path, remote: &str) -> Result<String> {
        run_git_ok(&["remote", "get-url", remote], Some(repo_dir))
    }

    /// Pull with ff-only (used for self-update).
    pub fn pull_ff_only(repo_dir: &Path) -> Result<()> {
        run_git_ok(&["pull", "--ff-only"], Some(repo_dir))?;
        Ok(())
    }

    /// Count how many local commits are ahead of the upstream tracking branch.
    ///
    /// Returns 0 if there is no upstream configured or if the count cannot
    /// be determined.
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
            // No upstream or other error — treat as 0 ahead
            Ok(0)
        }
    }

    /// Get diff stats for staged changes.
    pub fn diff_cached_stat(repo_dir: &Path) -> Result<String> {
        run_git_ok(&["diff", "--cached", "--stat"], Some(repo_dir))
    }

    /// Get full diff for staged changes.
    pub fn diff_cached(repo_dir: &Path) -> Result<String> {
        run_git_ok(&["diff", "--cached"], Some(repo_dir))
    }

    /// Reset staging area (unstage all staged changes).
    pub fn reset(repo_dir: &Path) -> Result<()> {
        run_git_ok(&["reset", "--quiet"], Some(repo_dir))?;
        Ok(())
    }

    /// Check if staging area is clean (no staged changes after `add_all`).
    pub fn is_staging_clean(repo_dir: &Path) -> Result<bool> {
        let output = run_git(&["diff", "--cached", "--quiet"], Some(repo_dir))?;
        Ok(output.success)
    }

    // --- Primitives for the library working tree ---

    /// Fetch from the default remote without touching the working tree.
    pub fn fetch(repo_dir: &Path) -> Result<()> {
        run_git_ok(&["fetch", "--quiet"], Some(repo_dir))?;
        Ok(())
    }

    /// Resolve a revision to a commit SHA.
    pub fn rev_parse(repo_dir: &Path, rev: &str) -> Result<String> {
        run_git_ok(&["rev-parse", rev], Some(repo_dir))
    }

    /// Name of the upstream tracking ref (e.g. `origin/main`).
    ///
    /// Returns `Err(Error::Git)` when the branch has no upstream configured.
    pub fn upstream_ref(repo_dir: &Path) -> Result<String> {
        run_git_ok(
            &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
            Some(repo_dir),
        )
    }

    /// Merge `rev` into the current branch, fast-forward only.
    ///
    /// A fast-forward preserves uncommitted local edits to paths the incoming
    /// commits do not touch, and aborts without writing anything when they do.
    /// That is the whole reason sync never performs a real merge: a merge could
    /// write conflict markers into files that are symlinked into live tool
    /// directories.
    ///
    /// Failures git does not describe in terms we recognise are returned as
    /// `Err` rather than guessed at — the caller must not touch the tree.
    pub fn merge_ff_only(repo_dir: &Path, rev: &str) -> Result<MergeOutcome> {
        let before = Self::rev_parse(repo_dir, "HEAD")?;
        let output = run_git(&["merge", "--ff-only", "--quiet", rev], Some(repo_dir))?;

        if output.success {
            let after = Self::rev_parse(repo_dir, "HEAD")?;
            return Ok(if before == after {
                MergeOutcome::UpToDate
            } else {
                MergeOutcome::FastForwarded
            });
        }

        if output.stderr.contains("would be overwritten by merge") {
            return Ok(MergeOutcome::Blocked {
                paths: parse_overwritten_paths(&output.stderr),
            });
        }

        if output.stderr.contains("Not possible to fast-forward") {
            return Ok(MergeOutcome::NotFastForward);
        }

        Err(Error::Git {
            args: format!("merge --ff-only {rev}"),
            stderr: output.stderr,
        })
    }

    /// Parsed `git status --porcelain`, with untracked directories expanded
    /// into individual files so every entry maps to one spec.
    pub fn status_porcelain(repo_dir: &Path) -> Result<Vec<StatusEntry>> {
        let stdout = run_git_ok(
            &["status", "--porcelain", "--untracked-files=all"],
            Some(repo_dir),
        )?;
        Ok(stdout.lines().filter_map(parse_status_line).collect())
    }

    /// Paths that differ between two revisions, optionally limited to pathspecs.
    pub fn diff_names(
        repo_dir: &Path,
        from: &str,
        to: &str,
        pathspecs: &[&str],
    ) -> Result<Vec<String>> {
        let mut args = vec!["diff", "--name-only", from, to];
        push_pathspecs(&mut args, pathspecs);
        let stdout = run_git_ok(&args, Some(repo_dir))?;
        Ok(stdout.lines().map(|l| l.to_string()).collect())
    }

    /// Textual diff between two revisions, limited to `pathspecs`.
    pub fn diff(repo_dir: &Path, from: &str, to: &str, pathspecs: &[&str]) -> Result<String> {
        let mut args = vec!["diff", from, to];
        push_pathspecs(&mut args, pathspecs);
        run_git_ok(&args, Some(repo_dir))
    }

    /// Textual diff of the working tree against a revision, limited to `pathspecs`.
    pub fn diff_worktree(repo_dir: &Path, rev: &str, pathspecs: &[&str]) -> Result<String> {
        let mut args = vec!["diff", rev];
        push_pathspecs(&mut args, pathspecs);
        run_git_ok(&args, Some(repo_dir))
    }

    /// Discard tracked changes under `pathspecs`, restoring them to `HEAD`.
    ///
    /// Untracked files are left alone — pair with [`Git::clean_path`] to wipe a
    /// path completely. Pathspecs git does not know about are not an error:
    /// callers pass the paths a spec *may* occupy, and a spec that exists only
    /// locally has nothing to restore.
    pub fn restore_path(repo_dir: &Path, pathspecs: &[&str]) -> Result<()> {
        let mut args = vec!["restore", "--source=HEAD", "--staged", "--worktree"];
        push_pathspecs(&mut args, pathspecs);

        let output = run_git(&args, Some(repo_dir))?;
        if output.success || output.stderr.contains("did not match any file") {
            Ok(())
        } else {
            Err(Error::Git {
                args: args.join(" "),
                stderr: output.stderr,
            })
        }
    }

    /// Remove untracked files and directories under `pathspecs`.
    pub fn clean_path(repo_dir: &Path, pathspecs: &[&str]) -> Result<()> {
        let mut args = vec!["clean", "-qfd"];
        push_pathspecs(&mut args, pathspecs);
        run_git_ok(&args, Some(repo_dir))?;
        Ok(())
    }

    /// Overwrite `pathspecs` in the working tree with their content at `rev`.
    pub fn checkout_from(repo_dir: &Path, rev: &str, pathspecs: &[&str]) -> Result<()> {
        let mut args = vec!["checkout", rev];
        push_pathspecs(&mut args, pathspecs);
        run_git_ok(&args, Some(repo_dir))?;
        Ok(())
    }

    /// Stage the given paths, including deletions.
    ///
    /// Paths that match nothing are skipped rather than failing, so a caller
    /// can hand over every path a spec might occupy.
    pub fn add_path(repo_dir: &Path, pathspecs: &[&str]) -> Result<()> {
        let mut args = vec!["add", "--all"];
        push_pathspecs(&mut args, pathspecs);

        let output = run_git(&args, Some(repo_dir))?;
        if output.success || output.stderr.contains("did not match any file") {
            Ok(())
        } else {
            Err(Error::Git {
                args: args.join(" "),
                stderr: output.stderr,
            })
        }
    }
}

/// Append `-- <pathspec>...` to a git argument list, omitting the separator
/// when there is nothing to limit the command to.
fn push_pathspecs<'a>(args: &mut Vec<&'a str>, pathspecs: &[&'a str]) {
    if pathspecs.is_empty() {
        return;
    }
    args.push("--");
    args.extend_from_slice(pathspecs);
}

/// Extract the file list from git's "would be overwritten by merge" message.
///
/// git indents each path with a tab under the header line. `run_git` trims the
/// captured stderr as a whole, which only affects the first and last lines, so
/// the path lines keep their indentation.
fn parse_overwritten_paths(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .filter(|line| line.starts_with('\t'))
        .map(|line| unquote(line.trim()))
        .collect()
}

/// Parse one `git status --porcelain` line into a [`StatusEntry`].
///
/// Rename entries are reported as `R  old -> new`; the destination is what
/// matters for mapping a change back to a spec.
fn parse_status_line(line: &str) -> Option<StatusEntry> {
    if line.len() < 4 {
        return None;
    }
    let (code, rest) = line.split_at(2);
    let rest = rest.trim_start();
    let path = match rest.split_once(" -> ") {
        Some((_, dest)) => dest,
        None => rest,
    };
    Some(StatusEntry {
        code: code.to_string(),
        path: unquote(path),
    })
}

/// Strip the double quotes git adds around paths with unusual characters.
fn unquote(path: &str) -> String {
    path.strip_prefix('"')
        .and_then(|p| p.strip_suffix('"'))
        .unwrap_or(path)
        .to_string()
}
