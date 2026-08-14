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
use crate::library::spec::SpecMeta;
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

/// Locate the directory that holds a checkout's skill directories, and report
/// whether the layout is the canonical `skills/` one.
///
/// Two layouts are accepted. A canonical registry keeps its skills under a
/// `skills/` directory. A dead-simple shared repo — the kind a team throws
/// together, one directory per skill and nothing else — keeps them at the root.
/// A present `skills/` always wins: it is an explicit declaration of layout, so
/// a repo that has one is never scanned flat.
///
/// Returns `(container, canonical)`. `canonical` is true only for the `skills/`
/// layout, where every child directory is *expected* to be a skill and a
/// missing `SKILL.md` is worth reporting. In the flat layout a child without a
/// `SKILL.md` is just some other directory (`docs/`, `.github/`), not a broken
/// skill.
fn skills_container(dir: &Path) -> Option<(PathBuf, bool)> {
    let canonical = dir.join("skills");
    if canonical.is_dir() {
        return Some((canonical, true));
    }
    if has_skill_child(dir) {
        return Some((dir.to_path_buf(), false));
    }
    None
}

/// True if `dir` directly contains a subdirectory holding a `SKILL.md`.
fn has_skill_child(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.is_dir() && path.join("SKILL.md").is_file()
    })
}

