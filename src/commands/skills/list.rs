//! `akm skills list` — browse library specs.
//!
//! Bash: `cmd_skills_list()` at bin/akm:1215–1248.
//!
//! Behavior:
//! 1. Load the library
//! 2. Apply optional --tag and --type filters
//! 3. Print each matching spec: type, id, description, \[CORE\] marker
//!
//! Plain output for now. TUI will be added in Task 9.

use crate::error::Result;
use crate::library::spec::SpecType;
use crate::library::Library;
use crate::paths::Paths;

/// Run the `akm skills list` command.
///
/// # Arguments
/// * `paths` — Resolved XDG paths
/// * `tag` — Optional tag filter
/// * `type_filter` — Optional type filter ("skill" or "agent")
pub fn run(paths: &Paths, tag: Option<&str>, type_filter: Option<&str>) -> Result<()> {
    let library = Library::load_checked(paths)?;

    // Parse type filter once if provided
    let parsed_type: Option<SpecType> = type_filter.map(|t| t.parse::<SpecType>()).transpose()?;

    for spec in &library.specs {
        // Apply type filter
        if let Some(ref ft) = parsed_type {
            if spec.spec_type != *ft {
                continue;
            }
        }

        // Apply tag filter
        if let Some(tag) = tag {
            if !spec.tags.iter().any(|t| t == tag) {
                continue;
            }
        }

        let type_label = format!("{:<6}", spec.spec_type);
        if spec.core {
            println!(
                "  {type_label}  {:<35} {} [CORE]",
                spec.id, spec.description
            );
        } else {
            println!("  {type_label}  {:<35} {}", spec.id, spec.description);
        }
    }

    Ok(())
}
