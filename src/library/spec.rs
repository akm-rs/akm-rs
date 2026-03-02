//! Spec model — represents a skill or agent in the AKM library.
//!
//! Each spec has an ID, type, name, description, and metadata (tags, core flag,
//! triggers). This struct is the serialized form stored in `library.json`.
//!
//! Corresponds to individual JSON objects in the Bash `library.json`:
//! ```json
//! {"id":"test-driven-development","type":"skill","name":"Test-Driven Development",
//!  "description":"Use when...","core":false,"tags":["testing"],"triggers":{}}
//! ```

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The type of a spec: skill (directory with SKILL.md) or agent (single .md file).
///
/// Bash: `_spec_type()` returns "skill" or "agent" from library.json.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpecType {
    /// A skill: directory containing SKILL.md and optional references/.
    Skill,
    /// An agent: single .md file.
    Agent,
}

impl SpecType {
    /// The subdirectory name for this spec type.
    ///
    /// Bash: `_type_subdir()` at bin/akm:156
    pub fn subdir(&self) -> &'static str {
        match self {
            SpecType::Skill => "skills",
            SpecType::Agent => "agents",
        }
    }
}

impl std::fmt::Display for SpecType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpecType::Skill => write!(f, "skill"),
            SpecType::Agent => write!(f, "agent"),
        }
    }
}

impl std::str::FromStr for SpecType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "skill" => Ok(SpecType::Skill),
            "agent" => Ok(SpecType::Agent),
            other => Err(Error::InvalidSpecType {
                value: other.to_string(),
            }),
        }
    }
}

/// Triggers for automatic skill activation.
///
/// Currently an opaque map — the Bash version uses `"triggers": {}`
/// as a placeholder. BTreeMap is used for deterministic JSON key ordering.
pub type Triggers = BTreeMap<String, serde_json::Value>;

/// A single spec (skill or agent) in the AKM library.
///
/// This is the canonical representation stored in `library.json`.
/// All fields match the JSON schema from the Bash `_skills_libgen_for_dir()`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Spec {
    /// Unique identifier derived from directory name (skills) or filename (agents).
    pub id: String,

    /// Spec type: skill or agent.
    #[serde(rename = "type")]
    pub spec_type: SpecType,

    /// Human-readable name from frontmatter. Falls back to the id.
    pub name: String,

    /// Description text from frontmatter.
    pub description: String,

    /// Whether this is a core spec (globally symlinked).
    #[serde(default)]
    pub core: bool,

    /// Classification tags for filtering/searching.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Triggers for automatic activation (placeholder for future use).
    #[serde(default)]
    pub triggers: Triggers,

    /// Source registry URL (set during sync to track provenance).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl Spec {
    /// Create a new spec with minimal required fields. Other fields are defaults.
    pub fn new(
        id: impl Into<String>,
        spec_type: SpecType,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            spec_type,
            name: name.into(),
            description: description.into(),
            core: false,
            tags: Vec::new(),
            triggers: BTreeMap::new(),
            source: None,
        }
    }

    /// Resolve the filesystem path for this spec in a library directory.
    ///
    /// Bash: `_spec_source_path()` at bin/akm:164
    pub fn source_path(&self, library_dir: &Path) -> PathBuf {
        let subdir = self.spec_type.subdir();
        match self.spec_type {
            SpecType::Skill => library_dir.join(subdir).join(&self.id),
            SpecType::Agent => library_dir.join(subdir).join(format!("{}.md", self.id)),
        }
    }

    /// Check if this spec's source files exist on disk.
    pub fn exists_on_disk(&self, library_dir: &Path) -> bool {
        let path = self.source_path(library_dir);
        match self.spec_type {
            SpecType::Skill => path.is_dir(),
            SpecType::Agent => path.is_file(),
        }
    }

    /// Path to the entry-point markdown file for this spec.
    ///
    /// Skills: `<library_dir>/skills/<id>/SKILL.md`
    /// Agents: `<library_dir>/agents/<id>.md`
    pub fn markdown_path(&self, library_dir: &Path) -> PathBuf {
        match self.spec_type {
            SpecType::Skill => library_dir.join("skills").join(&self.id).join("SKILL.md"),
            SpecType::Agent => library_dir.join("agents").join(format!("{}.md", self.id)),
        }
    }
}
