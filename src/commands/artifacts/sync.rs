//! `akm artifacts sync` command handler.
//!
//! Performs bidirectional git sync of the artifacts repository:
//! 1. Pull latest changes (rebase + autostash)
//! 2. Push if local commits exist
//! 3. Clone on first run if no local repo
//!
//! Bash: `cmd_artifacts_sync()` (bin/akm:474–518)

use crate::artifacts::{ArtifactRepo, SyncOutcome};
use crate::config::Config;
use crate::error::Result;
use crate::paths::Paths;

/// Execute the `akm artifacts sync` command.
///
/// Output behavior mirrors Bash exactly:
/// - No remote configured → warning to stderr, exit 0
/// - Clone success → "Artifacts cloned to `dir`"
/// - Pull success → "Artifacts pulled"
/// - Push success → "Artifacts pushed (N commits)"
/// - Any failure → warning to stderr, exit 0 (non-fatal)
///
/// Bash: `cmd_artifacts_sync()` (bin/akm:474–518)
pub fn run(config: &Config, paths: &Paths) -> Result<()> {
    match ArtifactRepo::sync(config, paths) {
        Ok(outcome) => {
            match outcome {
                SyncOutcome::NoRemote => {
                    eprintln!(
                        "Warning: No artifacts remote configured. \
                         Run 'akm setup' to configure."
                    );
                }
                SyncOutcome::Cloned => {
                    let dir = config.artifacts_dir(paths);
                    println!("Artifacts cloned to {}", dir.display());
                }
                SyncOutcome::Pulled => {
                    println!("Artifacts pulled");
                }
                SyncOutcome::PulledAndPushed { commits_pushed } => {
                    println!("Artifacts pulled");
                    println!("Artifacts pushed ({commits_pushed} commits)");
                }
            }
            Ok(())
        }
        Err(e) => {
            // All sync errors are non-fatal warnings (matching Bash `return 0`)
            eprintln!("Warning: {e}");
            Ok(())
        }
    }
}
