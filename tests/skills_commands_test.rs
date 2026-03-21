//! Integration tests for skills subcommands (Task 4).
//!
//! Tests use temp directories and mock library data to exercise
//! add, remove, list, search, status, load, unload, loaded, clean, promote.

use akm::error::Error;
use akm::library::spec::{Spec, SpecType};
use akm::library::tool_dirs::ToolDirs;
use akm::library::Library;
use akm::paths::Paths;
use predicates::str::contains as pred_contains;
use std::path::Path;
use tempfile::TempDir;

/// Helper: create Paths from a temp dir.
fn test_paths(tmp: &TempDir) -> Paths {
    Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        &tmp.path().join("home"),
    )
}

/// Helper: create ToolDirs for temp home.
fn test_tool_dirs(tmp: &TempDir) -> ToolDirs {
    ToolDirs::builtin(&tmp.path().join("home"))
}

/// Helper: create a minimal library with some specs.
fn create_test_library(paths: &Paths) -> Library {
    let mut library = Library::new();
    let mut tdd = Spec::new("tdd", SpecType::Skill, "TDD", "Test-driven development");
    tdd.core = true;
    tdd.tags = vec!["testing".to_string(), "methodology".to_string()];

    let reviewer = Spec::new("reviewer", SpecType::Agent, "Reviewer", "Code review agent");

    let debug = Spec::new("debug", SpecType::Skill, "Debug", "Debugging helper");

    library.specs.push(tdd);
    library.specs.push(reviewer);
    library.specs.push(debug);
    library.save(paths).unwrap();
    library
}

/// Helper: create skill files on disk so symlinks can work.
fn create_spec_on_disk(data_dir: &Path, id: &str, spec_type: SpecType) {
    match spec_type {
        SpecType::Skill => {
            let dir = data_dir.join("skills").join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {id}\ndescription: test\n---\nContent"),
            )
            .unwrap();
        }
        SpecType::Agent => {
            let dir = data_dir.join("agents");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("{id}.md")),
                format!("---\nname: {id}\ndescription: test\n---\nContent"),
            )
            .unwrap();
        }
    }
}

// =============================================================================
// List tests
// =============================================================================

#[test]
fn list_all_specs() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    create_test_library(&paths);

    let result = akm::commands::skills::list::run(&paths, None, None, true, &test_tool_dirs(&tmp));
    assert!(result.is_ok());
}

#[test]
fn list_filter_by_type() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    create_test_library(&paths);

    let result =
        akm::commands::skills::list::run(&paths, None, Some("skill"), true, &test_tool_dirs(&tmp));
    assert!(result.is_ok());

    let result =
        akm::commands::skills::list::run(&paths, None, Some("agent"), true, &test_tool_dirs(&tmp));
    assert!(result.is_ok());
}

#[test]
fn list_filter_by_tag() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    create_test_library(&paths);

    let result = akm::commands::skills::list::run(
        &paths,
        Some("testing"),
        None,
        true,
        &test_tool_dirs(&tmp),
    );
    assert!(result.is_ok());
}

#[test]
fn list_combined_filters() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    create_test_library(&paths);

    let result = akm::commands::skills::list::run(
        &paths,
        Some("testing"),
        Some("skill"),
        true,
        &test_tool_dirs(&tmp),
    );
    assert!(result.is_ok());
}

#[test]
fn list_invalid_type_returns_error() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    create_test_library(&paths);

    let result =
        akm::commands::skills::list::run(&paths, None, Some("bogus"), true, &test_tool_dirs(&tmp));
    assert!(result.is_err());
}

#[test]
fn list_no_library_returns_error() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);

    let result = akm::commands::skills::list::run(&paths, None, None, true, &test_tool_dirs(&tmp));
    assert!(result.is_err());
}

// =============================================================================
// Search tests
// =============================================================================

#[test]
fn search_by_id() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    create_test_library(&paths);

    let result = akm::commands::skills::search::run(&paths, "tdd", true, &test_tool_dirs(&tmp));
    assert!(result.is_ok());
}

#[test]
fn search_by_description() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    create_test_library(&paths);

    let result =
        akm::commands::skills::search::run(&paths, "Code review", true, &test_tool_dirs(&tmp));
    assert!(result.is_ok());
}

#[test]
fn search_by_tag() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    create_test_library(&paths);

    let result = akm::commands::skills::search::run(&paths, "testing", true, &test_tool_dirs(&tmp));
    assert!(result.is_ok());
}

#[test]
fn search_case_insensitive() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    create_test_library(&paths);

    let result = akm::commands::skills::search::run(&paths, "TDD", true, &test_tool_dirs(&tmp));
    assert!(result.is_ok());
}

