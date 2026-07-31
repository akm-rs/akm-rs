//! `akm instructions publish` — push the global instructions to the registry.
//!
//! The instructions file is one tracked path inside the library working tree,
//! so publishing it is the same operation as publishing a spec: stage that
//! path, commit it, push it. A registry that also moved is handled ours-wins,
//! never by a merge.

use crate::commands::instructions::seed_from_legacy;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::library::drift::{DriftState, INSTRUCTIONS_PATH};
use crate::paths::Paths;
use crate::registry::{PublishOutcome, Registry};
use std::io::{self, BufRead, IsTerminal, Write};

/// Run `akm instructions publish`.
pub fn run(paths: &Paths, config: &Config) -> Result<()> {
    let url = config.registry_url().ok_or(Error::NoPersonalRegistry)?;
    let registry = Registry::new(url, paths.library_dir());

    seed_from_legacy(paths)?;

    let source = paths.instructions_file();
    if !source.is_file() {
        eprintln!(
            "Warning: No global instructions file found at {}",
            source.display()
        );
        eprintln!("Run 'akm instructions edit' to create one.");
        return Ok(());
    }

    if !registry.is_cloned() {
        return Err(Error::RegistrySync {
            name: "personal".into(),
            message: "The library is not a registry checkout. Run 'akm skills sync' first.".into(),
        });
    }

    // Drift is measured against `@{upstream}`; publishing on a stale picture of
    // the registry is how a push gets rejected halfway through.
    registry.refresh()?;
    let state = registry.drift()?.instructions();
    let pathspecs = vec![INSTRUCTIONS_PATH.to_string()];

    if state == DriftState::Diverged {
        println!("  Registry also changed the instructions — keeping the local version");
        registry.adopt_remote_then_keep_local(&pathspecs)?;
    }

    let message = "docs: publish global instructions";
    match registry.publish(&pathspecs, message)? {
        PublishOutcome::NothingToDo => {
            println!("No changes to publish — global instructions already match the registry.");
        }
        PublishOutcome::Published => {
            println!("  Committed: {message}");
            println!("  Pushed to {url}");
            println!();
            println!("Published global instructions to the personal registry");
        }
    }

    Ok(())
}

/// Offer to publish the global instructions, after editing them.
///
/// Silently does nothing unless stdin is a TTY and a personal registry is
/// configured. A failed publish is a warning, never a failure: the edit is
/// already on disk and already distributed.
pub(crate) fn offer(paths: &Paths, config: &Config) {
    if !io::stdin().is_terminal() || config.registry_url().is_none() {
        return;
    }

    println!();
    print!("Publish to personal registry? [y/N]: ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input).ok();
    if !input.trim().eq_ignore_ascii_case("y") {
        return;
    }

    println!();
    if let Err(e) = run(paths, config) {
        eprintln!("Warning: publish failed: {e}");
        eprintln!("Retry with: akm instructions publish");
    }
}
