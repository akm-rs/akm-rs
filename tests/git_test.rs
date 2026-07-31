use akm::git::{Git, MergeOutcome};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// Run a git command in `dir`, panicking with git's own stderr on failure.
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

/// An origin repo with two skills, plus a clone tracking it.
///
/// Mirrors the shape of the personal registry: `skills/<id>/SKILL.md`.
struct Fixture {
    _tmp: TempDir,
    origin: PathBuf,
    local: PathBuf,
}

fn fixture() -> Fixture {
    let tmp = TempDir::new().unwrap();
    let origin = tmp.path().join("origin");
    std::fs::create_dir_all(&origin).unwrap();

    git(&origin, &["init", "-b", "main"]);
    git(&origin, &["config", "user.email", "test@example.com"]);
    git(&origin, &["config", "user.name", "Test"]);
    write(&origin.join("skills/alpha/SKILL.md"), "alpha v1\n");
    write(&origin.join("skills/beta/SKILL.md"), "beta v1\n");
    git(&origin, &["add", "-A"]);
    git(&origin, &["commit", "-m", "initial"]);
    // Detach so the clone can push back into this non-bare repo if needed.
    git(&origin, &["config", "receive.denyCurrentBranch", "ignore"]);

    let local = tmp.path().join("local");
    git(
        tmp.path(),
        &[
            "clone",
            "--quiet",
            &origin.to_string_lossy(),
            &local.to_string_lossy(),
        ],
    );
    git(&local, &["config", "user.email", "test@example.com"]);
    git(&local, &["config", "user.name", "Test"]);

    Fixture {
        _tmp: tmp,
        origin,
        local,
    }
}

/// Advance origin by rewriting one skill.
fn advance_origin(fx: &Fixture, id: &str, content: &str) {
    write(&fx.origin.join(format!("skills/{id}/SKILL.md")), content);
    git(&fx.origin, &["add", "-A"]);
    git(&fx.origin, &["commit", "-m", &format!("update {id}")]);
}

#[test]
fn is_inside_work_tree_false_outside_repo() {
    let tmp = TempDir::new().unwrap();
    assert!(!akm::git::Git::is_inside_work_tree(Some(tmp.path())));
}

#[test]
fn is_inside_work_tree_true_inside_repo() {
    let tmp = TempDir::new().unwrap();
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(akm::git::Git::is_inside_work_tree(Some(tmp.path())));
}

#[test]
fn toplevel_returns_repo_root() {
    let tmp = TempDir::new().unwrap();
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    let root = akm::git::Git::toplevel(Some(tmp.path())).unwrap();
    // Canonicalize both to handle symlinks (e.g., /tmp → /private/tmp on macOS)
    assert_eq!(
        root.canonicalize().unwrap(),
        tmp.path().canonicalize().unwrap()
    );
}

#[test]
fn repo_name_returns_dirname() {
    let tmp = TempDir::new().unwrap();
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    let name = akm::git::Git::repo_name(Some(tmp.path())).unwrap();
    let expected = tmp
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert_eq!(name, expected);
}

#[test]
fn has_changes_detects_new_file() {
    let tmp = TempDir::new().unwrap();
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    std::fs::write(tmp.path().join("test.txt"), "hello").unwrap();
    assert!(akm::git::Git::has_changes(tmp.path()).unwrap());
}

// --- Library working-tree primitives ---

#[test]
fn upstream_ref_resolves_for_a_clone() {
    let fx = fixture();
    assert_eq!(Git::upstream_ref(&fx.local).unwrap(), "origin/main");
}

#[test]
fn merge_ff_only_reports_up_to_date() {
    let fx = fixture();
    Git::fetch(&fx.local).unwrap();
    assert_eq!(
        Git::merge_ff_only(&fx.local, "origin/main").unwrap(),
        MergeOutcome::UpToDate
    );
}

/// The core guarantee behind D3: a fast-forward applies remote updates while
/// leaving uncommitted edits to *other* paths byte-for-byte intact.
#[test]
fn merge_ff_only_preserves_local_edits_to_untouched_paths() {
    let fx = fixture();
    let alpha = fx.local.join("skills/alpha/SKILL.md");
    write(&alpha, "alpha WIP\n");

    advance_origin(&fx, "beta", "beta v2\n");
    Git::fetch(&fx.local).unwrap();

    assert_eq!(
        Git::merge_ff_only(&fx.local, "origin/main").unwrap(),
        MergeOutcome::FastForwarded
    );

    assert_eq!(std::fs::read_to_string(&alpha).unwrap(), "alpha WIP\n");
    assert_eq!(
        std::fs::read_to_string(fx.local.join("skills/beta/SKILL.md")).unwrap(),
        "beta v2\n"
    );
}

