//! YAML frontmatter parser for SKILL.md and agent .md files.
//!
//! Frontmatter is YAML enclosed between two `---` markers at the start of
//! a Markdown file. This is the standard format used by the Agent Skills
//! specification.
//!
//! Bash equivalent: `_extract_fm_field()` at bin/akm:177

use crate::error::{Error, IoContext, Result};
use std::path::Path;

/// Parsed frontmatter fields from a spec markdown file.
///
/// Only the fields AKM cares about are extracted. Unknown YAML keys are
/// silently ignored (matching the Bash behavior).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frontmatter {
    /// The `name:` field. Required by the Agent Skills spec.
    pub name: Option<String>,
    /// The `description:` field. Required by the Agent Skills spec.
    pub description: Option<String>,
}

/// Extract the YAML frontmatter block from markdown content.
///
/// Returns `None` if no frontmatter block is found.
fn extract_yaml_block(content: &str) -> Option<String> {
    // Normalize line endings (matches Bash: sed 's/\r$//')
    let content = content.replace("\r\n", "\n");
    let content = content.trim_start();

    if !content.starts_with("---") {
        return None;
    }

    let after_first = match content.strip_prefix("---") {
        Some(rest) => {
            let rest = rest.strip_prefix('\n').unwrap_or(rest);
            rest
        }
        None => return None,
    };

    let mut yaml_lines = Vec::new();
    let mut found_end = false;
    for line in after_first.lines() {
        if line == "---" {
            found_end = true;
            break;
        }
        yaml_lines.push(line);
    }

    if !found_end {
        return None;
    }

    Some(yaml_lines.join("\n"))
}

impl Frontmatter {
    /// Parse frontmatter from markdown file content.
    ///
    /// Returns `Ok(Frontmatter)` with whatever fields are found.
    /// Returns `Ok(Frontmatter::default())` if no frontmatter block exists.
    pub fn parse(content: &str) -> Result<Self> {
        let yaml_str = match extract_yaml_block(content) {
            Some(s) => s,
            None => return Ok(Self::default()),
        };

        if yaml_str.trim().is_empty() {
            return Ok(Self::default());
        }

        let mut fm = Frontmatter::default();

        let mut current_key: Option<String> = None;
        let mut current_value_lines: Vec<String> = Vec::new();

        let flush = |key: &Option<String>, lines: &[String], fm: &mut Frontmatter| {
            if let Some(ref k) = key {
                let value = lines.join("\n").trim().to_string();
                let value = strip_yaml_quotes(&value);
                if !value.is_empty() {
                    match k.as_str() {
                        "name" => fm.name = Some(value),
                        "description" => fm.description = Some(value),
                        _ => {} // Ignore unknown fields
                    }
                }
            }
        };

        for line in yaml_str.lines() {
            // Check if this line starts a new key (not indented, has colon)
            if !line.starts_with(' ') && !line.starts_with('\t') {
                if let Some(colon_pos) = line.find(':') {
                    // Flush previous key
                    flush(&current_key, &current_value_lines, &mut fm);

                    let key = line[..colon_pos].trim().to_string();
                    let value_part = line[colon_pos + 1..].trim().to_string();

                    current_key = Some(key);
                    current_value_lines.clear();

                    // YAML block scalar indicators
                    if value_part == "|"
                        || value_part == ">"
                        || value_part == "|+"
                        || value_part == ">+"
                        || value_part == "|-"
                        || value_part == ">-"
                    {
                        // Multi-line value follows
                    } else if !value_part.is_empty() {
                        current_value_lines.push(value_part);
                    }
                    continue;
                }
            }

            // Continuation line for multi-line value
            if current_key.is_some() {
                current_value_lines.push(line.trim_start().to_string());
            }
        }

        // Flush last key
        flush(&current_key, &current_value_lines, &mut fm);

        Ok(fm)
    }

    /// Parse frontmatter from a file on disk.
    pub fn parse_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .io_context(format!("Reading frontmatter from {}", path.display()))?;
        Self::parse(&content)
    }

    /// Validate that required fields are present.
    pub fn require_name_and_description(&self, path: &Path) -> Result<()> {
        if self.name.is_none() {
            return Err(Error::FrontmatterMissing {
                field: "name".into(),
                path: path.to_path_buf(),
            });
        }
        if self.description.is_none() {
            return Err(Error::FrontmatterMissing {
                field: "description".into(),
                path: path.to_path_buf(),
            });
        }
        Ok(())
    }
}

/// Strip surrounding YAML quotes from a value.
///
/// Bash: `sed "s/^[\"']//; s/[\"']$//"`
fn strip_yaml_quotes(s: &str) -> String {
    let s = s.trim();
    for quote in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(quote).and_then(|s| s.strip_suffix(quote)) {
            return inner.to_string();
        }
    }
    s.to_string()
}
