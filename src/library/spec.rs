//! Spec model — represents a skill or agent in the AKM library.
//!
//! Each spec has an ID, type, name, description, and metadata (tags, core flag,
//! triggers). This struct is the serialized form stored in `library.json`.
//!
//! Corresponds to individual JSON objects in `library.json`:
//! ```json
//! {"id":"test-driven-development","type":"skill","name":"Test-Driven Development",
//!  "description":"Use when...","core":false,"tags":["testing"],"triggers":{}}
//! ```

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The type of a spec: skill (directory with SKILL.md) or agent (single .md file).
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
    pub fn subdir(&self) -> &'static str {
        match self {
            SpecType::Skill => "skills",
            SpecType::Agent => "agents",
        }
    }

    /// Path of the `akm.json` metadata sidecar for `id` in a library tree.
    ///
    /// Skills own a directory, so the sidecar goes inside it. Agents are a
    /// single file, so it sits beside them as `<id>.akm.json` — a suffix the
    /// `*.md` scan in libgen ignores.
    pub fn sidecar_path(&self, library_dir: &Path, id: &str) -> PathBuf {
        match self {
            SpecType::Skill => library_dir.join("skills").join(id).join("akm.json"),
            SpecType::Agent => library_dir.join("agents").join(format!("{id}.akm.json")),
        }
    }

    /// Every path belonging to `id`, relative to the library root.
    ///
    /// This is the unit git operates on. A skill is one directory; an agent is
    /// its markdown file plus its sidecar, listed explicitly rather than
    /// globbed so an id can never capture a neighbour's files.
    pub fn pathspecs(&self, id: &str) -> Vec<String> {
        match self {
            SpecType::Skill => vec![format!("skills/{id}")],
            SpecType::Agent => vec![format!("agents/{id}.md"), format!("agents/{id}.akm.json")],
        }
    }

    /// The sidecar's path relative to the library root.
    ///
    /// Distinct from [`Self::pathspecs`], which covers a skill's whole
    /// directory: staging metadata must never sweep in an in-flight `SKILL.md`
    /// edit, so a metadata commit stages this and nothing else.
    pub fn sidecar_pathspec(&self, id: &str) -> String {
        match self {
            SpecType::Skill => format!("skills/{id}/akm.json"),
            SpecType::Agent => format!("agents/{id}.akm.json"),
        }
    }

    /// Map a path inside a library tree back to the spec id that owns it.
    ///
    /// Returns `None` for paths that belong to no spec (`library.json`,
    /// `instructions/global.md`, anything outside `skills/` and `agents/`).
    pub fn owner_of(path: &str) -> Option<(SpecType, String)> {
        if let Some(rest) = path.strip_prefix("skills/") {
            let id = rest.split('/').next()?;
            if id.is_empty() {
                return None;
            }
            return Some((SpecType::Skill, id.to_string()));
        }

        let rest = path.strip_prefix("agents/")?;
        if rest.contains('/') {
            return None;
        }
        let id = rest
            .strip_suffix(".akm.json")
            .or_else(|| rest.strip_suffix(".md"))?;
        if id.is_empty() {
            return None;
        }
        Some((SpecType::Agent, id.to_string()))
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
/// Currently an opaque map — specs use `"triggers": {}`
/// as a placeholder. BTreeMap is used for deterministic JSON key ordering.
pub type Triggers = BTreeMap<String, serde_json::Value>;

/// A single spec (skill or agent) in the AKM library.
///
/// This is the canonical representation stored in `library.json`.
/// All fields match the `library.json` schema produced by libgen.
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

/// Human-facing metadata for one spec, stored beside it as `akm.json`.
///
/// This is the publishable half of a spec's metadata — what a person reads
/// when browsing or picking a skill. It is deliberately *not* the same as the
/// SKILL.md frontmatter: frontmatter is LLM-facing, where `description` is an
/// activation trigger kept token-tight, while this one is prose written for a
/// human. Neither is derived from the other, and sync never reconciles them.
///
/// Keeping it per-spec is what makes publishing atomic: two machines editing
/// two different skills touch two different files, so their metadata can never
/// conflict the way a single shared index would.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpecMeta {
    /// Human-readable name.
    pub name: String,

    /// Human-facing description, shown when browsing the library.
    pub description: String,

    /// Classification tags for filtering and searching.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Whether this spec is core *by default*, on every machine.
    ///
    /// Per-machine deviations live in `local.json` and are never written here,
    /// so a newly published core skill still reaches every machine.
    #[serde(default)]
    pub core: bool,

    /// Triggers for automatic activation (placeholder for future use).
    #[serde(default)]
    pub triggers: Triggers,

    /// Where the spec came from, when it was imported rather than authored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl SpecMeta {
    /// Derive starter metadata from a spec's markdown frontmatter.
    ///
    /// Used the first time AKM sees a spec that has no sidecar yet: the
    /// frontmatter values are the only description available, so they seed the
    /// human-facing ones. From then on the two evolve independently.
    pub fn from_frontmatter(id: &str, fm: &crate::library::frontmatter::Frontmatter) -> Self {
        Self {
            name: fm.name.clone().unwrap_or_else(|| id.to_string()),
            description: fm.description.clone().unwrap_or_default(),
            tags: Vec::new(),
            core: false,
            triggers: BTreeMap::new(),
            source: None,
        }
    }

    /// Read a sidecar from disk.
    pub fn load_from(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|source| Error::Io {
            context: format!("Reading spec metadata from {}", path.display()),
            source,
        })?;

        serde_json::from_str(&content).map_err(|e| Error::SidecarParse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }

    /// Write a sidecar to disk, creating parent directories as needed.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                context: format!("Creating directory for {}", path.display()),
                source,
            })?;
        }

        let content = serde_json::to_string_pretty(self).map_err(|e| Error::Io {
            context: format!("Serializing spec metadata for {}", path.display()),
            source: std::io::Error::other(e),
        })?;

        std::fs::write(path, format!("{content}\n")).map_err(|source| Error::Io {
            context: format!("Writing spec metadata to {}", path.display()),
            source,
        })
    }
}

