//! `akm skills remove` — remove spec(s) from project manifest.
//!
//! Bash: `cmd_skills_remove()` at bin/akm:1331–1407.
//!
//! Behavior:
//! 1. Validate we're in a git repository
//! 2. Load the manifest (if missing, warn and return OK)
//! 3. For each ID:
//!    a. If in library, determine type and remove from that array
//!    b. If NOT in library, try removing from both arrays
//!    c. If AKM_SESSION is active, also remove session symlinks
//! 4. Save the manifest
//!
//! Idempotency: Removing an ID that's not present warns but doesn't fail.

use crate::error::{Error, Result};
use crate::git::Git;
use crate::library::manifest::Manifest;
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use std::env;
use std::path::PathBuf;

/// Run the `akm skills remove` command.
pub fn run(paths: &Paths, ids: &[String], tool_dirs: &ToolDirs) -> Result<()> {
    let library = Library::load_checked(paths)?;

    let project_root = Git::toplevel(None).map_err(|_| Error::ManifestNoProject)?;

    // Bash: if manifest doesn't exist, warn and return 0
    let manifest_path = Manifest::path(&project_root);
    if !manifest_path.exists() {
        eprintln!("Warning: No manifest found at {}", manifest_path.display());
        return Ok(());
    }

    let mut manifest = Manifest::load(&project_root)?;
    let session_dir = env::var("AKM_SESSION").ok().map(PathBuf::from);

    for id in ids {
        // Bash: determine type if spec exists in library, otherwise try both arrays
        let spec_type = library.get(id).map(|s| s.spec_type);

        let removed = manifest.remove(id, spec_type);

        if removed {
            let type_label = spec_type
                .map(|t| format!("{t}"))
                .unwrap_or_else(|| "spec".to_string());
            println!("✓ Removed from manifest: {id} ({type_label})");

            // Auto-refresh staging if session is active
            if let Some(ref staging) = session_dir {
                if staging.is_dir() {
                    let _ = symlinks::remove_session(id, staging, tool_dirs.dirs());
                }
            }
        } else {
            eprintln!("Warning: {id} not found in manifest");
        }
    }

    manifest.save()?;
    Ok(())
}
