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
  akm skills list --type skill   # list all skills
  akm skills import <github-url> # import a skill from a GitHub URL
  akm artifacts sync             # sync artifacts repo
  akm config artifacts.auto-push false")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

/// Top-level commands.
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
    ///
    /// With no arguments, prints all config values. With KEY, prints that
    /// value. With KEY and VALUE, sets it.
    ///
    /// Keys:
    ///   features                  Enabled features (comma-separated: skills,artifacts,instructions)
    ///   registry.url              Registry git URL (canonical)
    ///   skills.personal-registry  Pre-rc4 alias for registry.url
    ///   artifacts.remote          Artifacts repo git URL
    ///   artifacts.dir             Artifacts directory (default: ~/.akm/artifacts)
    ///   artifacts.auto-push       Auto-commit/push artifacts on session end (true/false)
    ///   update.url                GitHub Releases API URL for update checks
    ///   update.check-interval     Seconds between background update checks
    ///   update.auto-check         Enable background update checks (true/false)
    #[command(verbatim_doc_comment)]
    Config {
        /// Config key (e.g. artifacts.auto-push, features)
        key: Option<String>,
        /// New value to set
        value: Option<String>,
    },
    /// Sync all enabled features (skills, artifacts, instructions)
    Sync,
    /// Check for a new release and self-update the akm binary
    Update,
    /// [hidden] Rewrite akm-init.sh and tools.json from this binary
    ///
    /// Invoked by `akm update` on the *new* binary after the swap: the running
    /// process embeds the old template, so it cannot refresh the script itself.
    #[command(hide = true, name = "shell-install")]
    ShellInstall,
    /// Skills management (defaults to `status` when no subcommand is given)
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
    #[command(verbatim_doc_comment)]
    Completions {
        /// Target shell
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Turn off shell integration so harnesses run vanilla (reversible)
    ///
    /// New shells get the plain claude/copilot/opencode/pi binaries (no
    /// session wrappers) and global core-spec symlinks are removed from tool
    /// dirs. Nothing is deleted from config or the library. Undo with
    /// `akm enable`.
    ///
    /// For a one-off bypass in the current shell, `command claude` skips the
    /// wrapper without disabling anything.
    Disable,
    /// Re-enable shell integration after `akm disable`
    Enable,
    /// Remove akm from this machine
    ///
    /// Removes the binary, the ~/.bashrc integration block, global spec
    /// symlinks, config, and caches. Preserves artifacts (~/.akm/artifacts),
    /// legacy global instructions, and the library (the registry checkout,
    /// which may hold unpublished local edits) unless --purge is given.
    Uninstall {
        /// Also delete artifacts, global instructions, and the library
        #[arg(long)]
        purge: bool,
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
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
    /// Import a skill from a GitHub URL into cold storage
    Import {
        /// GitHub URL to a directory containing SKILL.md
        url: String,
        /// Overwrite without confirmation
        #[arg(long)]
        force: bool,
        /// Override the skill ID (default: last path segment from URL)
        #[arg(long)]
        id: Option<String>,
    },
    /// Edit a spec's markdown in $EDITOR
    Edit {
        /// Spec ID to edit
        #[arg(add = ArgValueCandidates::new(SpecIdCompleter))]
        id: String,
        /// Edit the spec's metadata sidecar instead of its markdown
        #[arg(long)]
        meta: bool,
    },
    /// Rename a spec's id (its slug / directory name)
    Rename {
        /// Current spec ID
        #[arg(add = ArgValueCandidates::new(SpecIdCompleter))]
        old: String,
        /// New spec ID
        new: String,
    },
    /// Delete a spec from the library (and the personal registry)
    Delete {
        /// Spec ID to delete
        #[arg(add = ArgValueCandidates::new(SpecIdCompleter))]
        id: String,
        /// Skip the confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Publish spec to personal registry
    Publish {
        /// Spec ID to publish. Omit to publish everything pending.
        #[arg(add = ArgValueCandidates::new(SpecIdCompleter))]
        id: Option<String>,
        /// Preview changes without applying
        #[arg(long)]
        dry_run: bool,
    },
    /// Show what changed locally and in the registry for a spec
    Diff {
        /// Spec ID to compare
        #[arg(add = ArgValueCandidates::new(SpecIdCompleter))]
        id: String,
    },
    /// Discard local changes to a spec
    Revert {
        /// Spec ID to revert
        #[arg(add = ArgValueCandidates::new(SpecIdCompleter))]
        id: String,
        /// Take the registry's current version instead of the last synced one
        #[arg(long)]
        remote: bool,
        /// Skip the confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Show or reconcile which specs are globally mounted
    Core {
        /// Drop local overrides and follow the registry's defaults
        #[arg(long, conflicts_with = "publish")]
        adopt: bool,
        /// Promote this machine's core choices and push them to the registry
        #[arg(long)]
        publish: bool,
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
    /// Publish global instructions to personal registry
    Publish,
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

    // Uninstall must not race the background check, whose cache write would
    // re-create ~/.cache/akm after uninstall deletes it.
    let is_uninstall_command = matches!(&cli.command, Some(Commands::Uninstall { .. }));

    // Spawn background version check
    let update_rx = {
        match config::Config::load(&paths) {
            Ok(cfg) if !is_uninstall_command => {
                update::version_check::spawn_background_check(&cfg.update, &paths)
            }
            _ => {
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
            // Default: show help
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
        Some(Commands::ShellInstall) => akm::shell::install_shell_init(&paths)
            .and_then(|()| akm::shell::install_tools_json(&paths)),
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
                Some(SkillsCommands::Clean { project, dry_run }) => {
                    commands::skills::clean::run(&paths, &tool_dirs, project, dry_run)
                }
                Some(SkillsCommands::Promote { path, force }) => {
                    let config = akm::config::Config::load(&paths).unwrap_or_default();
                    commands::skills::promote::run(&paths, &config, &path, force, &tool_dirs)
                }
                Some(SkillsCommands::Import { url, force, id }) => {
                    let config = akm::config::Config::load(&paths).unwrap_or_default();
                    commands::skills::import::run(
                        &paths,
                        &config,
                        &url,
                        force,
                        id.as_deref(),
                        &tool_dirs,
                    )
                }
                Some(SkillsCommands::Edit { id, meta }) => {
                    let config = akm::config::Config::load(&paths).unwrap_or_default();
                    commands::skills::edit::run(&paths, &config, &id, meta, &tool_dirs)
                }
                Some(SkillsCommands::Rename { old, new }) => {
                    let config = akm::config::Config::load(&paths).unwrap_or_default();
                    commands::skills::rename::run(&paths, &config, &old, &new, &tool_dirs)
                }
                Some(SkillsCommands::Delete { id, force }) => {
                    let config = akm::config::Config::load(&paths).unwrap_or_default();
                    commands::skills::delete::run(&paths, &config, &id, force, &tool_dirs)
                }
                Some(SkillsCommands::Publish { id, dry_run }) => {
                    let config = akm::config::Config::load(&paths).unwrap_or_default();
                    match id {
                        Some(id) => commands::skills::publish::run(&paths, &config, &id, dry_run),
                        None => commands::skills::publish::run_all(&paths, &config, dry_run),
                    }
                }
                Some(SkillsCommands::Diff { id }) => {
                    let config = akm::config::Config::load(&paths).unwrap_or_default();
                    commands::skills::diff::run(&paths, &config, &id)
                }
                Some(SkillsCommands::Revert { id, remote, force }) => {
                    let config = akm::config::Config::load(&paths).unwrap_or_default();
                    commands::skills::revert::run(&paths, &config, &id, remote, force, &tool_dirs)
                }
                Some(SkillsCommands::Core {
                    adopt,
                    publish,
                    dry_run,
                }) => {
                    let config = akm::config::Config::load(&paths).unwrap_or_default();
                    let action = match (adopt, publish) {
                        (true, _) => commands::skills::core::CoreAction::Adopt,
                        (_, true) => commands::skills::core::CoreAction::Publish,
                        _ => commands::skills::core::CoreAction::Show,
                    };
                    commands::skills::core::run(&paths, &config, action, &tool_dirs, dry_run)
                }
                Some(SkillsCommands::SessionSetup {
                    staging_dir,
                    project_root,
                }) => commands::skills::session_setup::run(&paths, &staging_dir, &project_root),
                None => {
                    // Default: `akm skills` with no subcommand → show status
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
            InstructionsCommands::Edit => {
                let config = akm::config::Config::load(&paths).unwrap_or_default();
                commands::instructions::edit::run(&paths, &config)
            }
            InstructionsCommands::ScaffoldProject => commands::instructions::scaffold::run(),
            InstructionsCommands::Publish => {
                let config = akm::config::Config::load(&paths).unwrap_or_default();
                commands::instructions::publish::run(&paths, &config)
            }
        },
        Some(Commands::Completions { shell }) => commands::completions::run(&shell),
        Some(Commands::Disable) => {
            let tool_dirs = akm::library::tool_dirs::ToolDirs::load(&paths);
            commands::disable::run(&paths, &tool_dirs)
        }
        Some(Commands::Enable) => {
            let tool_dirs = akm::library::tool_dirs::ToolDirs::load(&paths);
            commands::enable::run(&paths, &tool_dirs)
        }
        Some(Commands::Uninstall { purge, yes }) => {
            let tool_dirs = akm::library::tool_dirs::ToolDirs::load(&paths);
            let mut prompter = commands::setup::StdinPrompter;
            commands::uninstall::run(&paths, &tool_dirs, purge, yes, &mut prompter)
        }
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
