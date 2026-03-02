//! `akm skills search` — keyword search across library specs.
//!
//! Bash: `cmd_skills_search()` at bin/akm:1250–1277.
//!
//! Case-insensitive substring match against id, description, and tags.

use crate::error::Result;
use crate::library::Library;
use crate::paths::Paths;

/// Run the `akm skills search` command.
///
/// Bash: `cmd_skills_search()` at bin/akm:1250.
/// Case-insensitive substring match against id, description, and tags.
pub fn run(paths: &Paths, query: &str) -> Result<()> {
    let library = Library::load_checked(paths)?;

    let lquery = query.to_lowercase();

    for spec in &library.specs {
        let tags_str = spec.tags.join(",");
        let searchable = format!("{} {} {}", spec.id, spec.description, tags_str).to_lowercase();

        if searchable.contains(&lquery) {
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
    }

    Ok(())
}
