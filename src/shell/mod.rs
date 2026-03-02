//! Shell init generation and .bashrc wiring.
//!
//! The `akm-init.sh` script is compiled into the binary and written to disk
//! by `akm setup`. It provides the session lifecycle (staging dir creation,
//! artifact pull/push) and tool wrappers (claude, copilot, opencode) that
//! pass `--add-dir` flags to the underlying tools.
//!
//! Bash equivalents:
//! - Template: `shell/akm-init.sh` (entire file)
//! - Install: `install.sh:36–38` (cp to data dir)
//! - .bashrc wiring: `_patch_bashrc()` at bin/akm:93

use crate::error::{Error, IoContext, Result};
use crate::paths::Paths;

/// The akm-init.sh template, compiled into the binary.
///
/// This is the FULL shell init script that handles session lifecycle,
/// tool wrappers, and domain integration. It is written verbatim to
/// `$XDG_DATA_HOME/akm/shell/akm-init.sh` by `akm setup`.
const SHELL_INIT_TEMPLATE: &str = include_str!("akm-init.sh");

/// The tools.json definitions, compiled into the binary.
///
/// Written to `$XDG_DATA_HOME/akm/tools.json` by `akm setup`.
/// Used by the shell init to generate tool wrappers, and by the CLI
/// for tool directory resolution.
const TOOLS_JSON: &str = include_str!("tools.json");

/// Marker lines for .bashrc integration block.
const BASHRC_MARKER_START: &str = "# >>> akm >>>";
const BASHRC_MARKER_END: &str = "# <<< akm <<<";

/// Install the shell init script to the data directory.
///
/// Creates `$XDG_DATA_HOME/akm/shell/akm-init.sh` from the embedded template.
/// Idempotent — overwrites any existing file (picks up new version on update).
///
/// Bash equivalent: `install.sh:36–37`
pub fn install_shell_init(paths: &Paths) -> Result<()> {
    let shell_dir = paths.data_dir().join("shell");
    std::fs::create_dir_all(&shell_dir).io_context(format!(
        "Creating shell init directory {}",
        shell_dir.display()
    ))?;

    let init_path = paths.shell_init();
    std::fs::write(&init_path, SHELL_INIT_TEMPLATE).map_err(|e| Error::ShellInitInstall {
        path: init_path,
        source: e,
    })?;

    Ok(())
}

/// Install tools.json to the data directory.
///
/// Creates `$XDG_DATA_HOME/akm/tools.json` from the embedded content.
/// Idempotent — overwrites any existing file.
///
/// Bash equivalent: `install.sh:38`
pub fn install_tools_json(paths: &Paths) -> Result<()> {
    let tools_path = paths.tools_json();
    if let Some(parent) = tools_path.parent() {
        std::fs::create_dir_all(parent)
            .io_context(format!("Creating data directory {}", parent.display()))?;
    }

    std::fs::write(&tools_path, TOOLS_JSON)
        .io_context(format!("Writing tools.json to {}", tools_path.display()))?;

    Ok(())
}

/// Patch .bashrc with AKM shell integration.
///
/// Uses marker blocks (`# >>> akm >>>` / `# <<< akm <<<`) for idempotent
/// insertion. If the markers already exist, the block between them is replaced.
/// If .bashrc doesn't exist, it is created.
///
/// Bash equivalent: `_patch_bashrc()` at bin/akm:93
pub fn patch_bashrc(paths: &Paths) -> Result<()> {
    let home = paths.home();
    let bashrc = home.join(".bashrc");

    // Read existing content (empty string if file doesn't exist)
    let existing = if bashrc.is_file() {
        std::fs::read_to_string(&bashrc).map_err(|e| Error::ShellInitWrite {
            path: bashrc.clone(),
            source: e,
        })?
    } else {
        String::new()
    };

    // Remove existing marker block if present
    let cleaned = remove_marker_block(&existing);

    // Build the new marker block
    let init_path_expr = r#"${XDG_DATA_HOME:-$HOME/.local/share}/akm/shell/akm-init.sh"#;
    let block = format!(
        "{BASHRC_MARKER_START}\n\
         [ -f \"{init_path_expr}\" ] && source \"{init_path_expr}\"\n\
         {BASHRC_MARKER_END}\n"
    );

    // Compose final content: cleaned content + newline separator + block
    let mut final_content = cleaned;
    if !final_content.is_empty() && !final_content.ends_with('\n') {
        final_content.push('\n');
    }
    final_content.push_str(&block);

    std::fs::write(&bashrc, &final_content).map_err(|e| Error::ShellInitWrite {
        path: bashrc,
        source: e,
    })?;

    Ok(())
}

