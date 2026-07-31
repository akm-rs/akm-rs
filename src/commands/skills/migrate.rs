//! rc3 → rc4 data layout migration.
//!
//! rc3 kept the cold library as a *copy* of a registry clone: `skills/`,
//! `agents/` and `library.json` sat directly under `$XDG_DATA_HOME/akm`, with
//! the clone itself hidden in the cache. rc4 checks the registry out at
//! `akm/library` and works in it, which is what makes local edits survivable.
//!
//! The two layouts cannot coexist — the rc3 copies are not in the new working
//! tree, carry no git history, and would linger as dead weight. Decision D11 in
//! the sync-rework plan is that the migration is a clean wipe: everything the
//! old layout held came from the registry, so a fresh clone restores it.
//!
//! Only the paths the old layout owned are removed. `tools.json`, `shell/` and
//! `local.json` are siblings of `library/`, not part of it, and survive.

use crate::error::{IoContext, Result};
use crate::paths::Paths;
use std::path::PathBuf;

/// What the migration removed.
#[derive(Debug, Default)]
pub struct Rc3Wipe {
    /// Paths that were deleted, in the order they were removed.
    pub removed: Vec<PathBuf>,
}

impl Rc3Wipe {
    /// Whether anything was actually removed.
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty()
    }
}

/// Whether this machine still holds the rc3 layout.
///
/// The test is deliberately narrow: an rc3 install has specs directly under
/// the data dir *and* no `library/` checkout. Once `library/` exists, sync is
/// on the new layout and must never wipe anything again.
pub fn needs_migration(paths: &Paths) -> bool {
    paths.data_dir().join("skills").is_dir() && !paths.library_dir().exists()
}

/// Remove the rc3 cold library and its registry cache.
///
/// Idempotent: a path that is already gone is skipped rather than reported.
pub fn run(paths: &Paths) -> Result<Rc3Wipe> {
    let data_dir = paths.data_dir();
    let candidates = [
        data_dir.join("skills"),
        data_dir.join("agents"),
        data_dir.join("library.json"),
        // The rc3 clone lived here; rc4 has no cache at all.
        paths.cache_dir().join("skills-personal-registry"),
    ];

    let mut removed = Vec::new();
    for path in candidates {
        if path.is_dir() {
            std::fs::remove_dir_all(&path).io_context(format!("Removing {}", path.display()))?;
        } else if path.exists() {
            std::fs::remove_file(&path).io_context(format!("Removing {}", path.display()))?;
        } else {
            continue;
        }
        removed.push(path);
    }

    Ok(Rc3Wipe { removed })
}

/// Print what the migration did.
pub fn print_wipe(wipe: &Rc3Wipe) {
    if wipe.is_empty() {
        return;
    }

    println!("Migrating to the rc4 library layout — the registry is now checked");
    println!("out as the library itself. Removed the previous copy:");
    for path in &wipe.removed {
        println!("  {}", path.display());
    }
    println!("Anything not published to the registry is gone; a fresh clone follows.");
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Paths over a temp dir shaped like an rc3 install.
    fn rc3_install() -> (Paths, TempDir) {
        let tmp = TempDir::new().unwrap();
        let paths = Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            &tmp.path().join("home"),
        );

        let data = paths.data_dir().to_path_buf();
        std::fs::create_dir_all(data.join("skills").join("tdd")).unwrap();
        std::fs::write(data.join("skills").join("tdd").join("SKILL.md"), "old").unwrap();
        std::fs::create_dir_all(data.join("agents")).unwrap();
        std::fs::write(data.join("library.json"), "{}").unwrap();
        std::fs::create_dir_all(paths.cache_dir().join("skills-personal-registry")).unwrap();

        // AKM-owned files, siblings of the library.
        std::fs::create_dir_all(data.join("shell")).unwrap();
        std::fs::write(data.join("shell").join("akm-init.sh"), "#!/bin/bash").unwrap();
        std::fs::write(data.join("tools.json"), "[]").unwrap();
        std::fs::write(paths.local_json(), "{}").unwrap();

        (paths, tmp)
    }

    #[test]
    fn detects_the_rc3_layout() {
        let (paths, _tmp) = rc3_install();
        assert!(needs_migration(&paths));
    }

    #[test]
    fn a_checked_out_library_is_never_migrated() {
        let (paths, _tmp) = rc3_install();
        std::fs::create_dir_all(paths.library_dir()).unwrap();
        assert!(!needs_migration(&paths));
    }

    #[test]
    fn a_fresh_install_needs_no_migration() {
        let tmp = TempDir::new().unwrap();
        let paths = Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            &tmp.path().join("home"),
        );
        assert!(!needs_migration(&paths));
    }

    #[test]
    fn wipes_the_old_layout_only() {
        let (paths, _tmp) = rc3_install();
        let data = paths.data_dir().to_path_buf();

        let wipe = run(&paths).unwrap();
        assert_eq!(wipe.removed.len(), 4);

        assert!(!data.join("skills").exists());
        assert!(!data.join("agents").exists());
        assert!(!data.join("library.json").exists());
        assert!(!paths.cache_dir().join("skills-personal-registry").exists());

        // The invariant pinned by tests/paths_test.rs: akm-owned files are
        // siblings of the tree, not part of it.
        assert!(data.join("shell").join("akm-init.sh").is_file());
        assert!(data.join("tools.json").is_file());
        assert!(paths.local_json().is_file());
    }

    #[test]
    fn is_idempotent() {
        let (paths, _tmp) = rc3_install();
        run(&paths).unwrap();
        let second = run(&paths).unwrap();
        assert!(second.is_empty());
    }
}
