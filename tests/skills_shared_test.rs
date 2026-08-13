//! End-to-end shared registries: browse, import, contribute back.
//!
//! A shared registry is somebody else's repository, so these tests stand one up
//! as a real git remote and drive the whole loop against it. What they are
//! really pinning down is the boundary: importing copies files into the
//! personal library and nothing else, and sharing pushes a branch and nothing
//! else.

use akm::commands::skills::{import, list, share, shared};
use akm::config::Config;
use akm::library::libgen;
use akm::library::tool_dirs::ToolDirs;
use akm::library::Library;
use akm::paths::Paths;
use assert_cmd::cargo::cargo_bin_cmd;
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

/// Give the test process a git identity.
///
/// `share` commits inside a clone it creates and destroys itself, so unlike
/// every other checkout in these tests there is nothing for `git config` to be
/// run against. Without this the suite would quietly lean on the developer's
/// global git config and fail on a machine that has none — CI, for one.
fn identify_process() {
    for (key, value) in [
        ("GIT_AUTHOR_NAME", "Test"),
        ("GIT_AUTHOR_EMAIL", "test@example.com"),
        ("GIT_COMMITTER_NAME", "Test"),
        ("GIT_COMMITTER_EMAIL", "test@example.com"),
    ] {
        std::env::set_var(key, value);
    }
}

struct Env {
    _tmp: TempDir,
    /// Bare repository standing in for the shared registry's remote.
    origin: PathBuf,
    /// A checkout of it, standing in for whoever maintains that registry.
    upstream: PathBuf,
    paths: Paths,
    config: Config,
    home: PathBuf,
}

impl Env {
    /// A shared registry holding two usable skills and two that are not.
    fn new() -> Self {
        identify_process();
        let tmp = TempDir::new().unwrap();
        let origin = tmp.path().join("origin.git");
        git(tmp.path(), &["init", "--bare", "-b", "main", "origin.git"]);

        let upstream = tmp.path().join("upstream");
        git(
            tmp.path(),
            &["clone", "--quiet", &origin.to_string_lossy(), "upstream"],
        );
        git(&upstream, &["config", "user.email", "test@example.com"]);
        git(&upstream, &["config", "user.name", "Test"]);

        for id in ["house-style", "tdd"] {
            write(
                &upstream.join(format!("skills/{id}/SKILL.md")),
                &skill_md(id, "v1"),
            );
        }
        // The registry's owner considers this one core. That is a statement
        // about their machines, not about ours.
        write(
            &upstream.join("skills/house-style/akm.json"),
            r#"{"name":"house-style","description":"the house style","tags":["style"],"core":true,"triggers":{}}"#,
        );
        // Neither of these is importable.
        write(&upstream.join("skills/notes/README.md"), "just notes\n");
        write(
            &upstream.join("skills/half-written/SKILL.md"),
            "---\nname: half-written\n---\nno description\n",
        );

        git(&upstream, &["add", "-A"]);
        git(&upstream, &["commit", "-m", "initial"]);
        git(&upstream, &["push", "--quiet", "-u", "origin", "main"]);

        let home = tmp.path().join("home");
        let paths = Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            &home,
        );

        let mut config = Config::default();
        config
            .shared
            .insert("acme".to_string(), origin.to_string_lossy().to_string());

