//! `akm uninstall` — remove akm from the machine.
//!
//! Default mode preserves user content: artifacts (`~/.akm/artifacts`),
//! the legacy global instructions file (`~/.akm/global-instructions.md`),
//! and the library (`$XDG_DATA_HOME/akm/library/` — the checkout of the
//! personal registry, which may hold unpublished local edits). `--purge`
//! removes everything.
//!
//! Instruction files distributed into tool dirs (e.g. `~/.claude/CLAUDE.md`)
//! are never touched — they may contain user edits that drifted from the
//! library copy, so deleting them could destroy user content.
//!
//! The binary itself is removed last, so a failure partway through leaves
//! a working `akm` to re-run (all steps tolerate already-missing targets).

use crate::commands::setup::Prompter;
use crate::error::{IoContext, Result};
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::paths::Paths;
use crate::shell;
use std::path::Path;

/// Run the `akm uninstall` command.
///
/// Prompts for confirmation unless `yes` is set (Enter defaults to no).
pub fn run(
    paths: &Paths,
    tool_dirs: &ToolDirs,
    purge: bool,
    yes: bool,
    prompter: &mut dyn Prompter,
) -> Result<()> {
    print_plan(paths, purge);

    if !yes {
        let confirmed = prompter.confirm("Proceed with uninstall?", false)?;
        if !confirmed {
            println!("Aborted.");
            return Ok(());
        }
    }

    remove_files(paths, tool_dirs, purge)?;
    remove_binary()?;

    println!();
    println!("akm uninstalled.");
    if !purge {
        println!("Preserved:");
        println!("  {}", paths.default_artifacts_dir().display());
        println!("  {}", paths.legacy_global_instructions().display());
        println!("  {}", paths.library_dir().display());
    }
    println!("Restart your shell to drop the claude/copilot/opencode/pi wrappers.");
    Ok(())
}

/// Print what will be removed and what will be preserved.
fn print_plan(paths: &Paths, purge: bool) {
    println!("This will remove:");
    println!("  - this binary ({})", current_exe_display());
    println!("  - the akm block in ~/.bashrc");
    println!("  - global spec symlinks in tool directories");
    println!("  - config: {}", paths.config_dir().display());
    println!("  - cache:  {}", paths.cache_dir().display());
    if purge {
        println!(
            "  - data:   {} (including the library checkout — any",
            paths.data_dir().display()
        );
        println!("            unpublished local changes are lost)");
        println!("  - {} (including artifacts)", paths.akm_home().display());
    } else {
        println!(
            "  - data:   {} (library.json, local.json, tools.json, shell/)",
            paths.data_dir().display()
        );
        println!();
        println!("Preserved:");
        println!("  - artifacts: {}", paths.default_artifacts_dir().display());
        println!(
            "  - legacy global instructions: {}",
            paths.legacy_global_instructions().display()
        );
        println!(
            "  - library (registry checkout): {}",
            paths.library_dir().display()
        );
    }
    println!();
    println!("Instruction files distributed to tool dirs (e.g. ~/.claude/CLAUDE.md)");
    println!("are left in place — they may contain your own edits.");
    println!();
}

/// Remove akm's files, honoring the preserve set unless `purge`.
///
/// Separated from [`run`] so tests can exercise it against temp dirs
/// without prompting or deleting the test binary.
pub fn remove_files(paths: &Paths, tool_dirs: &ToolDirs, purge: bool) -> Result<()> {
    // 1. Shell integration block
    if shell::unpatch_bashrc(paths)? {
        println!("Removed akm block from ~/.bashrc");
    }

    // 2. Global spec symlinks (before tools.json goes away)
    let cleared = symlinks::clear_all(tool_dirs.dirs())?;
    if cleared > 0 {
        println!("Removed {cleared} global spec symlink(s)");
    }

    // 3. Config dir (config.toml, disabled sentinel)
    remove_dir_if_exists(paths.config_dir())?;

    // 4. Cache dir (registry caches, staging, update-check cache)
    remove_dir_if_exists(paths.cache_dir())?;

    // 5. Data dir: everything, or everything except the library checkout
    if purge {
        remove_dir_if_exists(paths.data_dir())?;
        remove_dir_if_exists(paths.akm_home())?;
    } else {
        remove_file_if_exists(&paths.library_json())?;
        remove_file_if_exists(&paths.local_json())?;
        remove_file_if_exists(&paths.tools_json())?;
        remove_dir_if_exists(&paths.data_dir().join("shell"))?;
    }

    Ok(())
}

/// Delete the running executable. Last step — see module doc.
fn remove_binary() -> Result<()> {
    let exe = std::env::current_exe().io_context("Resolving current executable path")?;
    std::fs::remove_file(&exe).io_context(format!("Removing binary {}", exe.display()))?;
    println!("Removed binary {}", exe.display());
    Ok(())
}

fn current_exe_display() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn remove_dir_if_exists(dir: &Path) -> Result<()> {
    if dir.is_dir() {
        std::fs::remove_dir_all(dir).io_context(format!("Removing directory {}", dir.display()))?;
        println!("Removed {}", dir.display());
    }
    Ok(())
}

fn remove_file_if_exists(file: &Path) -> Result<()> {
    if file.is_file() {
        std::fs::remove_file(file).io_context(format!("Removing {}", file.display()))?;
        println!("Removed {}", file.display());
    }
    Ok(())
}
