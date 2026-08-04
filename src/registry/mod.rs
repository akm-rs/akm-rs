//! The personal registry — a git repository checked out as the cold library.
//!
//! There is no cache and no copy step: `$XDG_DATA_HOME/akm/library` *is* the
//! registry's working tree. That single decision is what makes local edits
//! survivable, drift knowable and reverts free, because git already answers
//! all three questions and AKM no longer throws its answers away at a copy
//! boundary.
//!
//! Layering: [`crate::git`] is the only module that executes git, and this
//! module (with [`crate::library::drift`]) is the only one that calls it for
//! the library. Commands go through the [`Registry`] methods below, so no
//! command ever has to know what a fast-forward is.

use crate::error::{Error, Result};
use crate::git::{Git, MergeOutcome};
use crate::library::drift::DriftReport;
use crate::library::spec::SpecType;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// What `akm sync` did to the library working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    /// First run — the registry was cloned.
    Cloned,
    /// Already level with the remote.
    UpToDate,
    /// Fast-forwarded. `parked` names the specs whose uncommitted edits had to
    /// be set aside and put back around the fast-forward; they are now
    /// diverged and need a decision.
    FastForwarded { parked: Vec<String> },
    /// Local commits the remote does not have. Nothing was touched — this is a
    /// human's problem to resolve in the repository itself.
    LocalCommitsDiverged,
    /// The fast-forward could not be applied even after parking. Nothing was
    /// touched; `paths` are the files git refused to overwrite.
    Blocked { paths: Vec<String> },
    /// The branch has no upstream, so there is nothing to fast-forward from.
    NoUpstream,
}

/// Result of publishing one spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishOutcome {
    /// Committed and pushed.
    Published,
    /// The spec already matched the remote — nothing to commit.
    NothingToDo,
}

/// A git-backed registry, checked out at `dir`.
pub struct Registry {
    name: String,
    url: String,
    dir: PathBuf,
}

impl Registry {
    /// Construct a handle to the personal registry. Does not touch the filesystem.
    pub fn new(url: impl Into<String>, dir: impl Into<PathBuf>) -> Self {
        Self::named("personal", url, dir)
    }

