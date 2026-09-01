//! Harness config sync — which files under a harness's global dir travel with
//! the personal registry, and how they are captured and applied.
//!
//! Layering: pure filesystem. This module never runs git; the command layer
//! hands the resulting pathspec to `Registry`.

use crate::error::{IoContext, Result};
use std::path::{Path, PathBuf};

/// A match pattern for a config file, relative to the harness global dir.
///
/// Three shapes, chosen so no glob crate is needed:
/// * `Prefix("prompts/")` — the whole subtree under a directory.
/// * `Suffix(".json")` — any file ending in an extension (from `*.json`).
/// * `Exact("theme.json")` — one specific relative path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    Prefix(String),
    Suffix(String),
    Exact(String),
}

impl Pattern {
    /// Parse a pattern string: trailing `/` → prefix, leading `*` → suffix,
    /// otherwise exact.
    pub fn parse(s: &str) -> Self {
        if let Some(dir) = s.strip_suffix('/') {
            Pattern::Prefix(format!("{dir}/"))
        } else if let Some(suffix) = s.strip_prefix('*') {
            Pattern::Suffix(suffix.to_string())
        } else {
            Pattern::Exact(s.to_string())
        }
    }

    /// Whether `rel` (forward-slash relative path) matches this pattern.
    pub fn matches(&self, rel: &str) -> bool {
        match self {
            Pattern::Prefix(dir) => rel.starts_with(dir.as_str()),
            Pattern::Suffix(suffix) => rel.ends_with(suffix.as_str()),
            Pattern::Exact(path) => rel == path,
        }
    }
}

/// How a file under a harness dir is treated by capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileClass {
    /// Matches the allowlist and no exclude — captured.
    Allowed,
    /// Matches a secret/state exclude — never captured, never offered.
    Excluded,
    /// Matches neither — offered to the user as an opt-in.
    Unrecognized,
}

/// Curated sync rules for one harness, keyed by CLI `command`.
#[derive(Debug, Clone)]
pub struct HarnessConfigDef {
    /// CLI command (matches `ToolDef::command`): "pi", "claude", "opencode".
    pub command: String,
    /// Shareable file patterns (curated defaults; user opt-ins extend this).
    pub allow: Vec<Pattern>,
    /// Secret/state patterns that always win over `allow`.
    pub exclude: Vec<Pattern>,
}

impl HarnessConfigDef {
    /// Classify one relative path. Exclude always wins over allow.
    pub fn classify(&self, rel: &str) -> FileClass {
        if self.exclude.iter().any(|p| p.matches(rel)) {
            FileClass::Excluded
        } else if self.allow.iter().any(|p| p.matches(rel)) {
            FileClass::Allowed
        } else {
            FileClass::Unrecognized
        }
    }

    /// Classify with the user's extra allow patterns folded in (from config).
    pub fn classify_with(&self, rel: &str, extra_allow: &[Pattern]) -> FileClass {
        if self.exclude.iter().any(|p| p.matches(rel)) {
            FileClass::Excluded
        } else if self.allow.iter().chain(extra_allow).any(|p| p.matches(rel)) {
            FileClass::Allowed
        } else {
            FileClass::Unrecognized
        }
    }
}

fn pats(items: &[&str]) -> Vec<Pattern> {
    items.iter().map(|s| Pattern::parse(s)).collect()
}

/// Built-in per-harness sync rules. Order is Pi, Claude, OpenCode.
///
/// NOTE: `allow` lists are conservative first cuts; widen them (or let users
/// opt in) as real configs are seen. `exclude` lists are the secret/state
/// boundary and must stay strict.
pub fn builtin_harness_configs() -> Vec<HarnessConfigDef> {
    vec![
        HarnessConfigDef {
            command: "pi".into(),
            allow: pats(&[
                "theme.json",
                "settings.json",
                "config.json",
                "prompts/",
                "APPEND_SYSTEM.md",
            ]),
            exclude: pats(&["auth.json", "sessions/", "models-store.json", "*.log"]),
        },
        HarnessConfigDef {
            command: "claude".into(),
            // Only settings.json for v1 — the safe, well-understood file.
            allow: pats(&["settings.json"]),
            exclude: pats(&[
                ".credentials.json",
                "projects/",
                "todos/",
                "statsig/",
                "history.jsonl",
                "*.log",
            ]),
        },
        HarnessConfigDef {
            command: "opencode".into(),
            allow: pats(&["opencode.json", "config.json", "themes/"]),
            exclude: pats(&["auth.json", "sessions/", "*.log"]),
        },
    ]
}