#[test]
fn search_no_results() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    create_test_library(&paths);

    let result =
        akm::commands::skills::search::run(&paths, "nonexistent-zzz", true, &test_tool_dirs(&tmp));
    assert!(result.is_ok()); // No error, just no output
}

// =============================================================================
// Load tests
// =============================================================================

#[test]
fn load_no_session_errors() {
    // Test via assert_cmd where we control the environment
    use assert_cmd::cargo::cargo_bin_cmd;

    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    create_test_library(&paths);

    cargo_bin_cmd!("akm")
        .args(["skills", "load", "tdd"])
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path().join("home"))
        .env_remove("AKM_SESSION")
        .assert()
        .failure()
        .stderr(pred_contains("session"));
}

#[test]
fn load_creates_symlinks_in_session() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    create_test_library(&paths);
    create_spec_on_disk(paths.data_dir(), "tdd", SpecType::Skill);

    let tool_dirs = test_tool_dirs(&tmp);
    let staging = tmp.path().join("session");
    std::fs::create_dir_all(&staging).unwrap();

    // Manually test the symlink creation (since we can't set env var safely in parallel)
    let library = Library::load_checked(&paths).unwrap();
    let spec = library.get("tdd").unwrap();
    let created =
        akm::library::symlinks::create_session(spec, paths.data_dir(), &staging, tool_dirs.dirs())
            .unwrap();
    assert!(created);

    // Verify symlink exists
    assert!(staging
        .join(".claude")
        .join("skills")
        .join("tdd")
        .is_symlink());
}

#[test]
fn load_idempotent() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    create_test_library(&paths);
    create_spec_on_disk(paths.data_dir(), "tdd", SpecType::Skill);

    let tool_dirs = test_tool_dirs(&tmp);
    let staging = tmp.path().join("session");
    std::fs::create_dir_all(&staging).unwrap();

    let library = Library::load_checked(&paths).unwrap();
    let spec = library.get("tdd").unwrap();

    // Load twice — both should succeed
    let c1 =
        akm::library::symlinks::create_session(spec, paths.data_dir(), &staging, tool_dirs.dirs())
            .unwrap();
    let c2 =
        akm::library::symlinks::create_session(spec, paths.data_dir(), &staging, tool_dirs.dirs())
            .unwrap();
    assert!(c1);
    assert!(c2);
}

// =============================================================================
// Unload tests
// =============================================================================

#[test]
fn unload_removes_symlinks() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    create_test_library(&paths);
    create_spec_on_disk(paths.data_dir(), "tdd", SpecType::Skill);

    let tool_dirs = test_tool_dirs(&tmp);
    let staging = tmp.path().join("session");
    std::fs::create_dir_all(&staging).unwrap();

    let library = Library::load_checked(&paths).unwrap();
    let spec = library.get("tdd").unwrap();

    akm::library::symlinks::create_session(spec, paths.data_dir(), &staging, tool_dirs.dirs())
        .unwrap();

    let removed =
        akm::library::symlinks::remove_session("tdd", &staging, tool_dirs.dirs()).unwrap();
    assert!(removed);
    assert!(!staging.join(".claude").join("skills").join("tdd").exists());
}

#[test]
fn unload_not_loaded_returns_false() {
    let tmp = TempDir::new().unwrap();
    let tool_dirs = test_tool_dirs(&tmp);
    let staging = tmp.path().join("session");
    std::fs::create_dir_all(&staging).unwrap();

    let removed =
        akm::library::symlinks::remove_session("nonexistent", &staging, tool_dirs.dirs()).unwrap();
    assert!(!removed);
}

// =============================================================================
// Clean tests
// =============================================================================

#[test]
fn clean_global_removes_non_symlinks() {
    let tmp = TempDir::new().unwrap();
    let tool_dirs = test_tool_dirs(&tmp);
    let paths = test_paths(&tmp);

    // Create a non-symlink file in the skills dir
    let skills_dir = tmp.path().join("home").join(".claude").join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    let real_dir = skills_dir.join("stale-skill");
    std::fs::create_dir_all(&real_dir).unwrap();
    std::fs::write(real_dir.join("file.txt"), "stale data").unwrap();

    let result = akm::commands::skills::clean::run(&paths, &tool_dirs, false, false);
    assert!(result.is_ok());
    assert!(!real_dir.exists());
}

