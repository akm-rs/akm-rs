//! `akm enable` — re-activate shell integration after `akm disable`.
//!
//! Removes the `disabled` sentinel and rebuilds global core-spec symlinks
//! from the library (skipped gracefully if the library doesn't exist yet).

use crate::error::{IoContext, Result};
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;

/// Run the `akm enable` command. Idempotent.
pub fn run(paths: &Paths, tool_dirs: &ToolDirs) -> Result<()> {
    let sentinel = paths.disabled_sentinel();
    if sentinel.is_file() {
        std::fs::remove_file(&sentinel)
            .io_context(format!("Removing sentinel {}", sentinel.display()))?;
        println!("akm enabled.");
    } else {
        println!("akm was not disabled.");
    }

    let rebuilt = match Library::load(paths) {
        Ok(library) => {
            let core_specs = library.core_specs();
            symlinks::rebuild_core(&core_specs, paths.data_dir(), tool_dirs.dirs())?
        }
        Err(_) => 0, // No library yet — nothing to symlink
    };
    println!("  - Restored {rebuilt} global core symlink(s)");
    println!();
    println!("Start a new shell to restore the claude/copilot/opencode wrappers.");

    Ok(())
}
