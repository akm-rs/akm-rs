//! `akm update` — download and install the latest version.
//!
//! New in the Rust version. Replaces the Bash `cmd_update()` which
//! ran `git pull` on the AKM_REPO clone.

use crate::config::Config;
use crate::error::Result;
use crate::paths::Paths;
use crate::update::download;
use crate::update::{normalize_version, CURRENT_VERSION};

/// Run the `akm update` command.
///
/// Checks for a newer version and downloads it if available.
/// The binary is replaced atomically — if anything goes wrong,
/// the current binary is left untouched.
pub fn run(paths: &Paths, config: &Config) -> Result<()> {
    println!(
        "akm {} — checking for updates...",
        normalize_version(CURRENT_VERSION)
    );
    download::download_and_replace(&config.update, paths)
}
