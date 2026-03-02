//! `akm setup` — interactive feature configuration wizard.
//!
//! Bash equivalent: `cmd_setup()` at bin/akm:645.
//!
//! Prompts the user to enable/disable each domain (skills, artifacts,
//! instructions), configure remotes, then writes config, installs shell
//! init, patches .bashrc, and runs initial sync.

use crate::config::{Config, Feature};
use crate::error::{Error, Result};
use crate::library::tool_dirs::ToolDirs;
use crate::paths::Paths;
use crate::shell;
use std::io::{self, BufRead, Write};

/// Default community registry URL.
///
/// Bash: `DEFAULT_SKILLS_COMMUNITY_REGISTRY` at bin/akm:11
const DEFAULT_COMMUNITY_REGISTRY: &str = "https://github.com/akm-rs/skillverse.git";

/// Scoped setup flags — which domains to configure.
///
/// Bash: `do_skills`, `do_artifacts`, `do_instructions` flags at bin/akm:647–648
#[derive(Debug, Clone)]
pub struct SetupScope {
    /// Whether to configure the skills domain.
    pub skills: bool,
    /// Whether to configure the artifacts domain.
    pub artifacts: bool,
    /// Whether to configure the instructions domain.
    pub instructions: bool,
}

impl SetupScope {
    /// All domains (default when no scope flags passed).
    pub fn all() -> Self {
        Self {
            skills: true,
            artifacts: true,
            instructions: true,
        }
    }
}

/// Trait for interactive prompts — allows testing without TTY.
///
/// The real implementation reads from stdin. Tests provide canned responses.
pub trait Prompter {
    /// Ask a yes/no question. Returns true for yes.
    /// `default_yes` controls what Enter (empty input) means.
    fn confirm(&mut self, message: &str, default_yes: bool) -> Result<bool>;

    /// Ask for a text input. Returns the entered string (may be empty).
    /// `default` is shown in brackets and returned on empty input.
    fn input(&mut self, message: &str, default: &str) -> Result<String>;
}

/// Interactive prompter that reads from stdin.
pub struct StdinPrompter;

impl Prompter for StdinPrompter {
    fn confirm(&mut self, message: &str, default_yes: bool) -> Result<bool> {
        let suffix = if default_yes { "[Y/n]" } else { "[y/N]" };
        print!("  {} {}: ", message, suffix);
        io::stdout().flush().map_err(|e| Error::Io {
            context: "flushing stdout".into(),
            source: e,
        })?;

        let mut input = String::new();
        io::stdin()
            .lock()
            .read_line(&mut input)
            .map_err(|e| Error::Io {
                context: "reading input".into(),
                source: e,
            })?;

        let trimmed = input.trim();
        if trimmed.is_empty() {
            Ok(default_yes)
        } else {
            Ok(trimmed.eq_ignore_ascii_case("y") || trimmed.eq_ignore_ascii_case("yes"))
        }
    }

    fn input(&mut self, message: &str, default: &str) -> Result<String> {
        if default.is_empty() {
            print!("  {}: ", message);
        } else {
            print!("  {} [{}]: ", message, default);
        }
        io::stdout().flush().map_err(|e| Error::Io {
            context: "flushing stdout".into(),
            source: e,
        })?;

        let mut input = String::new();
        io::stdin()
            .lock()
            .read_line(&mut input)
            .map_err(|e| Error::Io {
                context: "reading input".into(),
                source: e,
            })?;

        let trimmed = input.trim();
        if trimmed.is_empty() {
            Ok(default.to_string())
        } else {
            Ok(trimmed.to_string())
        }
    }
}

/// Run the setup wizard.
///
/// Bash equivalent: `cmd_setup()` at bin/akm:645.
///
/// Flow:
/// 1. Load existing config (for defaults on re-run)
/// 2. Display header with supported tools
/// 3. For each enabled scope: prompt to enable/disable + configure
/// 4. Write config to disk
/// 5. Install shell init + tools.json
/// 6. Patch .bashrc
/// 7. Run initial sync for enabled domains
pub fn run(paths: &Paths, scope: SetupScope, prompter: &mut dyn Prompter) -> Result<()> {
    // Step 1: Load existing config
    let mut config = Config::load(paths)?;
    let tool_dirs = ToolDirs::load(paths);

    // Step 2: Header
    println!("AKM — Agent Kit Manager");
    println!("Supported tools: {}", tool_dirs.display_names());
    println!();

    // Step 3: Interactive configuration per domain
    if scope.skills {
        configure_skills(&mut config, prompter)?;
    }
    if scope.artifacts {
        configure_artifacts(&mut config, paths, prompter)?;
    }
    if scope.instructions {
        configure_instructions(&mut config, paths, prompter)?;
    }

    // Step 4: Write config
    println!("Writing config to {}...", paths.config_file().display());
    config.save(paths)?;
    println!("Config saved");
    println!();

    // Step 5: Install shell init + tools.json
    shell::install_shell_init(paths)?;
    shell::install_tools_json(paths)?;

    // Step 6: Patch .bashrc
    println!("Patching ~/.bashrc...");
    shell::patch_bashrc(paths)?;
    println!("Shell integration installed");
    println!();

    // Step 7: Initial sync
    println!("Running initial sync...");
    run_initial_sync(paths, &config);
    println!();

    println!("Done! Open a new terminal or run: source ~/.bashrc");

    Ok(())
}

