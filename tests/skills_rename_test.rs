//! Integration tests for `akm skills rename`.

use akm::commands::skills::rename;
use akm::error::Error;
use akm::library::libgen;
use akm::library::local::LocalOverrides;
use akm::library::spec::SpecType;
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

/// Regenerate library.json from whatever is currently on disk.
fn regen(paths: &Paths) {
    libgen::generate(&paths.library_dir(), &paths.library_json()).unwrap();
}

#[test]
fn rename_moves_a_skill_directory() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let tool_dirs = test_tool_dirs(&tmp);
    write_skill(&paths.library_dir(), "old-skill");
    regen(&paths);

    let spec_type = rename::apply(&paths, "old-skill", "new-skill", &tool_dirs).unwrap();
    assert_eq!(spec_type, SpecType::Skill);

    assert!(!paths
        .library_dir()
        .join("skills")
        .join("old-skill")
        .exists());
    assert!(paths
        .library_dir()
        .join("skills")
        .join("new-skill")
        .join("SKILL.md")
        .is_file());

    let library = Library::load(&paths).unwrap();
    assert!(library.get("old-skill").is_none());
    assert!(library.get("new-skill").is_some());
}

#[test]
fn rename_moves_agent_markdown_and_sidecar() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let tool_dirs = test_tool_dirs(&tmp);
    let agents = paths.library_dir().join("agents");
    write_agent(&paths.library_dir(), "old-agent");
    std::fs::write(
        agents.join("old-agent.akm.json"),
        r#"{"name":"Old","description":"d","tags":[],"core":false,"triggers":{}}"#,
    )
    .unwrap();
    regen(&paths);

    rename::apply(&paths, "old-agent", "new-agent", &tool_dirs).unwrap();

    assert!(!agents.join("old-agent.md").exists());
    assert!(!agents.join("old-agent.akm.json").exists());
    assert!(agents.join("new-agent.md").is_file());
    assert!(agents.join("new-agent.akm.json").is_file());
}

#[test]
fn rename_rejects_collision_with_existing_spec() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let tool_dirs = test_tool_dirs(&tmp);
    write_skill(&paths.library_dir(), "alpha");
    write_skill(&paths.library_dir(), "beta");
    regen(&paths);

    let err = rename::apply(&paths, "alpha", "beta", &tool_dirs).unwrap_err();
    assert!(matches!(err, Error::SpecAlreadyExists { .. }));
    // Nothing moved.
    assert!(paths.library_dir().join("skills").join("alpha").exists());
}

#[test]
fn rename_rejects_invalid_new_id() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let tool_dirs = test_tool_dirs(&tmp);
    write_skill(&paths.library_dir(), "alpha");
    regen(&paths);

    let err = rename::apply(&paths, "alpha", "a/b", &tool_dirs).unwrap_err();
    assert!(matches!(err, Error::InvalidSpecId { .. }));
}

#[test]
fn rename_fails_for_missing_spec() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let tool_dirs = test_tool_dirs(&tmp);
    write_skill(&paths.library_dir(), "alpha");
    regen(&paths);

    let err = rename::apply(&paths, "ghost", "phantom", &tool_dirs).unwrap_err();
    assert!(matches!(err, Error::SpecNotFound { .. }));
}

#[test]
fn rename_carries_the_local_core_override() {
    let tmp = TempDir::new().unwrap();
    let paths = test_paths(&tmp);
    let tool_dirs = test_tool_dirs(&tmp);
    write_skill(&paths.library_dir(), "old-skill");
    regen(&paths);

    // This machine has flipped the skill's core on, deviating from the default.
    let mut overrides = LocalOverrides::default();
    overrides.core.insert("old-skill".to_string(), true);
    overrides.save_to(&paths.local_json()).unwrap();

    rename::apply(&paths, "old-skill", "new-skill", &tool_dirs).unwrap();

    let overrides = LocalOverrides::load_from(&paths.local_json()).unwrap();
    assert_eq!(overrides.core.get("old-skill"), None);
    assert_eq!(overrides.core.get("new-skill"), Some(&true));
    assert!(
        Library::load(&paths)
            .unwrap()
            .get("new-skill")
            .unwrap()
            .core
    );
}
