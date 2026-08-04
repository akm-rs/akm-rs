//! `akm skills share <remote> <id>` — offer one of your specs to a shared registry.
//!
//! Distinct from `publish`, which pushes to *your* registry, on your own
//! authority. A shared registry belongs to someone else, so a contribution is a
//! branch and a request, and whoever owns the registry decides. AKM pushes the
//! branch and stops there.
//!
//! The push is deliberately all AKM does about the pull request. Git's own
//! output already carries the URL for opening one — GitHub, GitLab, Gitea and
//! Bitbucket all print it on a first push — so relaying that verbatim works on
//! every forge and needs no API token.

use crate::config::Config;
use crate::error::{Error, IoContext, Result};
use crate::library::spec::Spec;
use crate::library::Library;
use crate::paths::Paths;
use crate::registry::Registry;
use std::path::Path;

/// Run the `akm skills share` command.
pub fn run(paths: &Paths, config: &Config, remote: &str, id: &str, dry_run: bool) -> Result<()> {
    let url = config.shared_remote(remote)?;

    let library_dir = paths.library_dir();
    let library = Library::load_checked(paths)?;
    let spec = library
        .get(id)
        .ok_or_else(|| Error::SpecNotFound { id: id.to_string() })?;

    if !spec.exists_on_disk(&library_dir) {
        return Err(Error::SpecNotFound { id: id.to_string() });
    }

    let branch = format!("akm/{id}");

    println!("Sharing {} '{id}' with '{remote}':", spec.spec_type);
    println!("  remote: {url}");
    println!("  branch: {branch}");
    println!();

    if dry_run {
        println!("Dry run — nothing was pushed.");
        return Ok(());
    }

    // A throwaway clone, not the cached checkout: sharing must never leave the
    // browsable copy of a shared registry parked on a contribution branch.
    let tmp = tempfile::tempdir().io_context("Creating a temporary directory for the clone")?;
    let staging = Registry::named(remote, url, tmp.path().join("registry"));
    staging.clone_fresh()?;
    println!("  Cloned {remote}");

    staging.checkout_contribution_branch(&branch)?;
    copy_spec(spec, &library_dir, staging.dir())?;

    let message = format!("feat: add {} '{id}'", spec.spec_type);
    match staging.push_contribution(&branch, &spec.pathspecs(), &message)? {
        None => {
            println!();
            println!(
                "Nothing to share — '{remote}' already has this exact {}.",
                spec.spec_type
            );
        }
        Some(hint) => {
            println!("  Committed: {message}");
            println!("  Pushed branch {branch}");
            if !hint.is_empty() {
                println!();
                println!("{hint}");
            }
            println!();
            println!("Open a pull request on '{remote}' to finish sharing '{id}'.");
        }
    }

    Ok(())
}

/// Copy every file belonging to a spec into the staging checkout.
///
/// Driven by the spec's own pathspecs, so an agent's markdown and its sidecar
/// travel together and a skill's whole directory goes as one — the same unit
/// git operates on everywhere else.
fn copy_spec(spec: &Spec, library_dir: &Path, dest_root: &Path) -> Result<()> {
    for rel in spec.pathspecs() {
        let src = library_dir.join(&rel);
        let dest = dest_root.join(&rel);

        if src.is_dir() {
            // The remote may already have an older copy; replacing it wholesale
            // is what makes a re-share a clean update rather than a merge of
            // two file sets.
            if dest.exists() {
                std::fs::remove_dir_all(&dest)
                    .io_context(format!("Removing {} before copying", dest.display()))?;
            }
            super::promote::copy_dir_recursive(&src, &dest)?;
        } else if src.is_file() {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .io_context(format!("Creating {}", parent.display()))?;
            }
            std::fs::copy(&src, &dest).io_context(format!(
                "Copying {} to {}",
                src.display(),
                dest.display()
            ))?;
        }
    }

    Ok(())
}
