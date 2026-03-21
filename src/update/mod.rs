//! Self-update functionality for AKM.
//!
//! Provides version checking against GitHub Releases and atomic binary
//! replacement. The version check runs in a background thread to avoid
//! blocking command execution.

/// Binary download and atomic self-replacement.
pub mod download;
/// Background version checking with cache TTL.
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

    /// Direct download URL for the platform-specific binary asset.
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

/// Return the expected release asset name for the current platform.
///
/// Maps compile-time `target_os` and `target_arch` to the asset names
/// produced by the release workflow (e.g., `akm-linux-x86_64`,
/// `akm-macos-aarch64`).
pub fn platform_asset_name() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "akm-linux-x86_64"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "akm-macos-aarch64"
    }
    // Fallback for unsupported platforms — the asset won't match any
    // release artifact, so the update will report "no compatible binary".
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
    )))]
    {
        "akm-unsupported"
    }
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
/// suffixes according to semver ordering:
/// - A stable release is always newer than a pre-release with the same base
///   (e.g., `1.0.0` > `1.0.0-alpha.1`).
/// - Pre-release identifiers are compared left-to-right: numeric segments
///   compare numerically, string segments compare lexically
///   (e.g., `alpha.5` < `alpha.6`, `alpha.6` < `beta.1`).
pub fn is_newer(current: &str, latest: &str) -> bool {
    fn split_pre(v: &str) -> (&str, Option<&str>) {
        match v.split_once('-') {
            Some((base, pre)) => (base, Some(pre)),
            None => (v, None),
        }
    }

    let parse = |base: &str| -> Vec<u64> {
        base.split('.')
            .filter_map(|s| s.parse::<u64>().ok())
            .collect()
    };

    let (current_base, current_pre) = split_pre(current);
    let (latest_base, latest_pre) = split_pre(latest);

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

    // Same base version — compare pre-release status
    match (current_pre, latest_pre) {
        // pre-release → stable is an upgrade
        (Some(_), None) => true,
        // stable → pre-release is a downgrade
        (None, Some(_)) => false,
        // both stable, same base → equal
        (None, None) => false,
        // both pre-release — compare identifiers per semver
        (Some(cur), Some(lat)) => compare_pre(cur, lat),
    }
}

/// Compare two pre-release strings per semver 2.0.0 §11.
///
/// Identifiers are split by `.` and compared left-to-right.
/// Numeric identifiers compare numerically; string identifiers
/// compare lexically; numeric < string when types differ.
fn compare_pre(current: &str, latest: &str) -> bool {
    let cur_ids: Vec<&str> = current.split('.').collect();
    let lat_ids: Vec<&str> = latest.split('.').collect();

    for (c, l) in cur_ids.iter().zip(lat_ids.iter()) {
        match (c.parse::<u64>(), l.parse::<u64>()) {
            // Both numeric — compare numerically
            (Ok(cn), Ok(ln)) => {
                if ln > cn {
                    return true;
                }
                if ln < cn {
                    return false;
                }
            }
            // Numeric < string per semver
            (Ok(_), Err(_)) => return true,
            (Err(_), Ok(_)) => return false,
            // Both strings — compare lexically
            (Err(_), Err(_)) => {
                if l > c {
                    return true;
                }
                if l < c {
                    return false;
                }
            }
        }
    }

    // All matched identifiers are equal — more identifiers = higher precedence
    lat_ids.len() > cur_ids.len()
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
    fn test_is_newer_prerelease_bump() {
        // The exact scenario: alpha.5 → alpha.6
        assert!(is_newer("1.0.0-alpha.5", "1.0.0-alpha.6"));
        assert!(!is_newer("1.0.0-alpha.6", "1.0.0-alpha.5"));
        assert!(!is_newer("1.0.0-alpha.5", "1.0.0-alpha.5"));
    }

    #[test]
    fn test_is_newer_prerelease_channel_progression() {
        // alpha < beta < rc < stable
        assert!(is_newer("1.0.0-alpha.1", "1.0.0-beta.1"));
        assert!(is_newer("1.0.0-beta.1", "1.0.0-rc.1"));
        assert!(is_newer("1.0.0-rc.1", "1.0.0"));
        assert!(!is_newer("1.0.0-beta.1", "1.0.0-alpha.1"));
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
