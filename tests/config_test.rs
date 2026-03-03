use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

fn akm_cmd() -> assert_cmd::Command {
    cargo_bin_cmd!("akm")
}

#[test]
fn config_no_args_no_file_shows_message() {
    let tmp = TempDir::new().unwrap();
    akm_cmd()
        .arg("config")
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No config file"));
}

#[test]
fn config_set_and_get_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let env_args = [
        ("XDG_CONFIG_HOME", tmp.path().join("config")),
        ("XDG_DATA_HOME", tmp.path().join("data")),
        ("XDG_CACHE_HOME", tmp.path().join("cache")),
        ("HOME", tmp.path().to_path_buf()),
    ];

    // Set
    let mut cmd = akm_cmd();
    cmd.args(["config", "artifacts.auto-push", "false"]);
    for (k, v) in &env_args {
        cmd.env(k, v);
    }
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Set"));

    // Get
    let mut cmd = akm_cmd();
    cmd.args(["config", "artifacts.auto-push"]);
    for (k, v) in &env_args {
        cmd.env(k, v);
    }
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("false"));
}

#[test]
fn config_unknown_key_fails() {
    let tmp = TempDir::new().unwrap();
    akm_cmd()
        .args(["config", "nonexistent"])
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown config key"));
}

#[test]
fn config_invalid_auto_push_value_fails() {
    let tmp = TempDir::new().unwrap();
    akm_cmd()
        .args(["config", "artifacts.auto-push", "maybe"])
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path())
        .assert()
        .failure();
}

#[test]
fn config_print_all_snapshot() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config").join("akm");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        r#"
features = ["skills", "artifacts"]

[skills]
community_registry = "https://github.com/akm-rs/skillverse.git"

[artifacts]
auto_push = true
"#,
    )
    .unwrap();

    let output = akm_cmd()
        .arg("config")
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    // Normalize the config file path for snapshot stability
    let normalized = stdout.replace(&config_dir.display().to_string(), "<CONFIG_DIR>");
    insta::assert_snapshot!(normalized);
}
