//! `akm skills shared` — interactively manage shared registries.
//!
//! Shared registries are named git URLs (`[shared]` in config) that you browse
//! and import from. The scriptable way to manage them is `akm config
//! shared.<name> <url>`; this command is the human-facing front-end over the
//! same config, so both an agent and a person can drive AKM.
//!
//! On a terminal it loops a small add/remove menu. Piped, redirected, or with
//! `--plain`, it prints the list and exits — the read path for agents and the
//! "just show me what's configured" path for people.
//!
//! Writes still go through `config.save`; nothing here is a second source of
//! truth. Removal also drops the cached checkout, matching the sweep that
//! `akm skills sync` does for the scriptable removal path.

use crate::config::{is_valid_shared_name, Config};
use crate::error::{Error, Result};
use crate::paths::Paths;
use crate::registry::Registry;
use std::io::{self, BufRead, Write};

use super::shared;

/// Interactive prompts, abstracted so the menu is testable without a terminal.
///
/// Local to this command, and small on purpose: it mirrors [`super::super::setup`]'s
/// `Prompter` rather than sharing it, so this front-end owns its own prompt
/// styling without touching the setup wizard.
///
/// The empty line is load-bearing. Every prompt returns its `default` on both
/// an empty line and EOF, and the menu chooses defaults that always terminate:
/// `q` at the top level (so Enter and Ctrl-D both quit), and an empty string in
/// the sub-flows (so Enter and Ctrl-D back out). That is what keeps the loop
/// from spinning forever when stdin is exhausted.
trait Prompter {
    /// Ask a yes/no question. Empty line or EOF yields `default_yes`.
    fn confirm(&mut self, message: &str, default_yes: bool) -> Result<bool>;

    /// Print `message`, read one trimmed line. Empty line or EOF yields `default`.
    fn input(&mut self, message: &str, default: &str) -> Result<String>;
}

/// The real prompter: reads from stdin.
struct StdinPrompter;

impl Prompter for StdinPrompter {
    fn confirm(&mut self, message: &str, default_yes: bool) -> Result<bool> {
        let suffix = if default_yes { "[Y/n]" } else { "[y/N]" };
        print!("{message} {suffix} ");
        flush()?;
        let line = read_line()?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            Ok(default_yes)
        } else {
            Ok(trimmed.eq_ignore_ascii_case("y") || trimmed.eq_ignore_ascii_case("yes"))
        }
    }

    fn input(&mut self, message: &str, default: &str) -> Result<String> {
        print!("{message} ");
        flush()?;
        let line = read_line()?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            Ok(default.to_string())
        } else {
            Ok(trimmed.to_string())
        }
    }
}

fn flush() -> Result<()> {
    io::stdout().flush().map_err(|e| Error::Io {
        context: "flushing stdout".into(),
        source: e,
    })
}

fn read_line() -> Result<String> {
    let mut input = String::new();
    io::stdin()
        .lock()
        .read_line(&mut input)
        .map_err(|e| Error::Io {
            context: "reading input".into(),
            source: e,
        })?;
    Ok(input)
}

/// Run `akm skills shared`.
///
/// On an interactive terminal (and not `--plain`) this drives the menu;
/// otherwise it prints the list and returns, so a pipe or an agent never hangs.
pub fn run(paths: &Paths, plain: bool) -> Result<()> {
    let mut config = Config::load(paths)?;

    if !super::list::should_use_tui(plain) {
        print_list(&config);
        return Ok(());
    }

    let mut prompter = StdinPrompter;
    run_menu(&mut prompter, paths, &mut config)
}

/// Print the configured registries, or the empty-state hint.
///
/// Shared by the non-interactive list path and the menu's reprint between
/// actions. Reads the in-memory `Config` the loop keeps mutating, never disk.
fn print_list(config: &Config) {
    if config.shared.is_empty() {
        println!("No shared registries configured.");
        println!("Add one here, or with 'akm config shared.<name> <url>'.");
        return;
    }

    println!("Shared registries:");
    let width = config.shared.keys().map(String::len).max().unwrap_or(0);
    for (name, url) in &config.shared {
        println!("  {name:<width$}  {url}");
    }
}

/// The menu loop: reprint the list, offer add/remove/quit, act, repeat.
fn run_menu(prompter: &mut impl Prompter, paths: &Paths, config: &mut Config) -> Result<()> {
    loop {
        println!();
        print_list(config);
        println!();

        let prompt = if config.shared.is_empty() {
            "[a]dd / [q]uit >"
        } else {
            "[a]dd / [r]emove / [q]uit >"
        };
        // `q` default: Enter and EOF both quit.
        let choice = prompter.input(prompt, "q")?;

        match choice.to_ascii_lowercase().as_str() {
            "a" | "add" => add_flow(prompter, paths, config)?,
            "r" | "remove" if !config.shared.is_empty() => remove_flow(prompter, paths, config)?,
            "q" | "quit" => return Ok(()),
            _ => {} // anything else: reprint and ask again
        }
    }
}