        Self {
            _tmp: tmp,
            origin,
            upstream,
            paths,
            config,
            home,
        }
    }

    fn tool_dirs(&self) -> ToolDirs {
        ToolDirs::builtin(&self.home)
    }

    fn import(&self, id: &str) {
        import::run_remote(
            &self.paths,
            &self.config,
            "acme",
            id,
            false,
            None,
            &self.tool_dirs(),
        )
        .unwrap();
    }

    fn import_all(&self) {
        import::run_all_remote(&self.paths, &self.config, "acme", false, &self.tool_dirs())
            .unwrap();
    }

    fn library(&self) -> Library {
        Library::load_from(&self.paths.library_json()).unwrap()
    }

    fn skill(&self, id: &str) -> PathBuf {
        self.paths.library_dir().join("skills").join(id)
    }

    /// Put a skill of our own in the personal library, ready to share.
    fn author(&self, id: &str, body: &str) {
        write(&self.skill(id).join("SKILL.md"), &skill_md(id, body));
        libgen::generate(&self.paths.library_dir(), &self.paths.library_json()).unwrap();
    }

    /// Publish a change to the shared registry, as its maintainer would.
    fn upstream_commit(&self, rel: &str, content: &str) {
        write(&self.upstream.join(rel), content);
        git(&self.upstream, &["add", "-A"]);
        git(&self.upstream, &["commit", "-m", &format!("update {rel}")]);
        git(&self.upstream, &["push", "--quiet"]);
    }
}

#[test]
fn browsing_clones_the_registry_and_reads_what_it_offers() {
    let env = Env::new();

    list::run_shared(&env.paths, &env.config, "acme").unwrap();

    let registry = shared::open(&env.paths, &env.config, "acme").unwrap();
    assert_eq!(registry.dir(), env.paths.shared_dir("acme"));
    assert!(registry.is_cloned());

    let catalogue = shared::catalogue(registry.dir()).unwrap();
    let ids: Vec<&str> = catalogue.skills.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, vec!["house-style", "tdd"]);

    // Unusable directories are reported rather than silently dropped.
    let skipped: Vec<&str> = catalogue.skipped.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(skipped, vec!["half-written", "notes"]);
}

/// The registry lives in the cache and is never mounted: no symlink, and
/// nothing outside the personal library points into it.
#[test]
fn a_shared_registry_is_never_mounted() {
    let env = Env::new();

    list::run_shared(&env.paths, &env.config, "acme").unwrap();

    assert!(env
        .paths
        .shared_dir("acme")
        .starts_with(env.paths.cache_dir()));
    assert!(!env.home.join(".claude/skills/house-style").exists());
    assert!(!env.paths.library_json().exists());
}

#[test]
fn importing_copies_the_skill_into_the_personal_library() {
    let env = Env::new();

    env.import("tdd");

    assert!(env.skill("tdd").join("SKILL.md").is_file());
    let library = env.library();
    let spec = library.get("tdd").expect("tdd is in the library");
    assert_eq!(spec.description, "desc for tdd");
    assert!(spec.source.as_deref().unwrap().contains("acme"));
}

/// `core: true` in a shared registry is its owner's statement about their own
/// machines. Importing must not turn it into a global mount here.
#[test]
fn core_is_not_inherited_from_a_shared_registry() {
    let env = Env::new();

    env.import("house-style");

    let library = env.library();
    assert!(!library.get("house-style").unwrap().core);
    assert!(!env.home.join(".claude/skills/house-style").exists());
}

#[test]
fn import_all_takes_every_valid_skill_and_leaves_the_rest() {
    let env = Env::new();

    env.import_all();

    let library = env.library();
    assert!(library.contains("tdd"));
    assert!(library.contains("house-style"));
    assert!(!library.contains("notes"));
    assert!(!library.contains("half-written"));
}

/// Re-running an import must be a no-op while nothing has moved, or `--all`
/// would ask about every skill in the registry every time.
///
/// Once the registry *has* moved, the update is a clash like any other: AKM
/// does not record which version was taken, so it cannot tell an untouched
/// import from a local rewrite. Without a terminal to ask, that means `--force`
/// — with one, the prompt offers mine/theirs/both.
#[test]
fn re_importing_is_idempotent_until_the_registry_moves() {
    let env = Env::new();
    env.import_all();

    env.import_all();
    assert_eq!(env.library().len(), 2);

    env.upstream_commit("skills/tdd/SKILL.md", &skill_md("tdd", "v2 from upstream"));

    let err = import::run_remote(
        &env.paths,
        &env.config,
        "acme",
        "tdd",
        false,
        None,
        &env.tool_dirs(),
    )
    .unwrap_err();
    assert!(format!("{err}").contains("--force"));

    import::run_remote(
        &env.paths,
        &env.config,
        "acme",
        "tdd",
        true,
        None,
        &env.tool_dirs(),
    )
    .unwrap();
    assert!(std::fs::read_to_string(env.skill("tdd").join("SKILL.md"))
        .unwrap()
        .contains("v2 from upstream"));
}

