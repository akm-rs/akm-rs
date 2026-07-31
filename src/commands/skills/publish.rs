//! `akm skills publish` — commit and push one spec to the personal registry.
//!
//! The library *is* the registry's working tree, so publishing is no longer a
//! copy: it stages the spec's own paths, commits them and pushes. Metadata
//! travels in the spec's `akm.json` sidecar, so nothing has to be patched onto
//! a shared index on the way out.

use crate::config::Config;
use crate::error::{Error, Result};
use crate::library::drift::DriftState;
use crate::library::spec::Spec;
use crate::library::Library;
use crate::paths::Paths;
use crate::registry::{PublishOutcome, Registry};
use std::io::{self, BufRead, IsTerminal, Write};

/// Run the `akm skills publish` command.
pub fn run(paths: &Paths, config: &Config, id: &str, dry_run: bool) -> Result<()> {
    let url = config.registry_url().ok_or(Error::NoPersonalRegistry)?;
    let registry = Registry::new(url, paths.library_dir());

    // Resolve the spec before checking the checkout: a typo in the id deserves
    // "no such spec", not "run sync first".
    let library = Library::load_checked(paths)?;
    let spec = library
        .get(id)
        .ok_or_else(|| Error::SpecNotFound { id: id.to_string() })?;

    if !spec.exists_on_disk(&paths.library_dir()) {
        return Err(Error::SpecNotFound { id: id.to_string() });
    }

    if !registry.is_cloned() {
        return Err(Error::RegistrySync {
            name: "personal".into(),
            message: "The library is not a registry checkout. Run 'akm skills sync' first.".into(),
        });
    }

    println!("Publishing spec:");
    println!("  id:          {id}");
    println!("  type:        {}", spec.spec_type);
    println!("  name:        {}", spec.name);
    println!("  description: {}", spec.description);
    println!("  remote:      {url}");
    println!();

    let pathspecs = spec.pathspecs();

    // Ask the remote where it stands before deciding how to land the change:
    // publishing on a stale picture of the registry is how a push gets
    // rejected halfway through.
    registry.refresh()?;
    let state = registry.drift()?.state_of(id);

    if dry_run {
        return show_dry_run(&registry, spec, state);
    }

    // A spec the remote has also changed is rebased ours-wins: take the
    // remote's version of the path, lay the local content back over it, then
    // commit. No merge runs, so no conflict markers can reach a live skill.
    if state == DriftState::Diverged {
        println!(
            "  Registry also changed this {} — keeping the local version",
            spec.spec_type
        );
        registry.adopt_remote_then_keep_local(&pathspecs)?;
    }

    let message = format!("feat: publish {} '{id}'", spec.spec_type);
    match registry.publish(&pathspecs, &message)? {
        PublishOutcome::NothingToDo => {
            println!("No changes to publish — spec '{id}' already matches the registry.");
        }
        PublishOutcome::Published => {
            println!("  Committed: {message}");
            println!("  Pushed to {url}");
            println!();
            println!(
                "Published {} '{id}' to the personal registry",
                spec.spec_type
            );
        }
    }

    Ok(())
}

/// Show what publishing would send, without touching the remote.
fn show_dry_run(registry: &Registry, spec: &Spec, state: DriftState) -> Result<()> {
    let pathspecs = spec.pathspecs();

    println!("Dry run — {state}.");

    let local = registry.diff_local(&pathspecs)?;
    if local.is_empty() {
        println!("No local changes to publish.");
    } else {
        println!();
        println!("Changes that would be pushed:");
        println!("{local}");
    }

    if state == DriftState::Diverged {
        println!();
        println!("The registry also changed this spec. Publishing keeps the local version:");
        println!("{}", registry.diff_remote(&pathspecs)?);
    }

    Ok(())
}

/// Offer to publish `id` to the personal registry, after a promote or import.
///
/// Silently does nothing unless stdin is a TTY and a personal registry is
/// configured. A failed publish is reported as a warning: the caller's work is
/// already in the library, so it must not fail the whole command.
pub(crate) fn offer(paths: &Paths, config: &Config, id: &str) {
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
    if let Err(e) = run(paths, config, id, false) {
        eprintln!("Warning: publish failed: {e}");
        eprintln!("Retry with: akm skills publish {id}");
    }
}
