//! `akm skills publish` — publish spec to personal registry.
//!
//! Bash: `cmd_skills_publish()` at bin/akm:2084–2244.
//!
//! All git operations go through the RegistrySource trait / GitRegistry.

use crate::config::Config;
use crate::error::{Error, IoContext, Result};
use crate::git::Git;
use crate::library::libgen;
use crate::library::spec::SpecType;
use crate::library::Library;
use crate::paths::Paths;
use crate::registry::git::GitRegistry;
use crate::registry::RegistrySource;
use std::path::Path;

/// Run the `akm skills publish` command.
pub fn run(paths: &Paths, config: &Config, id: &str, dry_run: bool) -> Result<()> {
    // Step 1: Verify personal registry configured
    let personal_url = config
        .skills
        .personal_registry
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or(Error::NoPersonalRegistry)?;

    // Step 2: Verify spec exists in cold storage
    let library = Library::load_checked(paths)?;
    let spec = library
        .get(id)
        .ok_or_else(|| Error::SpecNotFound { id: id.to_string() })?;

    let source_path = spec.source_path(paths.data_dir());
    if !source_path.exists() {
        return Err(Error::SpecNotFound { id: id.to_string() });
    }

    // Display what will be published
    println!("Publishing spec:");
    println!("  id:          {id}");
    println!("  type:        {}", spec.spec_type);
    println!("  name:        {}", spec.name);
    println!("  description: {}", spec.description);
    println!("  remote:      {personal_url}");
    println!();

    // Step 3: Clone or pull personal registry
    let registry = GitRegistry::new("personal", personal_url, paths.personal_registry_cache());

    match registry.pull() {
        Ok(outcome) => {
            let msg = match outcome {
                crate::registry::PullOutcome::Fetched => "Personal registry cloned",
                crate::registry::PullOutcome::Updated => "Personal registry updated",
            };
            println!("  {msg}");
        }
        Err(e) => {
            if registry.is_cached() {
                eprintln!("Warning: {e}");
                eprintln!("Continuing with cached copy.");
            } else {
                return Err(e);
            }
        }
    }

    // Step 4: Copy spec into registry cache
    let cache_dir = registry.cache_dir();
    copy_spec_to_registry(spec, &source_path, cache_dir)?;
    println!("  Copied {} to personal registry", spec.spec_type);

    // Step 5: Regenerate library.json in cache
    libgen::generate(cache_dir)?;

    // Step 6: Dry run — show diff, reset
    if dry_run {
        println!();
        println!("Dry run — changes that would be pushed:");
        Git::add_all(cache_dir)?;
        let diff_output = Git::diff_cached_stat(cache_dir)?;
        println!("{diff_output}");
        let diff_full = Git::diff_cached(cache_dir)?;
        println!("{diff_full}");
        Git::reset(cache_dir)?;
        return Ok(());
    }

    // Step 7: Commit
    Git::add_all(cache_dir)?;

    if Git::is_staging_clean(cache_dir)? {
        println!("No changes to publish — spec '{id}' is already up to date in the registry.");
        return Ok(());
    }

    let commit_msg = format!("feat: publish {} '{id}'", spec.spec_type);
    Git::commit(cache_dir, &commit_msg)?;
    println!("  Committed: {commit_msg}");

    // Step 8: Push (via RegistrySource trait)
    registry.push()?;
    println!("  Pushed to {personal_url}");
    println!();
    println!("Published {} '{id}' to personal registry", spec.spec_type);

    Ok(())
}

/// Copy a spec from cold storage into the registry cache directory.
fn copy_spec_to_registry(
    spec: &crate::library::spec::Spec,
    source_path: &Path,
    cache_dir: &Path,
) -> Result<()> {
    let subdir = spec.spec_type.subdir();

    match spec.spec_type {
        SpecType::Skill => {
            let dest = cache_dir.join(subdir).join(&spec.id);
            std::fs::create_dir_all(dest.parent().unwrap_or(&dest))
                .io_context("Creating registry skill directory")?;
            if dest.exists() {
                std::fs::remove_dir_all(&dest)
                    .io_context(format!("Removing existing {}", dest.display()))?;
            }
            super::promote::copy_dir_recursive(source_path, &dest)?;
        }
        SpecType::Agent => {
            let dest = cache_dir.join(subdir).join(format!("{}.md", spec.id));
            std::fs::create_dir_all(cache_dir.join(subdir))
                .io_context("Creating registry agents directory")?;
            std::fs::copy(source_path, &dest)
                .io_context(format!("Copying agent to {}", dest.display()))?;
        }
    }

    Ok(())
}
