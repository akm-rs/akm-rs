//! `akm skills clean` — remove stale specs from global dirs or project.
//!
//! Bash: `cmd_skills_clean()` at bin/akm:1645–1765.
//!
//! Two modes:
//! - **Global** (default): removes non-symlink entries from global tool dirs.
//! - **Project** (`--project`): removes non-symlink copies from .claude/ dirs
//!   in the current project that exist in the library.

use crate::error::{Error, Result};
use crate::git::Git;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;

/// Spec subdirectory names.
const SPEC_SUBDIRS: &[&str] = &["skills", "agents"];

/// Run the `akm skills clean` command.
pub fn run(paths: &Paths, tool_dirs: &ToolDirs, project: bool, dry_run: bool) -> Result<()> {
    if project {
        clean_project(paths, dry_run)
    } else {
        clean_global(tool_dirs, dry_run)
    }
}

/// Clean non-symlink entries from global tool directories.
///
/// Bash: `_clean_global()` at bin/akm:1668–1701.
fn clean_global(tool_dirs: &ToolDirs, dry_run: bool) -> Result<()> {
    let mut removed = 0u32;

    for tool_dir in tool_dirs.dirs() {
        for subdir in SPEC_SUBDIRS {
            let dir = tool_dir.join(subdir);
            if !dir.is_dir() {
                continue;
            }

            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_symlink() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let display = format!("{}/{subdir}/{name}", tool_dir.display());
                    if dry_run {
                        println!("Would remove: {display}");
                    } else {
                        if path.is_dir() {
                            std::fs::remove_dir_all(&path).ok();
                        } else {
                            std::fs::remove_file(&path).ok();
                        }
                        println!("✓ Removed: {display}");
                    }
                    removed += 1;
                }
            }
        }
    }

    if removed == 0 {
        println!("Nothing to clean — all entries are symlinks");
    } else if dry_run {
        println!("\n{removed} items would be removed. Run without --dry-run to proceed.");
    } else {
        println!("\n✓ Cleaned {removed} non-symlink entries from global dirs");
    }

    Ok(())
}

/// Clean non-symlink copies from the current project.
///
/// Bash: `_clean_project()` at bin/akm:1704–1765.
fn clean_project(paths: &Paths, dry_run: bool) -> Result<()> {
    let library = Library::load_checked(paths)?;
    let project_root = Git::toplevel(None).map_err(|_| Error::ManifestNoProject)?;

    let mut removed = 0u32;
    let mut unknown: Vec<String> = Vec::new();

    for subdir in SPEC_SUBDIRS {
        let dir = project_root.join(".claude").join(subdir);
        if !dir.is_dir() {
            continue;
        }

        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_symlink() {
                continue; // skip symlinks (core specs)
            }

            let name = entry.file_name().to_string_lossy().to_string();
            // Bash: strip .md for agents to get ID
            let id = if *subdir == "agents" {
                name.strip_suffix(".md").unwrap_or(&name).to_string()
            } else {
                name.clone()
            };

            if library.contains(&id) {
                if dry_run {
                    println!("Would remove: .claude/{subdir}/{name}");
                } else {
                    if path.is_dir() {
                        std::fs::remove_dir_all(&path).ok();
                    } else {
                        std::fs::remove_file(&path).ok();
                    }
                    println!("  ✓ Removed .claude/{subdir}/{name}");
                }
                removed += 1;
            } else {
                unknown.push(format!(".claude/{subdir}/{name}"));
            }
        }
    }

    if removed == 0 && unknown.is_empty() {
        println!("Nothing to clean in project");
    } else if dry_run && removed > 0 {
        println!("\n{removed} items would be removed. Run without --dry-run to proceed.");
    } else if removed > 0 {
        println!("\n✓ Cleaned {removed} non-symlink entries from project");
    }

    if !unknown.is_empty() {
        println!();
        println!("Not in library (kept):");
        for u in &unknown {
            println!("  {u}  — publish or remove manually");
        }
    }

    Ok(())
}
