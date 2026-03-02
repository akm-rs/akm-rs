//! AKM — Agent Kit Manager
//!
//! A CLI tool for managing reusable LLM skills, artifacts, and global
//! instructions across coding assistants.

use akm::commands;
use akm::error;
use akm::paths;

use clap::{Parser, Subcommand};
use std::process::ExitCode;

/// AKM — Agent Kit Manager
///
/// Manage reusable LLM skills, artifacts, and global instructions
/// across coding assistants (Claude Code, GitHub Copilot CLI, OpenCode, and more).
#[derive(Parser, Debug)]
#[command(name = "akm", version, about, long_about = None)]
#[command(propagate_version = true)]
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
    Setup,
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
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Remove spec(s) from project manifest
    Remove {
        /// Spec IDs to remove
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Load spec(s) into active session (JIT)
    Load {
        /// Spec IDs to load
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Remove spec(s) from active session
    Unload {
        /// Spec IDs to unload
        #[arg(required = true)]
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
        id: String,
    },
    /// Publish spec to personal registry
    Publish {
        /// Spec ID to publish
        id: String,
        /// Preview changes without applying
        #[arg(long)]
        dry_run: bool,
    },
    /// Regenerate library.json from disk
    Libgen,
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
    let cli = Cli::parse();

    let paths = match paths::Paths::resolve() {
        Some(p) => p,
        None => {
            eprintln!("Error: Could not determine home directory");
            return ExitCode::FAILURE;
        }
    };

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
        Some(Commands::Setup) => {
            eprintln!("Not yet implemented: setup");
            Ok(())
        }
        Some(Commands::Sync) => {
            eprintln!("Not yet implemented: sync");
            Ok(())
        }
        Some(Commands::Update) => {
            eprintln!("Not yet implemented: update");
            Ok(())
        }
        Some(Commands::Skills { command }) => match command {
            Some(SkillsCommands::Libgen) => commands::skills::libgen::run(&paths),
            Some(_) => {
                eprintln!("Not yet implemented");
                Ok(())
            }
            None => (|| -> error::Result<()> {
                use clap::CommandFactory;
                let mut cmd = Cli::command();
                for sub in cmd.get_subcommands_mut() {
                    if sub.get_name() == "skills" {
                        sub.print_help().map_err(|e| error::Error::Io {
                            context: "printing skills help".into(),
                            source: e,
                        })?;
                        println!();
                        break;
                    }
                }
                Ok(())
            })(),
        },
        Some(Commands::Artifacts { command }) => {
            let config = akm::config::Config::load(&paths).unwrap_or_default();
            match command {
                commands::artifacts::ArtifactsCommands::Sync => {
                    commands::artifacts::sync::run(&config, &paths)
                }
            }
        }
        Some(Commands::Instructions { command }) => {
            let _ = command;
            eprintln!("Not yet implemented: instructions");
            Ok(())
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}
