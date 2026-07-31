//! Integration tests for `akm instructions` commands.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Helper: create a minimal AKM environment in a temp dir.
///
/// Returns `(home, config_dir, instructions_file)`. Global instructions live
/// inside the registry working tree, so the third element points at
/// `~/.local/share/akm/library/instructions/global.md`.
fn setup_env(tmp: &TempDir) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let home = tmp.path().join("home");
    let config_dir = home.join(".config").join("akm");
    let instructions_dir = home
        .join(".local/share/akm")
        .join("library")
        .join("instructions");

    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&instructions_dir).unwrap();

    // Minimal config with instructions enabled
    fs::write(
        config_dir.join("config.toml"),
        "[features]\nenabled = [\"instructions\"]\n",
    )
    .unwrap();

    (home, config_dir, instructions_dir.join("global.md"))
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
    let (home, _, instructions) = setup_env(&tmp);

    // Create source file
    fs::write(&instructions, "Be concise.").unwrap();

    cargo_bin_cmd!("akm")
        .args(["instructions", "sync"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .assert()
        .success()
        .stdout(predicate::str::contains("5 tool directories"));

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
    assert_eq!(
        fs::read_to_string(home.join(".pi/agent/AGENTS.md")).unwrap(),
        "Be concise."
    );
}

#[test]
fn instructions_sync_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let (home, _, instructions) = setup_env(&tmp);

    fs::write(&instructions, "content").unwrap();

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
    let (home, _, instructions) = setup_env(&tmp);
    fs::write(&instructions, "test").unwrap();

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

// --- Registry-hosted instructions ---

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A personal registry with one skill, plus a config pointing at it.
fn setup_registry(tmp: &TempDir, home: &std::path::Path) -> std::path::PathBuf {
    let origin = tmp.path().join("origin");
    fs::create_dir_all(origin.join("skills").join("alpha")).unwrap();
    fs::write(
        origin.join("skills").join("alpha").join("SKILL.md"),
        "---\nname: alpha\ndescription: desc\n---\nbody\n",
    )
    .unwrap();
    git(&origin, &["init", "-b", "main"]);
    git(&origin, &["config", "user.email", "test@example.com"]);
    git(&origin, &["config", "user.name", "Test"]);
    git(&origin, &["config", "receive.denyCurrentBranch", "ignore"]);
    git(&origin, &["add", "-A"]);
    git(&origin, &["commit", "-m", "initial"]);

    fs::write(
        home.join(".config").join("akm").join("config.toml"),
        format!(
            "features = [\"skills\", \"instructions\"]\n\n[skills]\npersonal_registry = \"{}\"\n",
            origin.display()
        ),
    )
    .unwrap();

    // The library directory is the clone target: it must not exist yet.
    let library = home.join(".local/share/akm/library");
    if library.exists() {
        fs::remove_dir_all(&library).unwrap();
    }

    origin
}

fn akm(home: &std::path::Path) -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("akm");
    cmd.env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com");
    cmd
}

/// An rc3 machine keeps what it wrote: the old bare file is carried into the
/// registry rather than silently replaced by an empty one.
#[test]
fn instructions_sync_seeds_from_the_pre_rc4_location() {
    let tmp = TempDir::new().unwrap();
    let (home, _, instructions) = setup_env(&tmp);

    let legacy = home.join(".akm").join("global-instructions.md");
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    fs::write(&legacy, "Written on rc3.").unwrap();
    fs::remove_dir_all(instructions.parent().unwrap()).unwrap();

    akm(&home)
        .args(["instructions", "sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("personal registry"))
        .stdout(predicate::str::contains("5 tool directories"));

    assert_eq!(
        fs::read_to_string(&instructions).unwrap(),
        "Written on rc3."
    );
    assert_eq!(
        fs::read_to_string(home.join(".claude/CLAUDE.md")).unwrap(),
        "Written on rc3."
    );
    // The old file is left alone.
    assert!(legacy.is_file());
}

#[test]
fn instructions_publish_pushes_to_the_registry() {
    let tmp = TempDir::new().unwrap();
    let (home, _, instructions) = setup_env(&tmp);
    let origin = setup_registry(&tmp, &home);

    akm(&home).args(["skills", "sync"]).assert().success();

    fs::create_dir_all(instructions.parent().unwrap()).unwrap();
    fs::write(&instructions, "Be concise.\n").unwrap();

    akm(&home)
        .args(["instructions", "publish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Published global instructions"));

    // The push landed on the origin's branch.
    let out = std::process::Command::new("git")
        .args(["show", "main:instructions/global.md"])
        .current_dir(&origin)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Be concise.\n");
}

#[test]
fn instructions_publish_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let (home, _, instructions) = setup_env(&tmp);
    setup_registry(&tmp, &home);

    akm(&home).args(["skills", "sync"]).assert().success();
    fs::create_dir_all(instructions.parent().unwrap()).unwrap();
    fs::write(&instructions, "Be concise.\n").unwrap();

    akm(&home)
        .args(["instructions", "publish"])
        .assert()
        .success();
    akm(&home)
        .args(["instructions", "publish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No changes to publish"));
}

#[test]
fn instructions_publish_needs_a_registry() {
    let tmp = TempDir::new().unwrap();
    let (home, _, _) = setup_env(&tmp);

    akm(&home)
        .args(["instructions", "publish"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No personal registry configured"));
}
