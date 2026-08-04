//! End-to-end `akm skills core --publish` against a real personal registry.
//!
//! Promoting this machine's core choices is one intent, so it must be one
//! commit and one push however many specs it names — and it must carry only
//! the sidecars, never a `SKILL.md` that happens to be under edit.

use akm::commands::skills::{core, sync};
use akm::config::Config;
use akm::library::tool_dirs::ToolDirs;
use akm::paths::Paths;
use akm::registry::Registry;
use std::os::unix::fs::PermissionsExt;
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

fn identify(dir: &Path) {
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test"]);
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
    /// The bare repository standing in for the remote.
    origin: PathBuf,
    /// A second checkout, standing in for another machine.
    other: PathBuf,
    paths: Paths,
    config: Config,
    home: PathBuf,
}

impl Env {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let origin = tmp.path().join("origin.git");
        git(tmp.path(), &["init", "--bare", "-b", "main", "origin.git"]);

        // Seed the registry through a throwaway clone.
        let seed = tmp.path().join("seed");
        git(
            tmp.path(),
            &["clone", "--quiet", &origin.to_string_lossy(), "seed"],
        );
        identify(&seed);
        for id in ["alpha", "beta", "gamma"] {
            write(
                &seed.join(format!("skills/{id}/SKILL.md")),
                &skill_md(id, "v1"),
            );
        }
        git(&seed, &["add", "-A"]);
        git(&seed, &["commit", "-m", "initial"]);
        git(&seed, &["push", "--quiet", "-u", "origin", "main"]);

        let other = tmp.path().join("other");
        git(
            tmp.path(),
            &["clone", "--quiet", &origin.to_string_lossy(), "other"],
        );
        identify(&other);

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
            other,
            paths,
            config,
            home,
        }
    }

    fn sync(&self) {
        let registry = Registry::new(
            self.config.registry_url().unwrap(),
            self.paths.library_dir(),
        );
        sync::execute(
            &self.paths,
            &registry,
            &ToolDirs::builtin(&self.home),
            Vec::new(),
        )
        .unwrap();
        identify(&self.paths.library_dir());
    }

    fn library_file(&self, rel: &str) -> PathBuf {
        self.paths.library_dir().join(rel)
    }

    /// Make pushes to the remote fail while fetches keep working.
    fn break_push(&self) {
        let hook = self.origin.join("hooks").join("pre-receive");
        write(&hook, "#!/bin/sh\nexit 1\n");
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn restore_push(&self) {
        std::fs::remove_file(self.origin.join("hooks").join("pre-receive")).unwrap();
    }

    /// What the registry holds for `rel`, as seen from the other checkout.
    fn remote_content(&self, rel: &str) -> String {
        git(&self.other, &["pull", "--quiet", "--ff-only"]);
        std::fs::read_to_string(self.other.join(rel)).unwrap()
    }

    fn commit_count(&self) -> u32 {
        git(&self.paths.library_dir(), &["rev-list", "--count", "HEAD"])
            .parse()
            .unwrap()
    }

    /// Switch `ids` on as this machine's deviation, the way the TUI does.
    ///
    /// The sync is not incidental: `library.json` carries the *effective* core
    /// value, and it is only folded in when the overrides are applied. Writing
    /// `local.json` alone would leave the promotion reading the old default.
    fn set_local_core(&self, ids: &[&str]) {
        let mut map = serde_json::Map::new();
        for id in ids {
            map.insert((*id).to_string(), serde_json::Value::Bool(true));
        }
        let json = serde_json::json!({ "core": map });
        write(
            &self.paths.local_json(),
            &serde_json::to_string_pretty(&json).unwrap(),
        );
        self.sync();
    }

    fn core_publish(&self) -> akm::error::Result<()> {
        core::run(
            &self.paths,
            &self.config,
            core::CoreAction::Publish,
            &ToolDirs::builtin(&self.home),
            false,
        )
    }
}

#[test]
fn publishing_core_defaults_makes_exactly_one_commit() {
    let env = Env::new();
    env.sync();

    // An unrelated content edit that must NOT be swept into the metadata commit.
    write(
        &env.library_file("skills/alpha/SKILL.md"),
        &skill_md("alpha", "work in progress"),
    );

    env.set_local_core(&["alpha", "beta", "gamma"]);
    let before = env.commit_count();

    env.core_publish().unwrap();

    assert_eq!(env.commit_count(), before + 1, "one intent, one commit");

    // All three sidecars reached the registry with core: true.
    for id in ["alpha", "beta", "gamma"] {
        let sidecar = env.remote_content(&format!("skills/{id}/akm.json"));
        assert!(
            sidecar.contains("\"core\": true"),
            "{id} sidecar: {sidecar}"
        );
    }

    // The in-flight SKILL.md edit stayed home.
    assert!(env.remote_content("skills/alpha/SKILL.md").contains("v1"));
}

#[test]
fn a_dry_run_publishes_nothing() {
    let env = Env::new();
    env.sync();
    env.set_local_core(&["alpha", "beta"]);
    let before = env.commit_count();

    core::run(
        &env.paths,
        &env.config,
        core::CoreAction::Publish,
        &ToolDirs::builtin(&env.home),
        true,
    )
    .unwrap();

    assert_eq!(env.commit_count(), before, "dry run must not commit");
    let local = std::fs::read_to_string(env.paths.local_json()).unwrap();
    assert!(local.contains("alpha"), "dry run must not clear deviations");
}

#[test]
fn a_failed_push_keeps_the_deviations_for_a_retry() {
    let env = Env::new();
    env.sync();
    env.set_local_core(&["alpha", "beta"]);

    env.break_push();
    assert!(env.core_publish().is_err());

    // local.json must still hold the deviations, or the retry has nothing to
    // promote and the remote never receives them.
    let local = std::fs::read_to_string(env.paths.local_json()).unwrap();
    assert!(
        local.contains("alpha"),
        "deviations cleared too early: {local}"
    );

    env.restore_push();
    env.core_publish().unwrap();
    assert!(env
        .remote_content("skills/alpha/akm.json")
        .contains("\"core\": true"));
}
