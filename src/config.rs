//! TOML configuration for AKM.
//!
//! Config lives at `~/.config/akm/config.toml` (XDG-compliant) and is
//! parsed as TOML into typed structs.

use crate::error::{Error, IoContext, Result};
use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Default GitHub Releases API URL for update checks.
pub const DEFAULT_UPDATE_URL: &str = "https://api.github.com/repos/akm-rs/akm-rs/releases/latest";

/// Old (incorrect) update URL that pointed at the wrong repo.
/// Used for automatic migration of existing configs created before the fix.
const OLD_UPDATE_URL: &str = "https://api.github.com/repos/akm-rs/akm/releases/latest";

/// Default interval between update checks (24 hours in seconds).
pub const DEFAULT_CHECK_INTERVAL_SECS: u64 = 86400;

/// The three AKM feature domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Feature {
    Skills,
    Artifacts,
    Instructions,
}

impl std::fmt::Display for Feature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Feature::Skills => write!(f, "skills"),
            Feature::Artifacts => write!(f, "artifacts"),
            Feature::Instructions => write!(f, "instructions"),
        }
    }
}

impl std::str::FromStr for Feature {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "skills" => Ok(Feature::Skills),
            "artifacts" => Ok(Feature::Artifacts),
            "instructions" => Ok(Feature::Instructions),
            other => Err(Error::ConfigValidation {
                key: "features".into(),
                message: format!(
                    "Invalid feature: '{other}' (must be skills, artifacts, or instructions)"
                ),
            }),
        }
    }
}

/// Top-level AKM configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Enabled feature domains.
    /// Default: all three domains enabled.
    #[serde(default)]
    pub features: BTreeSet<Feature>,

    /// Skills configuration section.
    #[serde(default)]
    pub skills: SkillsConfig,

    /// Artifacts configuration section.
    #[serde(default)]
    pub artifacts: ArtifactsConfig,

    /// Update configuration (new in Rust version).
    #[serde(default)]
    pub update: UpdateConfig,
}

/// Skills-specific configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillsConfig {
    /// Git URL for the personal (read-write) registry.
    #[serde(default)]
    pub personal_registry: Option<String>,
}

/// Artifacts-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactsConfig {
    /// Git remote URL for the artifacts repository.
    #[serde(default)]
    pub remote: Option<String>,

    /// Local directory for artifacts.
    /// Default: `$HOME/.akm/artifacts`.
    #[serde(default)]
    pub dir: Option<PathBuf>,

    /// Whether to auto-push artifacts on session exit.
    /// Default: true.
    #[serde(default = "default_true")]
    pub auto_push: bool,
}

impl Default for ArtifactsConfig {
    fn default() -> Self {
        Self {
            remote: None,
            dir: None,
            auto_push: true,
        }
    }
}

/// Update-specific configuration.
///
/// `url` is a full GitHub Releases API URL, so forks can point elsewhere.
/// Updates download pre-built release binaries rather than building from source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfig {
    /// URL for the GitHub Releases API (or compatible endpoint).
    /// Default: `https://api.github.com/repos/akm-rs/akm-rs/releases/latest`
    #[serde(default = "default_update_url", skip_serializing_if = "is_default_url")]
    pub url: String,

    /// Interval between automatic update checks, in seconds.
    /// Default: 86400 (24 hours)
    #[serde(default = "default_check_interval")]
    pub check_interval: u64,

    /// Whether to automatically check for updates on every invocation.
    /// Default: true
    #[serde(default = "default_true")]
    pub auto_check: bool,
}

fn default_update_url() -> String {
    DEFAULT_UPDATE_URL.to_string()
}

fn default_check_interval() -> u64 {
    DEFAULT_CHECK_INTERVAL_SECS
}

fn is_default_url(url: &String) -> bool {
    url == DEFAULT_UPDATE_URL
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            url: default_update_url(),
            check_interval: default_check_interval(),
            auto_check: true,
        }
    }
}

fn default_true() -> bool {
    true
}

#[allow(clippy::derivable_impls)] // Explicit to ensure ArtifactsConfig::default() auto_push=true
impl Default for Config {
    fn default() -> Self {
        Self {
            features: BTreeSet::new(),
            skills: SkillsConfig::default(),
            artifacts: ArtifactsConfig::default(),
            update: UpdateConfig::default(),
        }
    }
}

