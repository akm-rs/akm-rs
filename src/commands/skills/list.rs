//! `akm skills list` — browse library specs.
//!
//! Bash: `cmd_skills_list()` at bin/akm:1215–1248.
//!
//! In Task 4 this was plain-only. Task 9 adds TUI as the default mode
//! when stdout is a TTY and --plain is not passed.

use crate::error::Result;
use crate::library::spec::SpecType;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use std::io::IsTerminal;

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
