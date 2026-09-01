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
}
