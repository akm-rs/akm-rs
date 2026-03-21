//! Background version check with caching.
//!
//! On every AKM invocation (when `auto_check` is enabled), a background
//! thread checks the GitHub Releases API for a newer version. Results are
//! cached to `$XDG_CACHE_HOME/akm/last-update-check.json` and only one
//! check per `check_interval` is performed.
//!
//! The check is non-blocking: the main command runs immediately, and the
//! thread is joined at the end to print a one-liner notice if an update
//! is available.

use crate::config::UpdateConfig;
use crate::paths::Paths;
use crate::update::{
    is_newer, normalize_version, platform_asset_name, CachedCheck, ReleaseInfo, CURRENT_VERSION,
};

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Result from the background version check thread.
///
/// Returned via an `mpsc::Receiver` so the main thread can retrieve it
/// after command execution completes.
#[derive(Debug)]
pub enum CheckResult {
    /// An update is available.
    UpdateAvailable {
        /// The latest version string (normalized, no "v" prefix).
        latest: String,
    },
    /// Already up to date.
    UpToDate,
    /// Check was skipped (cache is fresh or auto_check disabled).
    Skipped,
    /// Check failed (network error, parse error, etc.). Silently ignored.
    Failed(String),
}

/// Trait abstracting HTTP fetching for testability.
///
/// Production code uses `UreqFetcher`; tests inject a mock that returns
/// pre-defined responses without network access.
pub trait ReleaseFetcher: Send + 'static {
    /// Fetch the latest release info from the given URL.
    fn fetch(&self, url: &str) -> std::result::Result<ReleaseInfo, String>;
}

/// Production HTTP fetcher using `ureq` 3.x.
///
/// Creates a per-request agent with the configured timeout.
#[derive(Clone)]
pub struct UreqFetcher {
    /// HTTP request timeout.
    timeout: Duration,
}

impl UreqFetcher {
    /// Create a new fetcher with the given timeout.
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl Default for UreqFetcher {
    fn default() -> Self {
        Self::new(Duration::from_secs(10))
    }
}

impl ReleaseFetcher for UreqFetcher {
    fn fetch(&self, url: &str) -> std::result::Result<ReleaseInfo, String> {
        fetch_latest_release(url, self.timeout)
    }
}

/// Read the cached version check result from disk.
///
/// Returns `None` if the cache file doesn't exist or can't be parsed.
pub(crate) fn read_cache(paths: &Paths) -> Option<CachedCheck> {
    let cache_path = paths.update_check_cache();
    let content = std::fs::read_to_string(&cache_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Write a version check result to the cache file.
///
/// Creates the parent directory if needed. Errors are silently ignored
/// (the cache is advisory — a failed write just means we'll re-check next time).
pub(crate) fn write_cache(paths: &Paths, check: &CachedCheck) {
    let cache_path = paths.update_check_cache();
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(check) {
        let _ = std::fs::write(&cache_path, json);
    }
}

/// Get the current unix timestamp in seconds.
pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Check whether the cache is still fresh (within the check interval).
///
/// Handles forward clock skew: if `checked_at` is in the future (clock was
/// corrected backward), the cache is treated as stale.
pub(crate) fn cache_is_fresh(cached: &CachedCheck, check_interval: u64) -> bool {
    let now = now_secs();
    if cached.checked_at > now {
        return false; // Future timestamp → stale (clock was corrected)
    }
    (now - cached.checked_at) < check_interval
}

/// Query the GitHub Releases API (or custom URL) for the latest release.
///
/// Parses the JSON response to extract `tag_name` and the download URL
/// for the Linux x86_64 binary asset.
pub(crate) fn fetch_latest_release(
    url: &str,
    timeout: Duration,
) -> std::result::Result<ReleaseInfo, String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .new_agent();

    let mut request = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", &format!("akm/{}", CURRENT_VERSION))
        .header("X-GitHub-Api-Version", "2022-11-28");

    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }

    let mut response = request.call().map_err(|e| match e {
        ureq::Error::StatusCode(403) => "GitHub API rate limit exceeded or access forbidden. \
                 Try again later, or set GITHUB_TOKEN in your environment."
            .to_string(),
        ureq::Error::StatusCode(404) => "No releases found for this repository.".to_string(),
        _ => format!("HTTP request failed: {e}"),
    })?;

    let body: serde_json::Value = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("Failed to parse JSON response: {e}"))?;

    let tag_name = body["tag_name"]
        .as_str()
        .ok_or_else(|| "Missing 'tag_name' in release response".to_string())?
        .to_string();

    // Find the binary asset matching the current platform
    let expected_asset = platform_asset_name();
    let download_url = body["assets"].as_array().and_then(|assets| {
        assets.iter().find_map(|asset| {
            let name = asset["name"].as_str().unwrap_or("");
            if name == expected_asset || name == "akm" {
                asset["browser_download_url"].as_str().map(String::from)
            } else {
                None
            }
        })
    });

