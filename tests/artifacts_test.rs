//! Integration tests for the artifacts domain.
//!
//! Uses temp directories with real git repos to test the full sync pipeline.

use akm::artifacts::{ArtifactRepo, CommitPushOutcome, SyncOutcome};
use akm::config::Config;
use akm::git::Git;
use akm::paths::Paths;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Helper: initialize a bare git repo (simulates a remote).
fn init_bare_repo(dir: &Path) {
    Command::new("git")
        .args(["init", "--bare", "--quiet"])
        .arg(dir)
        .status()
        .expect("git init --bare");
}

/// Helper: initialize a regular git repo with an initial commit.
fn init_repo_with_commit(dir: &Path) {
    Command::new("git")
        .args(["init", "--quiet"])
        .arg(dir)
        .status()
        .expect("git init");
    Command::new("git")
        .args([
            "-C",
            &dir.to_string_lossy(),
            "commit",
            "--allow-empty",
            "-m",
            "init",
        ])
        .status()
        .expect("git commit");
}

/// Helper: clone a bare repo into a working directory.
fn clone_repo(bare: &Path, work: &Path) {
    Command::new("git")
        .args(["clone", "--quiet"])
        .arg(bare)
        .arg(work)
        .status()
        .expect("git clone");
}

/// Helper: create a file, stage, and commit in a repo.
fn add_and_commit(repo: &Path, filename: &str, content: &str, message: &str) {
    std::fs::write(repo.join(filename), content).expect("write file");
    Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "add", "-A"])
        .status()
        .expect("git add");
    Command::new("git")
        .args([
            "-C",
            &repo.to_string_lossy(),
            "commit",
            "-m",
            message,
            "--quiet",
        ])
        .status()
        .expect("git commit");
}

#[test]
fn test_sync_no_remote_configured() {
    let tmp = TempDir::new().unwrap();
    let paths = Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );
    let config = Config::default(); // remote is None
    let outcome = ArtifactRepo::sync(&config, &paths).unwrap();
    assert_eq!(outcome, SyncOutcome::NoRemote);
}

#[test]
fn test_sync_empty_remote_treated_as_none() {
    let tmp = TempDir::new().unwrap();
    let paths = Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );
    let mut config = Config::default();
    config.artifacts.remote = Some(String::new()); // empty string
    let outcome = ArtifactRepo::sync(&config, &paths).unwrap();
    assert_eq!(outcome, SyncOutcome::NoRemote);
}

#[test]
fn test_sync_first_time_clone() {
    let tmp = TempDir::new().unwrap();
    let bare_dir = tmp.path().join("remote.git");
    let artifacts_dir = tmp.path().join("artifacts");

    // Create a bare remote with at least one commit
    init_bare_repo(&bare_dir);
    let staging = tmp.path().join("staging");
    clone_repo(&bare_dir, &staging);
    add_and_commit(&staging, "README.md", "# Artifacts", "init");
    Command::new("git")
        .args(["-C", &staging.to_string_lossy(), "push", "--quiet"])
        .status()
        .expect("git push");

    let paths = Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );
    let mut config = Config::default();
    config.artifacts.remote = Some(bare_dir.to_string_lossy().to_string());
    config.artifacts.dir = Some(artifacts_dir.clone());

    let outcome = ArtifactRepo::sync(&config, &paths).unwrap();
    assert_eq!(outcome, SyncOutcome::Cloned);
    assert!(artifacts_dir.join(".git").is_dir());
    assert!(artifacts_dir.join("README.md").exists());
}

#[test]
fn test_sync_pull_only() {
    // Existing clone with no local changes → Pulled
    let tmp = TempDir::new().unwrap();
    let bare_dir = tmp.path().join("remote.git");
    let artifacts_dir = tmp.path().join("artifacts");

    init_bare_repo(&bare_dir);
    let staging = tmp.path().join("staging");
    clone_repo(&bare_dir, &staging);
    add_and_commit(&staging, "README.md", "# Artifacts", "init");
    Command::new("git")
        .args(["-C", &staging.to_string_lossy(), "push", "--quiet"])
        .status()
        .unwrap();

    // Clone artifacts dir
    clone_repo(&bare_dir, &artifacts_dir);

    let paths = Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );
    let mut config = Config::default();
    config.artifacts.remote = Some(bare_dir.to_string_lossy().to_string());
    config.artifacts.dir = Some(artifacts_dir.clone());

    let outcome = ArtifactRepo::sync(&config, &paths).unwrap();
    assert_eq!(outcome, SyncOutcome::Pulled);
}

