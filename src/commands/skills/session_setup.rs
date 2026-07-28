//! Hidden helper: set up session staging from project manifest in a single invocation.
//!
//! Called by the generated akm-init.sh to populate a staging directory with
//! symlinks for all specs declared in the project manifest.
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

        // These must match the tool dirs in akm-init.sh's _akm_skills_session_start().
        // Vibe is intentionally excluded (doesn't support --add-dir).
        // Pi only reads `.pi/skills` (mounted with `--skill`) — it has no
        // subagent concept, so `.pi/agents` stays unused.
        for tool_dir in &[".claude", ".copilot", ".agents", ".pi"] {
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