/// Configure the skills domain interactively.
///
/// Bash equivalent: bin/akm:687–733
fn configure_skills(config: &mut Config, prompter: &mut dyn Prompter) -> Result<()> {
    println!("Skills management (cold library, session loading, project manifests)");

    if !prompter.confirm("Enable?", true)? {
        config.features.remove(&Feature::Skills);
        println!("  Skills disabled");
        println!();
        return Ok(());
    }

    config.features.insert(Feature::Skills);

    // Community registry
    if prompter.confirm(
        "Use Skillverse (community skills registry) as a source?",
        true,
    )? {
        config.skills.community_registry = Some(DEFAULT_COMMUNITY_REGISTRY.to_string());
    } else {
        let custom = prompter.input("Enter custom skills registry URL (or leave empty)", "")?;
        config.skills.community_registry = if custom.is_empty() {
            None
        } else {
            Some(custom)
        };
    }

    // Personal registry
    if prompter.confirm("Configure a personal skills registry?", false)? {
        let default = config.skills.personal_registry.as_deref().unwrap_or("");
        let url = prompter.input("Enter your personal registry URL", default)?;
        config.skills.personal_registry = if url.is_empty() { None } else { Some(url) };
    }

    println!("  Skills enabled");
    println!();
    Ok(())
}

/// Configure the artifacts domain interactively.
///
/// Bash equivalent: bin/akm:737–783
fn configure_artifacts(
    config: &mut Config,
    paths: &Paths,
    prompter: &mut dyn Prompter,
) -> Result<()> {
    println!("Artifacts (auto-sync LLM session outputs to a git repo)");

    if !prompter.confirm("Enable?", true)? {
        config.features.remove(&Feature::Artifacts);
        println!("  Artifacts disabled");
        println!();
        return Ok(());
    }

    // Remote URL (required)
    let remote_default = config.artifacts.remote.as_deref().unwrap_or("");
    let remote = prompter.input("Git remote URL (SSH or HTTPS)", remote_default)?;

    if remote.is_empty() {
        eprintln!("  Artifacts remote is required when artifacts is enabled.");
        config.features.remove(&Feature::Artifacts);
        println!("  Artifacts disabled");
        println!();
        return Ok(());
    }

    config.features.insert(Feature::Artifacts);
    config.artifacts.remote = Some(remote);

    // Local directory
    let dir_default = config
        .artifacts
        .dir
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| paths.default_artifacts_dir().display().to_string());
    let dir = prompter.input("Local directory", &dir_default)?;
    config.artifacts.dir = Some(dir.into());

    // Auto-push
    config.artifacts.auto_push = prompter.confirm("Auto-push on session exit?", true)?;

    println!("  Artifacts enabled");
    println!();
    Ok(())
}

/// Configure the instructions domain interactively.
///
/// Bash equivalent: bin/akm:786–819
fn configure_instructions(
    config: &mut Config,
    paths: &Paths,
    prompter: &mut dyn Prompter,
) -> Result<()> {
    println!("Global instructions (distribute a shared instructions file to all LLM tools)");

    if !prompter.confirm("Enable?", true)? {
        config.features.remove(&Feature::Instructions);
        println!("  Instructions disabled");
        println!();
        return Ok(());
    }

    config.features.insert(Feature::Instructions);

    // Create global-instructions.md if it doesn't exist
    let instructions_file = paths.global_instructions();
    if !instructions_file.exists() {
        if let Some(parent) = instructions_file.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!(
                    "  Warning: could not create directory {}: {}",
                    parent.display(),
                    e
                );
            }
        }
        match std::fs::write(&instructions_file, "") {
            Ok(()) => {
                println!("  Instructions enabled");
                println!(
                    "  Created {} (edit with 'akm instructions edit')",
                    instructions_file.display()
                );
            }
            Err(e) => {
                eprintln!(
                    "  Warning: could not create {}: {}",
                    instructions_file.display(),
                    e
                );
                println!("  Instructions enabled (file creation failed, create manually)");
            }
        }
    } else {
        println!("  Instructions enabled");
        println!(
            "  Instructions file exists at {}",
            instructions_file.display()
        );
    }

    println!();
    Ok(())
}

