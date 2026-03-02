//! `akm skills edit` — edit spec metadata in $EDITOR.
//!
//! Bash: `cmd_skills_edit()` at bin/akm:2452–2588.
//!
//! This command is interactive (requires a TTY for the editor).

use crate::editor::resolve_editor;
use crate::error::{Error, IoContext, Result};
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use std::io::{self, BufRead, IsTerminal, Write};
use std::process::Command;

/// Run the `akm skills edit` command.
pub fn run(paths: &Paths, id: &str, tool_dirs: &ToolDirs) -> Result<()> {
    let mut library = Library::load_checked(paths)?;

    // Verify spec exists
    if !library.contains(id) {
        return Err(Error::SpecNotFound { id: id.to_string() });
    }

    // Extract entry to temp file
    let spec = library
        .get(id)
        .ok_or_else(|| Error::SpecNotFound { id: id.to_string() })?
        .clone();
    let spec_json = serde_json::to_string_pretty(&spec).map_err(|e| Error::Io {
        context: format!("Serializing spec '{id}' to JSON"),
        source: std::io::Error::other(e),
    })?;

    let tmp_dir = std::env::temp_dir();
    let tmp_file = tmp_dir.join(format!("akm-edit-{id}.json"));

    std::fs::write(&tmp_file, &spec_json)
        .io_context(format!("Writing temp file {}", tmp_file.display()))?;

    // Ensure cleanup on all exit paths
    let _cleanup = TempFileGuard {
        path: tmp_file.clone(),
    };

    let editor = resolve_editor();

    // Edit loop: open editor, validate, retry on error
    loop {
        let status =
            Command::new(&editor)
                .arg(&tmp_file)
                .status()
                .map_err(|e| Error::EditorFailed {
                    editor: editor.clone(),
                    message: e.to_string(),
                })?;

        if !status.success() {
            return Err(Error::EditorFailed {
                editor,
                message: format!("exited with status {}", status.code().unwrap_or(1)),
            });
        }

        // Read and validate
        let edited_content = std::fs::read_to_string(&tmp_file)
            .io_context(format!("Reading edited file {}", tmp_file.display()))?;

        let edited_spec: crate::library::spec::Spec = match serde_json::from_str(&edited_content) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error: Invalid JSON: {e}");
                if !prompt_retry()? {
                    println!("Changes discarded.");
                    return Ok(());
                }
                continue;
            }
        };

        // Validate ID wasn't changed
        if edited_spec.id != id {
            eprintln!(
                "Error: Cannot change spec id. Expected '{}', got '{}'.",
                id, edited_spec.id
            );
            if !prompt_retry()? {
                println!("Changes discarded.");
                return Ok(());
            }
            continue;
        }

        // Valid — merge back into library
        if let Some(existing) = library.get_mut(id) {
            *existing = edited_spec;
        }
        break;
    }

    library.save(paths)?;
    println!("Updated library.json entry for '{id}'");

    // Prompt to rebuild symlinks (relevant if core flag changed)
    if io::stdin().is_terminal() {
        print!("Rebuild global symlinks? [Y/n]: ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input).ok();
        let input = input.trim();
        if input.is_empty() || input.eq_ignore_ascii_case("y") {
            let library_dir = paths.data_dir();
            let core_specs = library.core_specs();
            let count = symlinks::rebuild_core(&core_specs, library_dir, tool_dirs.dirs())?;
            println!("{count} core symlinks rebuilt");
        }
    }

    Ok(())
}

/// Prompt user "Re-edit? [Y/n]:" and return true for yes.
fn prompt_retry() -> Result<bool> {
    if !io::stdin().is_terminal() {
        return Ok(false);
    }

    print!("Re-edit? [Y/n]: ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input).ok();
    let input = input.trim();
    Ok(input.is_empty() || input.eq_ignore_ascii_case("y"))
}

/// RAII guard to remove a temp file on drop.
struct TempFileGuard {
    path: std::path::PathBuf,
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