/// Look up the built-in def for a command.
pub fn builtin_for(command: &str) -> Option<HarnessConfigDef> {
    builtin_harness_configs()
        .into_iter()
        .find(|d| d.command == command)
}

/// Heuristic: does this file content look like it carries a secret?
///
/// Deliberately simple — a warning backstop, not a scanner. False positives are
/// acceptable (the user confirms); the hard `exclude` list is the real guard.
pub fn looks_like_secret(content: &str) -> bool {
    const MARKERS: &[&str] = &[
        "-----BEGIN",
        "PRIVATE KEY",
        "sk-",
        "ghp_",
        "gho_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "AKIA",
        "\"access_token\"",
        "\"refresh_token\"",
        "\"api_key\"",
    ];
    MARKERS.iter().any(|m| content.contains(m))
}

/// The result of classifying a harness global dir. Paths are relative,
/// forward-slash, sorted.
#[derive(Debug, Clone, Default)]
pub struct DirScan {
    pub allowed: Vec<String>,
    pub unrecognized: Vec<String>,
    pub excluded: Vec<String>,
}

/// Recursively list files under `dir` as forward-slash relative paths.
fn walk_rel(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        let entries = std::fs::read_dir(&cur).io_context(format!("Reading {}", cur.display()))?;
        for entry in entries {
            let entry = entry.io_context(format!("Reading entry in {}", cur.display()))?;
            let path = entry.path();
            let ty = entry
                .file_type()
                .io_context(format!("Stat {}", path.display()))?;
            if ty.is_dir() {
                stack.push(path);
            } else if ty.is_file() {
                if let Ok(rel) = path.strip_prefix(dir) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
            // symlinks and other types are ignored — we only sync real files
        }
    }
    out.sort();
    Ok(out)
}

/// Classify every file under a harness global dir.
///
/// `extra_allow` are the user's opt-in patterns from config for this harness.
/// A missing directory is not an error — it yields an empty scan.
pub fn classify_dir(
    dir: &Path,
    def: &HarnessConfigDef,
    extra_allow: &[Pattern],
) -> Result<DirScan> {
    if !dir.is_dir() {
        return Ok(DirScan::default());
    }
    let mut scan = DirScan::default();
    for rel in walk_rel(dir)? {
        match def.classify_with(&rel, extra_allow) {
            FileClass::Allowed => scan.allowed.push(rel),
            FileClass::Unrecognized => scan.unrecognized.push(rel),
            FileClass::Excluded => scan.excluded.push(rel),
        }
    }
    Ok(scan)
}

/// Copy the given relative paths from `live` into the registry `tree`, and
/// remove any file already in `tree` that is not in `include` (so deletions
/// travel). Returns the sorted list of paths now present in the tree.
pub fn capture_files(live: &Path, tree: &Path, include: &[String]) -> Result<Vec<String>> {
    let wanted: std::collections::BTreeSet<&String> = include.iter().collect();

    // 1. Remove tree files no longer wanted.
    if tree.is_dir() {
        for rel in walk_rel(tree)? {
            if !wanted.contains(&rel) {
                let path = tree.join(&rel);
                std::fs::remove_file(&path).io_context(format!("Removing {}", path.display()))?;
            }
        }
        prune_empty_dirs(tree)?;
    }

    // 2. Copy wanted files in.
    let mut done = Vec::new();
    for rel in include {
        let src = live.join(rel);
        if !src.is_file() {
            continue;
        }
        let dst = tree.join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).io_context(format!("Creating {}", parent.display()))?;
        }
        std::fs::copy(&src, &dst).io_context(format!(
            "Copying {} -> {}",
            src.display(),
            dst.display()
        ))?;
        done.push(rel.clone());
    }
    done.sort();
    Ok(done)
}

/// Copy every file in `tree` over the live dir (add/overwrite only). Never
/// deletes live files. Returns the sorted list applied.
pub fn apply_files(tree: &Path, live: &Path) -> Result<Vec<String>> {
    if !tree.is_dir() {
        return Ok(Vec::new());
    }
    let mut done = Vec::new();
    for rel in walk_rel(tree)? {
        let src = tree.join(&rel);
        let dst = live.join(&rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).io_context(format!("Creating {}", parent.display()))?;
        }
        std::fs::copy(&src, &dst).io_context(format!(
            "Copying {} -> {}",
            src.display(),
            dst.display()
        ))?;
        done.push(rel);
    }
    done.sort();
    Ok(done)
}

