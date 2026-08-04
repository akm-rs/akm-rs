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
personal_registry = "https://github.com/rplsmn/skillfab.git"

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

/// Shared registries are addressed by a name the user chooses, so the key is
/// open-ended where every other config key is one of a fixed set.
#[test]
fn shared_registries_round_trip_and_can_be_removed() {
    let tmp = TempDir::new().unwrap();
    let env_args = [
        ("XDG_CONFIG_HOME", tmp.path().join("config")),
        ("XDG_DATA_HOME", tmp.path().join("data")),
        ("XDG_CACHE_HOME", tmp.path().join("cache")),
        ("HOME", tmp.path().to_path_buf()),
    ];
    let run = |args: &[&str]| {
        let mut cmd = akm_cmd();
        cmd.args(args);
        for (k, v) in &env_args {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    };

    run(&["config", "shared.acme", "git@github.com:acme/skills.git"]);
    let out = run(&["config", "shared.acme"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("git@github.com:acme/skills.git"));

    // Two registries coexist, and both show up in the full listing.
    run(&["config", "shared.oss", "https://github.com/oss/skills.git"]);
    let out = run(&["config"]);
    let listing = String::from_utf8_lossy(&out.stdout);
    assert!(listing.contains("shared.acme"));
    assert!(listing.contains("shared.oss"));

    // An empty value removes one, which is the only way to drop a registry
    // without hand-editing config.toml.
    run(&["config", "shared.acme", ""]);
    let out = run(&["config"]);
    let listing = String::from_utf8_lossy(&out.stdout);
    assert!(!listing.contains("shared.acme"));
    assert!(listing.contains("shared.oss"));
}

#[test]
fn a_shared_key_without_a_name_is_rejected() {
    let tmp = TempDir::new().unwrap();
    akm_cmd()
        .args(["config", "shared.", "url"])
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path())
        .assert()
        .failure();
}
