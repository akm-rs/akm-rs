//! Download and atomically replace the AKM binary.
//!
//! The update flow:
//! 1. Resolve the download URL (from cache, or fetch fresh release info)
//! 2. Download the binary to a temporary file in the same directory as self
//! 3. Set executable permissions on the temp file
//! 4. Atomically rename the temp file over the current binary
//!
//! Placing the temp file in the same directory guarantees `rename(2)` is
//! atomic (same filesystem). If anything fails, the temp file is cleaned up
//! and the original binary is untouched.

use crate::config::UpdateConfig;
use crate::error::{Error, Result};
use crate::paths::Paths;
use crate::update::version_check;
use crate::update::{is_newer, normalize_version, CachedCheck, CURRENT_VERSION};

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Minimum valid binary size (64 KB). Anything smaller is almost certainly
/// corrupt or an error page.
const MIN_BINARY_SIZE: u64 = 64 * 1024;

/// Resolve the download URL for the latest release.
///
/// First checks the cache; if the cache has a download URL and its version
/// is newer than current, uses that. Otherwise fetches fresh release info.
fn resolve_download_url(config: &UpdateConfig, paths: &Paths) -> Result<(String, String)> {
    // Try cache first
    if let Some(cached) = version_check::read_cache(paths) {
        let latest = normalize_version(&cached.latest_version);
        if is_newer(normalize_version(CURRENT_VERSION), latest) {
            if let Some(ref url) = cached.download_url {
                return Ok((url.clone(), latest.to_string()));
            }
        }
    }

    // Fetch fresh
    let release = version_check::fetch_latest_release(&config.url, Duration::from_secs(10))
        .map_err(|e| Error::UpdateCheck { message: e })?;

    let latest = normalize_version(&release.tag_name).to_string();
    let url = release.download_url.ok_or_else(|| Error::UpdateDownload {
        url: config.url.clone(),
        message: "No compatible binary asset found in the release. \
                  Expected a Linux x86_64 binary."
            .to_string(),
    })?;

    Ok((url, latest))
}

/// Get the path of the currently running binary.
///
/// Uses `std::env::current_exe()` and resolves symlinks to find
/// the actual binary on disk.
fn self_exe_path() -> Result<PathBuf> {
    std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| Error::UpdateSelfPath {
            message: e.to_string(),
        })
}

/// Download a file from a URL to the specified path.
///
/// Streams the response body to disk. Uses a 60-second timeout.
fn download_to_file(url: &str, dest: &Path) -> Result<()> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .new_agent();

    let response = agent
        .get(url)
        .header("User-Agent", &format!("akm/{}", CURRENT_VERSION))
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(|e| Error::UpdateDownload {
            url: url.to_string(),
            message: format!("HTTP request failed: {e}"),
        })?;

    let mut file = std::fs::File::create(dest).map_err(|e| Error::UpdateReplace {
        path: dest.to_path_buf(),
        source: e,
    })?;

    let mut reader = response.into_body().into_reader();
    std::io::copy(&mut reader, &mut file).map_err(|e| Error::UpdateReplace {
        path: dest.to_path_buf(),
        source: e,
    })?;

    Ok(())
}

/// Validate that the downloaded file looks like a valid binary.
fn validate_binary(path: &Path) -> Result<()> {
    let metadata = std::fs::metadata(path).map_err(|e| Error::UpdateReplace {
        path: path.to_path_buf(),
        source: e,
    })?;

    if metadata.len() < MIN_BINARY_SIZE {
        return Err(Error::UpdateInvalidBinary {
            reason: format!(
                "file is only {} bytes (minimum: {} bytes)",
                metadata.len(),
                MIN_BINARY_SIZE
            ),
        });
    }

    // Check for ELF magic bytes on Linux
    let mut file = std::fs::File::open(path).map_err(|e| Error::UpdateReplace {
        path: path.to_path_buf(),
        source: e,
    })?;
    let mut magic = [0u8; 4];
    use std::io::Read;
    file.read_exact(&mut magic)
        .map_err(|e| Error::UpdateReplace {
            path: path.to_path_buf(),
            source: e,
        })?;

    if magic != [0x7f, b'E', b'L', b'F'] {
        return Err(Error::UpdateInvalidBinary {
            reason: "file does not appear to be a valid ELF binary".to_string(),
        });
    }

    Ok(())
}

