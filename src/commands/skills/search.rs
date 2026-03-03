//! `akm skills search` — keyword search across library specs.
//!
//! Bash: `cmd_skills_search()` at bin/akm:1250–1277.
//! Task 9 wraps with TUI (pre-populated filter bar).

use crate::error::Result;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;

use super::list::should_use_tui;

/// Run the `akm skills search` command.
pub fn run(paths: &Paths, query: &str, plain: bool, tool_dirs: &ToolDirs) -> Result<()> {
    if should_use_tui(plain) {
        crate::tui::list::run(paths, None, None, Some(query), tool_dirs)
    } else {
        run_plain(paths, query)
    }
}

/// Plain output mode — identical to Task 4 implementation.
fn run_plain(paths: &Paths, query: &str) -> Result<()> {
    let library = Library::load_checked(paths)?;
    let lquery = query.to_lowercase();

    for spec in &library.specs {
        let tags_str = spec.tags.join(",");
        let searchable = format!("{} {} {}", spec.id, spec.description, tags_str).to_lowercase();

        if searchable.contains(&lquery) {
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
    }
    Ok(())
}