/// Remove the AKM marker block from a string.
///
/// Finds lines between `# >>> akm >>>` and `# <<< akm <<<` (inclusive)
/// and removes them. Returns the cleaned content.
fn remove_marker_block(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut inside_block = false;

    for line in content.lines() {
        if line.trim() == BASHRC_MARKER_START {
            inside_block = true;
            continue;
        }
        if line.trim() == BASHRC_MARKER_END {
            inside_block = false;
            continue;
        }
        if !inside_block {
            result.push_str(line);
            result.push('\n');
        }
    }

    result
}

/// Check if .bashrc already contains the AKM marker block.
///
/// Used by setup to report status without re-patching.
pub fn bashrc_has_akm_block(paths: &Paths) -> bool {
    let bashrc = paths.home().join(".bashrc");
    if let Ok(content) = std::fs::read_to_string(&bashrc) {
        content.contains(BASHRC_MARKER_START)
    } else {
        false
    }
}

/// Return a reference to the embedded shell init template content.
///
/// Useful for snapshot tests that need to verify the template.
pub fn shell_init_content() -> &'static str {
    SHELL_INIT_TEMPLATE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remove_marker_block_no_markers() {
        let input = "# some config\nexport PATH=/usr/bin\n";
        assert_eq!(remove_marker_block(input), input);
    }

    #[test]
    fn test_remove_marker_block_with_markers() {
        let input = "before\n# >>> akm >>>\nsource something\n# <<< akm <<<\nafter\n";
        let expected = "before\nafter\n";
        assert_eq!(remove_marker_block(input), expected);
    }

    #[test]
    fn test_remove_marker_block_at_end() {
        let input = "before\n# >>> akm >>>\nsource something\n# <<< akm <<<\n";
        let expected = "before\n";
        assert_eq!(remove_marker_block(input), expected);
    }

    #[test]
    fn test_remove_marker_block_at_start() {
        let input = "# >>> akm >>>\nsource something\n# <<< akm <<<\nafter\n";
        let expected = "after\n";
        assert_eq!(remove_marker_block(input), expected);
    }

    #[test]
    fn test_remove_marker_block_unclosed_preserves_content() {
        let input = "before\n# >>> akm >>>\nsource something\nafter\n";
        let expected = "before\n";
        assert_eq!(remove_marker_block(input), expected);
    }

    #[test]
    fn test_remove_marker_block_multiple_blocks() {
        let input =
            "a\n# >>> akm >>>\nfirst\n# <<< akm <<<\nb\n# >>> akm >>>\nsecond\n# <<< akm <<<\nc\n";
        let expected = "a\nb\nc\n";
        assert_eq!(remove_marker_block(input), expected);
    }

    #[test]
    fn test_patch_bashrc_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );

        let bashrc = dir.path().join(".bashrc");
        std::fs::write(&bashrc, "# existing config\n").unwrap();

        patch_bashrc(&paths).unwrap();
        let first = std::fs::read_to_string(&bashrc).unwrap();

        patch_bashrc(&paths).unwrap();
        let second = std::fs::read_to_string(&bashrc).unwrap();

        assert_eq!(first, second);
        assert!(first.contains(BASHRC_MARKER_START));
        assert!(first.contains(BASHRC_MARKER_END));
        assert_eq!(first.matches(BASHRC_MARKER_START).count(), 1);
        assert_eq!(first.matches(BASHRC_MARKER_END).count(), 1);
    }

    #[test]
    fn test_patch_bashrc_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );

        let bashrc = dir.path().join(".bashrc");
        assert!(!bashrc.exists());

        patch_bashrc(&paths).unwrap();

        assert!(bashrc.exists());
        let content = std::fs::read_to_string(&bashrc).unwrap();
        assert!(content.contains(BASHRC_MARKER_START));
    }

    #[test]
    fn test_install_shell_init_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );

        install_shell_init(&paths).unwrap();
        assert!(paths.shell_init().exists());
        let content = std::fs::read_to_string(paths.shell_init()).unwrap();
        assert!(content.contains("_akm_session_start"));
    }

    #[test]
    fn test_install_tools_json_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );

        install_tools_json(&paths).unwrap();
        assert!(paths.tools_json().exists());
        let content = std::fs::read_to_string(paths.tools_json()).unwrap();
        assert!(content.contains("claude"));
    }

    #[test]
    fn test_bashrc_has_akm_block_false_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );
        assert!(!bashrc_has_akm_block(&paths));
    }

    #[test]
    fn test_bashrc_has_akm_block_true_after_patch() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );
        patch_bashrc(&paths).unwrap();
        assert!(bashrc_has_akm_block(&paths));
    }
}
