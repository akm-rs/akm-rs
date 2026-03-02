//! Snapshot test for CLI help output.

use assert_cmd::cargo::cargo_bin_cmd;

#[test]
fn test_help_output_snapshot() {
    let output = cargo_bin_cmd!("akm").arg("help").output().unwrap();

    insta::assert_snapshot!(
        "help_output",
        String::from_utf8_lossy(&output.stdout).to_string()
    );
}