/// Migrate the known-bad update URL to the correct repo.
///
/// Early releases shipped with `DEFAULT_UPDATE_URL` pointing at `akm-rs/akm`.
/// Once `akm setup` serialized that to `config.toml`, the wrong URL persisted
/// even after the code default was fixed. This migration corrects it on load.
fn migrate_update_url(config: &mut Config) {
    if config.update.url == OLD_UPDATE_URL {
        config.update.url = DEFAULT_UPDATE_URL.to_string();
    }
}

impl Config {
    /// Load config from the TOML file at the path determined by `Paths`.
    ///
    /// If the file does not exist, returns a default config — an absent
    /// config file is not an error.
    ///
    /// Unknown keys at any level produce a warning to stderr but do not crash.
    /// Invalid values in sub-sections produce a warning and fall back to defaults.
    pub fn load(paths: &Paths) -> Result<Self> {
        let config_file = paths.config_file();
        if !config_file.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_file).map_err(|e| Error::ConfigRead {
            path: config_file.clone(),
            source: Box::new(e),
        })?;

        // Step 1: Parse as raw TOML Value to check for unknown keys at every level
        let raw: toml::Value = toml::from_str(&content).map_err(|e| Error::ConfigRead {
            path: config_file.clone(),
            source: Box::new(e),
        })?;

        // Step 2: Walk the table tree and warn about unknown keys
        if let Some(table) = raw.as_table() {
            let known_top: &[&str] = &["features", "skills", "artifacts", "update"];
            // `community_registry` is obsolete but still tolerated silently so
            // configs written before it was dropped do not warn on every run.
            let known_skills: &[&str] = &["community_registry", "personal_registry"];
            let known_artifacts: &[&str] = &["remote", "dir", "auto_push"];
            let known_update: &[&str] = &["url", "check_interval", "auto_check"];

            for key in table.keys() {
                if !known_top.contains(&key.as_str()) {
                    eprintln!(
                        "Warning: unknown config key '{}' in {}",
                        key,
                        config_file.display()
                    );
                }
            }
            if let Some(toml::Value::Table(skills)) = table.get("skills") {
                for key in skills.keys() {
                    if !known_skills.contains(&key.as_str()) {
                        eprintln!(
                            "Warning: unknown config key 'skills.{}' in {}",
                            key,
                            config_file.display()
                        );
                    }
                }
            }
            if let Some(toml::Value::Table(artifacts)) = table.get("artifacts") {
                for key in artifacts.keys() {
                    if !known_artifacts.contains(&key.as_str()) {
                        eprintln!(
                            "Warning: unknown config key 'artifacts.{}' in {}",
                            key,
                            config_file.display()
                        );
                    }
                }
            }
            if let Some(toml::Value::Table(update)) = table.get("update") {
                for key in update.keys() {
                    if !known_update.contains(&key.as_str()) {
                        eprintln!(
                            "Warning: unknown config key 'update.{}' in {}",
                            key,
                            config_file.display()
                        );
                    }
                }
            }
        }

