//! `akm instructions scaffold-project` — create AGENTS.md + CLAUDE.md in project root.
//!
//! Bash: `cmd_instructions_scaffold_project()` at bin/akm:586–612.
//!
//! Behavior:
//! 1. Detect project root via `git rev-parse --show-toplevel`
//! 2. If not in a git repo, return error
//! 3. Create AGENTS.md with starter content (only if missing)
//! 4. Create CLAUDE.md with pointer to AGENTS.md (only if missing)
//! 5. Print status for each file (created vs already exists)
//!
//! Idempotent: never overwrites existing files.

use crate::error::{IoContext, Result};
use crate::git::Git;
use std::fs;
use std::path::Path;

/// Default content for AGENTS.md.
/// Bash: `echo "# Project LLM Instructions" > "$project_root/AGENTS.md"`
const AGENTS_MD_CONTENT: &str = "# Project LLM Instructions\n";

/// Default content for CLAUDE.md.
/// Bash: `echo "LLM instructions live in @AGENTS.md" > "$project_root/CLAUDE.md"`
const CLAUDE_MD_CONTENT: &str = "LLM instructions live in @AGENTS.md\n";

/// Run `akm instructions scaffold-project`.
///
/// Creates `AGENTS.md` and `CLAUDE.md` at the project root with starter content.
/// Skips files that already exist (idempotent).
///
/// # Errors
/// - `Error::NotInGitRepo` if not inside a git repository
/// - IO errors if files cannot be written
pub fn run() -> Result<()> {
    let project_root = Git::toplevel(None)?;
    scaffold_at(&project_root)
}

/// Core scaffold logic, separated for testability.
///
/// # Arguments
/// * `project_root` — Absolute path to the project root directory
pub(crate) fn scaffold_at(project_root: &Path) -> Result<()> {
    println!(
        "Scaffolding project instructions: {}",
        project_root.display()
    );

    // Create AGENTS.md if missing
    let agents_path = project_root.join("AGENTS.md");
    if !agents_path.exists() {
        fs::write(&agents_path, AGENTS_MD_CONTENT)
            .io_context(format!("Creating {}", agents_path.display()))?;
        println!("  Created AGENTS.md");
    } else {
        println!("  AGENTS.md already exists");
    }

    // Create CLAUDE.md if missing
    let claude_path = project_root.join("CLAUDE.md");
    if !claude_path.exists() {
        fs::write(&claude_path, CLAUDE_MD_CONTENT)
            .io_context(format!("Creating {}", claude_path.display()))?;
        println!("  Created CLAUDE.md");
    } else {
        println!("  CLAUDE.md already exists");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn scaffold_creates_both_files() {
        let tmp = TempDir::new().unwrap();

        let result = scaffold_at(tmp.path());
        assert!(result.is_ok());

        let agents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
        assert_eq!(agents, "# Project LLM Instructions\n");

        let claude = fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
        assert_eq!(claude, "LLM instructions live in @AGENTS.md\n");
    }

    #[test]
    fn scaffold_skips_existing_agents_md() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("AGENTS.md"), "Custom AGENTS content").unwrap();

        let result = scaffold_at(tmp.path());
        assert!(result.is_ok());

        // AGENTS.md should NOT be overwritten
        let agents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
        assert_eq!(agents, "Custom AGENTS content");

        // CLAUDE.md should be created
        let claude = fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
        assert_eq!(claude, "LLM instructions live in @AGENTS.md\n");
    }

    #[test]
    fn scaffold_skips_existing_claude_md() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("CLAUDE.md"), "Custom CLAUDE content").unwrap();

        let result = scaffold_at(tmp.path());
        assert!(result.is_ok());

        // AGENTS.md should be created
        let agents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
        assert_eq!(agents, "# Project LLM Instructions\n");

        // CLAUDE.md should NOT be overwritten
        let claude = fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
        assert_eq!(claude, "Custom CLAUDE content");
    }

    #[test]
    fn scaffold_skips_both_if_both_exist() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("AGENTS.md"), "existing").unwrap();
        fs::write(tmp.path().join("CLAUDE.md"), "existing").unwrap();

        let result = scaffold_at(tmp.path());
        assert!(result.is_ok());

        assert_eq!(
            fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap(),
            "existing"
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap(),
            "existing"
        );
    }

    #[test]
    fn scaffold_is_idempotent() {
        let tmp = TempDir::new().unwrap();

        scaffold_at(tmp.path()).unwrap();
        scaffold_at(tmp.path()).unwrap();

        // Second run should not error or change content
        let agents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
        assert_eq!(agents, "# Project LLM Instructions\n");
    }
}
