//! Shell completion registration and installation.
//!
//! Uses `clap_complete::CompleteEnv` exclusively. The `akm completions <shell>`
//! command outputs a registration script that tells the shell to invoke
//! `COMPLETE=<shell> akm` on Tab press. The binary then returns candidates
//! (both static subcommands/flags and dynamic spec IDs).
//!
//! This module does NOT use `clap_complete::generate()` (the legacy AOT approach),
//! which produces static scripts that cannot call back for dynamic completions.

pub mod dynamic;

use crate::error::{Error, IoContext, Result};
use crate::paths::Paths;
use std::path::{Path, PathBuf};

/// Supported shells for completion registration.
///
/// Uses clap's `ValueEnum` derive for CLI argument parsing, avoiding
/// a redundant custom `from_str` implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl Shell {
    /// Generate the completion registration bootstrap for this shell.
    ///
    /// These bootstrap scripts invoke the binary with `COMPLETE=<shell>` set,
    /// which triggers `CompleteEnv` to output the actual registration script.
    /// Delegating to the binary makes completions self-healing — upgrading
    /// `akm` automatically upgrades the registration logic.
    pub fn registration_script(self) -> &'static str {
        match self {
            Self::Bash => "eval \"$(COMPLETE=bash akm)\"\n",
            Self::Zsh => "#compdef akm\neval \"$(COMPLETE=zsh akm)\"\n",
            Self::Fish => "COMPLETE=fish akm | source\n",
        }
    }

    /// Default installation path for this shell's completion registration script.
    ///
    /// Follows shell conventions:
    /// - Bash: `$XDG_DATA_HOME/bash-completion/completions/akm`
    /// - Zsh: `$XDG_DATA_HOME/zsh/site-functions/_akm`
    /// - Fish: `$XDG_DATA_HOME/fish/vendor_completions.d/akm.fish`
    pub fn completion_path(self, paths: &Paths) -> PathBuf {
        let xdg_data = paths.xdg_data_home();
        match self {
            Self::Bash => xdg_data
                .join("bash-completion")
                .join("completions")
                .join("akm"),
            Self::Zsh => xdg_data.join("zsh").join("site-functions").join("_akm"),
            Self::Fish => xdg_data
                .join("fish")
                .join("vendor_completions.d")
                .join("akm.fish"),
        }
    }

    /// All supported shells.
    pub fn all() -> &'static [Shell] {
        &[Self::Bash, Self::Zsh, Self::Fish]
    }
}

impl std::fmt::Display for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bash => write!(f, "bash"),
            Self::Zsh => write!(f, "zsh"),
            Self::Fish => write!(f, "fish"),
        }
    }
}

/// Install completion registration scripts for all detected shells.
///
/// Called by `akm setup` after config and shell init installation.
/// Only installs for shells that are actually available on the system.
/// Errors for individual shells are non-fatal (logged as warnings).
pub fn install_completions(paths: &Paths) {
    let mut installed_any = false;
    for &shell in Shell::all() {
        if !shell_available(shell) {
            continue;
        }

        let target = shell.completion_path(paths);

        if let Err(e) = install_single(shell, &target) {
            eprintln!("  Warning: could not install {} completions: {}", shell, e);
        } else {
            println!("  Installed {} completions to {}", shell, target.display());
            installed_any = true;
        }
    }
    if !installed_any {
        println!("  No supported shells detected (bash, zsh, fish)");
    }
}

/// Install a completion registration script for a single shell.
fn install_single(shell: Shell, target: &Path) -> Result<()> {
    let script = shell.registration_script();

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).io_context(format!(
            "Creating completion directory {}",
            parent.display()
        ))?;
    }

    std::fs::write(target, script).map_err(|e| Error::CompletionInstall {
        path: target.to_path_buf(),
        source: e,
    })?;

    Ok(())
}

/// Check if a shell binary is available on PATH.
///
/// Public for use in tests.
pub fn shell_available(shell: Shell) -> bool {
    let binary = match shell {
        Shell::Bash => "bash",
        Shell::Zsh => "zsh",
        Shell::Fish => "fish",
    };
    which_exists(binary)
}

/// Check if a binary exists on PATH and is executable.
fn which_exists(binary: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let full = dir.join(binary);
                full.is_file()
                    && full
                        .metadata()
                        .map(|m| m.permissions().mode() & 0o111 != 0)
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_display() {
        assert_eq!(format!("{}", Shell::Bash), "bash");
        assert_eq!(format!("{}", Shell::Zsh), "zsh");
        assert_eq!(format!("{}", Shell::Fish), "fish");
    }

    #[test]
    fn test_completion_path_bash() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );
        let path = Shell::Bash.completion_path(&paths);
        assert!(path.ends_with("bash-completion/completions/akm"));
        assert!(!path.to_str().unwrap().contains("akm/bash-completion"));
    }

    #[test]
    fn test_completion_path_zsh() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );
        let path = Shell::Zsh.completion_path(&paths);
        assert!(path.ends_with("zsh/site-functions/_akm"));
    }

    #[test]
    fn test_completion_path_fish() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(
            &dir.path().join("data"),
            &dir.path().join("config"),
            &dir.path().join("cache"),
            dir.path(),
        );
        let path = Shell::Fish.completion_path(&paths);
        assert!(path.ends_with("fish/vendor_completions.d/akm.fish"));
    }

    #[test]
    fn test_bash_registration_script_bootstraps_via_complete_env() {
        let script = Shell::Bash.registration_script();
        assert!(script.contains("COMPLETE=bash akm"));
        assert!(script.contains("eval"));
    }

    #[test]
    fn test_zsh_registration_script_bootstraps_via_complete_env() {
        let script = Shell::Zsh.registration_script();
        assert!(script.contains("#compdef akm"));
        assert!(script.contains("COMPLETE=zsh akm"));
        assert!(script.contains("eval"));
    }

    #[test]
    fn test_fish_registration_script_bootstraps_via_complete_env() {
        let script = Shell::Fish.registration_script();
        assert!(script.contains("COMPLETE=fish akm"));
        assert!(script.contains("source"));
    }

    #[test]
    fn test_which_exists_for_common_binary() {
        assert!(which_exists("sh"));
        assert!(!which_exists("nonexistent-binary-xyz-12345"));
    }

    #[test]
    fn test_install_single_creates_directories_and_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("deep").join("nested").join("akm");
        install_single(Shell::Bash, &target).unwrap();
        assert!(target.exists());
        let content = std::fs::read_to_string(&target).unwrap();
        assert_eq!(content, Shell::Bash.registration_script());
    }

    #[test]
    fn test_install_single_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("akm");
        install_single(Shell::Bash, &target).unwrap();
        let first = std::fs::read_to_string(&target).unwrap();
        install_single(Shell::Bash, &target).unwrap();
        let second = std::fs::read_to_string(&target).unwrap();
        assert_eq!(first, second);
    }
}
