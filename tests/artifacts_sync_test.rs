//! CLI integration tests for `akm artifacts sync`.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::path::Path;
use tempfile::TempDir;

/// Helper: create a minimal config file.
fn write_config(config_dir: &Path, content: &str) {
    std::fs::create_dir_all(config_dir.join("akm")).unwrap();
    std::fs::write(config_dir.join("akm/config.toml"), content).unwrap();
}

#[test]
fn test_artifacts_sync_no_remote_warns() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    write_config(&config_dir, "features = [\"artifacts\"]\n");

    cargo_bin_cmd!("akm")
        .args(["artifacts", "sync"])
        .env("XDG_CONFIG_HOME", &config_dir)
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("No artifacts remote configured"));
}

#[test]
fn test_artifacts_sync_clones_on_first_run() {
    let tmp = TempDir::new().unwrap();
    let bare_dir = tmp.path().join("remote.git");
    let config_dir = tmp.path().join("config");

    // Create bare remote
    std::process::Command::new("git")
        .args(["init", "--bare", "--quiet"])
        .arg(&bare_dir)
        .status()
        .unwrap();
    let staging = tmp.path().join("staging");
    std::process::Command::new("git")
        .args(["clone", "--quiet"])
        .arg(&bare_dir)
        .arg(&staging)
        .status()
        .unwrap();
    std::fs::write(staging.join("README.md"), "# test").unwrap();
    std::process::Command::new("git")
        .args(["-C", &staging.to_string_lossy(), "add", "-A"])
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-C",
            &staging.to_string_lossy(),
            "commit",
            "-m",
            "init",
            "--quiet",
        ])
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["-C", &staging.to_string_lossy(), "push", "--quiet"])
        .status()
        .unwrap();

    write_config(
        &config_dir,
        &format!(
            "features = [\"artifacts\"]\n\n[artifacts]\nremote = \"{}\"\n",
            bare_dir.display()
        ),
    );

    cargo_bin_cmd!("akm")
        .args(["artifacts", "sync"])
        .env("XDG_CONFIG_HOME", &config_dir)
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path().join("home"))
        .assert()
        .success()
        .stdout(predicate::str::contains("Artifacts cloned to"));
}

#[test]
fn test_artifacts_help() {
    cargo_bin_cmd!("akm")
        .args(["artifacts", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("sync"))
        .stdout(predicate::str::contains("Bidirectional sync"));
}