        // Step 3: Deserialize into Config, warning about section-level failures
        match toml::from_str::<Config>(&content) {
            Ok(mut config) => {
                migrate_update_url(&mut config);
                Ok(config)
            }
            Err(_) => {
                // Partial parse: deserialize each section independently
                let mut config = Config::default();

                if let Some(table) = raw.as_table() {
                    if let Some(features) = table.get("features") {
                        match features.clone().try_into::<BTreeSet<Feature>>() {
                            Ok(f) => config.features = f,
                            Err(e) => eprintln!(
                                "Warning: invalid 'features' in {}, using defaults: {e}",
                                config_file.display()
                            ),
                        }
                    }
                    if let Some(skills) = table.get("skills") {
                        match skills.clone().try_into::<SkillsConfig>() {
                            Ok(s) => config.skills = s,
                            Err(e) => eprintln!(
                                "Warning: invalid [skills] config in {}, using defaults: {e}",
                                config_file.display()
                            ),
                        }
                    }
                    if let Some(artifacts) = table.get("artifacts") {
                        match artifacts.clone().try_into::<ArtifactsConfig>() {
                            Ok(a) => config.artifacts = a,
                            Err(e) => eprintln!(
                                "Warning: invalid [artifacts] config in {}, using defaults: {e}",
                                config_file.display()
                            ),
                        }
                    }
                    if let Some(update) = table.get("update") {
                        match update.clone().try_into::<UpdateConfig>() {
                            Ok(u) => config.update = u,
                            Err(e) => eprintln!(
                                "Warning: invalid [update] config in {}, using defaults: {e}",
                                config_file.display()
                            ),
                        }
                    }
                }
                migrate_update_url(&mut config);
                Ok(config)
            }
        }
    }

    /// Save config to TOML file. Creates parent directories if needed.
    ///
    /// Idempotent — safe to call multiple times.
    pub fn save(&self, paths: &Paths) -> Result<()> {
        let config_file = paths.config_file();
        let config_dir = paths.config_dir();

        std::fs::create_dir_all(config_dir).io_context(format!(
            "Creating config directory {}",
            config_dir.display()
        ))?;

        let content = toml::to_string_pretty(self).map_err(|e| Error::ConfigWrite {
            path: config_file.clone(),
            source: Box::new(e),
        })?;

        std::fs::write(&config_file, content).map_err(|e| Error::ConfigWrite {
            path: config_file,
            source: Box::new(e),
        })
    }

    /// Check if a feature is enabled.
    pub fn is_feature_enabled(&self, feature: Feature) -> bool {
        self.features.contains(&feature)
    }

    /// Resolve the artifacts directory, falling back to the default.
    pub fn artifacts_dir(&self, paths: &Paths) -> PathBuf {
        self.artifacts
            .dir
            .clone()
            .unwrap_or_else(|| paths.default_artifacts_dir())
    }

    /// Get the personal registry URL (may be None).
    pub fn registry_url(&self) -> Option<&str> {
        self.skills
            .personal_registry
            .as_deref()
            .filter(|url| !url.is_empty())
    }
}

/// Addressable config keys for the `akm config <key> [value]` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigKey {
    /// `features` — comma-separated enabled features
    Features,
    /// `skills.personal-registry` → `skills.personal_registry`
    SkillsPersonalRegistry,
    /// `artifacts.remote`
    ArtifactsRemote,
    /// `artifacts.dir`
    ArtifactsDir,
    /// `artifacts.auto-push` → `artifacts.auto_push`
    ArtifactsAutoPush,
    /// `update.url` — GitHub Releases API URL
    UpdateUrl,
    /// `update.check-interval` — seconds between checks
    UpdateCheckInterval,
    /// `update.auto-check` — enable/disable auto-check
    UpdateAutoCheck,
}

/// All valid config key names for help text.
pub const ALL_CONFIG_KEYS: &str = "features, skills.personal-registry, \
     artifacts.remote, artifacts.dir, artifacts.auto-push, \
     update.url, update.check-interval, update.auto-check";

impl std::str::FromStr for ConfigKey {
    type Err = Error;

    /// Parse a CLI key string to ConfigKey.
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "features" => Ok(ConfigKey::Features),
            "skills.personal-registry" => Ok(ConfigKey::SkillsPersonalRegistry),
            "artifacts.remote" => Ok(ConfigKey::ArtifactsRemote),
            "artifacts.dir" => Ok(ConfigKey::ArtifactsDir),
            "artifacts.auto-push" => Ok(ConfigKey::ArtifactsAutoPush),
            "update.url" => Ok(ConfigKey::UpdateUrl),
            "update.check-interval" => Ok(ConfigKey::UpdateCheckInterval),
            "update.auto-check" => Ok(ConfigKey::UpdateAutoCheck),
            other => Err(Error::UnknownConfigKey {
                key: other.to_string(),
                available: ALL_CONFIG_KEYS.to_string(),
            }),
        }
    }
}

impl ConfigKey {
    /// Get the current value of this key from a Config.
    pub fn get(&self, config: &Config) -> String {
        match self {
            ConfigKey::Features => config
                .features
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(","),
            ConfigKey::SkillsPersonalRegistry => {
                config.skills.personal_registry.clone().unwrap_or_default()
            }
            ConfigKey::ArtifactsRemote => config.artifacts.remote.clone().unwrap_or_default(),
            ConfigKey::ArtifactsDir => config
                .artifacts
                .dir
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            ConfigKey::ArtifactsAutoPush => config.artifacts.auto_push.to_string(),
            ConfigKey::UpdateUrl => config.update.url.clone(),
            ConfigKey::UpdateCheckInterval => config.update.check_interval.to_string(),
            ConfigKey::UpdateAutoCheck => config.update.auto_check.to_string(),
        }
    }

