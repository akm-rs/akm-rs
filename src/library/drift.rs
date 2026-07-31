//! Drift detection — which side of the registry has moved since the last sync.
//!
//! The cold library is the registry's git working tree, so this module asks
//! git rather than tracking state of its own:
//!
//! * **local drift** is `git status --porcelain` — anything uncommitted;
//! * **remote drift** is `git diff --name-only HEAD @{upstream}` — anything the
//!   fetched remote has that `HEAD` does not.
//!
//! Both are lists of paths, which map back to the spec that owns them. A spec
//! whose paths appear in neither list is level with the remote; one that
//! appears in both has diverged and needs a decision.

use crate::error::Result;
use crate::git::Git;
use crate::library::spec::SpecType;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Where a spec stands relative to the remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DriftState {
    /// Level with the remote — nothing to do.
    #[default]
    Clean,
    /// Edited here since the last sync. Offer to publish.
    LocalNewer,
    /// The remote moved ahead. A fast-forward applies it automatically.
    RemoteNewer,
    /// Both sides moved. Needs a decision: diff, publish or revert.
    Diverged,
}

impl DriftState {
    /// Classify a spec from whether each side touched it.
    pub fn from_sides(local: bool, remote: bool) -> Self {
        match (local, remote) {
            (false, false) => DriftState::Clean,
            (true, false) => DriftState::LocalNewer,
            (false, true) => DriftState::RemoteNewer,
            (true, true) => DriftState::Diverged,
        }
    }

    /// Whether this machine holds changes the remote has not seen.
    pub fn has_local_changes(self) -> bool {
        matches!(self, DriftState::LocalNewer | DriftState::Diverged)
    }

    /// Short marker for list views. Clean specs get a blank so the column
    /// only draws attention when there is something to act on.
    pub fn marker(self) -> &'static str {
        match self {
            DriftState::Clean => " ",
            DriftState::LocalNewer => "*",
            DriftState::RemoteNewer => "v",
            DriftState::Diverged => "!",
        }
    }
}

impl std::fmt::Display for DriftState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let word = match self {
            DriftState::Clean => "clean",
            DriftState::LocalNewer => "local newer",
            DriftState::RemoteNewer => "remote newer",
            DriftState::Diverged => "diverged",
        };
        f.write_str(word)
    }
}

/// What a changed path belongs to.
enum Owner {
    Spec(String),
    Instructions,
    /// `library.json`, `.gitignore`, anything AKM does not track per-spec.
    Other,
}

/// Path of the global instructions file within the registry tree.
pub const INSTRUCTIONS_PATH: &str = "instructions/global.md";

fn owner_of(path: &str) -> Owner {
    if path == INSTRUCTIONS_PATH {
        return Owner::Instructions;
    }
    match SpecType::owner_of(path) {
        Some((_, id)) => Owner::Spec(id),
        None => Owner::Other,
    }
}

/// Per-spec drift across the whole library.
#[derive(Debug, Clone, Default)]
pub struct DriftReport {
    /// Only specs that are *not* clean. Absence means clean.
    specs: BTreeMap<String, DriftState>,
    /// Drift of the global instructions file.
    instructions: DriftState,
}

impl DriftReport {
    /// Compute drift for a library working tree.
    ///
    /// A directory that is not a git repository, or a branch with no upstream,
    /// yields whatever local drift can still be seen — never an error. Drift is
    /// advisory: it must not be able to fail a sync.
    pub fn compute(library_dir: &Path) -> Result<Self> {
        if !Git::is_repo(library_dir) {
            return Ok(Self::default());
        }

        let local: BTreeSet<String> = Git::status_porcelain(library_dir)?
            .into_iter()
            .map(|entry| entry.path)
            .collect();

        // No upstream (fresh init, detached, no remote) simply means nothing is
        // known to be newer on the far side.
        let remote: BTreeSet<String> = match Git::upstream_ref(library_dir) {
            Ok(upstream) => Git::diff_names(library_dir, "HEAD", &upstream, &[])?
                .into_iter()
                .collect(),
            Err(_) => BTreeSet::new(),
        };

        Ok(Self::from_paths(&local, &remote))
    }

