//! Integration tests for TUI commands with --plain flag and piped output.
//!
//! Tests verify that:
//! 1. `--plain` always produces plain text output
//! 2. Non-TTY (piped) output falls back to plain mode
//! 3. No ANSI escape codes from ratatui leak into plain output

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains as pred_contains;
use tempfile::TempDir;

/// Set up a temp environment with a test library.
fn setup_env() -> TempDir {
    let tmp = TempDir::new().unwrap();

    // Create necessary directories
    let data_dir = tmp.path().join("data").join("akm");
    std::fs::create_dir_all(data_dir.join("skills").join("tdd")).unwrap();
    std::fs::create_dir_all(data_dir.join("agents")).unwrap();

    // Write SKILL.md for tdd
    std::fs::write(
        data_dir.join("skills").join("tdd").join("SKILL.md"),
        "---\nname: TDD\ndescription: Test-driven development\n---\nContent",
    )
    .unwrap();

    // Write agent file
    std::fs::write(
        data_dir.join("agents").join("reviewer.md"),
        "---\nname: Reviewer\ndescription: Code review agent\n---\nAgent content",
    )
    .unwrap();

    // Write library.json
    let library_json = serde_json::json!({
        "version": 1,
        "specs": [
            {
                "id": "tdd",
                "type": "skill",
                "name": "TDD",
                "description": "Test-driven development",
                "core": true,
                "tags": ["testing", "methodology"],
                "triggers": {}
            },
            {
                "id": "reviewer",
                "type": "agent",
                "name": "Reviewer",
                "description": "Code review agent",
                "core": false,
                "tags": [],
                "triggers": {}
            },
            {
                "id": "debug",
                "type": "skill",
                "name": "Debug",
                "description": "Debugging helper",
                "core": false,
                "tags": ["debugging"],
                "triggers": {}
            }
        ]
    });
    std::fs::write(
        data_dir.join("library.json"),
        serde_json::to_string_pretty(&library_json).unwrap(),
    )
    .unwrap();

    tmp
}

fn base_cmd(tmp: &TempDir) -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("akm");
    cmd.env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path().join("home"))
        .env_remove("AKM_SESSION");
    cmd
}

#[test]
fn test_skills_list_plain_flag() {
    let tmp = setup_env();
    base_cmd(&tmp)
        .args(["skills", "list", "--plain"])
        .assert()
        .success()
        .stdout(pred_contains("tdd"))
        .stdout(pred_contains("reviewer"))
        .stdout(pred_contains("debug"));
}

#[test]
fn test_skills_list_piped_falls_back_to_plain() {
    // assert_cmd runs without a TTY by default, so this tests non-TTY fallback
    let tmp = setup_env();
    let output = base_cmd(&tmp).args(["skills", "list"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should contain spec data (plain mode)
    assert!(stdout.contains("tdd"));
    // Should NOT contain ratatui ANSI escape sequences
    assert!(
        !stdout.contains("\x1b["),
        "Plain/piped output should not contain ANSI escape codes"
    );
}

#[test]
fn test_skills_list_plain_type_filter() {
    let tmp = setup_env();
    let output = base_cmd(&tmp)
        .args(["skills", "list", "--plain", "--type", "agent"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("reviewer"));
    assert!(!stdout.contains("tdd"));
}

#[test]
fn test_skills_list_plain_tag_filter() {
    let tmp = setup_env();
    let output = base_cmd(&tmp)
        .args(["skills", "list", "--plain", "--tag", "testing"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("tdd"));
    assert!(!stdout.contains("reviewer"));
}

#[test]
fn test_skills_search_plain_flag() {
    let tmp = setup_env();
    base_cmd(&tmp)
        .args(["skills", "search", "tdd", "--plain"])
        .assert()
        .success()
        .stdout(pred_contains("tdd"));
}

#[test]
fn test_skills_search_piped_falls_back_to_plain() {
    let tmp = setup_env();
    let output = base_cmd(&tmp)
        .args(["skills", "search", "review"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("reviewer"));
    assert!(
        !stdout.contains("\x1b["),
        "Piped search output should not contain ANSI escape codes"
    );
}

#[test]
fn test_skills_status_plain_flag() {
    let tmp = setup_env();
    base_cmd(&tmp)
        .args(["skills", "status", "--plain"])
        .assert()
        .success()
        .stdout(pred_contains("Core specs"))
        .stdout(pred_contains("tdd"));
}

#[test]
fn test_skills_status_piped_falls_back_to_plain() {
    let tmp = setup_env();
    let output = base_cmd(&tmp).args(["skills", "status"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Core specs"));
    assert!(
        !stdout.contains("\x1b["),
        "Piped status output should not contain ANSI escape codes"
    );
}

#[test]
fn test_skills_list_plain_shows_core_marker() {
    let tmp = setup_env();
    let output = base_cmd(&tmp)
        .args(["skills", "list", "--plain"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // tdd is core=true, should show [CORE]
    assert!(
        stdout.contains("[CORE]"),
        "Core specs should show [CORE] marker in plain output"
    );
}
