//! `akm skills unload` — remove spec(s) from active session.
//!
//! Bash: `cmd_skills_unload()` at bin/akm:1807–1828.
//!
//! Idempotency: Unloading an already-unloaded spec warns but succeeds.

use crate::error::Result;
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::paths::Paths;

use super::load::resolve_session;

/// Run the `akm skills unload` command.
pub fn run(_paths: &Paths, ids: &[String], tool_dirs: &ToolDirs) -> Result<()> {
    let staging = resolve_session()?;

    for id in ids {
        match symlinks::remove_session(id, &staging, tool_dirs.dirs()) {
            Ok(true) => {
                println!("Unloaded: {id}");
            }
            Ok(false) => {
                eprintln!("Not loaded: {id}");
            }
            Err(e) => {
                eprintln!("Failed to unload: {id}: {e}");
            }
        }
    }

    Ok(())
}
