//! `akm skills import` — copy a skill from elsewhere into cold storage.
//!
//! Two address forms, one destination:
//!
//! - `import <url>` downloads a directory through the GitHub Contents API;
//! - `import <remote> <id>` takes it from a configured shared registry, which
//!   is a git checkout AKM already keeps in the cache.
//!
//! Either way the skill lands in the personal library as an ordinary spec. The
//! only trace of where it came from is `source` in its sidecar: there is no
//! lasting link, no upstream to track, and re-running the same import is how
//! you take a later version.
//!
//! `core` is never inherited from a registry. Its `core: true` means "core on
//! my machines", which is its owner's call about their own machines and nobody
//! else's — so an imported skill lands unmounted and the choice stays local.

use crate::commands::skills::promote::copy_dir_recursive;
use crate::config::Config;
use crate::error::{Error, IoContext, Result};
use crate::github::{self, GitHubClient, GitHubHttpClient, ParsedGitHubUrl};
use crate::library::frontmatter::Frontmatter;
use crate::library::libgen;
use crate::library::spec::SpecMeta;
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;

use super::shared;

/// What to do about an id that is already in the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    /// Keep what is already there; skip the incoming copy.
    Mine,
    /// Overwrite with the incoming copy.
    Theirs,
    /// Keep both — the incoming one is stored under a prefixed id.
    Both,
}

/// How an incoming skill relates to what is already in the library.
#[derive(Debug, PartialEq, Eq)]
enum Standing {
    /// Nothing of that id in the library.
    Fresh,
    /// Byte-identical to what is already there.
    Same,
    /// An id clash with different content.
    Clash,
}

/// A choice the user asked to apply to every remaining conflict.
#[derive(Default)]
struct Sticky(Option<Choice>);

/// Skills waiting to be installed: the id they arrived under, where their
/// files are staged, and the metadata to write beside them.
type Incoming = Vec<(String, std::path::PathBuf, SpecMeta)>;

/// Run `akm skills import <url>` — one skill from a GitHub URL.
pub fn run(
    paths: &Paths,
    config: &Config,
    url: &str,
    force: bool,
    custom_id: Option<&str>,
    tool_dirs: &ToolDirs,
) -> Result<()> {
    let parsed = github::parse_github_url(url)?;
    let id = match custom_id {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => parsed.default_skill_id().to_string(),
    };

    println!("Importing skill from GitHub...");
    println!("  repo:  {}/{}", parsed.owner, parsed.repo);
    println!("  ref:   {}", parsed.git_ref);
    println!("  path:  {}", parsed.path);
    println!("  id:    {id}");
    println!();

    let client = GitHubHttpClient::new();
    let staged = tempfile::tempdir().io_context("Creating temporary directory for import")?;
    let files = github::download_directory(&client, &parsed, staged.path())?;
    if files.is_empty() {
        return Err(Error::ImportNoSkillMd {
            url: url.to_string(),
        });
    }
    println!("  Downloaded {} file(s)", files.len());

    let skill_md = staged.path().join("SKILL.md");
    if !skill_md.is_file() {
        return Err(Error::ImportNoSkillMd {
            url: url.to_string(),
        });
    }
    let fm = Frontmatter::parse_file(&skill_md)?;
    fm.require_name_and_description(&skill_md)?;

    let library_dir = paths.library_dir();
    let mut sticky = Sticky::default();
    let resolved = match settle(
        &library_dir,
        &id,
        staged.path(),
        &parsed.owner,
        force,
        false,
        &mut sticky,
    )? {
        Some(id) => id,
        None => {
            println!("Aborted.");
            return Ok(());
        }
    };

    let meta = prompted_meta(&id, &fm, Some(parsed.browsable_url()));
    install(&library_dir, &resolved, staged.path(), meta)?;
    println!("  Copied skill to cold storage");

    rebuild(paths, tool_dirs, &library_dir)?;
    println!();
    println!("Imported skill '{resolved}' from GitHub");

    super::publish::offer(paths, config, &resolved);
    Ok(())
}

