//! `akm skills sync` — pull the registry → cold library → libgen → symlinks.
//!
//! Pipeline:
//! 1. Pull the personal registry to cache (clone if first time)
//! 2. Copy cache → cold library (clean slate)
//! 3. Run libgen to regenerate library.json
//! 4. Rebuild global symlinks for core specs

use crate::config::Config;
use crate::error::{Error, IoContext, Result};
use crate::library::libgen;
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use crate::registry::RegistrySource;
use std::path::Path;

/// Result of a sync operation, used for display.
#[derive(Debug)]
pub struct SyncReport {
    /// What happened with the skills registry.
    pub registry: RegistryOutcome,
    /// Whether the cold library was updated from the registry cache.
    pub library_copied: bool,
    /// Whether libgen ran and how many specs were found.
    pub spec_count: Option<usize>,
    /// Number of user core overrides preserved across sync.
    pub core_overrides_preserved: usize,
    /// Number of core symlinks created.
    pub symlink_count: usize,
    /// Number of global tool directories.
    pub tool_dir_count: usize,
}

/// Outcome of a single registry pull attempt.
#[derive(Debug)]
pub enum RegistryOutcome {
    /// Successfully pulled/cloned.
    Updated,
    /// Successfully cloned (first time).
    Cloned,
    /// Pull failed but cached copy is available.
    FailedWithCache { message: String },
    /// Pull failed, no cache, but library.json exists.
    FailedNoCacheButLibraryExists { message: String },
    /// Not configured — skipped (existing library available).
    Skipped,
    /// Not configured and no existing library — nothing to do.
    SkippedNoLibrary,
}

/// Execute the full sync pipeline.
///
/// This is the main entry point called by the CLI handler. It takes a
/// trait object for the registry to allow testing with mocks.
pub fn execute(
    paths: &Paths,
    registry: &dyn RegistrySource,
    tool_dirs: &ToolDirs,
) -> Result<SyncReport> {
    let library_dir = paths.data_dir();
    let library_json = paths.library_json();

    // --- Step 1: Sync the registry ---
    let registry_outcome = sync_registry(registry, &library_json)?;

    // Early exit: no registry configured and no existing library
    if matches!(registry_outcome, RegistryOutcome::SkippedNoLibrary) {
        return Ok(SyncReport {
            registry: registry_outcome,
            library_copied: false,
            spec_count: None,
            core_overrides_preserved: 0,
            symlink_count: 0,
            tool_dir_count: tool_dirs.count(),
        });
    }

    // --- Snapshot user core overrides (before the copy overwrites library.json) ---
    let core_overrides: std::collections::HashSet<String> =
        match Library::load_or_default(&library_json) {
            Ok(library) => library
                .specs
                .iter()
                .filter(|s| s.core)
                .map(|s| s.id.clone())
                .collect(),
            Err(e) => {
                eprintln!(
                    "Warning: Could not read existing library to preserve core overrides: {e}"
                );
                std::collections::HashSet::new()
            }
        };

    // --- Step 2: Copy registry cache → cold library ---
    let library_copied = if registry.is_cached() {
        copy_registry_to_library(registry.cache_dir(), library_dir)?;
        true
    } else {
        false
    };

    // --- Step 3: Run libgen ---
    let spec_count = if library_dir.join("skills").is_dir() || library_dir.join("agents").is_dir() {
        let result = libgen::generate(library_dir)?;
        Some(result.count)
    } else {
        None
    };

    // --- Step 3b: Restore user core overrides ---
    let core_overrides_preserved = if !core_overrides.is_empty() && library_json.is_file() {
        let mut library = Library::load_from(&library_json)?;
        let mut restored = 0usize;
        for spec in &mut library.specs {
            if core_overrides.contains(&spec.id) && !spec.core {
                spec.core = true;
                restored += 1;
            }
        }
        if restored > 0 {
            library.save_to(&library_json)?;
        }
        restored
    } else {
        0
    };

    // --- Step 4: Rebuild global symlinks ---
    let symlink_count = if library_json.is_file() {
        let library = Library::load_from(&library_json)?;
        let core_specs = library.core_specs();
        symlinks::rebuild_core(&core_specs, library_dir, tool_dirs.dirs())?
    } else {
        0
    };

    Ok(SyncReport {
        registry: registry_outcome,
        library_copied,
        spec_count,
        core_overrides_preserved,
        symlink_count,
        tool_dir_count: tool_dirs.count(),
    })
}

