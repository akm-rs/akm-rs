//! `akm sync` — sync all enabled domains.
//!
//! Bash equivalent: `cmd_sync_all()` at bin/akm:1012.
//!
//! Iterates over enabled features (skills, artifacts, instructions)
//! and runs each domain's sync command. Each domain sync is independent —
//! a failure in one does not prevent others from running.

use crate::config::{Config, Feature};
use crate::error::{Error, Result};
use crate::paths::Paths;

/// Run sync for all enabled domains.
///
/// Bash equivalent: `cmd_sync_all()` at bin/akm:1012
pub fn run(paths: &Paths) -> Result<()> {
    let config = Config::load(paths)?;

    if config.features.is_empty() {
        return Err(Error::NoFeaturesConfigured);
    }

    let mut had_error = false;

    if config.is_feature_enabled(Feature::Skills) {
        println!("==> Skills");
        match crate::commands::skills::sync::run_cli(paths, false) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("Error: {e}");
                had_error = true;
            }
        }
        println!();
    }

    if config.is_feature_enabled(Feature::Artifacts) {
        println!("==> Artifacts");
        match crate::commands::artifacts::sync::run(&config, paths) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("Error: {e}");
                had_error = true;
            }
        }
        println!();
    }

    if config.is_feature_enabled(Feature::Instructions) {
        println!("==> Instructions");
        match crate::commands::instructions::sync::run(paths) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("Error: {e}");
                had_error = true;
            }
        }
        println!();
    }

    if had_error {
        Err(Error::SyncPartialFailure)
    } else {
        Ok(())
    }
}