/// Run `akm skills import <remote> <id>` — one skill from a shared registry.
pub fn run_remote(
    paths: &Paths,
    config: &Config,
    remote: &str,
    id: &str,
    force: bool,
    custom_id: Option<&str>,
    tool_dirs: &ToolDirs,
) -> Result<()> {
    let registry = shared::open(paths, config, remote)?;
    println!("Refreshing '{remote}' ({})...", registry.url());
    shared::refresh_for_use(&registry)?;

    let catalogue = shared::catalogue(registry.dir())?;
    let candidate = catalogue
        .get(id)
        .ok_or_else(|| Error::SpecNotFound { id: id.to_string() })?;

    let stored_as = custom_id.filter(|id| !id.is_empty()).unwrap_or(id);
    let library_dir = paths.library_dir();
    let mut sticky = Sticky::default();

    let resolved = match settle(
        &library_dir,
        stored_as,
        &candidate.dir,
        remote,
        force,
        false,
        &mut sticky,
    )? {
        Some(id) => id,
        None => {
            println!("Kept your own '{stored_as}' — nothing was imported.");
            return Ok(());
        }
    };

    let meta = source_meta(&candidate.meta, remote, registry.url());
    let was_core = candidate.meta.core;
    install(&library_dir, &resolved, &candidate.dir, meta)?;
    rebuild(paths, tool_dirs, &library_dir)?;

    println!("Imported skill '{resolved}' from '{remote}'");
    if was_core {
        println!("  ('{remote}' marks it core; run 'akm skills edit {resolved} --meta' to do the same here)");
    }

    super::publish::offer(paths, config, &resolved);
    Ok(())
}

/// Copy one shared-registry skill into the library, non-interactively.
///
/// The TUI import path: no id-clash prompt (callers only offer skills not
/// already present), no `publish::offer` (deferred to a `pending_publish`
/// entry). Returns the pathspecs of what changed, for the exit-time publish
/// offer.
pub(crate) fn import_candidate(
    paths: &Paths,
    tool_dirs: &ToolDirs,
    candidate: &shared::Candidate,
    remote: &str,
    url: &str,
) -> Result<Vec<String>> {
    let library_dir = paths.library_dir();
    let meta = source_meta(&candidate.meta, remote, url);
    install(&library_dir, &candidate.id, &candidate.dir, meta)?;
    rebuild(paths, tool_dirs, &library_dir)?;
    Ok(crate::library::spec::SpecType::Skill.pathspecs(&candidate.id))
}

/// Run `akm skills import <remote> --all` — every valid skill in a registry.
pub fn run_all_remote(
    paths: &Paths,
    config: &Config,
    remote: &str,
    force: bool,
    tool_dirs: &ToolDirs,
) -> Result<()> {
    let registry = shared::open(paths, config, remote)?;
    println!("Refreshing '{remote}' ({})...", registry.url());
    shared::refresh_for_use(&registry)?;

    let catalogue = shared::catalogue(registry.dir())?;
    report_skipped(&catalogue.skipped);

    let url = registry.url().to_string();
    let incoming: Incoming = catalogue
        .skills
        .iter()
        .map(|c| {
            (
                c.id.clone(),
                c.dir.clone(),
                source_meta(&c.meta, remote, &url),
            )
        })
        .collect();

    import_many(paths, config, remote, incoming, force, tool_dirs)
}

/// Run `akm skills import <url> --all` — every valid skill under a GitHub path.
///
/// Each subdirectory is listed before anything is downloaded, so a repository
/// holding a dozen non-skill directories costs a dozen cheap listings rather
/// than a dozen full downloads.
pub fn run_all_url(
    paths: &Paths,
    config: &Config,
    url: &str,
    force: bool,
    tool_dirs: &ToolDirs,
) -> Result<()> {
    let parsed = github::parse_github_url(url)?;
    let client = GitHubHttpClient::new();

    println!(
        "Scanning {}/{} at {}...",
        parsed.owner, parsed.repo, parsed.path
    );

    let staged = tempfile::tempdir().io_context("Creating temporary directory for import")?;
    let (incoming, skipped) = download_all(&client, &parsed, staged.path())?;
    report_skipped(&skipped);

    import_many(paths, config, &parsed.owner, incoming, force, tool_dirs)
}

