//! AKM — Agent Kit Manager
//!
//! A CLI tool for managing reusable LLM skills, artifacts, and global
//! instructions across coding assistants.

use akm::commands;
use akm::completions::dynamic::SpecIdCompleter;
use akm::completions::Shell;
use akm::config;
use akm::error;
use akm::paths;
use akm::update;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::engine::ArgValueCandidates;
use std::process::ExitCode;

/// AKM — Agent Kit Manager
///
/// Manage reusable LLM skills, artifacts, and global instructions
/// across coding assistants (Claude Code, GitHub Copilot CLI, OpenCode, and more).
#[derive(Parser, Debug)]
#[command(name = "akm", version, about, long_about = None)]
#[command(propagate_version = true)]
#[command(after_help = "\
Examples:
  akm setup                      # interactive feature configuration
  akm sync                       # sync all enabled features
  akm skills add vitest tdd      # add specs to project manifest
  akm skills load debugging      # load spec into active session
  akm skills list --type skill   # list all skills
  akm artifacts sync             # sync artifacts repo
  akm config artifacts.auto-push false")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

/// Top-level commands.
///
/// Matches the Bash `main()` case statement at bin/akm:2592.
#[derive(Subcommand, Debug)]
enum Commands {
    /// Interactive feature configuration
    Setup {
        /// Configure skills only
        #[arg(long)]
        skills: bool,
        /// Configure artifacts only
        #[arg(long)]
        artifacts: bool,
        /// Configure instructions only
        #[arg(long)]
        instructions: bool,
    },
    /// View, get, or set configuration values
    Config {
        /// Config key (e.g. artifacts.auto-push, features)
        key: Option<String>,
        /// New value to set
        value: Option<String>,
    },
    /// Sync all enabled domains
    Sync,
    /// Pull latest and re-install
    Update,
    /// Skills management
    Skills {
        #[command(subcommand)]
        command: Option<SkillsCommands>,
    },
    /// Artifact sync
    Artifacts {
        #[command(subcommand)]
        command: commands::artifacts::ArtifactsCommands,
    },
    /// Global instruction management
    Instructions {
        #[command(subcommand)]
        command: InstructionsCommands,
    },
    /// Generate shell completion script
    ///
    /// Outputs a completion registration script for the specified shell.
    /// Source the output to enable Tab completions for akm.
    ///
    /// For automatic installation, use `akm setup` instead.
    ///
    /// Examples:
    ///   eval "$(akm completions bash)"
    ///   akm completions zsh >> ~/.zshrc
    ///   akm completions fish > ~/.config/fish/completions/akm.fish
    Completions {
        /// Target shell
        #[arg(value_enum)]
        shell: Shell,
    },
}

/// Skills subcommands.
///
/// Matches `cmd_skills()` case statement at bin/akm:403.
#[derive(Subcommand, Debug)]
enum SkillsCommands {
    /// Pull remote → cold library → rebuild symlinks
    Sync {
        /// Suppress output
        #[arg(long)]
        quiet: bool,
    },
    /// Add spec(s) to project manifest
    Add {
        /// Spec IDs to add
        #[arg(required = true, add = ArgValueCandidates::new(SpecIdCompleter))]
        ids: Vec<String>,
    },
    /// Remove spec(s) from project manifest
    Remove {
        /// Spec IDs to remove
        #[arg(required = true, add = ArgValueCandidates::new(SpecIdCompleter))]
        ids: Vec<String>,
    },
    /// Load spec(s) into active session (JIT)
    Load {
        /// Spec IDs to load
        #[arg(required = true, add = ArgValueCandidates::new(SpecIdCompleter))]
        ids: Vec<String>,
    },
    /// Remove spec(s) from active session
    Unload {
        /// Spec IDs to unload
        #[arg(required = true, add = ArgValueCandidates::new(SpecIdCompleter))]
        ids: Vec<String>,
    },
    /// Show specs in active session
    Loaded,
    /// Browse library
    List {
        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
        /// Filter by type (skill or agent)
        #[arg(long, value_name = "TYPE")]
        r#type: Option<String>,
        /// Plain output (no TUI)
        #[arg(long)]
        plain: bool,
    },
    /// Search library by keyword
    Search {
        /// Search query
        query: String,
        /// Plain output (no TUI)
        #[arg(long)]
        plain: bool,
    },
    /// Full status overview
    Status {
        /// Plain output (no TUI)
        #[arg(long)]
        plain: bool,
    },
    /// Remove stale specs
    Clean {
        /// Clean project directories
        #[arg(long)]
        project: bool,
        /// Preview changes without applying
        #[arg(long)]
        dry_run: bool,
    },
    /// Import local skill into cold storage
    Promote {
        /// Path to skill directory
        path: String,
        /// Overwrite without confirmation
        #[arg(long)]
        force: bool,
    },
    /// Edit spec metadata in $EDITOR
    Edit {
        /// Spec ID to edit
        #[arg(add = ArgValueCandidates::new(SpecIdCompleter))]
        id: String,
    },
    /// Publish spec to personal registry
    Publish {
        /// Spec ID to publish
        #[arg(add = ArgValueCandidates::new(SpecIdCompleter))]
        id: String,
        /// Preview changes without applying
        #[arg(long)]
        dry_run: bool,
    },
    /// Regenerate library.json from disk
    Libgen,
    /// [hidden] Set up session staging from project manifest (used by shell init)
    #[command(hide = true, name = "session-setup")]
    SessionSetup {
        /// Path to the staging directory
        staging_dir: String,
        /// Path to the project root
        project_root: String,
    },
}

