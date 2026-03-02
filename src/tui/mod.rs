//! TUI framework for AKM.
//!
//! Provides terminal setup/teardown with crossterm, a panic handler that
//! restores the terminal, and the shared `run_app` function that drives
//! the event loop for any view.

pub mod app;
pub mod detail;
pub mod edit;
pub mod event;
pub mod list;
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
fn install_panic_hook() {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        restore_terminal();
        original_hook(panic_info);
    }));
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