    Ok(ReleaseInfo {
        tag_name,
        download_url,
        name: body["name"].as_str().map(String::from),
    })
}

/// Spawn a background thread that checks for updates.
///
/// Returns a `Receiver<CheckResult>` that the main thread should receive from
/// after the primary command completes. If `auto_check` is disabled, the
/// returned receiver immediately yields `CheckResult::Skipped`.
pub fn spawn_background_check(config: &UpdateConfig, paths: &Paths) -> mpsc::Receiver<CheckResult> {
    spawn_background_check_with(config, paths, UreqFetcher::default())
}

/// Spawn a background version check with an injectable fetcher (for testing).
pub fn spawn_background_check_with<F: ReleaseFetcher>(
    config: &UpdateConfig,
    paths: &Paths,
    fetcher: F,
) -> mpsc::Receiver<CheckResult> {
    let (tx, rx) = mpsc::channel();

    if !config.auto_check {
        let _ = tx.send(CheckResult::Skipped);
        return rx;
    }

    let url = config.url.clone();
    let check_interval = config.check_interval;
    let paths_clone = paths.clone();

    thread::spawn(move || {
        let result = run_check_with(&paths_clone, &url, check_interval, &fetcher);
        let _ = tx.send(result);
    });

    rx
}

/// Core check logic, runs inside the background thread.
///
/// Separated from `spawn_background_check` for testability — this function
/// can be called directly in tests without spawning a thread.
pub fn run_check(paths: &Paths, url: &str, check_interval: u64) -> CheckResult {
    run_check_with(paths, url, check_interval, &UreqFetcher::default())
}

/// Core check logic with injectable fetcher.
///
/// Used by both production code (with `UreqFetcher`) and tests (with mocks).
pub fn run_check_with<F: ReleaseFetcher>(
    paths: &Paths,
    url: &str,
    check_interval: u64,
    fetcher: &F,
) -> CheckResult {
    // Step 1: Check cache freshness
    if let Some(cached) = read_cache(paths) {
        if cache_is_fresh(&cached, check_interval) {
            let latest = normalize_version(&cached.latest_version);
            let current = normalize_version(CURRENT_VERSION);
            return if is_newer(current, latest) {
                CheckResult::UpdateAvailable {
                    latest: latest.to_string(),
                }
            } else {
                CheckResult::UpToDate
            };
        }
    }

    // Step 2: Fetch from network via the injected fetcher
    let release = match fetcher.fetch(url) {
        Ok(r) => r,
        Err(e) => return CheckResult::Failed(e),
    };

    let latest = normalize_version(&release.tag_name).to_string();

    // Step 3: Write to cache
    let cached = CachedCheck {
        checked_at: now_secs(),
        latest_version: latest.clone(),
        download_url: release.download_url,
    };
    write_cache(paths, &cached);

    // Step 4: Compare versions
    let current = normalize_version(CURRENT_VERSION);
    if is_newer(current, &latest) {
        CheckResult::UpdateAvailable { latest }
    } else {
        CheckResult::UpToDate
    }
}