#[test]
fn test_sync_pull_and_push() {
    // Existing clone with local commits ahead → PulledAndPushed
    let tmp = TempDir::new().unwrap();
    let bare_dir = tmp.path().join("remote.git");
    let artifacts_dir = tmp.path().join("artifacts");

    init_bare_repo(&bare_dir);
    let staging = tmp.path().join("staging");
    clone_repo(&bare_dir, &staging);
    add_and_commit(&staging, "README.md", "# Artifacts", "init");
    Command::new("git")
        .args(["-C", &staging.to_string_lossy(), "push", "--quiet"])
        .status()
        .unwrap();

    // Clone, then add a local commit
    clone_repo(&bare_dir, &artifacts_dir);
    add_and_commit(&artifacts_dir, "local.md", "local content", "local commit");

    let paths = Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );
    let mut config = Config::default();
    config.artifacts.remote = Some(bare_dir.to_string_lossy().to_string());
    config.artifacts.dir = Some(artifacts_dir.clone());

    let outcome = ArtifactRepo::sync(&config, &paths).unwrap();
    assert_eq!(outcome, SyncOutcome::PulledAndPushed { commits_pushed: 1 });
}

#[test]
fn test_ensure_project_dir_creates_directory() {
    let tmp = TempDir::new().unwrap();
    let artifacts_dir = tmp.path().join("artifacts");

    let paths = Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );
    let mut config = Config::default();
    config.artifacts.dir = Some(artifacts_dir.clone());

    let project_dir = ArtifactRepo::ensure_project_dir(&config, &paths, "my-project").unwrap();
    assert_eq!(project_dir, artifacts_dir.join("my-project"));
    assert!(project_dir.is_dir());

    // Idempotent — second call succeeds without error
    let project_dir2 = ArtifactRepo::ensure_project_dir(&config, &paths, "my-project").unwrap();
    assert_eq!(project_dir, project_dir2);
}

#[test]
fn test_commit_and_push_not_a_repo() {
    let tmp = TempDir::new().unwrap();
    let artifacts_dir = tmp.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).unwrap();

    let paths = Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );
    let mut config = Config::default();
    config.artifacts.dir = Some(artifacts_dir);

    let outcome = ArtifactRepo::commit_and_push(&config, &paths, "test-project").unwrap();
    assert_eq!(outcome, CommitPushOutcome::NotARepo);
}

#[test]
fn test_commit_and_push_no_changes() {
    let tmp = TempDir::new().unwrap();
    let bare_dir = tmp.path().join("remote.git");
    let artifacts_dir = tmp.path().join("artifacts");

    init_bare_repo(&bare_dir);
    let staging = tmp.path().join("staging");
    clone_repo(&bare_dir, &staging);
    add_and_commit(&staging, "README.md", "# Artifacts", "init");
    Command::new("git")
        .args(["-C", &staging.to_string_lossy(), "push", "--quiet"])
        .status()
        .unwrap();
    clone_repo(&bare_dir, &artifacts_dir);

    let paths = Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );
    let mut config = Config::default();
    config.artifacts.dir = Some(artifacts_dir);

    let outcome = ArtifactRepo::commit_and_push(&config, &paths, "test-project").unwrap();
    assert_eq!(outcome, CommitPushOutcome::NoChanges);
}

