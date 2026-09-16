//! Symlink management for spec distribution.
//!
//! Handles three symlink operations:
//! 1. **Global mounts** — core specs mounted into global tool dirs (~/.claude/, etc.)
//! 2. **Session symlinks** — project/JIT specs symlinked into per-session staging dirs
//! 3. **Cleanup** — remove broken symlinks, clear existing links before rebuild
//!
//! Global mounts come in two kinds (see [`Mount`]):
//! - [`Mount::Symlink`] — `<tool dir>/skills/<id>` is a symlink to the library
//!   skill, `<tool dir>/agents/<id>.md` a symlink to the library agent.
//! - [`Mount::Tree`] — `<tool dir>/skills/<id>/` is a *real* directory whose
//!   entries are symlinks into the library skill, for harnesses whose skill
//!   discovery skips symlinked directories. Agents are not mounted at all.
//!
//! A target with `agents == false` (Vibe, whose agents are TOML configs) gets
//! skills only under either kind.
//!
//! All mount functions take the targets as a parameter — no global state.

use crate::error::{Error, IoContext, Result};
use crate::library::spec::{Spec, SpecType};
use crate::library::tool_dirs::{Mount, MountTarget};
use std::path::Path;

/// Spec subdirectory names used inside tool dirs and staging dirs.
const SPEC_SUBDIRS: &[&str] = &["skills", "agents"];

