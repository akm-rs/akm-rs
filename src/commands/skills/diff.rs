//! `akm skills diff` — show what changed on each side for one spec.
//!
//! Two diffs, because there are two questions. "What have I changed since the
//! last sync?" is the working tree against `HEAD`. "What did the registry
//! change that I do not have?" is `HEAD` against the fetched remote. A spec
//! that has diverged has both, and seeing them side by side is what makes the
//! publish-or-revert decision answerable.

use crate::config::Config;
use crate::error::{Error, Result};
use crate::library::drift::DriftState;
use crate::library::Library;
use crate::paths::Paths;
use crate::registry::Registry;

/// Run the `akm skills diff` command.
pub fn run(paths: &Paths, config: &Config, id: &str) -> Result<()> {
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

    // Compare against the registry as it is now, not as it was at the last sync.
    if registry.is_configured() {
        registry.refresh()?;
    }

    let pathspecs = spec.pathspecs();
    let state = registry.drift()?.state_of(id);

    println!("{id} — {state}");

    if state == DriftState::Clean {
        return Ok(());
    }

    if state.has_local_changes() {
        println!();
        println!("Local changes (yours, not yet published):");
        print_or_note(&registry.diff_local(&pathspecs)?);
    }

    if matches!(state, DriftState::RemoteNewer | DriftState::Diverged) {
        println!();
        println!("Registry changes (not yet applied here):");
        print_or_note(&registry.diff_remote(&pathspecs)?);
    }

    println!();
    match state {
        DriftState::LocalNewer => {
            println!(
                "Publish with 'akm skills publish {id}', or discard with 'akm skills revert {id}'."
            );
        }
        DriftState::RemoteNewer => {
            println!("Apply with 'akm skills sync'.");
        }
        DriftState::Diverged => {
            println!("Keep yours with 'akm skills publish {id}', or take the registry's with 'akm skills revert {id} --remote'.");
        }
        DriftState::Clean => {}
    }

    Ok(())
}

fn print_or_note(diff: &str) {
    if diff.trim().is_empty() {
        println!("  (no textual diff — the change is a file being added or removed)");
    } else {
        println!("{diff}");
    }
}
