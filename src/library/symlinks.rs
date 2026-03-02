//! Symlink management for spec distribution.
//!
//! Handles three symlink operations:
//! 1. **Global symlinks** — core specs symlinked into global tool dirs (~/.claude/, etc.)
//! 2. **Session symlinks** — project/JIT specs symlinked into per-session staging dirs
//! 3. **Cleanup** — remove broken symlinks, clear existing links before rebuild
//!
//! All symlink functions take tool dirs as a parameter (no global state).
//! This makes them testable with temp directories.

use crate::error::{Error, IoContext, Result};
use crate::library::spec::{Spec, SpecType};
use std::path::{Path, PathBuf};

/// Spec subdirectory names used inside tool dirs and staging dirs.
const SPEC_SUBDIRS: &[&str] = &["skills", "agents"];

/// Create global symlinks for a single spec across all tool directories.
///
/// Bash: `_create_symlink()` at bin/akm:214–248
///
/// Returns `Ok(false)` if source doesn't exist on disk.
/// Returns `Ok(true)` if symlinks were created.
pub fn create_global(spec: &Spec, library_dir: &Path, tool_dirs: &[PathBuf]) -> Result<bool> {
    let source_path = spec.source_path(library_dir);

    if !source_path.exists() {
        return Ok(false);
    }

    for tool_dir in tool_dirs {
        let subdir = spec.spec_type.subdir();
        let target_dir = tool_dir.join(subdir);

        std::fs::create_dir_all(&target_dir)
            .io_context(format!("Creating directory {}", target_dir.display()))?;

        let link_path = match spec.spec_type {
            SpecType::Skill => target_dir.join(&spec.id),
            SpecType::Agent => target_dir.join(format!("{}.md", spec.id)),
        };

        // Remove existing real file/dir that would block symlink creation.
        if link_path.exists() && !link_path.is_symlink() {
            if link_path.is_dir() {
                std::fs::remove_dir_all(&link_path).io_context(format!(
                    "Removing existing directory at symlink target {}",
                    link_path.display()
                ))?;
            } else {
                std::fs::remove_file(&link_path).io_context(format!(
                    "Removing existing file at symlink target {}",
                    link_path.display()
                ))?;
            }
        }

        // Create symlink if it doesn't already exist.
        if !link_path.is_symlink() {
            create_symlink(&source_path, &link_path)?;
        }
    }

    Ok(true)
}

/// Create session symlinks for a single spec in a staging directory.
///
/// Bash: `_create_session_symlink()` at bin/akm:251–277
pub fn create_session(
    spec: &Spec,
    library_dir: &Path,
    staging_dir: &Path,
    tool_dirs: &[PathBuf],
) -> Result<bool> {
    let source_path = spec.source_path(library_dir);

    if !source_path.exists() {
        return Ok(false);
    }

    for tool_dir in tool_dirs {
        let tool_name = tool_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let subdir = spec.spec_type.subdir();
        let target_dir = staging_dir.join(&tool_name).join(subdir);

        std::fs::create_dir_all(&target_dir).io_context(format!(
            "Creating staging directory {}",
            target_dir.display()
        ))?;

        let link_path = match spec.spec_type {
            SpecType::Skill => target_dir.join(&spec.id),
            SpecType::Agent => target_dir.join(format!("{}.md", spec.id)),
        };

        // In session mode, always force-create (ln -sfn)
        create_symlink(&source_path, &link_path)?;
    }

    Ok(true)
}

/// Remove session symlinks for a spec from a staging directory.
///
/// Bash: `_remove_session_symlink()` at bin/akm:280–307
///
/// Returns `Ok(true)` if any symlinks were found and removed.
pub fn remove_session(id: &str, staging_dir: &Path, tool_dirs: &[PathBuf]) -> Result<bool> {
    let mut found = false;

    for tool_dir in tool_dirs {
        let tool_name = tool_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let skill_link = staging_dir.join(&tool_name).join("skills").join(id);
        if skill_link.is_symlink() {
            std::fs::remove_file(&skill_link)
                .io_context(format!("Removing session symlink {}", skill_link.display()))?;
            found = true;
        }

        let agent_link = staging_dir
            .join(&tool_name)
            .join("agents")
            .join(format!("{id}.md"));
        if agent_link.is_symlink() {
            std::fs::remove_file(&agent_link)
                .io_context(format!("Removing session symlink {}", agent_link.display()))?;
            found = true;
        }
    }

    Ok(found)
}

/// Clear all symlinks (not regular files/dirs) from tool dirs.
///
/// Bash: bin/akm:1617–1624
///
/// Only removes symlinks — real files and directories are left intact.
/// Returns the number of symlinks removed.
pub fn clear_all(tool_dirs: &[PathBuf]) -> Result<usize> {
    let mut count = 0;

    for tool_dir in tool_dirs {
        for subdir in SPEC_SUBDIRS {
            let dir = tool_dir.join(subdir);
            if !dir.is_dir() {
                continue;
            }

            let entries = std::fs::read_dir(&dir)
                .io_context(format!("Reading directory {}", dir.display()))?;

            for entry in entries {
                let entry =
                    entry.io_context(format!("Reading directory entry in {}", dir.display()))?;
                let path = entry.path();

                if path.is_symlink() {
                    std::fs::remove_file(&path)
                        .io_context(format!("Removing symlink {}", path.display()))?;
                    count += 1;
                }
            }
        }
    }

    Ok(count)
}