#[test]
fn clean_global_preserves_symlinks() {
    let tmp = TempDir::new().unwrap();
    let tool_dirs = test_tool_dirs(&tmp);
    let paths = test_paths(&tmp);

    // Create a symlink in the skills dir
    let skills_dir = tmp.path().join("home").join(".claude").join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    let target = tmp.path().join("target");
    std::fs::create_dir_all(&target).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, skills_dir.join("valid-skill")).unwrap();

    let result = akm::commands::skills::clean::run(&paths, &tool_dirs, false, false);
    assert!(result.is_ok());

    #[cfg(unix)]
    assert!(skills_dir.join("valid-skill").is_symlink());
}

#[test]
fn clean_global_dry_run() {
    let tmp = TempDir::new().unwrap();
    let tool_dirs = test_tool_dirs(&tmp);
    let paths = test_paths(&tmp);

    let skills_dir = tmp.path().join("home").join(".claude").join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::write(skills_dir.join("stale.md"), "stale").unwrap();

    let result = akm::commands::skills::clean::run(&paths, &tool_dirs, false, true);
    assert!(result.is_ok());
    // File should still exist (dry run)
    assert!(skills_dir.join("stale.md").exists());
}

// =============================================================================
// Promote tests
// =============================================================================

#[test]
fn promote_requires_skill_md() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let tool_dirs = test_tool_dirs(&tmp);

    let skill_dir = tmp.path().join("no-skill-md");
    std::fs::create_dir_all(&skill_dir).unwrap();

    let result =
        akm::commands::skills::promote::run(&paths, &skill_dir.to_string_lossy(), true, &tool_dirs);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, Error::NoSkillMd { .. }));
}

#[test]
fn promote_nonexistent_dir() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let tool_dirs = test_tool_dirs(&tmp);

    let result =
        akm::commands::skills::promote::run(&paths, "/nonexistent/path/xyz", true, &tool_dirs);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, Error::PromoteDirNotFound { .. }));
}

#[test]
fn promote_requires_name_in_frontmatter() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let tool_dirs = test_tool_dirs(&tmp);

    let skill_dir = tmp.path().join("missing-name");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\ndescription: A test\n---\nContent",
    )
    .unwrap();

    let result =
        akm::commands::skills::promote::run(&paths, &skill_dir.to_string_lossy(), true, &tool_dirs);
    assert!(result.is_err());
}

/// Promote integration tests use assert_cmd so stdin is piped (non-interactive).
#[test]
fn promote_new_skill_via_cmd() {
    use assert_cmd::cargo::cargo_bin_cmd;

    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);

    std::fs::create_dir_all(paths.data_dir().join("skills")).unwrap();
    Library::new().save(&paths).unwrap();

    let skill_dir = tmp.path().join("local-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: Local Skill\ndescription: A test skill\n---\nContent",
    )
    .unwrap();

    cargo_bin_cmd!("akm")
        .args(["skills", "promote", &skill_dir.to_string_lossy(), "--force"])
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path().join("home"))
        .assert()
        .success();

    assert!(paths.data_dir().join("skills").join("local-skill").is_dir());
    let lib = Library::load(&paths).unwrap();
    assert!(lib.contains("local-skill"));
}

#[test]
fn promote_overwrites_with_force_via_cmd() {
    use assert_cmd::cargo::cargo_bin_cmd;

    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);

    let existing = paths.data_dir().join("skills").join("my-skill");
    std::fs::create_dir_all(&existing).unwrap();
    std::fs::write(
        existing.join("SKILL.md"),
        "---\nname: Old\ndescription: old version\n---\nOld content",
    )
    .unwrap();
    Library::new().save(&paths).unwrap();

    let skill_dir = tmp.path().join("my-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: New\ndescription: new version\n---\nNew content",
    )
    .unwrap();

    cargo_bin_cmd!("akm")
        .args(["skills", "promote", &skill_dir.to_string_lossy(), "--force"])
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path().join("home"))
        .assert()
        .success();

    let content = std::fs::read_to_string(
        paths
            .data_dir()
            .join("skills")
            .join("my-skill")
            .join("SKILL.md"),
    )
    .unwrap();
    assert!(content.contains("New content"));
}

#[test]
fn promote_rejects_without_force_non_tty() {
    use assert_cmd::cargo::cargo_bin_cmd;

    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);

    let existing = paths.data_dir().join("skills").join("my-skill");
    std::fs::create_dir_all(&existing).unwrap();
    std::fs::write(
        existing.join("SKILL.md"),
        "---\nname: Old\ndescription: old\n---\nOld",
    )
    .unwrap();
    Library::new().save(&paths).unwrap();

    let skill_dir = tmp.path().join("my-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: New\ndescription: new\n---\nNew",
    )
    .unwrap();

    cargo_bin_cmd!("akm")
        .args(["skills", "promote", &skill_dir.to_string_lossy()])
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path().join("home"))
        .assert()
        .failure()
        .stderr(pred_contains("already exists"));
}

