//! End-to-end `akm skills sync` against a real personal registry.
//!
//! These exercise the property the whole rc4 rework exists for: syncing must
//! apply the registry's updates *without* destroying uncommitted local work.

use akm::commands::skills::sync::{self, RegistryOutcome};
use akm::library::drift::DriftState;
use akm::library::tool_dirs::ToolDirs;
use akm::library::Library;
use akm::paths::Paths;
use akm::registry::Registry;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn skill_md(id: &str, body: &str) -> String {
    format!("---\nname: {id}\ndescription: desc for {id}\n---\n{body}\n")
}

struct Env {
    _tmp: TempDir,
    origin: PathBuf,
    paths: Paths,
    home: PathBuf,
}

impl Env {
    /// A registry with two skills, and an AKM install that has never synced.
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let origin = tmp.path().join("origin");
        std::fs::create_dir_all(&origin).unwrap();

        git(&origin, &["init", "-b", "main"]);
        git(&origin, &["config", "user.email", "test@example.com"]);
        git(&origin, &["config", "user.name", "Test"]);
        for id in ["alpha", "beta"] {
            write(
                &origin.join(format!("skills/{id}/SKILL.md")),
                &skill_md(id, "v1"),
            );
        }
        git(&origin, &["add", "-A"]);
        git(&origin, &["commit", "-m", "initial"]);
        git(&origin, &["config", "receive.denyCurrentBranch", "ignore"]);

        let home = tmp.path().join("home");
        let paths = Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            &home,
        );

        Self {
            _tmp: tmp,
            origin,
            paths,
            home,
        }
    }

    fn registry(&self) -> Registry {
        Registry::new(self.origin.to_string_lossy(), self.paths.library_dir())
    }

    fn sync(&self) -> sync::SyncReport {
        sync::execute(
            &self.paths,
            &self.registry(),
            &ToolDirs::builtin(&self.home),
        )
        .unwrap()
    }

    fn library_file(&self, rel: &str) -> PathBuf {
        self.paths.library_dir().join(rel)
    }

    fn commit_to_origin(&self, rel: &str, content: &str) {
        write(&self.origin.join(rel), content);
        git(&self.origin, &["add", "-A"]);
        git(&self.origin, &["commit", "-m", &format!("update {rel}")]);
    }
}

#[test]
fn first_sync_clones_the_registry_and_builds_the_library() {
    let env = Env::new();

    let report = env.sync();

    assert!(matches!(report.registry, RegistryOutcome::Cloned));
    assert_eq!(report.spec_count, Some(2));
    // A registry without sidecars is not "modified" — nothing was written to it.
    assert!(report.drift.is_clean());

    let library = Library::load_from(&env.paths.library_json()).unwrap();
    assert!(library.contains("alpha"));
    assert!(library.contains("beta"));
}

/// The whole point of the rework: an uncommitted edit to one skill survives a
/// sync that updates a different one.
#[test]
fn sync_preserves_local_edits_to_untouched_specs() {
    let env = Env::new();
    env.sync();

    let alpha = env.library_file("skills/alpha/SKILL.md");
    write(&alpha, &skill_md("alpha", "LOCAL WORK IN PROGRESS"));

    env.commit_to_origin("skills/beta/SKILL.md", &skill_md("beta", "v2"));
    let report = env.sync();

    assert!(matches!(report.registry, RegistryOutcome::Updated { .. }));
    assert!(std::fs::read_to_string(&alpha)
        .unwrap()
        .contains("LOCAL WORK IN PROGRESS"));
    assert!(
        std::fs::read_to_string(env.library_file("skills/beta/SKILL.md"))
            .unwrap()
            .contains("v2")
    );
    assert_eq!(report.drift.state_of("alpha"), DriftState::LocalNewer);
    assert_eq!(report.drift.state_of("beta"), DriftState::Clean);
}

/// When both sides changed the *same* skill, the fast-forward is blocked. Sync
/// parks the local edit, applies the update, and puts the edit back on top —
/// so the other forty-nine skills are not held hostage by one conflict.
#[test]
fn sync_parks_and_restores_edits_that_block_the_fast_forward() {
    let env = Env::new();
    env.sync();

    let alpha = env.library_file("skills/alpha/SKILL.md");
    write(&alpha, &skill_md("alpha", "LOCAL WORK IN PROGRESS"));

    env.commit_to_origin("skills/alpha/SKILL.md", &skill_md("alpha", "remote v2"));
    env.commit_to_origin("skills/beta/SKILL.md", &skill_md("beta", "remote v2"));

    let report = env.sync();

    match &report.registry {
        RegistryOutcome::Updated { parked } => assert_eq!(parked, &vec!["alpha".to_string()]),
        other => panic!("expected Updated, got {other:?}"),
    }

    // The local edit is still there, on top of the new baseline...
    assert!(std::fs::read_to_string(&alpha)
        .unwrap()
        .contains("LOCAL WORK IN PROGRESS"));
    // ...the untouched skill did get the update...
    assert!(
        std::fs::read_to_string(env.library_file("skills/beta/SKILL.md"))
            .unwrap()
            .contains("remote v2")
    );
    // ...and no merge ever ran, so nothing has conflict markers in it.
    assert!(!std::fs::read_to_string(&alpha).unwrap().contains("<<<<<<<"));
    // The remote version is now the baseline and the local edit sits on top of
    // it, so publishing from here is a clean fast-forward.
    assert_eq!(report.drift.state_of("alpha"), DriftState::LocalNewer);
}

