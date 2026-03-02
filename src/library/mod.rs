//! Library — the collection of specs in `library.json`.
//!
//! This module handles loading, saving, and querying the spec library.
//! The library is the "cold storage" index that tracks all installed
//! skills and agents.

pub mod frontmatter;
pub mod libgen;
pub mod manifest;
pub mod spec;

use crate::error::{Error, IoContext, Result};
use crate::paths::Paths;
use spec::Spec;

use serde::{Deserialize, Serialize};
use std::path::Path;

/// The library.json format version. Currently always 1.
const LIBRARY_VERSION: u32 = 1;

/// In-memory representation of `library.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    /// Format version (currently 1).
    pub version: u32,
    /// All known specs.
    pub specs: Vec<Spec>,
}

impl Library {
    /// Create an empty library.
    pub fn new() -> Self {
        Self {
            version: LIBRARY_VERSION,
            specs: Vec::new(),
        }
    }

    /// Load library.json from the default path.
    ///
    /// Returns `Err(LibraryNotFound)` if the file doesn't exist.
    pub fn load(paths: &Paths) -> Result<Self> {
        let path = paths.library_json();
        Self::load_from(&path)
    }

    /// Load library.json from a specific path.
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(Error::LibraryNotFound {
                path: path.to_path_buf(),
            });
        }

        let content = std::fs::read_to_string(path)
            .io_context(format!("Reading library.json from {}", path.display()))?;

        serde_json::from_str(&content).map_err(|e| Error::LibraryParse {
            path: path.to_path_buf(),
            source: Box::new(e),
        })
    }

    /// Load library, returning a default empty library if the file doesn't exist.
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new());
        }
        Self::load_from(path)
    }

    /// Load library and fail with a user-friendly error if not found.
    ///
    /// This is the designated entry point for commands that require an existing
    /// library. Currently delegates to `load()` — the `LibraryNotFound` error
    /// already contains user-facing guidance ("Run 'akm skills sync'...").
    pub fn load_checked(paths: &Paths) -> Result<Self> {
        Self::load(paths)
    }

    /// Save library.json to the default path.
    pub fn save(&self, paths: &Paths) -> Result<()> {
        let path = paths.library_json();
        self.save_to(&path)
    }

    /// Save library.json to a specific path.
    ///
    /// Uses atomic write pattern (write to .tmp, then rename).
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .io_context(format!("Creating directory for {}", path.display()))?;
        }

        let content = serde_json::to_string_pretty(self).map_err(|e| Error::LibraryWrite {
            path: path.to_path_buf(),
            source: std::io::Error::other(e),
        })?;
        let content = format!("{content}\n");

        let tmp_path = path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &content).map_err(|e| Error::LibraryWrite {
            path: path.to_path_buf(),
            source: e,
        })?;

        std::fs::rename(&tmp_path, path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            Error::LibraryWrite {
                path: path.to_path_buf(),
                source: e,
            }
        })
    }

    // --- Query methods ---

    /// Look up a spec by ID.
    pub fn get(&self, id: &str) -> Option<&Spec> {
        self.specs.iter().find(|s| s.id == id)
    }

    /// Look up a spec by ID (mutable).
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Spec> {
        self.specs.iter_mut().find(|s| s.id == id)
    }

    /// Check if a spec exists by ID.
    pub fn contains(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    /// Get all spec IDs.
    pub fn all_ids(&self) -> Vec<&str> {
        self.specs.iter().map(|s| s.id.as_str()).collect()
    }

    /// Get IDs of core specs (globally symlinked).
    pub fn core_ids(&self) -> Vec<&str> {
        self.specs
            .iter()
            .filter(|s| s.core)
            .map(|s| s.id.as_str())
            .collect()
    }

    /// Get core specs.
    pub fn core_specs(&self) -> Vec<&Spec> {
        self.specs.iter().filter(|s| s.core).collect()
    }

    /// Return the number of specs.
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// Check if the library is empty.
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// Build a lookup map from ID to Spec for efficient repeated lookups.
    pub fn spec_map(&self) -> std::collections::HashMap<&str, &Spec> {
        self.specs.iter().map(|s| (s.id.as_str(), s)).collect()
    }
}

impl Default for Library {
    fn default() -> Self {
        Self::new()
    }
}
