//! Shared registries — read-only troves to browse and import from.
//!
//! A shared registry is somebody else's skills repository: a colleague's, a
//! team's, a community's. AKM never mounts anything from one. It clones it,
//! lets you look, and copies what you pick into your personal library, where
//! the copy becomes an ordinary personal spec with no lasting tie to its
//! source beyond the `source` field in its sidecar.
//!
//! That restraint is the whole design. Mounting two registries at once would
//! force a precedence rule into the one namespace whose names are also the
//! strings a model matches on, and would make drift, publish and revert
//! per-registry. Importing instead leaves every one of those exactly as it was.
//!
//! The checkout lives in the cache ([`Paths::shared_dir`]) because it is
//! reconstructible from its remote and owned by nobody here.

use crate::config::Config;
use crate::error::{Error, IoContext, Result};
use crate::library::libgen;
use crate::library::spec::{SpecMeta, SpecType};
use crate::paths::Paths;
use crate::registry::{Registry, SyncOutcome};
use std::path::{Path, PathBuf};

/// What refreshing a shared registry's checkout did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// First contact — the registry was cloned.
    Cloned,
    /// Already level with the remote.
    UpToDate,
    /// Fast-forwarded to the remote.
    Updated,
    /// Could not reach the remote. Whatever is on disk is still usable.
    Failed { message: String },
}

impl std::fmt::Display for RefreshOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefreshOutcome::Cloned => write!(f, "cloned"),
            RefreshOutcome::UpToDate => write!(f, "up to date"),
            RefreshOutcome::Updated => write!(f, "updated"),
            RefreshOutcome::Failed { message } => write!(f, "unreachable ({message})"),
        }
    }
}

/// One importable skill found in a shared registry.
pub struct Candidate {
    /// Directory name under `skills/`, and the id it would be imported under.
    pub id: String,
    /// Absolute path to the skill's directory in the checkout.
    pub dir: PathBuf,
    /// Metadata from the skill's sidecar, or its frontmatter when it has none.
    pub meta: SpecMeta,
}

/// A directory that could not be imported, and why.
pub struct Skipped {
    pub id: String,
    pub reason: String,
}

/// Everything a shared registry offers, plus what it offered that was unusable.
pub struct Catalogue {
    pub skills: Vec<Candidate>,
    pub skipped: Vec<Skipped>,
}

impl Catalogue {
    /// Look up one candidate by id.
    pub fn get(&self, id: &str) -> Option<&Candidate> {
        self.skills.iter().find(|c| c.id == id)
    }
}

/// Open a handle to a configured shared registry. Does not touch the network.
pub fn open(paths: &Paths, config: &Config, name: &str) -> Result<Registry> {
    let url = config.shared_remote(name)?;
    Ok(Registry::named(name, url, paths.shared_dir(name)))
}

/// Clone the registry, or fast-forward the checkout that is already there.
///
/// Never fatal on its own: a registry that cannot be reached leaves the
/// previous checkout in place, so browsing and importing keep working offline.
/// Callers that need content check [`Registry::is_cloned`] afterwards.
pub fn refresh(registry: &Registry) -> RefreshOutcome {
    if !registry.is_cloned() {
        return match registry.clone_fresh() {
            Ok(()) => RefreshOutcome::Cloned,
            Err(e) => RefreshOutcome::Failed {
                message: one_line(&e, registry.name()),
            },
        };
    }

    match registry.update() {
        Ok(SyncOutcome::UpToDate) => RefreshOutcome::UpToDate,
        Ok(_) => RefreshOutcome::Updated,
        Err(e) => RefreshOutcome::Failed {
            message: one_line(&e, registry.name()),
        },
    }
}

/// Reduce a failure to something that fits in a one-line report.
///
/// Git answers a failed fetch with a paragraph, and this string is printed
/// mid-sentence in `akm skills sync`'s registry list. The first line carries
/// which operation failed; the rest is advice about SSH keys that belongs to
/// whoever goes looking. The registry's own name is stripped because the line
/// it lands in already starts with it.
fn one_line(error: &Error, name: &str) -> String {
    let text = format!("{error}");
    let first = text.lines().next().unwrap_or_default();
    first
        .strip_prefix(&format!("Failed to sync registry '{name}': "))
        .unwrap_or(first)
        .trim()
        .to_string()
}

