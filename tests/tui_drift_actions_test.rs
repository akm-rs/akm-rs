//! The list TUI's in-place drift verbs: publish (`p`) queues a drifted spec for
//! the exit-time publish offer, and revert (`u`) discards local edits back to
//! the synced version eagerly. Both are driven here through the `App` methods
//! the key handlers call, against a real registry checkout so drift is genuine.

use std::path::{Path, PathBuf};
use std::process::Command;

use akm::commands::skills::sync;
use akm::config::Config;
use akm::library::drift::DriftState;
use akm::library::tool_dirs::ToolDirs;
use akm::paths::Paths;
use akm::registry::Registry;
use akm::tui::app::{App, RevertOutcome};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn skill_md(id: &str, body: &str) -> String {
    format!("---\nname: {id}\ndescription: a test skill\n---\n{body}\n")
}

/// An origin repo plus a synced library checkout, ready to drift.
struct Env {
    _tmp: TempDir,
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
        git(
            &origin,
            &["config", "receive.denyCurrentBranch", "updateInstead"],
        );
        write(
            &origin.join("skills/beta/SKILL.md"),
            &skill_md("beta", "v1"),
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

        let env = Self {
            _tmp: tmp,
            paths,
            config,
            home,
        };
        env.sync();
        env
    }

    fn tool_dirs(&self) -> ToolDirs {
        ToolDirs::builtin(&self.home)
    }

    fn sync(&self) {
        let registry = Registry::new(
            self.config.registry_url().unwrap(),
            self.paths.library_dir(),
        );
        sync::execute(&self.paths, &registry, &self.tool_dirs(), Vec::new()).unwrap();
        let library_dir = self.paths.library_dir();
        git(&library_dir, &["config", "user.email", "test@example.com"]);
        git(&library_dir, &["config", "user.name", "Test"]);
    }

    fn library_file(&self, rel: &str) -> PathBuf {
        self.paths.library_dir().join(rel)
    }

    /// Build an `App` over the current on-disk state (drift is read at start).
    fn app(&self) -> App {
        App::new(self.paths.clone(), self.tool_dirs()).unwrap()
    }
}

#[test]
fn revert_discards_a_local_edit_and_clears_the_marker() {
    let env = Env::new();
    let beta = env.library_file("skills/beta/SKILL.md");
    write(&beta, &skill_md("beta", "unwanted edit"));

    let mut app = env.app();
    assert_eq!(app.drift.state_of("beta"), DriftState::LocalNewer);

    assert_eq!(app.revert_spec("beta").unwrap(), RevertOutcome::Reverted);

    // Content is back to the synced version and the marker has cleared, both
    // without leaving the TUI.
    assert_eq!(
        std::fs::read_to_string(&beta).unwrap(),
        skill_md("beta", "v1")
    );
    assert_eq!(app.drift.state_of("beta"), DriftState::Clean);
}

#[test]
fn revert_is_a_no_op_on_a_clean_spec() {
    let env = Env::new();
    let mut app = env.app();
    assert_eq!(
        app.revert_spec("beta").unwrap(),
        RevertOutcome::NothingToRevert
    );
}

#[test]
fn publish_queues_a_drifted_spec_for_the_exit_offer() {
    let env = Env::new();
    write(
        &env.library_file("skills/beta/SKILL.md"),
        &skill_md("beta", "local edit"),
    );

    let mut app = env.app();
    assert_eq!(app.queue_publish("beta"), Some(DriftState::LocalNewer));

    assert_eq!(app.pending_publish.len(), 1);
    assert!(app.pending_publish[0]
        .pathspecs
        .iter()
        .any(|p| p.contains("skills/beta")));
}

#[test]
fn publish_queues_nothing_for_a_clean_spec() {
    let env = Env::new();
    let mut app = env.app();
    assert_eq!(app.queue_publish("beta"), None);
    assert!(app.pending_publish.is_empty());
}