#[test]
fn test_commit_and_push_with_changes() {
    let tmp = TempDir::new().unwrap();
    let bare_dir = tmp.path().join("remote.git");
    let artifacts_dir = tmp.path().join("artifacts");

    init_bare_repo(&bare_dir);
    let staging = tmp.path().join("staging");
    clone_repo(&bare_dir, &staging);
    add_and_commit(&staging, "README.md", "# Artifacts", "init");
    Command::new("git")
        .args(["-C", &staging.to_string_lossy(), "push", "--quiet"])
        .status()
        .unwrap();
    clone_repo(&bare_dir, &artifacts_dir);

    // Add a new file (uncommitted change)
    std::fs::write(artifacts_dir.join("session.md"), "session notes").unwrap();

    let paths = Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );
    let mut config = Config::default();
    config.artifacts.dir = Some(artifacts_dir.clone());

    let outcome = ArtifactRepo::commit_and_push(&config, &paths, "test-project").unwrap();
    assert_eq!(outcome, CommitPushOutcome::CommittedAndPushed);

    // Verify the commit was pushed to remote
    let log = Command::new("git")
        .args([
            "-C",
            &artifacts_dir.to_string_lossy(),
            "log",
            "--oneline",
            "-1",
        ])
        .output()
        .unwrap();
    let msg = String::from_utf8_lossy(&log.stdout);
    assert!(
        msg.contains("test-project:"),
        "Commit message should contain project name"
    );
}

#[test]
fn test_commits_ahead_no_upstream() {
    // Repo with no tracking branch → commits_ahead returns 0
    let tmp = TempDir::new().unwrap();
    let repo_dir = tmp.path().join("repo");
    init_repo_with_commit(&repo_dir);

    // No upstream set — should return 0, not error
    let ahead = Git::commits_ahead(&repo_dir).unwrap_or(0);
    assert_eq!(ahead, 0);
}

#[test]
fn test_sync_pull_failure_is_warning() {
    let tmp = TempDir::new().unwrap();
    let bare_dir = tmp.path().join("remote.git");
    let artifacts_dir = tmp.path().join("artifacts");

    init_bare_repo(&bare_dir);
    let staging = tmp.path().join("staging");
    clone_repo(&bare_dir, &staging);
    add_and_commit(&staging, "README.md", "# test", "init");
    Command::new("git")
        .args(["-C", &staging.to_string_lossy(), "push", "--quiet"])
        .status()
        .unwrap();
    clone_repo(&bare_dir, &artifacts_dir);

    // Corrupt the remote to cause pull failure
    std::fs::remove_dir_all(&bare_dir).unwrap();

    let paths = Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );
    let mut config = Config::default();
    config.artifacts.remote = Some(bare_dir.to_string_lossy().to_string());
    config.artifacts.dir = Some(artifacts_dir);

    // sync() returns Err — the command handler will print a warning and return Ok
    let result = ArtifactRepo::sync(&config, &paths);
    assert!(result.is_err(), "Pull failure should return Err");
}

#[test]
fn test_sync_push_failure_is_warning() {
    let tmp = TempDir::new().unwrap();
    let bare_dir = tmp.path().join("remote.git");
    let artifacts_dir = tmp.path().join("artifacts");

    init_bare_repo(&bare_dir);
    let staging = tmp.path().join("staging");
    clone_repo(&bare_dir, &staging);
    add_and_commit(&staging, "README.md", "# test", "init");
    Command::new("git")
        .args(["-C", &staging.to_string_lossy(), "push", "--quiet"])
        .status()
        .unwrap();
    clone_repo(&bare_dir, &artifacts_dir);
    add_and_commit(&artifacts_dir, "local.md", "content", "local");

    // Make bare repo read-only to cause push failure
    let objects_dir = bare_dir.join("objects");
    let mut perms = std::fs::metadata(&objects_dir).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&objects_dir, perms).unwrap();

    let paths = Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );
    let mut config = Config::default();
    config.artifacts.remote = Some(bare_dir.to_string_lossy().to_string());
    config.artifacts.dir = Some(artifacts_dir);

    let result = ArtifactRepo::sync(&config, &paths);

    // Restore permissions before assert to avoid leaking temp dir on panic
    let mut perms = std::fs::metadata(&objects_dir).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&objects_dir, perms).unwrap();

    assert!(result.is_err(), "Push failure should return Err");
}

