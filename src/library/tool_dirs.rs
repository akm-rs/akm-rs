//! Tool directory resolution — maps LLM tools to their global directories.
//!
//! Definitions load from `tools.json` in the data dir, falling back to the
//! built-in defaults below. New harnesses can therefore be added without
//! recompiling.
//!
//! `tools.json` format:
//! ```json
//! [
//!   {"name": "Claude Code", "command": "claude", "dir": ".claude"},
//!   {"name": "Github Copilot CLI", "command": "copilot", "dir": ".copilot"},
//!   {"name": "Mistral Vibe", "command": "vibe", "dir": ".vibe"},
//!   {"name": "OpenCode", "command": "opencode", "dir": ".agents"},
//!   {"name": "Pi", "command": "pi", "dir": ".pi/agent"}
//! ]
//! ```

use crate::error::{Error, IoContext};
use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A single tool definition from tools.json.
///
/// Matches the JSON objects in tools.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    /// Human-readable tool name (e.g., "Claude Code").
    pub name: String,
    /// CLI command name (e.g., "claude").
    pub command: String,
    /// Directory name relative to $HOME (e.g., ".claude").
    pub dir: String,
}

impl ToolDef {
    /// Directory name for this tool inside a session staging directory.
    ///
    /// The staging tree is flat — one directory per tool at its root — so it
    /// mirrors only the first component of [`ToolDef::dir`]. This matters for
    /// Pi, whose global dir is `~/.pi/agent` but whose staging dir is `.pi`.
    pub fn staging_dir(&self) -> &str {
        self.dir.split('/').next().unwrap_or(&self.dir)
    }
}

/// Built-in tool definitions matching the tools.json shipped with AKM.
///
/// Order matches tools.json (the canonical source).
fn builtin_tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "Claude Code".into(),
            command: "claude".into(),
            dir: ".claude".into(),
        },
        ToolDef {
            name: "Github Copilot CLI".into(),
            command: "copilot".into(),
            dir: ".copilot".into(),
        },
        ToolDef {
            name: "Mistral Vibe".into(),
            command: "vibe".into(),
            dir: ".vibe".into(),
        },
        ToolDef {
            name: "OpenCode".into(),
            command: "opencode".into(),
            dir: ".agents".into(),
        },
        // Pi reads its global AGENTS.md and skills/ from ~/.pi/agent,
        // not ~/.pi — the config dir is one level down.
        ToolDef {
            name: "Pi".into(),
            command: "pi".into(),
            dir: ".pi/agent".into(),
        },
    ]
}

/// Resolved tool directories.
///
/// Provides the full paths to global tool directories where core specs
/// are symlinked, and display names for UI.
#[derive(Debug, Clone)]
pub struct ToolDirs {
    /// Loaded tool definitions.
    tools: Vec<ToolDef>,
    /// Resolved absolute paths to global tool directories.
    dirs: Vec<PathBuf>,
}

impl ToolDirs {
    /// Load tool definitions from tools.json, falling back to built-in defaults.
    ///
    /// Reads from `Paths::tools_json()`. If the file is missing or
    /// unparseable, prints a warning to stderr and uses built-in defaults.
    pub fn load(paths: &Paths) -> Self {
        let home = paths
            .akm_home()
            .parent()
            .unwrap_or(paths.akm_home())
            .to_path_buf();

        let tools_json = paths.tools_json();
        let tools = if tools_json.is_file() {
            match Self::load_from_file(&tools_json) {
                Ok(tools) => tools,
                Err(e) => {
                    eprintln!("Warning: {e}\nUsing built-in tool definitions.");
                    builtin_tools()
                }
            }
        } else {
            builtin_tools()
        };

        let dirs = tools.iter().map(|t| home.join(&t.dir)).collect();

        Self { tools, dirs }
    }

    /// Create ToolDirs from explicit tool definitions and home directory.
    ///
    /// Used in tests to avoid filesystem dependency.
    pub fn from_tools(tools: Vec<ToolDef>, home: &Path) -> Self {
        let dirs = tools.iter().map(|t| home.join(&t.dir)).collect();
        Self { tools, dirs }
    }

    /// Create ToolDirs with built-in defaults for a given home directory.
    pub fn builtin(home: &Path) -> Self {
        let tools = builtin_tools();
        let dirs = tools.iter().map(|t| home.join(&t.dir)).collect();
        Self { tools, dirs }
    }

    /// Load tool definitions from a file path.
    fn load_from_file(path: &Path) -> crate::error::Result<Vec<ToolDef>> {
        let content = std::fs::read_to_string(path)
            .io_context(format!("Reading tools.json from {}", path.display()))?;

        serde_json::from_str(&content).map_err(|e| Error::ToolsParse {
            path: path.to_path_buf(),
            source: Box::new(e),
        })
    }

    /// Get the resolved global tool directory paths.
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    /// Get the tool definitions.
    pub fn tools(&self) -> &[ToolDef] {
        &self.tools
    }

    /// Per-tool directory names to use inside a session staging directory.
    ///
    /// See [`ToolDef::staging_dir`] — this is not the same as the last
    /// component of [`ToolDirs::dirs`].
    pub fn staging_names(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.staging_dir()).collect()
    }

    /// Get display names of all tools (for help text).
    pub fn display_names(&self) -> String {
        self.tools
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Number of tool directories.
    pub fn count(&self) -> usize {
        self.dirs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn builtin_has_five_tools() {
        let tmp = TempDir::new().unwrap();
        let td = ToolDirs::builtin(tmp.path());
        assert_eq!(td.count(), 5);
        assert_eq!(td.dirs()[0], tmp.path().join(".claude"));
        assert_eq!(td.dirs()[1], tmp.path().join(".copilot"));
        assert_eq!(td.dirs()[2], tmp.path().join(".vibe"));
        assert_eq!(td.dirs()[3], tmp.path().join(".agents"));
        assert_eq!(td.dirs()[4], tmp.path().join(".pi").join("agent"));
    }

    #[test]
    fn display_names_lists_every_tool() {
        let tmp = TempDir::new().unwrap();
        let td = ToolDirs::builtin(tmp.path());
        assert_eq!(
            td.display_names(),
            "Claude Code, Github Copilot CLI, Mistral Vibe, OpenCode, Pi"
        );
    }

    #[test]
    fn staging_names_flatten_nested_tool_dirs() {
        let tmp = TempDir::new().unwrap();
        let td = ToolDirs::builtin(tmp.path());
        assert_eq!(
            td.staging_names(),
            vec![".claude", ".copilot", ".vibe", ".agents", ".pi"]
        );
    }

    #[test]
    fn load_from_json_file() {
        let tmp = TempDir::new().unwrap();
        let json_path = tmp.path().join("tools.json");
        std::fs::write(
            &json_path,
            r#"[{"name":"TestTool","command":"test","dir":".test"}]"#,
        )
        .unwrap();

        let tools = ToolDirs::load_from_file(&json_path).unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "TestTool");
        assert_eq!(tools[0].dir, ".test");
    }

    #[test]
    fn from_tools_resolves_paths() {
        let tmp = TempDir::new().unwrap();
        let tools = vec![ToolDef {
            name: "Test".into(),
            command: "test".into(),
            dir: ".testtool".into(),
        }];
        let td = ToolDirs::from_tools(tools, tmp.path());
        assert_eq!(td.dirs(), &[tmp.path().join(".testtool")]);
    }
}
