use std::process::Command;
use tempfile::TempDir;

#[test]
fn is_inside_work_tree_false_outside_repo() {
    let tmp = TempDir::new().unwrap();
    assert!(!akm::git::Git::is_inside_work_tree(Some(tmp.path())));
}

#[test]
fn is_inside_work_tree_true_inside_repo() {
    let tmp = TempDir::new().unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(akm::git::Git::is_inside_work_tree(Some(tmp.path())));
}

#[test]
fn toplevel_returns_repo_root() {
    let tmp = TempDir::new().unwrap();
    Command::new("git")
        .args(["init"])
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
        .args(["init"])
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
        .args(["init"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    std::fs::write(tmp.path().join("test.txt"), "hello").unwrap();
    assert!(akm::git::Git::has_changes(tmp.path()).unwrap());
}
