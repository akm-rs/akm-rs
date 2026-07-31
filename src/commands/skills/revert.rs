//! `akm skills revert` — throw away local changes to one spec.
//!
//! Two targets, matching the two things "undo" can mean here:
//!
//! * by default, back to the last synced state (`HEAD`) — undo my edits;
//! * with `--remote`, all the way to what the registry holds now — take
//!   theirs, discarding both my edits and my baseline.
//!
//! Either way this only touches the named spec's own paths, so reverting one
//! skill can never disturb work in progress on another.

use crate::config::Config;
use crate::error::{Error, Result};
use crate::library::libgen;
use crate::library::local::LocalOverrides;
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use crate::registry::Registry;
use std::io::{self, BufRead, IsTerminal, Write};

/// Run the `akm skills revert` command.
pub fn run(
    paths: &Paths,
    config: &Config,
    id: &str,
    remote: bool,
    force: bool,
    tool_dirs: &ToolDirs,
) -> Result<()> {
    let library = Library::load_checked(paths)?;
    let spec = library
        .get(id)
        .ok_or_else(|| Error::SpecNotFound { id: id.to_string() })?;

    let registry = Registry::new(
        config.registry_url().unwrap_or_default(),
        paths.library_dir(),
    );
    if !registry.is_cloned() {
        return Err(Error::RegistrySync {
            name: "personal".into(),
            message: "The library is not a registry checkout. Run 'akm skills sync' first.".into(),
        });
    }

    let pathspecs = spec.pathspecs();
    let target = if remote {
        "the version in the registry"
    } else {
        "the last synced version"
    };

    if !force && !confirm(id, target)? {
        println!("Aborted.");
        return Ok(());
    }

    if remote {
        // Reverting to "theirs" is only meaningful against the current remote.
        registry.refresh()?;
        registry.take_remote(&pathspecs)?;
    } else {
        registry.revert_to_head(&pathspecs)?;
    }

    println!("Reverted '{id}' to {target}.");

    // The spec's metadata may have moved with it, so rebuild the index and the
    // symlinks that follow from `core`.
    let library_dir = paths.library_dir();
    libgen::generate(&library_dir, &paths.library_json())?;

    let mut library = Library::load_from(&paths.library_json())?;
    let mut overrides = LocalOverrides::load_from(&paths.local_json())?;
    if overrides.apply(&mut library) {
        overrides.save_to(&paths.local_json())?;
    }
    library.save_to(&paths.library_json())?;

    let count = symlinks::rebuild_core(&library.core_specs(), &library_dir, tool_dirs.dirs())?;
    println!("{count} core symlinks rebuilt");

    Ok(())
}

/// Ask before discarding work. Non-interactive callers must pass `--force`.
fn confirm(id: &str, target: &str) -> Result<bool> {
    if !io::stdin().is_terminal() {
        return Err(Error::ConfirmationRequired {
            action: format!("revert '{id}' to {target}"),
        });
    }

    print!("Discard local changes to '{id}' and take {target}? [y/N]: ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input).ok();
    Ok(input.trim().eq_ignore_ascii_case("y"))
}
