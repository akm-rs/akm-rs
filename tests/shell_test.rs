//! Integration tests for shell init generation.

use std::path::{Path, PathBuf};
use std::process::Output;

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

/// A sourced akm-init.sh with stubbed `akm` and stubbed tools.
///
/// Tools are scripts that dump their argv (and, for `claude`, a probe of the
/// staging dir) into files under the temp root, so a test can assert on what
/// the wrapper built and on what the agent would have seen mid-session.
struct Harness {
    dir: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        let h = Harness {
            dir: tempfile::tempdir().unwrap(),
        };
        let root = h.dir.path();

        let stubs = root.join("stubs");
        std::fs::create_dir_all(&stubs).unwrap();
        std::fs::create_dir_all(h.artifacts()).unwrap();

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
                h.artifacts().display()
            ),
        );

        // Pi only needs to record argv.
        write_stub(
            &stubs.join("pi"),
            &format!(
                "#!/bin/bash\nprintf '%s\\n' \"$@\" > \"{}\"\n",
                h.probe("pi-argv").display()
            ),
        );

        // Claude additionally probes the session dir it was handed: what the
        // staging root contains, whether a stray write there succeeds, and
        // whether the signpost is readable.
        write_stub(
            &stubs.join("claude"),
            &format!(
                r#"#!/bin/bash
printf '%s\n' "$@" > "{argv}"
ls -A "$AKM_SESSION" > "{ls}" 2>&1
if mkdir "$AKM_SESSION/research" 2>"{stray}"; then
  echo "STRAY WRITE SUCCEEDED" >> "{stray}"
fi
cat "$AKM_SESSION/README.md" > "{readme}" 2>&1
"#,
                argv = h.probe("claude-argv").display(),
                ls = h.probe("staging-ls").display(),
                stray = h.probe("stray-write").display(),
                readme = h.probe("readme").display(),
            ),
        );

        std::fs::create_dir_all(root.join("myrepo")).unwrap();
        std::fs::write(
            root.join("akm-init.sh"),
            include_str!("../src/shell/akm-init.sh"),
        )
        .unwrap();

        h
    }

    /// The configured artifacts root (`artifacts.dir`).
    fn artifacts(&self) -> PathBuf {
        self.dir.path().join("artifacts")
    }

    /// This project's artifacts dir — where rescued files land.
    fn artifact_dir(&self) -> PathBuf {
        self.artifacts().join("myrepo")
    }

    fn probe(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn read_probe(&self, name: &str) -> String {
        std::fs::read_to_string(self.probe(name))
            .unwrap_or_else(|e| panic!("probe '{name}' was never written: {e}"))
    }

    /// Source akm-init.sh in a repo-rooted shell and run `body`.
    ///
    /// No `set -e`: akm-init.sh is written for interactive shells and relies on
    /// functions returning non-zero (see the note at the top of the script).
    fn run(&self, body: &str) -> Output {
        use std::process::Command;

        let root = self.dir.path();
        let script = format!(
            r#"export PATH="{stubs}:$PATH"
export XDG_CACHE_HOME="{cache}"
cd "{project}"
git init -q .
source "{init}"
{body}
"#,
            stubs = root.join("stubs").display(),
            cache = root.join("cache").display(),
            project = root.join("myrepo").display(),
            init = root.join("akm-init.sh").display(),
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
        output
    }
}

fn write_stub(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

// --- Pi wrapper ---

/// The `pi` wrapper mounts session skills with `--skill`, not `--add-dir`.
#[test]
fn test_pi_wrapper_mounts_skills_dir() {
    let h = Harness::new();
    h.run("pi --model sonnet");
    let argv = h.read_probe("pi-argv");
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
    let h = Harness::new();
    h.run("pi --model sonnet");
    let argv = h.read_probe("pi-argv");

    assert!(
        argv.lines().any(|l| l == "--append-system-prompt"),
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
    let h = Harness::new();
    h.run("pi --model sonnet");
    let argv = h.read_probe("pi-argv");
    let lines: Vec<&str> = argv.lines().collect();
    assert_eq!(&lines[lines.len() - 2..], &["--model", "sonnet"]);
}

// --- Claude wrapper ---

/// Claude gets the staging dir mounted *and* the artifacts dir named, because
/// the mounted dir is ephemeral and the symlink inside it is easy to miss.
#[test]
fn test_claude_wrapper_mounts_and_announces() {
    let h = Harness::new();
    h.run("claude");
    let argv = h.read_probe("claude-argv");
    let lines: Vec<&str> = argv.lines().collect();

    let add_idx = lines
        .iter()
        .position(|l| *l == "--add-dir")
        .expect("--add-dir not passed to claude");
    assert!(
        lines[add_idx + 1].contains("/cache/akm/myrepo-"),
        "unexpected --add-dir value: {}",
        lines[add_idx + 1]
    );

    assert!(
        lines.contains(&"--append-system-prompt"),
        "artifacts dir was not announced: {argv}"
    );
    assert!(
        argv.contains("/artifacts/myrepo"),
        "append prompt missing the artifacts path: {argv}"
    );
    assert!(
        argv.contains("Never write into the AKM session staging directory"),
        "append prompt missing the staging warning: {argv}"
    );
}

/// The staging root is read-only during the session, so an agent that writes
/// there gets an error it can act on rather than silent loss at session end.
#[test]
fn test_staging_root_rejects_stray_writes() {
    let h = Harness::new();
    h.run("claude");

    let stray = h.read_probe("stray-write");
    assert!(
        !stray.contains("STRAY WRITE SUCCEEDED"),
        "staging root accepted a stray write: {stray}"
    );
    assert!(
        stray.contains("Permission denied"),
        "expected EACCES naming the path, got: {stray}"
    );
}

/// The signpost is readable from the mounted dir, for tools with no
/// `--append-system-prompt` to receive the announcement through.
#[test]
fn test_staging_readme_points_at_artifacts_dir() {
    let h = Harness::new();
    h.run("claude");

    let readme = h.read_probe("readme");
    assert!(
        readme.contains("EPHEMERAL"),
        "README missing the ephemeral warning: {readme}"
    );
    assert!(
        readme.contains("/artifacts/myrepo"),
        "README missing the artifacts path: {readme}"
    );

    let listing = h.read_probe("staging-ls");
    assert!(
        listing.contains("README.md") && listing.contains("artifacts"),
        "staging root should expose both signposts: {listing}"
    );
}

// --- Session teardown ---

/// A session that wrote nothing of its own leaves nothing behind.
#[test]
fn test_session_end_removes_clean_staging_dir() {
    let h = Harness::new();
    let out = h.run(
        r#"_akm_session_start
echo "$AKM_SESSION" > "$HOME/session-path"
_akm_session_end"#,
    );

    let staging = h.read_probe("session-path").trim().to_string();
    assert!(!staging.is_empty(), "session start produced no staging dir");
    assert!(
        !Path::new(&staging).exists(),
        "clean staging dir survived teardown: {staging}"
    );
    assert!(
        !h.artifact_dir().join("orphaned").exists(),
        "clean teardown should not quarantine anything: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A stray file is moved into the artifacts dir instead of being deleted, and
/// the rescue is reported. This is the data-loss path from issue #23.
#[test]
fn test_session_end_quarantines_stray_files() {
    let h = Harness::new();
    let out = h.run(
        r#"_akm_session_start
echo "$AKM_SESSION" > "$HOME/session-path"
# Simulate an agent that got past the read-only root — a stale shell init, a
# chmod, or a tool writing before the seal.
chmod u+w "$AKM_SESSION"
mkdir -p "$AKM_SESSION/research"
echo "700 lines of research" > "$AKM_SESSION/research/findings.md"
_akm_session_end"#,
    );

    let staging = h.read_probe("session-path").trim().to_string();
    let session_id = Path::new(&staging).file_name().unwrap();
    let rescued = h
        .artifact_dir()
        .join("orphaned")
        .join(session_id)
        .join("research")
        .join("findings.md");

    assert!(
        rescued.exists(),
        "stray file was not rescued to {}",
        rescued.display()
    );
    assert_eq!(
        std::fs::read_to_string(&rescued).unwrap().trim(),
        "700 lines of research"
    );
    assert!(
        !Path::new(&staging).exists(),
        "staging dir should be gone once emptied: {staging}"
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("rescued stray session files"),
        "rescue was not reported: {stderr}"
    );
}

/// The next session tells its agent about anything awaiting triage, since the
/// agent that wrote the files is gone by the time they are rescued.
#[test]
fn test_next_session_announces_rescued_files() {
    let h = Harness::new();
    let rescued = h.artifact_dir().join("orphaned").join("myrepo-1-2");
    std::fs::create_dir_all(&rescued).unwrap();
    std::fs::write(rescued.join("notes.md"), "rescued").unwrap();

    h.run("claude");
    let argv = h.read_probe("claude-argv");

    assert!(
        argv.contains("Rescued files awaiting triage"),
        "orphaned files were not announced: {argv}"
    );
    assert!(
        argv.contains("/artifacts/myrepo/orphaned"),
        "announcement missing the orphaned path: {argv}"
    );
}

/// No rescued files, no notice — the announcement stays quiet in the normal case.
#[test]
fn test_no_orphan_notice_when_nothing_rescued() {
    let h = Harness::new();
    h.run("claude");
    let argv = h.read_probe("claude-argv");

    assert!(
        !argv.contains("Rescued files awaiting triage"),
        "orphan notice appeared with nothing rescued: {argv}"
    );
}