/// Download every subdirectory that holds a usable `SKILL.md`.
fn download_all(
    client: &dyn GitHubClient,
    parsed: &ParsedGitHubUrl,
    staged: &Path,
) -> Result<(Incoming, Vec<shared::Skipped>)> {
    let mut incoming = Vec::new();
    let mut skipped = Vec::new();

    let mut entries = client.list_contents(parsed, "")?;
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    for entry in entries.iter().filter(|e| e.entry_type == "dir") {
        let child = ParsedGitHubUrl {
            path: entry.path.clone(),
            ..parsed.clone()
        };

        let contents = client.list_contents(&child, "")?;
        if !contents.iter().any(|e| e.name == "SKILL.md") {
            skipped.push(shared::Skipped {
                id: entry.name.clone(),
                reason: "no SKILL.md".into(),
            });
            continue;
        }

        let dest = staged.join(&entry.name);
        std::fs::create_dir_all(&dest)
            .io_context(format!("Creating staging dir {}", dest.display()))?;
        github::download_directory(client, &child, &dest)?;

        let skill_md = dest.join("SKILL.md");
        let fm = match Frontmatter::parse_file(&skill_md)
            .and_then(|fm| fm.require_name_and_description(&skill_md).map(|()| fm))
        {
            Ok(fm) => fm,
            Err(e) => {
                skipped.push(shared::Skipped {
                    id: entry.name.clone(),
                    reason: format!("{e}").replace('\n', " "),
                });
                continue;
            }
        };

        let mut meta = SpecMeta::from_frontmatter(&entry.name, &fm);
        meta.source = Some(child.browsable_url());
        incoming.push((entry.name.clone(), dest, meta));
    }

    Ok((incoming, skipped))
}

/// Import a batch, resolving each id clash as it comes.
fn import_many(
    paths: &Paths,
    config: &Config,
    prefix: &str,
    incoming: Incoming,
    force: bool,
    tool_dirs: &ToolDirs,
) -> Result<()> {
    let library_dir = paths.library_dir();
    let mut sticky = Sticky::default();
    let mut imported: Vec<String> = Vec::new();
    let mut unchanged = 0usize;
    let mut kept_mine: Vec<String> = Vec::new();

    let fresh = incoming
        .iter()
        .filter(|(id, dir, _)| {
            standing(&library_dir.join("skills").join(id), dir) == Standing::Fresh
        })
        .count();
    println!();
    println!(
        "{} skill(s) available: {fresh} new, {} already in your library",
        incoming.len(),
        incoming.len() - fresh
    );
    println!();

    for (id, dir, meta) in &incoming {
        match settle(&library_dir, id, dir, prefix, force, true, &mut sticky)? {
            Some(resolved) => {
                install(&library_dir, &resolved, dir, meta.clone())?;
                imported.push(resolved);
            }
            None if standing(&library_dir.join("skills").join(id), dir) == Standing::Same => {
                unchanged += 1;
            }
            None => kept_mine.push(id.clone()),
        }
    }

    if imported.is_empty() {
        println!("Nothing imported.");
        return Ok(());
    }

    rebuild(paths, tool_dirs, &library_dir)?;

    println!();
    println!("Imported {} skill(s):", imported.len());
    for id in &imported {
        println!("  {id}");
    }
    if unchanged > 0 {
        println!("{unchanged} already up to date.");
    }
    if !kept_mine.is_empty() {
        println!("Kept your own: {}", kept_mine.join(", "));
    }

    if config.registry_url().is_some() {
        println!();
        println!("Publish them to your registry with 'akm skills publish'.");
    }

    Ok(())
}