/// Instructions subcommands.
#[derive(Subcommand, Debug)]
enum InstructionsCommands {
    /// Distribute global instructions to tool directories
    Sync,
    /// Create AGENTS.md + CLAUDE.md in project root
    ScaffoldProject,
    /// Edit global instructions in $EDITOR
    Edit,
}

fn main() -> ExitCode {
    // Handle shell completion requests (Tab press) before parsing.
    // CompleteEnv detects the COMPLETE env var, generates candidates, and exits.
    // Normal invocations pass through as a no-op.
    clap_complete::CompleteEnv::with_factory(Cli::command).complete();

    let cli = Cli::parse();

    let paths = match paths::Paths::resolve() {
        Some(p) => p,
        None => {
            eprintln!("Error: Could not determine home directory");
            return ExitCode::FAILURE;
        }
    };

    // Spawn background version check
    let update_rx = {
        match config::Config::load(&paths) {
            Ok(cfg) => update::version_check::spawn_background_check(&cfg.update, &paths),
            Err(_) => {
                let (tx, rx) = std::sync::mpsc::channel();
                let _ = tx.send(update::version_check::CheckResult::Skipped);
                rx
            }
        }
    };

    // Track whether user ran `akm update` to suppress the post-command notice
    let is_update_command = matches!(&cli.command, Some(Commands::Update));

    let result = match cli.command {
        None => {
            // Default: show help (matches Bash `main()` default of "help")
            use clap::CommandFactory;
            Cli::command()
                .print_help()
                .map(|()| {
                    println!(); // trailing newline after help
                })
                .map_err(|e| error::Error::Io {
                    context: "printing help".into(),
                    source: e,
                })
        }
        Some(Commands::Config { key, value }) => commands::config::run(&paths, key, value),
        Some(Commands::Setup {
            skills,
            artifacts,
            instructions,
        }) => {
            let scope = if skills || artifacts || instructions {
                commands::setup::SetupScope {
                    skills,
                    artifacts,
                    instructions,
                }
            } else {
                commands::setup::SetupScope::all()
            };
            let mut prompter = commands::setup::StdinPrompter;
            commands::setup::run(&paths, scope, &mut prompter)
        }
        Some(Commands::Sync) => commands::sync::run(&paths),
        Some(Commands::Update) => match config::Config::load(&paths) {
            Ok(cfg) => commands::update::run(&paths, &cfg),
            Err(e) => Err(e),
        },
        Some(Commands::Skills { command }) => {
            let tool_dirs = akm::library::tool_dirs::ToolDirs::load(&paths);

            match command {
                Some(SkillsCommands::Libgen) => commands::skills::libgen::run(&paths),
                Some(SkillsCommands::Sync { quiet }) => {
                    commands::skills::sync::run_cli(&paths, quiet)
                }
                Some(SkillsCommands::Add { ids }) => {
                    commands::skills::add::run(&paths, &ids, &tool_dirs)
                }
                Some(SkillsCommands::Remove { ids }) => {
                    commands::skills::remove::run(&paths, &ids, &tool_dirs)
                }
                Some(SkillsCommands::List { tag, r#type, plain }) => commands::skills::list::run(
                    &paths,
                    tag.as_deref(),
                    r#type.as_deref(),
                    plain,
                    &tool_dirs,
                ),
                Some(SkillsCommands::Search { query, plain }) => {
                    commands::skills::search::run(&paths, &query, plain, &tool_dirs)
                }
                Some(SkillsCommands::Status { plain }) => {
                    commands::skills::status::run(&paths, &tool_dirs, plain)
                }
                Some(SkillsCommands::Load { ids }) => {
                    commands::skills::load::run(&paths, &ids, &tool_dirs)
                }
                Some(SkillsCommands::Unload { ids }) => {
                    commands::skills::unload::run(&paths, &ids, &tool_dirs)
                }
                Some(SkillsCommands::Loaded) => commands::skills::loaded::run(&paths, &tool_dirs),
                Some(SkillsCommands::Clean { project, dry_run }) => {
                    commands::skills::clean::run(&paths, &tool_dirs, project, dry_run)
                }
                Some(SkillsCommands::Promote { path, force }) => {
                    commands::skills::promote::run(&paths, &path, force, &tool_dirs)
                }
                Some(SkillsCommands::Edit { id }) => {
                    commands::skills::edit::run(&paths, &id, &tool_dirs)
                }
                Some(SkillsCommands::Publish { id, dry_run }) => {
                    let config = akm::config::Config::load(&paths).unwrap_or_default();
                    commands::skills::publish::run(&paths, &config, &id, dry_run)
                }
                Some(SkillsCommands::SessionSetup {
                    staging_dir,
                    project_root,
                }) => commands::skills::session_setup::run(&paths, &staging_dir, &project_root),
                None => {
                    // Default: `akm skills` with no subcommand → show status
                    // Bash: `local subcommand="${1:-status}"` at bin/akm:404
                    commands::skills::status::run(&paths, &tool_dirs, false)
                }
            }
        }
        Some(Commands::Artifacts { command }) => {
            let config = akm::config::Config::load(&paths).unwrap_or_default();
            match command {
                commands::artifacts::ArtifactsCommands::Sync => {
                    commands::artifacts::sync::run(&config, &paths)
                }
            }
        }
        Some(Commands::Instructions { command }) => match command {
            InstructionsCommands::Sync => commands::instructions::sync::run(&paths),
            InstructionsCommands::Edit => commands::instructions::edit::run(&paths),
            InstructionsCommands::ScaffoldProject => commands::instructions::scaffold::run(),
        },
        Some(Commands::Completions { shell }) => commands::completions::run(&shell),
    };

    // Print update notice after command output (unless user ran `akm update`)
    if !is_update_command {
        update::version_check::print_update_notice(update_rx);
    }

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}
