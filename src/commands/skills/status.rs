//! `akm skills status` — full status overview.
//!
//! Bash: `cmd_skills_status()` at bin/akm:1040–1213.
//! Task 9 adds a TUI dashboard mode.

use crate::error::Result;
use crate::git::Git;
use crate::library::manifest::Manifest;
use crate::library::spec::SpecType;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};

use super::list::should_use_tui;

/// Run the `akm skills status` command.
pub fn run(paths: &Paths, tool_dirs: &ToolDirs, plain: bool) -> Result<()> {
    if should_use_tui(plain) {
        crate::tui::status::run(paths, tool_dirs)
    } else {
        run_plain(paths, tool_dirs)
    }
}

/// Plain output mode — identical to Task 4 implementation.
fn run_plain(paths: &Paths, tool_dirs: &ToolDirs) -> Result<()> {
    let library = Library::load_checked(paths)?;

    // Section 1: Project info
    let project_root = Git::toplevel(None).ok();
    let project_name = project_root
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string());

    if let Some(ref name) = project_name {
        let root_display = project_root
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        println!("Project: {name} ({root_display})");
    } else {
        println!("Project: (none)");
    }
    println!();

    // Section 2: Core specs
    println!("Core specs (globally symlinked):");
    let core_specs = library.core_specs();
    let core_ids: HashSet<&str> = core_specs.iter().map(|s| s.id.as_str()).collect();

    for spec in &core_specs {
        let type_label = format!("{:<6}", spec.spec_type);
        println!("  ✓ {type_label}  {}", spec.id);
    }
    println!();

    // Section 3: Session specs (if AKM_SESSION is active)
    let session_dir = env::var("AKM_SESSION").ok().map(PathBuf::from);
    if let Some(ref staging) = session_dir {
        if staging.is_dir() {
            println!("Session specs (staging dir):");
            let session_specs = scan_session_dir(staging, tool_dirs);
            if session_specs.is_empty() {
                println!("  (none loaded)");
            } else {
                for (id, spec_type) in &session_specs {
                    let type_label = format!("{:<6}", spec_type);
                    println!("  ✓ {type_label}  {id}");
                }
            }
            println!();
        }
    }

    // Section 4: Manifest specs
    let mut manifest_ids: HashSet<String> = HashSet::new();

    if let Some(ref root) = project_root {
        let manifest_path = Manifest::path(root);
        if manifest_path.exists() {
            if let Ok(manifest) = Manifest::load(root) {
                println!("Manifest specs (.agents/akm.json):");
                let mut manifest_lines = Vec::new();

                for id in manifest.skill_ids() {
                    manifest_ids.insert(id.clone());
                    if library.contains(id) {
                        manifest_lines.push(format!("  ✓ {:<6}  {id}", "skill"));
                    } else {
                        manifest_lines.push(format!("  ? skill   {id} (not in library)"));
                    }
                }

                for id in manifest.agent_ids() {
                    manifest_ids.insert(id.clone());
                    if library.contains(id) {
                        manifest_lines.push(format!("  ✓ {:<6}  {id}", "agent"));
                    } else {
                        manifest_lines.push(format!("  ? agent   {id} (not in library)"));
                    }
                }

                if manifest_lines.is_empty() {
                    println!("  (empty manifest)");
                } else {
                    for line in &manifest_lines {
                        println!("{line}");
                    }
                }
                println!();
            }
        }
    }

    // Section 5: Cold (available) — not core, not in manifest
    println!("Cold (available):");
    for spec in &library.specs {
        if core_ids.contains(spec.id.as_str()) {
            continue;
        }
        if manifest_ids.contains(&spec.id) {
            continue;
        }
        let type_label = format!("{:<6}", spec.spec_type);
        println!("  ○ {type_label}  {}", spec.id);
    }

    Ok(())
}

/// Scan the session staging directory for loaded specs.
///
/// Returns (id, SpecType) pairs.
///
/// `pub(crate)` visibility: also used by `loaded.rs`.
pub(crate) fn scan_session_dir(staging: &Path, tool_dirs: &ToolDirs) -> Vec<(String, SpecType)> {
    let mut result = Vec::new();

    // Use first tool dir as representative (Bash uses `.claude`)
    let first_tool_name = tool_dirs
        .dirs()
        .first()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| ".claude".to_string());

    // Scan skills/ for directory symlinks
    let skills_dir = staging.join(&first_tool_name).join("skills");
    if let Ok(entries) = std::fs::read_dir(&skills_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_symlink() && path.is_dir() {
                if let Some(id) = path.file_name().map(|n| n.to_string_lossy().to_string()) {
                    result.push((id, SpecType::Skill));
                }
            }
        }
    }

    // Scan agents/ for file symlinks (.md)
    let agents_dir = staging.join(&first_tool_name).join("agents");
    if let Ok(entries) = std::fs::read_dir(&agents_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_symlink() && path.is_file() {
                if let Some(stem) = path.file_stem().map(|n| n.to_string_lossy().to_string()) {
                    if path.extension().map(|e| e == "md").unwrap_or(false) {
                        result.push((stem, SpecType::Agent));
                    }
                }
            }
        }
    }

    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}
