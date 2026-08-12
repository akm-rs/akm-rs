//! Machine-local overrides — the one piece of library state that must not
//! propagate.
//!
//! A spec's `core` flag has two halves. The sidecar carries the *default*:
//! "this skill should be globally mounted on every machine I own", and it
//! travels through the registry like any other metadata. `local.json` carries
//! only the *deviations* from that default on this particular machine.
//!
//! Storing deviations rather than a full list is what keeps both directions
//! working: a newly published core skill still reaches every machine (no local
//! entry means "take the default"), while a laptop that has switched one skill
//! off keeps it off across syncs without ever pushing that choice to anyone.

use crate::error::{Error, Result};
use crate::library::Library;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Contents of `$XDG_DATA_HOME/akm/local.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LocalOverrides {
    /// Spec id → the core value this machine wants, where it differs from the
    /// spec's published default. Absent ids follow the default.
    #[serde(default)]
    pub core: BTreeMap<String, bool>,
}

impl LocalOverrides {
    /// Load overrides from disk. A missing file means "no deviations".
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path).map_err(|source| Error::Io {
            context: format!("Reading local overrides from {}", path.display()),
            source,
        })?;

        serde_json::from_str(&content).map_err(|e| Error::LocalOverridesParse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }

    /// Write overrides to disk, creating parent directories as needed.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                context: format!("Creating directory for {}", path.display()),
                source,
            })?;
        }

        let content = serde_json::to_string_pretty(self).map_err(|e| Error::Io {
            context: format!("Serializing local overrides for {}", path.display()),
            source: std::io::Error::other(e),
        })?;

        std::fs::write(path, format!("{content}\n")).map_err(|source| Error::Io {
            context: format!("Writing local overrides to {}", path.display()),
            source,
        })
    }

    /// The core value in force on this machine for `id`.
    pub fn effective_core(&self, id: &str, default: bool) -> bool {
        self.core.get(id).copied().unwrap_or(default)
    }

    /// Record what this machine wants `core` to be for `id`.
    ///
    /// Setting it back to the published default drops the entry rather than
    /// pinning it, so the spec resumes following the registry.
    pub fn set_core(&mut self, id: &str, wanted: bool, default: bool) {
        if wanted == default {
            self.core.remove(id);
        } else {
            self.core.insert(id.to_string(), wanted);
        }
    }

    /// Forget any deviation for `id`.
    pub fn clear_core(&mut self, id: &str) {
        self.core.remove(id);
    }

    /// Move any deviation from `old` to `new`, following a spec rename.
    ///
    /// Without this the entry for `old` would be dropped by [`Self::apply`] as
    /// a stale id, silently resetting the renamed spec to its published core
    /// default on this machine.
    pub fn rename(&mut self, old: &str, new: &str) {
        if let Some(value) = self.core.remove(old) {
            self.core.insert(new.to_string(), value);
        }
    }

    /// Number of specs deviating from their published default.
    pub fn deviation_count(&self) -> usize {
        self.core.len()
    }

    /// Rewrite a library's `core` flags to what this machine actually wants.
    ///
    /// libgen fills `core` from the sidecars, i.e. the published defaults;
    /// everything downstream — symlinks, the TUI, `skills status` — reads
    /// `library.json`, so the deviations are folded in exactly once, here.
    ///
    /// Deviations naming specs that no longer exist are dropped, which is how
    /// stale entries get cleaned up after a spec is deleted upstream. Returns
    /// whether anything was dropped.
    pub fn apply(&mut self, library: &mut Library) -> bool {
        for spec in &mut library.specs {
            spec.core = self.effective_core(&spec.id, spec.core);
        }

        let before = self.core.len();
        self.core.retain(|id, _| library.contains(id));
        before != self.core.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::spec::{Spec, SpecType};

    fn library_with(core_defaults: &[(&str, bool)]) -> Library {
        Library {
            version: 1,
            specs: core_defaults
                .iter()
                .map(|(id, core)| Spec {
                    core: *core,
                    ..Spec::new(*id, SpecType::Skill, *id, "desc")
                })
                .collect(),
        }
    }

    #[test]
    fn no_overrides_means_the_published_defaults_win() {
        let mut overrides = LocalOverrides::default();
        let mut library = library_with(&[("a", true), ("b", false)]);

        assert!(!overrides.apply(&mut library));

        assert!(library.get("a").unwrap().core);
        assert!(!library.get("b").unwrap().core);
    }

    #[test]
    fn a_deviation_wins_over_the_published_default() {
        let mut overrides = LocalOverrides::default();
        overrides.set_core("a", false, true);
        overrides.set_core("b", true, false);

        let mut library = library_with(&[("a", true), ("b", false)]);
        overrides.apply(&mut library);

        assert!(!library.get("a").unwrap().core);
        assert!(library.get("b").unwrap().core);
    }

    /// Only deviations are stored, so a spec that agrees with the registry
    /// keeps following it — including when the registry later changes.
    #[test]
    fn setting_a_spec_back_to_its_default_drops_the_deviation() {
        let mut overrides = LocalOverrides::default();
        overrides.set_core("a", true, false);
        assert_eq!(overrides.deviation_count(), 1);

        overrides.set_core("a", false, false);
        assert_eq!(overrides.deviation_count(), 0);
    }

    /// A spec published as core after this machine last synced must become
    /// core here too — no local entry means no opinion.
    #[test]
    fn newly_published_core_specs_still_propagate() {
        let mut overrides = LocalOverrides::default();
        overrides.set_core("a", true, false);

        let mut library = library_with(&[("a", false), ("newly-core", true)]);
        overrides.apply(&mut library);

        assert!(library.get("newly-core").unwrap().core);
    }

    #[test]
    fn deviations_for_deleted_specs_are_pruned() {
        let mut overrides = LocalOverrides::default();
        overrides.set_core("gone", true, false);
        overrides.set_core("a", true, false);

        let mut library = library_with(&[("a", false)]);
        assert!(overrides.apply(&mut library));

        assert_eq!(overrides.deviation_count(), 1);
        assert!(overrides.core.contains_key("a"));
    }

    #[test]
    fn roundtrips_through_disk_and_treats_a_missing_file_as_empty() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("local.json");

        assert_eq!(
            LocalOverrides::load_from(&path).unwrap(),
            LocalOverrides::default()
        );

        let mut overrides = LocalOverrides::default();
        overrides.set_core("a", true, false);
        overrides.save_to(&path).unwrap();

        assert_eq!(LocalOverrides::load_from(&path).unwrap(), overrides);
    }
}
