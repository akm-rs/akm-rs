//! Integration tests for `akm setup`.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

/// Setup with skills and instructions enabled (artifacts disabled due to no remote)
#[test]
fn test_setup_all_defaults() {
    let dir = TempDir::new().unwrap();
    let data_dir = dir.path().join("data");
    let config_dir = dir.path().join("config");
    let cache_dir = dir.path().join("cache");
    let home_dir = dir.path().join("home");
    std::fs::create_dir_all(&home_dir).unwrap();

    // enable skills (y), use skillverse (y), no personal (n),
    // disable artifacts (n),
    // enable instructions (y)
    let stdin_input = "y\ny\nn\nn\ny\n";

    cargo_bin_cmd!("akm")
        .arg("setup")
        .env("XDG_DATA_HOME", &data_dir)
        .env("XDG_CONFIG_HOME", &config_dir)
        .env("XDG_CACHE_HOME", &cache_dir)
        .env("HOME", &home_dir)
        .write_stdin(stdin_input)
        .assert()
        .success()
        .stdout(predicate::str::contains("Config saved"))
        .stdout(predicate::str::contains("Shell integration installed"));

    // Verify config file was created
    let config_file = config_dir.join("akm").join("config.toml");
    assert!(config_file.exists());
    let config_content = std::fs::read_to_string(&config_file).unwrap();
    assert!(config_content.contains("skills"));
    assert!(config_content.contains("instructions"));

    // Verify shell init was installed
    let shell_init = data_dir.join("akm").join("shell").join("akm-init.sh");
    assert!(shell_init.exists());

    // Verify .bashrc was patched
    let bashrc = home_dir.join(".bashrc");
    assert!(bashrc.exists());
    let bashrc_content = std::fs::read_to_string(&bashrc).unwrap();
    assert!(bashrc_content.contains("# >>> akm >>>"));
    assert!(bashrc_content.contains("# <<< akm <<<"));
}

/// Setup with scoped flags
#[test]
fn test_setup_skills_only() {
    let dir = TempDir::new().unwrap();
    let data_dir = dir.path().join("data");
    let config_dir = dir.path().join("config");
    let home_dir = dir.path().join("home");
    std::fs::create_dir_all(&home_dir).unwrap();

    let stdin_input = "y\ny\nn\n";

    cargo_bin_cmd!("akm")
        .args(["setup", "--skills"])
        .env("XDG_DATA_HOME", &data_dir)
        .env("XDG_CONFIG_HOME", &config_dir)
        .env("HOME", &home_dir)
        .write_stdin(stdin_input)
        .assert()
        .success()
        .stdout(predicate::str::contains("Skills enabled"));
}

/// Re-running setup is idempotent
#[test]
fn test_setup_rerun_idempotent() {
    let dir = TempDir::new().unwrap();
    let home_dir = dir.path().join("home");
    std::fs::create_dir_all(&home_dir).unwrap();
    // Create pre-existing .bashrc
    std::fs::write(home_dir.join(".bashrc"), "# my config\n").unwrap();

    // Run setup twice with same inputs (disable all)
    let stdin_input = "n\nn\nn\n";
    for _ in 0..2 {
        cargo_bin_cmd!("akm")
            .arg("setup")
            .env("XDG_DATA_HOME", dir.path().join("data"))
            .env("XDG_CONFIG_HOME", dir.path().join("config"))
            .env("HOME", &home_dir)
            .write_stdin(stdin_input)
            .assert()
            .success();
    }

    // Verify .bashrc has exactly one marker block
    let content = std::fs::read_to_string(home_dir.join(".bashrc")).unwrap();
    assert_eq!(content.matches("# >>> akm >>>").count(), 1);
}

/// Setup with all disabled still writes config and patches bashrc
#[test]
fn test_setup_all_disabled() {
    let dir = TempDir::new().unwrap();
    let home_dir = dir.path().join("home");
    std::fs::create_dir_all(&home_dir).unwrap();

    let stdin_input = "n\nn\nn\n";

    cargo_bin_cmd!("akm")
        .arg("setup")
        .env("XDG_DATA_HOME", dir.path().join("data"))
        .env("XDG_CONFIG_HOME", dir.path().join("config"))
        .env("HOME", &home_dir)
        .write_stdin(stdin_input)
        .assert()
        .success()
        .stdout(predicate::str::contains("Done! Open a new terminal"));
}
