//! `akm skills import` — import a remote skill from a GitHub URL into cold storage.
//!
//! This is the remote counterpart to `promote`, which imports from local paths.
//! Downloads the skill directory via the GitHub Contents API, validates it,
//! runs interactive prompts, and copies to cold storage.

use crate::commands::skills::promote::copy_dir_recursive;
use crate::error::{Error, IoContext, Result};
use crate::github::{self, GitHubHttpClient};
use crate::library::frontmatter::Frontmatter;
use crate::library::libgen;
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use std::io::{self, BufRead, IsTerminal, Write};

/// Run the `akm skills import` command.
///
/// # Arguments
/// * `paths` — Resolved XDG paths
/// * `url` — GitHub URL to a directory containing SKILL.md
/// * `force` — Skip overwrite confirmation
/// * `custom_id` — Optional override for the skill ID
/// * `tool_dirs` — Tool directories for symlink rebuild
pub fn run(
    paths: &Paths,
    url: &str,
    force: bool,
    custom_id: Option<&str>,
    tool_dirs: &ToolDirs,
) -> Result<()> {
    // Step 1: Parse the GitHub URL
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

    // Step 2: Check overwrite BEFORE downloading (fail fast)
    let library_dir = paths.data_dir();
    let dest_path = library_dir.join("skills").join(&id);
    let is_tty = io::stdin().is_terminal();

    if dest_path.exists() && !force {
        if is_tty {
            print!("Skill '{id}' already exists in cold storage. Overwrite? [y/N]: ");
            io::stdout().flush().ok();
            let mut input = String::new();
            io::stdin().lock().read_line(&mut input).ok();
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Aborted.");
                return Ok(());
            }
        } else {
            return Err(Error::SpecAlreadyExists { id });
        }
    }

    // Step 3: Download to temp directory (atomic pattern)
    // TempDir is dropped (and cleaned up) on all exit paths, including errors
    let temp_dir = tempfile::tempdir().io_context("Creating temporary directory for import")?;

    let client = GitHubHttpClient::new();
    let files = github::download_directory(&client, &parsed, temp_dir.path())?;

    if files.is_empty() {
        return Err(Error::ImportNoSkillMd {
            url: url.to_string(),
        });
    }

    println!("  Downloaded {} file(s)", files.len());

    // Step 4: Validate SKILL.md exists and has valid frontmatter
    let skill_md = temp_dir.path().join("SKILL.md");
    if !skill_md.is_file() {
        return Err(Error::ImportNoSkillMd {
            url: url.to_string(),
        });
    }

    let fm = Frontmatter::parse_file(&skill_md)?;
    fm.require_name_and_description(&skill_md)?;

    let name = fm.name.unwrap_or_else(|| id.clone());
    let description = fm.description.unwrap_or_default();

    // Step 5: Interactive prompts (TTY only) — mirrors promote.rs
    let mut user_desc = description.clone();
    let mut user_tags: Vec<String> = Vec::new();
    let mut user_core = false;

    if is_tty {
        println!();
        println!("Importing skill:");
        println!("  id:     {id}");
        println!("  name:   {name}");
        println!("  source: {}", parsed.browsable_url());
        println!();

        print!("  Description [{description}]: ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input).ok();
        let input = input.trim();
        if !input.is_empty() {
            user_desc = input.to_string();
        }

        print!("  Tags (comma-separated) []: ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input).ok();
        let input = input.trim();
        if !input.is_empty() {
            user_tags = input
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }

        print!("  Core skill (always available globally)? [y/N]: ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input).ok();
        user_core = input.trim().eq_ignore_ascii_case("y");

        println!();
    }

    // Step 6: Copy to cold storage (from temp dir, not directly from GitHub)
    if dest_path.exists() {
        std::fs::remove_dir_all(&dest_path).io_context(format!(
            "Removing existing skill at {}",
            dest_path.display()
        ))?;
    }
    copy_dir_recursive(temp_dir.path(), &dest_path)?;
    println!("  Copied skill to cold storage");

    // Step 7: Regenerate library.json
    libgen::generate(library_dir)?;

    // Step 8: Patch entry with user-provided metadata + source URL
    let mut library = Library::load_from(&paths.library_json())?;
    if let Some(spec) = library.get_mut(&id) {
        spec.description = user_desc;
        spec.tags = user_tags;
        spec.core = user_core;
        spec.source = Some(parsed.browsable_url());
    }
    library.save(paths)?;

    // Step 9: Rebuild global symlinks
    let core_specs = library.core_specs();
    let count = symlinks::rebuild_core(&core_specs, library_dir, tool_dirs.dirs())?;
    println!("  {count} core symlinks rebuilt");
    println!();
    println!("Imported skill '{id}' from GitHub");

    Ok(())
}
