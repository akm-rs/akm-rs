//! GitHub URL parsing and Contents API client.
//!
//! Provides URL parsing for `github.com/owner/repo/tree/ref/path` URLs
//! and a client for the GitHub Contents API to recursively download
//! directory contents.

use crate::error::{Error, IoContext, Result};
use std::path::Path;

/// Parsed components of a GitHub URL pointing to a directory or file.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedGitHubUrl {
    /// Repository owner (user or org).
    pub owner: String,
    /// Repository name.
    pub repo: String,
    /// Git ref (branch, tag, or commit SHA).
    pub git_ref: String,
    /// Path within the repository (relative, no leading slash).
    pub path: String,
}

impl ParsedGitHubUrl {
    /// Build the GitHub Contents API URL for this parsed URL.
    ///
    /// Format: `https://api.github.com/repos/{owner}/{repo}/contents/{path}?ref={ref}`
    pub fn api_contents_url(&self) -> String {
        format!(
            "https://api.github.com/repos/{}/{}/contents/{}?ref={}",
            self.owner, self.repo, self.path, self.git_ref
        )
    }

    /// Extract the default skill ID from the URL path.
    ///
    /// Returns the last segment of the path (e.g., "my-skill" from "skills/my-skill").
    pub fn default_skill_id(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// Reconstruct the browsable GitHub URL (for storage in `source` field).
    pub fn browsable_url(&self) -> String {
        format!(
            "https://github.com/{}/{}/tree/{}/{}",
            self.owner, self.repo, self.git_ref, self.path
        )
    }
}

/// A single entry from the GitHub Contents API response.
///
/// Only the fields we need are deserialized; unknown fields are ignored
/// via serde's default behavior.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GitHubEntry {
    /// Entry name (filename or directory name).
    pub name: String,
    /// "file" or "dir".
    #[serde(rename = "type")]
    pub entry_type: String,
    /// Download URL for files (None for directories).
    pub download_url: Option<String>,
    /// Relative path within the repository.
    pub path: String,
}

/// Parse a GitHub URL into its components.
///
/// Supports two formats:
/// - `https://github.com/owner/repo/tree/ref/path` (directory URL)
/// - `https://github.com/owner/repo/blob/ref/path/SKILL.md` (file URL -> parent dir)
///
/// # Errors
///
/// - `ImportNotGithub` if the domain is not `github.com`
/// - `ImportInvalidUrl` if the URL structure doesn't match expected formats
pub fn parse_github_url(url: &str) -> Result<ParsedGitHubUrl> {
    let url_trimmed = url.trim_end_matches('/');

    let without_scheme = url_trimmed
        .strip_prefix("https://")
        .or_else(|| url_trimmed.strip_prefix("http://"))
        .ok_or_else(|| Error::ImportInvalidUrl {
            url: url.to_string(),
        })?;

    let (host, rest) = without_scheme
        .split_once('/')
        .ok_or_else(|| Error::ImportInvalidUrl {
            url: url.to_string(),
        })?;

    if host != "github.com" && host != "www.github.com" {
        return Err(Error::ImportNotGithub {
            url: url.to_string(),
        });
    }

    let parts: Vec<&str> = rest.splitn(4, '/').collect();

    if parts.len() < 4 {
        return Err(Error::ImportInvalidUrl {
            url: url.to_string(),
        });
    }

    let owner = parts[0].to_string();
    let repo = parts[1].to_string();
    let kind = parts[2];
    let ref_and_path = parts[3];

    if kind != "tree" && kind != "blob" {
        return Err(Error::ImportInvalidUrl {
            url: url.to_string(),
        });
    }

    if owner.is_empty() || repo.is_empty() || ref_and_path.is_empty() {
        return Err(Error::ImportInvalidUrl {
            url: url.to_string(),
        });
    }

    let (git_ref, path) = match ref_and_path.split_once('/') {
        Some((r, p)) => (r.to_string(), p.to_string()),
        None => {
            return Err(Error::ImportInvalidUrl {
                url: url.to_string(),
            });
        }
    };

    if git_ref.is_empty() || path.is_empty() {
        return Err(Error::ImportInvalidUrl {
            url: url.to_string(),
        });
    }

    let final_path = if kind == "blob" {
        if let Some(parent) = path.strip_suffix("/SKILL.md") {
            parent.to_string()
        } else if path == "SKILL.md" {
            return Err(Error::ImportInvalidUrl {
                url: url.to_string(),
            });
        } else {
            match path.rsplit_once('/') {
                Some((parent, _)) => parent.to_string(),
                None => {
                    return Err(Error::ImportInvalidUrl {
                        url: url.to_string(),
                    });
                }
            }
        }
    } else {
        path
    };

    Ok(ParsedGitHubUrl {
        owner,
        repo,
        git_ref,
        path: final_path,
    })
}

