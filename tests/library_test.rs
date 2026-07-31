//! Library and libgen integration tests.
//!
//! These use temp directories to test the full libgen pipeline:
//! scan dirs → extract frontmatter → generate library.json → load and query.

use akm::library::libgen;
use akm::library::spec::{Spec, SpecMeta, SpecType};
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

    let result = libgen::generate(base, &base.join("library.json")).unwrap();
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

/// libgen runs on every sync, so it must never write into the registry tree.
/// Seeding sidecars there would make a fresh clone of a sidecar-less registry
/// report every single spec as locally modified.
#[test]
fn libgen_never_writes_sidecars() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path();

    create_skill(base, "tdd", "TDD", "Use when writing tests first");
    create_agent(base, "reviewer", "Reviewer", "Reviews code");

    libgen::generate(base, &base.join("library.json")).unwrap();

    assert!(!base.join("skills").join("tdd").join("akm.json").exists());
    assert!(!base.join("agents").join("reviewer.akm.json").exists());
}

/// Without a sidecar the frontmatter answers for the spec, so the library is
/// fully usable against a registry that has never seen AKM metadata.
#[test]
fn libgen_falls_back_to_frontmatter_when_there_is_no_sidecar() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path();

    create_skill(base, "tdd", "TDD", "Use when writing tests first");
    libgen::generate(base, &base.join("library.json")).unwrap();

    let lib = Library::load_from(&base.join("library.json")).unwrap();
    let spec = lib.get("tdd").unwrap();
    assert_eq!(spec.name, "TDD");
    assert_eq!(spec.description, "Use when writing tests first");
    assert!(!spec.core);
    assert!(spec.tags.is_empty());
}

/// The sidecar is the source of truth: metadata edited there survives libgen,
/// and its human-facing description is independent of the frontmatter one.
#[test]
fn libgen_reads_metadata_from_the_sidecar() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path();

    create_skill(base, "tdd", "TDD", "LLM-facing trigger text");

    let sidecar = base.join("skills").join("tdd").join("akm.json");
    SpecMeta {
        name: "TDD".into(),
        description: "Human-facing prose".into(),
        tags: vec!["testing".into(), "tdd".into()],
        core: true,
        triggers: Default::default(),
        source: Some("https://example.com".into()),
    }
    .save_to(&sidecar)
    .unwrap();

    libgen::generate(base, &base.join("library.json")).unwrap();

    let lib = Library::load_from(&base.join("library.json")).unwrap();
    let spec = lib.get("tdd").unwrap();
    assert_eq!(spec.description, "Human-facing prose");
    assert_eq!(spec.tags, vec!["testing", "tdd"]);
    assert!(spec.core);
    assert_eq!(spec.source.as_deref(), Some("https://example.com"));

    // The frontmatter is untouched — the two descriptions are separate fields.
    let md = fs::read_to_string(base.join("skills").join("tdd").join("SKILL.md")).unwrap();
    assert!(md.contains("LLM-facing trigger text"));
}

/// library.json is derived, never authoritative: edits made to it directly are
/// discarded on the next libgen rather than fighting the sidecars.
#[test]
fn libgen_discards_edits_made_to_the_derived_index() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path();

    create_skill(base, "tdd", "TDD", "Original desc");
    libgen::generate(base, &base.join("library.json")).unwrap();

    let mut lib = Library::load_from(&base.join("library.json")).unwrap();
    lib.get_mut("tdd").unwrap().core = true;
    lib.save_to(&base.join("library.json")).unwrap();

    libgen::generate(base, &base.join("library.json")).unwrap();

    let lib = Library::load_from(&base.join("library.json")).unwrap();
    assert!(!lib.get("tdd").unwrap().core);
}

/// A corrupt sidecar must not take the whole library down with it.
#[test]
fn libgen_survives_an_unparseable_sidecar() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path();

    create_skill(base, "tdd", "TDD", "Original desc");
    create_skill(base, "debugging", "Debugging", "Other desc");

    let sidecar = base.join("skills").join("tdd").join("akm.json");
    fs::write(&sidecar, "{ not json").unwrap();

    let result = libgen::generate(base, &base.join("library.json")).unwrap();
    assert_eq!(result.count, 2);

    // Falls back to the frontmatter in memory, and leaves the broken file be.
    let lib = Library::load_from(&base.join("library.json")).unwrap();
    assert_eq!(lib.get("tdd").unwrap().description, "Original desc");
    assert_eq!(fs::read_to_string(&sidecar).unwrap(), "{ not json");
}