/// Attempt to sync the registry. Handles failure modes.
fn sync_registry(registry: &dyn RegistrySource, library_json: &Path) -> Result<RegistryOutcome> {
    if !registry.is_available() {
        return if library_json.is_file() {
            Ok(RegistryOutcome::Skipped)
        } else {
            Ok(RegistryOutcome::SkippedNoLibrary)
        };
    }

    match registry.pull() {
        Ok(crate::registry::PullOutcome::Fetched) => Ok(RegistryOutcome::Cloned),
        Ok(crate::registry::PullOutcome::Updated) => Ok(RegistryOutcome::Updated),
        Err(e) => {
            let message = format!("{e}");

            if registry.is_cached() {
                Ok(RegistryOutcome::FailedWithCache { message })
            } else if library_json.is_file() {
                Ok(RegistryOutcome::FailedNoCacheButLibraryExists { message })
            } else {
                Err(Error::NoSkillsAvailable)
            }
        }
    }
}

/// Copy registry cache contents to the cold library (clean slate).
fn copy_registry_to_library(cache_dir: &Path, library_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(library_dir).io_context(format!(
        "Creating cold library dir {}",
        library_dir.display()
    ))?;

    let cache_skills = cache_dir.join("skills");
    if cache_skills.is_dir() {
        let lib_skills = library_dir.join("skills");
        if lib_skills.exists() {
            std::fs::remove_dir_all(&lib_skills).io_context(format!(
                "Removing existing skills dir {}",
                lib_skills.display()
            ))?;
        }
        copy_dir_recursive(&cache_skills, &lib_skills)?;
    }

    let cache_agents = cache_dir.join("agents");
    if cache_agents.is_dir() {
        let lib_agents = library_dir.join("agents");
        if lib_agents.exists() {
            std::fs::remove_dir_all(&lib_agents).io_context(format!(
                "Removing existing agents dir {}",
                lib_agents.display()
            ))?;
        }
        copy_dir_recursive(&cache_agents, &lib_agents)?;
    }

    let cache_library = cache_dir.join("library.json");
    if cache_library.is_file() {
        std::fs::copy(&cache_library, library_dir.join("library.json")).io_context(format!(
            "Copying library.json from {}",
            cache_library.display()
        ))?;
    }

    Ok(())
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest).io_context(format!("Creating directory {}", dest.display()))?;

    let entries =
        std::fs::read_dir(src).io_context(format!("Reading directory {}", src.display()))?;

    for entry in entries {
        let entry = entry.io_context(format!("Reading entry in {}", src.display()))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path).io_context(format!(
                "Copying {} → {}",
                src_path.display(),
                dest_path.display()
            ))?;
        }
    }

    Ok(())
}

/// Print sync results to stdout.
pub fn print_report(report: &SyncReport, quiet: bool) {
    if quiet {
        return;
    }

    match &report.registry {
        RegistryOutcome::Updated => println!("Skills registry updated"),
        RegistryOutcome::Cloned => println!("Skills registry cloned"),
        RegistryOutcome::FailedWithCache { message } => {
            eprintln!("Warning: Failed to sync skills registry. {message}");
            eprintln!("Continuing with cached copy.");
        }
        RegistryOutcome::FailedNoCacheButLibraryExists { message } => {
            eprintln!("Warning: Failed to sync skills registry. {message}");
            eprintln!("Continuing with existing cold library.");
        }
        RegistryOutcome::Skipped => {
            println!("No skills registry configured. Working with existing cold library.");
        }
        RegistryOutcome::SkippedNoLibrary => {
            println!(
                "No skills registry configured and no existing cold library. Skipping skills sync."
            );
            return;
        }
    }

    if report.library_copied {
        println!("Cold library updated from skills registry");
    }

    if let Some(count) = report.spec_count {
        println!("Library regenerated ({count} specs)");
    }

    if report.core_overrides_preserved > 0 {
        println!(
            "Preserved {} local core override(s)",
            report.core_overrides_preserved
        );
    }

    println!(
        "{} core symlinks created across {} global tool directories",
        report.symlink_count, report.tool_dir_count
    );
}

