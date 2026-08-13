//! TUI framework for AKM.
//!
//! Provides terminal setup/teardown with crossterm, a panic handler that
//! restores the terminal, and the shared `run_app` function that drives
//! the event loop for any view.

pub mod app;
pub mod artifacts;
pub mod detail;
pub mod edit;
pub mod event;
pub mod input;
pub mod list;
pub mod settings;
pub mod status;
pub mod theme;

use crate::error::{Error, Result};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};
use std::panic;
use std::path::Path;
use std::process::Command;

/// Type alias for our terminal backend.
pub type Term = Terminal<CrosstermBackend<Stdout>>;

/// Initialize the terminal for TUI rendering.
///
/// 1. Enable raw mode (no line buffering, no echo)
/// 2. Enter alternate screen (preserves user's scrollback)
/// 3. Install panic hook that restores terminal before printing panic
///
/// # Errors
/// Returns `Error::Tui` if terminal initialization fails.
pub fn init_terminal() -> Result<Term> {
    // Install panic hook BEFORE entering raw mode so it's active even
    // if enable_raw_mode itself panics in some edge case.
    install_panic_hook();

    enable_raw_mode().map_err(|e| Error::Tui {
        message: format!("Failed to enable raw mode: {e}"),
    })?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| Error::Tui {
        message: format!("Failed to enter alternate screen: {e}"),
    })?;

    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).map_err(|e| Error::Tui {
        message: format!("Failed to create terminal: {e}"),
    })
}

/// Restore the terminal to its original state.
///
/// Called on normal exit AND by the panic handler.
/// Must never panic itself — uses `let _ =` for all operations.
pub fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
}

/// Install a panic hook that restores the terminal before printing the panic.
///
/// Without this, a panic in TUI code would leave the terminal in raw mode
/// with the alternate screen active, making it unusable.
/// Uses `Once` to ensure the hook is installed at most once.
fn install_panic_hook() {
    use std::sync::Once;
    static HOOK_INSTALLED: Once = Once::new();
    HOOK_INSTALLED.call_once(|| {
        let original_hook = panic::take_hook();
        panic::set_hook(Box::new(move |panic_info| {
            restore_terminal();
            original_hook(panic_info);
        }));
    });
}

/// Outcome of processing a single event in any view.
#[derive(Debug, Clone, PartialEq)]
pub enum EventOutcome {
    /// Continue the event loop.
    Continue,
    /// Exit the current view (q or Esc).
    Exit,
    /// Switch to a different view (e.g., Enter → detail, e → edit).
    SwitchTo(ViewSwitch),
}

/// Target view for a switch.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewSwitch {
    /// Show the detail view for a spec (Enter from list).
    Detail { spec_id: String },
    /// Show the inline editor for a spec (e from list).
    Edit { spec_id: String },
}

/// Helper to wrap `terminal.draw()` errors into `crate::error::Error::Tui`.
///
/// `terminal.draw()` returns `std::io::Result`, which doesn't implement
/// `From` for our error type. This function provides the conversion.
pub fn draw<F>(terminal: &mut Term, f: F) -> Result<()>
where
    F: FnOnce(&mut ratatui::Frame),
{
    terminal.draw(f).map_err(|e| Error::Tui {
        message: format!("Render failed: {e}"),
    })?;
    Ok(())
}

/// Suspend the TUI, open `path` in the user's editor, then resume.
///
/// Leaves the alternate screen and raw mode so the editor owns the real
/// terminal, spawns it as a child inheriting stdio, waits, then re-enters and
/// forces a full redraw. Editor resolution and spawn mirror
/// [`crate::commands::skills::edit`] exactly (the whole string is the program
/// name).
///
/// Operates on the *existing* `terminal` — it must not call [`init_terminal`],
/// which would build a second `Term` and re-install the panic hook.
///
/// The child-process dance is the one seam the house style does not snapshot;
/// keep it thin. A missing editor or spawn failure is returned, not panicked —
/// the caller stays in the TUI and shows it inline.
pub fn edit_file_in_terminal(terminal: &mut Term, path: &Path) -> Result<()> {
    restore_terminal(); // leave alt-screen + disable raw mode

    let editor = crate::editor::resolve_editor();
    let spawn = Command::new(&editor).arg(path).status();

    // Re-enter regardless of how the editor fared, so a failed spawn never
    // strands the user outside the TUI.
    enable_raw_mode().map_err(|e| Error::Tui {
        message: format!("Failed to re-enable raw mode after editor: {e}"),
    })?;
    execute!(io::stdout(), EnterAlternateScreen).map_err(|e| Error::Tui {
        message: format!("Failed to re-enter alternate screen after editor: {e}"),
    })?;
    terminal.clear().map_err(|e| Error::Tui {
        message: format!("Failed to redraw after editor: {e}"),
    })?;

    spawn.map_err(|e| Error::Tui {
        message: format!("Failed to launch editor '{editor}': {e}"),
    })?;
    Ok(())
}