/// Run initial sync for all enabled features.
///
/// Bash equivalent: bin/akm:841–865
/// Errors are caught and printed as warnings (non-fatal).
fn run_initial_sync(paths: &Paths, config: &Config) {
    if config.is_feature_enabled(Feature::Skills) {
        match crate::commands::skills::sync::run_cli(paths, true) {
            Ok(()) => println!("  Skills: cold library synced"),
            Err(e) => eprintln!("  Skills: sync skipped ({})", e),
        }
    }

    if config.is_feature_enabled(Feature::Artifacts) {
        match crate::commands::artifacts::sync::run(config, paths) {
            Ok(()) => {
                let dir = config
                    .artifacts
                    .dir
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "default".into());
                println!("  Artifacts: synced to {}", dir);
            }
            Err(e) => eprintln!("  Artifacts: sync failed ({})", e),
        }
    }

    if config.is_feature_enabled(Feature::Instructions) {
        match crate::commands::instructions::sync::run(paths) {
            Ok(()) => println!("  Instructions: distributed to tool directories"),
            Err(e) => eprintln!("  Instructions: sync skipped ({})", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test prompter with canned responses.
    struct TestPrompter {
        responses: Vec<String>,
        index: usize,
    }

    impl TestPrompter {
        fn new(responses: Vec<&str>) -> Self {
            Self {
                responses: responses.into_iter().map(String::from).collect(),
                index: 0,
            }
        }
    }

    impl Prompter for TestPrompter {
        fn confirm(&mut self, _message: &str, default_yes: bool) -> Result<bool> {
            if self.index >= self.responses.len() {
                return Ok(default_yes);
            }
            let resp = &self.responses[self.index];
            self.index += 1;
            Ok(resp.is_empty() && default_yes
                || resp.eq_ignore_ascii_case("y")
                || resp.eq_ignore_ascii_case("yes"))
        }

        fn input(&mut self, _message: &str, default: &str) -> Result<String> {
            if self.index >= self.responses.len() {
                return Ok(default.to_string());
            }
            let resp = &self.responses[self.index];
            self.index += 1;
            if resp.is_empty() {
                Ok(default.to_string())
            } else {
                Ok(resp.clone())
            }
        }
    }

    #[test]
    fn test_configure_skills_enabled_with_defaults() {
        let mut config = Config::default();
        // "y" for enable, "y" for skillverse, "n" for personal
        let mut prompter = TestPrompter::new(vec!["y", "y", "n"]);
        configure_skills(&mut config, &mut prompter).unwrap();
        assert!(config.features.contains(&Feature::Skills));
        assert_eq!(
            config.skills.community_registry.as_deref(),
            Some(DEFAULT_COMMUNITY_REGISTRY)
        );
    }

    #[test]
    fn test_configure_skills_disabled() {
        let mut config = Config::default();
        config.features.insert(Feature::Skills);
        let mut prompter = TestPrompter::new(vec!["n"]);
        configure_skills(&mut config, &mut prompter).unwrap();
        assert!(!config.features.contains(&Feature::Skills));
    }

    #[test]
    fn test_configure_artifacts_no_remote_disables() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );
        let mut config = Config::default();
        // "y" to enable, "" for remote (empty = required)
        let mut prompter = TestPrompter::new(vec!["y", ""]);
        configure_artifacts(&mut config, &paths, &mut prompter).unwrap();
        assert!(!config.features.contains(&Feature::Artifacts));
    }

    #[test]
    fn test_configure_artifacts_with_remote_enables() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );
        let mut config = Config::default();
        // "y" to enable, remote url, default dir, "y" for auto-push
        let mut prompter =
            TestPrompter::new(vec!["y", "git@example.com:user/artifacts.git", "", "y"]);
        configure_artifacts(&mut config, &paths, &mut prompter).unwrap();
        assert!(config.features.contains(&Feature::Artifacts));
        assert_eq!(
            config.artifacts.remote.as_deref(),
            Some("git@example.com:user/artifacts.git")
        );
    }

    #[test]
    fn test_configure_instructions_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );
        // Create .akm dir for global instructions
        std::fs::create_dir_all(dir.path().join(".akm")).unwrap();
        let mut config = Config::default();
        let mut prompter = TestPrompter::new(vec!["y"]);
        configure_instructions(&mut config, &paths, &mut prompter).unwrap();
        assert!(config.features.contains(&Feature::Instructions));
    }

    #[test]
    fn test_configure_instructions_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );
        let mut config = Config::default();
        config.features.insert(Feature::Instructions);
        let mut prompter = TestPrompter::new(vec!["n"]);
        configure_instructions(&mut config, &paths, &mut prompter).unwrap();
        assert!(!config.features.contains(&Feature::Instructions));
    }
}
