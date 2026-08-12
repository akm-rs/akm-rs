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

/// Publish every spec holding changes the registry has not seen.
///
/// One commit and one push for the whole set: the unit of work is the user's
/// intent, not the number of specs it happened to touch. Staging is by
/// explicit spec pathspec, never `-A`, so the derived `library.json` index is
/// left alone.
pub fn run_all(paths: &Paths, config: &Config, dry_run: bool) -> Result<()> {
    let url = config.registry_url().ok_or(Error::NoPersonalRegistry)?;
    let library_dir = paths.library_dir();
    let registry = Registry::new(url, &library_dir);

    if !registry.is_cloned() {
        return Err(Error::RegistrySync {
            name: "personal".into(),
            message: "The library is not a registry checkout. Run 'akm skills sync' first.".into(),
        });
    }

    let library = Library::load_checked(paths)?;

    // Ask the remote where it stands once, not once per spec.
    registry.refresh()?;
    let drift = registry.drift()?;

    let pending: Vec<&Spec> = library
        .specs
        .iter()
        .filter(|s| drift.state_of(&s.id).has_local_changes())
        .collect();

    if pending.is_empty() {
        println!("Nothing to publish — every spec matches the registry.");
        return Ok(());
    }

    let ids: Vec<String> = pending.iter().map(|s| s.id.clone()).collect();
    let mut pathspecs: Vec<String> = Vec::new();
    for spec in &pending {
        pathspecs.extend(spec.pathspecs());
    }

    if dry_run {
        println!("Dry run — {} spec(s) would be published:", ids.len());
        for id in &ids {
            println!("  {id}");
        }
        println!();
        println!("Changes that would be pushed:");
        println!("{}", registry.diff_local(&pathspecs)?);
        return Ok(());
    }

    // A spec the remote also changed is rebased ours-wins, same as the
    // single-spec path.
    let diverged: Vec<String> = pending
        .iter()
        .filter(|s| drift.state_of(&s.id) == DriftState::Diverged)
        .flat_map(|s| s.pathspecs())
        .collect();
    if !diverged.is_empty() {
        registry.adopt_remote_then_keep_local(&diverged)?;
    }

    let message = format!("feat: publish {} spec(s)\n\n{}", ids.len(), ids.join(", "));

    match registry.publish(&pathspecs, &message)? {
        PublishOutcome::NothingToDo => println!("Nothing to publish."),
        PublishOutcome::Published => {
            println!("Published {} spec(s):", ids.len());
            for id in &ids {
                println!("  {id}");
            }
            println!();
            println!("Pushed to {}", registry.url());
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

/// Offer to publish an explicit set of paths to the personal registry.
///
/// Used by rename and delete, whose change spans paths that do not map to a
/// single live spec id (a rename touches both the old and new paths; a delete
/// removes a spec entirely). Silently does nothing unless stdin is a TTY and a
/// personal registry is cloned. A failed publish is a warning, never a hard
/// error: the local library change has already landed.
pub(crate) fn offer_pathspecs(paths: &Paths, config: &Config, pathspecs: &[String], message: &str) {
    if !io::stdin().is_terminal() {
        return;
    }
    let Some(url) = config.registry_url() else {
        return;
    };
    let registry = Registry::new(url, paths.library_dir());
    if !registry.is_cloned() {
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

    if let Err(e) = publish_pathspecs(&registry, pathspecs, message) {
        eprintln!("Warning: publish failed: {e}");
        eprintln!("Retry with: akm skills publish");
    }
}

/// Refresh, then commit and push an explicit pathspec set.
///
/// Unlike the single-spec path there is no ours-wins rebase: a remote that has
/// diverged on these paths will reject the push, which is surfaced as-is.
fn publish_pathspecs(registry: &Registry, pathspecs: &[String], message: &str) -> Result<()> {
    registry.refresh()?;
    match registry.publish(pathspecs, message)? {
        PublishOutcome::NothingToDo => println!("No changes to publish."),
        PublishOutcome::Published => println!("Pushed to {}", registry.url()),
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
