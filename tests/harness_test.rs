//! Integration tests for `akm harness` — push/pull round trip, secrets excluded.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
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

/// A one-machine home with a config pointing at `origin`.
fn configure(home: &Path, origin: &Path) {
    let config_dir = home.join(".config").join("akm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.toml"),
        format!(
            "features = [\"skills\"]\n\n[skills]\npersonal_registry = \"{}\"\n",
            origin.display()
        ),
    )
    .unwrap();

    // The library directory is the clone target: it must not exist yet.
    let library = home.join(".local/share/akm/library");
    if library.exists() {
        fs::remove_dir_all(&library).unwrap();
    }
}

/// An empty personal registry the machines share.
fn setup_origin(tmp: &TempDir) -> std::path::PathBuf {
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
    origin
}

fn akm(home: &Path) -> assert_cmd::Command {
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

/// Read a path from the origin's `main` branch; `None` if it does not exist.
fn show_on_origin(origin: &Path, path: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["show", &format!("main:{path}")])
        .current_dir(origin)
        .output()
        .unwrap();
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

#[test]
fn harness_push_pull_round_trip_excludes_secrets() {
    let tmp = TempDir::new().unwrap();
    let origin = setup_origin(&tmp);

    // --- Machine 1: capture and push Pi's theme, never the secret. ---
    let home1 = tmp.path().join("home1");
    fs::create_dir_all(&home1).unwrap();
    configure(&home1, &origin);
    akm(&home1).args(["skills", "sync"]).assert().success();

    let pi1 = home1.join(".pi").join("agent");
    fs::create_dir_all(&pi1).unwrap();
    fs::write(pi1.join("theme.json"), "gold").unwrap();
    fs::write(pi1.join("auth.json"), "SECRET").unwrap();

    akm(&home1)
        .args(["harness", "push", "pi"])
        .assert()
        .success()
        .stdout(predicate::str::contains("pi:").and(predicate::str::contains("pushed")));

    // theme.json rode to the registry; auth.json never did.
    assert_eq!(
        show_on_origin(&origin, "harnesses/pi/theme.json").as_deref(),
        Some("gold")
    );
    assert!(show_on_origin(&origin, "harnesses/pi/auth.json").is_none());

    // --- Machine 2: a fresh clone applies the theme, not the secret. ---
    let home2 = tmp.path().join("home2");
    fs::create_dir_all(&home2).unwrap();
    configure(&home2, &origin);
    akm(&home2).args(["skills", "sync"]).assert().success();

    akm(&home2)
        .args(["harness", "pull", "pi"])
        .assert()
        .success()
        .stdout(predicate::str::contains("pi: applied"));

    let pi2 = home2.join(".pi").join("agent");
    assert_eq!(fs::read_to_string(pi2.join("theme.json")).unwrap(), "gold");
    assert!(!pi2.join("auth.json").exists());

    // --- status: pi is level with the registry on machine 2. ---
    akm(&home2)
        .args(["harness", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("pi").and(predicate::str::contains("clean")));
}
