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
            allow: pats(&["theme.json", "settings.json", "config.json", "prompts/", "APPEND_SYSTEM.md"]),
            exclude: pats(&["auth.json", "sessions/", "models-store.json", "*.log"]),
        },
        HarnessConfigDef {
            command: "claude".into(),
            // Only settings.json for v1 — the safe, well-understood file.
            allow: pats(&["settings.json"]),
            exclude: pats(&[
                ".credentials.json", "projects/", "todos/", "statsig/", "history.jsonl", "*.log",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_parse_and_match() {
        assert_eq!(Pattern::parse("prompts/"), Pattern::Prefix("prompts/".into()));
        assert_eq!(Pattern::parse("*.json"), Pattern::Suffix(".json".into()));
        assert_eq!(Pattern::parse("theme.json"), Pattern::Exact("theme.json".into()));

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
        assert!(looks_like_secret("-----BEGIN OPENSSH PRIVATE KEY-----\nabc"));
        assert!(looks_like_secret("token = \"ghp_0123456789abcdefABCDEF0123456789abcd\""));
        assert!(looks_like_secret("key: sk-ant-api03-XXXXXXXXXXXXXXXXXXXX"));
        assert!(!looks_like_secret("{ \"theme\": \"gold\", \"fontSize\": 14 }"));
    }
}