    /// Build a report from two sets of changed paths.
    fn from_paths(local: &BTreeSet<String>, remote: &BTreeSet<String>) -> Self {
        let mut local_specs: BTreeSet<String> = BTreeSet::new();
        let mut remote_specs: BTreeSet<String> = BTreeSet::new();
        let mut local_instructions = false;
        let mut remote_instructions = false;

        for (paths, specs, instructions) in [
            (local, &mut local_specs, &mut local_instructions),
            (remote, &mut remote_specs, &mut remote_instructions),
        ] {
            for path in paths {
                match owner_of(path) {
                    Owner::Spec(id) => {
                        specs.insert(id);
                    }
                    Owner::Instructions => *instructions = true,
                    Owner::Other => {}
                }
            }
        }

        let mut specs = BTreeMap::new();
        for id in local_specs.union(&remote_specs) {
            let state = DriftState::from_sides(local_specs.contains(id), remote_specs.contains(id));
            if state != DriftState::Clean {
                specs.insert(id.clone(), state);
            }
        }

        Self {
            specs,
            instructions: DriftState::from_sides(local_instructions, remote_instructions),
        }
    }

    /// Drift state of one spec.
    pub fn state_of(&self, id: &str) -> DriftState {
        self.specs.get(id).copied().unwrap_or_default()
    }

    /// Drift state of the global instructions file.
    pub fn instructions(&self) -> DriftState {
        self.instructions
    }

    /// Every spec that is not clean, in id order.
    pub fn drifted(&self) -> impl Iterator<Item = (&str, DriftState)> {
        self.specs.iter().map(|(id, state)| (id.as_str(), *state))
    }

    /// Specs in a given state, in id order.
    pub fn in_state(&self, state: DriftState) -> Vec<&str> {
        self.specs
            .iter()
            .filter(|(_, s)| **s == state)
            .map(|(id, _)| id.as_str())
            .collect()
    }

    /// Whether the whole library — specs and instructions — is level.
    pub fn is_clean(&self) -> bool {
        self.specs.is_empty() && self.instructions == DriftState::Clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// All four cells of the drift table, from one pair of path sets.
    #[test]
    fn classifies_every_combination_of_sides() {
        let report = DriftReport::from_paths(
            &paths(&["skills/local/SKILL.md", "skills/both/akm.json"]),
            &paths(&["skills/remote/SKILL.md", "skills/both/SKILL.md"]),
        );

        assert_eq!(report.state_of("local"), DriftState::LocalNewer);
        assert_eq!(report.state_of("remote"), DriftState::RemoteNewer);
        assert_eq!(report.state_of("both"), DriftState::Diverged);
        assert_eq!(report.state_of("untouched"), DriftState::Clean);
        assert!(!report.is_clean());
    }

    /// Editing a skill's sidecar and its markdown is still one drifted spec —
    /// the unit the user acts on is the spec, not the file.
    #[test]
    fn several_files_of_one_spec_collapse_to_one_entry() {
        let report = DriftReport::from_paths(
            &paths(&[
                "skills/tdd/SKILL.md",
                "skills/tdd/akm.json",
                "skills/tdd/references/notes.md",
            ]),
            &paths(&[]),
        );

        assert_eq!(report.drifted().count(), 1);
        assert_eq!(report.state_of("tdd"), DriftState::LocalNewer);
    }

    #[test]
    fn agents_are_tracked_through_both_of_their_files() {
        let report = DriftReport::from_paths(
            &paths(&["agents/reviewer.akm.json"]),
            &paths(&["agents/reviewer.md"]),
        );

        assert_eq!(report.state_of("reviewer"), DriftState::Diverged);
    }

    /// The derived index is regenerated on every sync, so it must never be
    /// mistaken for a change the user made.
    #[test]
    fn non_spec_paths_do_not_create_drift() {
        let report = DriftReport::from_paths(
            &paths(&["library.json", ".gitignore", "README.md"]),
            &paths(&[]),
        );

        assert!(report.is_clean());
        assert_eq!(report.drifted().count(), 0);
    }

    #[test]
    fn instructions_drift_is_tracked_separately_from_specs() {
        let report = DriftReport::from_paths(
            &paths(&["instructions/global.md"]),
            &paths(&["instructions/global.md"]),
        );

        assert_eq!(report.instructions(), DriftState::Diverged);
        assert_eq!(report.drifted().count(), 0);
        assert!(!report.is_clean());
    }

    #[test]
    fn in_state_lists_ids_in_order() {
        let report = DriftReport::from_paths(
            &paths(&["skills/zebra/SKILL.md", "skills/alpha/SKILL.md"]),
            &paths(&[]),
        );

        assert_eq!(
            report.in_state(DriftState::LocalNewer),
            vec!["alpha", "zebra"]
        );
        assert!(report.in_state(DriftState::Diverged).is_empty());
    }

    #[test]
    fn a_clean_tree_reports_clean() {
        let report = DriftReport::from_paths(&paths(&[]), &paths(&[]));
        assert!(report.is_clean());
        assert_eq!(report.instructions(), DriftState::Clean);
    }
}
