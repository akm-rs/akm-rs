//! Library and libgen integration tests.
//!
//! These use temp directories to test the full libgen pipeline:
//! scan dirs → extract frontmatter → generate library.json → load and query.

use akm::library::libgen;
use akm::library::spec::{Spec, SpecType};
use akm::library::Library;
use std::fs;
use tempfile::TempDir;

/// Helper: create a skill directory with SKILL.md
fn create_skill(base: &std::path::Path, id: &str, name: &str, desc: &str) {
    let dir = base.join("skills").join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {desc}\n---\n# {name}\n"),
    )
    .unwrap();
}

/// Helper: create an agent file
fn create_agent(base: &std::path::Path, id: &str, name: &str, desc: &str) {
    let dir = base.join("agents");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(format!("{id}.md")),
        format!("---\nname: {name}\ndescription: {desc}\n---\n# {name}\n"),
    )
    .unwrap();
}

#[test]
fn libgen_creates_library_from_skills_and_agents() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path();

    create_skill(
        base,
        "tdd",
        "Test-Driven Development",
        "Use when writing tests first",
    );
    create_skill(base, "debugging", "Debugging", "Use when debugging");
    create_agent(base, "reviewer", "Code Reviewer", "Reviews code");

    let result = libgen::generate(base).unwrap();
    assert_eq!(result.count, 3);

    let lib = Library::load_from(&result.library_path).unwrap();
    assert_eq!(lib.specs.len(), 3);

    let tdd = lib.get("tdd").unwrap();
    assert_eq!(tdd.spec_type, SpecType::Skill);
    assert_eq!(tdd.name, "Test-Driven Development");
    assert_eq!(tdd.description, "Use when writing tests first");
    assert!(!tdd.core);

    let reviewer = lib.get("reviewer").unwrap();
    assert_eq!(reviewer.spec_type, SpecType::Agent);
}

#[test]
fn libgen_preserves_existing_metadata() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path();

    create_skill(base, "tdd", "TDD", "Original desc");

    // First libgen
    libgen::generate(base).unwrap();

    // Modify the library to add metadata
    let mut lib = Library::load_from(&base.join("library.json")).unwrap();
    let spec = lib.get_mut("tdd").unwrap();
    spec.core = true;
    spec.tags = vec!["testing".into(), "tdd".into()];
    spec.source = Some("https://example.com".into());
    lib.save_to(&base.join("library.json")).unwrap();

    // Second libgen — should preserve the metadata
    libgen::generate(base).unwrap();

    let lib = Library::load_from(&base.join("library.json")).unwrap();
    let spec = lib.get("tdd").unwrap();
    assert!(spec.core, "core flag should be preserved");
    assert_eq!(
        spec.tags,
        vec!["testing", "tdd"],
        "tags should be preserved"
    );
    assert_eq!(
        spec.source.as_deref(),
        Some("https://example.com"),
        "source should be preserved"
    );
}

#[test]
fn libgen_drops_removed_specs() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path();

    create_skill(base, "tdd", "TDD", "Desc");
    create_skill(base, "debugging", "Debug", "Desc");
    libgen::generate(base).unwrap();

    // Remove one skill from disk
    fs::remove_dir_all(base.join("skills/debugging")).unwrap();

    libgen::generate(base).unwrap();
    let lib = Library::load_from(&base.join("library.json")).unwrap();
    assert_eq!(lib.specs.len(), 1);
    assert!(lib.contains("tdd"));
    assert!(!lib.contains("debugging"));
}

#[test]
fn libgen_errors_on_no_spec_dirs() {
    let tmp = TempDir::new().unwrap();
    let result = libgen::generate(tmp.path());
    assert!(result.is_err());
}

#[test]
fn library_query_methods() {
    let lib = Library {
        version: 1,
        specs: vec![
            Spec::new("tdd", SpecType::Skill, "TDD", "Desc"),
            Spec {
                core: true,
                ..Spec::new("core-skill", SpecType::Skill, "Core", "Desc")
            },
            Spec::new("reviewer", SpecType::Agent, "Reviewer", "Desc"),
        ],
    };

    assert_eq!(lib.len(), 3);
    assert!(lib.contains("tdd"));
    assert!(!lib.contains("nonexistent"));
    assert_eq!(lib.core_ids(), vec!["core-skill"]);
    assert_eq!(lib.all_ids().len(), 3);
}

#[test]
fn spec_source_path_skill() {
    let spec = Spec::new("tdd", SpecType::Skill, "TDD", "Desc");
    let path = spec.source_path(std::path::Path::new("/data/akm"));
    assert_eq!(path, std::path::PathBuf::from("/data/akm/skills/tdd"));
}

#[test]
fn spec_source_path_agent() {
    let spec = Spec::new("reviewer", SpecType::Agent, "Reviewer", "Desc");
    let path = spec.source_path(std::path::Path::new("/data/akm"));
    assert_eq!(
        path,
        std::path::PathBuf::from("/data/akm/agents/reviewer.md")
    );
}

#[test]
fn library_roundtrip_serialization() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("library.json");

    let mut lib = Library::new();
    lib.specs.push(Spec {
        source: Some("https://example.com".into()),
        ..Spec::new("tdd", SpecType::Skill, "TDD", "Desc")
    });

    lib.save_to(&path).unwrap();
    let loaded = Library::load_from(&path).unwrap();
    assert_eq!(loaded.specs.len(), 1);
    assert_eq!(loaded.specs[0].id, "tdd");
    assert_eq!(
        loaded.specs[0].source.as_deref(),
        Some("https://example.com")
    );
}

#[test]
fn libgen_skills_without_skill_md_are_skipped() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path();

    // Directory without SKILL.md — should be skipped
    fs::create_dir_all(base.join("skills/no-skill-md")).unwrap();
    fs::write(base.join("skills/no-skill-md/README.md"), "not a skill").unwrap();

    // Valid skill
    create_skill(base, "valid", "Valid", "Desc");

    let result = libgen::generate(base).unwrap();
    assert_eq!(result.count, 1);
}
