//! Shared application state for the TUI.
//!
//! The `App` struct holds all data loaded from disk. It is created once
//! when the TUI starts and passed to each view. Views read from `App`
//! and may mutate it (e.g., toggling core flag updates the library).

use crate::error::Result;
use crate::git::Git;
use crate::library::manifest::Manifest;
use crate::library::spec::{Spec, SpecType};
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use std::collections::HashSet;
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
    /// Whether the library has been modified and needs saving.
    pub library_dirty: bool,
    /// Whether the manifest has been modified and needs saving.
    pub manifest_dirty: bool,
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

        Ok(Self {
            paths,
            library,
            project_root,
            project_name,
            manifest,
            tool_dirs,
            manifest_ids,
            library_dirty: false,
            manifest_dirty: false,
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
    /// Mirrors Bash `cmd_skills_search` logic: case-insensitive substring
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
    /// Mutates the in-memory library. The caller is responsible for
    /// saving the library to disk (done on exit).
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

    /// Save any dirty state to disk. Called on TUI exit.
    ///
    /// This is the single point where mutations are persisted.
    pub fn save_if_dirty(&self) -> Result<()> {
        if self.library_dirty {
            self.library.save(&self.paths)?;
        }
        if self.manifest_dirty {
            if let Some(manifest) = &self.manifest {
                manifest.save()?;
            }
        }
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

        let md_path = spec.markdown_path(self.paths.data_dir());

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
