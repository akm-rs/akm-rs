//! `akm artifacts` (no subcommand) — browse the per-project artifacts tree.
//!
//! TTY + not `--plain` → the two-pane explorer. Non-TTY or `--plain` → the
//! plain box-drawing tree (the agent/scripting read path).

use crate::artifacts::render_tree;
use crate::commands::skills::list::should_use_tui;
use crate::config::Config;
use crate::error::Result;
use crate::paths::Paths;

pub fn run(paths: &Paths, config: &Config, plain: bool) -> Result<()> {
    let dir = config.artifacts_dir(paths);
    if !dir.is_dir() {
        println!("No artifacts yet ({} does not exist).", dir.display());
        return Ok(());
    }
    if should_use_tui(plain) {
        crate::tui::artifacts::run(paths, config)
    } else {
        print!("{}", render_tree(&dir)?);
        Ok(())
    }
}
