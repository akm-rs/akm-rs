//! `akm skills libgen` — regenerate library.json from disk.

use crate::error::Result;
use crate::library::libgen;
use crate::paths::Paths;

/// Run the `akm skills libgen` command.
///
/// Algorithm:
/// 1. Locate target directory:
///    a. If in a git repo with `skills/` → use repo root
///    b. Otherwise → use cold library (`Paths::library_dir()`)
/// 2. Call `libgen::generate(target_dir)`
/// 3. Report count
pub fn run(paths: &Paths) -> Result<()> {
    let target_dir = resolve_target_dir(paths)?;

    let result = libgen::generate(&target_dir)?;

    println!("Library regenerated ({} specs)", result.count);
    println!("  Specs on disk: {}", result.count);

    Ok(())
}

/// Resolve the target directory for libgen.
///
/// 1. Try the current git repo root (if it contains skills/ or agents/)
/// 2. Fall back to the cold library directory
fn resolve_target_dir(paths: &Paths) -> Result<std::path::PathBuf> {
    if let Ok(repo_root) = crate::git::Git::toplevel(None) {
        if repo_root.join("skills").is_dir() || repo_root.join("agents").is_dir() {
            return Ok(repo_root);
        }
    }

    let library_dir = paths.library_dir();
    if library_dir.join("skills").is_dir() || library_dir.join("agents").is_dir() {
        return Ok(library_dir);
    }

    Err(crate::error::Error::NoSpecDirs { path: library_dir })
}