    /// Construct a handle to a named registry.
    ///
    /// The name only ever reaches the user through [`Error::RegistrySync`], so
    /// a failure says which registry failed once more than one is in play.
    pub fn named(name: impl Into<String>, url: impl Into<String>, dir: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            dir: dir.into(),
        }
    }

    /// The registry's name, as used in error messages and reports.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The configured remote URL (empty when unconfigured).
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The library working tree.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Whether a remote URL is configured at all.
    pub fn is_configured(&self) -> bool {
        !self.url.is_empty()
    }

    /// Whether the working tree exists as a git repository.
    pub fn is_cloned(&self) -> bool {
        Git::is_repo(&self.dir)
    }

    /// Clone the registry into the library directory.
    pub fn clone_fresh(&self) -> Result<()> {
        Git::clone(&self.url, &self.dir).map_err(|e| Error::RegistrySync {
            name: self.name.clone(),
            message: format!("Clone failed: {e}"),
        })
    }

    /// Fetch and fast-forward the library working tree.
    ///
    /// Never performs a real merge. A merge could write conflict markers into
    /// files that are symlinked live into `~/.claude/skills/` and friends; a
    /// fast-forward either applies cleanly or refuses and changes nothing.
    /// When it refuses because of uncommitted local edits, those edits are
    /// parked, the fast-forward is retried, and the edits are put back on top —
    /// so one unresolved skill can never freeze the other forty-nine.
    pub fn update(&self) -> Result<SyncOutcome> {
        Git::fetch(&self.dir).map_err(|e| Error::RegistrySync {
            name: self.name.clone(),
            message: format!("Fetch failed: {e}"),
        })?;

        let upstream = match Git::upstream_ref(&self.dir) {
            Ok(upstream) => upstream,
            Err(_) => return Ok(SyncOutcome::NoUpstream),
        };

        match Git::merge_ff_only(&self.dir, &upstream)? {
            MergeOutcome::UpToDate => Ok(SyncOutcome::UpToDate),
            MergeOutcome::FastForwarded => Ok(SyncOutcome::FastForwarded { parked: Vec::new() }),
            MergeOutcome::NotFastForward => Ok(SyncOutcome::LocalCommitsDiverged),
            MergeOutcome::Blocked { paths } => self.park_and_retry(&upstream, paths),
        }
    }

    /// Set the blocking edits aside, fast-forward, then put them back.
    fn park_and_retry(&self, upstream: &str, blocked: Vec<String>) -> Result<SyncOutcome> {
        let parked: Vec<Parked> = blocked
            .iter()
            .map(|rel| Parked::take(&self.dir, rel))
            .collect::<Result<_>>()?;

        let refs: Vec<&str> = blocked.iter().map(String::as_str).collect();
        Git::restore_path(&self.dir, &refs)?;
        Git::clean_path(&self.dir, &refs)?;

        let outcome = Git::merge_ff_only(&self.dir, upstream)?;

        // The user's work goes back on top whatever happened: a failed
        // fast-forward must not also cost them their edits.
        for entry in &parked {
            entry.restore(&self.dir)?;
        }

        match outcome {
            MergeOutcome::UpToDate | MergeOutcome::FastForwarded => {
                Ok(SyncOutcome::FastForwarded {
                    parked: spec_ids(&blocked),
                })
            }
            MergeOutcome::Blocked { .. } | MergeOutcome::NotFastForward => {
                Ok(SyncOutcome::Blocked { paths: blocked })
            }
        }
    }

    /// Update the remote-tracking refs without touching the working tree.
    ///
    /// Drift is measured against `@{upstream}`, so anything that acts on drift
    /// has to refresh first — a stale tracking ref makes a diverged spec look
    /// merely unpublished, and the push that follows is rejected.
    pub fn refresh(&self) -> Result<()> {
        Git::fetch(&self.dir).map_err(|e| Error::RegistrySync {
            name: self.name.clone(),
            message: format!("Fetch failed: {e}"),
        })
    }

    /// Drift of every spec relative to the fetched remote.
    pub fn drift(&self) -> Result<DriftReport> {
        DriftReport::compute(&self.dir)
    }

    /// Commit and push the given paths on their own.
    ///
    /// Publishing is per-path so two machines editing two different skills can
    /// never collide, and so a half-finished spec is never dragged along by a
    /// neighbour's publish.
    pub fn publish(&self, pathspecs: &[String], message: &str) -> Result<PublishOutcome> {
        let refs: Vec<&str> = pathspecs.iter().map(String::as_str).collect();

        Git::add_path(&self.dir, &refs)?;
        if Git::is_staging_clean(&self.dir)? {
            // A commit from an earlier run whose push failed is still ours to
            // land: staging is clean, but the remote has never seen it.
            if Git::commits_ahead(&self.dir)? > 0 {
                self.push()?;
                return Ok(PublishOutcome::Published);
            }
            return Ok(PublishOutcome::NothingToDo);
        }

        Git::commit(&self.dir, message)?;
        self.push()?;

        Ok(PublishOutcome::Published)
    }

    /// Publish uncommitted working-tree changes to `pathspecs`, keeping the
    /// push a fast-forward.
    ///
    /// rename and delete leave their change in the working tree, uncommitted.
    /// Committing straight onto a remote that has moved on since the last sync
    /// would fork history: the push would be rejected and the user left with a
    /// diverged local commit to untangle in git by hand — exactly the kind of
    /// bare-git detour AKM exists to spare them. So the branch is fast-forwarded
    /// onto the remote *first*, and only then does the change get committed on
    /// top, where the push cannot be refused.
    ///
    /// When the update would overwrite our own change (both sides touched the
    /// same files) that change is parked — including as a deletion, which is
    /// what a rename or delete leaves behind — the fast-forward retried, and our
    /// version laid back on top, so ours wins on any overlap without a merge
    /// ever running.
    pub fn publish_worktree(&self, pathspecs: &[String], message: &str) -> Result<PublishOutcome> {
        // Only rebase when the remote has actually moved. With no upstream, or
        // when we are already level with it, the change commits straight on top
        // and the push is a fast-forward regardless.
        if let Ok(upstream) = Git::upstream_ref(&self.dir) {
            if Git::rev_parse(&self.dir, "HEAD")? != Git::rev_parse(&self.dir, &upstream)? {
                self.rebase_worktree_onto(&upstream, pathspecs)?;
            }
        }
        self.publish(pathspecs, message)
    }

    /// Fast-forward onto `upstream` while keeping our uncommitted changes to
    /// `pathspecs`, ours winning on any path both sides touched.
    ///
    /// The changes — including deletions, which a rename or delete leaves — are
    /// parked and set aside first, so a plain fast-forward can run (an unstaged
    /// deletion does not block `merge --ff-only`; git would silently resurrect
    /// the file). Our version is then laid back on top. Unrelated uncommitted
    /// work that blocks the fast-forward is parked around it too, exactly as
    /// sync does, so one draft cannot wedge a publish.
    fn rebase_worktree_onto(&self, upstream: &str, pathspecs: &[String]) -> Result<()> {
        let diverged = || Error::RegistrySync {
            name: "personal".into(),
            message: "The library has local commits the registry has not seen.\n\
                      Run 'akm skills sync' first, then retry."
                .into(),
        };

        // 1. Set our change to these paths aside, then clean the paths back to
        //    HEAD so nothing under them can block the fast-forward.
        let refs: Vec<&str> = pathspecs.iter().map(String::as_str).collect();
        let mine: Vec<Parked> = self
            .changed_paths_under(pathspecs)?
            .iter()
            .map(|rel| Parked::take(&self.dir, rel))
            .collect::<Result<_>>()?;
        Git::restore_path(&self.dir, &refs)?;
        Git::clean_path(&self.dir, &refs)?;

        // 2. Fast-forward. Unrelated work that blocks it is parked and restored.
        let outcome = match Git::merge_ff_only(&self.dir, upstream)? {
            outcome @ (MergeOutcome::UpToDate | MergeOutcome::FastForwarded) => outcome,
            MergeOutcome::Blocked { paths } => {
                let other: Vec<Parked> = paths
                    .iter()
                    .map(|rel| Parked::take(&self.dir, rel))
                    .collect::<Result<_>>()?;
                let orefs: Vec<&str> = paths.iter().map(String::as_str).collect();
                Git::restore_path(&self.dir, &orefs)?;
                Git::clean_path(&self.dir, &orefs)?;
                let outcome = Git::merge_ff_only(&self.dir, upstream)?;
                for entry in &other {
                    entry.restore(&self.dir)?;
                }
                outcome
            }
            other => other,
        };

        // 3. Lay our change back on top — ours wins on any overlap. It is
        //    restored whatever happened, so a failed retry never costs the edit.
        for entry in &mine {
            entry.restore(&self.dir)?;
        }

        match outcome {
            MergeOutcome::UpToDate | MergeOutcome::FastForwarded => Ok(()),
            _ => Err(diverged()),
        }
    }

    /// Paths under `pathspecs` that differ from HEAD in the working tree,
    /// including untracked additions and deletions.
    fn changed_paths_under(&self, pathspecs: &[String]) -> Result<Vec<String>> {
        let under = |path: &str| {
            pathspecs
                .iter()
                .any(|ps| path == ps || path.starts_with(&format!("{ps}/")))
        };
        Ok(Git::status_porcelain(&self.dir)?
            .into_iter()
            .filter(|e| under(&e.path))
            .map(|e| e.path)
            .collect())
    }

    /// Switch to the branch a contribution will be pushed on.
    ///
    /// Starts from the remote's copy of that branch when it already has one, so
    /// re-sharing a spec fast-forwards an open pull request instead of being
    /// rejected as a non-fast-forward.
    pub fn checkout_contribution_branch(&self, branch: &str) -> Result<()> {
        let remote_branch = format!("origin/{branch}");
        let start = Git::rev_parse(&self.dir, &remote_branch)
            .ok()
            .map(|_| remote_branch);
        Git::checkout_new_branch(&self.dir, branch, start.as_deref())
    }

    /// Commit `pathspecs` on the current branch and push it to `origin`.
    ///
    /// Returns git's stderr, which carries the forge's own "open a pull
    /// request" URL — see [`Git::push_branch`].
    pub fn push_contribution(
        &self,
        branch: &str,
        pathspecs: &[String],
        message: &str,
    ) -> Result<Option<String>> {
        let refs: Vec<&str> = pathspecs.iter().map(String::as_str).collect();

        Git::add_path(&self.dir, &refs)?;
        if Git::is_staging_clean(&self.dir)? {
            return Ok(None);
        }

        Git::commit(&self.dir, message)?;
        Git::push_branch(&self.dir, branch)
            .map(Some)
            .map_err(|e| Error::RegistrySync {
                name: self.name.clone(),
                message: format!("Push failed: {e}"),
            })
    }

    /// Push, reporting a failure as a registry-sync error.
    fn push(&self) -> Result<()> {
        Git::push(&self.dir).map_err(|e| Error::RegistrySync {
            name: self.name.clone(),
            message: format!("Push failed: {e}"),
        })
    }

    /// Rebase local work onto the remote for one set of paths, ours winning.
    ///
    /// Used when publishing a spec the remote has also changed: take the
    /// remote's version of the path, lay the local content back over it, and
    /// commit. That yields a clean fast-forwardable history without ever
    /// running a merge that could leave conflict markers in a live skill.
    pub fn adopt_remote_then_keep_local(&self, pathspecs: &[String]) -> Result<()> {
        let refs: Vec<&str> = pathspecs.iter().map(String::as_str).collect();
        let upstream = Git::upstream_ref(&self.dir)?;

        let parked: Vec<Parked> = self
            .files_under(&refs)?
            .iter()
            .map(|rel| Parked::take(&self.dir, rel))
            .collect::<Result<_>>()?;

        Git::restore_path(&self.dir, &refs)?;
        Git::clean_path(&self.dir, &refs)?;
        Git::merge_ff_only(&self.dir, &upstream)?;

        // Anything the remote added under these paths but the local version
        // does not have would otherwise survive as a stray.
        Git::clean_path(&self.dir, &refs)?;
        Git::restore_path(&self.dir, &refs)?;
        for entry in &parked {
            entry.restore(&self.dir)?;
        }

        Ok(())
    }

    /// Discard local changes to `pathspecs`, back to the last synced state.
    pub fn revert_to_head(&self, pathspecs: &[String]) -> Result<()> {
        let refs: Vec<&str> = pathspecs.iter().map(String::as_str).collect();
        Git::restore_path(&self.dir, &refs)?;
        Git::clean_path(&self.dir, &refs)
    }

    /// Replace `pathspecs` with the remote's version, discarding local work.
    pub fn take_remote(&self, pathspecs: &[String]) -> Result<()> {
        let refs: Vec<&str> = pathspecs.iter().map(String::as_str).collect();
        let upstream = Git::upstream_ref(&self.dir)?;
        Git::clean_path(&self.dir, &refs)?;
        Git::checkout_from(&self.dir, &upstream, &refs)
    }

    /// Diff of the working tree against the last synced state.
    pub fn diff_local(&self, pathspecs: &[String]) -> Result<String> {
        let refs: Vec<&str> = pathspecs.iter().map(String::as_str).collect();
        Git::diff_worktree(&self.dir, "HEAD", &refs)
    }

    /// Diff of the last synced state against the fetched remote.
    pub fn diff_remote(&self, pathspecs: &[String]) -> Result<String> {
        let refs: Vec<&str> = pathspecs.iter().map(String::as_str).collect();
        let upstream = Git::upstream_ref(&self.dir)?;
        Git::diff(&self.dir, "HEAD", &upstream, &refs)
    }

    /// Get the derived index out of the working tree.
    ///
    /// The index is machine-local (it carries this machine's core deviations)
    /// and lives outside the checkout. Installs that predate that carry a copy
    /// inside it: either tracked — the registry was seeded when the index was
    /// committed, and libgen has been rewriting it into a permanent local
    /// modification ever since — or untracked, from the rc4 releases that hid
    /// it with `.git/info/exclude`.
    ///
    /// A tracked copy is restored to `HEAD` rather than deleted: it belongs to
    /// the registry's history, and removing it there is the owner's call, not
    /// a session-start side effect. Nothing writes it any more either way, so
    /// it stays clean from here on. `git clean` is no use for the untracked
    /// case — it skips excluded files, and this one is excluded by definition.
    pub fn evict_derived_index(&self) -> Result<()> {
        let stray = self.dir.join("library.json");
        if !stray.exists() {
            return Ok(());
        }

        if Git::is_tracked(&self.dir, "library.json")? {
            return Git::restore_path(&self.dir, &["library.json"]);
        }

        std::fs::remove_file(&stray).map_err(|source| Error::Io {
            context: format!("Removing the stale derived index {}", stray.display()),
            source,
        })
    }

    /// Every file currently on disk under the given pathspecs.
    fn files_under(&self, pathspecs: &[&str]) -> Result<Vec<String>> {
        let mut found = Vec::new();
        for rel in pathspecs {
            collect_files(&self.dir, Path::new(rel), &mut found)?;
        }
        Ok(found)
    }
}