#[test]
fn promote_copies_nested_files_via_cmd() {
    use assert_cmd::cargo::cargo_bin_cmd;

    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);

    std::fs::create_dir_all(paths.data_dir().join("skills")).unwrap();
    Library::new().save(&paths).unwrap();

    let skill_dir = tmp.path().join("nested-skill");
    std::fs::create_dir_all(skill_dir.join("references")).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: Nested\ndescription: Nested skill test\n---\nContent",
    )
    .unwrap();
    std::fs::write(skill_dir.join("references").join("ref.md"), "reference").unwrap();

    cargo_bin_cmd!("akm")
        .args(["skills", "promote", &skill_dir.to_string_lossy(), "--force"])
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path().join("home"))
        .assert()
        .success();

    let dest = paths.data_dir().join("skills").join("nested-skill");
    assert!(dest.join("SKILL.md").is_file());
    assert!(dest.join("references").join("ref.md").is_file());
}

// =============================================================================
// Publish tests
// =============================================================================

#[test]
fn publish_no_personal_registry() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let config = akm::config::Config::default(); // no personal registry

    create_test_library(&paths);

    let result = akm::commands::skills::publish::run(&paths, &config, "tdd", false);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), Error::NoPersonalRegistry));
}

#[test]
fn publish_spec_not_found() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let mut config = akm::config::Config::default();
    config.skills.personal_registry = Some("https://example.com/repo.git".to_string());

    create_test_library(&paths);

    let result =
        akm::commands::skills::publish::run(&paths, &config, "nonexistent-spec-xyz", false);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), Error::SpecNotFound { .. }));
}

// =============================================================================
// Status tests — session scanning via symlinks module
// =============================================================================

#[test]
fn session_symlinks_create_and_scan() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    create_test_library(&paths);
    create_spec_on_disk(paths.data_dir(), "tdd", SpecType::Skill);
    create_spec_on_disk(paths.data_dir(), "reviewer", SpecType::Agent);

    let tool_dirs = test_tool_dirs(&tmp);
    let staging = tmp.path().join("session");
    std::fs::create_dir_all(&staging).unwrap();

    let library = Library::load_checked(&paths).unwrap();

    // Load both specs into session
    let tdd = library.get("tdd").unwrap();
    akm::library::symlinks::create_session(tdd, paths.data_dir(), &staging, tool_dirs.dirs())
        .unwrap();

    let reviewer = library.get("reviewer").unwrap();
    akm::library::symlinks::create_session(reviewer, paths.data_dir(), &staging, tool_dirs.dirs())
        .unwrap();

    // Verify symlinks exist
    assert!(staging
        .join(".claude")
        .join("skills")
        .join("tdd")
        .is_symlink());
    assert!(staging
        .join(".claude")
        .join("agents")
        .join("reviewer.md")
        .is_symlink());
}

#[test]
fn session_dir_empty_has_no_symlinks() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("empty-session");
    std::fs::create_dir_all(&staging).unwrap();

    // No symlinks should exist
    assert!(!staging.join(".claude").join("skills").exists());
}

// =============================================================================
// Error variant tests
// =============================================================================

#[test]
fn error_session_dir_not_found_message() {
    let err = Error::SessionDirNotFound {
        path: "/tmp/missing".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("Session directory does not exist"));
    assert!(msg.contains("/tmp/missing"));
}

#[test]
fn error_spec_already_exists_message() {
    let err = Error::SpecAlreadyExists {
        id: "my-skill".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("my-skill"));
    assert!(msg.contains("--force"));
}

#[test]
fn error_no_personal_registry_message() {
    let err = Error::NoPersonalRegistry;
    let msg = format!("{err}");
    assert!(msg.contains("No personal registry"));
}

#[test]
fn error_no_skill_md_message() {
    let err = Error::NoSkillMd {
        path: "/tmp/my-skill".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("SKILL.md"));
}

// =============================================================================
// Import error variant tests
// =============================================================================

#[test]
fn error_import_invalid_url_message() {
    let err = Error::ImportInvalidUrl {
        url: "https://example.com/not-github".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("Invalid GitHub URL"));
    assert!(msg.contains("https://example.com/not-github"));
    assert!(msg.contains("Expected format"));
}

#[test]
fn error_import_not_github_message() {
    let err = Error::ImportNotGithub {
        url: "https://gitlab.com/acme/repo".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("not from github.com"));
    assert!(msg.contains("gitlab.com"));
}

