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

/// Drive the `pi` wrapper end to end and capture the argv it builds.
///
/// Stubs `akm`, `git` is real (a temp repo is initialised), and `pi` is a
/// script that dumps its arguments one per line.
fn run_pi_wrapper() -> String {
    use std::process::Command;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let stubs = root.join("stubs");
    std::fs::create_dir_all(&stubs).unwrap();

    let artifacts = root.join("artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();

    let argv_file = root.join("pi-argv.txt");

    write_stub(
        &stubs.join("akm"),
        &format!(
            r#"#!/bin/bash
case "$1 $2" in
  "config features") echo "skills,artifacts" ;;
  "config artifacts.dir") echo "{}" ;;
  "config artifacts.auto-push") echo "false" ;;
esac
exit 0
"#,
            artifacts.display()
        ),
    );

    write_stub(
        &stubs.join("pi"),
        &format!(
            "#!/bin/bash\nprintf '%s\\n' \"$@\" > \"{}\"\n",
            argv_file.display()
        ),
    );

    let project = root.join("myrepo");
    std::fs::create_dir_all(&project).unwrap();
    let init_path = root.join("akm-init.sh");
    std::fs::write(&init_path, include_str!("../src/shell/akm-init.sh")).unwrap();

    // No `set -e`: akm-init.sh is written for interactive shells and relies on
    // functions returning non-zero (see the note at the top of the script).
    let script = format!(
        r#"export PATH="{stubs}:$PATH"
export XDG_CACHE_HOME="{cache}"
cd "{project}"
git init -q .
source "{init}"
pi --model sonnet
"#,
        stubs = stubs.display(),
        cache = root.join("cache").display(),
        project = project.display(),
        init = init_path.display(),
    );

    let output = Command::new("bash")
        .arg("-c")
        .arg(&script)
        .env("HOME", root)
        .output()
        .expect("bash not found");

    assert!(
        output.status.success(),
        "wrapper script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::read_to_string(&argv_file).expect("pi stub was never invoked")
}

fn write_stub(path: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The `pi` wrapper mounts session skills with `--skill`, not `--add-dir`.
#[test]
fn test_pi_wrapper_mounts_skills_dir() {
    let argv = run_pi_wrapper();
    let lines: Vec<&str> = argv.lines().collect();

    let skill_idx = lines
        .iter()
        .position(|l| *l == "--skill")
        .expect("--skill not passed to pi");
    assert!(
        lines[skill_idx + 1].ends_with("/.pi/skills"),
        "unexpected --skill value: {}",
        lines[skill_idx + 1]
    );

    assert!(
        !lines.contains(&"--add-dir"),
        "pi does not support --add-dir"
    );
}

/// The artifacts dir is announced in the system prompt, since pi has no
/// `--add-dir` and does not sandbox its tools to the working directory.
#[test]
fn test_pi_wrapper_announces_artifacts_dir() {
    let argv = run_pi_wrapper();
    let lines: Vec<&str> = argv.lines().collect();

    assert!(
        lines.contains(&"--append-system-prompt"),
        "artifacts dir was not announced: {argv}"
    );
    assert!(
        argv.contains("AKM artifacts directory"),
        "append prompt missing the artifacts heading: {argv}"
    );
    assert!(
        argv.contains("/artifacts/myrepo"),
        "append prompt missing the artifacts path: {argv}"
    );
}

/// User arguments are forwarded after the injected flags.
#[test]
fn test_pi_wrapper_forwards_user_args() {
    let argv = run_pi_wrapper();
    let lines: Vec<&str> = argv.lines().collect();
    assert_eq!(&lines[lines.len() - 2..], &["--model", "sonnet"]);
}
