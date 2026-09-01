//! `akm harness` — sync per-harness configs across machines via the registry.

pub mod pull;
pub mod push;
pub mod status;

use crate::error::{Error, Result};
use crate::library::harness_config::{builtin_for, HarnessConfigDef};
use crate::library::tool_dirs::ToolDirs;
use crate::paths::Paths;
use std::path::PathBuf;

/// Resolve the live global dir for a harness command from ToolDirs.
pub(crate) fn live_dir(paths: &Paths, command: &str) -> Option<PathBuf> {
    let tools = ToolDirs::load(paths);
    tools
        .tools()
        .iter()
        .position(|t| t.command == command)
        .map(|i| tools.dirs()[i].clone())
}

/// The registry sub-tree for a harness: `library/harnesses/<command>`.
pub(crate) fn tree_dir(paths: &Paths, command: &str) -> PathBuf {
    paths.library_dir().join("harnesses").join(command)
}

/// Registry pathspec for a harness sub-tree (forward slash, relative).
pub(crate) fn pathspec(command: &str) -> String {
    format!("harnesses/{command}")
}

/// The def for a command, or a friendly error naming the supported ones.
pub(crate) fn def_for(command: &str) -> Result<HarnessConfigDef> {
    builtin_for(command).ok_or_else(|| Error::ConfigValidation {
        key: "harness".into(),
        message: format!("'{command}' is not a supported harness (try: pi, claude, opencode)"),
    })
}

/// Commands to act on: the given one, or all supported ones.
pub(crate) fn targets(command: Option<&str>) -> Result<Vec<String>> {
    match command {
        Some(c) => {
            def_for(c)?; // validate
            Ok(vec![c.to_string()])
        }
        None => Ok(crate::library::harness_config::builtin_harness_configs()
            .into_iter()
            .map(|d| d.command)
            .collect()),
    }
}
