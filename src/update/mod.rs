//! Self-update functionality for AKM.
//!
//! Provides version checking against GitHub Releases and atomic binary
//! replacement. The version check runs in a background thread to avoid
//! blocking command execution.

pub mod download;
pub mod version_check;

use serde::{Deserialize, Serialize};

/// The current version of the AKM binary, set at compile time.
///
/// Uses the version from Cargo.toml via the `CARGO_PKG_VERSION` env var.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Parsed release information from the GitHub Releases API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseInfo {
    /// The version tag (e.g., "v1.0.0" or "1.0.0").
    pub tag_name: String,

    /// Direct download URL for the Linux x86_64 binary asset.
    /// Populated by scanning the `assets` array for a matching filename.
    pub download_url: Option<String>,

    /// Human-readable release name (optional, from GitHub).
    pub name: Option<String>,
}

/// Cached version check result, stored as JSON at `Paths::update_check_cache()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedCheck {
    /// Unix timestamp (seconds) when the check was performed.
    pub checked_at: u64,

    /// The latest version available (e.g., "1.0.1").
    pub latest_version: String,

    /// Direct download URL, if available.
    pub download_url: Option<String>,
}

/// Normalize a version tag by stripping a leading "v" prefix.
///
/// GitHub Releases conventionally use "v1.0.0" tags, but our
/// `CARGO_PKG_VERSION` is just "1.0.0".
pub fn normalize_version(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// Compare two semver strings. Returns true if `latest` is newer than `current`.
///
/// Uses numeric comparison on split version components. Handles pre-release
/// suffixes: a stable release (no suffix) is always newer than a pre-release
/// with the same base version (e.g., `1.0.0` > `1.0.0-alpha.1`).
pub fn is_newer(current: &str, latest: &str) -> bool {
    fn split_pre(v: &str) -> (&str, bool) {
        match v.split_once('-') {
            Some((base, _)) => (base, true),
            None => (v, false),
        }
    }

    let parse = |base: &str| -> Vec<u64> {
        base.split('.')
            .filter_map(|s| s.parse::<u64>().ok())
            .collect()
    };

    let (current_base, current_is_pre) = split_pre(current);
    let (latest_base, latest_is_pre) = split_pre(latest);

    let current_parts = parse(current_base);
    let latest_parts = parse(latest_base);

    // Compare base version components
    for (c, l) in current_parts.iter().zip(latest_parts.iter()) {
        if l > c {
            return true;
        }
        if l < c {
            return false;
        }
    }

    // If one has more components and all prior matched, longer is newer
    if latest_parts.len() != current_parts.len() {
        return latest_parts.len() > current_parts.len();
    }

    // Same base version — pre-release < stable
    if current_is_pre && !latest_is_pre {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_patch_bump() {
        assert!(is_newer("1.0.0", "1.0.1"));
    }

    #[test]
    fn test_is_newer_minor_bump() {
        assert!(is_newer("1.0.0", "1.1.0"));
    }

    #[test]
    fn test_is_newer_major_bump() {
        assert!(is_newer("1.0.0", "2.0.0"));
    }

    #[test]
    fn test_is_newer_same_version() {
        assert!(!is_newer("1.0.0", "1.0.0"));
    }

    #[test]
    fn test_is_newer_older_version() {
        assert!(!is_newer("1.1.0", "1.0.0"));
    }

    #[test]
    fn test_is_newer_with_v_prefix() {
        let current = normalize_version("1.0.0");
        let latest = normalize_version("v1.1.0");
        assert!(is_newer(current, latest));
    }

    #[test]
    fn test_is_newer_prerelease_to_stable() {
        assert!(is_newer("1.0.0-alpha.1", "1.0.0"));
    }

    #[test]
    fn test_is_newer_stable_not_older_than_prerelease() {
        assert!(!is_newer("1.0.0", "1.0.0-beta.1"));
    }

    #[test]
    fn test_is_newer_exhaustive() {
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(is_newer("0.1.0", "0.1.1"));
        assert!(is_newer("0.1.1", "0.2.0"));
        assert!(is_newer("0.9.9", "1.0.0"));
        assert!(!is_newer("2.0.0", "1.9.9"));
        assert!(is_newer("1.0.0-alpha.1", "1.0.1"));
    }

    #[test]
    fn test_normalize_version_variants() {
        assert_eq!(normalize_version("v1.0.0"), "1.0.0");
        assert_eq!(normalize_version("1.0.0"), "1.0.0");
        assert_eq!(normalize_version("v"), "");
        assert_eq!(normalize_version(""), "");
        assert_eq!(normalize_version("v1.0.0-alpha.1"), "1.0.0-alpha.1");
    }
}
