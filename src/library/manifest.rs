//! Project manifest — `.agents/akm.json` read/write.
//!
//! The manifest declares which specs a project uses. These are loaded into
//! the per-session staging directory at session start (Layer 2 activation).
//!
//! Bash: `_manifest_path()`, `_ensure_manifest()`, `_read_manifest_ids()`

use crate::error::{Error, IoContext, Result};
use crate::library::spec::SpecType;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Project manifest (.agents/akm.json).
///
/// Tracks which skills and agents are activated for a project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    /// Skill IDs declared for this project.
    #[serde(default)]
    pub skills: Vec<String>,
    /// Agent IDs declared for this project.
    #[serde(default)]
    pub agents: Vec<String>,

    /// The file path this manifest was loaded from (not serialized).
    #[serde(skip)]
    file_path: PathBuf,
}

impl Manifest {
    /// Compute the manifest path for a given project root.
    ///
    /// Bash: `_manifest_path()` at bin/akm:182
    pub fn path(project_root: &Path) -> PathBuf {
        project_root.join(".agents").join("akm.json")
    }

    /// Load an existing manifest from disk.
    pub fn load(project_root: &Path) -> Result<Self> {
        let file_path = Self::path(project_root);
        if !file_path.exists() {
            return Err(Error::PathNotFound {
                path: file_path,
                description: "Project manifest".into(),
            });
        }

        let content = std::fs::read_to_string(&file_path)
            .io_context(format!("Reading manifest {}", file_path.display()))?;

        let mut manifest: Manifest =
            serde_json::from_str(&content).map_err(|e| Error::ManifestParse {
                path: file_path.clone(),
                source: Box::new(e),
            })?;
        manifest.file_path = file_path;
        Ok(manifest)
    }

    /// Load manifest if it exists, or create a new empty one.
    ///
    /// Bash: `_ensure_manifest()` at bin/akm:192
    /// Idempotent — safe to call multiple times.
    pub fn load_or_create(project_root: &Path) -> Result<Self> {
        let file_path = Self::path(project_root);

        if file_path.exists() {
            return Self::load(project_root);
        }

        let dir = file_path.parent().ok_or_else(|| Error::Io {
            context: format!("Invalid manifest path: {}", file_path.display()),
            source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent"),
        })?;

        std::fs::create_dir_all(dir)
            .io_context(format!("Creating manifest directory {}", dir.display()))?;

        let manifest = Manifest {
            skills: Vec::new(),
            agents: Vec::new(),
            file_path: file_path.clone(),
        };

        manifest.save()?;
        Ok(manifest)
    }

    /// Save the manifest to disk.
    ///
    /// Uses atomic write pattern (write to .tmp, then rename) for safety.
    pub fn save(&self) -> Result<()> {
        let content = serde_json::to_string_pretty(self).map_err(|e| Error::ManifestWrite {
            path: self.file_path.clone(),
            source: std::io::Error::other(e),
        })?;
        let content = format!("{content}\n");

        let tmp_path = self.file_path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &content).map_err(|e| Error::ManifestWrite {
            path: self.file_path.clone(),
            source: e,
        })?;

        std::fs::rename(&tmp_path, &self.file_path).map_err(|e| Error::ManifestWrite {
            path: self.file_path.clone(),
            source: e,
        })?;

        Ok(())
    }

    /// Get skill IDs.
    ///
    /// Bash: `_read_manifest_ids "$manifest" "skill"`
    pub fn skill_ids(&self) -> &[String] {
        &self.skills
    }

    /// Get agent IDs.
    ///
    /// Bash: `_read_manifest_ids "$manifest" "agent"`
    pub fn agent_ids(&self) -> &[String] {
        &self.agents
    }

    /// Get all IDs (skills + agents).
    pub fn all_ids(&self) -> Vec<&str> {
        self.skills
            .iter()
            .chain(self.agents.iter())
            .map(|s| s.as_str())
            .collect()
    }

    /// Check if a spec ID is in the manifest (either skills or agents).
    pub fn contains(&self, id: &str) -> bool {
        self.skills.iter().any(|s| s == id) || self.agents.iter().any(|s| s == id)
    }

    /// Add a spec to the manifest. Idempotent — no-op if already present.
    ///
    /// Returns `true` if the ID was added, `false` if already present.
    pub fn add(&mut self, id: &str, spec_type: SpecType) -> bool {
        let list = match spec_type {
            SpecType::Skill => &mut self.skills,
            SpecType::Agent => &mut self.agents,
        };

        if list.iter().any(|s| s == id) {
            return false;
        }

        list.push(id.to_string());
        list.sort();
        true
    }

    /// Remove a spec from the manifest. Idempotent — no-op if not present.
    ///
    /// If `spec_type` is provided, only removes from that array.
    /// If `None`, tries both arrays.
    ///
    /// Returns `true` if the ID was removed from any array.
    pub fn remove(&mut self, id: &str, spec_type: Option<SpecType>) -> bool {
        match spec_type {
            Some(SpecType::Skill) => {
                let before = self.skills.len();
                self.skills.retain(|s| s != id);
                self.skills.len() < before
            }
            Some(SpecType::Agent) => {
                let before = self.agents.len();
                self.agents.retain(|s| s != id);
                self.agents.len() < before
            }
            None => {
                let skills_before = self.skills.len();
                self.skills.retain(|s| s != id);
                let agents_before = self.agents.len();
                self.agents.retain(|s| s != id);
                self.skills.len() < skills_before || self.agents.len() < agents_before
            }
        }
    }

    /// The file path this manifest is stored at.
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }
}
