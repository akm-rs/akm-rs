//! Editor resolution utility.
//!
//! Shared between `instructions edit` and `skills edit`. Resolves the user's
//! preferred editor via the same priority chain as the Bash version:
//! `$EDITOR` → `git var GIT_EDITOR` → `"nano"`.

use std::process::Command;

/// Resolve the user's preferred text editor.
///
/// Priority:
/// 1. `$EDITOR` environment variable
/// 2. `git var GIT_EDITOR` (reads git config core.editor)
/// 3. `"nano"` as last resort
///
/// Bash: `local editor="${EDITOR:-$(git var GIT_EDITOR 2>/dev/null || echo "nano")}"`
/// Used in both `cmd_instructions_edit` (bin/akm:632) and `cmd_skills_edit` (bin/akm:2506).
pub fn resolve_editor() -> String {
    if let Ok(editor) = std::env::var("EDITOR") {
        if !editor.is_empty() {
            return editor;
        }
    }

    if let Ok(output) = Command::new("git").args(["var", "GIT_EDITOR"]).output() {
        if output.status.success() {
            let editor = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !editor.is_empty() {
                return editor;
            }
        }
    }

    "nano".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn resolve_editor_respects_env() {
        unsafe {
            std::env::set_var("EDITOR", "vim");
        }
        assert_eq!(resolve_editor(), "vim");
        unsafe {
            std::env::remove_var("EDITOR");
        }
    }

    #[test]
    #[serial]
    fn resolve_editor_empty_env_falls_through() {
        unsafe {
            std::env::set_var("EDITOR", "");
        }
        let editor = resolve_editor();
        // Should not be empty — falls through to git or nano
        assert!(!editor.is_empty());
        unsafe {
            std::env::remove_var("EDITOR");
        }
    }

    #[test]
    #[serial]
    fn resolve_editor_returns_something() {
        unsafe {
            std::env::remove_var("EDITOR");
        }
        let editor = resolve_editor();
        // Must return *something* — nano at minimum
        assert!(!editor.is_empty());
        unsafe {
            std::env::remove_var("EDITOR");
        }
    }
}
