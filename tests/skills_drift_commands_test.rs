//! `akm skills diff`, `revert` and `core` against a real personal registry.

use akm::commands::skills::core::{self, CoreAction};
use akm::commands::skills::{diff, revert, sync};
use akm::config::Config;
use akm::library::local::LocalOverrides;
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
    config: Config,
    home: PathBuf,
}

impl Env {
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
        write(
            &origin.join("skills/alpha/akm.json"),
            r#"{"name":"alpha","description":"desc","tags":[],"core":true,"triggers":{}}"#,
        );
        git(&origin, &["add", "-A"]);
        git(&origin, &["commit", "-m", "initial"]);

        let home = tmp.path().join("home");
        let paths = Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            &home,
        );

        let mut config = Config::default();
        config.skills.personal_registry = Some(origin.to_string_lossy().to_string());

        Self {
            _tmp: tmp,
            origin,
            paths,
            config,
            home,
        }
    }

    fn tool_dirs(&self) -> ToolDirs {
        ToolDirs::builtin(&self.home)
    }

    fn sync(&self) {
        let registry = Registry::new(
            self.config.registry_url().unwrap(),
            self.paths.library_dir(),
        );
        sync::execute(&self.paths, &registry, &self.tool_dirs()).unwrap();
    }

    fn library_file(&self, rel: &str) -> PathBuf {
        self.paths.library_dir().join(rel)
    }

    fn commit_to_origin(&self, rel: &str, content: &str) {
        write(&self.origin.join(rel), content);
        git(&self.origin, &["add", "-A"]);
        git(&self.origin, &["commit", "-m", "upstream change"]);
    }
}

// --- diff ---

#[test]
fn diff_runs_for_every_drift_state() {
    let env = Env::new();
    env.sync();

    // Clean.
    diff::run(&env.paths, &env.config, "beta").unwrap();

    // Local only.
    write(
        &env.library_file("skills/beta/SKILL.md"),
        &skill_md("beta", "local edit"),
    );
    diff::run(&env.paths, &env.config, "beta").unwrap();

    // Diverged.
    env.commit_to_origin("skills/beta/SKILL.md", &skill_md("beta", "remote edit"));
    diff::run(&env.paths, &env.config, "beta").unwrap();
}

#[test]
fn diff_rejects_an_unknown_spec() {
    let env = Env::new();
    env.sync();

    assert!(diff::run(&env.paths, &env.config, "nope").is_err());
}

// --- revert ---

#[test]
fn revert_restores_the_last_synced_version() {
    let env = Env::new();
    env.sync();

    let beta = env.library_file("skills/beta/SKILL.md");
    write(&beta, &skill_md("beta", "unwanted edit"));

    revert::run(
        &env.paths,
        &env.config,
        "beta",
        false,
        true,
        &env.tool_dirs(),
    )
    .unwrap();

    assert!(std::fs::read_to_string(&beta).unwrap().contains("v1"));
}

/// A brand-new skill has no synced version, so reverting removes it entirely.
#[test]
fn revert_removes_a_spec_that_was_never_published() {
    let env = Env::new();
    env.sync();

    write(
        &env.library_file("skills/gamma/SKILL.md"),
        &skill_md("gamma", "draft"),
    );
    env.sync();

    revert::run(
        &env.paths,
        &env.config,
        "gamma",
        false,
        true,
        &env.tool_dirs(),
    )
    .unwrap();

    assert!(!env.library_file("skills/gamma").exists());
}

/// `--remote` goes past the last synced state to whatever the registry holds
/// now, discarding both the local edit and the stale baseline.
#[test]
fn revert_remote_takes_the_registrys_current_version() {
    let env = Env::new();
    env.sync();

    let beta = env.library_file("skills/beta/SKILL.md");
    write(&beta, &skill_md("beta", "my edit"));
    env.commit_to_origin("skills/beta/SKILL.md", &skill_md("beta", "their v2"));

    revert::run(
        &env.paths,
        &env.config,
        "beta",
        true,
        true,
        &env.tool_dirs(),
    )
    .unwrap();

    assert!(std::fs::read_to_string(&beta).unwrap().contains("their v2"));
}

/// Reverting one spec must not disturb work in progress on another.
#[test]
fn revert_only_touches_the_named_spec() {
    let env = Env::new();
    env.sync();

    write(
        &env.library_file("skills/alpha/SKILL.md"),
        &skill_md("alpha", "KEEP ME"),
    );
    write(
        &env.library_file("skills/beta/SKILL.md"),
        &skill_md("beta", "discard me"),
    );

    revert::run(
        &env.paths,
        &env.config,
        "beta",
        false,
        true,
        &env.tool_dirs(),
    )
    .unwrap();

    assert!(
        std::fs::read_to_string(env.library_file("skills/alpha/SKILL.md"))
            .unwrap()
            .contains("KEEP ME")
    );
}

/// Discarding work without a terminal to confirm on needs `--force`.
#[test]
fn revert_refuses_to_discard_work_unconfirmed() {
    let env = Env::new();
    env.sync();

    let err = revert::run(
        &env.paths,
        &env.config,
        "beta",
        false,
        false,
        &env.tool_dirs(),
    )
    .unwrap_err();

    assert!(matches!(
        err,
        akm::error::Error::ConfirmationRequired { .. }
    ));
}

// --- core ---

#[test]
fn core_show_lists_the_globally_mounted_specs() {
    let env = Env::new();
    env.sync();

    core::run(&env.paths, CoreAction::Show, &env.tool_dirs()).unwrap();

    let library = Library::load_from(&env.paths.library_json()).unwrap();
    assert_eq!(library.core_ids(), vec!["alpha"]);
}

/// `--adopt` throws this machine's opinion away and follows the registry.
#[test]
fn core_adopt_drops_local_overrides_and_relinks() {
    let env = Env::new();
    env.sync();

    let mut overrides = LocalOverrides::default();
    overrides.set_core("alpha", false, true);
    overrides.save_to(&env.paths.local_json()).unwrap();
    env.sync();
    assert!(!env.home.join(".claude/skills/alpha").exists());

    core::run(&env.paths, CoreAction::Adopt, &env.tool_dirs()).unwrap();

    assert_eq!(
        LocalOverrides::load_from(&env.paths.local_json())
            .unwrap()
            .deviation_count(),
        0
    );
    assert!(env.home.join(".claude/skills/alpha").is_symlink());
}

/// `--publish` promotes this machine's choices into the sidecars, which is
/// what makes them publishable — and leaves the spec showing as changed.
#[test]
fn core_publish_promotes_overrides_into_the_sidecars() {
    let env = Env::new();
    env.sync();

    let mut overrides = LocalOverrides::default();
    overrides.set_core("beta", true, false);
    overrides.save_to(&env.paths.local_json()).unwrap();
    env.sync();

    core::run(&env.paths, CoreAction::Publish, &env.tool_dirs()).unwrap();

    let sidecar = std::fs::read_to_string(env.library_file("skills/beta/akm.json")).unwrap();
    assert!(sidecar.contains("\"core\": true"));
    assert_eq!(
        LocalOverrides::load_from(&env.paths.local_json())
            .unwrap()
            .deviation_count(),
        0
    );

    let registry = Registry::new(env.config.registry_url().unwrap(), env.paths.library_dir());
    assert!(registry
        .drift()
        .unwrap()
        .state_of("beta")
        .has_local_changes());
}
