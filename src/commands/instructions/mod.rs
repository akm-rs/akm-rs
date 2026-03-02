//! Instructions domain commands.
//!
//! Distributes global instructions from `~/.akm/global-instructions.md` to
//! tool-specific directories with tool-specific filenames.

pub mod edit;
pub mod scaffold;
pub mod sync;

use std::path::{Path, PathBuf};

/// An instructions sync target: a directory + filename pair.
///
/// Each tool expects global instructions at a different path with a different
/// filename. This struct captures that mapping.
///
/// Bash: the `targets` associative array in `cmd_instructions_sync()` (bin/akm:567–572).
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
/// Maps the Bash associative array:
/// ```bash
/// local -A targets=(
///     ["$HOME/.claude"]="CLAUDE.md"
///     ["$HOME/.copilot"]="copilot-instructions.md"
///     ["$HOME/.vibe/prompts"]="cli.md"
///     ["$HOME/.agents"]="AGENTS.md"
/// )
/// ```
///
/// Note: The `.vibe` target uses a subdirectory (`prompts/`), which differs from
/// the generic tool dir (`.vibe`). This is instructions-specific behavior.
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
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn default_targets_has_four_entries() {
        let targets = default_targets(Path::new("/home/user"));
        assert_eq!(targets.len(), 4);
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
    }

    #[test]
    fn vibe_target_uses_prompts_subdirectory() {
        let targets = default_targets(Path::new("/home/user"));
        let vibe = &targets[2];
        assert_eq!(vibe.dir, PathBuf::from("/home/user/.vibe/prompts"));
        assert_eq!(vibe.filename, "cli.md");
    }
}