#[test]
fn libgen_drops_removed_specs() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path();

    create_skill(base, "tdd", "TDD", "Desc");
    create_skill(base, "debugging", "Debug", "Desc");
    libgen::generate(base, &base.join("library.json")).unwrap();

    // Remove one skill from disk
    fs::remove_dir_all(base.join("skills/debugging")).unwrap();

    libgen::generate(base, &base.join("library.json")).unwrap();
    let lib = Library::load_from(&base.join("library.json")).unwrap();
    assert_eq!(lib.specs.len(), 1);
    assert!(lib.contains("tdd"));
    assert!(!lib.contains("debugging"));
}

/// Drift detection maps changed paths back to the spec that owns them, so the
/// mapping has to hold for every shape a spec takes on disk.
#[test]
fn owner_of_maps_paths_back_to_their_spec() {
    use SpecType::{Agent, Skill};

    for (path, expected) in [
        ("skills/tdd/SKILL.md", Some((Skill, "tdd"))),
        ("skills/tdd/akm.json", Some((Skill, "tdd"))),
        ("skills/tdd/references/deep/file.md", Some((Skill, "tdd"))),
        ("agents/reviewer.md", Some((Agent, "reviewer"))),
        ("agents/reviewer.akm.json", Some((Agent, "reviewer"))),
        ("library.json", None),
        ("instructions/global.md", None),
        ("skills/", None),
        ("agents/nested/thing.md", None),
        ("agents/notes.txt", None),
    ] {
        let got = SpecType::owner_of(path);
        let expected = expected.map(|(t, id)| (t, id.to_string()));
        assert_eq!(got, expected, "for {path}");
    }
}

/// An agent's two files are named explicitly, so a spec can never stage or
/// discard a neighbour's files through a loose glob.
#[test]
fn pathspecs_cover_exactly_one_spec() {
    assert_eq!(SpecType::Skill.pathspecs("tdd"), vec!["skills/tdd"]);
    assert_eq!(
        SpecType::Agent.pathspecs("reviewer"),
        vec!["agents/reviewer.md", "agents/reviewer.akm.json"]
    );
}

#[test]
fn libgen_errors_on_no_spec_dirs() {
    let tmp = TempDir::new().unwrap();
    let result = libgen::generate(tmp.path(), &tmp.path().join("library.json"));
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

    let result = libgen::generate(base, &base.join("library.json")).unwrap();
    assert_eq!(result.count, 1);
}

/// The cold library's index is machine-local and lives outside the registry
/// working tree, so the directory libgen scans and the file it writes are two
/// different places.
#[test]
fn libgen_writes_the_index_outside_the_scanned_directory() {
    let tmp = TempDir::new().unwrap();
    let scan = tmp.path().join("library");
    fs::create_dir_all(&scan).unwrap();
    create_skill(&scan, "tdd", "TDD", "Desc");
    let out = tmp.path().join("library.json");

    let result = libgen::generate(&scan, &out).unwrap();

    assert_eq!(result.library_path, out);
    assert!(!scan.join("library.json").exists());
    assert!(Library::load_from(&out).unwrap().contains("tdd"));
}

#[test]
fn libgen_empty_skills_dir_succeeds_with_zero_specs() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let result = libgen::generate(tmp.path(), &tmp.path().join("library.json")).unwrap();
    assert_eq!(result.count, 0);
}

#[test]
fn library_load_malformed_json_returns_parse_error() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("library.json"), "not json").unwrap();
    assert!(matches!(
        Library::load_from(&tmp.path().join("library.json")),
        Err(akm::error::Error::LibraryParse { .. })
    ));
}

#[test]
fn library_load_nonexistent_returns_not_found() {
    let tmp = TempDir::new().unwrap();
    assert!(matches!(
        Library::load_from(&tmp.path().join("library.json")),
        Err(akm::error::Error::LibraryNotFound { .. })
    ));
}
