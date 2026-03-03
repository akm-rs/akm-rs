//! `akm instructions edit` — edit global instructions in $EDITOR.
//!
//! Bash: `cmd_instructions_edit()` at bin/akm:614–643.
//!
//! Behavior:
//! 1. Ensure global-instructions.md exists (create with starter header if not)
//! 2. Resolve editor ($EDITOR → git var GIT_EDITOR → nano)
//! 3. Open editor
//! 4. After editor exits, prompt to sync (if TTY)
//! 5. If user says yes, run instructions sync
//!
//! No backward-compat migration (Rust is v1.0).

use crate::commands::instructions::default_targets;
use crate::commands::instructions::sync::sync_instructions;
use crate::editor::resolve_editor;
use crate::error::{Error, IoContext, Result};
use crate::paths::Paths;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;
use std::process::Command;

/// Ensure global instructions file exists, creating with starter header if needed.
///
/// Bash: `echo "# Global LLM Instructions" > "$instructions_file"` (bin/akm:627)
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
pub fn run(paths: &Paths) -> Result<()> {
    let instructions_file = paths.global_instructions();

    // Create with starter header if it doesn't exist
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
    // Bash: `read -rp "Sync changes to tool directories? [Y/n]: " sync_answer`
    if io::stdin().is_terminal() && prompt_sync()? {
        let home = paths.home();
        let targets = default_targets(home);
        sync_instructions(&instructions_file, &targets)?;
    }

    Ok(())
}

/// Prompt "Sync changes to tool directories? [Y/n]: " and return true for yes.
///
/// Bash: `read -rp "Sync changes to tool directories? [Y/n]: " sync_answer`
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
