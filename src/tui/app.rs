//! Shared application state for the TUI.
//!
//! The `App` struct holds all data loaded from disk. It is created once
//! when the TUI starts and passed to each view. Views read from `App`
//! and may mutate it (e.g., toggling core flag updates the library).

use crate::commands::skills::{delete, import, rename, revert, shared};
use crate::error::{Error, Result};
use crate::git::Git;
use crate::library::drift::{DriftReport, DriftState};
use crate::library::manifest::Manifest;
use crate::library::spec::{Spec, SpecType};
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use crate::registry::Registry;
use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

/// Application state shared across TUI views.
pub struct App {
    /// Resolved XDG paths.
    pub paths: Paths,
    /// Loaded library (may be mutated by core toggle, edit).
    pub library: Library,
    /// Project root (if inside a git repo).
    pub project_root: Option<PathBuf>,
    /// Project name (basename of project root).
    pub project_name: Option<String>,
    /// Project manifest (if exists). May be mutated by add/remove.
    pub manifest: Option<Manifest>,
    /// Tool dirs configuration.
    pub tool_dirs: ToolDirs,
    /// Set of spec IDs currently in the manifest (for quick lookup).
    pub manifest_ids: HashSet<String>,
    /// Per-spec drift as of TUI start.
    ///
    /// Computed once: it shells out to git twice, which is far too expensive
    /// to redo per frame, and drift only changes when something outside the
    /// TUI does.
    pub drift: DriftReport,
    /// Whether the library has been modified and needs saving.
    pub library_dirty: bool,
    /// Spec ids whose human-facing metadata was edited in this session.
    ///
    /// Tracked separately from `library_dirty` because the two land in
    /// different places: metadata belongs in the spec's publishable sidecar,
    /// while a core toggle is a preference for this machine only.
    pub edited_meta: BTreeSet<String>,
    /// Whether the manifest has been modified and needs saving.
    pub manifest_dirty: bool,
    /// Library changes made this session that the remote has not seen.
    ///
    /// Rename and delete run eagerly against the working tree, but the publish
    /// prompt reads stdin line-by-line and cannot run inside the raw-mode TUI.
    /// So each such change is recorded here and offered for publishing once the
    /// terminal has been restored on exit.
    pub pending_publish: Vec<PendingPublish>,
}

/// A library change awaiting a publish offer after the TUI exits.
#[derive(Debug, Clone)]
pub struct PendingPublish {
    /// One-line summary shown to the user (e.g. "Renamed skill 'a' -> 'b'").
    pub summary: String,
    /// The paths whose change should be pushed.
    pub pathspecs: Vec<String>,
    /// The commit message for the push.
    pub message: String,
}

/// Result of a rename attempt in the TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameOutcome {
    /// The spec was renamed.
    Renamed,
    /// The proposed id is not a usable slug; carries the reason.
    InvalidId(String),
    /// Another spec already uses the proposed id.
    Collision,
    /// The spec to rename was not found.
    NotFound,
}

/// Result of a delete attempt in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteOutcome {
    /// The spec was deleted.
    Deleted,
    /// The spec to delete was not found.
    NotFound,
}

/// Result of reverting a spec to its last synced state from the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevertOutcome {
    /// Local edits were discarded; the spec now matches the registry.
    Reverted,
    /// The spec had no local changes, so there was nothing to revert.
    NothingToRevert,
    /// The library is not a registry checkout — `akm skills sync` first.
    NotACheckout,
    /// The spec to revert was not found.
    NotFound,
}

/// Result of an add-to-manifest operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddResult {
    /// Spec was added successfully.
    Added,
    /// Spec was already in the manifest.
    AlreadyPresent,
    /// No project root detected (not inside a git repo).
    NoProject,
    /// Spec ID not found in the library.
    SpecNotFound,
}

/// Result of a remove-from-manifest operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveResult {
    /// Spec was removed successfully.
    Removed,
    /// Spec was not in the manifest.
    NotPresent,
    /// No manifest exists.
    NoManifest,
}