/// Without a terminal there is nobody to ask, so a clash leaves the local copy
/// alone. A batch says so and carries on; a single import fails outright, which
/// is what it has always done.
#[test]
fn a_clash_keeps_the_local_copy_when_nobody_can_be_asked() {
    let env = Env::new();
    env.author("tdd", "MY OWN VERSION");

    env.import_all();

    assert!(std::fs::read_to_string(env.skill("tdd").join("SKILL.md"))
        .unwrap()
        .contains("MY OWN VERSION"));
    // The skill that did not clash still came through.
    assert!(env.skill("house-style").join("SKILL.md").is_file());

    let err = import::run_remote(
        &env.paths,
        &env.config,
        "acme",
        "tdd",
        false,
        None,
        &env.tool_dirs(),
    )
    .unwrap_err();
    assert!(format!("{err}").contains("already exists"));
}

#[test]
fn force_takes_the_registrys_version() {
    let env = Env::new();
    env.author("tdd", "MY OWN VERSION");

    import::run_remote(
        &env.paths,
        &env.config,
        "acme",
        "tdd",
        true,
        None,
        &env.tool_dirs(),
    )
    .unwrap();

    assert!(std::fs::read_to_string(env.skill("tdd").join("SKILL.md"))
        .unwrap()
        .contains("v1"));
}

/// `--id` is how you keep both without being asked: the incoming copy lands
/// under a name of your choosing.
#[test]
fn an_import_can_be_stored_under_another_id() {
    let env = Env::new();
    env.author("tdd", "MY OWN VERSION");

    import::run_remote(
        &env.paths,
        &env.config,
        "acme",
        "tdd",
        false,
        Some("acme-tdd"),
        &env.tool_dirs(),
    )
    .unwrap();

    let library = env.library();
    assert!(library.contains("tdd"));
    assert!(library.contains("acme-tdd"));
    assert!(std::fs::read_to_string(env.skill("tdd").join("SKILL.md"))
        .unwrap()
        .contains("MY OWN VERSION"));
}

#[test]
fn sharing_pushes_a_branch_and_leaves_the_default_branch_alone() {
    let env = Env::new();
    env.author("my-skill", "something worth sharing");

    share::run(&env.paths, &env.config, "acme", "my-skill", false).unwrap();

    // The contribution is on its own branch...
    let on_branch = git(
        &env.origin,
        &["show", "akm/my-skill:skills/my-skill/SKILL.md"],
    );
    assert!(on_branch.contains("something worth sharing"));

    // ...and main is untouched, because merging is the registry owner's call.
    let main_files = git(&env.origin, &["ls-tree", "-r", "--name-only", "main"]);
    assert!(!main_files.contains("my-skill"));
}

/// Re-sharing has to fast-forward the branch an open pull request is built on,
/// not be rejected for having diverged from it.
#[test]
fn sharing_again_updates_the_existing_branch() {
    let env = Env::new();
    env.author("my-skill", "first draft");
    share::run(&env.paths, &env.config, "acme", "my-skill", false).unwrap();

    env.author("my-skill", "second draft");
    share::run(&env.paths, &env.config, "acme", "my-skill", false).unwrap();

    let on_branch = git(
        &env.origin,
        &["show", "akm/my-skill:skills/my-skill/SKILL.md"],
    );
    assert!(on_branch.contains("second draft"));
    assert_eq!(
        git(&env.origin, &["rev-list", "--count", "main..akm/my-skill"]),
        "2"
    );
}

