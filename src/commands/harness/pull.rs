//! `akm harness pull` — apply registry harness config to the live dir.

use crate::commands::harness::{def_for, live_dir, targets, tree_dir};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::library::harness_config::apply_files;
use crate::paths::Paths;
use crate::registry::Registry;

/// Run `akm harness pull [<command>] [--force]`.
pub fn run(paths: &Paths, config: &Config, command: Option<&str>, force: bool) -> Result<()> {
    let url = config.registry_url().ok_or(Error::NoPersonalRegistry)?;
    let registry = Registry::new(url, paths.library_dir());
    if !registry.is_cloned() {
        return Err(Error::RegistrySync {
            name: "personal".into(),
            message: "The library is not a registry checkout. Run 'akm skills sync' first.".into(),
        });
    }

    registry.refresh()?;
    let drift = registry.drift()?;

    for cmd in targets(command)? {
        def_for(&cmd)?;
        if !force && drift.harness(&cmd).has_local_changes() {
            eprintln!(
                "{cmd}: local config differs from the registry ({}). \
                 Push first, or re-run with --force to overwrite.",
                drift.harness(&cmd)
            );
            continue;
        }
        // Fast-forward the whole library (parks unrelated edits, ours-wins).
        registry.update()?;
        let Some(live) = live_dir(paths, &cmd) else {
            continue;
        };
        std::fs::create_dir_all(&live).ok();
        let applied = apply_files(&tree_dir(paths, &cmd), &live)?;
        if applied.is_empty() {
            println!("{cmd}: nothing in the registry to apply");
        } else {
            println!(
                "{cmd}: applied {} file(s) to {}",
                applied.len(),
                live.display()
            );
        }
    }
    Ok(())
}
