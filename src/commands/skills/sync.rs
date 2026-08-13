//! `akm skills sync` — fast-forward the library, regenerate it, rebuild links.
//!
//! Pipeline:
//! 0. Clear out an rc3 data layout, if this machine still has one
//! 1. Clone the personal registry into the library directory, or fast-forward
//!    an existing checkout (parking local edits around it when needed)
//! 2. Run libgen to regenerate `library.json` from the specs and their sidecars
//! 3. Fold in this machine's `local.json` core deviations
//! 4. Rebuild global symlinks for the resulting core specs
//! 5. Report drift — sync tells you what needs a decision, it never asks
//!
//! Step 5 is deliberately passive. Sync runs non-interactively at session
//! start, so it prints what it found and exits; only `akm skills edit` prompts.

use crate::config::Config;
use crate::error::Result;
use crate::library::drift::{DriftReport, DriftState};
use crate::library::libgen;
use crate::library::local::LocalOverrides;
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use crate::registry::{Registry, SyncOutcome};

use super::migrate::{self, Rc3Wipe};
use super::shared;

/// Result of a sync operation, used for display.
#[derive(Debug)]
pub struct SyncReport {
    /// What the rc3 → rc4 migration removed, if it ran.
    pub migration: Rc3Wipe,
    /// What happened to the library working tree.
    pub registry: RegistryOutcome,
    /// Number of specs found on disk, if libgen ran.
    pub spec_count: Option<usize>,
    /// Number of core symlinks created.
    pub symlink_count: usize,
    /// Number of global tool directories.
    pub tool_dir_count: usize,
    /// Per-spec divergence after the update.
    pub drift: DriftReport,
    /// One entry per configured shared registry: name, outcome, skill count.
    pub shared: Vec<(String, shared::RefreshOutcome, usize)>,
    /// Names of cached checkouts swept because config no longer names them.
    pub swept: Vec<String>,
}

/// Outcome of a single registry update attempt.
#[derive(Debug)]
pub enum RegistryOutcome {
    /// Cloned for the first time.
    Cloned,
    /// Fast-forwarded; `parked` names specs whose local edits were set aside
    /// and put back, and which are therefore now diverged.
    Updated { parked: Vec<String> },
    /// Already level with the remote.
    UpToDate,
    /// The library has local commits the remote does not. Left untouched.
    LocalCommitsDiverged,
    /// The fast-forward could not be applied. Left untouched.
    Blocked { paths: Vec<String> },
    /// No upstream branch to fast-forward from.
    NoUpstream,
    /// Update failed, but the existing checkout is usable.
    Failed { message: String },
    /// No registry configured — working with whatever is on disk.
    Skipped,
    /// No registry configured and no library on disk. Nothing to do.
    SkippedNoLibrary,
}

/// Execute the full sync pipeline.
///
/// `shared` is the outcome of refreshing the configured shared registries. It
/// is passed in rather than computed here because nothing downstream of the
/// personal registry depends on it: shared registries are browsable troves,
/// never mounted, so refreshing one can neither add a symlink nor change the
/// index.
pub fn execute(
    paths: &Paths,
    registry: &Registry,
    tool_dirs: &ToolDirs,
    shared: Vec<(String, shared::RefreshOutcome, usize)>,
) -> Result<SyncReport> {
    let library_dir = registry.dir().to_path_buf();

    // The wipe is only safe while a registry is configured to clone back from.
    let migration = if registry.is_configured() && migrate::needs_migration(paths) {
        migrate::run(paths)?
    } else {
        Rc3Wipe::default()
    };

    let registry_outcome = update_registry(registry)?;

    if matches!(registry_outcome, RegistryOutcome::SkippedNoLibrary) {
        return Ok(SyncReport {
            migration,
            registry: registry_outcome,
            spec_count: None,
            symlink_count: 0,
            tool_dir_count: tool_dirs.count(),
            drift: DriftReport::default(),
            shared,
            swept: Vec::new(),
        });
    }

    if registry.is_cloned() {
        registry.evict_derived_index()?;
    }

    // --- Regenerate the derived index from the specs on disk ---
    let has_specs = library_dir.join("skills").is_dir() || library_dir.join("agents").is_dir();
    let spec_count = if has_specs {
        Some(libgen::generate(&library_dir, &paths.library_json())?.count)
    } else {
        None
    };

    // --- Fold in this machine's core deviations ---
    let library_json = paths.library_json();
    let mut library = Library::load_or_default(&library_json)?;
    let mut overrides = LocalOverrides::load_from(&paths.local_json())?;
    if overrides.apply(&mut library) {
        overrides.save_to(&paths.local_json())?;
    }
    if library_json.is_file() {
        library.save_to(&library_json)?;
    }

    // --- Rebuild global symlinks ---
    let symlink_count = if library_json.is_file() {
        symlinks::rebuild_core(&library.core_specs(), &library_dir, tool_dirs.dirs())?
    } else {
        0
    };

    Ok(SyncReport {
        migration,
        registry: registry_outcome,
        spec_count,
        symlink_count,
        tool_dir_count: tool_dirs.count(),
        drift: registry.drift()?,
        shared,
        swept: Vec::new(),
    })
}

