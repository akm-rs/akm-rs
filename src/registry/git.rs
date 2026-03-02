//! Git-based registry implementation.
//!
//! Uses the `Git` helper from `git.rs` (Task 1) for all git operations.
//! This is the only module that knows about git semantics — the rest of
//! AKM interacts through the `RegistrySource` trait.

use crate::error::{Error, Result};
use crate::git::Git;
use crate::registry::{PullOutcome, RegistrySource};
use std::path::{Path, PathBuf};

/// A git-backed spec registry.
///
/// Represents a remote git repository containing skills/, agents/,
/// and optionally library.json. The repository is cloned/pulled into
/// a local cache directory.
///
/// Config mapping:
/// - Community: `skills.community_registry` → cache at `Paths::community_registry_cache()`
/// - Personal: `skills.personal_registry` → cache at `Paths::personal_registry_cache()`
pub struct GitRegistry {
    /// Human-readable name for display and error messages.
    display_name: String,
    /// Remote URL (HTTPS or SSH).
    url: String,
    /// Local cache directory path.
    cache_path: PathBuf,
}

impl GitRegistry {
    /// Create a new GitRegistry.
    ///
    /// # Arguments
    /// * `name` — Display name (e.g., "community", "personal")
    /// * `url` — Git remote URL
    /// * `cache_path` — Local directory for the cloned repository
    pub fn new(
        name: impl Into<String>,
        url: impl Into<String>,
        cache_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            display_name: name.into(),
            url: url.into(),
            cache_path: cache_path.into(),
        }
    }

    /// The remote URL for this registry.
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl RegistrySource for GitRegistry {
    fn name(&self) -> &str {
        &self.display_name
    }

    /// Clone or pull the git repository to the cache directory.
    fn pull(&self) -> Result<PullOutcome> {
        if Git::is_repo(&self.cache_path) {
            Git::pull(&self.cache_path).map_err(|e| Error::RegistrySync {
                name: self.display_name.clone(),
                message: format!("Pull failed: {e}"),
            })?;
            Ok(PullOutcome::Updated)
        } else {
            Git::clone(&self.url, &self.cache_path).map_err(|e| Error::RegistrySync {
                name: self.display_name.clone(),
                message: format!("Clone failed: {e}"),
            })?;
            Ok(PullOutcome::Fetched)
        }
    }

    /// Push local cache to the remote.
    fn push(&self) -> Result<()> {
        if !Git::is_repo(&self.cache_path) {
            return Err(Error::RegistrySync {
                name: self.display_name.clone(),
                message: "No local cache to push from. Run 'akm skills sync' first.".into(),
            });
        }
        Git::push(&self.cache_path).map_err(|e| Error::RegistrySync {
            name: self.display_name.clone(),
            message: format!("Push failed: {e}"),
        })
    }

    /// Check if this registry has a non-empty URL.
    fn is_available(&self) -> bool {
        !self.url.is_empty()
    }

    fn cache_dir(&self) -> &Path {
        &self.cache_path
    }

    /// Check if a local cached copy exists.
    fn is_cached(&self) -> bool {
        Git::is_repo(&self.cache_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn new_registry_not_cached() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new(
            "test",
            "https://example.com/repo.git",
            tmp.path().join("cache"),
        );
        assert!(!reg.is_cached());
        assert!(reg.is_available());
        assert_eq!(reg.name(), "test");
        assert_eq!(reg.url(), "https://example.com/repo.git");
    }

    #[test]
    fn empty_url_not_available() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "", tmp.path().join("cache"));
        assert!(!reg.is_available());
    }
}
