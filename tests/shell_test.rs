//! Integration tests for shell init generation.

/// Verify the embedded akm-init.sh is valid bash syntax
#[test]
fn test_shell_init_syntax_check() {
    use std::process::Command;

    let dir = tempfile::tempdir().unwrap();
    let init_path = dir.path().join("akm-init.sh");

    std::fs::write(&init_path, include_str!("../src/shell/akm-init.sh")).unwrap();

    let output = Command::new("bash")
        .arg("-n")
        .arg(&init_path)
        .output()
        .expect("bash not found");

    assert!(
        output.status.success(),
        "akm-init.sh has syntax errors: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Snapshot test for the generated shell init
#[test]
fn test_shell_init_snapshot() {
    insta::assert_snapshot!("shell_init", akm::shell::shell_init_content());
}
