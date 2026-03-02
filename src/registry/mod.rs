//! Registry abstraction — trait-based access to spec registries.
//!
//! AKM is a **client** of spec registries. It pulls specs from registries
//! into the local cold library. The `RegistrySource` trait abstracts the
//! transport mechanism (git, HTTP, local dir).
//!
//! Only `GitRegistry` is implemented at launch. Future backends (HTTP/REST,
//! local directory) implement the same trait — no sync logic changes needed.

pub mod git;

use crate::error::Result;
use std::path::Path;

/// Outcome of a registry pull operation.
///
/// The sync command uses this to decide what to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullOutcome {
    /// First-time fetch — registry was newly downloaded.
    Fetched,
    /// Existing cache was updated with latest changes.
    Updated,
}

/// Abstraction over a spec registry that AKM can pull from.
///
/// Design invariants:
/// - All methods are synchronous (no async runtime per spec).
/// - Implementations must be idempotent — `pull()` is safe to call repeatedly.
/// - No git-specific concepts leak through this trait. A future HTTP registry
///   or local directory registry would implement the same interface.
/// - The `cache_dir` is where the registry's content is materialized locally.
///   After a successful `pull()`, the caller can read `skills/`, `agents/`,
///   and `library.json` from this directory.
///
/// Spec reference: "RegistrySource trait with pull(), push(), is_available() methods"
pub trait RegistrySource {
    /// Human-readable name for display (e.g., "community", "personal").
    fn name(&self) -> &str;

    /// Synchronize the local cache with the remote registry.
    ///
    /// On first invocation (no local cache), downloads the full registry.
    /// On subsequent invocations, fetches only changes.
    ///
    /// Returns `Ok(PullOutcome)` on success.
    /// Returns `Err` if the sync fails — the caller should check `is_cached()`
    /// to determine whether a stale cached copy can be used as fallback.
    fn pull(&self) -> Result<PullOutcome>;

    /// Push local changes in the cache directory to the remote.
    ///
    /// Used by `akm skills publish` to push specs to the personal registry.
    /// Returns `Err` for read-only registries (e.g., community).
    fn push(&self) -> Result<()>;

    /// Check if the remote is configured and theoretically reachable.
    ///
    /// This is a lightweight check — it does NOT contact the remote.
    /// It verifies that the URL is non-empty and the configuration is valid.
    /// Used to decide whether to attempt a pull at all.
    fn is_available(&self) -> bool;

    /// Path to the local cache directory.
    ///
    /// After a successful `pull()`, this directory contains the registry
    /// contents (skills/, agents/, library.json). The sync command reads
    /// from here to populate the cold library.
    fn cache_dir(&self) -> &Path;

    /// Whether a local cached copy of the registry exists.
    ///
    /// When `pull()` fails, the sync command checks this to decide whether
    /// to continue with stale data or abort.
    fn is_cached(&self) -> bool;
}