/// A skill added upstream reaches the library, and one deleted upstream leaves.
#[test]
fn sync_applies_additions_and_deletions_from_the_registry() {
    let env = Env::new();
    env.sync();

    env.commit_to_origin("skills/gamma/SKILL.md", &skill_md("gamma", "v1"));
    std::fs::remove_dir_all(env.origin.join("skills/beta")).unwrap();
    git(&env.origin, &["add", "-A"]);
    git(&env.origin, &["commit", "-m", "drop beta"]);

    let report = env.sync();

    let library = Library::load_from(&env.paths.library_json()).unwrap();
    assert!(library.contains("gamma"));
    assert!(!library.contains("beta"));
    assert!(report.drift.is_clean());
}

/// `core` published in a sidecar reaches every machine, and the symlinks that
/// follow from it are rebuilt on sync.
#[test]
fn published_core_defaults_propagate_and_create_symlinks() {
    let env = Env::new();
    env.sync();

    env.commit_to_origin(
        "skills/alpha/akm.json",
        r#"{"name":"alpha","description":"desc","tags":[],"core":true,"triggers":{}}"#,
    );

    let report = env.sync();

    assert_eq!(report.symlink_count, 1);
    assert!(env.home.join(".claude/skills/alpha").is_symlink());
    assert!(!env.home.join(".claude/skills/beta").exists());
}

/// A machine that has switched a skill off keeps it off across syncs, without
/// that choice ever reaching the registry.
#[test]
fn machine_local_core_deviations_survive_sync() {
    use akm::library::local::LocalOverrides;

    let env = Env::new();
    env.commit_to_origin(
        "skills/alpha/akm.json",
        r#"{"name":"alpha","description":"desc","tags":[],"core":true,"triggers":{}}"#,
    );
    env.sync();
    assert!(env.home.join(".claude/skills/alpha").is_symlink());

    // This machine opts out.
    let mut overrides = LocalOverrides::default();
    overrides.set_core("alpha", false, true);
    overrides.save_to(&env.paths.local_json()).unwrap();

    let report = env.sync();

    assert_eq!(report.symlink_count, 0);
    assert!(!env.home.join(".claude/skills/alpha").exists());

    // The sidecar in the registry still says core — the opt-out stayed local.
    let sidecar = std::fs::read_to_string(env.library_file("skills/alpha/akm.json")).unwrap();
    assert!(sidecar.contains("\"core\": true") || sidecar.contains("\"core\":true"));
    assert!(report.drift.state_of("alpha") == DriftState::Clean);
}

/// Sync regenerates `library.json` every run, so it must not be allowed to
/// register as a change the user might publish.
#[test]
fn the_derived_index_never_shows_up_as_drift() {
    let env = Env::new();
    env.sync();
    let report = env.sync();

    assert!(report.drift.is_clean());
    assert!(env.paths.library_json().is_file());
}

/// `library.json` is derived, so the TUI's edits have to land in the files
/// that own them — the sidecar and `local.json` — or the next sync erases them.
#[test]
fn tui_edits_survive_the_next_sync() {
    use akm::tui::app::App;

    let env = Env::new();
    env.sync();

    let mut app = App::new(env.paths.clone(), ToolDirs::builtin(&env.home)).unwrap();
    if let Some(spec) = app.library.get_mut("alpha") {
        spec.description = "Human-facing prose".into();
        spec.tags = vec!["curated".into()];
    }
    app.library_dirty = true;
    app.edited_meta.insert("alpha".into());
    app.toggle_core("beta");
    app.save_if_dirty().unwrap();

    env.sync();

    let library = Library::load_from(&env.paths.library_json()).unwrap();
    let alpha = library.get("alpha").unwrap();
    assert_eq!(alpha.description, "Human-facing prose");
    assert_eq!(alpha.tags, vec!["curated"]);
    assert!(library.get("beta").unwrap().core);

    // The metadata edit is publishable; the core toggle stayed on this machine.
    let sidecar = std::fs::read_to_string(env.library_file("skills/alpha/akm.json")).unwrap();
    assert!(sidecar.contains("Human-facing prose"));
    assert!(!env.library_file("skills/beta/akm.json").exists());
}

/// Sync is idempotent: running it twice changes nothing the second time.
#[test]
fn sync_is_idempotent() {
    let env = Env::new();
    env.sync();

    let report = env.sync();

    assert!(matches!(report.registry, RegistryOutcome::UpToDate));
    assert_eq!(report.spec_count, Some(2));
    assert!(report.drift.is_clean());
}

/// With no registry configured, sync still regenerates and links what is on
/// disk rather than failing — an offline machine keeps working.
#[test]
fn sync_without_a_registry_uses_what_is_on_disk() {
    let env = Env::new();
    write(
        &env.library_file("skills/local-only/SKILL.md"),
        &skill_md("local-only", "v1"),
    );

    let registry = Registry::new("", env.paths.library_dir());
    let report = sync::execute(&env.paths, &registry, &ToolDirs::builtin(&env.home)).unwrap();

    assert!(matches!(report.registry, RegistryOutcome::Skipped));
    assert_eq!(report.spec_count, Some(1));
}
