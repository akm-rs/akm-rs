//! Instructions domain commands.
//!
//! The global instructions file lives in the personal registry, at
//! `library/instructions/global.md`. That gives it the same drift model,
//! publish flow and between-machine propagation as a skill, for free. From
//! there it is distributed to tool-specific directories under tool-specific
//! filenames.

pub mod edit;
pub mod publish;
pub mod scaffold;
pub mod sync;

use crate::error::{IoContext, Result};
use crate::paths::Paths;
use std::fs;
use std::path::{Path, PathBuf};

/// Copy pre-rc4 global instructions into the registry, once.
///
/// Instructions used to be a bare `~/.akm/global-instructions.md` with no
/// remote at all. A machine upgrading from rc3 keeps what it wrote: the old
/// file is copied in the first time the new one is needed. The old file is
/// left where it is — moving it would make a downgrade lossy for no gain.
///
/// Returns whether a copy was made.
pub(crate) fn seed_from_legacy(paths: &Paths) -> Result<bool> {
    let target = paths.instructions_file();
    let legacy = paths.legacy_global_instructions();

    if target.exists() || !legacy.is_file() {
        return Ok(false);
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .io_context(format!("Creating directory {}", parent.display()))?;
    }
    fs::copy(&legacy, &target).io_context(format!(
        "Copying {} to {}",
        legacy.display(),
        target.display()
    ))?;

    println!(
        "Moved global instructions into the personal registry ({} → {})",
        legacy.display(),
        target.display()
    );
    println!("Publish them with 'akm instructions publish'.");
    Ok(true)
}

/// An instructions sync target: a directory + filename pair.
///
/// Each tool expects global instructions at a different path with a different
/// filename. This struct captures that mapping.
#[derive(Debug, Clone)]
pub struct InstructionsTarget {
    /// Absolute path to the target directory.
    pub dir: PathBuf,
    /// Filename within that directory (e.g., "CLAUDE.md", "copilot-instructions.md").
    pub filename: String,
}

impl InstructionsTarget {
    /// Full path to the target file.
    pub fn path(&self) -> PathBuf {
        self.dir.join(&self.filename)
    }
}

/// Build the list of instructions sync targets.
///
/// | Directory | Filename |
/// |-----------|----------|
/// | `~/.claude` | `CLAUDE.md` |
/// | `~/.copilot` | `copilot-instructions.md` |
/// | `~/.vibe/prompts` | `cli.md` |
/// | `~/.agents` | `AGENTS.md` |
/// | `~/.pi/agent` | `AGENTS.md` |
///
/// Note: The `.vibe` target uses a subdirectory (`prompts/`), which differs from
/// the generic tool dir (`.vibe`). This is instructions-specific behavior.
///
/// Pi reads its global context file from its config dir (`~/.pi/agent`), using
/// the same `AGENTS.md` name as OpenCode.
///
/// # Arguments
/// * `home` — User home directory (for resolving `~/.claude`, etc.)
pub fn default_targets(home: &Path) -> Vec<InstructionsTarget> {
    vec![
        InstructionsTarget {
            dir: home.join(".claude"),
            filename: "CLAUDE.md".into(),
        },
        InstructionsTarget {
            dir: home.join(".copilot"),
            filename: "copilot-instructions.md".into(),
        },
        InstructionsTarget {
            dir: home.join(".vibe").join("prompts"),
            filename: "cli.md".into(),
        },
        InstructionsTarget {
            dir: home.join(".agents"),
            filename: "AGENTS.md".into(),
        },
        InstructionsTarget {
            dir: home.join(".pi").join("agent"),
            filename: "AGENTS.md".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    /// Paths rooted in a temp dir, plus the guard keeping it alive.
    fn test_paths() -> (Paths, TempDir) {
        let tmp = TempDir::new().unwrap();
        let paths = Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            &tmp.path().join("home"),
        );
        (paths, tmp)
    }

    fn write(path: &std::path::Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn seeding_carries_a_pre_rc4_file_into_the_registry() {
        let (paths, _tmp) = test_paths();
        write(&paths.legacy_global_instructions(), "Be terse.");

        assert!(seed_from_legacy(&paths).unwrap());
        assert_eq!(
            fs::read_to_string(paths.instructions_file()).unwrap(),
            "Be terse."
        );
        // The old file stays put — a downgrade must not be lossy.
        assert!(paths.legacy_global_instructions().is_file());
    }

    #[test]
    fn seeding_never_overwrites_the_registry_copy() {
        let (paths, _tmp) = test_paths();
        write(&paths.legacy_global_instructions(), "old");
        write(&paths.instructions_file(), "current");

        assert!(!seed_from_legacy(&paths).unwrap());
        assert_eq!(
            fs::read_to_string(paths.instructions_file()).unwrap(),
            "current"
        );
    }

    #[test]
    fn seeding_is_a_no_op_without_a_legacy_file() {
        let (paths, _tmp) = test_paths();
        assert!(!seed_from_legacy(&paths).unwrap());
        assert!(!paths.instructions_file().exists());
    }

    #[test]
    fn default_targets_has_five_entries() {
        let targets = default_targets(Path::new("/home/user"));
        assert_eq!(targets.len(), 5);
    }

    #[test]
    fn default_targets_paths_are_correct() {
        let targets = default_targets(Path::new("/home/user"));

        assert_eq!(
            targets[0].path(),
            PathBuf::from("/home/user/.claude/CLAUDE.md")
        );
        assert_eq!(
            targets[1].path(),
            PathBuf::from("/home/user/.copilot/copilot-instructions.md")
        );
        assert_eq!(
            targets[2].path(),
            PathBuf::from("/home/user/.vibe/prompts/cli.md")
        );
        assert_eq!(
            targets[3].path(),
            PathBuf::from("/home/user/.agents/AGENTS.md")
        );
        assert_eq!(
            targets[4].path(),
            PathBuf::from("/home/user/.pi/agent/AGENTS.md")
        );
    }

    #[test]
    fn vibe_target_uses_prompts_subdirectory() {
        let targets = default_targets(Path::new("/home/user"));
        let vibe = &targets[2];
        assert_eq!(vibe.dir, PathBuf::from("/home/user/.vibe/prompts"));
        assert_eq!(vibe.filename, "cli.md");
    }
}
