//! Error types for the AKM CLI.
//!
//! All fallible operations return `Result<T, Error>`. User-facing errors
//! include both what went wrong and a suggestion for what to do next.

use std::path::PathBuf;

/// Top-level error type for AKM.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Config file could not be read or parsed.
    #[error("Failed to read config at {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Config file could not be written.
    #[error("Failed to write config to {path}: {source}")]
    ConfigWrite {
        path: PathBuf,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Config validation failed (e.g. invalid feature name, bad boolean).
    #[error("Invalid config value for '{key}': {message}")]
    ConfigValidation { key: String, message: String },

    /// Unknown config key (maps to Bash _config_key_to_var default case).
    #[error("Unknown config key: '{key}'\nAvailable keys: {available}")]
    UnknownConfigKey { key: String, available: String },

    /// Git command failed.
    #[error("Git command failed: `git {args}`\n{stderr}")]
    Git { args: String, stderr: String },

    /// Git is not available on PATH.
    #[error("git is not installed or not on PATH. Install git to continue.")]
    GitNotFound,

    /// Not inside a git repository (maps to _akm_in_git_repo failing).
    #[error("Not inside a git repository")]
    NotInGitRepo,

    /// A required path does not exist.
    #[error("{description}: path does not exist: {path}")]
    PathNotFound { path: PathBuf, description: String },

    /// IO error wrapper.
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },

    /// Library not found (maps to _check_library).
    #[error("Library not found at {path}\nRun 'akm skills sync' to populate the library.")]
    LibraryNotFound { path: PathBuf },

    /// No active session (maps to _check_session).
    #[error(
        "No active skill session.\nLaunch via claude/copilot/opencode wrapper to start a session."
    )]
    NoActiveSession,

    /// Spec not found in library.
    #[error("Spec not found in library: '{id}'\nRun 'akm skills sync' to update your library, or 'akm skills list' to browse.")]
    SpecNotFound { id: String },

    /// Invalid spec type.
    #[error("Invalid spec type: '{value}' (expected 'skill' or 'agent')")]
    InvalidSpecType { value: String },

    /// Frontmatter missing required field.
    #[error("Missing required frontmatter field '{field}' in {path}\nAdd '{field}: ...' to the YAML frontmatter.")]
    FrontmatterMissing { field: String, path: PathBuf },

    /// Frontmatter parse error.
    #[error("Failed to parse frontmatter in {path}: {message}")]
    FrontmatterParse { path: PathBuf, message: String },

    /// No skills or agents directories found for libgen.
    #[error("Cannot locate a directory with skills/ or agents/ in {path}")]
    NoSpecDirs { path: PathBuf },

    /// Manifest error — not in a git repo.
    #[error("Cannot manage project manifest: not in a git repository")]
    ManifestNoProject,

    /// Library JSON deserialization error.
    #[error("Failed to parse library.json at {path}: {source}")]
    LibraryParse {
        path: PathBuf,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Library JSON write error.
    #[error("Failed to write library.json to {path}: {source}")]
    LibraryWrite {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Manifest JSON deserialization error.
    #[error("Failed to parse manifest at {path}: {source}")]
    ManifestParse {
        path: PathBuf,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Manifest JSON write error.
    #[error("Failed to write manifest to {path}: {source}")]
    ManifestWrite {
        path: PathBuf,
        source: std::io::Error,
    },

    /// No artifacts remote configured.
    ///
    /// Bash: bin/akm:480–482 — warning that returns 0.
    /// In Rust we model it as a distinct error so the caller can decide presentation.
    #[error(
        "No artifacts remote configured.\nRun 'akm setup' to configure an artifacts repository."
    )]
    #[allow(dead_code)] // Available for callers that need an error representation of NoRemote
    ArtifactsNoRemote,

    /// Artifacts sync failed (pull or push).
    ///
    /// Bash: bin/akm:488–491 / bin/akm:501–504
    #[error("Artifacts sync failed: {operation}\n{message}\nCheck your connection or SSH keys.")]
    ArtifactsSync { operation: String, message: String },

    /// Artifacts clone failed on first-time setup.
    ///
    /// Bash: bin/akm:511–514
    #[error(
        "Failed to clone artifacts from {remote}\n{message}\nCheck the URL and your SSH keys."
    )]
    ArtifactsClone { remote: String, message: String },

    /// Registry sync failed but a cached copy exists.
    /// The sync can continue with the cached data.
    /// This is NOT a hard error — it becomes a warning printed to stderr.
    ///
    /// Bash: bin/akm:1510 "Warning: Failed to pull community registry..."
    /// Bash: bin/akm:1571 "Warning: Failed to pull personal registry..."
    #[error("Failed to sync registry '{name}': {message}")]
    RegistrySync { name: String, message: String },

    /// No skills available — community clone failed with no cache and no library.
    ///
    /// Bash: bin/akm:1522–1524
    #[error("No cached skills and no existing cold library. Cannot proceed.\nRun 'akm setup' to configure a skills registry.")]
    NoSkillsAvailable,

    /// Symlink creation failed.
    ///
    /// Bash: `ln -sfn` doesn't report errors (failures are silently counted),
    /// but Rust should surface them.
    #[error("Failed to create symlink {link} → {target}: {source}")]
    SymlinkCreate {
        link: PathBuf,
        target: PathBuf,
        source: std::io::Error,
    },

    /// tools.json parse error (non-fatal, falls back to defaults).
    /// Printed as warning, not a hard error.
    #[error("Failed to parse tools.json at {path}: {source}")]
    ToolsParse {
        path: PathBuf,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Editor command not found.
    #[error("Editor '{editor}' not found. Set $EDITOR or install nano.\nTried: $EDITOR → git config core.editor → nano")]
    EditorNotFound { editor: String },

    /// Editor command exited with non-zero status.
    #[error("Editor '{editor}' exited with status {status}")]
    EditorFailed { editor: String, status: i32 },
}

/// Convenience alias used throughout the codebase.
pub type Result<T> = std::result::Result<T, Error>;

/// Extension trait for adding context to `std::io::Error`.
pub trait IoContext<T> {
    /// Wrap an IO error with human-readable context.
    fn io_context(self, context: impl Into<String>) -> Result<T>;
}

impl<T> IoContext<T> for std::result::Result<T, std::io::Error> {
    fn io_context(self, context: impl Into<String>) -> Result<T> {
        self.map_err(|source| Error::Io {
            context: context.into(),
            source,
        })
    }
}
