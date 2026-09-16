//! Project sidecar materialization for harnesses with no session lifecycle.
//!
//! Most harnesses mount project skills into an ephemeral per-session staging
//! directory (see [`crate::library::symlinks::create_session`]). Posit
//! Assistant runs inside the Positron IDE with no session lifecycle to hook,
//! so its project skills instead live persistently on disk, at the harness's
//! declared [`crate::library::tool_dirs::ToolDef::project_dir`], as tree
//! mounts — a real directory whose entries are symlinks into the library
//! skill, because Posit's skill discovery skips symlinked directories.
//!
//! The sidecar is kept invisible to git: `skills/.gitignore` containing `*`
//! ignores everything under `skills/`, including itself, so only
//! `.agents/akm.json` is ever tracked. Anything the user placed under the
//! harness's project dir by hand — a `settings.json`, a hand-authored skill —
//! is left alone.

use crate::error::{IoContext, Result};
use crate::library::manifest::Manifest;
use crate::library::symlinks;
use crate::library::tool_dirs::ToolDef;
use std::path::Path;

/// Counts returned by [`refresh`] for the callers' messages.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SidecarReport {
    /// Number of skill trees created or refreshed.
    pub mounted: usize,
    /// Number of akm-owned trees or old-style symlinks removed.
    pub removed: usize,
}

/// Materialize the manifest's skills for every tool with a `project_dir`.
///
/// Layout for Posit: `<root>/.posit/assistant/skills/<id>/` (tree mounts) plus
/// `<root>/.posit/assistant/skills/.gitignore` containing `*` so the whole
/// sidecar is invisible to git and nothing outside it is touched.
///
/// Reconciles: removes akm-owned trees not in the manifest, (re)creates the
/// ones that are, leaves non-owned entries alone. When nothing is left to
/// mount and the dir holds only our `.gitignore`, removes `skills/`, then
/// `rmdir`s the project_dir and each of its parents up to (not including)
/// `project_root` while empty (never `remove_dir_all` on those).
///
/// Manifest agents are ignored. `akm skills clean --project` does not look
/// here (it inspects `<root>/<last component of the global dir>/`).
pub fn refresh(
    project_root: &Path,
    manifest: &Manifest,
    library_dir: &Path,
    tools: &[ToolDef],
) -> Result<SidecarReport> {
    let mut report = SidecarReport::default();

    for tool in tools {
        let Some(project_dir) = &tool.project_dir else {
            continue;
        };

        refresh_tool(
            project_root,
            project_dir,
            manifest,
            library_dir,
            &mut report,
        )?;
    }

    Ok(report)
}

/// Refresh a single tool's sidecar. See [`refresh`].
fn refresh_tool(
    project_root: &Path,
    project_dir: &str,
    manifest: &Manifest,
    library_dir: &Path,
    report: &mut SidecarReport,
) -> Result<()> {
    let tool_dir = project_root.join(project_dir);
    let skills_dir = tool_dir.join("skills");
    let library_skills_dir = library_dir.join("skills");

    let wanted: Vec<&str> = manifest
        .skill_ids()
        .iter()
        .map(String::as_str)
        .filter(|id| is_valid_id(id) && library_skills_dir.join(id).exists())
        .collect();

    if wanted.is_empty() && !skills_dir.exists() {
        return Ok(());
    }

    std::fs::create_dir_all(&skills_dir)
        .io_context(format!("Creating directory {}", skills_dir.display()))?;

    let gitignore_path = skills_dir.join(".gitignore");
    let current_gitignore = std::fs::read_to_string(&gitignore_path).unwrap_or_default();
    if current_gitignore != "*\n" {
        std::fs::write(&gitignore_path, "*\n")
            .io_context(format!("Writing {}", gitignore_path.display()))?;
    }

    // Reconcile: drop akm-owned trees and old-style symlinks no longer wanted.
    let entries = std::fs::read_dir(&skills_dir)
        .io_context(format!("Reading directory {}", skills_dir.display()))?;
    for entry in entries {
        let entry = entry.io_context(format!(
            "Reading directory entry in {}",
            skills_dir.display()
        ))?;
        let name = entry.file_name();
        if wanted.iter().any(|id| name == std::ffi::OsStr::new(id)) {
            continue;
        }

        let path = entry.path();
        if path.is_symlink() {
            std::fs::remove_file(&path)
                .io_context(format!("Removing symlink {}", path.display()))?;
            report.removed += 1;
        } else if path.is_dir() && symlinks::remove_tree(&path, library_dir)? {
            report.removed += 1;
        }
    }

    // Mount everything wanted.
    for id in &wanted {
        let source = library_skills_dir.join(id);
        let target = skills_dir.join(id);
        if symlinks::create_tree(&source, &target, library_dir)? {
            report.mounted += 1;
        } else {
            eprintln!(
                "Warning: {} exists and is not managed by akm — left in place",
                target.display()
            );
        }
    }

    prune(project_root, &tool_dir, &skills_dir)?;

    Ok(())
}