/// Refresh a registry and fail if that left nothing to read.
///
/// The distinction matters: `akm skills sync` reports an unreachable registry
/// and moves on, while `list` and `import` have nothing to do without content.
pub fn refresh_for_use(registry: &Registry) -> Result<RefreshOutcome> {
    let outcome = refresh(registry);
    if !registry.is_cloned() {
        if let RefreshOutcome::Failed { message } = &outcome {
            return Err(Error::RegistrySync {
                name: registry.name().to_string(),
                message: message.clone(),
            });
        }
    }
    Ok(outcome)
}

/// Scan a checkout for importable skills.
///
/// Validity is deliberately strict and reported rather than silent: a shared
/// registry is written by someone else, and a directory that looks like a skill
/// but has no `SKILL.md`, or frontmatter missing the two fields every harness
/// needs, would otherwise be imported as something that never activates.
///
/// Skills only — same as `import` and `promote`, which have never handled
/// agents either.
pub fn catalogue(dir: &Path) -> Result<Catalogue> {
    let skills_dir = dir.join("skills");
    if !skills_dir.is_dir() {
        return Err(Error::NoSpecDirs {
            path: dir.to_path_buf(),
        });
    }

    let mut entries: Vec<_> = std::fs::read_dir(&skills_dir)
        .io_context(format!("Reading skills directory {}", skills_dir.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut skills = Vec::new();
    let mut skipped = Vec::new();

    for entry in entries {
        let id = entry.file_name().to_string_lossy().to_string();
        let spec_dir = entry.path();
        let md_file = spec_dir.join("SKILL.md");

        if !md_file.is_file() {
            skipped.push(Skipped {
                id,
                reason: "no SKILL.md".into(),
            });
            continue;
        }

        match crate::library::frontmatter::Frontmatter::parse_file(&md_file)
            .and_then(|fm| fm.require_name_and_description(&md_file))
        {
            Ok(()) => skills.push(Candidate {
                meta: libgen::resolve_meta(dir, &id, SpecType::Skill, &md_file),
                id,
                dir: spec_dir,
            }),
            Err(e) => skipped.push(Skipped {
                id,
                reason: format!("{e}").replace('\n', " "),
            }),
        }
    }

    Ok(Catalogue { skills, skipped })
}

/// Refresh every configured shared registry, reporting each one.
///
/// One unreachable registry never stops the others: the report carries its
/// failure and the loop continues.
pub fn refresh_all(paths: &Paths, config: &Config) -> Vec<(String, RefreshOutcome, usize)> {
    config
        .shared
        .iter()
        .filter(|(_, url)| !url.is_empty())
        .map(|(name, url)| {
            let registry = Registry::named(name, url, paths.shared_dir(name));
            let outcome = refresh(&registry);
            let count = catalogue(registry.dir())
                .map(|c| c.skills.len())
                .unwrap_or(0);
            (name.clone(), outcome, count)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn catalogue_reports_why_a_directory_was_unusable() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        write(
            &root.join("skills/good/SKILL.md"),
            "---\nname: good\ndescription: a real skill\n---\nbody\n",
        );
        write(
            &root.join("skills/no-description/SKILL.md"),
            "---\nname: no-description\n---\nbody\n",
        );
        write(&root.join("skills/notes/README.md"), "not a skill\n");

        let catalogue = catalogue(root).unwrap();

        let ids: Vec<&str> = catalogue.skills.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["good"]);
        assert_eq!(catalogue.skills[0].meta.description, "a real skill");

        let skipped: Vec<(&str, bool)> = catalogue
            .skipped
            .iter()
            .map(|s| (s.id.as_str(), s.reason.contains("SKILL.md")))
            .collect();
        assert_eq!(skipped, vec![("no-description", true), ("notes", true)]);
    }

    #[test]
    fn catalogue_prefers_the_sidecar_over_the_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        write(
            &root.join("skills/tdd/SKILL.md"),
            "---\nname: tdd\ndescription: llm-facing trigger\n---\nbody\n",
        );
        write(
            &root.join("skills/tdd/akm.json"),
            r#"{"name":"TDD","description":"human-facing prose","tags":["testing"],"core":false}"#,
        );

        let catalogue = catalogue(root).unwrap();
        assert_eq!(catalogue.skills[0].meta.description, "human-facing prose");
        assert_eq!(catalogue.skills[0].meta.tags, vec!["testing"]);
    }

    #[test]
    fn a_checkout_without_skills_is_not_a_registry() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(catalogue(tmp.path()).is_err());
    }
}
