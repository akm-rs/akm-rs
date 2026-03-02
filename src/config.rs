//! TOML configuration for AKM.
//!
//! Corresponds to the Bash config at `~/.config/akm/config`.
//! The Rust version uses TOML format (`config.toml`) instead of flat
//! key=value shell sourcing. This is a fresh start — no migration needed.

use crate::error::{Error, IoContext, Result};
use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Default community registry URL.
/// Bash: `DEFAULT_SKILLS_COMMUNITY_REGISTRY="https://github.com/akm-rs/skillverse.git"`
pub const DEFAULT_COMMUNITY_REGISTRY: &str = "https://github.com/akm-rs/skillverse.git";

/// The three AKM feature domains.
/// Bash: validated in `_config_validate()` for the `features` key.
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
///
/// Maps to the Bash config file variables:
/// - `FEATURES` → `features`
/// - `SKILLS_COMMUNITY_REGISTRY` → `skills.community_registry`
/// - `SKILLS_PERSONAL_REGISTRY` → `skills.personal_registry`
/// - `ARTIFACTS_REMOTE` → `artifacts.remote`
/// - `ARTIFACTS_DIR` → `artifacts.dir`
/// - `ARTIFACTS_AUTO_PUSH` → `artifacts.auto_push`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Enabled feature domains.
    /// Bash: `FEATURES="skills,artifacts,instructions"`
    #[serde(default)]
    pub features: BTreeSet<Feature>,

    /// Skills configuration section.
    #[serde(default)]
    pub skills: SkillsConfig,

    /// Artifacts configuration section.
    #[serde(default)]
    pub artifacts: ArtifactsConfig,
}

/// Skills-specific configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillsConfig {
    /// Git URL for the community (read-only) registry.
    /// Bash: `SKILLS_COMMUNITY_REGISTRY`
    /// Default: `None` (resolved to DEFAULT_COMMUNITY_REGISTRY at point of use)
    #[serde(default)]
    pub community_registry: Option<String>,

    /// Git URL for the personal (read-write) registry.
    /// Bash: `SKILLS_PERSONAL_REGISTRY`
    #[serde(default)]
    pub personal_registry: Option<String>,
}

/// Artifacts-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactsConfig {
    /// Git remote URL for the artifacts repository.
    /// Bash: `ARTIFACTS_REMOTE`
    #[serde(default)]
    pub remote: Option<String>,

    /// Local directory for artifacts.
    /// Bash: `ARTIFACTS_DIR` (default: `$HOME/.akm/artifacts`)
    #[serde(default)]
    pub dir: Option<PathBuf>,

    /// Whether to auto-push artifacts on session exit.
    /// Bash: `ARTIFACTS_AUTO_PUSH` (default: true)
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
        }
    }
}

impl Config {
    /// Load config from the TOML file at the path determined by `Paths`.
    ///
    /// If the file does not exist, returns a default config (matches Bash
    /// behavior where `_load_config()` silently returns if file is absent).
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
            let known_top: &[&str] = &["features", "skills", "artifacts"];
            let known_skills: &[&str] = &["community_registry", "personal_registry"];
            let known_artifacts: &[&str] = &["remote", "dir", "auto_push"];

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
        }

        // Step 3: Deserialize into Config, warning about section-level failures
        match toml::from_str::<Config>(&content) {
            Ok(config) => Ok(config),
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
                }
                Ok(config)
            }
        }
    }

    /// Save config to TOML file. Creates parent directories if needed.
    ///
    /// Bash equivalent: `_write_config()`
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
    ///
    /// Bash equivalent: `_feature_enabled()`
    /// ```bash
    /// [[ ",$features," == *",$feature,"* ]]
    /// ```
    pub fn is_feature_enabled(&self, feature: Feature) -> bool {
        self.features.contains(&feature)
    }

    /// Resolve the artifacts directory, falling back to the default.
    ///
    /// Bash: `ARTIFACTS_DIR="${ARTIFACTS_DIR:-$HOME/.akm/artifacts}"`
    pub fn artifacts_dir(&self, paths: &Paths) -> PathBuf {
        self.artifacts
            .dir
            .clone()
            .unwrap_or_else(|| paths.default_artifacts_dir())
    }

    /// Get the effective community registry URL, falling back to the default.
    ///
    /// Bash: `SKILLS_COMMUNITY_REGISTRY="${SKILLS_COMMUNITY_REGISTRY:-$DEFAULT_SKILLS_COMMUNITY_REGISTRY}"`
    /// Returns the configured value or the built-in default. Use
    /// `community_registry_is_explicit()` to distinguish user-set from default.
    pub fn community_registry_url(&self) -> &str {
        self.skills
            .community_registry
            .as_deref()
            .unwrap_or(DEFAULT_COMMUNITY_REGISTRY)
    }

    /// Whether the community registry was explicitly set (not the default).
    /// Used by `print_all()` to show "(default)" annotation.
    pub fn community_registry_is_explicit(&self) -> bool {
        self.skills.community_registry.is_some()
    }

    /// Get the personal registry URL (may be None).
    pub fn personal_registry_url(&self) -> Option<&str> {
        self.skills.personal_registry.as_deref()
    }
}

