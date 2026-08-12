//! Integration tests for `akm skills delete`.

use akm::commands::skills::delete;
use akm::error::Error;
use akm::library::libgen;
use akm::library::local::LocalOverrides;
use akm::library::tool_dirs::ToolDirs;
use akm::library::Library;
use akm::paths::Paths;
use std::path::Path;
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

fn write_skill(library_dir: &Path, id: &str) {
    let dir = library_dir.join("skills").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {id}\ndescription: a test skill\n---\nBody"),
    )
    .unwrap();
}

fn write_agent(library_dir: &Path, id: &str) {
    let dir = library_dir.join("agents");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{id}.md")),
        format!("---\nname: {id}\ndescription: a test agent\n---\nBody"),
    )
    .unwrap();
}

fn regen(paths: &Paths) {
    libgen::generate(&paths.library_dir(), &paths.library_json()).unwrap();
}

#[test]
fn delete_removes_a_skill_directory() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let tool_dirs = test_tool_dirs(&tmp);
    write_skill(&paths.library_dir(), "doomed");
    write_skill(&paths.library_dir(), "keeper");
    regen(&paths);

    delete::apply(&paths, "doomed", &tool_dirs).unwrap();

    assert!(!paths.library_dir().join("skills").join("doomed").exists());
    assert!(paths.library_dir().join("skills").join("keeper").exists());

    let library = Library::load(&paths).unwrap();
    assert!(library.get("doomed").is_none());
    assert!(library.get("keeper").is_some());
}

#[test]
fn delete_removes_agent_markdown_and_sidecar() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let tool_dirs = test_tool_dirs(&tmp);
    let agents = paths.library_dir().join("agents");
    write_agent(&paths.library_dir(), "bot");
    std::fs::write(
        agents.join("bot.akm.json"),
        r#"{"name":"Bot","description":"d","tags":[],"core":false,"triggers":{}}"#,
    )
    .unwrap();
    regen(&paths);

    delete::apply(&paths, "bot", &tool_dirs).unwrap();

    assert!(!agents.join("bot.md").exists());
    assert!(!agents.join("bot.akm.json").exists());
}

#[test]
fn delete_clears_the_local_core_override() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let tool_dirs = test_tool_dirs(&tmp);
    write_skill(&paths.library_dir(), "doomed");
    regen(&paths);

    let mut overrides = LocalOverrides::default();
    overrides.core.insert("doomed".to_string(), true);
    overrides.save_to(&paths.local_json()).unwrap();

    delete::apply(&paths, "doomed", &tool_dirs).unwrap();

    let overrides = LocalOverrides::load_from(&paths.local_json()).unwrap();
    assert_eq!(overrides.core.get("doomed"), None);
}

#[test]
fn delete_fails_for_missing_spec() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let tool_dirs = test_tool_dirs(&tmp);
    write_skill(&paths.library_dir(), "alpha");
    regen(&paths);

    let err = delete::apply(&paths, "ghost", &tool_dirs).unwrap_err();
    assert!(matches!(err, Error::SpecNotFound { .. }));
}

/// The `run` entry point refuses to delete in a non-TTY without `--force`.
/// Tests run without a controlling terminal, so this exercises that path.
#[test]
fn delete_requires_force_without_a_tty() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let tool_dirs = test_tool_dirs(&tmp);
    write_skill(&paths.library_dir(), "alpha");
    regen(&paths);
    let config = akm::config::Config::default();

    let err = delete::run(&paths, &config, "alpha", false, &tool_dirs).unwrap_err();
    assert!(matches!(err, Error::ConfirmationRequired { .. }));
    // The spec is untouched.
    assert!(paths.library_dir().join("skills").join("alpha").exists());

    // With --force it goes through even without a TTY.
    delete::run(&paths, &config, "alpha", true, &tool_dirs).unwrap();
    assert!(!paths.library_dir().join("skills").join("alpha").exists());
}
