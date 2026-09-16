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
//!   {"name": "Pi", "command": "pi", "dir": ".pi/agent"},
//!   {"name": "Posit Assistant", "command": "pa", "dir": ".posit/assistant", "mount": "tree", "project_dir": ".posit/assistant"}
//! ]
//! ```
//!
//! `mount` defaults to `symlink` when absent; `project_dir` is optional.

use crate::error::{Error, IoContext};
use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How a harness's global dir receives mounted specs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mount {
    /// `<dir>/skills/<id>` is a symlink to the library skill (all harnesses so far).
    #[default]
    Symlink,
    /// `<dir>/skills/<id>/` is a real directory whose entries are symlinks into
    /// the library skill. For harnesses whose discovery skips symlinked dirs.
    /// Agents are not mounted.
    Tree,
}

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
    /// How specs are mounted into `dir` (see [`Mount`]).
    #[serde(default)]
    pub mount: Mount,
    /// Project-relative dir the harness reads project skills from, for
    /// harnesses with no session mount. Materialized as a sidecar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
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
            mount: Mount::Symlink,
            project_dir: None,
        },
        ToolDef {
            name: "Github Copilot CLI".into(),
            command: "copilot".into(),
            dir: ".copilot".into(),
            mount: Mount::Symlink,
            project_dir: None,
        },
        ToolDef {
            name: "Mistral Vibe".into(),
            command: "vibe".into(),
            dir: ".vibe".into(),
            mount: Mount::Symlink,
            project_dir: None,
        },
        ToolDef {
            name: "OpenCode".into(),
            command: "opencode".into(),
            dir: ".agents".into(),
            mount: Mount::Symlink,
            project_dir: None,
        },
        // Pi reads its global AGENTS.md and skills/ from ~/.pi/agent,
        // not ~/.pi — the config dir is one level down.
        ToolDef {
            name: "Pi".into(),
            command: "pi".into(),
            dir: ".pi/agent".into(),
            mount: Mount::Symlink,
            project_dir: None,
        },
        // Posit Assistant's skill discovery skips symlinked directories, so
        // its skills need a real directory per skill (tree mount) rather than
        // a single symlink. It also has no session lifecycle, so project
        // skills are materialized into a project_dir sidecar instead.
        ToolDef {
            name: "Posit Assistant".into(),
            command: "pa".into(),
            dir: ".posit/assistant".into(),
            mount: Mount::Tree,
            project_dir: Some(".posit/assistant".into()),
        },
    ]
}

/// A resolved global tool dir together with how specs are mounted into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountTarget {
    /// Resolved absolute path to the global tool directory.
    pub dir: PathBuf,
    /// How specs are mounted into `dir`.
    pub mount: Mount,
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

    /// Resolved global tool dirs paired with their mount kind.
    pub fn mounts(&self) -> Vec<MountTarget> {
        self.dirs
            .iter()
            .zip(self.tools.iter())
            .map(|(dir, tool)| MountTarget {
                dir: dir.clone(),
                mount: tool.mount,
            })
            .collect()
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
    fn builtin_has_six_tools() {
        let tmp = TempDir::new().unwrap();
        let td = ToolDirs::builtin(tmp.path());
        assert_eq!(td.count(), 6);
        assert_eq!(td.dirs()[0], tmp.path().join(".claude"));
        assert_eq!(td.dirs()[1], tmp.path().join(".copilot"));
        assert_eq!(td.dirs()[2], tmp.path().join(".vibe"));
        assert_eq!(td.dirs()[3], tmp.path().join(".agents"));
        assert_eq!(td.dirs()[4], tmp.path().join(".pi").join("agent"));
        assert_eq!(td.dirs()[5], tmp.path().join(".posit").join("assistant"));
    }

    #[test]
    fn display_names_lists_every_tool() {
        let tmp = TempDir::new().unwrap();
        let td = ToolDirs::builtin(tmp.path());
        assert_eq!(
            td.display_names(),
            "Claude Code, Github Copilot CLI, Mistral Vibe, OpenCode, Pi, Posit Assistant"
        );
    }

    #[test]
    fn staging_names_flatten_nested_tool_dirs() {
        let tmp = TempDir::new().unwrap();
        let td = ToolDirs::builtin(tmp.path());
        assert_eq!(
            td.staging_names(),
            vec![".claude", ".copilot", ".vibe", ".agents", ".pi", ".posit"]
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
            mount: Mount::Symlink,
            project_dir: None,
        }];
        let td = ToolDirs::from_tools(tools, tmp.path());
        assert_eq!(td.dirs(), &[tmp.path().join(".testtool")]);
    }

    #[test]
    fn mount_defaults_to_symlink_when_absent_from_json() {
        let tmp = TempDir::new().unwrap();
        let json_path = tmp.path().join("tools.json");
        std::fs::write(
            &json_path,
            r#"[{"name":"TestTool","command":"test","dir":".test"}]"#,
        )
        .unwrap();

        let tools = ToolDirs::load_from_file(&json_path).unwrap();
        assert_eq!(tools[0].mount, Mount::Symlink);
        assert_eq!(tools[0].project_dir, None);
    }

    #[test]
    fn posit_entry_parses_tree_mount_and_project_dir() {
        let tmp = TempDir::new().unwrap();
        let json_path = tmp.path().join("tools.json");
        std::fs::write(
            &json_path,
            r#"[{"name":"Posit Assistant","command":"pa","dir":".posit/assistant","mount":"tree","project_dir":".posit/assistant"}]"#,
        )
        .unwrap();

        let tools = ToolDirs::load_from_file(&json_path).unwrap();
        assert_eq!(tools[0].mount, Mount::Tree);
        assert_eq!(tools[0].project_dir.as_deref(), Some(".posit/assistant"));
    }

    #[test]
    fn mounts_pairs_dir_with_mount_kind() {
        let tmp = TempDir::new().unwrap();
        let td = ToolDirs::builtin(tmp.path());
        let mounts = td.mounts();
        assert_eq!(mounts.len(), 6);
        assert_eq!(mounts[0].dir, tmp.path().join(".claude"));
        assert_eq!(mounts[0].mount, Mount::Symlink);
        assert_eq!(mounts[5].dir, tmp.path().join(".posit").join("assistant"));
        assert_eq!(mounts[5].mount, Mount::Tree);
    }
}
