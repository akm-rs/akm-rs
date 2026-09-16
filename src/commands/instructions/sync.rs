//! `akm instructions sync` — distribute global instructions to tool directories.
//!
//! Behavior:
//! 1. Carry a pre-rc4 instructions file into the registry, if there is one
//! 2. Check the registry-hosted source exists
//! 3. If not, print warning and return Ok (not an error)
//! 4. For each target (tool dir + filename): create dir, write the file
//! 5. For targets with `Delivery::Include`: also ensure the host file in the
//!    same directory contains a single `@<filename>` include line, appending
//!    it only if not already present — the host file's existing content is
//!    never overwritten
//! 6. Print count of distributed copies
//! 7. Retire a `~/.vibe/prompts/cli.md` left by 1.1.0 and earlier, when its
//!    content matches the source (proof akm wrote it) — on Vibe ≥ 2.9.0 that
//!    file replaces Vibe's built-in system prompt outright

use crate::commands::instructions::{
    default_targets, seed_from_legacy, Delivery, InstructionsTarget,
};
use crate::error::{IoContext, Result};
use crate::paths::Paths;
use std::fs;
use std::path::Path;

/// Run `akm instructions sync`.
///
/// Distributes `library/instructions/global.md` to all tool directories.
///
/// # Errors
/// Returns `Err` only on filesystem failures (permission denied, disk full).
/// Missing source file is a warning, not an error.
pub fn run(paths: &Paths) -> Result<()> {
    seed_from_legacy(paths)?;

    let source = paths.instructions_file();
    let home = paths.home();

    let targets = default_targets(home);
    sync_instructions(&source, &targets)?;
    retire_vibe_prompt_override(home, &source)
}

/// Remove `~/.vibe/prompts/cli.md` if it is a copy of the global instructions.
///
/// Releases up to 1.1.0 delivered Vibe's instructions there. Vibe 2.9.0 made a
/// user file named after a built-in prompt override it wholesale, so the copy
/// replaced Vibe's entire system prompt. Only a byte-identical copy is
/// removed — a hand-written `cli.md`, or one from a since-edited source, is
/// left alone.
pub(crate) fn retire_vibe_prompt_override(home: &Path, source: &Path) -> Result<()> {
    let stale = home.join(".vibe").join("prompts").join("cli.md");
    if !stale.is_file() || !source.is_file() {
        return Ok(());
    }

    let current = fs::read_to_string(source).io_context(format!(
        "Reading global instructions from {}",
        source.display()
    ))?;
    let leftover = fs::read_to_string(&stale).io_context(format!("Reading {}", stale.display()))?;
    if leftover != current {
        return Ok(());
    }

    fs::remove_file(&stale).io_context(format!("Removing {}", stale.display()))?;
    println!(
        "Removed {} (Vibe now reads ~/.vibe/AGENTS.md; that file overrode its system prompt)",
        stale.display()
    );
    Ok(())
}

/// Core sync logic, separated for testability.
///
/// # Arguments
/// * `source` — Path to the global instructions file
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

        if let Delivery::Include { host } = &target.delivery {
            ensure_include_line(&target.dir, host, &target.filename)?;
        }
    }

    println!("Global instructions distributed to {count} tool directories");
    Ok(())
}

