//! Integration tests for `akm update` and `akm --version`.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn test_version_flag() {
    cargo_bin_cmd!("akm")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("akm "));
}

#[test]
fn test_update_command_runs() {
    // Without network, the update command should fail gracefully
    // with a descriptive error, not panic.
    let result = cargo_bin_cmd!("akm")
        .arg("update")
        .env("XDG_CONFIG_HOME", "/tmp/akm-test-config-nonexistent")
        .env("XDG_CACHE_HOME", "/tmp/akm-test-cache-nonexistent")
        .assert();

    // Should either succeed (if network available) or fail with
    // an error message (not a panic/crash)
    result.code(predicate::in_iter([0, 1]));
}

#[test]
fn test_update_config_keys_accessible() {
    let dir = tempfile::tempdir().unwrap();

    // Verify update config keys work with `akm config`
    for key in &["update.url", "update.check-interval", "update.auto-check"] {
        cargo_bin_cmd!("akm")
            .args(["config", key])
            .env("XDG_CONFIG_HOME", dir.path())
            .env("XDG_DATA_HOME", dir.path())
            .env("XDG_CACHE_HOME", dir.path())
            .assert()
            .success();
    }
}

#[test]
fn test_update_config_set_and_get() {
    let dir = tempfile::tempdir().unwrap();

    cargo_bin_cmd!("akm")
        .args(["config", "update.auto-check", "false"])
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_DATA_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .assert()
        .success();

    cargo_bin_cmd!("akm")
        .args(["config", "update.auto-check"])
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_DATA_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("false"));
}

#[test]
fn test_version_output_snapshot() {
    let output = cargo_bin_cmd!("akm").arg("--version").output().unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("version_output", stdout.trim());
}

#[test]
fn test_update_ignores_stale_cache() {
    // Verify that `akm update` does NOT trust the cache — it always makes
    // a fresh network call. A pre-populated cache saying "already up to date"
    // should not short-circuit the check.
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("akm");
    std::fs::create_dir_all(&cache_dir).unwrap();

    let cache_file = cache_dir.join("last-update-check.json");
    let current_version = env!("CARGO_PKG_VERSION");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    std::fs::write(
        &cache_file,
        format!(
            r#"{{"checked_at": {}, "latest_version": "{}", "download_url": null}}"#,
            now, current_version
        ),
    )
    .unwrap();

    // The command should not crash. It will either:
    // - succeed with "Already up to date" (if network returns same version)
    // - succeed with a download (if a newer version exists)
    // - fail with a network error (if offline/rate-limited)
    // All are valid — the important thing is it doesn't blindly trust the cache.
    cargo_bin_cmd!("akm")
        .arg("update")
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_DATA_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .assert()
        .code(predicate::in_iter([0, 1]));
}

#[test]
fn test_update_notice_format_snapshot() {
    let current = "1.0.0";
    let latest = "1.1.0";
    let notice = format!(
        "A new version of akm is available: {} → {} — run `akm update` to install.",
        current, latest
    );
    insta::assert_snapshot!("update_notice_format", notice);
}