// ---------------------------------------------------------------------------
// GitHub API Client
// ---------------------------------------------------------------------------

/// Trait abstracting GitHub API calls for testability.
///
/// Production code uses [`GitHubHttpClient`]; tests can inject a mock
/// that returns pre-defined responses without network access.
pub trait GitHubClient {
    /// List directory contents via the GitHub Contents API.
    ///
    /// `subpath` is relative to the parsed URL's path. Empty string means
    /// the root directory of the parsed URL.
    fn list_contents(&self, parsed: &ParsedGitHubUrl, subpath: &str) -> Result<Vec<GitHubEntry>>;

    /// Download a single file by its download URL, returning the bytes.
    fn download_file(&self, download_url: &str) -> Result<Vec<u8>>;
}

/// Production GitHub API client using `ureq`.
///
/// Uses `GITHUB_TOKEN` env var for authentication if available.
/// Falls back to unauthenticated requests (60 req/hr rate limit).
pub struct GitHubHttpClient {
    agent: ureq::Agent,
}

impl Default for GitHubHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHubHttpClient {
    /// Create a new client with a 30-second timeout.
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build()
            .new_agent();
        Self { agent }
    }
}

impl GitHubClient for GitHubHttpClient {
    fn list_contents(&self, parsed: &ParsedGitHubUrl, subpath: &str) -> Result<Vec<GitHubEntry>> {
        let api_path = if subpath.is_empty() {
            parsed.path.clone()
        } else {
            format!("{}/{}", parsed.path, subpath)
        };

        let url = format!(
            "https://api.github.com/repos/{}/{}/contents/{}?ref={}",
            parsed.owner, parsed.repo, api_path, parsed.git_ref
        );

        let mut request = self
            .agent
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", &format!("akm/{}", env!("CARGO_PKG_VERSION")))
            .header("X-GitHub-Api-Version", "2022-11-28");

        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }

        let mut response = request.call().map_err(|e| match e {
            ureq::Error::StatusCode(code) => Error::ImportApiFailed {
                url: url.clone(),
                status: code,
                message: format!("HTTP {code}"),
            },
            _ => Error::ImportApiFailed {
                url: url.clone(),
                status: 0,
                message: format!("{e}"),
            },
        })?;

        let entries: Vec<GitHubEntry> =
            response
                .body_mut()
                .read_json()
                .map_err(|e| Error::ImportApiFailed {
                    url: url.clone(),
                    status: 0,
                    message: format!("Failed to parse API response: {e}"),
                })?;

        Ok(entries)
    }

    fn download_file(&self, download_url: &str) -> Result<Vec<u8>> {
        let mut request = self
            .agent
            .get(download_url)
            .header("User-Agent", &format!("akm/{}", env!("CARGO_PKG_VERSION")));

        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }

        let response = request.call().map_err(|e| Error::ImportDownloadFailed {
            url: download_url.to_string(),
            file: download_url.to_string(),
            reason: format!("{e}"),
        })?;

        let mut buf = Vec::new();
        use std::io::Read;
        response
            .into_body()
            .into_reader()
            .read_to_end(&mut buf)
            .map_err(|e| Error::ImportDownloadFailed {
                url: download_url.to_string(),
                file: download_url.to_string(),
                reason: format!("Read error: {e}"),
            })?;

        Ok(buf)
    }
}

// ---------------------------------------------------------------------------
// Recursive download
// ---------------------------------------------------------------------------

/// Download a GitHub directory recursively into a local directory.
///
/// Creates the directory structure on disk, preserving relative paths.
/// Returns the list of files downloaded (relative paths).
///
/// # Arguments
/// * `client` — GitHub API client (trait object for testability)
/// * `parsed` — Parsed GitHub URL
/// * `dest_dir` — Local destination directory (must exist)
pub fn download_directory(
    client: &dyn GitHubClient,
    parsed: &ParsedGitHubUrl,
    dest_dir: &Path,
) -> Result<Vec<String>> {
    let mut downloaded_files = Vec::new();
    download_directory_recursive(client, parsed, "", dest_dir, &mut downloaded_files)?;
    Ok(downloaded_files)
}

