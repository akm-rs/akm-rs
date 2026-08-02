//! `akm disable` — restore vanilla harness entrypoints without removing anything.
//!
//! Creates the `disabled` sentinel (checked by akm-init.sh at source time,
//! so new shells skip the claude/copilot/opencode wrappers) and clears the
//! global core-spec symlinks from tool directories. Config, library, and
//! manifests are untouched. Reversed by `akm enable`.

use crate::error::{IoContext, Result};
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::paths::Paths;

/// Run the `akm disable` command. Idempotent.
pub fn run(paths: &Paths, tool_dirs: &ToolDirs) -> Result<()> {
    let sentinel = paths.disabled_sentinel();
    let already = sentinel.is_file();

    std::fs::create_dir_all(paths.config_dir()).io_context(format!(
        "Creating config directory {}",
        paths.config_dir().display()
    ))?;
    std::fs::write(&sentinel, "").io_context(format!("Writing sentinel {}", sentinel.display()))?;

    let cleared = symlinks::clear_all(tool_dirs.dirs())?;

    if already {
        println!("akm was already disabled.");
    } else {
        println!("akm disabled.");
    }
    println!("  - New shells get vanilla claude/copilot/opencode (no wrappers)");
    println!("  - Removed {cleared} global spec symlink(s) from tool directories");
    println!();
    println!("This shell still has the wrappers. To drop them now:");
    println!("  unset -f claude copilot opencode");
    println!();
    println!("Instruction files distributed to tool dirs (e.g. ~/.claude/CLAUDE.md)");
    println!("are left in place. Re-enable with: akm enable");

    Ok(())
}
