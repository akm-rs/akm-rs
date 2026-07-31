//! `akm skills core` — inspect and reconcile which specs are globally mounted.
//!
//! `core` has two layers. A spec's `akm.json` carries the *published default*,
//! which every machine inherits; `local.json` carries this machine's
//! *deviations* from it. This command shows both, and offers the two ways of
//! collapsing them:
//!
//! * `--adopt` throws this machine's deviations away and follows the registry;
//! * `--publish` promotes them into the sidecars so they become the default
//!   everywhere, and sends them to the registry.

use crate::config::Config;
use crate::error::{Error, Result};
use crate::library::local::LocalOverrides;
use crate::library::spec::SpecMeta;
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use crate::registry::{PublishOutcome, Registry};

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
pub fn run(
    paths: &Paths,
    config: &Config,
    action: CoreAction,
    tool_dirs: &ToolDirs,
    dry_run: bool,
) -> Result<()> {
    let library = Library::load_checked(paths)?;
    let mut overrides = LocalOverrides::load_from(&paths.local_json())?;

    // `library.json` already holds the effective value; the sidecars hold the
    // published default. Reading both is what makes the deviation visible.
    match action {
        CoreAction::Show => return show(&library, &overrides),
        CoreAction::Adopt => adopt(paths, &mut overrides, tool_dirs)?,
        CoreAction::Publish => publish_defaults(paths, config, &library, &mut overrides, dry_run)?,
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
    crate::library::libgen::generate(&library_dir, &paths.library_json())?;
    let library = Library::load_from(&paths.library_json())?;

    let count = symlinks::rebuild_core(&library.core_specs(), &library_dir, tool_dirs.dirs())?;

    println!("Dropped {dropped} local core override(s).");
    println!("{count} core symlinks rebuilt from the registry's defaults.");
    Ok(())
}

/// Promote this machine's core choices and send them to the registry.
///
/// One intent is one commit: every sidecar goes in a single commit and a
/// single push. Only sidecar paths are staged, so a `SKILL.md` under edit is
/// never swept into a metadata change.
fn publish_defaults(
    paths: &Paths,
    config: &Config,
    library: &Library,
    overrides: &mut LocalOverrides,
    dry_run: bool,
) -> Result<()> {
    if overrides.deviation_count() == 0 {
        println!("No local core overrides to publish.");
        return Ok(());
    }

    let url = config.registry_url().ok_or(Error::NoPersonalRegistry)?;
    let library_dir = paths.library_dir();
    let registry = Registry::new(url, &library_dir);

    if !registry.is_cloned() {
        return Err(Error::RegistrySync {
            name: "personal".into(),
            message: "The library is not a registry checkout. Run 'akm skills sync' first.".into(),
        });
    }

    // Resolve everything before writing anything, so a dry run and a real run
    // report the same set.
    let ids: Vec<String> = overrides.core.keys().cloned().collect();
    let mut promoted: Vec<String> = Vec::new();
    let mut pathspecs: Vec<String> = Vec::new();
    let mut stale: Vec<String> = Vec::new();

    for id in &ids {
        match library.get(id) {
            Some(spec) => {
                promoted.push(id.clone());
                pathspecs.push(spec.sidecar_pathspec());
            }
            // A deviation for a spec that no longer exists is just noise.
            None => stale.push(id.clone()),
        }
    }

    if promoted.is_empty() {
        println!("No local core overrides to publish.");
        for id in &stale {
            overrides.clear_core(id);
        }
        return Ok(());
    }

    let message = commit_message(&promoted);

    if dry_run {
        println!(
            "Dry run — would publish {} core setting(s):",
            promoted.len()
        );
        for id in &promoted {
            println!("  {id}");
        }
        println!();
        println!(
            "As one commit: {}",
            message.lines().next().unwrap_or(&message)
        );
        println!("To: {}", registry.url());
        return Ok(());
    }

    for id in &promoted {
        let Some(spec) = library.get(id) else {
            continue;
        };
        let sidecar = spec.sidecar_path(&library_dir);
        let mut meta = if sidecar.is_file() {
            SpecMeta::load_from(&sidecar)?
        } else {
            spec.to_meta()
        };
        // `library.json` already carries the effective value for this machine,
        // which is precisely what is being promoted to the default.
        meta.core = spec.core;
        meta.save_to(&sidecar)?;
    }

    match registry.publish(&pathspecs, &message)? {
        PublishOutcome::NothingToDo => {
            println!("Core defaults already match the registry.");
        }
        PublishOutcome::Published => {
            println!("Published {} core setting(s):", promoted.len());
            for id in &promoted {
                println!("  {id}");
            }
            println!();
            println!("Pushed to {}", registry.url());
        }
    }

    // Only a landed push makes it safe to forget the deviations: clearing them
    // earlier would leave a failed publish with nothing to retry from.
    for id in promoted.iter().chain(stale.iter()) {
        overrides.clear_core(id);
    }

    Ok(())
}

/// One line naming the change, one body line naming the specs.
///
/// *update*, not *set*: a deviation can turn `core` off as well as on.
fn commit_message(ids: &[String]) -> String {
    format!(
        "chore(core): update core defaults for {} spec(s)\n\n{}",
        ids.len(),
        ids.join(", ")
    )
}
