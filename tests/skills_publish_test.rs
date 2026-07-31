//! End-to-end `akm skills publish` against a real personal registry.
//!
//! Publishing is per-spec: it must send exactly the spec named and nothing
//! else, and it must be able to land on top of a remote that moved on.

use akm::commands::skills::{publish, sync};
use akm::config::Config;
use akm::library::tool_dirs::ToolDirs;
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
        for id in ["alpha", "beta"] {
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
        sync::execute(&self.paths, &registry, &ToolDirs::builtin(&self.home)).unwrap();
        identify(&self.paths.library_dir());
    }

    fn library_file(&self, rel: &str) -> PathBuf {
        self.paths.library_dir().join(rel)
    }

    fn publish(&self, id: &str) -> akm::error::Result<()> {
        publish::run(&self.paths, &self.config, id, false)
    }

    /// What the registry holds for `rel`, as seen from the other checkout.
    fn remote_content(&self, rel: &str) -> String {
        git(&self.other, &["pull", "--quiet", "--ff-only"]);
        std::fs::read_to_string(self.other.join(rel)).unwrap()
    }

    /// Commit and push a change from the other machine.
    fn push_from_other(&self, rel: &str, content: &str) {
        git(&self.other, &["pull", "--quiet", "--ff-only"]);
        write(&self.other.join(rel), content);
        git(&self.other, &["add", "-A"]);
        git(&self.other, &["commit", "-m", "from other machine"]);
        git(&self.other, &["push", "--quiet"]);
    }
}

#[test]
fn publishing_sends_the_local_version_to_the_registry() {
    let env = Env::new();
    env.sync();

    write(
        &env.library_file("skills/alpha/SKILL.md"),
        &skill_md("alpha", "edited locally"),
    );

    env.publish("alpha").unwrap();

    assert!(env
        .remote_content("skills/alpha/SKILL.md")
        .contains("edited locally"));
}

/// Publishing one spec must not drag a half-finished neighbour along with it.
#[test]
fn publishing_one_spec_leaves_other_local_work_alone() {
    let env = Env::new();
    env.sync();

    write(
        &env.library_file("skills/alpha/SKILL.md"),
        &skill_md("alpha", "ready"),
    );
    write(
        &env.library_file("skills/beta/SKILL.md"),
        &skill_md("beta", "STILL A DRAFT"),
    );

    env.publish("alpha").unwrap();

    assert!(env
        .remote_content("skills/alpha/SKILL.md")
        .contains("ready"));
    assert!(env.remote_content("skills/beta/SKILL.md").contains("v1"));

    // The draft is untouched on disk, and still marked as local-only.
    assert!(
        std::fs::read_to_string(env.library_file("skills/beta/SKILL.md"))
            .unwrap()
            .contains("STILL A DRAFT")
    );
    let registry = Registry::new(env.config.registry_url().unwrap(), env.paths.library_dir());
    assert!(registry
        .drift()
        .unwrap()
        .state_of("beta")
        .has_local_changes());
}

/// Another machine published the same skill in the meantime. Publishing rebases
/// ours on top of theirs rather than merging — no conflict markers can reach a
/// file that is symlinked live into a tool directory.
#[test]
fn publishing_over_a_diverged_remote_keeps_the_local_version() {
    let env = Env::new();
    env.sync();

    write(
        &env.library_file("skills/alpha/SKILL.md"),
        &skill_md("alpha", "MY VERSION"),
    );
    env.push_from_other("skills/alpha/SKILL.md", &skill_md("alpha", "their version"));

    env.publish("alpha").unwrap();

    let published = env.remote_content("skills/alpha/SKILL.md");
    assert!(published.contains("MY VERSION"), "got: {published}");
    assert!(!published.contains("<<<<<<<"));
    assert!(
        std::fs::read_to_string(env.library_file("skills/alpha/SKILL.md"))
            .unwrap()
            .contains("MY VERSION")
    );
}

/// A diverged publish must not silently drop the *other* changes the remote
/// brought in — only the published spec is overridden.
#[test]
fn publishing_over_a_diverged_remote_keeps_their_other_changes() {
    let env = Env::new();
    env.sync();

    write(
        &env.library_file("skills/alpha/SKILL.md"),
        &skill_md("alpha", "MY VERSION"),
    );
    env.push_from_other("skills/alpha/SKILL.md", &skill_md("alpha", "their alpha"));
    env.push_from_other("skills/beta/SKILL.md", &skill_md("beta", "their beta"));

    env.publish("alpha").unwrap();

    assert!(env
        .remote_content("skills/beta/SKILL.md")
        .contains("their beta"));
}

/// A brand-new skill that exists only on this machine is published whole,
/// including its metadata sidecar.
#[test]
fn publishing_a_new_skill_sends_all_of_its_files() {
    let env = Env::new();
    env.sync();

    write(
        &env.library_file("skills/gamma/SKILL.md"),
        &skill_md("gamma", "brand new"),
    );
    write(
        &env.library_file("skills/gamma/akm.json"),
        r#"{"name":"gamma","description":"human prose","tags":["new"],"core":false,"triggers":{}}"#,
    );
    env.sync(); // pick the new skill up into library.json

    env.publish("gamma").unwrap();

    assert!(env
        .remote_content("skills/gamma/SKILL.md")
        .contains("brand new"));
    assert!(env
        .remote_content("skills/gamma/akm.json")
        .contains("human prose"));
}

#[test]
fn publishing_an_unchanged_spec_does_nothing() {
    let env = Env::new();
    env.sync();

    env.publish("alpha").unwrap();

    // No commit was created, so the remote is still at the seeding commit.
    git(&env.other, &["fetch", "--quiet"]);
    let count = git(&env.other, &["rev-list", "--count", "origin/main"]);
    assert_eq!(count, "1");
}
