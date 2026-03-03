//! Integration tests for `akm sync`.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

/// Sync with no config shows error
#[test]
fn test_sync_no_config() {
    let dir = TempDir::new().unwrap();

    cargo_bin_cmd!("akm")
        .arg("sync")
        .env("XDG_DATA_HOME", dir.path().join("data"))
        .env("XDG_CONFIG_HOME", dir.path().join("config"))
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("No features configured"));
}

/// Sync with empty features configured
#[test]
fn test_sync_empty_features() {
    let dir = TempDir::new().unwrap();
    let config_dir = dir.path().join("config").join("akm");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("config.toml"), "features = []\n").unwrap();

    cargo_bin_cmd!("akm")
        .arg("sync")
        .env("XDG_DATA_HOME", dir.path().join("data"))
        .env("XDG_CONFIG_HOME", dir.path().join("config"))
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("No features configured"));
}