/// When both sides touch the same path git aborts, names the path, and writes
/// nothing — no conflict markers reach files symlinked into tool directories.
#[test]
fn merge_ff_only_blocked_names_the_path_and_changes_nothing() {
    let fx = fixture();
    let alpha = fx.local.join("skills/alpha/SKILL.md");
    write(&alpha, "alpha WIP\n");

    advance_origin(&fx, "alpha", "alpha v2\n");
    Git::fetch(&fx.local).unwrap();

    let head_before = Git::rev_parse(&fx.local, "HEAD").unwrap();
    let outcome = Git::merge_ff_only(&fx.local, "origin/main").unwrap();

    match outcome {
        MergeOutcome::Blocked { paths } => {
            assert_eq!(paths, vec!["skills/alpha/SKILL.md".to_string()]);
        }
        other => panic!("expected Blocked, got {other:?}"),
    }

    assert_eq!(std::fs::read_to_string(&alpha).unwrap(), "alpha WIP\n");
    assert_eq!(Git::rev_parse(&fx.local, "HEAD").unwrap(), head_before);
}

/// A local commit that the remote does not contain makes the histories
/// diverge, which is a different failure from a blocked working tree.
#[test]
fn merge_ff_only_reports_diverged_history() {
    let fx = fixture();
    write(&fx.local.join("skills/alpha/SKILL.md"), "alpha local\n");
    git(&fx.local, &["add", "-A"]);
    git(&fx.local, &["commit", "-m", "local change"]);

    advance_origin(&fx, "beta", "beta v2\n");
    Git::fetch(&fx.local).unwrap();

    assert_eq!(
        Git::merge_ff_only(&fx.local, "origin/main").unwrap(),
        MergeOutcome::NotFastForward
    );
}

#[test]
fn status_porcelain_reports_modified_and_untracked_files() {
    let fx = fixture();
    write(&fx.local.join("skills/alpha/SKILL.md"), "alpha WIP\n");
    write(&fx.local.join("skills/gamma/SKILL.md"), "gamma new\n");

    let entries = Git::status_porcelain(&fx.local).unwrap();
    let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();

    assert!(paths.contains(&"skills/alpha/SKILL.md"), "got {paths:?}");
    // --untracked-files=all expands the new directory into its files.
    assert!(paths.contains(&"skills/gamma/SKILL.md"), "got {paths:?}");
    assert!(entries
        .iter()
        .any(|e| e.path == "skills/gamma/SKILL.md" && e.is_untracked()));
}

#[test]
fn status_porcelain_is_empty_for_a_clean_tree() {
    let fx = fixture();
    assert!(Git::status_porcelain(&fx.local).unwrap().is_empty());
}

#[test]
fn diff_names_lists_paths_the_remote_changed() {
    let fx = fixture();
    advance_origin(&fx, "beta", "beta v2\n");
    Git::fetch(&fx.local).unwrap();

    let changed = Git::diff_names(&fx.local, "HEAD", "origin/main", &[]).unwrap();
    assert_eq!(changed, vec!["skills/beta/SKILL.md".to_string()]);

    let scoped = Git::diff_names(&fx.local, "HEAD", "origin/main", &["skills/alpha"]).unwrap();
    assert!(scoped.is_empty());
}

#[test]
fn restore_path_discards_local_edits() {
    let fx = fixture();
    let alpha = fx.local.join("skills/alpha/SKILL.md");
    write(&alpha, "alpha WIP\n");

    Git::restore_path(&fx.local, &["skills/alpha"]).unwrap();

    assert_eq!(std::fs::read_to_string(&alpha).unwrap(), "alpha v1\n");
}

#[test]
fn clean_path_removes_untracked_files_under_it_only() {
    let fx = fixture();
    write(&fx.local.join("skills/gamma/SKILL.md"), "gamma\n");
    write(&fx.local.join("skills/delta/SKILL.md"), "delta\n");

    Git::clean_path(&fx.local, &["skills/gamma"]).unwrap();

    assert!(!fx.local.join("skills/gamma").exists());
    assert!(fx.local.join("skills/delta/SKILL.md").exists());
}

#[test]
fn checkout_from_takes_the_remote_version_of_one_path() {
    let fx = fixture();
    write(&fx.local.join("skills/alpha/SKILL.md"), "alpha WIP\n");
    advance_origin(&fx, "alpha", "alpha v2\n");
    Git::fetch(&fx.local).unwrap();

    Git::checkout_from(&fx.local, "origin/main", &["skills/alpha"]).unwrap();

    assert_eq!(
        std::fs::read_to_string(fx.local.join("skills/alpha/SKILL.md")).unwrap(),
        "alpha v2\n"
    );
}

#[test]
fn add_path_stages_only_the_named_path() {
    let fx = fixture();
    write(&fx.local.join("skills/alpha/SKILL.md"), "alpha WIP\n");
    write(&fx.local.join("skills/beta/SKILL.md"), "beta WIP\n");

    Git::add_path(&fx.local, &["skills/alpha"]).unwrap();

    let staged = git(&fx.local, &["diff", "--cached", "--name-only"]);
    assert_eq!(staged, "skills/alpha/SKILL.md");
}
