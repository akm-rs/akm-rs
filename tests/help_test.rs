//! Snapshot tests for CLI help output.

use assert_cmd::cargo::cargo_bin_cmd;

#[test]
fn test_help_output_snapshot() {
    let output = cargo_bin_cmd!("akm").arg("help").output().unwrap();

    insta::assert_snapshot!(
        "help_output",
        String::from_utf8_lossy(&output.stdout).to_string()
    );
}

#[test]
fn test_import_help_output_snapshot() {
    let output = cargo_bin_cmd!("akm")
        .args(["skills", "import", "--help"])
        .output()
        .unwrap();

    insta::assert_snapshot!(
        "import_help_output",
        String::from_utf8_lossy(&output.stdout).to_string()
    );
}

#[test]
fn test_skills_help_output_snapshot() {
    let output = cargo_bin_cmd!("akm")
        .args(["skills", "--help"])
        .output()
        .unwrap();

    insta::assert_snapshot!(
        "skills_help_output",
        String::from_utf8_lossy(&output.stdout).to_string()
    );
}

/// `shared` has a TTY-vs-pipe split and a `--plain` escape hatch, so its help —
/// and the fact that it takes no verb subcommands — is pinned.
#[test]
fn test_shared_help_output_snapshot() {
    let output = cargo_bin_cmd!("akm")
        .args(["skills", "shared", "--help"])
        .output()
        .unwrap();

    insta::assert_snapshot!(
        "shared_help_output",
        String::from_utf8_lossy(&output.stdout).to_string()
    );
}

/// Rename and delete are destructive/structural, so their contract — argument
/// order and `delete`'s `--force` — is pinned.
#[test]
fn test_rename_delete_help_snapshots() {
    for command in ["rename", "delete"] {
        let output = cargo_bin_cmd!("akm")
            .args(["skills", command, "--help"])
            .output()
            .unwrap();

        insta::assert_snapshot!(
            format!("{command}_help_output"),
            String::from_utf8_lossy(&output.stdout).to_string()
        );
    }
}

/// The drift commands are the user-facing half of the sync rework, so their
/// contract is pinned. `publish` is on the list because its id is optional —
/// omitting it publishes everything pending, and that has to stay visible.
#[test]
fn test_drift_command_help_snapshots() {
    for command in ["diff", "revert", "core", "publish"] {
        let output = cargo_bin_cmd!("akm")
            .args(["skills", command, "--help"])
            .output()
            .unwrap();

        insta::assert_snapshot!(
            format!("{command}_help_output"),
            String::from_utf8_lossy(&output.stdout).to_string()
        );
    }
}

/// Instructions gained a publish command; `sync.rs` tells the user to run it,
/// so it has to stay on the list.
#[test]
fn test_instructions_help_output_snapshot() {
    let output = cargo_bin_cmd!("akm")
        .args(["instructions", "--help"])
        .output()
        .unwrap();

    insta::assert_snapshot!(
        "instructions_help_output",
        String::from_utf8_lossy(&output.stdout).to_string()
    );
}
