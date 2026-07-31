//! Drift detection against real git repositories.
//!
//! The unit tests in `library::drift` cover path classification from synthetic
//! path sets; these check that what git actually reports lands in the right
//! cell of the drift table.

use akm::git::Git;
use akm::library::drift::{DriftReport, DriftState};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
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
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

struct Fixture {
    _tmp: TempDir,
    origin: PathBuf,
    library: PathBuf,
}

/// A registry with four skills, cloned into a library working tree.
fn fixture() -> Fixture {
    let tmp = TempDir::new().unwrap();
    let origin = tmp.path().join("origin");
    std::fs::create_dir_all(&origin).unwrap();

    git(&origin, &["init", "-b", "main"]);
    git(&origin, &["config", "user.email", "test@example.com"]);
    git(&origin, &["config", "user.name", "Test"]);
    for id in ["clean", "local", "remote", "both"] {
        write(&origin.join(format!("skills/{id}/SKILL.md")), "v1\n");
    }
    write(&origin.join("instructions/global.md"), "be concise\n");
    write(&origin.join(".gitignore"), "library.json\n");
    git(&origin, &["add", "-A"]);
    git(&origin, &["commit", "-m", "initial"]);

    let library = tmp.path().join("library");
    git(
        tmp.path(),
        &[
            "clone",
            "--quiet",
            &origin.to_string_lossy(),
            &library.to_string_lossy(),
        ],
    );

    Fixture {
        _tmp: tmp,
        origin,
        library,
    }
}

fn commit_to_origin(fx: &Fixture, rel: &str, content: &str) {
    write(&fx.origin.join(rel), content);
    git(&fx.origin, &["add", "-A"]);
    git(&fx.origin, &["commit", "-m", &format!("update {rel}")]);
}

#[test]
fn a_fresh_clone_has_no_drift() {
    let fx = fixture();
    let report = DriftReport::compute(&fx.library).unwrap();
    assert!(
        report.is_clean(),
        "{:?}",
        report.drifted().collect::<Vec<_>>()
    );
}

/// The full drift table, in one tree: a local edit, a remote commit, both on
/// the same spec, and one spec left alone.
#[test]
fn every_cell_of_the_drift_table_is_reachable() {
    let fx = fixture();

    write(&fx.library.join("skills/local/SKILL.md"), "local WIP\n");
    write(&fx.library.join("skills/both/SKILL.md"), "both WIP\n");

    commit_to_origin(&fx, "skills/remote/SKILL.md", "remote v2\n");
    commit_to_origin(&fx, "skills/both/SKILL.md", "both v2\n");
    Git::fetch(&fx.library).unwrap();

    let report = DriftReport::compute(&fx.library).unwrap();

    assert_eq!(report.state_of("clean"), DriftState::Clean);
    assert_eq!(report.state_of("local"), DriftState::LocalNewer);
    assert_eq!(report.state_of("remote"), DriftState::RemoteNewer);
    assert_eq!(report.state_of("both"), DriftState::Diverged);
    assert_eq!(report.drifted().count(), 3);
}

/// A brand-new skill exists only on this machine until it is published.
#[test]
fn an_untracked_skill_reads_as_local_newer() {
    let fx = fixture();
    write(&fx.library.join("skills/brand-new/SKILL.md"), "new\n");

    let report = DriftReport::compute(&fx.library).unwrap();
    assert_eq!(report.state_of("brand-new"), DriftState::LocalNewer);
}

/// The derived index sits in the tree but is gitignored, so regenerating it
/// must never look like a change worth publishing.
#[test]
fn regenerating_the_derived_index_does_not_show_as_drift() {
    let fx = fixture();
    write(
        &fx.library.join("library.json"),
        r#"{"version":1,"specs":[]}"#,
    );

    assert!(DriftReport::compute(&fx.library).unwrap().is_clean());
}

#[test]
fn instructions_drift_is_reported_on_its_own() {
    let fx = fixture();
    write(&fx.library.join("instructions/global.md"), "be terse\n");

    let report = DriftReport::compute(&fx.library).unwrap();
    assert_eq!(report.instructions(), DriftState::LocalNewer);
    assert_eq!(report.drifted().count(), 0);
    assert!(!report.is_clean());
}

/// Drift is advisory. A library that is not a git working tree — an offline
/// scratch install, say — reports clean rather than failing the sync.
#[test]
fn a_non_repo_library_reports_clean_instead_of_failing() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("skills/foo/SKILL.md"), "x\n");

    assert!(DriftReport::compute(tmp.path()).unwrap().is_clean());
}