/// Ensure `dir/host` contains a line `@<filename>`, appending it if missing.
///
/// The host file's existing content is never rewritten — this only appends,
/// and only when the include line is not already present (checked with
/// leading/trailing whitespace trimmed, so a manually reformatted line is
/// still recognized). If `dir/host` does not exist, it is created containing
/// just the include line.
fn ensure_include_line(dir: &Path, host: &str, filename: &str) -> Result<()> {
    let host_path = dir.join(host);
    let include_line = format!("@{filename}");

    let existing = if host_path.exists() {
        fs::read_to_string(&host_path).io_context(format!("Reading {}", host_path.display()))?
    } else {
        String::new()
    };

    if existing.lines().any(|line| line.trim() == include_line) {
        return Ok(());
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&include_line);
    updated.push('\n');

    fs::write(&host_path, updated).io_context(format!("Writing {}", host_path.display()))?;
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
            delivery: Delivery::Overwrite,
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
                delivery: Delivery::Overwrite,
            },
            InstructionsTarget {
                dir: tmp.path().join("tool2"),
                filename: "instructions.md".into(),
                delivery: Delivery::Overwrite,
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
            delivery: Delivery::Overwrite,
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
            delivery: Delivery::Overwrite,
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
            delivery: Delivery::Overwrite,
        }];

        // Run twice — second run should not fail
        sync_instructions(&source, &targets).unwrap();
        sync_instructions(&source, &targets).unwrap();

        let content = fs::read_to_string(tmp.path().join("tool/out.md")).unwrap();
        assert_eq!(content, "content");
    }

    #[test]
    fn retire_removes_prompt_override_that_matches_source() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let source = home.join("global.md");
        fs::write(&source, "content").unwrap();
        let stale = home.join(".vibe/prompts/cli.md");
        fs::create_dir_all(stale.parent().unwrap()).unwrap();
        fs::write(&stale, "content").unwrap();

        retire_vibe_prompt_override(home, &source).unwrap();

        assert!(!stale.exists());
        assert!(home.join(".vibe/prompts").is_dir());
    }

    #[test]
    fn retire_keeps_prompt_override_that_differs_from_source() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        let source = home.join("global.md");
        fs::write(&source, "content").unwrap();
        let stale = home.join(".vibe/prompts/cli.md");
        fs::create_dir_all(stale.parent().unwrap()).unwrap();
        fs::write(&stale, "my own prompt").unwrap();

        retire_vibe_prompt_override(home, &source).unwrap();

        assert_eq!(fs::read_to_string(&stale).unwrap(), "my own prompt");
    }

    #[test]
    fn retire_is_a_no_op_without_the_file() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("global.md");
        fs::write(&source, "content").unwrap();
        retire_vibe_prompt_override(tmp.path(), &source).unwrap();
    }

    #[test]
    fn include_appends_line_to_existing_host_keeping_its_text() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("global-instructions.md");
        fs::write(&source, "content").unwrap();

        let target_dir = tmp.path().join("posit");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("AGENTS.md"), "# My memory\nfact one").unwrap();

        let targets = vec![InstructionsTarget {
            dir: target_dir.clone(),
            filename: "akm-instructions.md".into(),
            delivery: Delivery::Include {
                host: "AGENTS.md".into(),
            },
        }];

        sync_instructions(&source, &targets).unwrap();

        let host = fs::read_to_string(target_dir.join("AGENTS.md")).unwrap();
        assert_eq!(host, "# My memory\nfact one\n@akm-instructions.md\n");
        let sibling = fs::read_to_string(target_dir.join("akm-instructions.md")).unwrap();
        assert_eq!(sibling, "content");
    }

    #[test]
    fn include_is_idempotent_on_second_sync() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("global-instructions.md");
        fs::write(&source, "content").unwrap();

        let target_dir = tmp.path().join("posit");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("AGENTS.md"), "# My memory\nfact one").unwrap();

        let targets = vec![InstructionsTarget {
            dir: target_dir.clone(),
            filename: "akm-instructions.md".into(),
            delivery: Delivery::Include {
                host: "AGENTS.md".into(),
            },
        }];

        sync_instructions(&source, &targets).unwrap();
        let after_first = fs::read(target_dir.join("AGENTS.md")).unwrap();

        sync_instructions(&source, &targets).unwrap();
        let after_second = fs::read(target_dir.join("AGENTS.md")).unwrap();

        assert_eq!(after_first, after_second);
    }

    #[test]
    fn include_creates_host_when_absent() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("global-instructions.md");
        fs::write(&source, "content").unwrap();

        let target_dir = tmp.path().join("posit");

        let targets = vec![InstructionsTarget {
            dir: target_dir.clone(),
            filename: "akm-instructions.md".into(),
            delivery: Delivery::Include {
                host: "AGENTS.md".into(),
            },
        }];

        sync_instructions(&source, &targets).unwrap();

        let host = fs::read_to_string(target_dir.join("AGENTS.md")).unwrap();
        assert_eq!(host, "@akm-instructions.md\n");
    }

    #[test]
    fn include_recognises_line_with_surrounding_whitespace() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("global-instructions.md");
        fs::write(&source, "content").unwrap();

        let target_dir = tmp.path().join("posit");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("AGENTS.md"), "  @akm-instructions.md \n").unwrap();

        let targets = vec![InstructionsTarget {
            dir: target_dir.clone(),
            filename: "akm-instructions.md".into(),
            delivery: Delivery::Include {
                host: "AGENTS.md".into(),
            },
        }];

        sync_instructions(&source, &targets).unwrap();

        let host = fs::read_to_string(target_dir.join("AGENTS.md")).unwrap();
        assert_eq!(host, "  @akm-instructions.md \n");
    }
}
