//! Library generation (libgen) — scan directories and build library.json.
//!
//! Bash equivalent: `_skills_libgen_for_dir()` at bin/akm:1412.

use crate::error::{Error, IoContext, Result};
use crate::library::frontmatter::Frontmatter;
use crate::library::spec::{Spec, SpecType};
use crate::library::Library;
use std::path::Path;

/// Result of a libgen operation.
#[derive(Debug)]
pub struct LibgenResult {
    /// Number of specs found on disk.
    pub count: usize,
    /// Path to the generated library.json.
    pub library_path: std::path::PathBuf,
}

/// Generate library.json for a directory containing skills/ and/or agents/.
///
/// This is the core libgen algorithm. It:
/// 1. Loads the existing library.json from `target_dir` (if present)
/// 2. Scans `target_dir/skills/` and `target_dir/agents/`
/// 3. Preserves metadata for existing specs, creates entries for new ones
/// 4. Writes the updated library.json
///
/// Bash: `_skills_libgen_for_dir()` at bin/akm:1412-1488
pub fn generate(target_dir: &Path) -> Result<LibgenResult> {
    let skills_dir = target_dir.join("skills");
    let agents_dir = target_dir.join("agents");

    if !skills_dir.is_dir() && !agents_dir.is_dir() {
        return Err(Error::NoSpecDirs {
            path: target_dir.to_path_buf(),
        });
    }

    let library_path = target_dir.join("library.json");

    // Load existing library to preserve metadata
    let existing = Library::load_or_default(&library_path)?;
    let existing_map = existing.spec_map();

    let mut specs: Vec<Spec> = Vec::new();

    // Scan skills/
    if skills_dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&skills_dir)
            .io_context(format!("Reading skills directory {}", skills_dir.display()))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .collect();

        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let dir_path = entry.path();
            let id = entry.file_name().to_string_lossy().to_string();
            let md_file = dir_path.join("SKILL.md");

            if !md_file.is_file() {
                continue;
            }

            if let Some(existing_spec) = existing_map.get(id.as_str()) {
                specs.push((*existing_spec).clone());
            } else {
                let fm = Frontmatter::parse_file(&md_file).unwrap_or_default();
                let name = fm.name.unwrap_or_else(|| id.clone());
                let description = fm.description.unwrap_or_default();
                specs.push(Spec::new(id, SpecType::Skill, name, description));
            }
        }
    }

    // Scan agents/
    if agents_dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&agents_dir)
            .io_context(format!("Reading agents directory {}", agents_dir.display()))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let path = entry.path();
                path.is_file() && path.extension().map(|ext| ext == "md").unwrap_or(false)
            })
            .collect();

        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let file_path = entry.path();
            let id = file_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            if id.is_empty() {
                continue;
            }

            if let Some(existing_spec) = existing_map.get(id.as_str()) {
                specs.push((*existing_spec).clone());
            } else {
                let fm = Frontmatter::parse_file(&file_path).unwrap_or_default();
                let name = fm.name.unwrap_or_else(|| id.clone());
                let description = fm.description.unwrap_or_default();
                specs.push(Spec::new(id, SpecType::Agent, name, description));
            }
        }
    }

    let count = specs.len();

    let library = Library { version: 1, specs };
    library.save_to(&library_path)?;

    Ok(LibgenResult {
        count,
        library_path,
    })
}