impl Spec {
    /// Build a library entry from a spec's sidecar metadata.
    pub fn from_meta(id: impl Into<String>, spec_type: SpecType, meta: SpecMeta) -> Self {
        Self {
            id: id.into(),
            spec_type,
            name: meta.name,
            description: meta.description,
            core: meta.core,
            tags: meta.tags,
            triggers: meta.triggers,
            source: meta.source,
        }
    }

    /// Project this entry back to the sidecar form.
    ///
    /// `core` carries the *default*, so callers that hold a machine-local
    /// override must strip it before saving — see `LocalOverrides`.
    pub fn to_meta(&self) -> SpecMeta {
        SpecMeta {
            name: self.name.clone(),
            description: self.description.clone(),
            tags: self.tags.clone(),
            core: self.core,
            triggers: self.triggers.clone(),
            source: self.source.clone(),
        }
    }

    /// Path to this spec's `akm.json` sidecar in a library directory.
    pub fn sidecar_path(&self, library_dir: &Path) -> PathBuf {
        self.spec_type.sidecar_path(library_dir, &self.id)
    }

    /// Every path belonging to this spec, relative to the library root.
    pub fn pathspecs(&self) -> Vec<String> {
        self.spec_type.pathspecs(&self.id)
    }

    /// This spec's sidecar path, relative to the library root.
    pub fn sidecar_pathspec(&self) -> String {
        self.spec_type.sidecar_pathspec(&self.id)
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_pathspec_covers_only_the_metadata_file() {
        assert_eq!(
            SpecType::Skill.sidecar_pathspec("alpha"),
            "skills/alpha/akm.json"
        );
        assert_eq!(
            SpecType::Agent.sidecar_pathspec("bot"),
            "agents/bot.akm.json"
        );
        // The distinction that matters: a skill's full pathspec is its whole
        // directory, so staging metadata must not use it.
        assert_eq!(SpecType::Skill.pathspecs("alpha"), vec!["skills/alpha"]);
    }
}