impl App {
    /// Create a new App by loading all data from disk.
    ///
    /// # Errors
    /// Returns error if library cannot be loaded (required).
    /// Manifest and project root are optional — missing is not an error.
    pub fn new(paths: Paths, tool_dirs: ToolDirs) -> Result<Self> {
        let library = Library::load_checked(&paths)?;

        let project_root = Git::toplevel(None).ok();
        let project_name = project_root
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string());

        let manifest = project_root
            .as_ref()
            .and_then(|root| Manifest::load(root).ok());

        let manifest_ids: HashSet<String> = manifest
            .as_ref()
            .map(|m| {
                m.skill_ids()
                    .iter()
                    .chain(m.agent_ids().iter())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        // Advisory only — a library that is not a git checkout still opens.
        let drift = DriftReport::compute(&paths.library_dir()).unwrap_or_default();

        Ok(Self {
            paths,
            library,
            project_root,
            project_name,
            manifest,
            tool_dirs,
            manifest_ids,
            drift,
            library_dirty: false,
            edited_meta: BTreeSet::new(),
            manifest_dirty: false,
            pending_publish: Vec::new(),
        })
    }

    /// Get filtered specs based on optional tag and type filters.
    ///
    /// Used by the list view. Applies CLI-level filters (--tag, --type)
    /// before the interactive search filter is applied.
    pub fn filtered_specs(&self, tag: Option<&str>, type_filter: Option<SpecType>) -> Vec<&Spec> {
        self.library
            .specs
            .iter()
            .filter(|spec| {
                if let Some(tag) = tag {
                    if !spec.tags.iter().any(|t| t == tag) {
                        return false;
                    }
                }
                if let Some(tf) = type_filter {
                    if spec.spec_type != tf {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Apply interactive search filter to a list of specs.
    ///
    /// Case-insensitive substring
    /// match against id + description + tags.
    pub fn search_filter<'a>(specs: &[&'a Spec], query: &str) -> Vec<&'a Spec> {
        if query.is_empty() {
            return specs.to_vec();
        }
        let lquery = query.to_lowercase();
        specs
            .iter()
            .filter(|spec| {
                let tags_str = spec.tags.join(",");
                let searchable =
                    format!("{} {} {}", spec.id, spec.description, tags_str).to_lowercase();
                searchable.contains(&lquery)
            })
            .copied()
            .collect()
    }

    /// Toggle the core flag for a spec. Returns the new core value.
    ///
    /// Mutates the in-memory library; the change is persisted on exit as a
    /// machine-local deviation, never as a change to the published default.
    /// Promoting it for every machine is what `akm skills core --publish` is
    /// for.
    pub fn toggle_core(&mut self, spec_id: &str) -> Option<bool> {
        if let Some(spec) = self.library.get_mut(spec_id) {
            spec.core = !spec.core;
            self.library_dirty = true;
            Some(spec.core)
        } else {
            None
        }
    }

    /// Add a spec to the project manifest.
    ///
    /// Returns a typed result distinguishing success, already-present,
    /// no-project, and spec-not-found cases for contextual user feedback.
    pub fn add_to_manifest(&mut self, spec_id: &str) -> Result<AddResult> {
        let project_root = match &self.project_root {
            Some(root) => root.clone(),
            None => return Ok(AddResult::NoProject),
        };

        if self.manifest_ids.contains(spec_id) {
            return Ok(AddResult::AlreadyPresent);
        }

        let spec = match self.library.get(spec_id) {
            Some(s) => s.clone(),
            None => return Ok(AddResult::SpecNotFound),
        };

        if self.manifest.is_none() {
            self.manifest = Some(Manifest::load_or_create(&project_root)?);
        }
        let Some(manifest) = &mut self.manifest else {
            return Ok(AddResult::NoProject);
        };

        let added = manifest.add(&spec.id, spec.spec_type);
        if !added {
            return Ok(AddResult::AlreadyPresent);
        }
        self.manifest_ids.insert(spec_id.to_string());
        self.manifest_dirty = true;
        Ok(AddResult::Added)
    }

    /// Remove a spec from the project manifest.
    ///
    /// Returns a typed result distinguishing success, not-present,
    /// and no-manifest cases for contextual user feedback.
    pub fn remove_from_manifest(&mut self, spec_id: &str) -> Result<RemoveResult> {
        if !self.manifest_ids.contains(spec_id) {
            return Ok(RemoveResult::NotPresent);
        }

        let Some(manifest) = &mut self.manifest else {
            return Ok(RemoveResult::NoManifest);
        };

        let spec_type = self.library.get(spec_id).map(|s| s.spec_type);
        let removed = manifest.remove(spec_id, spec_type);
        if !removed {
            return Ok(RemoveResult::NotPresent);
        }
        self.manifest_ids.remove(spec_id);
        self.manifest_dirty = true;
        Ok(RemoveResult::Removed)
    }

    /// Rename a spec's id, eagerly, from within the TUI.
    ///
    /// Unlike core/manifest toggles this cannot be deferred to exit: it moves
    /// files and rewrites the index. Deferred edits are flushed first so the
    /// reload afterwards does not drop them, and the change is queued for a
    /// publish offer once the terminal is restored.
    pub fn rename_spec(&mut self, old: &str, new: &str) -> Result<RenameOutcome> {
        let Some(spec) = self.library.get(old) else {
            return Ok(RenameOutcome::NotFound);
        };
        let spec_type = spec.spec_type;
        let mut pathspecs = spec_type.pathspecs(old);
        pathspecs.extend(spec_type.pathspecs(new));

        self.flush_deferred()?;

        match rename::apply(&self.paths, old, new, &self.tool_dirs) {
            Ok(_) => {}
            Err(Error::InvalidSpecId { reason, .. }) => {
                return Ok(RenameOutcome::InvalidId(reason))
            }
            Err(Error::SpecAlreadyExists { .. }) => return Ok(RenameOutcome::Collision),
            Err(Error::SpecNotFound { .. }) => return Ok(RenameOutcome::NotFound),
            Err(e) => return Err(e),
        }

        if self.manifest_ids.remove(old) {
            if let Some(manifest) = &mut self.manifest {
                manifest.remove(old, Some(spec_type));
                manifest.add(new, spec_type);
            }
            self.manifest_ids.insert(new.to_string());
            self.manifest_dirty = true;
        }

        self.reload_library()?;
        self.pending_publish.push(PendingPublish {
            summary: format!("Renamed {spec_type} '{old}' -> '{new}'"),
            pathspecs,
            message: format!("refactor: rename {spec_type} '{old}' -> '{new}'"),
        });
        Ok(RenameOutcome::Renamed)
    }

    /// Delete a spec from the library, eagerly, from within the TUI.
    ///
    /// See [`Self::rename_spec`] for why this cannot be deferred to exit.
    pub fn delete_spec(&mut self, id: &str) -> Result<DeleteOutcome> {
        let Some(spec) = self.library.get(id) else {
            return Ok(DeleteOutcome::NotFound);
        };
        let spec_type = spec.spec_type;
        let pathspecs = spec.pathspecs();

        self.flush_deferred()?;

        match delete::apply(&self.paths, id, &self.tool_dirs) {
            Ok(()) => {}
            Err(Error::SpecNotFound { .. }) => return Ok(DeleteOutcome::NotFound),
            Err(e) => return Err(e),
        }

        if self.manifest_ids.remove(id) {
            if let Some(manifest) = &mut self.manifest {
                manifest.remove(id, Some(spec_type));
            }
            self.manifest_dirty = true;
        }

        self.reload_library()?;
        self.pending_publish.push(PendingPublish {
            summary: format!("Deleted {spec_type} '{id}'"),
            pathspecs,
            message: format!("chore: remove {spec_type} '{id}'"),
        });
        Ok(DeleteOutcome::Deleted)
    }

    /// Import one shared-registry skill into the library, eagerly, from the TUI.
    ///
    /// Mirrors [`Self::rename_spec`]: deferred edits are flushed first so the
    /// reload afterwards does not drop them, the copy is applied on disk, and the
    /// change is queued for a publish offer once the terminal is restored.
    pub fn import_shared_candidate(
        &mut self,
        candidate: &shared::Candidate,
        remote: &str,
        url: &str,
    ) -> Result<()> {
        self.flush_deferred()?;
        let pathspecs =
            import::import_candidate(&self.paths, &self.tool_dirs, candidate, remote, url)?;
        self.reload_library()?;
        let id = &candidate.id;
        self.pending_publish.push(PendingPublish {
            summary: format!("Imported skill '{id}' from '{remote}'"),
            pathspecs,
            message: format!("feat: import skill '{id}' from '{remote}'"),
        });
        Ok(())
    }

    /// Queue a drifted spec to be published once the TUI exits.
    ///
    /// Publishing pushes to a remote and reads a `y/N` from stdin, neither of
    /// which can happen inside the raw-mode TUI — so, exactly like rename and
    /// delete, the intent is recorded and offered on the restored terminal via
    /// [`crate::tui::list`]'s exit handler.
    ///
    /// Only specs the remote has not already seen are queued; returns the drift
    /// state that was queued, or `None` when there was nothing to publish.
    pub fn queue_publish(&mut self, id: &str) -> Option<DriftState> {
        let state = self.drift.state_of(id);
        if !state.has_local_changes() {
            return None;
        }
        let spec = self.library.get(id)?;
        let spec_type = spec.spec_type;
        self.pending_publish.push(PendingPublish {
            summary: format!("Publish {spec_type} '{id}'"),
            pathspecs: spec.pathspecs(),
            message: format!("feat: publish {spec_type} '{id}'"),
        });
        Some(state)
    }

    /// Discard a spec's local edits back to the last synced state, eagerly.
    ///
    /// The revert to `HEAD` is a local git operation — no network — so unlike
    /// publishing it can run inside the TUI, mirroring [`Self::delete_spec`]:
    /// deferred edits are flushed first so the reload afterwards keeps them, the
    /// files are restored on disk, the derived index and symlinks are rebuilt,
    /// and drift is recomputed so the marker clears on screen. Reverting to the
    /// *remote* ("take theirs") touches the network and stays a CLI-only affair.
    pub fn revert_spec(&mut self, id: &str) -> Result<RevertOutcome> {
        let Some(spec) = self.library.get(id) else {
            return Ok(RevertOutcome::NotFound);
        };
        if !self.drift.state_of(id).has_local_changes() {
            return Ok(RevertOutcome::NothingToRevert);
        }
        let pathspecs = spec.pathspecs();

        self.flush_deferred()?;

        let registry = Registry::new(String::new(), self.paths.library_dir());
        if !registry.is_cloned() {
            return Ok(RevertOutcome::NotACheckout);
        }
        registry.revert_to_head(&pathspecs)?;
        revert::rebuild_after_revert(&self.paths, &self.tool_dirs)?;

        self.reload_library()?;
        Ok(RevertOutcome::Reverted)
    }

    /// Persist any deferred edits (core toggles, metadata, manifest) and clear
    /// the dirty flags, so a subsequent reload from disk keeps them.
    fn flush_deferred(&mut self) -> Result<()> {
        self.save_if_dirty()?;
        self.library_dirty = false;
        self.edited_meta.clear();
        self.manifest_dirty = false;
        Ok(())
    }

    /// Reload the library index and drift after an eager on-disk change.
    fn reload_library(&mut self) -> Result<()> {
        self.library = Library::load_from(&self.paths.library_json())?;
        self.drift = DriftReport::compute(&self.paths.library_dir()).unwrap_or_default();
        Ok(())
    }

    /// Save any dirty state to disk and rebuild symlinks. Called on TUI exit.
    ///
    /// This is the single point where mutations are persisted. `library.json`
    /// is derived and regenerated here rather than written from memory, so
    /// each edit is routed to the file that actually owns it: metadata to the
    /// spec's sidecar, core toggles to `local.json`.
    pub fn save_if_dirty(&self) -> Result<()> {
        if self.manifest_dirty {
            if let Some(manifest) = &self.manifest {
                manifest.save()?;
            }
        }

        if !self.library_dirty && self.edited_meta.is_empty() {
            return Ok(());
        }

        let library_dir = self.paths.library_dir();

        // Metadata first: it is what the regenerated index is built from.
        for id in &self.edited_meta {
            let Some(spec) = self.library.get(id) else {
                continue;
            };
            let sidecar = spec.sidecar_path(&library_dir);
            let mut meta = if sidecar.is_file() {
                crate::library::spec::SpecMeta::load_from(&sidecar)?
            } else {
                spec.to_meta()
            };
            meta.name = spec.name.clone();
            meta.description = spec.description.clone();
            meta.tags = spec.tags.clone();
            // `core` is deliberately not written: see `toggle_core`.
            meta.save_to(&sidecar)?;
        }

        crate::library::libgen::generate(&library_dir, &self.paths.library_json())?;
        let mut published = Library::load_from(&self.paths.library_json())?;

        // Whatever the session's core flags disagree with the published
        // defaults about becomes this machine's deviation.
        let mut overrides =
            crate::library::local::LocalOverrides::load_from(&self.paths.local_json())?;
        for spec in &published.specs {
            if let Some(current) = self.library.get(&spec.id) {
                overrides.set_core(&spec.id, current.core, spec.core);
            }
        }
        overrides.apply(&mut published);
        overrides.save_to(&self.paths.local_json())?;
        published.save_to(&self.paths.library_json())?;

        crate::library::symlinks::rebuild_core(
            &published.core_specs(),
            &library_dir,
            self.tool_dirs.dirs(),
        )?;

        Ok(())
    }

    /// Read the SKILL.md/agent .md content for a spec.
    ///
    /// Returns the full markdown content as a string.
    /// Used by the detail view (Enter from list).
    pub fn read_spec_content(&self, spec_id: &str) -> Result<String> {
        let spec = self
            .library
            .get(spec_id)
            .ok_or_else(|| crate::error::Error::SpecNotFound {
                id: spec_id.to_string(),
            })?;

        let md_path = spec.markdown_path(&self.paths.library_dir());

        if md_path.exists() {
            std::fs::read_to_string(&md_path).map_err(|e| crate::error::Error::Io {
                context: format!("Reading {}", md_path.display()),
                source: e,
            })
        } else {
            Ok(format!("(No content file found at {})", md_path.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::spec::{Spec, SpecType};
    use crate::library::Library;

    /// Create a test library with known specs.
    fn test_library() -> Library {
        Library {
            version: 1,
            specs: vec![
                Spec::new(
                    "test-driven-development",
                    SpecType::Skill,
                    "TDD",
                    "TDD methodology for implementing features",
                ),
                Spec {
                    core: true,
                    tags: vec!["testing".to_string(), "tdd".to_string()],
                    ..Spec::new(
                        "verification-before-completion",
                        SpecType::Skill,
                        "Verify",
                        "Verify before claiming done",
                    )
                },
                Spec::new(
                    "code-review-agent",
                    SpecType::Agent,
                    "Code Review",
                    "Reviews code changes",
                ),
                Spec {
                    tags: vec!["git".to_string()],
                    ..Spec::new(
                        "git-commit",
                        SpecType::Skill,
                        "Git Commit",
                        "Structured git commits",
                    )
                },
            ],
        }
    }

    #[test]
    fn test_search_filter_empty_query_returns_all() {
        let library = test_library();
        let all_specs: Vec<&Spec> = library.specs.iter().collect();
        let result = App::search_filter(&all_specs, "");
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_search_filter_matches_id() {
        let library = test_library();
        let all_specs: Vec<&Spec> = library.specs.iter().collect();
        let result = App::search_filter(&all_specs, "git-commit");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "git-commit");
    }

    #[test]
    fn test_search_filter_case_insensitive() {
        let library = test_library();
        let all_specs: Vec<&Spec> = library.specs.iter().collect();
        let result = App::search_filter(&all_specs, "TDD");
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|s| s.id == "test-driven-development"));
        assert!(result
            .iter()
            .any(|s| s.id == "verification-before-completion"));
    }

    #[test]
    fn test_search_filter_matches_description() {
        let library = test_library();
        let all_specs: Vec<&Spec> = library.specs.iter().collect();
        let result = App::search_filter(&all_specs, "Reviews code");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "code-review-agent");
    }

    #[test]
    fn test_search_filter_matches_tags() {
        let library = test_library();
        let all_specs: Vec<&Spec> = library.specs.iter().collect();
        let result = App::search_filter(&all_specs, "git");
        assert!(result.iter().any(|s| s.id == "git-commit"));
    }

    #[test]
    fn test_search_filter_no_match() {
        let library = test_library();
        let all_specs: Vec<&Spec> = library.specs.iter().collect();
        let result = App::search_filter(&all_specs, "xyznonexistent");
        assert!(result.is_empty());
    }

    #[test]
    fn test_toggle_core_on() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(tmp.path(), tmp.path(), tmp.path(), tmp.path());
        let library = test_library();
        library.save(&paths).unwrap();
        let tool_dirs = ToolDirs::builtin(tmp.path());
        let mut app = App::new(paths, tool_dirs).unwrap();

        let result = app.toggle_core("test-driven-development");
        assert_eq!(result, Some(true));
        assert!(app.library_dirty);

        let result = app.toggle_core("test-driven-development");
        assert_eq!(result, Some(false));
    }

    #[test]
    fn test_toggle_core_nonexistent_spec() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(tmp.path(), tmp.path(), tmp.path(), tmp.path());
        let library = test_library();
        library.save(&paths).unwrap();
        let tool_dirs = ToolDirs::builtin(tmp.path());
        let mut app = App::new(paths, tool_dirs).unwrap();

        let result = app.toggle_core("nonexistent-spec-id");
        assert_eq!(result, None);
        assert!(!app.library_dirty);
    }

    #[test]
    fn test_plain_flag_forces_plain() {
        assert!(!crate::commands::skills::list::should_use_tui(true));
    }

    #[test]
    fn test_filtered_specs_by_type() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(tmp.path(), tmp.path(), tmp.path(), tmp.path());
        let library = test_library();
        library.save(&paths).unwrap();
        let tool_dirs = ToolDirs::builtin(tmp.path());
        let app = App::new(paths, tool_dirs).unwrap();

        let agents = app.filtered_specs(None, Some(SpecType::Agent));
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, "code-review-agent");
    }

