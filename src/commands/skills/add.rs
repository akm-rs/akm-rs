//! `akm skills add` — add spec(s) to project manifest.
//!
//! Behavior:
//! 1. Validate we're in a git repository
//! 2. Load or create the manifest (.agents/akm.json)
//! 3. For each ID:
//!    a. Check it exists in the library
//!    b. Determine its type (skill/agent)
//!    c. Add to the appropriate manifest array (idempotent — `unique`)
//!    d. If AKM_SESSION is active, also create session symlinks
//! 4. Save the manifest
//! 5. Refresh the project sidecar for harnesses with a `project_dir` (e.g.
//!    Posit Assistant), printing a mount count for those harnesses
//!
//! Idempotency: Adding an ID that's already present is a no-op for that ID.

use crate::error::{Error, Result};
use crate::git::Git;
use crate::library::manifest::Manifest;
use crate::library::sidecar;
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDirs;
use crate::library::Library;
use crate::paths::Paths;
use std::env;
use std::path::PathBuf;

/// Run the `akm skills add` command.
///
/// # Arguments
/// * `paths` — Resolved XDG paths
/// * `ids` — One or more spec IDs to add
/// * `tool_dirs` — Tool directories for session symlink creation
pub fn run(paths: &Paths, ids: &[String], tool_dirs: &ToolDirs) -> Result<()> {
    let library = Library::load_checked(paths)?;

    let project_root = Git::toplevel(None).map_err(|_| Error::ManifestNoProject)?;

    let mut manifest = Manifest::load_or_create(&project_root)?;

    let session_dir = env::var("AKM_SESSION").ok().map(PathBuf::from);
    let mut any_error = false;

    for id in ids {
        // Skip with an error if the spec is not in the library
        let spec = match library.get(id) {
            Some(s) => s,
            None => {
                eprintln!("✗ Not found in library: {id}");
                any_error = true;
                continue;
            }
        };

        let added = manifest.add(id, spec.spec_type);

        if added {
            println!("✓ Added to manifest: {id} ({})", spec.spec_type);
        } else {
            println!("✓ Already in manifest: {id} ({})", spec.spec_type);
        }

        // Auto-refresh staging if a session is active
        if let Some(ref staging) = session_dir {
            if staging.is_dir() {
                let _ = symlinks::create_session(
                    spec,
                    &paths.library_dir(),
                    staging,
                    &tool_dirs.staging_names(),
                );
            }
        }
    }

    manifest.save()?;

    let sidecar_report = sidecar::refresh(
        &project_root,
        &manifest,
        &paths.library_dir(),
        tool_dirs.tools(),
    )?;
    if tool_dirs.tools().iter().any(|t| t.project_dir.is_some()) {
        println!("✓ Posit sidecar: {} skill(s)", sidecar_report.mounted);
    }

    if any_error {
        // Partial failure already printed to stderr.
    }

    Ok(())
}