/// A preview of what [`capture_files`] would change in the registry tree,
/// relative to what the tree already holds. Paths are relative, sorted.
#[derive(Debug, Clone, Default)]
pub struct CapturePlan {
    /// Not yet in the tree — would be added.
    pub new: Vec<String>,
    /// In the tree but with different content — would be overwritten.
    pub changed: Vec<String>,
    /// Already match the tree — capture is a no-op for these.
    pub unchanged: Vec<String>,
    /// In the tree, no longer wanted — deletion would propagate.
    pub removed: Vec<String>,
}

impl CapturePlan {
    /// Whether a push would change the registry at all. `unchanged` does not
    /// count — a plan of only unchanged files is nothing to push.
    pub fn is_empty(&self) -> bool {
        self.new.is_empty() && self.changed.is_empty() && self.removed.is_empty()
    }
}

/// Compare what [`capture_files`]`(live, tree, include)` would write against
/// what `tree` already holds, without touching either. Pure filesystem, so it
/// backs `push --dry-run` and the pre-flight header of a real push.
pub fn plan_capture(live: &Path, tree: &Path, include: &[String]) -> Result<CapturePlan> {
    let wanted: std::collections::BTreeSet<&String> = include.iter().collect();
    let mut plan = CapturePlan::default();

    for rel in include {
        let src = live.join(rel);
        if !src.is_file() {
            continue; // capture skips missing sources too
        }
        let dst = tree.join(rel);
        if !dst.is_file() {
            plan.new.push(rel.clone());
            continue;
        }
        let incoming = std::fs::read(&src).io_context(format!("Reading {}", src.display()))?;
        let existing = std::fs::read(&dst).io_context(format!("Reading {}", dst.display()))?;
        if incoming == existing {
            plan.unchanged.push(rel.clone());
        } else {
            plan.changed.push(rel.clone());
        }
    }

    if tree.is_dir() {
        for rel in walk_rel(tree)? {
            if !wanted.contains(&rel) {
                plan.removed.push(rel);
            }
        }
    }

    plan.new.sort();
    plan.changed.sort();
    plan.unchanged.sort();
    plan.removed.sort();
    Ok(plan)
}