/// Create global mounts for a single spec across all mount targets.
///
/// Returns `Ok(false)` if source doesn't exist on disk.
/// Returns `Ok(true)` if the source existed and was mounted wherever the
/// target's mount kind applies — [`Mount::Tree`] targets and targets with
/// `agents == false` skip agents.
pub fn create_global(spec: &Spec, library_dir: &Path, targets: &[MountTarget]) -> Result<bool> {
    let source_path = spec.source_path(library_dir);

    if !source_path.exists() {
        return Ok(false);
    }

    for target in targets {
        if spec.spec_type == SpecType::Agent && !target.agents {
            continue;
        }

        let subdir = spec.spec_type.subdir();
        let target_dir = target.dir.join(subdir);

        if target.mount == Mount::Tree {
            // Tree mounts carry skills only.
            if spec.spec_type != SpecType::Skill {
                continue;
            }

            std::fs::create_dir_all(&target_dir)
                .io_context(format!("Creating directory {}", target_dir.display()))?;

            let tree_path = target_dir.join(&spec.id);
            if !create_tree(&source_path, &tree_path, library_dir)? {
                eprintln!(
                    "Warning: {} exists and is not managed by akm — left in place",
                    tree_path.display()
                );
            }
            continue;
        }

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

/// Materialize `source_dir` as a real directory at `target` whose entries are
/// symlinks to the corresponding entries of `source_dir`.
///
/// Idempotent: stale entries (symlinks whose name no longer exists in the
/// source) are removed, missing ones added. Refuses to touch a target that
/// exists and is not akm-owned, returning `Ok(false)` — under a tree mount the
/// user's own hand-authored skills live in the same directory.
pub fn create_tree(source_dir: &Path, target: &Path, library_dir: &Path) -> Result<bool> {
    // A symlink at the target is an old-style mount — ours to replace.
    if target.is_symlink() {
        std::fs::remove_file(target)
            .io_context(format!("Removing symlink mount {}", target.display()))?;
    } else if target.exists() && !is_akm_tree(target, library_dir) {
        return Ok(false);
    }

    std::fs::create_dir_all(target)
        .io_context(format!("Creating directory {}", target.display()))?;

    let entries = std::fs::read_dir(source_dir)
        .io_context(format!("Reading directory {}", source_dir.display()))?;

    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.io_context(format!(
            "Reading directory entry in {}",
            source_dir.display()
        ))?;
        let name = entry.file_name();
        create_symlink(&source_dir.join(&name), &target.join(&name))?;
        names.push(name);
    }

    // Drop links whose source entry is gone.
    let existing =
        std::fs::read_dir(target).io_context(format!("Reading directory {}", target.display()))?;
    for entry in existing {
        let entry = entry.io_context(format!("Reading directory entry in {}", target.display()))?;
        let path = entry.path();
        if path.is_symlink() && !names.contains(&entry.file_name()) {
            std::fs::remove_file(&path)
                .io_context(format!("Removing stale symlink {}", path.display()))?;
        }
    }

    Ok(true)
}

/// Whether `dir` is a tree akm created.
///
/// True iff `dir` is a real directory (not a symlink) and every entry is a
/// symlink whose target lies under `library_dir`. An empty directory counts as
/// owned — a skill deleted from the library leaves one behind.
pub fn is_akm_tree(dir: &Path, library_dir: &Path) -> bool {
    if dir.is_symlink() || !dir.is_dir() {
        return false;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };

    for entry in entries {
        let Ok(entry) = entry else {
            return false;
        };
        // read_link fails on anything that is not a symlink.
        let Ok(link_target) = std::fs::read_link(entry.path()) else {
            return false;
        };
        if !link_target.starts_with(library_dir) {
            return false;
        }
    }

    true
}

/// Remove a tree created by [`create_tree`].
///
/// Returns `Ok(false)` without touching anything if `dir` is not akm-owned
/// (see [`is_akm_tree`]).
pub fn remove_tree(dir: &Path, library_dir: &Path) -> Result<bool> {
    if !is_akm_tree(dir, library_dir) {
        return Ok(false);
    }

    std::fs::remove_dir_all(dir).io_context(format!("Removing akm tree {}", dir.display()))?;
    Ok(true)
}

/// Create session symlinks for a single spec in a staging directory.
///
/// `staging_names` are the per-tool directory names inside the staging tree
/// (see [`crate::library::tool_dirs::ToolDirs::staging_names`]), not the
/// absolute global tool dirs used by [`create_global`].
pub fn create_session(
    spec: &Spec,
    library_dir: &Path,
    staging_dir: &Path,
    staging_names: &[&str],
) -> Result<bool> {
    let source_path = spec.source_path(library_dir);

    if !source_path.exists() {
        return Ok(false);
    }

    for tool_name in staging_names {
        let subdir = spec.spec_type.subdir();
        let target_dir = staging_dir.join(tool_name).join(subdir);

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
/// Returns `Ok(true)` if any symlinks were found and removed.
///
/// `staging_names` matches [`create_session`].
pub fn remove_session(id: &str, staging_dir: &Path, staging_names: &[&str]) -> Result<bool> {
    let mut found = false;

    for tool_name in staging_names {
        let skill_link = staging_dir.join(tool_name).join("skills").join(id);
        if skill_link.is_symlink() {
            std::fs::remove_file(&skill_link)
                .io_context(format!("Removing session symlink {}", skill_link.display()))?;
            found = true;
        }

        let agent_link = staging_dir
            .join(tool_name)
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

/// Clear all akm-owned mounts from the mount targets.
///
/// Removes symlinks everywhere, plus — under a [`Mount::Tree`] target — the
/// real directories akm materialized (see [`is_akm_tree`]). Real files and the
/// user's own skill directories are left intact.
/// Returns the number of mounts removed.
pub fn clear_all(targets: &[MountTarget], library_dir: &Path) -> Result<usize> {
    let mut count = 0;

    for target in targets {
        for subdir in SPEC_SUBDIRS {
            let dir = target.dir.join(subdir);
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
                } else if target.mount == Mount::Tree
                    && path.is_dir()
                    && remove_tree(&path, library_dir)?
                {
                    count += 1;
                }
            }
        }
    }

    Ok(count)
}

/// Clean broken mounts from the mount targets.
///
/// A broken symlink is one where `is_symlink()` is true but `exists()` is
/// false. Under a [`Mount::Tree`] target an akm-owned tree is broken when it is
/// empty or its `SKILL.md` link dangles — the library skill is gone.
/// Returns the number of broken mounts removed.
pub fn clean_broken(targets: &[MountTarget], library_dir: &Path) -> Result<usize> {
    let mut count = 0;

    for target in targets {
        for subdir in SPEC_SUBDIRS {
            let dir = target.dir.join(subdir);
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
                } else if target.mount == Mount::Tree && !path.is_symlink() && path.is_dir() {
                    let skill_md = path.join("SKILL.md");
                    let broken = (skill_md.is_symlink() && !skill_md.exists()) || is_empty(&path);
                    if broken && remove_tree(&path, library_dir)? {
                        count += 1;
                    }
                }
            }
        }
    }

    Ok(count)
}

/// Whether `dir` has no entries. Unreadable directories count as non-empty.
fn is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none())
}

