//! `akm harness push` — capture live harness config into the personal registry.

use crate::commands::harness::{def_for, live_dir, pathspec, targets, tree_dir};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::library::drift::DriftState;
use crate::library::harness_config::{
    capture_files, classify_dir, looks_like_secret, plan_capture, CapturePlan, DirScan, Pattern,
};
use crate::paths::Paths;
use crate::registry::{PublishOutcome, Registry};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;

/// Run `akm harness push [<command>] [--dry-run] [--verbose]`.
pub fn run(
    paths: &Paths,
    config: &Config,
    command: Option<&str>,
    dry_run: bool,
    verbose: bool,
) -> Result<()> {
    let url = config.registry_url().ok_or(Error::NoPersonalRegistry)?;
    let registry = Registry::new(url, paths.library_dir());
    if !registry.is_cloned() {
        return Err(Error::RegistrySync {
            name: "personal".into(),
            message: "The library is not a registry checkout. Run 'akm skills sync' first.".into(),
        });
    }

    // Drift feeds only the banner. A dry run tolerates being offline; a real
    // push must see the current remote before it commits on top of it.
    if dry_run {
        registry.refresh().ok();
    } else {
        registry.refresh()?;
    }
    let drift = registry.drift().unwrap_or_default();

    // Config may gain opt-ins during a real run; keep a mutable working copy.
    let mut config = config.clone();

    for cmd in targets(command)? {
        push_one(
            paths,
            &mut config,
            &registry,
            url,
            &cmd,
            drift.harness(&cmd),
            dry_run,
            verbose,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_one(
    paths: &Paths,
    config: &mut Config,
    registry: &Registry,
    url: &str,
    cmd: &str,
    drift: DriftState,
    dry_run: bool,
    verbose: bool,
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

    // Interactive opt-in for unrecognized files — real run on a TTY only. A dry
    // run never prompts: it just reports what would be offered.
    let mut include = scan.allowed.clone();
    if !dry_run && io::stdin().is_terminal() && !scan.unrecognized.is_empty() {
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
    }
    include.sort();
    include.dedup();

    // Secret-scan splits the candidates: skipped files never travel, and are
    // surfaced separately rather than counted as a capture.
    let mut secret_skips = Vec::new();
    include.retain(|rel| {
        let is_secret = std::fs::read_to_string(live.join(rel))
            .map(|c| looks_like_secret(&c))
            .unwrap_or(false);
        if is_secret {
            secret_skips.push(rel.clone());
        }
        !is_secret
    });

    let tree = tree_dir(paths, cmd);
    let plan = plan_capture(&live, &tree, &include)?;

    render_plan(cmd, &live, drift, &scan, &plan, &secret_skips, verbose);

    if dry_run {
        return Ok(());
    }

    capture_files(&live, &tree, &include)?;
    match registry.publish_worktree(
        &[pathspec(cmd)],
        &format!("chore(harness): sync {cmd} config"),
    )? {
        PublishOutcome::Published => {
            let n = plan.new.len() + plan.changed.len() + plan.removed.len();
            if n > 0 {
                println!("{cmd}: pushed {n} change(s) to {url}");
            } else {
                println!("{cmd}: pushed to {url}");
            }
        }
        PublishOutcome::NothingToDo => {
            // A clean plan already said "nothing to push"; only speak up when the
            // plan expected a change the registry turned out to already have.
            if !plan.is_empty() {
                println!("{cmd}: already up to date with the registry");
            }
        }
    }
    Ok(())
}

/// Print the capture plan: a per-harness header, the changes, and the buckets
/// held back. `verbose` lists every file; otherwise it is a compact summary.
fn render_plan(
    cmd: &str,
    live: &Path,
    drift: DriftState,
    scan: &DirScan,
    plan: &CapturePlan,
    secret_skips: &[String],
    verbose: bool,
) {
    let ahead = matches!(drift, DriftState::RemoteNewer | DriftState::Diverged);
    let banner = if plan.is_empty() {
        "nothing to push"
    } else if ahead {
        "remote newer — push rebases, ours-wins"
    } else {
        "clean push"
    };
    println!(
        "{cmd}  {} → registry (harnesses/{cmd})   [{banner}]",
        live.display()
    );

    if verbose {
        render_verbose(scan, plan);
    } else {
        render_concise(scan, plan);
    }

    for rel in secret_skips {
        println!("  ⚠ {rel} looks like a secret — would be skipped");
    }
}

/// Compact view: one line of `~/+/-` changes, then a counts tail.
fn render_concise(scan: &DirScan, plan: &CapturePlan) {
    let mut tokens = Vec::new();
    tokens.extend(plan.changed.iter().map(|r| format!("~ {r}")));
    tokens.extend(plan.new.iter().map(|r| format!("+ {r}")));
    tokens.extend(plan.removed.iter().map(|r| format!("- {r}")));
    if !tokens.is_empty() {
        println!("  {}", tokens.join("   "));
    }

    let mut segs = Vec::new();
    if !plan.unchanged.is_empty() {
        segs.push(format!("{} unchanged", plan.unchanged.len()));
    }
    if !scan.excluded.is_empty() {
        segs.push(format!(
            "{} excluded ({})",
            scan.excluded.len(),
            summarize(&scan.excluded, 2)
        ));
    }
    if !scan.unrecognized.is_empty() {
        segs.push(format!("{} unrecognized", scan.unrecognized.len()));
    }
    if !segs.is_empty() {
        println!("  {}", segs.join(" · "));
    }
}

/// Detailed view: every file, in its bucket, with a status word.
fn render_verbose(scan: &DirScan, plan: &CapturePlan) {
    if !plan.is_empty() || !plan.unchanged.is_empty() {
        println!("  capture:");
        for r in &plan.changed {
            println!("    ~ {r}   changed");
        }
        for r in &plan.new {
            println!("    + {r}   new");
        }
        for r in &plan.unchanged {
            println!("    = {r}   unchanged");
        }
        for r in &plan.removed {
            println!("    - {r}   removed (gone locally)");
        }
    }
    if !scan.excluded.is_empty() {
        println!("  excluded (never captured):");
        println!("    {}", scan.excluded.join(" · "));
    }
    if !scan.unrecognized.is_empty() {
        println!("  unrecognized (opt in via interactive push):");
        println!("    {}", scan.unrecognized.join(" · "));
    }
}

/// First `k` items joined, with a `+N` tail when there are more.
fn summarize(items: &[String], k: usize) -> String {
    if items.len() <= k {
        items.join(", ")
    } else {
        format!("{}, +{}", items[..k].join(", "), items.len() - k)
    }
}
