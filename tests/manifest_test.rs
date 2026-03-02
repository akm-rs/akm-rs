//! Manifest integration tests.
//!
//! Tests read/write of .agents/akm.json.

use akm::library::manifest::Manifest;
use akm::library::spec::SpecType;
use std::fs;
use tempfile::TempDir;

#[test]
fn manifest_load_or_create_creates_empty() {
    let tmp = TempDir::new().unwrap();
    let manifest = Manifest::load_or_create(tmp.path()).unwrap();
    assert!(manifest.skills.is_empty());
    assert!(manifest.agents.is_empty());
    assert!(tmp.path().join(".agents/akm.json").exists());
}

#[test]
fn manifest_load_or_create_idempotent() {
    let tmp = TempDir::new().unwrap();
    let m1 = Manifest::load_or_create(tmp.path()).unwrap();
    let m2 = Manifest::load_or_create(tmp.path()).unwrap();
    assert_eq!(m1.skills, m2.skills);
    assert_eq!(m1.agents, m2.agents);
}

#[test]
fn manifest_add_skill_idempotent() {
    let tmp = TempDir::new().unwrap();
    let mut manifest = Manifest::load_or_create(tmp.path()).unwrap();

    assert!(manifest.add("tdd", SpecType::Skill));
    assert!(!manifest.add("tdd", SpecType::Skill)); // second add returns false
    assert_eq!(manifest.skills, vec!["tdd"]);
}

#[test]
fn manifest_add_agent() {
    let tmp = TempDir::new().unwrap();
    let mut manifest = Manifest::load_or_create(tmp.path()).unwrap();

    manifest.add("reviewer", SpecType::Agent);
    assert_eq!(manifest.agents, vec!["reviewer"]);
    assert!(manifest.skills.is_empty());
}

#[test]
fn manifest_remove_skill() {
    let tmp = TempDir::new().unwrap();
    let mut manifest = Manifest::load_or_create(tmp.path()).unwrap();

    manifest.add("tdd", SpecType::Skill);
    manifest.add("debugging", SpecType::Skill);

    assert!(manifest.remove("tdd", Some(SpecType::Skill)));
    assert_eq!(manifest.skills, vec!["debugging"]);

    // Remove non-existent is no-op
    assert!(!manifest.remove("tdd", Some(SpecType::Skill)));
}

#[test]
fn manifest_remove_unknown_type_tries_both() {
    let tmp = TempDir::new().unwrap();
    let mut manifest = Manifest::load_or_create(tmp.path()).unwrap();

    manifest.add("something", SpecType::Skill);
    assert!(manifest.remove("something", None)); // Should find it in skills
    assert!(manifest.skills.is_empty());
}

#[test]
fn manifest_save_and_reload() {
    let tmp = TempDir::new().unwrap();
    let mut manifest = Manifest::load_or_create(tmp.path()).unwrap();

    manifest.add("tdd", SpecType::Skill);
    manifest.add("reviewer", SpecType::Agent);
    manifest.save().unwrap();

    let reloaded = Manifest::load(tmp.path()).unwrap();
    assert_eq!(reloaded.skills, vec!["tdd"]);
    assert_eq!(reloaded.agents, vec!["reviewer"]);
}

#[test]
fn manifest_contains() {
    let tmp = TempDir::new().unwrap();
    let mut manifest = Manifest::load_or_create(tmp.path()).unwrap();

    manifest.add("tdd", SpecType::Skill);
    manifest.add("reviewer", SpecType::Agent);

    assert!(manifest.contains("tdd"));
    assert!(manifest.contains("reviewer"));
    assert!(!manifest.contains("nonexistent"));
}

#[test]
fn manifest_all_ids() {
    let tmp = TempDir::new().unwrap();
    let mut manifest = Manifest::load_or_create(tmp.path()).unwrap();

    manifest.add("tdd", SpecType::Skill);
    manifest.add("reviewer", SpecType::Agent);

    let all = manifest.all_ids();
    assert_eq!(all.len(), 2);
    assert!(all.contains(&"tdd"));
    assert!(all.contains(&"reviewer"));
}

#[test]
fn manifest_load_existing_file() {
    let tmp = TempDir::new().unwrap();
    let agents_dir = tmp.path().join(".agents");
    fs::create_dir_all(&agents_dir).unwrap();
    fs::write(
        agents_dir.join("akm.json"),
        r#"{"skills":["tdd","debugging"],"agents":["reviewer"]}"#,
    )
    .unwrap();

    let manifest = Manifest::load(tmp.path()).unwrap();
    assert_eq!(manifest.skills, vec!["tdd", "debugging"]);
    assert_eq!(manifest.agents, vec!["reviewer"]);
}

#[test]
fn manifest_add_maintains_sorted_order() {
    let tmp = TempDir::new().unwrap();
    let mut manifest = Manifest::load_or_create(tmp.path()).unwrap();

    manifest.add("zebra", SpecType::Skill);
    manifest.add("alpha", SpecType::Skill);
    manifest.add("mid", SpecType::Skill);

    assert_eq!(manifest.skills, vec!["alpha", "mid", "zebra"]);
}
