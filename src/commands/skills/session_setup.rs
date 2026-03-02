//! Hidden helper: set up session staging from project manifest in a single invocation.
//!
//! Called by the generated akm-init.sh to populate a staging directory with
//! symlinks for all specs declared in the project manifest. Replaces the
//! Bash version's per-spec jq lookups with a single process.
//!
//! Not intended for direct user invocation (hidden from help).

use crate::error::Result;
use crate::library::manifest::Manifest;
use crate::library::spec::SpecType;
use crate::library::Library;
use crate::paths::Paths;
use std::path::Path;

/// Set up session staging: read manifest, create symlinks for each spec.
///
/// Bash equivalent: the loop in `_akm_skills_session_start()` that calls
/// `_akm_create_session_symlink()` for each manifest entry.
///
/// This replaces N+1 subprocess invocations with a single one.
/// Returns Ok(()) even on partial failures (shell init handles gracefully).
pub fn run(paths: &Paths, staging_dir: &str, project_root: &str) -> Result<()> {
    let staging = Path::new(staging_dir);
    let root = Path::new(project_root);

    // Load library for spec type resolution
    let library = match Library::load(paths) {
        Ok(lib) => lib,
        Err(_) => return Ok(()), // No library — no specs to load
    };

    // Load manifest
    let manifest = match Manifest::load(root) {
        Ok(m) => m,
        Err(_) => return Ok(()), // No manifest — nothing to do
    };

    // Create symlinks for each spec in manifest
    let all_ids: Vec<&str> = manifest
        .skill_ids()
        .iter()
        .chain(manifest.agent_ids().iter())
        .map(|s| s.as_str())
        .collect();

    for id in all_ids {
        let spec = match library.get(id) {
            Some(s) => s,
            None => continue, // Spec not in library — skip
        };

        let (subdir, source_path) = match spec.spec_type {
            SpecType::Skill => ("skills", paths.skills_dir().join(id)),
            SpecType::Agent => ("agents", paths.agents_dir().join(format!("{id}.md"))),
        };

        if !source_path.exists() {
            continue;
        }

        // Create symlinks in each tool dir within staging
        for tool_dir in &[".claude", ".copilot", ".agents"] {
            let target_dir = staging.join(tool_dir).join(subdir);
            let link = if spec.spec_type == SpecType::Skill {
                target_dir.join(id)
            } else {
                target_dir.join(format!("{id}.md"))
            };
            // Use symlink, ignore errors (non-fatal)
            let _ = std::os::unix::fs::symlink(&source_path, &link);
        }
    }

    Ok(())
}