/// Clean broken symlinks from tool dirs.
///
/// Bash: bin/akm:1627–1633
///
/// A broken symlink is one where `is_symlink()` is true but `exists()` is false.
/// Returns the number of broken symlinks removed.
pub fn clean_broken(tool_dirs: &[PathBuf]) -> Result<usize> {
    let mut count = 0;

    for tool_dir in tool_dirs {
        for subdir in SPEC_SUBDIRS {
            let dir = tool_dir.join(subdir);
            if !dir.is_dir() {
                continue;
            }

            let entries = std::fs::read_dir(&dir)
                .io_context(format!("Reading directory {}", dir.display()))?;

            for entry in entries {
                let entry =
                    entry.io_context(format!("Reading directory entry in {}", dir.display()))?;
                let path = entry.path();

                // Broken symlink: is_symlink() == true, exists() == false
                if path.is_symlink() && !path.exists() {
                    std::fs::remove_file(&path)
                        .io_context(format!("Removing broken symlink {}", path.display()))?;
                    count += 1;
                }
            }
        }
    }

    Ok(count)
}

/// Rebuild global symlinks for all core specs.
///
/// Bash: bin/akm:1611–1641 (steps 5 of cmd_skills_sync)
///
/// This is the high-level function called by the sync command.
/// It clears all existing symlinks, cleans broken ones, then creates
/// fresh symlinks for every core spec.
///
/// Returns the number of symlinks successfully created.
pub fn rebuild_core(
    core_specs: &[&Spec],
    library_dir: &Path,
    tool_dirs: &[PathBuf],
) -> Result<usize> {
    // Step 1: Clear all existing symlinks
    clear_all(tool_dirs)?;

    // Step 2: Clean any broken symlinks
    clean_broken(tool_dirs)?;

    // Step 3: Create symlinks for each core spec
    let mut count = 0;
    for spec in core_specs {
        match create_global(spec, library_dir, tool_dirs) {
            Ok(true) => count += 1,
            Ok(false) => {
                // Source doesn't exist — skip silently (matches Bash || true)
            }
            Err(e) => {
                // Log warning but continue (matches Bash || true)
                eprintln!("Warning: Failed to create symlink for '{}': {e}", spec.id);
            }
        }
    }

    Ok(count)
}