/// Print a one-liner update notice to stderr if an update is available.
///
/// Called from `main()` after the primary command has completed and its
/// output has been flushed. Uses stderr so it doesn't pollute piped output.
///
/// Respects non-TTY: only prints if stderr is a terminal.
pub fn print_update_notice(rx: mpsc::Receiver<CheckResult>) {
    // Wait up to 5 seconds for the background thread to complete.
    let result = rx.recv_timeout(std::time::Duration::from_secs(5));

    if let Ok(CheckResult::UpdateAvailable { latest }) = result {
        use std::io::IsTerminal;
        if std::io::stderr().is_terminal() {
            let current = normalize_version(CURRENT_VERSION);
            eprintln!(
                "\x1b[33mA new version of akm is available: {} → {} — run `akm update` to install.\x1b[0m",
                current, latest
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{UpdateConfig, DEFAULT_UPDATE_URL};
    use crate::update::{CachedCheck, ReleaseInfo};

    /// Mock fetcher for testing without network access.
    struct MockFetcher {
        result: std::result::Result<ReleaseInfo, String>,
    }

    impl MockFetcher {
        fn ok(tag: &str, url: Option<&str>) -> Self {
            Self {
                result: Ok(ReleaseInfo {
                    tag_name: tag.to_string(),
                    download_url: url.map(|s| s.to_string()),
                    name: None,
                }),
            }
        }

        fn err(msg: &str) -> Self {
            Self {
                result: Err(msg.to_string()),
            }
        }
    }

    impl ReleaseFetcher for MockFetcher {
        fn fetch(&self, _url: &str) -> std::result::Result<ReleaseInfo, String> {
            self.result.clone()
        }
    }

    // --- Cache tests ---

    #[test]
    fn test_cache_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(dir.path(), dir.path(), dir.path(), dir.path());

        let check = CachedCheck {
            checked_at: 1000,
            latest_version: "1.2.0".to_string(),
            download_url: Some("https://example.com/akm".to_string()),
        };

        write_cache(&paths, &check);
        let read_back = read_cache(&paths).unwrap();

        assert_eq!(read_back.checked_at, 1000);
        assert_eq!(read_back.latest_version, "1.2.0");
        assert_eq!(
            read_back.download_url.as_deref(),
            Some("https://example.com/akm")
        );
    }

    #[test]
    fn test_cache_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(dir.path(), dir.path(), dir.path(), dir.path());
        assert!(read_cache(&paths).is_none());
    }

    #[test]
    fn test_cache_is_fresh_within_interval() {
        let cached = CachedCheck {
            checked_at: now_secs() - 100,
            latest_version: "1.0.0".to_string(),
            download_url: None,
        };
        assert!(cache_is_fresh(&cached, 86400));
    }

    #[test]
    fn test_cache_is_stale_past_interval() {
        let cached = CachedCheck {
            checked_at: now_secs() - 90000,
            latest_version: "1.0.0".to_string(),
            download_url: None,
        };
        assert!(!cache_is_fresh(&cached, 86400));
    }

    #[test]
    fn test_cache_future_timestamp_is_stale() {
        let cached = CachedCheck {
            checked_at: now_secs() + 10000,
            latest_version: "1.0.0".to_string(),
            download_url: None,
        };
        assert!(!cache_is_fresh(&cached, 86400));
    }

    #[test]
    fn test_cache_zero_interval_always_stale() {
        let cached = CachedCheck {
            checked_at: now_secs(),
            latest_version: "1.0.0".to_string(),
            download_url: None,
        };
        assert!(!cache_is_fresh(&cached, 0));
    }

    // --- run_check_with tests (using MockFetcher) ---

    #[test]
    fn test_run_check_stale_cache_fetches_and_updates() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(dir.path(), dir.path(), dir.path(), dir.path());

        let fetcher = MockFetcher::ok("v99.0.0", Some("https://example.com/akm-linux"));
        let result = run_check_with(&paths, "https://example.com", 86400, &fetcher);

        assert!(matches!(result, CheckResult::UpdateAvailable { latest } if latest == "99.0.0"));

        // Cache should now be written
        let cached = read_cache(&paths).unwrap();
        assert_eq!(cached.latest_version, "99.0.0");
    }

    #[test]
    fn test_run_check_network_failure_returns_failed() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(dir.path(), dir.path(), dir.path(), dir.path());

        let fetcher = MockFetcher::err("connection refused");
        let result = run_check_with(&paths, "https://example.com", 86400, &fetcher);

        assert!(matches!(result, CheckResult::Failed(msg) if msg == "connection refused"));
    }

    #[test]
    fn test_run_check_fresh_cache_skips_network() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(dir.path(), dir.path(), dir.path(), dir.path());

        // Pre-populate cache with fresh result
        let cached = CachedCheck {
            checked_at: now_secs(),
            latest_version: CURRENT_VERSION.to_string(),
            download_url: None,
        };
        write_cache(&paths, &cached);

        // The fetcher should NOT be called (cache is fresh)
        let fetcher = MockFetcher::err("should not be called");
        let result = run_check_with(&paths, "https://example.com", 86400, &fetcher);

        assert!(matches!(result, CheckResult::UpToDate));
    }

    // --- Config tests ---

    #[test]
    fn test_update_config_defaults() {
        let config = UpdateConfig::default();
        assert_eq!(config.url, DEFAULT_UPDATE_URL);
        assert_eq!(config.check_interval, 86400);
        assert!(config.auto_check);
    }

    #[test]
    fn test_config_with_update_section_roundtrip() {
        use crate::config::Config;
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(dir.path(), dir.path(), dir.path(), dir.path());

        let mut config = Config::default();
        config.update.url = "https://custom.example.com/releases/latest".to_string();
        config.update.check_interval = 3600;
        config.update.auto_check = false;
        config.save(&paths).unwrap();

        let loaded = Config::load(&paths).unwrap();
        assert_eq!(
            loaded.update.url,
            "https://custom.example.com/releases/latest"
        );
        assert_eq!(loaded.update.check_interval, 3600);
        assert!(!loaded.update.auto_check);
    }

    #[test]
    fn test_config_missing_update_section_uses_defaults() {
        use crate::config::Config;
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(dir.path(), dir.path(), dir.path(), dir.path());

        let config_file = paths.config_file();
        std::fs::create_dir_all(config_file.parent().unwrap()).unwrap();
        std::fs::write(&config_file, "features = [\"skills\"]\n").unwrap();

        let loaded = Config::load(&paths).unwrap();
        assert_eq!(loaded.update.url, DEFAULT_UPDATE_URL);
        assert_eq!(loaded.update.check_interval, 86400);
        assert!(loaded.update.auto_check);
    }
}
