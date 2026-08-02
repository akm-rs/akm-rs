//! Integration tests for `akm uninstall`, `akm disable`, and `akm enable`.
//!
//! All tests use temp directories via `Paths::from_roots` and call the
//! testable inner functions — never the binary-deleting path.

use akm::commands::setup::Prompter;
use akm::commands::{disable, enable, uninstall};
use akm::error::Result;
use akm::library::spec::{Spec, SpecType};
use akm::library::tool_dirs::ToolDirs;
use akm::library::Library;
use akm::paths::Paths;
use akm::shell;
use tempfile::TempDir;

fn test_paths(tmp: &TempDir) -> Paths {
    Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        &tmp.path().join("home"),
    )
}

fn test_tool_dirs(tmp: &TempDir) -> ToolDirs {
    ToolDirs::builtin(&tmp.path().join("home"))
}

/// Lay down a realistic installed-akm file layout.
///
/// The library is the registry's git working tree — a `.git` marker stands in
/// for the clone so tests can assert it survives a default uninstall.
fn install_fixture(paths: &Paths) {
    std::fs::create_dir_all(paths.home()).unwrap();

    // Library checkout with one core skill and a .git marker
    let skill_dir = paths.skills_dir().join("tdd");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "# tdd").unwrap();
    let git_dir = paths.library_dir().join(".git");
    std::fs::create_dir_all(&git_dir).unwrap();
    std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();

    let mut spec = Spec::new("tdd", SpecType::Skill, "TDD", "Test-driven development");
    spec.core = true;
    let library = Library {
        version: 1,
        specs: vec![spec],
    };
    library.save(paths).unwrap();

    // Machine-local metadata next to library.json
    std::fs::write(paths.local_json(), "{\"core\":{}}\n").unwrap();

    // tools.json + shell init + bashrc block
    shell::install_tools_json(paths).unwrap();
    shell::install_shell_init(paths).unwrap();
    shell::patch_bashrc(paths).unwrap();

    // Config + cache
    std::fs::create_dir_all(paths.config_dir()).unwrap();
    std::fs::write(paths.config_file(), "features = [\"skills\"]\n").unwrap();
    std::fs::create_dir_all(paths.cache_dir().join("registry")).unwrap();

    // User content in ~/.akm
    let artifacts = paths.default_artifacts_dir().join("my-repo");
    std::fs::create_dir_all(&artifacts).unwrap();
    std::fs::write(artifacts.join("note.md"), "keep me").unwrap();
    std::fs::write(paths.legacy_global_instructions(), "be nice").unwrap();

    // Global symlinks in tool dirs (what rebuild_core produces). Pi's global
    // dir is nested (~/.pi/agent) — cover it explicitly.
    for tool_dir in [
        paths.home().join(".claude"),
        paths.home().join(".pi").join("agent"),
    ] {
        let skills = tool_dir.join("skills");
        std::fs::create_dir_all(&skills).unwrap();
        std::os::unix::fs::symlink(&skill_dir, skills.join("tdd")).unwrap();
    }
}

fn bashrc_content(paths: &Paths) -> String {
    std::fs::read_to_string(paths.home().join(".bashrc")).unwrap_or_default()
}

// =============================================================================
// uninstall::remove_files
// =============================================================================

#[test]
fn uninstall_default_preserves_user_content() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    install_fixture(&paths);

    uninstall::remove_files(&paths, &test_tool_dirs(&tmp), false).unwrap();

    // Removed
    assert!(!paths.config_dir().exists(), "config dir should be removed");
    assert!(!paths.cache_dir().exists(), "cache dir should be removed");
    assert!(!paths.library_json().exists());
    assert!(!paths.local_json().exists(), "local.json is metadata");
    assert!(!paths.tools_json().exists());
    assert!(!paths.data_dir().join("shell").exists());
    assert!(
        !paths.home().join(".claude/skills/tdd").is_symlink(),
        "global symlink should be removed"
    );
    assert!(
        !paths.home().join(".pi/agent/skills/tdd").is_symlink(),
        "pi global symlink should be removed"
    );
    assert!(!bashrc_content(&paths).contains(">>> akm >>>"));

    // Preserved: the library checkout (git clone), artifacts, legacy instructions
    assert!(paths.skills_dir().join("tdd/SKILL.md").is_file());
    assert!(paths.library_dir().join(".git/HEAD").is_file());
    assert!(paths
        .default_artifacts_dir()
        .join("my-repo/note.md")
        .is_file());
    assert!(paths.legacy_global_instructions().is_file());
}

#[test]
fn uninstall_purge_removes_everything() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    install_fixture(&paths);

    uninstall::remove_files(&paths, &test_tool_dirs(&tmp), true).unwrap();

    assert!(!paths.config_dir().exists());
    assert!(!paths.cache_dir().exists());
    assert!(
        !paths.data_dir().exists(),
        "data dir (including the library checkout) should be fully removed"
    );
    assert!(!paths.akm_home().exists(), "~/.akm should be fully removed");
    assert!(!bashrc_content(&paths).contains(">>> akm >>>"));
}

#[test]
fn uninstall_idempotent_on_missing_files() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    // Nothing installed at all — must not error
    uninstall::remove_files(&paths, &test_tool_dirs(&tmp), false).unwrap();
    uninstall::remove_files(&paths, &test_tool_dirs(&tmp), true).unwrap();
}