/// Rebuild global mounts for all core specs.
///
/// This is the high-level function called by the sync command.
/// It clears all existing mounts, cleans broken ones, then creates
/// fresh mounts for every core spec.
///
/// Returns the number of specs successfully mounted.
pub fn rebuild_core(
    core_specs: &[&Spec],
    library_dir: &Path,
    targets: &[MountTarget],
) -> Result<usize> {
    // Step 1: Clear all existing mounts
    clear_all(targets, library_dir)?;

    // Step 2: Clean any broken mounts
    clean_broken(targets, library_dir)?;

    // Step 3: Create mounts for each core spec
    let mut count = 0;
    for spec in core_specs {
        match create_global(spec, library_dir, targets) {
            Ok(true) => count += 1,
            Ok(false) => {
                // Source doesn't exist — skip silently
            }
            Err(e) => {
                // Log warning but continue
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
    use crate::library::tool_dirs::ToolDirs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn symlink_target(dir: &Path) -> MountTarget {
        MountTarget {
            dir: dir.to_path_buf(),
            mount: Mount::Symlink,
            agents: true,
        }
    }

    fn tree_target(dir: &Path) -> MountTarget {
        MountTarget {
            dir: dir.to_path_buf(),
            mount: Mount::Tree,
            agents: true,
        }
    }

    fn skills_only_target(dir: &Path) -> MountTarget {
        MountTarget {
            dir: dir.to_path_buf(),
            mount: Mount::Symlink,
            agents: false,
        }
    }

    #[test]
    fn create_global_skips_agents_when_target_has_none() {
        let tmp = TempDir::new().unwrap();
        let library_dir = tmp.path().join("library");
        let tool_dir = tmp.path().join(".vibe");
        create_skill_on_disk(&library_dir, "tdd");
        create_agent_on_disk(&library_dir, "reviewer");
        let targets = vec![skills_only_target(&tool_dir)];

        assert!(create_global(&make_skill_spec("tdd"), &library_dir, &targets).unwrap());
        assert!(create_global(&make_agent_spec("reviewer"), &library_dir, &targets).unwrap());

        assert!(tool_dir.join("skills").join("tdd").is_symlink());
        assert!(!tool_dir.join("agents").exists());
    }

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
        let targets = vec![symlink_target(&tool_dir)];

        let created = create_global(&spec, &lib_dir, &targets).unwrap();
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
        let targets = vec![symlink_target(&tool_dir)];

        let created = create_global(&spec, &lib_dir, &targets).unwrap();
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
        let targets = vec![symlink_target(&tool_dir)];

        let created = create_global(&spec, &lib_dir, &targets).unwrap();
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

        let removed = clear_all(&[symlink_target(&tool_dir)], tmp.path()).unwrap();

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

        let removed = clean_broken(&[symlink_target(&tool_dir)], tmp.path()).unwrap();

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
        let targets = vec![symlink_target(&tool_dir)];

        let count = rebuild_core(&core_specs, &lib_dir, &targets).unwrap();
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
        let targets = vec![symlink_target(&tool_dir)];

        let count1 = rebuild_core(&core_specs, &lib_dir, &targets).unwrap();
        let count2 = rebuild_core(&core_specs, &lib_dir, &targets).unwrap();
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
        let created = create_global(&spec, &lib_dir, &[symlink_target(&tool_dir)]).unwrap();
        assert!(created);

        let link = tool_dir.join("skills").join("tdd");
        assert!(link.is_symlink());
    }

    #[test]
    fn session_symlinks_create_and_remove() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let staging = tmp.path().join("session");
        let staging_names = [".claude", ".copilot"];

        create_skill_on_disk(&lib_dir, "tdd");
        let spec = make_skill_spec("tdd");

        let created = create_session(&spec, &lib_dir, &staging, &staging_names).unwrap();
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

        let removed = remove_session("tdd", &staging, &staging_names).unwrap();
        assert!(removed);
        assert!(!staging.join(".claude").join("skills").join("tdd").exists());
    }

    // =========================================================================
    // Tree mounts
    // =========================================================================

    /// A library skill with a nested `references/` dir.
    fn create_skill_with_references(library_dir: &Path, id: &str) -> PathBuf {
        create_skill_on_disk(library_dir, id);
        let skill_dir = library_dir.join("skills").join(id);
        std::fs::create_dir_all(skill_dir.join("references")).unwrap();
        std::fs::write(skill_dir.join("references").join("a.md"), "ref").unwrap();
        skill_dir
    }

    #[test]
    fn create_tree_makes_dir_of_links() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let source = create_skill_with_references(&lib_dir, "tdd");
        let target = tmp.path().join("home/.posit/assistant/skills/tdd");

        assert!(create_tree(&source, &target, &lib_dir).unwrap());

        assert!(target.is_dir());
        assert!(!target.is_symlink());
        assert!(target.join("SKILL.md").is_symlink());
        assert!(target.join("references").is_symlink());
        assert!(target.join("references").join("a.md").is_file());
        assert_eq!(
            std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
            std::fs::read_to_string(source.join("SKILL.md")).unwrap()
        );
    }

    #[test]
    fn create_tree_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let source = create_skill_with_references(&lib_dir, "tdd");
        let target = tmp.path().join("target");

        assert!(create_tree(&source, &target, &lib_dir).unwrap());
        assert!(create_tree(&source, &target, &lib_dir).unwrap());

        let mut names: Vec<_> = std::fs::read_dir(&target)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["SKILL.md", "references"]);
    }

    #[test]
    fn create_tree_removes_stale_entry() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let source = create_skill_with_references(&lib_dir, "tdd");
        let target = tmp.path().join("target");

        // A link left over from a previous version of the skill.
        std::fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink(source.join("SKILL.md"), target.join("old.md")).unwrap();

        assert!(create_tree(&source, &target, &lib_dir).unwrap());

        assert!(!target.join("old.md").is_symlink());
        assert!(target.join("SKILL.md").is_symlink());
    }

    #[test]
    fn create_tree_refuses_non_owned_dir() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let source = create_skill_with_references(&lib_dir, "tdd");
        let target = tmp.path().join("target");

        // A skill the user wrote by hand — real files, not ours.
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("SKILL.md"), "mine").unwrap();

        assert!(!create_tree(&source, &target, &lib_dir).unwrap());
        assert_eq!(
            std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "mine"
        );
    }

    #[test]
    fn create_tree_replaces_old_style_symlink() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let source = create_skill_with_references(&lib_dir, "tdd");
        let target = tmp.path().join("target");

        std::os::unix::fs::symlink(&source, &target).unwrap();

        assert!(create_tree(&source, &target, &lib_dir).unwrap());

        assert!(!target.is_symlink());
        assert!(target.is_dir());
        assert!(target.join("SKILL.md").is_symlink());
        // The library skill itself is untouched.
        assert!(source.join("SKILL.md").is_file());
    }

    #[test]
    fn is_akm_tree_cases() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let source = create_skill_with_references(&lib_dir, "tdd");

        let owned = tmp.path().join("owned");
        create_tree(&source, &owned, &lib_dir).unwrap();
        assert!(is_akm_tree(&owned, &lib_dir));

        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(is_akm_tree(&empty, &lib_dir));

        let with_file = tmp.path().join("with-file");
        std::fs::create_dir_all(&with_file).unwrap();
        std::fs::write(with_file.join("SKILL.md"), "mine").unwrap();
        assert!(!is_akm_tree(&with_file, &lib_dir));

        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let elsewhere = tmp.path().join("elsewhere.md");
        std::fs::write(&elsewhere, "not ours").unwrap();
        std::os::unix::fs::symlink(&elsewhere, outside.join("SKILL.md")).unwrap();
        assert!(!is_akm_tree(&outside, &lib_dir));

        assert!(!is_akm_tree(&tmp.path().join("missing"), &lib_dir));
    }

    #[test]
    fn clear_all_removes_owned_tree_and_keeps_user_skill_dir() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let tool_dir = tmp.path().join("home/.posit/assistant");
        let source = create_skill_with_references(&lib_dir, "tdd");

        let skills_dir = tool_dir.join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        create_tree(&source, &skills_dir.join("tdd"), &lib_dir).unwrap();

        // The user's own skill — a real dir with real files.
        let mine = skills_dir.join("mine");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::write(mine.join("SKILL.md"), "mine").unwrap();

        let removed = clear_all(&[tree_target(&tool_dir)], &lib_dir).unwrap();

        assert_eq!(removed, 1);
        assert!(!skills_dir.join("tdd").exists());
        assert!(mine.join("SKILL.md").is_file());
    }

    #[test]
    fn clean_broken_removes_tree_whose_source_vanished() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let tool_dir = tmp.path().join("home/.posit/assistant");
        let source = create_skill_with_references(&lib_dir, "tdd");

        let tree = tool_dir.join("skills").join("tdd");
        std::fs::create_dir_all(tool_dir.join("skills")).unwrap();
        create_tree(&source, &tree, &lib_dir).unwrap();

        // The skill is deleted from the library — every link now dangles.
        std::fs::remove_dir_all(&source).unwrap();

        let removed = clean_broken(&[tree_target(&tool_dir)], &lib_dir).unwrap();

        assert_eq!(removed, 1);
        assert!(!tree.exists());
    }

    #[test]
    fn create_global_tree_skips_agents() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let tool_dir = tmp.path().join("home/.posit/assistant");

        create_agent_on_disk(&lib_dir, "reviewer");
        let spec = make_agent_spec("reviewer");

        let created = create_global(&spec, &lib_dir, &[tree_target(&tool_dir)]).unwrap();

        assert!(created);
        assert!(!tool_dir.join("agents").exists());
    }

    #[test]
    fn rebuild_core_mixed_targets() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let claude = tmp.path().join("home/.claude");
        let posit = tmp.path().join("home/.posit/assistant");

        create_skill_with_references(&lib_dir, "core-skill");
        create_agent_on_disk(&lib_dir, "core-agent");

        let mut skill_spec = make_skill_spec("core-skill");
        skill_spec.core = true;
        let mut agent_spec = make_agent_spec("core-agent");
        agent_spec.core = true;
        let core_specs: Vec<&Spec> = vec![&skill_spec, &agent_spec];

        let targets = vec![symlink_target(&claude), tree_target(&posit)];
        let count = rebuild_core(&core_specs, &lib_dir, &targets).unwrap();
        assert_eq!(count, 2);

        assert!(claude.join("skills/core-skill").is_symlink());
        assert!(claude.join("agents/core-agent.md").is_symlink());

        let tree = posit.join("skills/core-skill");
        assert!(tree.is_dir() && !tree.is_symlink());
        assert!(tree.join("SKILL.md").is_symlink());
        assert!(!posit.join("agents").exists());
    }

    /// Pi's global dir is `~/.pi/agent`, but its staging dir is `.pi` — the
    /// staging tree is flat, so only the first path component is mirrored.
    #[test]
    fn session_symlinks_use_pi_staging_name() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("library");
        let staging = tmp.path().join("session");
        let tool_dirs = ToolDirs::builtin(&tmp.path().join("home"));

        create_skill_on_disk(&lib_dir, "tdd");
        let spec = make_skill_spec("tdd");

        create_session(&spec, &lib_dir, &staging, &tool_dirs.staging_names()).unwrap();

        assert!(staging.join(".pi").join("skills").join("tdd").is_symlink());
        assert!(!staging.join("agent").exists());
    }
}