/// Internal recursive implementation.
fn download_directory_recursive(
    client: &dyn GitHubClient,
    parsed: &ParsedGitHubUrl,
    subpath: &str,
    dest_dir: &Path,
    downloaded_files: &mut Vec<String>,
) -> Result<()> {
    let entries = client.list_contents(parsed, subpath)?;

    for entry in &entries {
        let relative_path = if subpath.is_empty() {
            entry.name.clone()
        } else {
            format!("{}/{}", subpath, entry.name)
        };

        match entry.entry_type.as_str() {
            "file" => {
                let download_url =
                    entry
                        .download_url
                        .as_deref()
                        .ok_or_else(|| Error::ImportDownloadFailed {
                            url: parsed.browsable_url(),
                            file: relative_path.clone(),
                            reason: "No download URL provided by GitHub API".to_string(),
                        })?;

                let file_bytes = client.download_file(download_url)?;

                let dest_file = dest_dir.join(&relative_path);
                if let Some(parent) = dest_file.parent() {
                    std::fs::create_dir_all(parent)
                        .io_context(format!("Creating directory for {}", dest_file.display()))?;
                }

                std::fs::write(&dest_file, &file_bytes).io_context(format!(
                    "Writing downloaded file to {}",
                    dest_file.display()
                ))?;

                downloaded_files.push(relative_path);
            }
            "dir" => {
                download_directory_recursive(
                    client,
                    parsed,
                    &relative_path,
                    dest_dir,
                    downloaded_files,
                )?;
            }
            other => {
                eprintln!(
                    "Warning: Skipping unsupported entry type '{}' for '{}'",
                    other, entry.name
                );
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tree_url() {
        let parsed =
            parse_github_url("https://github.com/acme/skills-repo/tree/main/skills/my-skill")
                .unwrap();

        assert_eq!(parsed.owner, "acme");
        assert_eq!(parsed.repo, "skills-repo");
        assert_eq!(parsed.git_ref, "main");
        assert_eq!(parsed.path, "skills/my-skill");
    }

    #[test]
    fn parse_blob_url_skill_md() {
        let parsed = parse_github_url(
            "https://github.com/acme/skills-repo/blob/main/skills/my-skill/SKILL.md",
        )
        .unwrap();

        assert_eq!(parsed.owner, "acme");
        assert_eq!(parsed.repo, "skills-repo");
        assert_eq!(parsed.git_ref, "main");
        assert_eq!(parsed.path, "skills/my-skill");
    }

    #[test]
    fn parse_blob_url_other_file() {
        let parsed =
            parse_github_url("https://github.com/acme/repo/blob/main/skills/my-skill/README.md")
                .unwrap();

        assert_eq!(parsed.path, "skills/my-skill");
    }

    #[test]
    fn parse_trailing_slash() {
        let parsed =
            parse_github_url("https://github.com/acme/repo/tree/main/skills/my-skill/").unwrap();

        assert_eq!(parsed.path, "skills/my-skill");
    }

    #[test]
    fn parse_http_scheme() {
        let parsed =
            parse_github_url("http://github.com/acme/repo/tree/main/skills/my-skill").unwrap();

        assert_eq!(parsed.owner, "acme");
    }

    #[test]
    fn parse_www_github() {
        let parsed =
            parse_github_url("https://www.github.com/acme/repo/tree/main/skills/my-skill").unwrap();

        assert_eq!(parsed.owner, "acme");
    }

    #[test]
    fn reject_non_github() {
        let err = parse_github_url("https://gitlab.com/acme/repo/tree/main/skills/x").unwrap_err();
        assert!(matches!(err, Error::ImportNotGithub { .. }));
    }

    #[test]
    fn reject_raw_githubusercontent() {
        let err =
            parse_github_url("https://raw.githubusercontent.com/acme/repo/main/skills/x/SKILL.md")
                .unwrap_err();
        assert!(matches!(err, Error::ImportNotGithub { .. }));
    }

    #[test]
    fn reject_no_path() {
        let err = parse_github_url("https://github.com/acme/repo").unwrap_err();
        assert!(matches!(err, Error::ImportInvalidUrl { .. }));
    }

    #[test]
    fn reject_no_scheme() {
        let err = parse_github_url("github.com/acme/repo/tree/main/skills/x").unwrap_err();
        assert!(matches!(err, Error::ImportInvalidUrl { .. }));
    }

    #[test]
    fn reject_ref_only_no_path() {
        let err = parse_github_url("https://github.com/acme/repo/tree/main").unwrap_err();
        assert!(matches!(err, Error::ImportInvalidUrl { .. }));
    }

    #[test]
    fn default_skill_id_last_segment() {
        let parsed =
            parse_github_url("https://github.com/acme/repo/tree/main/skills/my-skill").unwrap();

        assert_eq!(parsed.default_skill_id(), "my-skill");
    }

    #[test]
    fn default_skill_id_single_segment() {
        let parsed = parse_github_url("https://github.com/acme/repo/tree/main/my-skill").unwrap();

        assert_eq!(parsed.default_skill_id(), "my-skill");
    }

    #[test]
    fn api_contents_url_format() {
        let parsed = parse_github_url("https://github.com/acme/repo/tree/v1.0/skills/tdd").unwrap();

        assert_eq!(
            parsed.api_contents_url(),
            "https://api.github.com/repos/acme/repo/contents/skills/tdd?ref=v1.0"
        );
    }

    #[test]
    fn browsable_url_format() {
        let parsed = parse_github_url("https://github.com/acme/repo/tree/main/skills/tdd").unwrap();

        assert_eq!(
            parsed.browsable_url(),
            "https://github.com/acme/repo/tree/main/skills/tdd"
        );
    }

    // -----------------------------------------------------------------------
    // Mock client + download tests
    // -----------------------------------------------------------------------

    /// Mock GitHub client for testing download logic without network access.
    struct MockGitHubClient {
        /// Map from subpath to entries.
        entries: std::collections::HashMap<String, Vec<GitHubEntry>>,
        /// Map from download_url to file content bytes.
        files: std::collections::HashMap<String, Vec<u8>>,
    }

    impl MockGitHubClient {
        fn new() -> Self {
            Self {
                entries: std::collections::HashMap::new(),
                files: std::collections::HashMap::new(),
            }
        }

        /// Add a directory listing response for a given subpath.
        fn add_listing(&mut self, subpath: &str, entries: Vec<GitHubEntry>) {
            self.entries.insert(subpath.to_string(), entries);
        }

        /// Add a downloadable file.
        fn add_file(&mut self, download_url: &str, content: &[u8]) {
            self.files
                .insert(download_url.to_string(), content.to_vec());
        }
    }

    impl GitHubClient for MockGitHubClient {
        fn list_contents(
            &self,
            _parsed: &ParsedGitHubUrl,
            subpath: &str,
        ) -> Result<Vec<GitHubEntry>> {
            self.entries
                .get(subpath)
                .cloned()
                .ok_or_else(|| Error::ImportApiFailed {
                    url: format!("mock://subpath/{subpath}"),
                    status: 404,
                    message: "Not found in mock".to_string(),
                })
        }

        fn download_file(&self, download_url: &str) -> Result<Vec<u8>> {
            self.files
                .get(download_url)
                .cloned()
                .ok_or_else(|| Error::ImportDownloadFailed {
                    url: download_url.to_string(),
                    file: download_url.to_string(),
                    reason: "Not found in mock".to_string(),
                })
        }
    }

    fn make_file_entry(name: &str, download_url: &str) -> GitHubEntry {
        GitHubEntry {
            name: name.to_string(),
            entry_type: "file".to_string(),
            download_url: Some(download_url.to_string()),
            path: name.to_string(),
        }
    }

    fn make_dir_entry(name: &str) -> GitHubEntry {
        GitHubEntry {
            name: name.to_string(),
            entry_type: "dir".to_string(),
            download_url: None,
            path: name.to_string(),
        }
    }

    #[test]
    fn download_directory_flat() {
        let mut client = MockGitHubClient::new();

        client.add_listing(
            "",
            vec![
                make_file_entry("SKILL.md", "https://example.com/SKILL.md"),
                make_file_entry("README.md", "https://example.com/README.md"),
            ],
        );
        client.add_file(
            "https://example.com/SKILL.md",
            b"---\nname: Test\ndescription: A test\n---\nContent",
        );
        client.add_file("https://example.com/README.md", b"# README");

        let parsed =
            parse_github_url("https://github.com/acme/repo/tree/main/skills/my-skill").unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let files = download_directory(&client, &parsed, tmp.path()).unwrap();

        assert_eq!(files.len(), 2);
        assert!(files.contains(&"SKILL.md".to_string()));
        assert!(files.contains(&"README.md".to_string()));
        assert!(tmp.path().join("SKILL.md").is_file());
        assert!(tmp.path().join("README.md").is_file());
    }

    #[test]
    fn download_directory_recursive_with_subdirs() {
        let mut client = MockGitHubClient::new();

        // Root listing
        client.add_listing(
            "",
            vec![
                make_file_entry("SKILL.md", "https://example.com/SKILL.md"),
                make_dir_entry("references"),
            ],
        );
        // Subdirectory listing
        client.add_listing(
            "references",
            vec![make_file_entry("ref.md", "https://example.com/ref.md")],
        );
        client.add_file(
            "https://example.com/SKILL.md",
            b"---\nname: Test\ndescription: A test\n---\nContent",
        );
        client.add_file("https://example.com/ref.md", b"# Reference");

        let parsed =
            parse_github_url("https://github.com/acme/repo/tree/main/skills/my-skill").unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let files = download_directory(&client, &parsed, tmp.path()).unwrap();

        assert_eq!(files.len(), 2);
        assert!(files.contains(&"SKILL.md".to_string()));
        assert!(files.contains(&"references/ref.md".to_string()));
        assert!(tmp.path().join("SKILL.md").is_file());
        assert!(tmp.path().join("references").join("ref.md").is_file());
    }

    #[test]
    fn download_directory_empty() {
        let mut client = MockGitHubClient::new();
        client.add_listing("", vec![]);

        let parsed =
            parse_github_url("https://github.com/acme/repo/tree/main/skills/my-skill").unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let files = download_directory(&client, &parsed, tmp.path()).unwrap();

        assert!(files.is_empty());
    }

    #[test]
    fn download_directory_api_error_propagates() {
        let client = MockGitHubClient::new(); // no entries registered

        let parsed =
            parse_github_url("https://github.com/acme/repo/tree/main/skills/my-skill").unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let result = download_directory(&client, &parsed, tmp.path());

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::ImportApiFailed { .. }));
    }

    #[test]
    fn download_directory_skips_unsupported_types() {
        let mut client = MockGitHubClient::new();

        client.add_listing(
            "",
            vec![
                make_file_entry("SKILL.md", "https://example.com/SKILL.md"),
                GitHubEntry {
                    name: "submodule".to_string(),
                    entry_type: "submodule".to_string(),
                    download_url: None,
                    path: "submodule".to_string(),
                },
            ],
        );
        client.add_file(
            "https://example.com/SKILL.md",
            b"---\nname: Test\ndescription: Test\n---\nContent",
        );

        let parsed =
            parse_github_url("https://github.com/acme/repo/tree/main/skills/my-skill").unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let files = download_directory(&client, &parsed, tmp.path()).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0], "SKILL.md");
    }

    #[test]
    fn download_file_missing_url_errors() {
        let mut client = MockGitHubClient::new();

        client.add_listing(
            "",
            vec![GitHubEntry {
                name: "SKILL.md".to_string(),
                entry_type: "file".to_string(),
                download_url: None, // missing!
                path: "SKILL.md".to_string(),
            }],
        );

        let parsed =
            parse_github_url("https://github.com/acme/repo/tree/main/skills/my-skill").unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let result = download_directory(&client, &parsed, tmp.path());

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            Error::ImportDownloadFailed { .. }
        ));
    }

    #[test]
    fn github_entry_deserialization() {
        let json = r#"[
            {
                "name": "SKILL.md",
                "type": "file",
                "download_url": "https://raw.githubusercontent.com/acme/repo/main/skills/tdd/SKILL.md",
                "path": "skills/tdd/SKILL.md",
                "size": 1234,
                "sha": "abc123"
            },
            {
                "name": "references",
                "type": "dir",
                "download_url": null,
                "path": "skills/tdd/references",
                "size": 0,
                "sha": "def456"
            }
        ]"#;

        let entries: Vec<GitHubEntry> = serde_json::from_str(json).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "SKILL.md");
        assert_eq!(entries[0].entry_type, "file");
        assert!(entries[0].download_url.is_some());
        assert_eq!(entries[1].name, "references");
        assert_eq!(entries[1].entry_type, "dir");
        assert!(entries[1].download_url.is_none());
    }
}
