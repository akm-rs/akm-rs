//! Library generation (libgen) — scan directories and build library.json.

use crate::error::{Error, IoContext, Result};
use crate::library::frontmatter::Frontmatter;
use crate::library::spec::{Spec, SpecMeta, SpecType};
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
/// `library.json` is a pure *derived* index. Every entry comes from the spec's
/// `akm.json` sidecar where one exists, and otherwise from its markdown
/// frontmatter. Nothing is carried over from a previous `library.json`, which
/// is what stops the index and the sidecars from drifting apart.
///
/// libgen deliberately never *writes* a sidecar. It runs on every sync, and a
/// sidecar is a tracked file in the registry — writing one would mean a fresh
/// clone of a registry without sidecars instantly reported every spec as
/// locally modified and awaiting publication. Sidecars appear when the user
/// actually edits metadata; until then the frontmatter answers for the spec.
pub fn generate(target_dir: &Path) -> Result<LibgenResult> {
    let skills_dir = target_dir.join("skills");
    let agents_dir = target_dir.join("agents");

    if !skills_dir.is_dir() && !agents_dir.is_dir() {
        return Err(Error::NoSpecDirs {
            path: target_dir.to_path_buf(),
        });
    }

    let library_path = target_dir.join("library.json");
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
            let id = entry.file_name().to_string_lossy().to_string();
            let md_file = entry.path().join("SKILL.md");

            if !md_file.is_file() {
                continue;
            }

            let meta = resolve_meta(target_dir, &id, SpecType::Skill, &md_file);
            specs.push(Spec::from_meta(id, SpecType::Skill, meta));
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

            let meta = resolve_meta(target_dir, &id, SpecType::Agent, &file_path);
            specs.push(Spec::from_meta(id, SpecType::Agent, meta));
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

/// Resolve a spec's metadata: its sidecar if it has one, its frontmatter
/// otherwise.
///
/// A sidecar that exists but does not parse is reported and left untouched —
/// falling back in memory keeps the rest of the library usable without
/// overwriting whatever the user was in the middle of editing.
fn resolve_meta(library_dir: &Path, id: &str, spec_type: SpecType, md_file: &Path) -> SpecMeta {
    let sidecar = spec_type.sidecar_path(library_dir, id);

    if sidecar.is_file() {
        match SpecMeta::load_from(&sidecar) {
            Ok(meta) => return meta,
            Err(e) => eprintln!("Warning: {e}"),
        }
    }

    meta_from_frontmatter(id, md_file)
}

/// Derive starter metadata from a spec's markdown, tolerating a bad file.
fn meta_from_frontmatter(id: &str, md_file: &Path) -> SpecMeta {
    let fm = match Frontmatter::parse_file(md_file) {
        Ok(fm) => fm,
        Err(e) => {
            eprintln!("Warning: {e}");
            Frontmatter::default()
        }
    };
    SpecMeta::from_frontmatter(id, &fm)
}