/// Remove now-empty directories under `root` (but keep `root` itself).
fn prune_empty_dirs(root: &Path) -> Result<()> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(cur) = stack.pop() {
        for entry in std::fs::read_dir(&cur).io_context(format!("Reading {}", cur.display()))? {
            let entry = entry.io_context(format!("Reading entry in {}", cur.display()))?;
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let p = entry.path();
                dirs.push(p.clone());
                stack.push(p);
            }
        }
    }
    // Deepest first.
    dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for d in dirs {
        // Ignore "directory not empty"; only prune the ones that are empty.
        let _ = std::fs::remove_dir(&d);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_parse_and_match() {
        assert_eq!(
            Pattern::parse("prompts/"),
            Pattern::Prefix("prompts/".into())
        );
        assert_eq!(Pattern::parse("*.json"), Pattern::Suffix(".json".into()));
        assert_eq!(
            Pattern::parse("theme.json"),
            Pattern::Exact("theme.json".into())
        );

        assert!(Pattern::parse("prompts/").matches("prompts/a/b.md"));
        assert!(!Pattern::parse("prompts/").matches("prompt.md"));
        assert!(Pattern::parse("*.json").matches("theme.json"));
        assert!(!Pattern::parse("*.json").matches("theme.toml"));
        assert!(Pattern::parse("theme.json").matches("theme.json"));
        assert!(!Pattern::parse("theme.json").matches("themes/theme.json"));
    }

    #[test]
    fn builtin_covers_pi_claude_opencode() {
        let defs = builtin_harness_configs();
        let commands: Vec<&str> = defs.iter().map(|d| d.command.as_str()).collect();
        assert_eq!(commands, vec!["pi", "claude", "opencode"]);
    }

    #[test]
    fn pi_excludes_auth_even_if_it_matched_allow() {
        let pi = builtin_harness_configs()
            .into_iter()
            .find(|d| d.command == "pi")
            .unwrap();
        // exclude wins over allow
        assert!(pi.classify("auth.json") == FileClass::Excluded);
        assert!(pi.classify("sessions/2026/x.json") == FileClass::Excluded);
        assert!(pi.classify("theme.json") == FileClass::Allowed);
        assert!(pi.classify("random-note.md") == FileClass::Unrecognized);
    }

    #[test]
    fn secret_scan_flags_obvious_credentials() {
        assert!(looks_like_secret(
            "-----BEGIN OPENSSH PRIVATE KEY-----\nabc"
        ));
        assert!(looks_like_secret(
            "token = \"ghp_0123456789abcdefABCDEF0123456789abcd\""
        ));
        assert!(looks_like_secret("key: sk-ant-api03-XXXXXXXXXXXXXXXXXXXX"));
        assert!(!looks_like_secret(
            "{ \"theme\": \"gold\", \"fontSize\": 14 }"
        ));
    }

    #[test]
    fn classify_dir_buckets_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("theme.json"), "{}").unwrap();
        std::fs::write(dir.join("auth.json"), "secret").unwrap();
        std::fs::write(dir.join("notes.md"), "hi").unwrap();
        std::fs::create_dir_all(dir.join("sessions")).unwrap();
        std::fs::write(dir.join("sessions/s1.json"), "x").unwrap();

        let def = builtin_for("pi").unwrap();
        let scan = classify_dir(dir, &def, &[]).unwrap();

        assert_eq!(scan.allowed, vec!["theme.json".to_string()]);
        assert_eq!(scan.unrecognized, vec!["notes.md".to_string()]);
        // auth.json and sessions/ are excluded and not surfaced
        assert!(!scan.excluded.is_empty());
    }

    #[test]
    fn capture_then_apply_round_trips() {
        let tmp = tempfile::TempDir::new().unwrap();
        let live = tmp.path().join("live");
        let tree = tmp.path().join("registry/harnesses/pi");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::write(live.join("theme.json"), "gold").unwrap();
        std::fs::write(live.join("auth.json"), "SECRET").unwrap();

        // capture: only theme.json lands in the tree; auth.json never does
        let captured = capture_files(&live, &tree, &["theme.json".to_string()]).unwrap();
        assert_eq!(captured, vec!["theme.json".to_string()]);
        assert_eq!(
            std::fs::read_to_string(tree.join("theme.json")).unwrap(),
            "gold"
        );
        assert!(!tree.join("auth.json").exists());

        // apply into a fresh live dir
        let live2 = tmp.path().join("live2");
        std::fs::create_dir_all(&live2).unwrap();
        std::fs::write(live2.join("machine-local.json"), "keep me").unwrap();
        let applied = apply_files(&tree, &live2).unwrap();
        assert_eq!(applied, vec!["theme.json".to_string()]);
        assert_eq!(
            std::fs::read_to_string(live2.join("theme.json")).unwrap(),
            "gold"
        );
        // apply never deletes machine-local files
        assert!(live2.join("machine-local.json").exists());
    }

    #[test]
    fn capture_propagates_deletions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let live = tmp.path().join("live");
        let tree = tmp.path().join("tree");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(tree.join("stale.json"), "old").unwrap(); // in tree, not live
        std::fs::write(live.join("theme.json"), "new").unwrap();

        capture_files(&live, &tree, &["theme.json".to_string()]).unwrap();
        assert!(!tree.join("stale.json").exists()); // removed to mirror live
        assert!(tree.join("theme.json").exists());
    }

    #[test]
    fn plan_capture_classifies_against_the_tree() {
        let tmp = tempfile::TempDir::new().unwrap();
        let live = tmp.path().join("live");
        let tree = tmp.path().join("tree");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(live.join("theme.json"), "new").unwrap(); // changed
        std::fs::write(live.join("settings.json"), "same").unwrap(); // unchanged
        std::fs::write(live.join("prompts.md"), "brand").unwrap(); // new
        std::fs::write(tree.join("theme.json"), "old").unwrap();
        std::fs::write(tree.join("settings.json"), "same").unwrap();
        std::fs::write(tree.join("gone.json"), "x").unwrap(); // removed

        let include = vec![
            "theme.json".to_string(),
            "settings.json".to_string(),
            "prompts.md".to_string(),
        ];
        let plan = plan_capture(&live, &tree, &include).unwrap();

        assert_eq!(plan.new, vec!["prompts.md".to_string()]);
        assert_eq!(plan.changed, vec!["theme.json".to_string()]);
        assert_eq!(plan.unchanged, vec!["settings.json".to_string()]);
        assert_eq!(plan.removed, vec!["gone.json".to_string()]);
        assert!(!plan.is_empty());
    }

    #[test]
    fn plan_capture_is_empty_when_only_unchanged() {
        let tmp = tempfile::TempDir::new().unwrap();
        let live = tmp.path().join("live");
        let tree = tmp.path().join("tree");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(live.join("theme.json"), "gold").unwrap();
        std::fs::write(tree.join("theme.json"), "gold").unwrap();

        let plan = plan_capture(&live, &tree, &["theme.json".to_string()]).unwrap();
        assert!(plan.is_empty()); // unchanged does not count as a change
        assert_eq!(plan.unchanged, vec!["theme.json".to_string()]);
    }
}
