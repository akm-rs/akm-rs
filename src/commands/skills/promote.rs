//! `akm skills promote` — import a local skill into cold storage.
//!
//! Bash: `cmd_skills_promote()` at bin/akm:2273–2449.
//!
//! Only skills (directories with SKILL.md) can be promoted, not agents.

use crate::error::{Error, IoContext, Result};
use crate::library::frontmatter::Frontmatter;
use crate::library::libgen;
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

/// Run the `akm skills promote` command.
///
/// # Arguments
/// * `paths` — Resolved XDG paths
/// * `spec_path` — Path to directory containing SKILL.md
/// * `force` — Skip overwrite confirmation
/// * `tool_dirs` — Tool directories for symlink rebuild
pub fn run(paths: &Paths, spec_path: &str, force: bool, tool_dirs: &ToolDirs) -> Result<()> {
    // Step 1: Resolve and validate path
    let spec_path = PathBuf::from(spec_path);
    let spec_path = if spec_path.is_absolute() {
        spec_path
    } else {
        std::env::current_dir()
            .io_context("Getting current directory")?
            .join(&spec_path)
    };

    if !spec_path.is_dir() {
        return Err(Error::PromoteDirNotFound { path: spec_path });
    }

    let md_file = spec_path.join("SKILL.md");
    if !md_file.is_file() {
        return Err(Error::NoSkillMd { path: spec_path });
    }

    // Step 2: Extract and validate frontmatter
    let fm = Frontmatter::parse_file(&md_file)?;
    fm.require_name_and_description(&md_file)?;

    let id = spec_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let name = fm.name.unwrap_or_else(|| id.clone());
    let description = fm.description.unwrap_or_default();

    // Step 3: Interactive prompts (TTY only)
    let is_tty = io::stdin().is_terminal();

    let mut user_desc = description.clone();
    let mut user_tags: Vec<String> = Vec::new();
    let mut user_core = false;

    if is_tty {
        println!("Promoting skill:");
        println!("  id:   {id}");
        println!("  name: {name}");
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

    // Step 4: Check overwrite
    let library_dir = paths.data_dir();
    let dest_path = library_dir.join("skills").join(&id);

    if dest_path.exists() && !force {
        if is_tty {
            print!("Overwrite existing skill '{id}'? [y/N]: ");
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

    // Step 5: Copy to cold storage
    if dest_path.exists() {
        std::fs::remove_dir_all(&dest_path).io_context(format!(
            "Removing existing skill at {}",
            dest_path.display()
        ))?;
    }
    copy_dir_recursive(&spec_path, &dest_path)?;
    println!("  Copied skill to cold storage");

    // Step 6: Regenerate library.json
    libgen::generate(library_dir)?;

    // Step 7: Patch entry with user-provided metadata
    let mut library = Library::load_from(&paths.library_json())?;
    if let Some(spec) = library.get_mut(&id) {
        spec.description = user_desc;
        spec.tags = user_tags;
        spec.core = user_core;
    }
    library.save(paths)?;

    // Step 8: Rebuild global symlinks
    let core_specs = library.core_specs();
    let count = symlinks::rebuild_core(&core_specs, library_dir, tool_dirs.dirs())?;
    println!("  {count} core symlinks rebuilt");
    println!();
    println!("Promoted skill '{id}' to cold storage");

    Ok(())
}

/// Recursively copy a directory.
///
/// Bash: `cp -r "$source" "$dest"`
///
/// `pub(crate)` visibility: also used by `publish.rs`.
pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).io_context(format!("Creating directory {}", dst.display()))?;

    for entry in
        std::fs::read_dir(src).io_context(format!("Reading directory {}", src.display()))?
    {
        let entry = entry.io_context("Reading directory entry")?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).io_context(format!(
                "Copying {} to {}",
                src_path.display(),
                dst_path.display()
            ))?;
        }
    }

    Ok(())
}