/// Decide the id an incoming skill lands under, or `None` to skip it.
///
/// Identical content is skipped without asking: re-running an import must be
/// a no-op, or `--all` becomes a wall of prompts about skills that have not
/// moved. `--force` answers every clash with "theirs", which is what it has
/// always meant here.
fn settle(
    library_dir: &Path,
    id: &str,
    incoming: &Path,
    prefix: &str,
    force: bool,
    batch: bool,
    sticky: &mut Sticky,
) -> Result<Option<String>> {
    let dest = library_dir.join("skills").join(id);

    match standing(&dest, incoming) {
        Standing::Fresh => return Ok(Some(id.to_string())),
        Standing::Same => return Ok(None),
        Standing::Clash => {}
    }

    if force {
        return Ok(Some(id.to_string()));
    }

    // No terminal to ask. A single import fails, the way it always has, so a
    // script gets an exit code; a batch skips the clash and keeps going, since
    // one contested id must not cost the other eleven.
    if !io::stdin().is_terminal() {
        if !batch {
            return Err(Error::SpecAlreadyExists { id: id.to_string() });
        }
        println!("  {id}: already in your library, kept yours (no terminal to ask)");
        return Ok(None);
    }

    let choice = match sticky.0 {
        Some(choice) => choice,
        None => ask(id, prefix, sticky)?,
    };

    match choice {
        Choice::Mine => Ok(None),
        Choice::Theirs => Ok(Some(id.to_string())),
        Choice::Both => {
            let prefixed = format!("{prefix}-{id}");
            if library_dir.join("skills").join(&prefixed).exists() {
                return Err(Error::SpecAlreadyExists { id: prefixed });
            }
            Ok(Some(prefixed))
        }
    }
}

/// Prompt for one clash.
fn ask(id: &str, prefix: &str, sticky: &mut Sticky) -> Result<Choice> {
    loop {
        println!("  {id} — already in your library, and it differs.");
        print!("    [m]ine  [t]heirs  [b]oth as '{prefix}-{id}'  (add 'a' for all): ");
        io::stdout().flush().ok();

        let mut input = String::new();
        io::stdin().lock().read_line(&mut input).ok();
        let input = input.trim().to_lowercase();
        let all = input.ends_with('a') && input.len() > 1;

        let choice = match input.chars().next() {
            Some('m') => Choice::Mine,
            Some('t') => Choice::Theirs,
            Some('b') => Choice::Both,
            _ => {
                println!("    Please answer m, t or b.");
                continue;
            }
        };

        if all {
            sticky.0 = Some(choice);
        }
        return Ok(choice);
    }
}

/// Where an incoming skill stands relative to the library.
fn standing(dest: &Path, incoming: &Path) -> Standing {
    if !dest.exists() {
        Standing::Fresh
    } else if same_tree(dest, incoming) {
        Standing::Same
    } else {
        Standing::Clash
    }
}

/// Whether two directory trees hold the same files with the same bytes.
///
/// The sidecar is excluded: it carries `source`, which the import itself
/// writes, so comparing it would make every imported skill look changed the
/// moment it landed.
fn same_tree(a: &Path, b: &Path) -> bool {
    let (mut left, mut right) = (Vec::new(), Vec::new());
    if collect(a, a, &mut left).is_err() || collect(b, b, &mut right).is_err() {
        return false;
    }
    left.sort();
    right.sort();

    if left != right {
        return false;
    }

    left.iter().all(
        |rel| match (std::fs::read(a.join(rel)), std::fs::read(b.join(rel))) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        },
    )
}

/// Relative paths of every file under `dir`, ignoring the metadata sidecar.
fn collect(root: &Path, dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).io_context(format!("Reading {}", dir.display()))? {
        let path = entry.io_context("Reading directory entry")?.path();
        if path.is_dir() {
            collect(root, &path, out)?;
        } else if path.file_name().map(|n| n != "akm.json").unwrap_or(false) {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_path_buf());
            }
        }
    }
    Ok(())
}

