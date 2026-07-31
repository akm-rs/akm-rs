//! `akm instructions edit` — edit global instructions in $EDITOR.
//!
//! Behavior:
//! 1. Ensure `library/instructions/global.md` exists — seeded from the pre-rc4
//!    location if this machine has one, otherwise a starter header
//! 2. Resolve editor ($EDITOR → git var GIT_EDITOR → nano)
//! 3. Open editor
//! 4. After editor exits, prompt to distribute to the tool dirs (if TTY)
//! 5. Then offer to publish the change to the personal registry
//!
//! Editing is the only place that prompts. `akm sync` reports drift and moves
//! on, so a session start is never blocked on a question.

use crate::commands::instructions::sync::sync_instructions;
use crate::commands::instructions::{default_targets, publish, seed_from_legacy};
use crate::config::Config;
use crate::editor::resolve_editor;
use crate::error::{Error, IoContext, Result};
use crate::paths::Paths;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;
use std::process::Command;

/// Ensure global instructions file exists, creating with starter header if needed.
///
/// This is extracted as a helper for testability — `run()` opens an editor
/// which is inherently untestable in unit tests.
pub(crate) fn ensure_instructions_file(path: &Path) -> Result<()> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .io_context(format!("Creating directory {}", parent.display()))?;
        }
        fs::write(path, "# Global LLM Instructions\n")
            .io_context(format!("Creating {}", path.display()))?;
    }
    Ok(())
}

/// Run `akm instructions edit`.
///
/// Opens the global instructions file in the user's editor, then optionally
/// syncs to tool directories.
///
/// # Errors
/// Returns `Err` if the editor cannot be launched or exits with a non-zero status.
pub fn run(paths: &Paths, config: &Config) -> Result<()> {
    let instructions_file = paths.instructions_file();

    // An rc3 machine keeps what it already wrote; a fresh one gets a header.
    seed_from_legacy(paths)?;
    ensure_instructions_file(&instructions_file)?;

    // Resolve and launch editor
    let editor = resolve_editor();
    let status = Command::new(&editor)
        .arg(&instructions_file)
        .status()
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                Error::EditorNotFound {
                    editor: editor.clone(),
                }
            } else {
                Error::Io {
                    context: format!("Launching editor '{editor}'"),
                    source: e,
                }
            }
        })?;

    if !status.success() {
        return Err(Error::EditorFailed {
            editor,
            message: format!("exited with status {}", status.code().unwrap_or(-1)),
        });
    }

    // Prompt to sync after editing (only if interactive TTY)
    if io::stdin().is_terminal() && prompt_sync()? {
        let home = paths.home();
        let targets = default_targets(home);
        sync_instructions(&instructions_file, &targets)?;
    }

    // Distributing is local; the other machines only see it once it is pushed.
    publish::offer(paths, config);

    Ok(())
}

/// Prompt "Sync changes to tool directories? [Y/n]: " and return true for yes.
///
/// Default is Y (Enter = yes).
fn prompt_sync() -> Result<bool> {
    print!("Sync changes to tool directories? [Y/n]: ");
    io::stdout().flush().ok();

    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .io_context("Reading sync prompt input")?;

    let answer = line.trim();
    // Empty (just Enter) or Y/y = yes
    Ok(answer.is_empty() || answer.eq_ignore_ascii_case("y"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ensure_creates_file_if_missing() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("sub").join("global-instructions.md");
        assert!(!file.exists());

        ensure_instructions_file(&file).unwrap();
        assert!(file.exists());
        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "# Global LLM Instructions\n");
    }

    #[test]
    fn ensure_does_not_overwrite_existing_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("global-instructions.md");
        fs::write(&file, "My custom content").unwrap();

        ensure_instructions_file(&file).unwrap();
        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "My custom content");
    }

    #[test]
    fn ensure_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("global-instructions.md");

        ensure_instructions_file(&file).unwrap();
        ensure_instructions_file(&file).unwrap();

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "# Global LLM Instructions\n");
    }
}