/// Addressable config keys for the `akm config <key> [value]` command.
///
/// Maps 1:1 to Bash `_config_key_to_var()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigKey {
    /// `features` — comma-separated enabled features
    Features,
    /// `skills.community-registry` → `skills.community_registry`
    SkillsCommunityRegistry,
    /// `skills.personal-registry` → `skills.personal_registry`
    SkillsPersonalRegistry,
    /// `artifacts.remote`
    ArtifactsRemote,
    /// `artifacts.dir`
    ArtifactsDir,
    /// `artifacts.auto-push` → `artifacts.auto_push`
    ArtifactsAutoPush,
}

/// All valid config key names for help text.
pub const ALL_CONFIG_KEYS: &str = "features, skills.community-registry, skills.personal-registry, \
     artifacts.remote, artifacts.dir, artifacts.auto-push";

impl std::str::FromStr for ConfigKey {
    type Err = Error;

    /// Parse a CLI key string to ConfigKey.
    ///
    /// Bash equivalent: `_config_key_to_var()`
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "features" => Ok(ConfigKey::Features),
            "skills.community-registry" => Ok(ConfigKey::SkillsCommunityRegistry),
            "skills.personal-registry" => Ok(ConfigKey::SkillsPersonalRegistry),
            "artifacts.remote" => Ok(ConfigKey::ArtifactsRemote),
            "artifacts.dir" => Ok(ConfigKey::ArtifactsDir),
            "artifacts.auto-push" => Ok(ConfigKey::ArtifactsAutoPush),
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
            ConfigKey::SkillsCommunityRegistry => {
                config.skills.community_registry.clone().unwrap_or_default()
            }
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
        }
    }

    /// Set a value on a Config, with validation.
    ///
    /// Bash equivalent: `_config_validate()` + `eval "$var_name=\"$value\""`
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
            ConfigKey::SkillsCommunityRegistry => {
                config.skills.community_registry = if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                };
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
            "skills.community-registry".parse::<ConfigKey>().unwrap(),
            ConfigKey::SkillsCommunityRegistry
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
    fn config_default_community_registry_uses_fallback() {
        let config = Config::default();
        assert_eq!(config.community_registry_url(), DEFAULT_COMMUNITY_REGISTRY);
        assert!(!config.community_registry_is_explicit());
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
community_registry = "https://example.com"

[artifacts]
auto_push = "not_a_bool"
"#,
        )
        .unwrap();

        let config = Config::load(&paths).unwrap();
        assert!(config.features.contains(&Feature::Skills));
        assert_eq!(
            config.skills.community_registry.as_deref(),
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
community_registry = "https://example.com"
bogus_field = "also fine"
"#,
        )
        .unwrap();

        let config = Config::load(&paths).unwrap();
        assert!(config.features.contains(&Feature::Skills));
        assert_eq!(
            config.skills.community_registry.as_deref(),
            Some("https://example.com")
        );
    }
}