/// Remove `skills_dir` when it holds only our `.gitignore` (or nothing), then
/// `rmdir` `tool_dir` and each ancestor up to (not including) `project_root`
/// while empty.
fn prune(project_root: &Path, tool_dir: &Path, skills_dir: &Path) -> Result<()> {
    if !holds_only_gitignore(skills_dir) {
        return Ok(());
    }

    let gitignore_path = skills_dir.join(".gitignore");
    if gitignore_path.exists() {
        std::fs::remove_file(&gitignore_path)
            .io_context(format!("Removing {}", gitignore_path.display()))?;
    }
    remove_dir_ignore_missing(skills_dir)?;

    let mut dir = tool_dir.to_path_buf();
    while dir != project_root && dir.starts_with(project_root) {
        match std::fs::remove_dir(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => break, // Not empty — stop pruning upward.
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => break,
        }
    }

    Ok(())
}

/// Whether `dir`'s only entry (if any) is `.gitignore`.
fn holds_only_gitignore(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(std::result::Result::ok)
            .all(|e| e.file_name() == ".gitignore"),
        Err(_) => false,
    }
}

/// `remove_dir`, treating a missing directory as success.
fn remove_dir_ignore_missing(dir: &Path) -> Result<()> {
    match std::fs::remove_dir(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).io_context(format!("Removing directory {}", dir.display())),
    }
}

