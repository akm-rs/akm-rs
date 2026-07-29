//! `akm skills publish` — publish spec to personal registry.
//!
//! All git operations go through the RegistrySource trait / GitRegistry.

use crate::config::Config;
use crate::error::{Error, IoContext, Result};
use crate::git::Git;
use crate::library::libgen;
use crate::library::spec::{Spec, SpecType};
use crate::library::Library;
use crate::paths::Paths;
use crate::registry::git::GitRegistry;
use crate::registry::RegistrySource;
use std::io::{self, BufRead, IsTerminal, Write};
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

    // Step 5: Regenerate library.json in cache, then carry local metadata over
    libgen::generate(cache_dir)?;
    apply_local_metadata(cache_dir, spec)?;

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

/// Offer to publish `id` to the personal registry, after a promote or import.
///
/// Silently does nothing unless stdin is a TTY and a personal registry is
/// configured. A failed publish is reported as a warning: the caller's work is
/// already committed to cold storage, so it must not fail the whole command.
pub(crate) fn offer(paths: &Paths, config: &Config, id: &str) {
    if !io::stdin().is_terminal() {
        return;
    }

    let configured = config
        .skills
        .personal_registry
        .as_deref()
        .is_some_and(|url| !url.is_empty());
    if !configured {
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

/// Copy user-edited metadata onto the registry's `library.json` entry.
///
/// `libgen` derives entries for specs the registry has not seen from SKILL.md
/// frontmatter, so the description and tags set locally (via promote, import or
/// edit) would otherwise never be published. `core` is deliberately excluded —
/// sync treats it as a per-user override rather than a property of the spec.
fn apply_local_metadata(cache_dir: &Path, spec: &Spec) -> Result<()> {
    let library_path = cache_dir.join("library.json");
    let mut library = Library::load_from(&library_path)?;

    if let Some(entry) = library.get_mut(&spec.id) {
        entry.description = spec.description.clone();
        entry.tags = spec.tags.clone();
    }

    library.save_to(&library_path)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Build a registry cache holding one skill, with library.json generated
    /// by libgen (i.e. metadata derived from frontmatter).
    fn cache_with_generated_library(dir: &Path) {
        let skill_dir = dir.join("skills").join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: Test Skill\ndescription: Frontmatter description\n---\nContent",
        )
        .unwrap();
        libgen::generate(dir).unwrap();
    }

    fn local_spec() -> Spec {
        let mut spec = Spec::new(
            "test-skill".to_string(),
            SpecType::Skill,
            "Test Skill".to_string(),
            "Edited description".to_string(),
        );
        spec.tags = vec!["testing".to_string(), "rust".to_string()];
        spec.core = true;
        spec
    }

    /// The description and tags a user typed at the promote/import prompts must
    /// reach the registry, not the frontmatter values libgen derives.
    #[test]
    fn local_description_and_tags_are_published() {
        let tmp = TempDir::new().unwrap();
        cache_with_generated_library(tmp.path());

        let library = Library::load_from(&tmp.path().join("library.json")).unwrap();
        assert_eq!(
            library.get("test-skill").unwrap().description,
            "Frontmatter description"
        );

        apply_local_metadata(tmp.path(), &local_spec()).unwrap();

        let library = Library::load_from(&tmp.path().join("library.json")).unwrap();
        let entry = library.get("test-skill").unwrap();
        assert_eq!(entry.description, "Edited description");
        assert_eq!(entry.tags, vec!["testing", "rust"]);
    }

    /// `core` is a per-user preference that sync preserves as a local override,
    /// so publishing must not push it onto every consumer of the registry.
    #[test]
    fn core_flag_is_not_published() {
        let tmp = TempDir::new().unwrap();
        cache_with_generated_library(tmp.path());

        apply_local_metadata(tmp.path(), &local_spec()).unwrap();

        let library = Library::load_from(&tmp.path().join("library.json")).unwrap();
        assert!(!library.get("test-skill").unwrap().core);
    }

    /// A spec absent from the registry cache must not create a phantom entry.
    #[test]
    fn unknown_spec_leaves_library_untouched() {
        let tmp = TempDir::new().unwrap();
        cache_with_generated_library(tmp.path());

        let mut spec = local_spec();
        spec.id = "not-in-registry".to_string();
        apply_local_metadata(tmp.path(), &spec).unwrap();

        let library = Library::load_from(&tmp.path().join("library.json")).unwrap();
        assert!(library.get("not-in-registry").is_none());
        assert_eq!(
            library.get("test-skill").unwrap().description,
            "Frontmatter description"
        );
    }
}