/// CLI entry point for `akm skills sync [--quiet]`.
///
/// Constructs the concrete registry source from config and delegates
/// to `execute()` for the actual work.
pub fn run_cli(paths: &Paths, quiet: bool) -> Result<()> {
    let config = Config::load(paths)?;
    let tool_dirs = ToolDirs::load(paths);

    // An unconfigured registry yields an empty URL, which `is_available()`
    // reports as unavailable — sync then falls back to the cold library.
    let url = config.personal_registry_url().unwrap_or_default();
    let registry =
        crate::registry::git::GitRegistry::new("personal", url, paths.personal_registry_cache());

    let report = execute(paths, &registry, &tool_dirs)?;

    print_report(&report, quiet);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{PullOutcome, RegistrySource};
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// Mock registry for testing the sync pipeline without git/network.
    struct MockRegistry {
        name: String,
        cache_path: PathBuf,
        available: bool,
        pull_result: std::cell::RefCell<Option<Result<PullOutcome>>>,
    }

    impl MockRegistry {
        fn new(name: &str, cache_path: PathBuf) -> Self {
            Self {
                name: name.into(),
                cache_path,
                available: true,
                pull_result: std::cell::RefCell::new(None),
            }
        }

        fn with_pull_result(self, result: Result<PullOutcome>) -> Self {
            *self.pull_result.borrow_mut() = Some(result);
            self
        }

        fn with_available(mut self, available: bool) -> Self {
            self.available = available;
            self
        }
    }

    impl RegistrySource for MockRegistry {
        fn name(&self) -> &str {
            &self.name
        }

        fn pull(&self) -> Result<PullOutcome> {
            self.pull_result
                .borrow_mut()
                .take()
                .unwrap_or(Ok(PullOutcome::Updated))
        }

        fn push(&self) -> Result<()> {
            Ok(())
        }

        fn is_available(&self) -> bool {
            self.available
        }

        fn cache_dir(&self) -> &Path {
            &self.cache_path
        }

        fn is_cached(&self) -> bool {
            self.cache_path.is_dir()
        }
    }

    /// Create a minimal registry cache directory with one skill.
    fn create_mock_registry_cache(cache_dir: &Path) {
        let skill_dir = cache_dir.join("skills").join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: Test Skill\ndescription: A test\n---\nContent",
        )
        .unwrap();
    }

    #[test]
    fn copy_registry_to_library_clean_slate() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("cache");
        let library = tmp.path().join("library");

        create_mock_registry_cache(&cache);

        let old_skill = library.join("skills").join("old-skill");
        std::fs::create_dir_all(&old_skill).unwrap();
        std::fs::write(old_skill.join("SKILL.md"), "old").unwrap();

        copy_registry_to_library(&cache, &library).unwrap();

        assert!(library.join("skills").join("test-skill").is_dir());
        assert!(!library.join("skills").join("old-skill").exists());
    }

    #[test]
    fn full_pipeline_with_mock_registry() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");

        let paths = Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            &home,
        );

        let registry_cache = tmp
            .path()
            .join("cache")
            .join("akm")
            .join("skills-personal-registry");
        create_mock_registry_cache(&registry_cache);

        // Mark the skill core via its sidecar — the source of truth libgen reads.
        std::fs::write(
            registry_cache.join("skills/test-skill/akm.json"),
            r#"{"name":"Test Skill","description":"A test","tags":[],"core":true,"triggers":{}}"#,
        )
        .unwrap();

        let registry = MockRegistry::new("personal", registry_cache)
            .with_pull_result(Ok(PullOutcome::Updated));

        let tool_dirs = ToolDirs::builtin(&home);

        let report = execute(&paths, &registry, &tool_dirs).unwrap();

        assert!(matches!(report.registry, RegistryOutcome::Updated));
        assert!(report.library_copied);
        assert_eq!(report.spec_count, Some(1));
        assert_eq!(report.symlink_count, 1);
        assert_eq!(report.tool_dir_count, 5);

        // Verify the symlink was created in all tool dirs
        assert!(home
            .join(".claude")
            .join("skills")
            .join("test-skill")
            .is_symlink());
        assert!(home
            .join(".copilot")
            .join("skills")
            .join("test-skill")
            .is_symlink());
        assert!(home
            .join(".pi")
            .join("agent")
            .join("skills")
            .join("test-skill")
            .is_symlink());
    }

    #[test]
    fn sync_with_failed_registry_and_cache_continues() {
        let tmp = TempDir::new().unwrap();
        let paths = Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            &tmp.path().join("home"),
        );

        let registry_cache = tmp
            .path()
            .join("cache")
            .join("akm")
            .join("skills-personal-registry");
        create_mock_registry_cache(&registry_cache);

        let registry = MockRegistry::new("personal", registry_cache).with_pull_result(Err(
            Error::RegistrySync {
                name: "personal".into(),
                message: "network error".into(),
            },
        ));

        let tool_dirs = ToolDirs::builtin(tmp.path().join("home").as_path());

        let report = execute(&paths, &registry, &tool_dirs).unwrap();

        assert!(matches!(
            report.registry,
            RegistryOutcome::FailedWithCache { .. }
        ));
    }

    #[test]
    fn sync_with_no_cache_no_library_fails() {
        let tmp = TempDir::new().unwrap();
        let paths = Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            &tmp.path().join("home"),
        );

        let registry = MockRegistry::new("personal", tmp.path().join("nonexistent-cache"))
            .with_pull_result(Err(Error::RegistrySync {
                name: "personal".into(),
                message: "network error".into(),
            }));

        let tool_dirs = ToolDirs::builtin(tmp.path().join("home").as_path());

        let result = execute(&paths, &registry, &tool_dirs);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::NoSkillsAvailable));
    }

    /// A failed pull with no cache but an existing cold library degrades to a
    /// warning — the user keeps working with what is already on disk.
    #[test]
    fn sync_failure_with_existing_library_is_not_fatal() {
        let tmp = TempDir::new().unwrap();
        let paths = Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            &tmp.path().join("home"),
        );

        // Existing cold library on disk
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        std::fs::write(paths.library_json(), r#"{"version":1,"specs":[]}"#).unwrap();

        let registry = MockRegistry::new("personal", tmp.path().join("nonexistent-cache"))
            .with_pull_result(Err(Error::RegistrySync {
                name: "personal".into(),
                message: "network error".into(),
            }));

        let tool_dirs = ToolDirs::builtin(tmp.path().join("home").as_path());

        let report = execute(&paths, &registry, &tool_dirs)
            .expect("a failed pull must not be fatal when a library exists");

        assert!(matches!(
            report.registry,
            RegistryOutcome::FailedNoCacheButLibraryExists { .. }
        ));
        assert!(!report.library_copied);
    }

    /// Test the SkippedNoLibrary early exit path.
    #[test]
    fn no_registry_no_library_skips_cleanly() {
        let tmp = TempDir::new().unwrap();
        let paths = Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            &tmp.path().join("home"),
        );

        let registry =
            MockRegistry::new("personal", tmp.path().join("cache")).with_available(false);

        let tool_dirs = ToolDirs::builtin(tmp.path().join("home").as_path());

        let report = execute(&paths, &registry, &tool_dirs).unwrap();
        assert!(matches!(report.registry, RegistryOutcome::SkippedNoLibrary));
        assert_eq!(report.symlink_count, 0);
        assert!(report.spec_count.is_none());
    }

    #[test]
    fn copy_dir_recursive_works() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");

        std::fs::create_dir_all(src.join("subdir")).unwrap();
        std::fs::write(src.join("file.txt"), "hello").unwrap();
        std::fs::write(src.join("subdir").join("nested.txt"), "world").unwrap();

        copy_dir_recursive(&src, &dest).unwrap();

        assert!(dest.join("file.txt").is_file());
        assert!(dest.join("subdir").join("nested.txt").is_file());
        assert_eq!(
            std::fs::read_to_string(dest.join("file.txt")).unwrap(),
            "hello"
        );
    }

    /// User core overrides must survive a full sync cycle.
    ///
    /// Regression test: the registry ships `core: false` for a skill, but the
    /// user previously set it to `core: true`. After sync, the user's
    /// preference must be preserved.
    #[test]
    fn core_overrides_preserved_across_sync() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");

        let paths = Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            &home,
        );

        // Registry cache: ships test-skill with core: false
        let registry_cache = tmp
            .path()
            .join("cache")
            .join("akm")
            .join("skills-personal-registry");
        create_mock_registry_cache(&registry_cache);
        let cache_lib = r#"{"version":1,"specs":[{"id":"test-skill","type":"skill","name":"Test Skill","description":"A test","core":false,"tags":[],"triggers":{}}]}"#;
        std::fs::write(registry_cache.join("library.json"), cache_lib).unwrap();

        // User's existing library.json: test-skill is core: true
        let library_dir = paths.data_dir();
        std::fs::create_dir_all(library_dir).unwrap();
        let user_lib = r#"{"version":1,"specs":[{"id":"test-skill","type":"skill","name":"Test Skill","description":"A test","core":true,"tags":[],"triggers":{}}]}"#;
        std::fs::write(paths.library_json(), user_lib).unwrap();

        let registry = MockRegistry::new("personal", registry_cache)
            .with_pull_result(Ok(PullOutcome::Updated));

        let tool_dirs = ToolDirs::builtin(&home);

        let report = execute(&paths, &registry, &tool_dirs).unwrap();

        assert_eq!(report.core_overrides_preserved, 1);

        // Verify library.json on disk still has core: true
        let library = Library::load_from(&paths.library_json()).unwrap();
        let spec = library.get("test-skill").expect("test-skill must exist");
        assert!(spec.core, "User core override must survive sync");
    }
}
