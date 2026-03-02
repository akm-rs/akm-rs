//! `akm config [key] [value]` — view, get, or set configuration.
//!
//! Bash equivalent: `cmd_config()` at bin/akm:877.

use crate::config::{Config, ConfigKey};
use crate::error::Result;
use crate::paths::Paths;

/// Run the `akm config` command.
///
/// - No args → print all config
/// - One arg → get the value of a key
/// - Two args → set key=value, validate, and write
pub fn run(paths: &Paths, key: Option<String>, value: Option<String>) -> Result<()> {
    let mut config = Config::load(paths)?;

    match (key, value) {
        // No args: print all
        (None, _) => {
            print_all(paths, &config);
            Ok(())
        }
        // Get mode
        (Some(key_str), None) => {
            let config_key: ConfigKey = key_str.parse()?;
            let value = config_key.get(&config);
            if value.is_empty() {
                println!("(not set)");
            } else {
                println!("{value}");
            }
            Ok(())
        }
        // Set mode
        (Some(key_str), Some(val)) => {
            let config_key: ConfigKey = key_str.parse()?;
            config_key.set(&mut config, &val)?;
            config.save(paths)?;
            println!("Set {} = {}", key_str, val);
            Ok(())
        }
    }
}

/// Print all config values.
///
/// Bash equivalent: `_config_print_all()` at bin/akm:957.
fn print_all(paths: &Paths, config: &Config) {
    let config_file = paths.config_file();
    if !config_file.exists() {
        println!("No config file found. Run 'akm setup' to create one.");
        return;
    }

    println!("AKM Config ({})", config_file.display());
    println!();

    let features_str: String = config
        .features
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join(",");

    println!(
        "  features                  = {}",
        if features_str.is_empty() {
            "(not set)".to_string()
        } else {
            features_str
        }
    );

    println!(
        "  skills.community-registry = {}",
        if config.community_registry_is_explicit() {
            config.community_registry_url().to_string()
        } else {
            format!("{} (default)", config.community_registry_url())
        }
    );

    println!(
        "  skills.personal-registry  = {}",
        config
            .skills
            .personal_registry
            .as_deref()
            .unwrap_or("(not set)")
    );

    println!(
        "  artifacts.remote          = {}",
        config.artifacts.remote.as_deref().unwrap_or("(not set)")
    );

    println!(
        "  artifacts.dir             = {}",
        config
            .artifacts
            .dir
            .as_ref()
            .map(|p| p.display().to_string())
            .as_deref()
            .unwrap_or("(not set)")
    );

    println!(
        "  artifacts.auto-push       = {}",
        config.artifacts.auto_push
    );
}