/// Scan a checkout for importable skills.
///
/// Validity is deliberately strict and reported rather than silent: a shared
/// registry is written by someone else, and a directory that looks like a skill
/// but has frontmatter missing the two fields every harness needs would
/// otherwise be imported as something that never activates.
///
/// The one place reporting bends is a directory with no `SKILL.md` at all. Under
/// `skills/` that is a broken skill and is reported. In a flat repo — where the
/// scan runs over the repo root — it is just plumbing (`.git/`, `docs/`) and is
/// ignored, along with anything hidden.
///
/// Skills only — same as `import` and `promote`, which have never handled
/// agents either.
pub fn catalogue(dir: &Path) -> Result<Catalogue> {
    let (container, canonical) = skills_container(dir).ok_or_else(|| Error::NoSharedSkills {
        path: dir.to_path_buf(),
    })?;

    let mut entries: Vec<_> = std::fs::read_dir(&container)
        .io_context(format!("Reading skills directory {}", container.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut skills = Vec::new();
    let mut skipped = Vec::new();

    for entry in entries {
        let name = entry.file_name();
        let id = name.to_string_lossy().to_string();

        // Flat layout scans the repo root, so skip its plumbing.
        if !canonical && id.starts_with('.') {
            continue;
        }

        let spec_dir = entry.path();
        let md_file = spec_dir.join("SKILL.md");

        if !md_file.is_file() {
            if canonical {
                skipped.push(Skipped {
                    id,
                    reason: "no SKILL.md".into(),
                });
            }
            continue;
        }

        match crate::library::frontmatter::Frontmatter::parse_file(&md_file)
            .and_then(|fm| fm.require_name_and_description(&md_file))
        {
            Ok(()) => skills.push(Candidate {
                meta: libgen::resolve_skill_meta(&spec_dir, &id),
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

/// Delete cached checkouts of shared registries that config no longer names.
///
/// The scriptable removal path (`akm config shared.<name> ""`) drops the config
/// entry but cannot touch the cache — [`crate::config::ConfigKey::set`] has only
/// a `&mut Config`, no `Paths`. Sync reconciles the two: any subdirectory under
/// [`Paths::shared_root`] whose name is not a live, non-empty entry in
/// `config.shared` is an orphaned checkout, safe to delete because the whole
/// tree is reconstructible from its remote.
///
/// Best-effort and idempotent: a failed delete or an absent root is a no-op, and
/// a second sweep finds nothing. Returns the names it removed, sorted, for the
/// sync report.
pub fn sweep_orphans(paths: &Paths, config: &Config) -> Vec<String> {
    let live: std::collections::HashSet<&str> = config
        .shared
        .iter()
        .filter(|(_, url)| !url.is_empty())
        .map(|(name, _)| name.as_str())
        .collect();

    // A missing root is the normal empty case — nothing was ever cloned.
    let entries = match std::fs::read_dir(paths.shared_root()) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut swept = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !live.contains(name.as_str()) {
            std::fs::remove_dir_all(entry.path()).ok();
            swept.push(name);
        }
    }
    swept.sort();
    swept
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

    #[test]
    fn catalogue_scans_a_flat_repo_of_skill_directories() {
        // The dead-simple layout: one directory per skill at the repo root, no
        // enclosing skills/ folder.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        write(
            &root.join("tdd/SKILL.md"),
            "---\nname: tdd\ndescription: test-driven development\n---\nbody\n",
        );
        write(
            &root.join("caveman/SKILL.md"),
            "---\nname: caveman\ndescription: terse mode\n---\nbody\n",
        );
        // Plumbing and non-skill directories are ignored, not reported.
        write(&root.join("README.md"), "a shared library\n");
        write(&root.join("docs/guide.md"), "notes\n");
        write(&root.join(".git/config"), "[core]\n");

        let catalogue = catalogue(root).unwrap();

        let ids: Vec<&str> = catalogue.skills.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["caveman", "tdd"]);
        assert!(
            catalogue.skipped.is_empty(),
            "flat layout must not report non-skill dirs: {:?}",
            catalogue.skipped.iter().map(|s| &s.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn catalogue_reports_a_broken_skill_even_in_a_flat_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        write(
            &root.join("good/SKILL.md"),
            "---\nname: good\ndescription: fine\n---\nbody\n",
        );
        // Has a SKILL.md, so it is a skill attempt — a bad one worth reporting.
        write(
            &root.join("broken/SKILL.md"),
            "---\nname: broken\n---\nbody\n",
        );

        let catalogue = catalogue(root).unwrap();

        assert_eq!(
            catalogue.skills.iter().map(|c| &c.id).collect::<Vec<_>>(),
            vec!["good"]
        );
        assert_eq!(
            catalogue.skipped.iter().map(|s| &s.id).collect::<Vec<_>>(),
            vec!["broken"]
        );
    }

    #[test]
    fn a_present_skills_dir_wins_over_flat_dirs() {
        // A repo with a skills/ folder is canonical even if the root also holds
        // stray skill-shaped directories: skills/ declares the layout.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        write(
            &root.join("skills/inside/SKILL.md"),
            "---\nname: inside\ndescription: under skills/\n---\nbody\n",
        );
        write(
            &root.join("outside/SKILL.md"),
            "---\nname: outside\ndescription: at the root\n---\nbody\n",
        );

        let catalogue = catalogue(root).unwrap();

        assert_eq!(
            catalogue.skills.iter().map(|c| &c.id).collect::<Vec<_>>(),
            vec!["inside"]
        );
    }

    fn paths_under(root: &Path) -> Paths {
        Paths::from_roots(
            &root.join("data"),
            &root.join("config"),
            &root.join("cache"),
            &root.join("home"),
        )
    }

    #[test]
    fn sweep_orphans_deletes_only_unconfigured_checkouts() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_under(tmp.path());

        // Three checkouts sit in the cache...
        for name in ["keep", "gone", "also-gone"] {
            std::fs::create_dir_all(paths.shared_dir(name)).unwrap();
        }

        // ...but only one is a live entry. `also-gone` is configured with an
        // empty URL, which counts as removed — same as `refresh_all`.
        let mut config = Config::default();
        config
            .shared
            .insert("keep".into(), "git@example.com:keep.git".into());
        config.shared.insert("also-gone".into(), String::new());

        let swept = sweep_orphans(&paths, &config);

        assert_eq!(swept, vec!["also-gone".to_string(), "gone".to_string()]);
        assert!(paths.shared_dir("keep").is_dir());
        assert!(!paths.shared_dir("gone").exists());
        assert!(!paths.shared_dir("also-gone").exists());

        // Idempotent — a second sweep finds nothing.
        assert!(sweep_orphans(&paths, &config).is_empty());
    }

    #[test]
    fn sweep_orphans_is_a_noop_when_nothing_was_ever_cloned() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_under(tmp.path());
        assert!(sweep_orphans(&paths, &Config::default()).is_empty());
    }
}