/// Clone or fast-forward the library working tree.
///
/// A failed update is never fatal when there is already a checkout on disk:
/// an offline session must still get its skills.
fn update_registry(registry: &Registry) -> Result<RegistryOutcome> {
    if !registry.is_configured() {
        return Ok(if registry.dir().is_dir() {
            RegistryOutcome::Skipped
        } else {
            RegistryOutcome::SkippedNoLibrary
        });
    }

    if !registry.is_cloned() {
        registry.clone_fresh()?;
        return Ok(RegistryOutcome::Cloned);
    }

    match registry.update() {
        Ok(SyncOutcome::Cloned) => Ok(RegistryOutcome::Cloned),
        Ok(SyncOutcome::UpToDate) => Ok(RegistryOutcome::UpToDate),
        Ok(SyncOutcome::FastForwarded { parked }) => Ok(RegistryOutcome::Updated { parked }),
        Ok(SyncOutcome::LocalCommitsDiverged) => Ok(RegistryOutcome::LocalCommitsDiverged),
        Ok(SyncOutcome::Blocked { paths }) => Ok(RegistryOutcome::Blocked { paths }),
        Ok(SyncOutcome::NoUpstream) => Ok(RegistryOutcome::NoUpstream),
        Err(e) => Ok(RegistryOutcome::Failed {
            message: format!("{e}"),
        }),
    }
}

/// Print sync results to stdout.
pub fn print_report(report: &SyncReport, quiet: bool) {
    // A one-time breaking migration is reported even under --quiet: the
    // session-start sync is exactly where it happens, and it deleted files.
    migrate::print_wipe(&report.migration);

    if quiet {
        return;
    }

    match &report.registry {
        RegistryOutcome::Cloned => println!("Personal registry cloned"),
        RegistryOutcome::UpToDate => println!("Library up to date"),
        RegistryOutcome::Updated { parked } => {
            println!("Library updated from personal registry");
            if !parked.is_empty() {
                println!(
                    "  Local edits kept on top of the update: {}",
                    parked.join(", ")
                );
            }
        }
        RegistryOutcome::LocalCommitsDiverged => {
            eprintln!("Warning: the library has local commits the registry does not have.");
            eprintln!("Nothing was changed. Push or reset them in the library repository.");
        }
        RegistryOutcome::Blocked { paths } => {
            eprintln!("Warning: could not update the library — these files are in the way:");
            for path in paths {
                eprintln!("  {path}");
            }
            eprintln!("Nothing was changed. Publish or revert them, then sync again.");
        }
        RegistryOutcome::NoUpstream => {
            eprintln!("Warning: the library has no upstream branch. Skipping update.");
        }
        RegistryOutcome::Failed { message } => {
            eprintln!("Warning: Failed to update the personal registry. {message}");
            eprintln!("Continuing with the library already on disk.");
        }
        RegistryOutcome::Skipped => {
            println!("No personal registry configured. Working with the existing library.");
        }
        RegistryOutcome::SkippedNoLibrary => {
            println!("No personal registry configured and no existing library. Skipping.");
            return;
        }
    }

    if let Some(count) = report.spec_count {
        println!("Library regenerated ({count} specs)");
    }

    println!(
        "{} core symlinks created across {} global tool directories",
        report.symlink_count, report.tool_dir_count
    );

    print_shared(&report.shared, &report.swept);
    print_drift(&report.drift);
}

/// Report each shared registry's refresh.
///
/// Informational only — nothing here is mounted, so an unreachable registry is
/// a warning about browsing, not about this session's skills.
fn print_shared(shared: &[(String, shared::RefreshOutcome, usize)], swept: &[String]) {
    if shared.is_empty() && swept.is_empty() {
        return;
    }

    println!();
    println!("Shared registries:");
    for (name, outcome, count) in shared {
        match outcome {
            shared::RefreshOutcome::Failed { .. } => {
                println!("  {name}: {outcome}, browsing the copy on disk")
            }
            _ => println!("  {name}: {outcome} ({count} skills)"),
        }
    }
    // A cache vanishing silently is a debugging trap, so name what was swept.
    for name in swept {
        println!("  {name}: removed (no longer configured)");
    }
}

/// Report what needs a decision, without asking for one.
fn print_drift(drift: &DriftReport) {
    let diverged = drift.in_state(DriftState::Diverged);
    let local = drift.in_state(DriftState::LocalNewer);

    if !diverged.is_empty() {
        println!();
        println!("Diverged from the registry ({}):", diverged.len());
        for id in &diverged {
            println!("  {id}");
        }
        println!("  Review with 'akm skills diff <id>', then publish or revert.");
    }

    if !local.is_empty() {
        println!();
        println!("Not yet published ({}):", local.len());
        for id in &local {
            println!("  {id}");
        }
        println!("  Publish with 'akm skills publish <id>'.");
    }

    if drift.instructions().has_local_changes() {
        println!();
        println!(
            "Global instructions are {} — publish with 'akm instructions publish'.",
            drift.instructions()
        );
    }
}

/// CLI entry point for `akm skills sync [--quiet]`.
pub fn run_cli(paths: &Paths, quiet: bool) -> Result<()> {
    let config = Config::load(paths)?;
    let tool_dirs = ToolDirs::load(paths);

    let registry = Registry::new(
        config.registry_url().unwrap_or_default(),
        paths.library_dir(),
    );

    let shared = shared::refresh_all(paths, &config);
    let mut report = execute(paths, &registry, &tool_dirs, shared)?;
    // Reconcile the cache against config: drop checkouts of registries that were
    // removed with `akm config shared.<name> ""`, which cannot touch the cache
    // itself. Sync is the only place with both the config and the paths.
    report.swept = shared::sweep_orphans(paths, &config);
    print_report(&report, quiet);

    Ok(())
}