    /// Set a value on a Config, with validation.
    pub fn set(&self, config: &mut Config, value: &str) -> Result<()> {
        match self {
            ConfigKey::Features => {
                let mut features = BTreeSet::new();
                if !value.is_empty() {
                    for part in value.split(',') {
                        let f: Feature = part.trim().parse()?;
                        features.insert(f);
                    }
                }
                config.features = features;
            }
            ConfigKey::SkillsPersonalRegistry => {
                config.skills.personal_registry = if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                };
            }
            ConfigKey::ArtifactsRemote => {
                config.artifacts.remote = if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                };
            }
            ConfigKey::ArtifactsDir => {
                config.artifacts.dir = if value.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            ConfigKey::ArtifactsAutoPush => {
                config.artifacts.auto_push = match value {
                    "true" => true,
                    "false" => false,
                    other => {
                        return Err(Error::ConfigValidation {
                            key: "artifacts.auto-push".into(),
                            message: format!("'{other}' is not valid (must be true or false)"),
                        });
                    }
                };
            }
            ConfigKey::UpdateUrl => {
                config.update.url = value.to_string();
            }
            ConfigKey::UpdateCheckInterval => {
                let secs: u64 = value.parse().map_err(|_| Error::ConfigValidation {
                    key: "update.check-interval".into(),
                    message: format!("Expected a positive integer (seconds), got '{value}'"),
                })?;
                config.update.check_interval = secs;
            }
            ConfigKey::UpdateAutoCheck => {
                let b = match value {
                    "true" | "1" | "yes" => true,
                    "false" | "0" | "no" => false,
                    _ => {
                        return Err(Error::ConfigValidation {
                            key: "update.auto-check".into(),
                            message: format!("Expected true/false, got '{value}'"),
                        });
                    }
                };
                config.update.auto_check = b;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_from_str_valid() {
        assert_eq!("skills".parse::<Feature>().unwrap(), Feature::Skills);
        assert_eq!("artifacts".parse::<Feature>().unwrap(), Feature::Artifacts);
        assert_eq!(
            "instructions".parse::<Feature>().unwrap(),
            Feature::Instructions
        );
    }

    #[test]
    fn feature_from_str_invalid() {
        assert!("bogus".parse::<Feature>().is_err());
    }

    #[test]
    fn config_key_from_str_all_valid() {
        assert_eq!(
            "features".parse::<ConfigKey>().unwrap(),
            ConfigKey::Features
        );
        assert_eq!(
            "skills.personal-registry".parse::<ConfigKey>().unwrap(),
            ConfigKey::SkillsPersonalRegistry
        );
        assert_eq!(
            "artifacts.auto-push".parse::<ConfigKey>().unwrap(),
            ConfigKey::ArtifactsAutoPush
        );
    }

    #[test]
    fn config_key_from_str_unknown() {
        assert!("nonexistent.key".parse::<ConfigKey>().is_err());
    }

    #[test]
    fn config_key_set_auto_push_validates_bool() {
        let mut config = Config::default();
        assert!(ConfigKey::ArtifactsAutoPush
            .set(&mut config, "true")
            .is_ok());
        assert!(config.artifacts.auto_push);
        assert!(ConfigKey::ArtifactsAutoPush
            .set(&mut config, "false")
            .is_ok());
        assert!(!config.artifacts.auto_push);
        assert!(ConfigKey::ArtifactsAutoPush
            .set(&mut config, "maybe")
            .is_err());
    }

    #[test]
    fn config_key_set_features_validates() {
        let mut config = Config::default();
        assert!(ConfigKey::Features
            .set(&mut config, "skills,artifacts")
            .is_ok());
        assert!(config.features.contains(&Feature::Skills));
        assert!(config.features.contains(&Feature::Artifacts));
        assert!(ConfigKey::Features
            .set(&mut config, "skills,bogus")
            .is_err());
    }

    #[test]
    fn config_default_has_no_personal_registry() {
        let config = Config::default();
        assert!(config.registry_url().is_none());
    }

    #[test]
    fn config_default_auto_push_true() {
        let config = Config::default();
        assert!(config.artifacts.auto_push);
    }

    #[test]
    fn feature_enabled_check() {
        let mut config = Config::default();
        config.features.insert(Feature::Skills);
        assert!(config.is_feature_enabled(Feature::Skills));
        assert!(!config.is_feature_enabled(Feature::Artifacts));
    }

    #[test]
    fn config_save_load_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = crate::paths::Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            tmp.path(),
        );
        let mut config = Config::default();
        config.features.insert(Feature::Skills);
        config.artifacts.auto_push = false;
        config.artifacts.dir = Some(std::path::PathBuf::from("/custom/path"));

        config.save(&paths).unwrap();
        let loaded = Config::load(&paths).unwrap();

        assert_eq!(config.features, loaded.features);
        assert_eq!(config.artifacts.auto_push, loaded.artifacts.auto_push);
        assert_eq!(config.artifacts.dir, loaded.artifacts.dir);
    }