/// Add a registry: name (validated, unique), URL (opaque), optional reachability check.
fn add_flow(prompter: &mut impl Prompter, paths: &Paths, config: &mut Config) -> Result<()> {
    let name = loop {
        // Empty (Enter or EOF) backs out; a bad or taken name re-asks.
        let name = prompter.input("Name:", "")?;
        if name.is_empty() {
            return Ok(());
        }
        if !is_valid_shared_name(&name) {
            println!("Name must be a single segment with no '.' — try again.");
            continue;
        }
        if config.shared.contains_key(&name) {
            println!("'{name}' is already configured — pick another name.");
            continue;
        }
        break name;
    };

    // The URL is opaque: it is handed to `git clone` later, so anything git can
    // clone works (HTTPS, SSH, a local path). No host-sniffing, no validation.
    let url = prompter.input("URL:", "")?;
    if url.is_empty() {
        return Ok(());
    }

    config.shared.insert(name.clone(), url.clone());
    config.save(paths)?;
    println!("Added '{name}'.");

    if prompter.confirm("Verify it's reachable now?", false)? {
        let registry = Registry::named(name.as_str(), url.as_str(), paths.shared_dir(&name));
        println!("  {name}: {}", shared::refresh(&registry));
    }

    Ok(())
}

/// Remove a registry chosen by number, and delete its cached checkout.
fn remove_flow(prompter: &mut impl Prompter, paths: &Paths, config: &mut Config) -> Result<()> {
    // BTreeMap keys are sorted, so the numbering is stable for the whole prompt.
    let names: Vec<String> = config.shared.keys().cloned().collect();

    println!("Remove which?");
    for (i, name) in names.iter().enumerate() {
        println!("  {}) {name}", i + 1);
    }

    let index = loop {
        // Empty (Enter or EOF) backs out to the menu.
        let line = prompter.input("Number:", "")?;
        if line.is_empty() {
            return Ok(());
        }
        match line.parse::<usize>() {
            Ok(n) if (1..=names.len()).contains(&n) => break n - 1,
            _ => println!("Enter a number between 1 and {}.", names.len()),
        }
    };

    let name = &names[index];
    config.shared.remove(name);
    config.save(paths)?;
    // Best-effort: the checkout is reconstructible, and it may never have been
    // browsed, so an absent dir or a failed delete is a no-op.
    std::fs::remove_dir_all(paths.shared_dir(name)).ok();
    println!("Removed '{name}'.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::path::Path;
    use std::process::Command;

    /// A prompter that replays canned answers, then falls back to the default —
    /// which is how EOF (an exhausted queue) is modelled.
    struct TestPrompter {
        responses: VecDeque<String>,
    }

    impl TestPrompter {
        fn new(responses: &[&str]) -> Self {
            Self {
                responses: responses.iter().map(|s| s.to_string()).collect(),
            }
        }
    }

    impl Prompter for TestPrompter {
        fn confirm(&mut self, _message: &str, default_yes: bool) -> Result<bool> {
            match self.responses.pop_front() {
                Some(r) if r.is_empty() => Ok(default_yes),
                Some(r) => Ok(r.eq_ignore_ascii_case("y") || r.eq_ignore_ascii_case("yes")),
                None => Ok(default_yes),
            }
        }

        fn input(&mut self, _message: &str, default: &str) -> Result<String> {
            match self.responses.pop_front() {
                Some(r) if r.is_empty() => Ok(default.to_string()),
                Some(r) => Ok(r),
                None => Ok(default.to_string()),
            }
        }
    }

    fn temp_paths() -> (Paths, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            &tmp.path().join("home"),
        );
        (paths, tmp)
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn add_writes_the_entry_and_persists_it() {
        let (paths, _tmp) = temp_paths();
        let mut config = Config::default();
        // name, url, verify? -> no
        let mut p = TestPrompter::new(&["acme", "git@example.com:acme.git", "n"]);

        add_flow(&mut p, &paths, &mut config).unwrap();

        assert_eq!(
            config.shared.get("acme").map(String::as_str),
            Some("git@example.com:acme.git")
        );
        // Written through to disk, not just held in memory.
        let reloaded = Config::load(&paths).unwrap();
        assert_eq!(
            reloaded.shared.get("acme").map(String::as_str),
            Some("git@example.com:acme.git")
        );
        // "n" means no clone was attempted.
        assert!(!paths.shared_dir("acme").exists());
    }

    #[test]
    fn add_rejects_a_taken_name_then_takes_the_next() {
        let (paths, _tmp) = temp_paths();
        let mut config = Config::default();
        config
            .shared
            .insert("acme".into(), "git@example.com:acme.git".into());
        // "acme" collides -> re-ask -> "other"
        let mut p = TestPrompter::new(&["acme", "other", "git@example.com:other.git", "n"]);

        add_flow(&mut p, &paths, &mut config).unwrap();

        assert!(config.shared.contains_key("other"));
        // The existing entry is untouched.
        assert_eq!(
            config.shared.get("acme").map(String::as_str),
            Some("git@example.com:acme.git")
        );
    }

    #[test]
    fn add_reprompts_on_an_invalid_name() {
        let (paths, _tmp) = temp_paths();
        let mut config = Config::default();
        // "a.b" has a dot -> re-ask -> "ok"
        let mut p = TestPrompter::new(&["a.b", "ok", "git@example.com:ok.git", "n"]);

        add_flow(&mut p, &paths, &mut config).unwrap();

        assert!(config.shared.contains_key("ok"));
        assert!(!config.shared.contains_key("a.b"));
    }

    #[test]
    fn add_cancels_on_an_empty_name() {
        let (paths, _tmp) = temp_paths();
        let mut config = Config::default();
        // No responses: the name prompt sees EOF and backs out.
        let mut p = TestPrompter::new(&[]);

        add_flow(&mut p, &paths, &mut config).unwrap();

        assert!(config.shared.is_empty());
    }

    #[test]
    fn verify_no_does_not_clone() {
        let (paths, _tmp) = temp_paths();
        let mut config = Config::default();
        let mut p = TestPrompter::new(&["acme", "git@example.com:acme.git", "n"]);

        add_flow(&mut p, &paths, &mut config).unwrap();

        assert!(!paths.shared_dir("acme").exists());
    }

    #[test]
    fn verify_yes_clones_the_new_registry() {
        let (paths, tmp) = temp_paths();
        // A local bare repo stands in for the remote — no network in the test.
        let origin = tmp.path().join("origin.git");
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "--bare", "-q"]);

        let mut config = Config::default();
        let mut p = TestPrompter::new(&["acme", &origin.to_string_lossy(), "y"]);

        add_flow(&mut p, &paths, &mut config).unwrap();

        assert!(config.shared.contains_key("acme"));
        assert!(paths.shared_dir("acme").exists());
    }

    #[test]
    fn remove_drops_the_chosen_entry_and_its_cache() {
        let (paths, _tmp) = temp_paths();
        let mut config = Config::default();
        config
            .shared
            .insert("acme".into(), "git@example.com:acme.git".into());
        config
            .shared
            .insert("beta".into(), "git@example.com:beta.git".into());
        // Pretend "acme" was browsed once, leaving a checkout behind.
        std::fs::create_dir_all(paths.shared_dir("acme")).unwrap();

        // Sorted order is [acme, beta]; "1" removes acme.
        let mut p = TestPrompter::new(&["1"]);
        remove_flow(&mut p, &paths, &mut config).unwrap();

        assert!(!config.shared.contains_key("acme"));
        assert!(config.shared.contains_key("beta"));
        assert!(!paths.shared_dir("acme").exists());
    }

    #[test]
    fn remove_reprompts_on_a_bad_number_then_backs_out() {
        let (paths, _tmp) = temp_paths();
        let mut config = Config::default();
        config
            .shared
            .insert("acme".into(), "git@example.com:acme.git".into());
        // out-of-range, then non-numeric, then EOF backs out.
        let mut p = TestPrompter::new(&["9", "nope"]);

        remove_flow(&mut p, &paths, &mut config).unwrap();

        // Nothing removed.
        assert!(config.shared.contains_key("acme"));
    }

    #[test]
    fn menu_quits_on_q() {
        let (paths, _tmp) = temp_paths();
        let mut config = Config::default();
        config
            .shared
            .insert("acme".into(), "git@example.com:acme.git".into());
        let mut p = TestPrompter::new(&["q"]);

        run_menu(&mut p, &paths, &mut config).unwrap();
    }

    #[test]
    fn menu_quits_on_eof() {
        let (paths, _tmp) = temp_paths();
        let mut config = Config::default();
        // Empty queue -> the top-level prompt sees EOF -> quit.
        let mut p = TestPrompter::new(&[]);

        run_menu(&mut p, &paths, &mut config).unwrap();
    }

    #[test]
    fn menu_adds_then_quits() {
        let (paths, _tmp) = temp_paths();
        let mut config = Config::default();
        // top: add; name; url; verify no; top: quit
        let mut p = TestPrompter::new(&["a", "acme", "git@example.com:acme.git", "n", "q"]);

        run_menu(&mut p, &paths, &mut config).unwrap();

        assert!(config.shared.contains_key("acme"));
    }
}