/// Set the executable permission bit on a file (Unix only).
#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .map_err(|e| Error::UpdateReplace {
            path: path.to_path_buf(),
            source: e,
        })?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).map_err(|e| Error::UpdateReplace {
        path: path.to_path_buf(),
        source: e,
    })
}

/// Download the latest AKM binary and atomically replace the current one.
///
/// # Steps
///
/// 1. Resolve download URL (from cache or fresh API call)
/// 2. Determine the path of the running binary (`std::env::current_exe()`)
/// 3. Download to a temp file in the same directory (ensures same filesystem for atomic rename)
/// 4. Validate the downloaded binary (size check, ELF magic bytes)
/// 5. Set executable permissions
/// 6. Atomically rename temp file → current binary path
/// 7. Update the cache with the new version
///
/// If any step fails, the temp file is cleaned up and the original binary
/// is left untouched.
pub fn download_and_replace(config: &UpdateConfig, paths: &Paths) -> Result<()> {
    let current = normalize_version(CURRENT_VERSION);

    // Fast path: if a fresh cache says we're already on latest, skip network.
    if let Some(cached) = version_check::read_cache(paths) {
        if version_check::cache_is_fresh(&cached, config.check_interval) {
            let latest = normalize_version(&cached.latest_version);
            if !is_newer(current, latest) {
                println!("Already up to date (version {current}).");
                return Ok(());
            }
        }
    }

    // Step 1: Resolve URL (may fetch from network)
    let (url, latest_version) = resolve_download_url(config, paths)?;

    if !is_newer(current, &latest_version) {
        println!("Already up to date (version {current}).");
        return Ok(());
    }

    println!("Downloading akm {latest_version}...");

    // Step 2: Determine self path
    let self_path = self_exe_path()?;
    let self_dir = self_path.parent().ok_or_else(|| Error::UpdateSelfPath {
        message: "Binary has no parent directory".to_string(),
    })?;

    // Step 3: Download to temp file (same dir for atomic rename)
    let temp_path = self_dir.join(format!(".akm-update-{}", std::process::id()));

    // RAII cleanup guard — removes temp file on drop unless defused
    let mut cleanup = TempFileCleanup::new(temp_path.clone());

    download_to_file(&url, &temp_path)?;

    // Step 4: Validate
    validate_binary(&temp_path)?;

    // Step 5: Set executable permissions
    #[cfg(unix)]
    set_executable(&temp_path)?;

    // Step 6: Atomic rename
    std::fs::rename(&temp_path, &self_path).map_err(|e| Error::UpdateReplace {
        path: self_path.clone(),
        source: e,
    })?;

    // Step 7: Update cache to reflect new version
    let cached = CachedCheck {
        checked_at: version_check::now_secs(),
        latest_version: latest_version.clone(),
        download_url: Some(url),
    };
    version_check::write_cache(paths, &cached);

    println!("Updated akm: {current} → {latest_version}");
    println!("Restart your shell or run `akm --version` to verify.");

    // Defuse the cleanup guard — rename succeeded, don't delete the new binary
    cleanup.defuse();

    Ok(())
}

/// RAII guard to clean up the temp file if an error occurs.
///
/// On drop, removes the temp file unless `defuse()` was called.
struct TempFileCleanup {
    path: PathBuf,
    defused: bool,
}

impl TempFileCleanup {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            defused: false,
        }
    }

    /// Prevent cleanup on drop (call after successful rename).
    fn defuse(&mut self) {
        self.defused = true;
    }
}

impl Drop for TempFileCleanup {
    fn drop(&mut self) {
        if !self.defused {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
