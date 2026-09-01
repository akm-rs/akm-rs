//! `akm harness status` — drift of each harness config vs the registry.

use crate::commands::harness::live_dir;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::library::harness_config::builtin_harness_configs;
use crate::paths::Paths;
use crate::registry::Registry;

/// Run `akm harness status`.
pub fn run(paths: &Paths, config: &Config) -> Result<()> {
    let url = config.registry_url().ok_or(Error::NoPersonalRegistry)?;
    let registry = Registry::new(url, paths.library_dir());
    let drift = if registry.is_cloned() {
        registry.refresh().ok();
        registry.drift().unwrap_or_default()
    } else {
        Default::default()
    };

    for def in builtin_harness_configs() {
        let cmd = &def.command;
        let present = live_dir(paths, cmd).map(|d| d.is_dir()).unwrap_or(false);
        let state = drift.harness(cmd);
        let note = if present { "" } else { " (no local config)" };
        println!("  {} {:<10} {}{}", state.marker(), cmd, state, note);
    }
    Ok(())
}