#[test]
fn error_import_api_failed_message() {
    let err = Error::ImportApiFailed {
        url: "https://api.github.com/repos/acme/repo/contents/skills".into(),
        status: 404,
        message: "Not Found".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("GitHub API error"));
    assert!(msg.contains("404"));
}

#[test]
fn error_import_no_skill_md_message() {
    let err = Error::ImportNoSkillMd {
        url: "https://github.com/acme/repo/tree/main/skills/broken".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("No SKILL.md"));
    assert!(msg.contains("must contain a SKILL.md"));
}

#[test]
fn error_import_download_failed_message() {
    let err = Error::ImportDownloadFailed {
        url: "https://github.com/acme/repo".into(),
        file: "SKILL.md".into(),
        reason: "connection timeout".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("Failed to download"));
    assert!(msg.contains("SKILL.md"));
    assert!(msg.contains("connection timeout"));
}

// =============================================================================
// Import CLI integration tests
// =============================================================================

#[test]
fn import_invalid_url_returns_error() {
    use assert_cmd::cargo::cargo_bin_cmd;

    let tmp = TempDir::new().unwrap();

    cargo_bin_cmd!("akm")
        .args(["skills", "import", "not-a-url"])
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path().join("home"))
        .assert()
        .failure()
        .stderr(pred_contains("Invalid GitHub URL"));
}

#[test]
fn import_non_github_url_returns_error() {
    use assert_cmd::cargo::cargo_bin_cmd;

    let tmp = TempDir::new().unwrap();

    cargo_bin_cmd!("akm")
        .args([
            "skills",
            "import",
            "https://gitlab.com/acme/repo/tree/main/skills/tdd",
        ])
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path().join("home"))
        .assert()
        .failure()
        .stderr(pred_contains("not from github.com"));
}

#[test]
fn import_no_path_url_returns_error() {
    use assert_cmd::cargo::cargo_bin_cmd;

    let tmp = TempDir::new().unwrap();

    cargo_bin_cmd!("akm")
        .args(["skills", "import", "https://github.com/acme/repo"])
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path().join("home"))
        .assert()
        .failure()
        .stderr(pred_contains("Invalid GitHub URL"));
}

#[test]
fn import_missing_url_arg_returns_error() {
    use assert_cmd::cargo::cargo_bin_cmd;

    let tmp = TempDir::new().unwrap();

    cargo_bin_cmd!("akm")
        .args(["skills", "import"])
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CONFIG_HOME", tmp.path().join("config"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path().join("home"))
        .assert()
        .failure()
        .stderr(pred_contains("required"));
}

// =============================================================================
// Git helper tests
// =============================================================================

#[test]
fn git_diff_cached_stat_empty_repo() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::process::Command::new("git")
        .args(["init", "--quiet", "-b", "main"])
        .current_dir(&repo)
        .output()
        .unwrap();

    // Empty staging area should return empty diff stat
    let result = akm::git::Git::diff_cached_stat(&repo);
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[test]
fn git_is_staging_clean_empty() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::process::Command::new("git")
        .args(["init", "--quiet", "-b", "main"])
        .current_dir(&repo)
        .output()
        .unwrap();

    let clean = akm::git::Git::is_staging_clean(&repo).unwrap();
    assert!(clean);
}

#[test]
fn git_is_staging_clean_with_staged_changes() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::process::Command::new("git")
        .args(["init", "--quiet", "-b", "main"])
        .current_dir(&repo)
        .output()
        .unwrap();

    std::fs::write(repo.join("file.txt"), "content").unwrap();
    akm::git::Git::add_all(&repo).unwrap();

    let clean = akm::git::Git::is_staging_clean(&repo).unwrap();
    assert!(!clean);
}

#[test]
fn git_reset_unstages_changes() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::process::Command::new("git")
        .args(["init", "--quiet", "-b", "main"])
        .current_dir(&repo)
        .output()
        .unwrap();

    // Need an initial commit for reset to work — configure identity for CI
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&repo)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("init.txt"), "init").unwrap();
    akm::git::Git::add_all(&repo).unwrap();
    akm::git::Git::commit(&repo, "init").unwrap();

    std::fs::write(repo.join("file.txt"), "content").unwrap();
    akm::git::Git::add_all(&repo).unwrap();
    assert!(!akm::git::Git::is_staging_clean(&repo).unwrap());

    akm::git::Git::reset(&repo).unwrap();
    assert!(akm::git::Git::is_staging_clean(&repo).unwrap());
}
