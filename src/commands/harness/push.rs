//! `akm harness push` — capture live harness config into the personal registry.

use crate::commands::harness::{def_for, live_dir, pathspec, targets, tree_dir};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::library::harness_config::{capture_files, classify_dir, looks_like_secret, Pattern};
use crate::paths::Paths;
use crate::registry::{PublishOutcome, Registry};
use std::io::{self, BufRead, IsTerminal, Write};

/// Run `akm harness push [<command>]`.
pub fn run(paths: &Paths, config: &Config, command: Option<&str>) -> Result<()> {
    let url = config.registry_url().ok_or(Error::NoPersonalRegistry)?;
    let registry = Registry::new(url, paths.library_dir());
    if !registry.is_cloned() {
        return Err(Error::RegistrySync {
            name: "personal".into(),
            message: "The library is not a registry checkout. Run 'akm skills sync' first.".into(),
        });
    }

    // Config may gain opt-ins during the run; keep a mutable working copy.
    let mut config = config.clone();

    for cmd in targets(command)? {
        push_one(paths, &mut config, &registry, url, &cmd)?;
    }
    Ok(())
}

fn push_one(
    paths: &Paths,
    config: &mut Config,
    registry: &Registry,
    url: &str,
    cmd: &str,
) -> Result<()> {
    let def = def_for(cmd)?;
    let Some(live) = live_dir(paths, cmd) else {
        return Ok(());
    };
    if !live.is_dir() {
        println!(
            "{cmd}: no config directory at {} — nothing to push",
            live.display()
        );
        return Ok(());
    }

    let extra: Vec<Pattern> = config
        .harness_allow(cmd)
        .iter()
        .map(|s| Pattern::parse(s))
        .collect();
    let scan = classify_dir(&live, &def, &extra)?;

    // Interactive opt-in for unrecognized files (TTY only).
    let mut include = scan.allowed.clone();
    if io::stdin().is_terminal() && !scan.unrecognized.is_empty() {
        println!("{cmd}: unrecognized files in {}", live.display());
        for rel in &scan.unrecognized {
            print!("  add '{rel}' to sync? [y/N]: ");
            io::stdout().flush().ok();
            let mut input = String::new();
            io::stdin().lock().read_line(&mut input).ok();
            if input.trim().eq_ignore_ascii_case("y") {
                config
                    .harness
                    .entry(cmd.to_string())
                    .or_default()
                    .allow
                    .push(rel.clone());
                include.push(rel.clone());
            }
        }
        config.save(paths)?;
    } else if !scan.unrecognized.is_empty() {
        println!(
            "{cmd}: {} unrecognized file(s) not synced (run interactively to opt in)",
            scan.unrecognized.len()
        );
    }
    include.sort();
    include.dedup();

    // Secret-scan the files about to be committed (warn + skip). The hard
    // exclude list is the real guard; this is a backstop for opted-in files.
    let include: Vec<String> = include
        .into_iter()
        .filter(|rel| match std::fs::read_to_string(live.join(rel)) {
            Ok(content) if looks_like_secret(&content) => {
                eprintln!("Warning: '{cmd}/{rel}' looks like it may contain a secret — skipping.");
                false
            }
            _ => true,
        })
        .collect();

    let tree = tree_dir(paths, cmd);
    let captured = capture_files(&live, &tree, &include)?;
    if captured.is_empty() {
        println!("{cmd}: nothing to sync");
        return Ok(());
    }

    registry.refresh()?;
    match registry.publish_worktree(
        &[pathspec(cmd)],
        &format!("chore(harness): sync {cmd} config"),
    )? {
        PublishOutcome::NothingToDo => println!("{cmd}: already up to date with the registry"),
        PublishOutcome::Published => {
            println!("{cmd}: pushed {} file(s) to {url}", captured.len());
        }
    }
    Ok(())
}
