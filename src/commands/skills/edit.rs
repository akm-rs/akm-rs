//! `akm skills edit` — edit a spec in $EDITOR.
//!
//! By default this opens the spec's own markdown, because that is what people
//! actually want to change: a skill that misfires needs its instructions
//! fixed, not its index entry. `--meta` opens the `akm.json` sidecar for the
//! human-facing name, description, tags and core default.
//!
//! This is the one command that prompts to publish. Sync runs unattended at
//! session start and only reports; an edit is by definition someone sitting at
//! a terminal who just changed something, and asking them there is the moment
//! the answer is cheapest.

use crate::config::Config;
use crate::editor::resolve_editor;
use crate::error::{Error, Result};
use crate::library::libgen;
use crate::library::local::LocalOverrides;
use crate::library::spec::SpecMeta;
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use crate::registry::Registry;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;
use std::process::Command;

/// Run the `akm skills edit` command.
pub fn run(
    paths: &Paths,
    config: &Config,
    id: &str,
    meta: bool,
    tool_dirs: &ToolDirs,
) -> Result<()> {
    let library_dir = paths.library_dir();
    let library = Library::load_checked(paths)?;
    let spec = library
        .get(id)
        .ok_or_else(|| Error::SpecNotFound { id: id.to_string() })?;

    if meta {
        // The sidecar is created on first edit rather than on sync, so a
        // registry only grows metadata files for specs someone curated.
        let sidecar = spec.sidecar_path(&library_dir);
        if !sidecar.is_file() {
            spec.to_meta().save_to(&sidecar)?;
        }
        edit_until_valid(&sidecar)?;
    } else {
        let markdown = spec.markdown_path(&library_dir);
        if !markdown.is_file() {
            return Err(Error::SpecNotFound { id: id.to_string() });
        }
        open_editor(&markdown)?;
    }

    // The edit may have changed the name, description or core flag, so the
    // derived index and the symlinks that follow from it are rebuilt.
    libgen::generate(&library_dir, &paths.library_json())?;
    let mut library = Library::load_from(&paths.library_json())?;
    let mut overrides = LocalOverrides::load_from(&paths.local_json())?;
    if overrides.apply(&mut library) {
        overrides.save_to(&paths.local_json())?;
    }
    library.save_to(&paths.library_json())?;

    let count = symlinks::rebuild_core(&library.core_specs(), &library_dir, tool_dirs.dirs())?;
    println!("Updated '{id}' ({count} core symlinks rebuilt)");

    offer_publish_if_changed(paths, config, id)?;

    Ok(())
}

/// Open a JSON file in the editor, re-opening until it parses as `SpecMeta`.
fn edit_until_valid(sidecar: &Path) -> Result<()> {
    loop {
        open_editor(sidecar)?;

        match SpecMeta::load_from(sidecar) {
            Ok(_) => return Ok(()),
            Err(e) => {
                eprintln!("Error: {e}");
                if !prompt_retry()? {
                    println!("Leaving the file as you saved it.");
                    return Ok(());
                }
            }
        }
    }
}

/// Launch `$EDITOR` on a path and require a clean exit.
fn open_editor(path: &Path) -> Result<()> {
    let editor = resolve_editor();

    let status = Command::new(&editor).arg(path).status().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            Error::EditorNotFound {
                editor: editor.clone(),
            }
        } else {
            Error::EditorFailed {
                editor: editor.clone(),
                message: e.to_string(),
            }
        }
    })?;

    if status.success() {
        Ok(())
    } else {
        Err(Error::EditorFailed {
            editor,
            message: format!("exited with status {}", status.code().unwrap_or(1)),
        })
    }
}

/// Offer to publish, but only if the edit actually left something to publish.
fn offer_publish_if_changed(paths: &Paths, config: &Config, id: &str) -> Result<()> {
    let Some(url) = config.registry_url() else {
        return Ok(());
    };

    let registry = Registry::new(url, paths.library_dir());
    if !registry.is_cloned() {
        return Ok(());
    }

    if registry.drift()?.state_of(id).has_local_changes() {
        super::publish::offer(paths, config, id);
    }

    Ok(())
}

/// Prompt "Re-edit? [Y/n]:" and return true for yes.
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
