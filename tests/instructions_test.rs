//! Integration tests for `akm instructions` commands.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Helper: create a minimal AKM environment in a temp dir.
fn setup_env(tmp: &TempDir) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let home = tmp.path().join("home");
    let config_dir = home.join(".config").join("akm");
    let akm_home = home.join(".akm");

    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&akm_home).unwrap();

    // Minimal config with instructions enabled
    fs::write(
        config_dir.join("config.toml"),
        "[features]\nenabled = [\"instructions\"]\n",
    )
    .unwrap();

    (home, config_dir, akm_home)
}

#[test]
fn instructions_sync_warns_when_no_source_file() {
    let tmp = TempDir::new().unwrap();
    let (home, _, _) = setup_env(&tmp);

    cargo_bin_cmd!("akm")
        .args(["instructions", "sync"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "No global instructions file found",
        ));
}

#[test]
fn instructions_sync_distributes_to_all_tool_dirs() {
    let tmp = TempDir::new().unwrap();
    let (home, _, akm_home) = setup_env(&tmp);

    // Create source file
    fs::write(akm_home.join("global-instructions.md"), "Be concise.").unwrap();

    cargo_bin_cmd!("akm")
        .args(["instructions", "sync"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .assert()
        .success()
        .stdout(predicate::str::contains("4 tool directories"));

    // Verify each target
    assert_eq!(
        fs::read_to_string(home.join(".claude/CLAUDE.md")).unwrap(),
        "Be concise."
    );
    assert_eq!(
        fs::read_to_string(home.join(".copilot/copilot-instructions.md")).unwrap(),
        "Be concise."
    );
    assert_eq!(
        fs::read_to_string(home.join(".vibe/prompts/cli.md")).unwrap(),
        "Be concise."
    );
    assert_eq!(
        fs::read_to_string(home.join(".agents/AGENTS.md")).unwrap(),
        "Be concise."
    );
}

#[test]
fn instructions_sync_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let (home, _, akm_home) = setup_env(&tmp);

    fs::write(akm_home.join("global-instructions.md"), "content").unwrap();

    for _ in 0..3 {
        cargo_bin_cmd!("akm")
            .args(["instructions", "sync"])
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .assert()
            .success();
    }

    assert_eq!(
        fs::read_to_string(home.join(".claude/CLAUDE.md")).unwrap(),
        "content"
    );
}

#[test]
fn instructions_scaffold_requires_git_repo() {
    let tmp = TempDir::new().unwrap();
    let (home, _, _) = setup_env(&tmp);

    // Not in a git repo
    cargo_bin_cmd!("akm")
        .args(["instructions", "scaffold-project"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("git repository").or(predicate::str::contains("Git")));
}

#[test]
fn instructions_scaffold_creates_files_in_git_repo() {
    let tmp = TempDir::new().unwrap();
    let (home, _, _) = setup_env(&tmp);

    // Init a git repo
    let repo = tmp.path().join("myproject");
    fs::create_dir_all(&repo).unwrap();
    std::process::Command::new("git")
        .args(["init", "--quiet", "-b", "main"])
        .current_dir(&repo)
        .status()
        .unwrap();

    cargo_bin_cmd!("akm")
        .args(["instructions", "scaffold-project"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .current_dir(&repo)
        .assert()
        .success()
        .stdout(predicate::str::contains("Created AGENTS.md"))
        .stdout(predicate::str::contains("Created CLAUDE.md"));

    assert!(repo.join("AGENTS.md").exists());
    assert!(repo.join("CLAUDE.md").exists());
}

#[test]
fn instructions_scaffold_skips_existing_files() {
    let tmp = TempDir::new().unwrap();
    let (home, _, _) = setup_env(&tmp);

    let repo = tmp.path().join("myproject");
    fs::create_dir_all(&repo).unwrap();
    std::process::Command::new("git")
        .args(["init", "--quiet", "-b", "main"])
        .current_dir(&repo)
        .status()
        .unwrap();

    // Pre-create both files
    fs::write(repo.join("AGENTS.md"), "existing agents").unwrap();
    fs::write(repo.join("CLAUDE.md"), "existing claude").unwrap();

    cargo_bin_cmd!("akm")
        .args(["instructions", "scaffold-project"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .current_dir(&repo)
        .assert()
        .success()
        .stdout(predicate::str::contains("already exists"));

    // Files should NOT be overwritten
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.md")).unwrap(),
        "existing agents"
    );
    assert_eq!(
        fs::read_to_string(repo.join("CLAUDE.md")).unwrap(),
        "existing claude"
    );
}

#[test]
fn instructions_edit_fails_with_bad_editor() {
    let tmp = TempDir::new().unwrap();
    let (home, _, _) = setup_env(&tmp);

    cargo_bin_cmd!("akm")
        .args(["instructions", "edit"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("EDITOR", "/nonexistent/editor")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found").or(predicate::str::contains("Editor")));
}

// --- Snapshot tests ---

#[test]
fn snapshot_instructions_sync_no_source() {
    let tmp = TempDir::new().unwrap();
    let (home, _, _) = setup_env(&tmp);

    let output = cargo_bin_cmd!("akm")
        .args(["instructions", "sync"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Normalize the temp path for stable snapshots
    let normalized = stderr.replace(home.to_str().unwrap(), "$HOME");
    insta::assert_snapshot!("instructions_sync_no_source", normalized);
}

#[test]
fn snapshot_instructions_sync_success() {
    let tmp = TempDir::new().unwrap();
    let (home, _, akm_home) = setup_env(&tmp);
    fs::write(akm_home.join("global-instructions.md"), "test").unwrap();

    let output = cargo_bin_cmd!("akm")
        .args(["instructions", "sync"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("instructions_sync_success", stdout.to_string());
}

#[test]
fn snapshot_instructions_scaffold_fresh() {
    let tmp = TempDir::new().unwrap();
    let (home, _, _) = setup_env(&tmp);

    let repo = tmp.path().join("myproject");
    fs::create_dir_all(&repo).unwrap();
    std::process::Command::new("git")
        .args(["init", "--quiet", "-b", "main"])
        .current_dir(&repo)
        .status()
        .unwrap();

    let output = cargo_bin_cmd!("akm")
        .args(["instructions", "scaffold-project"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .current_dir(&repo)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Replace temp path to make snapshot stable
    let normalized = stdout.replace(repo.to_str().unwrap(), "/path/to/myproject");
    insta::assert_snapshot!("instructions_scaffold_fresh", normalized);
}
