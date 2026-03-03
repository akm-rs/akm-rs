//! `akm skills load` — load spec(s) into active session (JIT / Layer 3).
//!
//! Bash: `cmd_skills_load()` at bin/akm:1767–1805.
//!
//! Idempotency: Loading an already-loaded spec is safe (symlinks are
//! force-created with ln -sfn semantics).

use crate::error::{Error, Result};
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use std::env;
use std::path::PathBuf;

/// Resolve and validate the AKM_SESSION directory.
///
/// Bash: `_check_session()` at bin/akm:118–128.
/// Returns the path if valid, errors if unset or directory missing.
pub fn resolve_session() -> Result<PathBuf> {
    let session = env::var("AKM_SESSION").map_err(|_| Error::NoActiveSession)?;

    if session.is_empty() {
        return Err(Error::NoActiveSession);
    }

    let path = PathBuf::from(&session);
    if !path.is_dir() {
        return Err(Error::SessionDirNotFound { path });
    }

    Ok(path)
}

/// Run the `akm skills load` command.
pub fn run(paths: &Paths, ids: &[String], tool_dirs: &ToolDirs) -> Result<()> {
    let library = Library::load_checked(paths)?;
    let staging = resolve_session()?;

    let mut failures = 0u32;

    for id in ids {
        let spec = match library.get(id) {
            Some(s) => s,
            None => {
                eprintln!("Not found: {id}");
                failures += 1;
                continue;
            }
        };

        match symlinks::create_session(spec, paths.data_dir(), &staging, tool_dirs.dirs()) {
            Ok(true) => {
                println!("Loaded: {id} ({})", spec.spec_type);
            }
            Ok(false) => {
                eprintln!("Failed to load: {id} (source not found on disk)");
                failures += 1;
            }
            Err(e) => {
                eprintln!("Failed to load: {id}: {e}");
                failures += 1;
            }
        }
    }

    if failures > 0 {
        return Err(Error::Io {
            context: format!("{failures} spec(s) failed to load"),
            source: std::io::Error::other("partial failure"),
        });
    }

    Ok(())
}
