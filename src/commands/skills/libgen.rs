//! `akm skills libgen` — regenerate library.json from disk.

use crate::error::Result;
use crate::library::libgen;
use crate::paths::Paths;

/// Run the `akm skills libgen` command.
///
/// Algorithm:
/// 1. Locate the directory to scan, and where its index belongs:
///    a. If in a git repo with `skills/` → scan the repo root, write the index
///    beside its specs
///    b. Otherwise → scan the cold library (`Paths::library_dir()`) and write
///    the machine-local index (`Paths::library_json()`)
/// 2. Call `libgen::generate(scan_dir, out_path)`
/// 3. Report count
pub fn run(paths: &Paths) -> Result<()> {
    let (scan_dir, out_path) = resolve_target(paths)?;

    let result = libgen::generate(&scan_dir, &out_path)?;

    println!("Library regenerated ({} specs)", result.count);
    println!("  Specs on disk: {}", result.count);

    Ok(())
}

/// Resolve what to scan and where to write the index.
///
/// 1. Try the current git repo root (if it contains skills/ or agents/)
/// 2. Fall back to the cold library directory
///
/// Only the cold library's index is machine-local. Asked to run against some
/// other repository of specs, libgen writes the index into that repository —
/// the caller is looking at that directory, not at this machine's install.
fn resolve_target(paths: &Paths) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    let library_dir = paths.library_dir();

    if let Ok(repo_root) = crate::git::Git::toplevel(None) {
        if repo_root.join("skills").is_dir() || repo_root.join("agents").is_dir() {
            // Standing inside the cold library is still the cold library: its
            // index stays machine-local wherever the command was run from.
            if repo_root == library_dir {
                return Ok((repo_root, paths.library_json()));
            }
            let out = repo_root.join("library.json");
            return Ok((repo_root, out));
        }
    }

    if library_dir.join("skills").is_dir() || library_dir.join("agents").is_dir() {
        return Ok((library_dir, paths.library_json()));
    }

    Err(crate::error::Error::NoSpecDirs { path: library_dir })
}
