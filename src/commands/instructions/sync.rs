//! `akm instructions sync` — distribute global instructions to tool directories.
//!
//! Bash: `cmd_instructions_sync()` at bin/akm:549–584.
//!
//! Behavior:
//! 1. Check if global-instructions.md exists at `~/.akm/global-instructions.md`
//! 2. If not, print warning and return Ok (not an error — same as Bash)
//! 3. For each target (tool dir + filename): create dir, copy file
//! 4. Print count of distributed copies
//!
//! The Bash version also has a migration from the old XDG location. The Rust
//! version is v1.0 (no backward compat), so migration is omitted.

use crate::commands::instructions::{default_targets, InstructionsTarget};
use crate::error::{IoContext, Result};
use crate::paths::Paths;
use std::fs;
use std::path::Path;

/// Run `akm instructions sync`.
///
/// Distributes `~/.akm/global-instructions.md` to all tool directories.
///
/// # Errors
/// Returns `Err` only on filesystem failures (permission denied, disk full).
/// Missing source file is a warning, not an error (matches Bash behavior).
pub fn run(paths: &Paths) -> Result<()> {
    let source = paths.global_instructions();
    let home = paths.home();

    let targets = default_targets(home);
    sync_instructions(&source, &targets)
}

/// Core sync logic, separated for testability.
///
/// # Arguments
/// * `source` — Path to global-instructions.md
/// * `targets` — List of (dir, filename) targets
pub(crate) fn sync_instructions(source: &Path, targets: &[InstructionsTarget]) -> Result<()> {
    if !source.exists() {
        eprintln!(
            "Warning: No global instructions file found at {}",
            source.display()
        );
        eprintln!("Run 'akm instructions edit' to create one.");
        return Ok(());
    }

    let content = fs::read_to_string(source).io_context(format!(
        "Reading global instructions from {}",
        source.display()
    ))?;

    let mut count = 0u32;
    for target in targets {
        fs::create_dir_all(&target.dir)
            .io_context(format!("Creating directory {}", target.dir.display()))?;

        let dest = target.path();
        fs::write(&dest, &content)
            .io_context(format!("Writing instructions to {}", dest.display()))?;
        count += 1;
    }

    println!("Global instructions distributed to {count} tool directories");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::instructions::InstructionsTarget;
    use tempfile::TempDir;

    #[test]
    fn sync_missing_source_prints_warning_not_error() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("nonexistent.md");
        let targets = vec![InstructionsTarget {
            dir: tmp.path().join("target"),
            filename: "out.md".into(),
        }];

        // Should succeed (return Ok), not create any files
        let result = sync_instructions(&source, &targets);
        assert!(result.is_ok());
        assert!(!tmp.path().join("target").join("out.md").exists());
    }

    #[test]
    fn sync_distributes_to_all_targets() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("global-instructions.md");
        fs::write(&source, "# My Instructions\nBe helpful.").unwrap();

        let targets = vec![
            InstructionsTarget {
                dir: tmp.path().join("tool1"),
                filename: "INSTRUCTIONS.md".into(),
            },
            InstructionsTarget {
                dir: tmp.path().join("tool2"),
                filename: "instructions.md".into(),
            },
        ];

        let result = sync_instructions(&source, &targets);
        assert!(result.is_ok());

        // Both files should exist with correct content
        let content1 = fs::read_to_string(tmp.path().join("tool1/INSTRUCTIONS.md")).unwrap();
        let content2 = fs::read_to_string(tmp.path().join("tool2/instructions.md")).unwrap();
        assert_eq!(content1, "# My Instructions\nBe helpful.");
        assert_eq!(content2, "# My Instructions\nBe helpful.");
    }

    #[test]
    fn sync_creates_missing_directories() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("global-instructions.md");
        fs::write(&source, "content").unwrap();

        let targets = vec![InstructionsTarget {
            dir: tmp.path().join("deep").join("nested").join("dir"),
            filename: "out.md".into(),
        }];

        let result = sync_instructions(&source, &targets);
        assert!(result.is_ok());
        assert!(tmp.path().join("deep/nested/dir/out.md").exists());
    }

    #[test]
    fn sync_overwrites_existing_target() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("global-instructions.md");
        fs::write(&source, "new content").unwrap();

        let target_dir = tmp.path().join("tool");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("out.md"), "old content").unwrap();

        let targets = vec![InstructionsTarget {
            dir: target_dir,
            filename: "out.md".into(),
        }];

        let result = sync_instructions(&source, &targets);
        assert!(result.is_ok());

        let content = fs::read_to_string(tmp.path().join("tool/out.md")).unwrap();
        assert_eq!(content, "new content");
    }

    #[test]
    fn sync_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("global-instructions.md");
        fs::write(&source, "content").unwrap();

        let targets = vec![InstructionsTarget {
            dir: tmp.path().join("tool"),
            filename: "out.md".into(),
        }];

        // Run twice — second run should not fail
        sync_instructions(&source, &targets).unwrap();
        sync_instructions(&source, &targets).unwrap();

        let content = fs::read_to_string(tmp.path().join("tool/out.md")).unwrap();
        assert_eq!(content, "content");
    }
}
