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
