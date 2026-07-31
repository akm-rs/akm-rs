//! `akm skills core` — inspect and reconcile which specs are globally mounted.
//!
//! `core` has two layers. A spec's `akm.json` carries the *published default*,
//! which every machine inherits; `local.json` carries this machine's
//! *deviations* from it. This command shows both, and offers the two ways of
//! collapsing them:
//!
//! * `--adopt` throws this machine's deviations away and follows the registry;
//! * `--publish` promotes them into the sidecars so they become the default
//!   everywhere, then leaves the specs staged for `akm skills publish`.

use crate::error::Result;
use crate::library::local::LocalOverrides;
use crate::library::spec::SpecMeta;
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;

/// Which reconciliation the user asked for, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreAction {
    /// Just show the current state.
    Show,
    /// Drop local deviations and follow the published defaults.
    Adopt,
    /// Promote local deviations into the sidecars.
    Publish,
}

/// Run the `akm skills core` command.
pub fn run(paths: &Paths, action: CoreAction, tool_dirs: &ToolDirs) -> Result<()> {
    let library_dir = paths.library_dir();
    let library = Library::load_checked(paths)?;
    let mut overrides = LocalOverrides::load_from(&paths.local_json())?;

    // `library.json` already holds the effective value; the sidecars hold the
    // published default. Reading both is what makes the deviation visible.
    match action {
        CoreAction::Show => return show(&library, &overrides),
        CoreAction::Adopt => adopt(paths, &mut overrides, tool_dirs)?,
        CoreAction::Publish => publish_defaults(&library_dir, &library, &mut overrides)?,
    }

    overrides.save_to(&paths.local_json())?;
    Ok(())
}

/// Print the core set, marking specs this machine has overridden.
fn show(library: &Library, overrides: &LocalOverrides) -> Result<()> {
    let core = library.core_specs();

    if core.is_empty() {
        println!("No core specs. Nothing is globally mounted.");
    } else {
        println!("Core specs ({}):", core.len());
        for spec in &core {
            let marker = if overrides.core.contains_key(&spec.id) {
                " (local override)"
            } else {
                ""
            };
            println!("  {}{marker}", spec.id);
        }
    }

    let off: Vec<&String> = overrides
        .core
        .iter()
        .filter(|(_, wanted)| !**wanted)
        .map(|(id, _)| id)
        .collect();

    if !off.is_empty() {
        println!();
        println!("Switched off on this machine ({}):", off.len());
        for id in off {
            println!("  {id}");
        }
    }

    if overrides.deviation_count() > 0 {
        println!();
        println!(
            "{} spec(s) deviate from the registry. Use --adopt to follow it, or --publish to make this machine's choices the default.",
            overrides.deviation_count()
        );
    }

    Ok(())
}

/// Drop every deviation and rebuild symlinks from the published defaults.
fn adopt(paths: &Paths, overrides: &mut LocalOverrides, tool_dirs: &ToolDirs) -> Result<()> {
    let dropped = overrides.deviation_count();
    overrides.core.clear();

    // library.json holds effective values, so it has to be rebuilt from the
    // sidecars before the symlinks can follow.
    let library_dir = paths.library_dir();
    crate::library::libgen::generate(&library_dir)?;
    let library = Library::load_from(&paths.library_json())?;

    let count = symlinks::rebuild_core(&library.core_specs(), &library_dir, tool_dirs.dirs())?;

    println!("Dropped {dropped} local core override(s).");
    println!("{count} core symlinks rebuilt from the registry's defaults.");
    Ok(())
}

/// Write this machine's core choices into the sidecars, ready to publish.
fn publish_defaults(
    library_dir: &std::path::Path,
    library: &Library,
    overrides: &mut LocalOverrides,
) -> Result<()> {
    if overrides.deviation_count() == 0 {
        println!("No local core overrides to publish.");
        return Ok(());
    }

    let ids: Vec<String> = overrides.core.keys().cloned().collect();
    let mut promoted = Vec::new();

    for id in ids {
        let Some(spec) = library.get(&id) else {
            overrides.clear_core(&id);
            continue;
        };

        let sidecar = spec.sidecar_path(library_dir);
        let mut meta = if sidecar.is_file() {
            SpecMeta::load_from(&sidecar)?
        } else {
            spec.to_meta()
        };

        // `library.json` already carries the effective value for this machine,
        // which is precisely what we are promoting to the default.
        meta.core = spec.core;
        meta.save_to(&sidecar)?;

        overrides.clear_core(&id);
        promoted.push(id);
    }

    println!(
        "Promoted {} core setting(s) to the registry default:",
        promoted.len()
    );
    for id in &promoted {
        println!("  {id}");
    }
    println!();
    println!("Publish them with:");
    for id in &promoted {
        println!("  akm skills publish {id}");
    }

    Ok(())
}
