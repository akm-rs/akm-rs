//! Integration tests for shell completions.
//!
//! Tests the `akm completions <shell>` CLI command output and validates
//! the registration scripts are correct.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

/// Helper: create an AKM command with XDG overrides.
fn akm_cmd(dir: &TempDir) -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("akm");
    cmd.env("XDG_DATA_HOME", dir.path().join("data"));
    cmd.env("XDG_CONFIG_HOME", dir.path().join("config"));
    cmd.env("XDG_CACHE_HOME", dir.path().join("cache"));
    cmd.env("HOME", dir.path());
    cmd
}

#[test]
fn test_completions_bash_outputs_registration_script() {
    let dir = TempDir::new().unwrap();
    akm_cmd(&dir)
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("COMPLETE=bash akm"))
        .stdout(predicate::str::contains("eval"));
}

#[test]
fn test_completions_zsh_outputs_registration_script() {
    let dir = TempDir::new().unwrap();
    akm_cmd(&dir)
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("COMPLETE=zsh akm"))
        .stdout(predicate::str::contains("#compdef akm"));
}

#[test]
fn test_completions_fish_outputs_registration_script() {
    let dir = TempDir::new().unwrap();
    akm_cmd(&dir)
        .args(["completions", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("COMPLETE=fish akm"))
        .stdout(predicate::str::contains("source"));
}

#[test]
fn test_completions_invalid_shell_fails() {
    let dir = TempDir::new().unwrap();
    akm_cmd(&dir)
        .args(["completions", "powershell"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("possible values: bash, zsh, fish")
                .or(predicate::str::contains("invalid value")),
        );
}

#[test]
fn test_completions_missing_shell_arg_shows_usage() {
    let dir = TempDir::new().unwrap();
    akm_cmd(&dir)
        .args(["completions"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_completions_help_shows_shells() {
    let dir = TempDir::new().unwrap();
    akm_cmd(&dir)
        .args(["completions", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("bash"))
        .stdout(predicate::str::contains("zsh"))
        .stdout(predicate::str::contains("fish"));
}

// --- Snapshot tests ---

#[test]
fn test_completions_bash_snapshot() {
    let dir = TempDir::new().unwrap();
    let output = akm_cmd(&dir)
        .args(["completions", "bash"])
        .output()
        .unwrap();
    let script = String::from_utf8(output.stdout).unwrap();
    insta::assert_snapshot!("completions_bash", script);
}

#[test]
fn test_completions_zsh_snapshot() {
    let dir = TempDir::new().unwrap();
    let output = akm_cmd(&dir).args(["completions", "zsh"]).output().unwrap();
    let script = String::from_utf8(output.stdout).unwrap();
    insta::assert_snapshot!("completions_zsh", script);
}

#[test]
fn test_completions_fish_snapshot() {
    let dir = TempDir::new().unwrap();
    let output = akm_cmd(&dir)
        .args(["completions", "fish"])
        .output()
        .unwrap();
    let script = String::from_utf8(output.stdout).unwrap();
    insta::assert_snapshot!("completions_fish", script);
}

// --- CompleteEnv integration tests ---

/// Test that the CompleteEnv handler responds to COMPLETE env var.
///
/// Simulates what happens when the user presses Tab: the shell's registration
/// script sets COMPLETE=bash and invokes akm with the current words.
#[test]
fn test_complete_env_responds_to_completion_request() {
    let dir = TempDir::new().unwrap();
    // Set up a minimal library for dynamic completions
    let data_dir = dir.path().join("data").join("akm");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::write(
        data_dir.join("library.json"),
        r#"{"version": 1, "specs": [{"id": "tdd", "type": "skill", "name": "TDD", "description": "TDD workflow"}]}"#,
    )
    .unwrap();

    // Simulate Tab completion: COMPLETE=bash akm skills add <cursor>
    let output = akm_cmd(&dir)
        .env("COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", "3")
        .args(["--", "akm", "skills", "add", ""])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.is_empty(),
        "CompleteEnv should produce completion output"
    );
}

/// Test that CompleteEnv works for subcommand completion.
#[test]
fn test_complete_env_suggests_subcommands() {
    let dir = TempDir::new().unwrap();

    // Simulate: akm <Tab>
    let output = akm_cmd(&dir)
        .env("COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", "1")
        .args(["--", "akm", ""])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("setup") || stdout.contains("skills") || stdout.contains("config"),
        "CompleteEnv should suggest subcommands, got: {stdout}"
    );
}