    #[test]
    fn config_load_partial_parse_recovers_valid_sections() {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = crate::paths::Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            tmp.path(),
        );
        let config_dir = tmp.path().join("config").join("akm");
        std::fs::create_dir_all(&config_dir).unwrap();
        // Valid features and skills, but invalid artifacts (auto_push is a string)
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
features = ["skills"]

[skills]
personal_registry = "https://example.com"

[artifacts]
auto_push = "not_a_bool"
"#,
        )
        .unwrap();

        let config = Config::load(&paths).unwrap();
        assert!(config.features.contains(&Feature::Skills));
        assert_eq!(
            config.skills.personal_registry.as_deref(),
            Some("https://example.com")
        );
        // artifacts falls back to defaults because of invalid value
        assert!(config.artifacts.auto_push); // default is true
    }

    #[test]
    fn config_load_unknown_keys_does_not_crash() {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = crate::paths::Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            tmp.path(),
        );
        let config_dir = tmp.path().join("config").join("akm");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
features = ["skills"]
unknown_key = "should warn but not crash"

[skills]
personal_registry = "https://example.com"
bogus_field = "also fine"
"#,
        )
        .unwrap();

        let config = Config::load(&paths).unwrap();
        assert!(config.features.contains(&Feature::Skills));
        assert_eq!(
            config.skills.personal_registry.as_deref(),
            Some("https://example.com")
        );
    }

    /// Configs written before the community registry was dropped must load
    /// cleanly — the obsolete key is ignored, not rejected.
    #[test]
    fn config_load_tolerates_obsolete_community_registry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = crate::paths::Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            tmp.path(),
        );
        let config_dir = tmp.path().join("config").join("akm");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
features = ["skills"]

[skills]
community_registry = "https://github.com/akm-rs/skillverse.git"
personal_registry = "https://example.com/mine.git"
"#,
        )
        .unwrap();

        let config = Config::load(&paths).unwrap();
        assert!(config.features.contains(&Feature::Skills));
        assert_eq!(
            config.skills.personal_registry.as_deref(),
            Some("https://example.com/mine.git")
        );
    }

    #[test]
    fn config_load_migrates_old_update_url() {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = crate::paths::Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            tmp.path(),
        );
        let config_dir = tmp.path().join("config").join("akm");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
features = ["skills"]

[update]
url = "https://api.github.com/repos/akm-rs/akm/releases/latest"
check_interval = 86400
auto_check = true
"#,
        )
        .unwrap();

        let config = Config::load(&paths).unwrap();
        assert_eq!(config.update.url, DEFAULT_UPDATE_URL);
    }

    #[test]
    fn config_save_does_not_serialize_default_url() {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = crate::paths::Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            tmp.path(),
        );

        let config = Config::default();
        config.save(&paths).unwrap();

        let content = std::fs::read_to_string(paths.config_file()).unwrap();
        assert!(
            !content.contains("api.github.com"),
            "Default update URL should not be serialized to config"
        );
    }

    #[test]
    fn config_save_preserves_custom_url() {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = crate::paths::Paths::from_roots(
            &tmp.path().join("data"),
            &tmp.path().join("config"),
            &tmp.path().join("cache"),
            tmp.path(),
        );

        let mut config = Config::default();
        config.update.url = "https://custom.example.com/releases/latest".to_string();
        config.save(&paths).unwrap();

        let loaded = Config::load(&paths).unwrap();
        assert_eq!(
            loaded.update.url,
            "https://custom.example.com/releases/latest"
        );
    }
}