/// Defensive check — manifest IDs are validated upstream, but a sidecar path
/// is built directly from the ID, so reject anything path-shaped just in case.
fn is_valid_id(id: &str) -> bool {
    !id.contains('/') && !id.contains("..")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::manifest::Manifest;
    use crate::library::spec::SpecType;
    use crate::library::tool_dirs::Mount;
    use tempfile::TempDir;

    fn posit_tool() -> ToolDef {
        ToolDef {
            name: "Posit Assistant".into(),
            command: "pa".into(),
            dir: ".posit/assistant".into(),
            mount: Mount::Tree,
            agents: true,
            project_dir: Some(".posit/assistant".into()),
        }
    }

    fn claude_tool() -> ToolDef {
        ToolDef {
            name: "Claude Code".into(),
            command: "claude".into(),
            dir: ".claude".into(),
            mount: Mount::Symlink,
            agents: true,
            project_dir: None,
        }
    }

    fn create_library_skill(library_dir: &Path, id: &str) {
        let skill_dir = library_dir.join("skills").join(id);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {id}\ndescription: test\n---\nContent"),
        )
        .unwrap();
    }

    fn manifest_with(root: &Path, ids: &[&str]) -> Manifest {
        let mut manifest = Manifest::load_or_create(root).unwrap();
        for id in ids {
            manifest.add(id, SpecType::Skill);
        }
        manifest
    }

    #[test]
    fn fresh_project_gets_trees_and_gitignore() {
        let tmp = TempDir::new().unwrap();
        let library_dir = tmp.path().join("library");
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        create_library_skill(&library_dir, "tdd");
        create_library_skill(&library_dir, "reviewer");
        let manifest = manifest_with(&project_root, &["tdd", "reviewer"]);

        let report = refresh(
            &project_root,
            &manifest,
            &library_dir,
            &[posit_tool(), claude_tool()],
        )
        .unwrap();

        assert_eq!(report.mounted, 2);
        assert_eq!(report.removed, 0);

        let skills_dir = project_root.join(".posit/assistant/skills");
        for id in ["tdd", "reviewer"] {
            let tree = skills_dir.join(id);
            assert!(tree.is_dir() && !tree.is_symlink());
            assert!(tree.join("SKILL.md").is_symlink());
        }
        assert_eq!(
            std::fs::read_to_string(skills_dir.join(".gitignore")).unwrap(),
            "*\n"
        );
    }

    #[test]
    fn removing_from_manifest_removes_tree() {
        let tmp = TempDir::new().unwrap();
        let library_dir = tmp.path().join("library");
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        create_library_skill(&library_dir, "tdd");
        create_library_skill(&library_dir, "reviewer");
        let manifest = manifest_with(&project_root, &["tdd", "reviewer"]);
        refresh(&project_root, &manifest, &library_dir, &[posit_tool()]).unwrap();

        let manifest = manifest_with(&project_root, &["tdd"]);
        let report = refresh(&project_root, &manifest, &library_dir, &[posit_tool()]).unwrap();

        assert_eq!(report.removed, 1);
        let skills_dir = project_root.join(".posit/assistant/skills");
        assert!(skills_dir.join("tdd").exists());
        assert!(!skills_dir.join("reviewer").exists());
    }

    #[test]
    fn empty_manifest_removes_sidecar_and_empty_parents() {
        let tmp = TempDir::new().unwrap();
        let library_dir = tmp.path().join("library");
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        create_library_skill(&library_dir, "tdd");
        let manifest = manifest_with(&project_root, &["tdd"]);
        refresh(&project_root, &manifest, &library_dir, &[posit_tool()]).unwrap();

        let manifest = manifest_with(&project_root, &[]);
        let report = refresh(&project_root, &manifest, &library_dir, &[posit_tool()]).unwrap();

        assert_eq!(report.removed, 1);
        assert!(!project_root.join(".posit/assistant/skills").exists());
        assert!(!project_root.join(".posit/assistant").exists());
        assert!(!project_root.join(".posit").exists());
        assert!(project_root.exists());
    }

    #[test]
    fn nonempty_project_dir_is_kept() {
        let tmp = TempDir::new().unwrap();
        let library_dir = tmp.path().join("library");
        let project_root = tmp.path().join("project");
        let assistant_dir = project_root.join(".posit/assistant");
        std::fs::create_dir_all(&assistant_dir).unwrap();
        std::fs::write(assistant_dir.join("settings.json"), "{}").unwrap();

        create_library_skill(&library_dir, "tdd");
        let manifest = manifest_with(&project_root, &["tdd"]);
        refresh(&project_root, &manifest, &library_dir, &[posit_tool()]).unwrap();

        let manifest = manifest_with(&project_root, &[]);
        refresh(&project_root, &manifest, &library_dir, &[posit_tool()]).unwrap();

        assert!(!assistant_dir.join("skills").exists());
        assert!(assistant_dir.join("settings.json").is_file());
        assert!(assistant_dir.exists());
    }

    #[test]
    fn foreign_real_dir_in_sidecar_is_untouched() {
        let tmp = TempDir::new().unwrap();
        let library_dir = tmp.path().join("library");
        let project_root = tmp.path().join("project");
        let skills_dir = project_root.join(".posit/assistant/skills");
        std::fs::create_dir_all(skills_dir.join("mine")).unwrap();
        std::fs::write(skills_dir.join("mine").join("SKILL.md"), "mine").unwrap();

        let manifest = manifest_with(&project_root, &[]);
        refresh(&project_root, &manifest, &library_dir, &[posit_tool()]).unwrap();

        assert!(skills_dir.join("mine").join("SKILL.md").is_file());
        assert!(skills_dir.exists());
    }

    #[test]
    fn tool_without_project_dir_is_ignored() {
        let tmp = TempDir::new().unwrap();
        let library_dir = tmp.path().join("library");
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        create_library_skill(&library_dir, "tdd");
        let manifest = manifest_with(&project_root, &["tdd"]);

        let report = refresh(&project_root, &manifest, &library_dir, &[claude_tool()]).unwrap();

        assert_eq!(report.mounted, 0);
        assert!(!project_root.join(".claude").exists());
        assert!(!project_root.join(".posit").exists());
    }

    #[test]
    fn missing_library_skill_is_skipped() {
        let tmp = TempDir::new().unwrap();
        let library_dir = tmp.path().join("library");
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        let manifest = manifest_with(&project_root, &["ghost"]);
        let report = refresh(&project_root, &manifest, &library_dir, &[posit_tool()]).unwrap();

        assert_eq!(report.mounted, 0);
        assert!(!project_root.join(".posit").exists());
    }

    #[test]
    fn refresh_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let library_dir = tmp.path().join("library");
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        create_library_skill(&library_dir, "tdd");
        create_library_skill(&library_dir, "reviewer");
        let manifest = manifest_with(&project_root, &["tdd", "reviewer"]);

        let report1 = refresh(&project_root, &manifest, &library_dir, &[posit_tool()]).unwrap();
        let report2 = refresh(&project_root, &manifest, &library_dir, &[posit_tool()]).unwrap();

        assert_eq!(report1.mounted, 2);
        assert_eq!(report2.mounted, 2);
        assert_eq!(report2.removed, 0);
    }
}
