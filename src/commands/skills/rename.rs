//! `akm skills rename` — change a spec's id (its slug / directory name).
//!
//! The id is what a human types to invoke a skill as `/<slug>`, so renaming it
//! is a filesystem move, not a metadata edit: `skills/<old>` becomes
//! `skills/<new>` (or the agent's `<old>.md` + `<old>.akm.json` pair is moved).
//! The derived index and core symlinks are rebuilt from the new layout, and the
//! machine-local core override is carried across so the rename does not quietly
//! reset it.
//!
//! Like `edit`, this offers to publish when a personal registry is configured —
//! the push carries both the deletion of the old paths and the addition of the
//! new ones, so the remote is renamed too.

use crate::config::Config;
use crate::error::{Error, IoContext, Result};
use crate::library::libgen;
use crate::library::local::LocalOverrides;
use crate::library::manifest::Manifest;
use crate::library::spec::SpecType;
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use std::env;
use std::path::PathBuf;

/// Run the `akm skills rename` command.
pub fn run(
    paths: &Paths,
    config: &Config,
    old: &str,
    new: &str,
    tool_dirs: &ToolDirs,
) -> Result<()> {
    let spec_type = apply(paths, old, new, tool_dirs)?;

    // Fix the current project's manifest too. Manifests in other repositories
    // reference the old id and cannot be reached from here.
    update_current_manifest(old, new, spec_type, tool_dirs)?;

    println!("Renamed {spec_type} '{old}' -> '{new}'");

    let mut pathspecs = spec_type.pathspecs(old);
    pathspecs.extend(spec_type.pathspecs(new));
    let message = format!("refactor: rename {spec_type} '{old}' -> '{new}'");
    super::publish::offer_pathspecs(paths, config, &pathspecs, &message);

    Ok(())
}

/// Perform the library-level rename: validate, move files, rebuild the index,
/// carry the local override across and rebuild core symlinks.
///
/// Reused by the TUI. Does not touch project manifests, session symlinks, or
/// the remote. Returns the renamed spec's type.
pub fn apply(paths: &Paths, old: &str, new: &str, tool_dirs: &ToolDirs) -> Result<SpecType> {
    let library_dir = paths.library_dir();
    let library = Library::load_checked(paths)?;

    let spec = library.get(old).ok_or_else(|| Error::SpecNotFound {
        id: old.to_string(),
    })?;
    let spec_type = spec.spec_type;

    validate_id(new)?;
    if library.contains(new) {
        return Err(Error::SpecAlreadyExists {
            id: new.to_string(),
        });
    }

    move_files(&library_dir, spec_type, old, new)?;

    // Rebuild the derived index from the new on-disk layout, then fold in this
    // machine's core deviations — moving the renamed spec's entry first so the
    // preference survives the id change.
    libgen::generate(&library_dir, &paths.library_json())?;
    let mut library = Library::load_from(&paths.library_json())?;
    let mut overrides = LocalOverrides::load_from(&paths.local_json())?;
    overrides.rename(old, new);
    overrides.apply(&mut library);
    overrides.save_to(&paths.local_json())?;
    library.save_to(&paths.library_json())?;

    symlinks::rebuild_core(&library.core_specs(), &library_dir, tool_dirs.dirs())?;

    Ok(spec_type)
}

/// Move a spec's files from `old` to `new` on disk.
fn move_files(
    library_dir: &std::path::Path,
    spec_type: SpecType,
    old: &str,
    new: &str,
) -> Result<()> {
    match spec_type {
        SpecType::Skill => {
            let from = library_dir.join("skills").join(old);
            let to = library_dir.join("skills").join(new);
            std::fs::rename(&from, &to).io_context(format!(
                "Renaming {} to {}",
                from.display(),
                to.display()
            ))?;
        }
        SpecType::Agent => {
            let agents = library_dir.join("agents");
            let from_md = agents.join(format!("{old}.md"));
            let to_md = agents.join(format!("{new}.md"));
            std::fs::rename(&from_md, &to_md).io_context(format!(
                "Renaming {} to {}",
                from_md.display(),
                to_md.display()
            ))?;

            // The sidecar is optional — only present once someone curated metadata.
            let from_side = agents.join(format!("{old}.akm.json"));
            if from_side.is_file() {
                let to_side = agents.join(format!("{new}.akm.json"));
                std::fs::rename(&from_side, &to_side).io_context(format!(
                    "Renaming {} to {}",
                    from_side.display(),
                    to_side.display()
                ))?;
            }
        }
    }
    Ok(())
}

/// Rewrite the current project's manifest entry, if it references `old`.
fn update_current_manifest(
    old: &str,
    new: &str,
    spec_type: SpecType,
    tool_dirs: &ToolDirs,
) -> Result<()> {
    let Ok(project_root) = crate::git::Git::toplevel(None) else {
        return Ok(());
    };
    if !Manifest::path(&project_root).exists() {
        return Ok(());
    }

    let mut manifest = Manifest::load(&project_root)?;
    if manifest.remove(old, Some(spec_type)) {
        manifest.add(new, spec_type);
        manifest.save()?;

        // Drop the stale session symlink; the new one is created next session.
        if let Some(staging) = env::var("AKM_SESSION").ok().map(PathBuf::from) {
            if staging.is_dir() {
                let _ = symlinks::remove_session(old, &staging, &tool_dirs.staging_names());
            }
        }
    }
    Ok(())
}

/// Reject ids that are not usable slugs / directory names.
fn validate_id(id: &str) -> Result<()> {
    let reason = if id.is_empty() {
        Some("must not be empty")
    } else if id == "." || id == ".." {
        Some("must not be '.' or '..'")
    } else if id.contains('/') || id.contains('\\') {
        Some("must not contain path separators")
    } else if id.chars().any(char::is_whitespace) {
        Some("must not contain whitespace")
    } else {
        None
    };

    match reason {
        Some(reason) => Err(Error::InvalidSpecId {
            id: id.to_string(),
            reason: reason.to_string(),
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_id_rejects_bad_slugs() {
        assert!(validate_id("").is_err());
        assert!(validate_id(".").is_err());
        assert!(validate_id("..").is_err());
        assert!(validate_id("a/b").is_err());
        assert!(validate_id("a\\b").is_err());
        assert!(validate_id("a b").is_err());
    }

    #[test]
    fn validate_id_accepts_ordinary_slugs() {
        assert!(validate_id("git-commit").is_ok());
        assert!(validate_id("tdd_workflow").is_ok());
        assert!(validate_id("code-review-agent").is_ok());
    }
}
