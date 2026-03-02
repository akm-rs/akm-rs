//! Event handling for the TUI.
//!
//! Uses crossterm's event polling with a tick interval to drive redraws.
//! Events are read in the main thread (no async needed — spec says synchronous).

use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

/// Application event types.
#[derive(Debug)]
pub enum Event {
    /// A key was pressed.
    Key(KeyEvent),
    /// Tick interval elapsed (triggers redraw).
    Tick,
    /// Terminal was resized.
    Resize(u16, u16),
}

/// Poll interval for tick events (milliseconds).
const TICK_RATE_MS: u64 = 100;

/// Poll for the next event. Blocks up to `TICK_RATE_MS`.
///
/// Returns `Event::Tick` if no input arrives within the interval.
/// This is called in the main loop between renders.
///
/// # Errors
/// Returns `crate::error::Error::Tui` if crossterm event polling fails.
pub fn poll_event() -> crate::error::Result<Event> {
    if event::poll(Duration::from_millis(TICK_RATE_MS)).map_err(|e| crate::error::Error::Tui {
        message: format!("Event poll failed: {e}"),
    })? {
        match event::read().map_err(|e| crate::error::Error::Tui {
            message: format!("Event read failed: {e}"),
        })? {
            CrosstermEvent::Key(key) => Ok(Event::Key(key)),
            CrosstermEvent::Resize(w, h) => Ok(Event::Resize(w, h)),
            // Mouse, paste, etc. — ignore and treat as tick
            _ => Ok(Event::Tick),
        }
    } else {
        Ok(Event::Tick)
    }
}

/// Check if a key event is Ctrl+C.
pub fn is_ctrl_c(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

/// Check if a key event is Escape.
pub fn is_escape(key: &KeyEvent) -> bool {
    key.code == KeyCode::Esc
}