#[test]
fn sharing_an_unchanged_spec_pushes_nothing() {
    let env = Env::new();
    env.author("my-skill", "one and only draft");
    share::run(&env.paths, &env.config, "acme", "my-skill", false).unwrap();

    let before = git(&env.origin, &["rev-parse", "akm/my-skill"]);
    share::run(&env.paths, &env.config, "acme", "my-skill", false).unwrap();
    let after = git(&env.origin, &["rev-parse", "akm/my-skill"]);

    assert_eq!(before, after);
}

#[test]
fn an_unconfigured_registry_names_the_ones_that_are() {
    let env = Env::new();

    let err = list::run_shared(&env.paths, &env.config, "typo").unwrap_err();
    let message = format!("{err}");
    assert!(message.contains("typo"));
    assert!(message.contains("acme"));
}

/// An unreachable registry must not cost you the copy you already have.
#[test]
fn browsing_falls_back_to_the_copy_on_disk_when_the_remote_is_gone() {
    let env = Env::new();
    list::run_shared(&env.paths, &env.config, "acme").unwrap();

    std::fs::remove_dir_all(&env.origin).unwrap();

    list::run_shared(&env.paths, &env.config, "acme").unwrap();
    let catalogue = shared::catalogue(&env.paths.shared_dir("acme")).unwrap();
    assert_eq!(catalogue.skills.len(), 2);
}

/// A failure is printed inside a one-line entry in `akm skills sync`'s report,
/// but git answers a failed fetch with a paragraph.
#[test]
fn an_unreachable_registry_reports_on_one_line() {
    let env = Env::new();
    let registry = shared::open(&env.paths, &env.config, "acme").unwrap();
    shared::refresh(&registry);

    std::fs::remove_dir_all(&env.origin).unwrap();

    let outcome = shared::refresh(&registry);
    let rendered = format!("{outcome}");
    assert_eq!(rendered.lines().count(), 1, "rendered as: {rendered}");
    assert!(rendered.starts_with("unreachable ("));
    // The entry it lands in already begins with the registry's name.
    assert!(!rendered.contains("Failed to sync registry"));
}

/// The first clone is the one case where an unreachable registry is fatal:
/// there is nothing on disk to fall back to.
#[test]
fn a_registry_that_was_never_cloned_is_an_error() {
    let env = Env::new();
    std::fs::remove_dir_all(&env.origin).unwrap();

    let err = list::run_shared(&env.paths, &env.config, "acme").unwrap_err();
    assert!(format!("{err}").contains("acme"));
}

/// Run `akm skills shared` against an isolated home. Captured stdout is not a
/// terminal, so this exercises the non-interactive read path an agent or a pipe
/// hits: it prints and exits, never waiting for a menu answer.
fn run_shared_cli(tmp: &TempDir, config_toml: Option<&str>) -> std::process::Output {
    let config_dir = tmp.path().join("config");
    if let Some(toml) = config_toml {
        std::fs::create_dir_all(config_dir.join("akm")).unwrap();
        std::fs::write(config_dir.join("akm/config.toml"), toml).unwrap();
    }
    cargo_bin_cmd!("akm")
        .args(["skills", "shared"])
        .env("XDG_CONFIG_HOME", &config_dir)
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .env("HOME", tmp.path())
        .output()
        .unwrap()
}

#[test]
fn piped_shared_prints_the_list_and_exits() {
    let tmp = TempDir::new().unwrap();
    let output = run_shared_cli(
        &tmp,
        Some("[shared]\nacme = \"git@github.com:acme/skills.git\"\n"),
    );

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Shared registries:"));
    assert!(stdout.contains("acme"));
    assert!(stdout.contains("git@github.com:acme/skills.git"));
}

#[test]
fn piped_shared_with_no_registries_prints_the_hint() {
    let tmp = TempDir::new().unwrap();
    let output = run_shared_cli(&tmp, None);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No shared registries configured."));
    assert!(stdout.contains("akm config shared.<name> <url>"));
}