#[test]
fn uninstall_leaves_non_symlink_tool_dir_content() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    install_fixture(&paths);

    // A real (non-symlink) skill the user copied in manually
    let manual = paths.home().join(".claude/skills/hand-rolled");
    std::fs::create_dir_all(&manual).unwrap();
    std::fs::write(manual.join("SKILL.md"), "mine").unwrap();
    // Distributed instructions copy
    std::fs::write(paths.home().join(".claude/CLAUDE.md"), "instructions").unwrap();

    uninstall::remove_files(&paths, &test_tool_dirs(&tmp), false).unwrap();

    assert!(manual.join("SKILL.md").is_file());
    assert!(paths.home().join(".claude/CLAUDE.md").is_file());
}

#[test]
fn uninstall_purge_leaves_distributed_instructions() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    install_fixture(&paths);

    std::fs::write(paths.home().join(".claude/CLAUDE.md"), "instructions").unwrap();

    uninstall::remove_files(&paths, &test_tool_dirs(&tmp), true).unwrap();

    // Even --purge never touches copies distributed into tool dirs
    assert!(paths.home().join(".claude/CLAUDE.md").is_file());
}

// =============================================================================
// uninstall::run confirmation
// =============================================================================

struct DeclinePrompter;

impl Prompter for DeclinePrompter {
    fn confirm(&mut self, _message: &str, _default_yes: bool) -> Result<bool> {
        Ok(false)
    }
    fn input(&mut self, _message: &str, default: &str) -> Result<String> {
        Ok(default.to_string())
    }
}

#[test]
fn uninstall_declined_removes_nothing() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    install_fixture(&paths);

    uninstall::run(
        &paths,
        &test_tool_dirs(&tmp),
        false,
        false,
        &mut DeclinePrompter,
    )
    .unwrap();

    assert!(paths.config_file().is_file());
    assert!(paths.library_json().is_file());
    assert!(bashrc_content(&paths).contains(">>> akm >>>"));
}

// =============================================================================
// disable / enable
// =============================================================================

#[test]
fn disable_creates_sentinel_and_clears_symlinks() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    install_fixture(&paths);

    disable::run(&paths, &test_tool_dirs(&tmp)).unwrap();

    assert!(paths.disabled_sentinel().is_file());
    assert!(!paths.home().join(".claude/skills/tdd").is_symlink());
    assert!(!paths.home().join(".pi/agent/skills/tdd").is_symlink());
    // Library untouched
    assert!(paths.skills_dir().join("tdd/SKILL.md").is_file());
    assert!(paths.library_json().is_file());
}

#[test]
fn disable_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    install_fixture(&paths);

    disable::run(&paths, &test_tool_dirs(&tmp)).unwrap();
    disable::run(&paths, &test_tool_dirs(&tmp)).unwrap();
    assert!(paths.disabled_sentinel().is_file());
}

#[test]
fn enable_removes_sentinel_and_rebuilds_core_symlinks() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    install_fixture(&paths);

    disable::run(&paths, &test_tool_dirs(&tmp)).unwrap();
    enable::run(&paths, &test_tool_dirs(&tmp)).unwrap();

    assert!(!paths.disabled_sentinel().exists());
    assert!(
        paths.home().join(".claude/skills/tdd").is_symlink(),
        "core symlink should be rebuilt"
    );
    assert!(
        paths.home().join(".pi/agent/skills/tdd").is_symlink(),
        "pi core symlink should be rebuilt"
    );
}

#[test]
fn enable_without_disable_or_library_is_graceful() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    // No sentinel, no library
    enable::run(&paths, &test_tool_dirs(&tmp)).unwrap();
    assert!(!paths.disabled_sentinel().exists());
}

// =============================================================================
// akm-init.sh sentinel check
// =============================================================================

#[test]
fn shell_init_returns_early_when_disabled() {
    // The generated init script must consult the sentinel before defining
    // wrappers. Run it under bash with XDG_CONFIG_HOME pointed at a dir
    // containing the sentinel and verify no wrapper functions get defined.
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    shell::install_shell_init(&paths).unwrap();

    let config_akm = tmp.path().join("config").join("akm");
    std::fs::create_dir_all(&config_akm).unwrap();
    std::fs::write(config_akm.join("disabled"), "").unwrap();

    let out = std::process::Command::new("bash")
        .arg("-c")
        .arg(format!(
            "source '{}' && declare -F claude copilot opencode pi; echo \"defined=$?\"",
            paths.shell_init().display()
        ))
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .output()
        .expect("bash should run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("defined=1"),
        "wrappers must not be defined when sentinel present, got: {stdout}"
    );
}

#[test]
fn shell_init_defines_wrappers_when_enabled() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    shell::install_shell_init(&paths).unwrap();

    let out = std::process::Command::new("bash")
        .arg("-c")
        .arg(format!(
            "source '{}' && declare -F claude copilot opencode pi >/dev/null && echo defined=0",
            paths.shell_init().display()
        ))
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .output()
        .expect("bash should run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("defined=0"),
        "wrappers should be defined without sentinel, got: {stdout}"
    );
}

// =============================================================================
// shell::unpatch_bashrc
// =============================================================================

#[test]
fn unpatch_bashrc_removes_block_and_reports() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    std::fs::create_dir_all(paths.home()).unwrap();
    std::fs::write(paths.home().join(".bashrc"), "# mine\n").unwrap();
    shell::patch_bashrc(&paths).unwrap();

    let removed = shell::unpatch_bashrc(&paths).unwrap();
    assert!(removed);
    let content = bashrc_content(&paths);
    assert!(content.contains("# mine"));
    assert!(!content.contains(">>> akm >>>"));

    // Second call: nothing to do
    assert!(!shell::unpatch_bashrc(&paths).unwrap());
}

#[test]
fn unpatch_bashrc_missing_file_returns_false() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    assert!(!shell::unpatch_bashrc(&paths).unwrap());
}
