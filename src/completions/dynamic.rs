//! Dynamic completers for skill/agent IDs.
//!
//! These read `library.json` at Tab-completion time to provide real-time
//! suggestions. If the library doesn't exist or is malformed, completions
//! gracefully return an empty list.
//!
//! The `ValueCandidates` trait requires a zero-arg `candidates()` method,
//! so the public types use `Paths::resolve()` internally. The actual logic
//! is in `candidates_with_paths()` methods that accept injected `Paths` for testing.

use clap_complete::engine::{CompletionCandidate, ValueCandidates};

use crate::library::spec::Spec;
use crate::library::Library;
use crate::paths::Paths;

/// Shared helper: load library and map matching specs to completion candidates.
///
/// Accepts a filter predicate for spec selection. Returns empty vec on any error.
fn candidates_from_library(paths: &Paths, filter: fn(&Spec) -> bool) -> Vec<CompletionCandidate> {
    let library = match Library::load(paths) {
        Ok(lib) => lib,
        Err(_) => return Vec::new(),
    };

    library
        .specs
        .iter()
        .filter(|spec| filter(spec))
        .map(spec_to_candidate)
        .collect()
}

/// Convert a Spec to a CompletionCandidate.
///
/// Uses the description as help text if non-empty.
fn spec_to_candidate(spec: &Spec) -> CompletionCandidate {
    let mut candidate = CompletionCandidate::new(spec.id.clone());
    if !spec.description.is_empty() {
        candidate = candidate.help(Some(spec.description.clone().into()));
    }
    candidate
}

/// Completer that suggests all spec IDs (skills + agents) from the library.
///
/// Used for arguments that accept any spec ID:
/// - `akm skills add <id>...`
/// - `akm skills remove <id>...`
/// - `akm skills load <id>...`
/// - `akm skills unload <id>...`
/// - `akm skills edit <id>`
/// - `akm skills publish <id>`
#[derive(Clone, Debug)]
pub struct SpecIdCompleter;

impl SpecIdCompleter {
    /// Testable version that accepts explicit Paths.
    pub fn candidates_with_paths(paths: &Paths) -> Vec<CompletionCandidate> {
        candidates_from_library(paths, |_| true)
    }
}

impl ValueCandidates for SpecIdCompleter {
    fn candidates(&self) -> Vec<CompletionCandidate> {
        match Paths::resolve() {
            Some(paths) => Self::candidates_with_paths(&paths),
            None => Vec::new(),
        }
    }
}

/// Completer that suggests only skill IDs from the library.
///
/// Available for future use when commands need type-specific completion
/// (e.g., a hypothetical `akm skills install-skill <Tab>` vs `akm skills install-agent <Tab>`).
/// Currently all spec-accepting arguments use `SpecIdCompleter` for simplicity.
#[derive(Clone, Debug)]
pub struct SkillIdCompleter;

impl SkillIdCompleter {
    /// Testable version that accepts explicit Paths.
    pub fn candidates_with_paths(paths: &Paths) -> Vec<CompletionCandidate> {
        use crate::library::spec::SpecType;
        candidates_from_library(paths, |s| s.spec_type == SpecType::Skill)
    }
}

impl ValueCandidates for SkillIdCompleter {
    fn candidates(&self) -> Vec<CompletionCandidate> {
        match Paths::resolve() {
            Some(paths) => Self::candidates_with_paths(&paths),
            None => Vec::new(),
        }
    }
}

/// Completer that suggests only agent IDs from the library.
///
/// Available for future use when commands need type-specific completion.
/// See `SkillIdCompleter` doc for rationale.
#[derive(Clone, Debug)]
pub struct AgentIdCompleter;

impl AgentIdCompleter {
    /// Testable version that accepts explicit Paths.
    pub fn candidates_with_paths(paths: &Paths) -> Vec<CompletionCandidate> {
        use crate::library::spec::SpecType;
        candidates_from_library(paths, |s| s.spec_type == SpecType::Agent)
    }
}

impl ValueCandidates for AgentIdCompleter {
    fn candidates(&self) -> Vec<CompletionCandidate> {
        match Paths::resolve() {
            Some(paths) => Self::candidates_with_paths(&paths),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::spec::{Spec, SpecType};
    use crate::library::Library;
    use tempfile::TempDir;

    /// Helper: create a temp library with known specs and return Paths.
    fn create_test_library() -> (TempDir, Paths) {
        let dir = TempDir::new().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );

        let library = Library {
            version: 1,
            specs: vec![
                Spec::new(
                    "test-driven-development",
                    SpecType::Skill,
                    "TDD",
                    "Test-driven development workflow",
                ),
                Spec::new(
                    "code-reviewer",
                    SpecType::Agent,
                    "Code Reviewer",
                    "Reviews code changes",
                ),
                Spec::new("debugging", SpecType::Skill, "Debugging", ""),
            ],
        };

        std::fs::create_dir_all(paths.data_dir()).unwrap();
        library.save(&paths).unwrap();

        (dir, paths)
    }

    #[test]
    fn test_all_spec_ids_returns_all() {
        let (_dir, paths) = create_test_library();
        let candidates = SpecIdCompleter::candidates_with_paths(&paths);
        let ids: Vec<&str> = candidates
            .iter()
            .map(|c| c.get_value().to_str().unwrap())
            .collect();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&"test-driven-development"));
        assert!(ids.contains(&"code-reviewer"));
        assert!(ids.contains(&"debugging"));
    }

    #[test]
    fn test_skill_ids_filters_to_skills_only() {
        let (_dir, paths) = create_test_library();
        let candidates = SkillIdCompleter::candidates_with_paths(&paths);
        let ids: Vec<&str> = candidates
            .iter()
            .map(|c| c.get_value().to_str().unwrap())
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"test-driven-development"));
        assert!(ids.contains(&"debugging"));
        assert!(!ids.contains(&"code-reviewer"));
    }

    #[test]
    fn test_agent_ids_filters_to_agents_only() {
        let (_dir, paths) = create_test_library();
        let candidates = AgentIdCompleter::candidates_with_paths(&paths);
        let ids: Vec<&str> = candidates
            .iter()
            .map(|c| c.get_value().to_str().unwrap())
            .collect();
        assert_eq!(ids.len(), 1);
        assert!(ids.contains(&"code-reviewer"));
    }

    #[test]
    fn test_missing_library_returns_empty() {
        let dir = TempDir::new().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );
        let candidates = SpecIdCompleter::candidates_with_paths(&paths);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_empty_library_returns_empty() {
        let dir = TempDir::new().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );
        let library = Library::new();
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        library.save(&paths).unwrap();

        let candidates = SpecIdCompleter::candidates_with_paths(&paths);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_spec_without_description_has_no_help() {
        let (_dir, paths) = create_test_library();
        let candidates = SpecIdCompleter::candidates_with_paths(&paths);
        let debugging = candidates
            .iter()
            .find(|c| c.get_value().to_str().unwrap() == "debugging")
            .unwrap();
        assert!(debugging.get_help().is_none());
    }

    #[test]
    fn test_spec_with_description_has_help() {
        let (_dir, paths) = create_test_library();
        let candidates = SpecIdCompleter::candidates_with_paths(&paths);
        let tdd = candidates
            .iter()
            .find(|c| c.get_value().to_str().unwrap() == "test-driven-development")
            .unwrap();
        assert!(tdd.get_help().is_some());
    }
}
