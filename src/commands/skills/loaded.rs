//! `akm skills loaded` — show active session specs with provenance.
//!
//! Bash: `cmd_skills_loaded()` at bin/akm:1830–1907.
//!
//! Shows each spec with provenance: "manifest" or "loaded" (JIT).

use crate::error::Result;
use crate::git::Git;
use crate::library::manifest::Manifest;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use std::collections::HashSet;

use super::load::resolve_session;
use super::status::scan_session_dir;

/// Run the `akm skills loaded` command.
pub fn run(paths: &Paths, tool_dirs: &ToolDirs) -> Result<()> {
    let library = Library::load_checked(paths)?;
    let staging = resolve_session()?;

    let session_name = staging
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Build manifest ID set for provenance lookup
    let mut manifest_ids: HashSet<String> = HashSet::new();
    if let Ok(project_root) = Git::toplevel(None) {
        if let Ok(manifest) = Manifest::load(&project_root) {
            for id in manifest.skill_ids() {
                manifest_ids.insert(id.clone());
            }
            for id in manifest.agent_ids() {
                manifest_ids.insert(id.clone());
            }
        }
    }

    // Scan staging dir
    let session_specs = scan_session_dir(&staging, tool_dirs);

    // Print session specs with provenance
    println!("Active specs (session {session_name}):");
    if session_specs.is_empty() {
        println!("  (none)");
    } else {
        for (id, _spec_type) in &session_specs {
            let provenance = if manifest_ids.contains(id.as_str()) {
                "manifest"
            } else {
                "loaded"
            };
            println!("  + {id:<30} [{provenance}]");
        }
    }
    println!();

    // Print core specs
    println!("Core specs (global):");
    for id in library.core_ids() {
        println!("  + {id}");
    }

    Ok(())
}