    #[test]
    fn test_filtered_specs_by_tag() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(tmp.path(), tmp.path(), tmp.path(), tmp.path());
        let library = test_library();
        library.save(&paths).unwrap();
        let tool_dirs = ToolDirs::builtin(tmp.path());
        let app = App::new(paths, tool_dirs).unwrap();

        let tagged = app.filtered_specs(Some("testing"), None);
        assert_eq!(tagged.len(), 1);
        assert_eq!(tagged[0].id, "verification-before-completion");
    }

    #[test]
    fn test_read_spec_content_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(tmp.path(), tmp.path(), tmp.path(), tmp.path());
        let library = test_library();
        library.save(&paths).unwrap();
        let tool_dirs = ToolDirs::builtin(tmp.path());
        let app = App::new(paths, tool_dirs).unwrap();

        let content = app.read_spec_content("test-driven-development").unwrap();
        assert!(content.contains("No content file found"));
    }

    #[test]
    fn test_read_spec_content_nonexistent_spec() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(tmp.path(), tmp.path(), tmp.path(), tmp.path());
        let library = test_library();
        library.save(&paths).unwrap();
        let tool_dirs = ToolDirs::builtin(tmp.path());
        let app = App::new(paths, tool_dirs).unwrap();

        let result = app.read_spec_content("does-not-exist");
        assert!(result.is_err());
    }
}
