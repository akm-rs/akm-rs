//! `akm completions <shell>` — output shell completion registration script.
//!
//! Prints the registration script to stdout. Users can source directly
//! or redirect to a file. For automatic installation, use `akm setup`.
//!
//! Examples:
//!   eval "$(akm completions bash)"
//!   akm completions zsh >> ~/.zshrc
//!   akm completions fish > ~/.config/fish/completions/akm.fish

use crate::completions::Shell;
use crate::error::Result;

/// Run the `akm completions <shell>` command.
///
/// Outputs the registration script for the specified shell to stdout.
/// The script tells the shell to invoke `COMPLETE=<shell> akm` on Tab press.
pub fn run(shell: &Shell) -> Result<()> {
    print!("{}", shell.registration_script());
    Ok(())
}