/// Reduce a list of changed paths to the spec ids that own them.
fn spec_ids(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter_map(|path| SpecType::owner_of(path).map(|(_, id)| id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Recursively list files under `rel`, relative to `root`.
fn collect_files(root: &Path, rel: &Path, out: &mut Vec<String>) -> Result<()> {
    let abs = root.join(rel);

    if abs.is_file() {
        out.push(rel.to_string_lossy().to_string());
        return Ok(());
    }
    if !abs.is_dir() {
        return Ok(());
    }

    let entries = std::fs::read_dir(&abs).map_err(|source| Error::Io {
        context: format!("Reading {}", abs.display()),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| Error::Io {
            context: format!("Reading entry in {}", abs.display()),
            source,
        })?;
        collect_files(root, &rel.join(entry.file_name()), out)?;
    }
    Ok(())
}

/// One file's content, held aside across a git operation.
///
/// Kept in memory rather than in a temp directory: the lists are the handful
/// of paths git named as blocking, and an in-memory park cannot be left behind
/// by a crash.
struct Parked {
    rel: String,
    /// `None` when the file was deleted locally — the deletion is the edit.
    content: Option<Vec<u8>>,
}

impl Parked {
    fn take(root: &Path, rel: &str) -> Result<Self> {
        let abs = root.join(rel);
        let content = if abs.is_file() {
            Some(std::fs::read(&abs).map_err(|source| Error::Io {
                context: format!("Parking local changes to {}", abs.display()),
                source,
            })?)
        } else {
            None
        };
        Ok(Self {
            rel: rel.to_string(),
            content,
        })
    }

    fn restore(&self, root: &Path) -> Result<()> {
        let abs = root.join(&self.rel);
        match &self.content {
            Some(bytes) => {
                if let Some(parent) = abs.parent() {
                    std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                        context: format!("Creating {}", parent.display()),
                        source,
                    })?;
                }
                std::fs::write(&abs, bytes).map_err(|source| Error::Io {
                    context: format!("Restoring local changes to {}", abs.display()),
                    source,
                })
            }
            None if abs.exists() => std::fs::remove_file(&abs).map_err(|source| Error::Io {
                context: format!("Restoring local deletion of {}", abs.display()),
                source,
            }),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_ids_dedupes_and_ignores_non_spec_paths() {
        let paths = vec![
            "skills/tdd/SKILL.md".to_string(),
            "skills/tdd/akm.json".to_string(),
            "agents/reviewer.md".to_string(),
            "library.json".to_string(),
        ];
        assert_eq!(spec_ids(&paths), vec!["reviewer", "tdd"]);
    }

    #[test]
    fn an_unconfigured_registry_is_not_configured() {
        let registry = Registry::new("", "/nowhere");
        assert!(!registry.is_configured());
        assert!(!registry.is_cloned());
    }
}
