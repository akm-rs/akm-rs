//! `akm skills list` — browse library specs.
//!
//! In Task 4 this was plain-only. Task 9 adds TUI as the default mode
//! when stdout is a TTY and --plain is not passed.

use crate::config::Config;
use crate::error::Result;
use crate::library::spec::SpecType;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use std::io::IsTerminal;

use super::shared;

/// Determine whether to use TUI or plain output.
///
/// TUI is used when:
/// 1. `--plain` is NOT passed
/// 2. stdout is a TTY
///
/// This is the shared decision function used by list, search, and status.
pub fn should_use_tui(plain_flag: bool) -> bool {
    !plain_flag && std::io::stdout().is_terminal()
}

/// Run the `akm skills list` command.
///
/// Parses `--type` filter before the TUI/plain split so both paths get
/// identical validation (returns Error::InvalidSpecType for bad values).
pub fn run(
    paths: &Paths,
    tag: Option<&str>,
    type_filter: Option<&str>,
    plain: bool,
    tool_dirs: &ToolDirs,
) -> Result<()> {
    let parsed_type: Option<SpecType> = type_filter.map(|t| t.parse::<SpecType>()).transpose()?;

    if should_use_tui(plain) {
        crate::tui::list::run(paths, tag, parsed_type, None, tool_dirs)
    } else {
        run_plain(paths, tag, parsed_type)
    }
}

/// Run `akm skills list <remote>` — browse a shared registry.
///
/// Plain output only: there is nothing to drive a TUI with here. A shared
/// registry has no drift, no core state and no project manifest — the only
/// question it answers is what it holds and whether you already have it.
pub fn run_shared(paths: &Paths, config: &Config, remote: &str) -> Result<()> {
    let registry = shared::open(paths, config, remote)?;
    let outcome = shared::refresh_for_use(&registry)?;
    println!("{remote} ({}) — {outcome}", registry.url());
    println!();

    let catalogue = shared::catalogue(registry.dir())?;
    let library = Library::load_or_default(&paths.library_json())?;
    let library_dir = paths.library_dir();

    for candidate in &catalogue.skills {
        let mark = match library.get(&candidate.id) {
            Some(spec) if spec.exists_on_disk(&library_dir) => "  [have]",
            _ => "",
        };
        println!(
            "  {:<35} {}{mark}",
            candidate.id, candidate.meta.description
        );
    }

    if catalogue.skills.is_empty() {
        println!("  (no importable skills)");
    }

    for entry in &catalogue.skipped {
        println!("  {:<35} (skipped: {})", entry.id, entry.reason);
    }

    println!();
    println!("Import one with 'akm skills import {remote} <id>', or all of them with --all.");

    Ok(())
}

/// Plain output mode — identical to Task 4 implementation.
fn run_plain(paths: &Paths, tag: Option<&str>, parsed_type: Option<SpecType>) -> Result<()> {
    let library = Library::load_checked(paths)?;

    for spec in &library.specs {
        if let Some(ft) = parsed_type {
            if spec.spec_type != ft {
                continue;
            }
        }
        if let Some(tag) = tag {
            if !spec.tags.iter().any(|t| t == tag) {
                continue;
            }
        }
        let type_label = format!("{:<6}", spec.spec_type);
        if spec.core {
            println!(
                "  {type_label}  {:<35} {} [CORE]",
                spec.id, spec.description
            );
        } else {
            println!("  {type_label}  {:<35} {}", spec.id, spec.description);
        }
    }

    Ok(())
}