/// Copy an incoming skill into the library and write its sidecar.
fn install(library_dir: &Path, id: &str, src: &Path, meta: SpecMeta) -> Result<()> {
    let dest = library_dir.join("skills").join(id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .io_context(format!("Removing existing skill at {}", dest.display()))?;
    }
    copy_dir_recursive(src, &dest)?;
    meta.save_to(&crate::library::spec::SpecType::Skill.sidecar_path(library_dir, id))
}

/// Metadata for a skill taken from a shared registry.
fn source_meta(meta: &SpecMeta, remote: &str, url: &str) -> SpecMeta {
    let mut meta = meta.clone();
    meta.source = Some(format!("{remote} ({url})"));
    // Core is a local decision — see the module docs.
    meta.core = false;
    meta
}

/// Ask for the metadata an interactive URL import has always asked for.
fn prompted_meta(id: &str, fm: &Frontmatter, source: Option<String>) -> SpecMeta {
    let mut meta = SpecMeta::from_frontmatter(id, fm);
    meta.source = source;

    if !io::stdin().is_terminal() {
        return meta;
    }

    print!("  Description [{}]: ", meta.description);
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input).ok();
    if !input.trim().is_empty() {
        meta.description = input.trim().to_string();
    }

    print!("  Tags (comma-separated) []: ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input).ok();
    meta.tags = input
        .trim()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    print!("  Core skill (always available globally)? [y/N]: ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input).ok();
    meta.core = input.trim().eq_ignore_ascii_case("y");

    println!();
    meta
}

/// Regenerate the index and the global symlinks after an import.
fn rebuild(paths: &Paths, tool_dirs: &ToolDirs, library_dir: &Path) -> Result<()> {
    libgen::generate(library_dir, &paths.library_json())?;
    let library = Library::load_from(&paths.library_json())?;
    let count = symlinks::rebuild_core(&library.core_specs(), library_dir, tool_dirs.dirs())?;
    println!("  {count} core symlinks rebuilt");
    Ok(())
}

/// Name the directories that looked like skills but could not be imported.
fn report_skipped(skipped: &[shared::Skipped]) {
    for entry in skipped {
        println!("  skipped {}: {}", entry.id, entry.reason);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn paths_under(root: &Path) -> Paths {
        Paths::from_roots(
            &root.join("data"),
            &root.join("config"),
            &root.join("cache"),
            &root.join("home"),
        )
    }

    #[test]
    fn import_candidate_copies_the_skill_and_returns_its_pathspecs() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_under(tmp.path());
        let tool_dirs = ToolDirs::builtin(&tmp.path().join("home"));

        // A shared checkout holding one importable skill.
        let checkout = tmp.path().join("checkout");
        write(
            &checkout.join("skills/tdd/SKILL.md"),
            "---\nname: tdd\ndescription: test-first\n---\nbody\n",
        );
        let catalogue = shared::catalogue(&checkout).unwrap();
        let candidate = catalogue.get("tdd").unwrap();

        let pathspecs = import_candidate(
            &paths,
            &tool_dirs,
            candidate,
            "acme",
            "git@example.com:acme.git",
        )
        .unwrap();

        // The skill now lives in the personal library...
        let dest = paths.library_dir().join("skills/tdd");
        assert!(dest.join("SKILL.md").is_file());
        // ...its sidecar records where it came from, core stripped...
        let meta = SpecMeta::load_from(&dest.join("akm.json")).unwrap();
        assert_eq!(
            meta.source.as_deref(),
            Some("acme (git@example.com:acme.git)")
        );
        assert!(!meta.core);
        // ...and the returned pathspecs cover the skill directory.
        assert_eq!(pathspecs, vec!["skills/tdd".to_string()]);
    }
}
