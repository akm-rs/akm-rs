//! `akm skills delete` — remove a spec from the library (and the registry).
//!
//! This is the destructive counterpart to `remove`, which only unwires a spec
//! from a project manifest. Delete takes the spec's files off disk, rebuilds
//! the derived index and core symlinks without it, drops its machine-local core
//! override and its entry in the current project's manifest.
//!
//! Because it is irreversible it confirms first: `[y/N]` on a terminal, or an
//! explicit `--force` when there is no TTY to ask on. When a personal registry
//! is configured it then offers to publish the removal, so the spec is gone
//! from the remote too — subject to the caveat that manifests in *other*
//! repositories still reference the deleted id and cannot be reached from here.

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
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

/// Run the `akm skills delete` command.
pub fn run(
    paths: &Paths,
    config: &Config,
    id: &str,
    force: bool,
    tool_dirs: &ToolDirs,
) -> Result<()> {
    // Resolve first, so a typo gets "no such spec" rather than a prompt.
    let library = Library::load_checked(paths)?;
    let spec = library
        .get(id)
        .ok_or_else(|| Error::SpecNotFound { id: id.to_string() })?;
    let spec_type = spec.spec_type;
    let pathspecs = spec.pathspecs();

    if !force && !confirm(id, spec_type)? {
        println!("Aborted.");
        return Ok(());
    }

    apply(paths, id, tool_dirs)?;

    // Drop it from the current project's manifest too. Manifests in other
    // repositories reference the deleted id and cannot be reached from here.
    remove_from_current_manifest(id, spec_type, tool_dirs)?;

    println!("Deleted {spec_type} '{id}' from the library");

    let message = format!("chore: remove {spec_type} '{id}'");
    super::publish::offer_pathspecs(paths, config, &pathspecs, &message);

    Ok(())
}

/// Perform the library-level delete: remove files, rebuild the index without
/// the spec, drop its local override and rebuild core symlinks.
///
/// Reused by the TUI. Does not touch project manifests, session symlinks, or
/// the remote.
pub fn apply(paths: &Paths, id: &str, tool_dirs: &ToolDirs) -> Result<()> {
    let library_dir = paths.library_dir();
    let library = Library::load_checked(paths)?;
    let spec = library
        .get(id)
        .ok_or_else(|| Error::SpecNotFound { id: id.to_string() })?;
    let spec_type = spec.spec_type;

    remove_files(&library_dir, spec_type, id)?;

    libgen::generate(&library_dir, &paths.library_json())?;
    let mut library = Library::load_from(&paths.library_json())?;
    let mut overrides = LocalOverrides::load_from(&paths.local_json())?;
    overrides.clear_core(id);
    overrides.apply(&mut library);
    overrides.save_to(&paths.local_json())?;
    library.save_to(&paths.library_json())?;

    symlinks::rebuild_core(&library.core_specs(), &library_dir, tool_dirs.dirs())?;

    Ok(())
}

/// Delete a spec's files from disk.
fn remove_files(library_dir: &Path, spec_type: SpecType, id: &str) -> Result<()> {
    match spec_type {
        SpecType::Skill => {
            let dir = library_dir.join("skills").join(id);
            std::fs::remove_dir_all(&dir).io_context(format!("Removing {}", dir.display()))?;
        }
        SpecType::Agent => {
            let agents = library_dir.join("agents");
            let md = agents.join(format!("{id}.md"));
            std::fs::remove_file(&md).io_context(format!("Removing {}", md.display()))?;

            let sidecar = agents.join(format!("{id}.akm.json"));
            if sidecar.is_file() {
                std::fs::remove_file(&sidecar)
                    .io_context(format!("Removing {}", sidecar.display()))?;
            }
        }
    }
    Ok(())
}

/// Drop the deleted id from the current project's manifest, if present.
fn remove_from_current_manifest(id: &str, spec_type: SpecType, tool_dirs: &ToolDirs) -> Result<()> {
    let Ok(project_root) = crate::git::Git::toplevel(None) else {
        return Ok(());
    };
    if !Manifest::path(&project_root).exists() {
        return Ok(());
    }

    let mut manifest = Manifest::load(&project_root)?;
    if manifest.remove(id, Some(spec_type)) {
        manifest.save()?;

        if let Some(staging) = env::var("AKM_SESSION").ok().map(PathBuf::from) {
            if staging.is_dir() {
                let _ = symlinks::remove_session(id, &staging, &tool_dirs.staging_names());
            }
        }
    }
    Ok(())
}

/// Ask before deleting. Non-interactive callers must pass `--force`.
fn confirm(id: &str, spec_type: SpecType) -> Result<bool> {
    if !io::stdin().is_terminal() {
        return Err(Error::ConfirmationRequired {
            action: format!("delete {spec_type} '{id}' from the library"),
        });
    }

    print!("Delete {spec_type} '{id}' from the library? [y/N]: ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input).ok();
    Ok(input.trim().eq_ignore_ascii_case("y"))
}
