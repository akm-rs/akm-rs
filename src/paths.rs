//! XDG-compliant path resolution for AKM.
//!
//! All file system paths are resolved through this module. No other module
//! should hardcode `~/.config`, `~/.local/share`, or `~/.cache` paths.

use std::path::{Path, PathBuf};

/// Resolved AKM directory paths. Immutable after construction.
///
/// Corresponds to the Bash globals:
/// - `LIBRARY_DIR` → [`Paths::data_dir`]
/// - `LIBRARY_JSON` → [`Paths::library_json`]
/// - `TOOLS_JSON` → [`Paths::tools_json`]
/// - `${XDG_CONFIG_HOME}/akm/config` → [`Paths::config_file`]
/// - `$HOME/.cache/akm/...` → [`Paths::cache_dir`]
#[derive(Debug, Clone)]
pub struct Paths {
    /// `$XDG_DATA_HOME/akm` — cold library, shell init, tools.json
    data_dir: PathBuf,
    /// `$XDG_DATA_HOME` — base data directory (parent of data_dir)
    data_home: PathBuf,
    /// `$XDG_CONFIG_HOME/akm` — config directory
    config_dir: PathBuf,
    /// `$XDG_CACHE_HOME/akm` — registry caches, session staging
    cache_dir: PathBuf,
    /// `$HOME/.akm` — artifacts, global instructions (non-XDG, user-visible)
    akm_home: PathBuf,
    /// Resolved home directory.
    home: PathBuf,
}

impl Paths {
    /// Resolve paths from XDG environment using the `dirs` crate.
    ///
    /// Returns `None` only if the home directory cannot be determined
    /// (extremely rare on any real system).
    pub fn resolve() -> Option<Self> {
        let home = dirs::home_dir()?;
        let data_home = dirs::data_dir()?;
        let data_dir = data_home.join("akm");
        let config_dir = dirs::config_dir()?.join("akm");
        let cache_dir = dirs::cache_dir()?.join("akm");
        let akm_home = home.join(".akm");
        Some(Self {
            data_dir,
            data_home,
            config_dir,
            cache_dir,
            akm_home,
            home,
        })
    }

    /// Construct from explicit base directories (for testing).
    ///
    /// All paths are derived from the provided roots.
    pub fn from_roots(data: &Path, config: &Path, cache: &Path, home: &Path) -> Self {
        Self {
            data_dir: data.join("akm"),
            data_home: data.to_path_buf(),
            config_dir: config.join("akm"),
            cache_dir: cache.join("akm"),
            akm_home: home.join(".akm"),
            home: home.to_path_buf(),
        }
    }

    // --- Data dir (cold library) ---

    /// `$XDG_DATA_HOME/akm` — root of the cold library.
    /// Bash: `LIBRARY_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/akm"`
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// `$XDG_DATA_HOME` — base data directory (e.g., `~/.local/share`).
    ///
    /// Used for shell-specific completion file paths that live alongside
    /// other XDG data directories (bash-completion, zsh, fish).
    pub fn xdg_data_home(&self) -> &Path {
        &self.data_home
    }

    /// `$XDG_DATA_HOME/akm/library.json`
    /// Bash: `LIBRARY_JSON="$LIBRARY_DIR/library.json"`
    pub fn library_json(&self) -> PathBuf {
        self.data_dir.join("library.json")
    }

    /// `$XDG_DATA_HOME/akm/tools.json`
    /// Bash: `TOOLS_JSON="$LIBRARY_DIR/tools.json"`
    pub fn tools_json(&self) -> PathBuf {
        self.data_dir.join("tools.json")
    }

    /// `$XDG_DATA_HOME/akm/skills/` — installed skills
    pub fn skills_dir(&self) -> PathBuf {
        self.data_dir.join("skills")
    }

    /// `$XDG_DATA_HOME/akm/agents/` — installed agents
    pub fn agents_dir(&self) -> PathBuf {
        self.data_dir.join("agents")
    }

    /// `$XDG_DATA_HOME/akm/shell/akm-init.sh`
    pub fn shell_init(&self) -> PathBuf {
        self.data_dir.join("shell").join("akm-init.sh")
    }

    // --- Config dir ---

    /// `$XDG_CONFIG_HOME/akm` — config directory
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// `$XDG_CONFIG_HOME/akm/config.toml`
    /// Bash: `${XDG_CONFIG_HOME:-$HOME/.config}/akm/config`
    /// Note: Rust version uses `.toml` extension (fresh start, no migration).
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    // --- Cache dir ---

    /// `$XDG_CACHE_HOME/akm` — caches and ephemeral data
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// `$XDG_CACHE_HOME/akm/skills-community-registry/`
    /// Bash: `$HOME/.cache/akm/skills-community-registry`
    pub fn community_registry_cache(&self) -> PathBuf {
        self.cache_dir.join("skills-community-registry")
    }

    /// `$XDG_CACHE_HOME/akm/skills-personal-registry/`
    /// Bash: `$HOME/.cache/akm/skills-personal-registry`
    pub fn personal_registry_cache(&self) -> PathBuf {
        self.cache_dir.join("skills-personal-registry")
    }

    /// Session staging dir: `$XDG_CACHE_HOME/akm/<session_id>/`
    /// Bash: `$HOME/.cache/akm/$session_id`
    pub fn session_staging(&self, session_id: &str) -> PathBuf {
        self.cache_dir.join(session_id)
    }

    // --- AKM home (non-XDG, user-visible) ---

    /// `$HOME/.akm` — artifacts root, global instructions
    pub fn akm_home(&self) -> &Path {
        &self.akm_home
    }

    /// `$HOME/.akm/artifacts/` — default artifacts dir (overridable in config)
    pub fn default_artifacts_dir(&self) -> PathBuf {
        self.akm_home.join("artifacts")
    }

    /// `$HOME/.akm/global-instructions.md`
    pub fn global_instructions(&self) -> PathBuf {
        self.akm_home.join("global-instructions.md")
    }

    /// User's home directory. Used for resolving global tool dirs.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// `$XDG_CACHE_HOME/akm/last-update-check.json` — cached version check result.
    ///
    /// Contains a JSON blob with the latest known version and check timestamp.
    /// Used by the background version check to avoid hitting the network
    /// more than once per `check_interval`.
    pub fn update_check_cache(&self) -> PathBuf {
        self.cache_dir.join("last-update-check.json")
    }

    /// Global tool directories for symlink targets.
    /// Bash: `GLOBAL_TOOL_DIRS=("$HOME/.claude" "$HOME/.copilot" "$HOME/.agents" "$HOME/.vibe")`
    ///
    /// In the Rust version these come from tools.json, but we provide defaults
    /// matching the Bash hard-coded list for when tools.json is absent.
    pub fn default_global_tool_dirs(&self) -> Vec<PathBuf> {
        vec![
            self.home.join(".claude"),
            self.home.join(".copilot"),
            self.home.join(".agents"),
            self.home.join(".vibe"),
        ]
    }
}
