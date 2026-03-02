//! CLI commands for the artifacts domain.
//!
//! Currently only `sync` — the artifacts domain is intentionally simple.

pub mod sync;

use clap::Subcommand;

/// Artifacts subcommands.
///
/// Bash: `cmd_artifacts()` (bin/akm:449–472)
#[derive(Debug, Subcommand)]
pub enum ArtifactsCommands {
    /// Bidirectional sync with artifacts remote.
    ///
    /// Clones the artifacts repo on first run, then pulls and pushes
    /// on subsequent runs.
    Sync,
}