/// Platform-specific symlink creation.
fn create_symlink(source: &Path, link: &Path) -> Result<()> {
    // Remove existing symlink if present (equivalent of ln -sf)
    if link.is_symlink() {
        std::fs::remove_file(link)
            .io_context(format!("Removing existing symlink {}", link.display()))?;
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, link).map_err(|e| Error::SymlinkCreate {
            link: link.to_path_buf(),
            target: source.to_path_buf(),
            source: e,
        })?;
    }

    #[cfg(not(unix))]
    {
        compile_error!("AKM requires a Unix-like operating system for symlink support");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::spec::{Spec, SpecType};
    use tempfile::TempDir;

    fn make_skill_spec(id: &str) -> Spec {
        Spec::new(id, SpecType::Skill, id, "test skill")
    }

    fn make_agent_spec(id: &str) -> Spec {
        Spec::new(id, SpecType::Agent, id, "test agent")
    }

    fn create_skill_on_disk(library_dir: &Path, id: &str) {
        let skill_dir = library_dir.join("skills").join(id);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {id}\ndescription: test\n---\nContent"),
        )
        .unwrap();
    }

    fn create_agent_on_disk(library_dir: &Path, id: &str) {
        let agents_dir = library_dir.join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(
            agents_dir.join(format!("{id}.md")),
            format!("---\nname: {id}\ndescription: test\n---\nContent"),
        )
        .unwrap();
    }

    #[test]
    fn create_global_skill_symlink() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let tool_dir = tmp.path().join("home").join(".claude");

        create_skill_on_disk(&lib_dir, "tdd");
        let spec = make_skill_spec("tdd");
        let tool_dirs = vec![tool_dir.clone()];

        let created = create_global(&spec, &lib_dir, &tool_dirs).unwrap();
        assert!(created);

        let link = tool_dir.join("skills").join("tdd");
        assert!(link.is_symlink());
        assert!(link.join("SKILL.md").exists());
    }

    #[test]
    fn create_global_agent_symlink() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let tool_dir = tmp.path().join("home").join(".copilot");

        create_agent_on_disk(&lib_dir, "reviewer");
        let spec = make_agent_spec("reviewer");
        let tool_dirs = vec![tool_dir.clone()];

        let created = create_global(&spec, &lib_dir, &tool_dirs).unwrap();
        assert!(created);

        let link = tool_dir.join("agents").join("reviewer.md");
        assert!(link.is_symlink());
    }

    #[test]
    fn create_global_returns_false_for_missing_source() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let tool_dir = tmp.path().join("home").join(".claude");
        let spec = make_skill_spec("nonexistent");
        let tool_dirs = vec![tool_dir];

        let created = create_global(&spec, &lib_dir, &tool_dirs).unwrap();
        assert!(!created);
    }

    #[test]
    fn clear_all_removes_only_symlinks() {
        let tmp = TempDir::new().unwrap();
        let tool_dir = tmp.path().join("home").join(".claude");
        let skills_dir = tool_dir.join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();

        // Create a real file (should not be removed)
        std::fs::write(skills_dir.join("real-file.txt"), "data").unwrap();

        // Create a symlink (should be removed)
        let target = tmp.path().join("target");
        std::fs::create_dir_all(&target).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, skills_dir.join("test-skill")).unwrap();

        let removed = clear_all(&[tool_dir]).unwrap();

        #[cfg(unix)]
        {
            assert_eq!(removed, 1);
            assert!(!skills_dir.join("test-skill").exists());
        }
        assert!(skills_dir.join("real-file.txt").exists());
    }

    #[test]
    fn clean_broken_removes_dangling_symlinks() {
        let tmp = TempDir::new().unwrap();
        let tool_dir = tmp.path().join("home").join(".claude");
        let skills_dir = tool_dir.join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();

        let nonexistent = tmp.path().join("does-not-exist");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&nonexistent, skills_dir.join("broken-skill")).unwrap();

        let removed = clean_broken(&[tool_dir]).unwrap();

        #[cfg(unix)]
        assert_eq!(removed, 1);
    }

    #[test]
    fn rebuild_core_end_to_end() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let tool_dir = tmp.path().join("home").join(".claude");

        create_skill_on_disk(&lib_dir, "core-skill");
        create_agent_on_disk(&lib_dir, "core-agent");

        let mut skill_spec = make_skill_spec("core-skill");
        skill_spec.core = true;
        let mut agent_spec = make_agent_spec("core-agent");
        agent_spec.core = true;

        let core_specs: Vec<&Spec> = vec![&skill_spec, &agent_spec];
        let tool_dirs = vec![tool_dir.clone()];

        let count = rebuild_core(&core_specs, &lib_dir, &tool_dirs).unwrap();
        assert_eq!(count, 2);

        assert!(tool_dir.join("skills").join("core-skill").is_symlink());
        assert!(tool_dir.join("agents").join("core-agent.md").is_symlink());
    }

    #[test]
    fn rebuild_core_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let tool_dir = tmp.path().join("home").join(".claude");

        create_skill_on_disk(&lib_dir, "my-skill");
        let mut spec = make_skill_spec("my-skill");
        spec.core = true;

        let core_specs: Vec<&Spec> = vec![&spec];
        let tool_dirs = vec![tool_dir.clone()];

        let count1 = rebuild_core(&core_specs, &lib_dir, &tool_dirs).unwrap();
        let count2 = rebuild_core(&core_specs, &lib_dir, &tool_dirs).unwrap();
        assert_eq!(count1, count2);
        assert_eq!(count1, 1);
    }

    #[test]
    fn create_global_replaces_real_dir_with_symlink() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let tool_dir = tmp.path().join("home").join(".claude");

        create_skill_on_disk(&lib_dir, "tdd");

        // Create a real directory where the symlink should go
        let blocking_dir = tool_dir.join("skills").join("tdd");
        std::fs::create_dir_all(&blocking_dir).unwrap();
        std::fs::write(blocking_dir.join("stale.txt"), "old data").unwrap();

        let spec = make_skill_spec("tdd");
        let created = create_global(&spec, &lib_dir, std::slice::from_ref(&tool_dir)).unwrap();
        assert!(created);

        let link = tool_dir.join("skills").join("tdd");
        assert!(link.is_symlink());
    }

    #[test]
    fn session_symlinks_create_and_remove() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let staging = tmp.path().join("session");
        let tool_dirs = vec![
            tmp.path().join("home").join(".claude"),
            tmp.path().join("home").join(".copilot"),
        ];

        create_skill_on_disk(&lib_dir, "tdd");
        let spec = make_skill_spec("tdd");

        let created = create_session(&spec, &lib_dir, &staging, &tool_dirs).unwrap();
        assert!(created);
        assert!(staging
            .join(".claude")
            .join("skills")
            .join("tdd")
            .is_symlink());
        assert!(staging
            .join(".copilot")
            .join("skills")
            .join("tdd")
            .is_symlink());

        let removed = remove_session("tdd", &staging, &tool_dirs).unwrap();
        assert!(removed);
        assert!(!staging.join(".claude").join("skills").join("tdd").exists());
    }
}