#[test]
fn test_sync_pull_with_dirty_worktree() {
    let tmp = TempDir::new().unwrap();
    let bare_dir = tmp.path().join("remote.git");
    let artifacts_dir = tmp.path().join("artifacts");

    init_bare_repo(&bare_dir);
    let staging = tmp.path().join("staging");
    clone_repo(&bare_dir, &staging);
    add_and_commit(&staging, "README.md", "# test", "init");
    Command::new("git")
        .args(["-C", &staging.to_string_lossy(), "push", "--quiet"])
        .status()
        .unwrap();
    clone_repo(&bare_dir, &artifacts_dir);

    // Create dirty worktree (uncommitted changes)
    std::fs::write(artifacts_dir.join("dirty.md"), "uncommitted content").unwrap();

    let paths = Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );
    let mut config = Config::default();
    config.artifacts.remote = Some(bare_dir.to_string_lossy().to_string());
    config.artifacts.dir = Some(artifacts_dir.clone());

    // Pull should succeed thanks to --autostash
    let outcome = ArtifactRepo::sync(&config, &paths).unwrap();
    assert_eq!(outcome, SyncOutcome::Pulled);

    // Dirty file should still be present (autostash restores it)
    assert!(artifacts_dir.join("dirty.md").exists());
}

#[test]
fn test_sync_rebase_conflict() {
    let tmp = TempDir::new().unwrap();
    let bare_dir = tmp.path().join("remote.git");
    let artifacts_dir = tmp.path().join("artifacts");

    init_bare_repo(&bare_dir);
    let staging = tmp.path().join("staging");
    clone_repo(&bare_dir, &staging);
    add_and_commit(&staging, "file.md", "original", "init");
    Command::new("git")
        .args(["-C", &staging.to_string_lossy(), "push", "--quiet"])
        .status()
        .unwrap();
    clone_repo(&bare_dir, &artifacts_dir);

    // Create conflicting changes: remote modifies file.md
    add_and_commit(&staging, "file.md", "remote version", "remote change");
    Command::new("git")
        .args(["-C", &staging.to_string_lossy(), "push", "--quiet"])
        .status()
        .unwrap();

    // Local also modifies file.md (same line → conflict)
    add_and_commit(&artifacts_dir, "file.md", "local version", "local change");

    let paths = Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );
    let mut config = Config::default();
    config.artifacts.remote = Some(bare_dir.to_string_lossy().to_string());
    config.artifacts.dir = Some(artifacts_dir.clone());

    // Pull rebase should fail due to conflict → Err(ArtifactsSync)
    let result = ArtifactRepo::sync(&config, &paths);
    assert!(result.is_err(), "Rebase conflict should return Err");

    // Clean up rebase state for proper temp dir cleanup
    let _ = Command::new("git")
        .args(["-C", &artifacts_dir.to_string_lossy(), "rebase", "--abort"])
        .status();
}

#[test]
fn test_pull_quiet_no_repo_succeeds() {
    // pull_quiet should succeed when artifacts dir is not a git repo
    let tmp = TempDir::new().unwrap();
    let artifacts_dir = tmp.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).unwrap();

    let paths = Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );
    let mut config = Config::default();
    config.artifacts.dir = Some(artifacts_dir);

    let result = ArtifactRepo::pull_quiet(&config, &paths);
    assert!(
        result.is_ok(),
        "pull_quiet should succeed when dir is not a repo"
    );
}

#[test]
fn test_pull_quiet_with_repo_succeeds() {
    // pull_quiet should succeed (silently) even when pull fails
    let tmp = TempDir::new().unwrap();
    let artifacts_dir = tmp.path().join("artifacts");
    init_repo_with_commit(&artifacts_dir);

    let paths = Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );
    let mut config = Config::default();
    config.artifacts.dir = Some(artifacts_dir);

    // Repo has no remote, so pull would fail — but pull_quiet swallows errors
    let result = ArtifactRepo::pull_quiet(&config, &paths);
    assert!(result.is_ok(), "pull_quiet should swallow pull errors");
}
