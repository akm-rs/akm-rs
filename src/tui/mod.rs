//! TUI framework for AKM.
//!
//! Provides terminal setup/teardown with crossterm, a panic handler that
//! restores the terminal, and shared event/draw helpers. Each view owns its
//! own event loop (see [`list`], [`artifacts`], [`settings`]).

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
use std::io::{self, Stdout, Write};
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

/// Copy `text` to the system clipboard.
///
/// Two mechanisms, tried in order, because no single one covers the field:
///
/// 1. **A platform clipboard tool**, if one is on `PATH`: `clip.exe` (WSL →
///    the Windows clipboard), `pbcopy` (macOS), `wl-copy` (Wayland), `xclip`
///    /`xsel` (X11). This lands in the real OS clipboard regardless of any
///    terminal or tmux quirk, so a Windows-Terminal paste or Notepad `Ctrl+V`
///    just works. These are optional soft dependencies — absent, we fall
///    through, so the release artifact stays a single self-contained binary.
/// 2. **The OSC 52 escape sequence**, for the tool-less case (typically SSH
///    into a bare box). It rides the stdout the TUI already owns. Support is
///    the terminal's to give — most modern emulators honour it, the rest drop
///    it silently. Inside tmux the sequence is DCS-wrapped for passthrough
///    (needs `allow-passthrough on`); note that `set-clipboard external`
///    (tmux's default) otherwise diverts OSC 52 into tmux's own buffer and
///    never forwards it to the host clipboard, which is why the tool path is
///    preferred on the common WSL+tmux stack.
///
/// OSC 52 gives no receipt, so callers should also surface the copied text
/// (the artifacts explorer prints it to the status line) as a mouse-selectable
/// last resort.
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    if copy_via_tool(text) {
        return Ok(());
    }
    copy_via_osc52(text)
}

/// Clipboard tools to try, in preference order for the current platform.
///
/// Each entry is spawned in turn; one missing from `PATH` fails to spawn and we
/// move on. `clip.exe` is listed even off Windows because it is the WSL path to
/// the *Windows* clipboard — the one a native Windows app reads.
fn clipboard_commands() -> Vec<(&'static str, &'static [&'static str])> {
    let mut cmds: Vec<(&'static str, &'static [&'static str])> = Vec::new();
    if cfg!(target_os = "macos") {
        cmds.push(("pbcopy", &[]));
    }
    // WSL: reaches the Windows clipboard that Windows Terminal / Notepad read.
    cmds.push(("clip.exe", &[]));
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        cmds.push(("wl-copy", &[]));
    }
    cmds.push(("xclip", &["-selection", "clipboard"]));
    cmds.push(("xsel", &["--clipboard", "--input"]));
    cmds
}

/// Try each candidate tool until one accepts the text. Returns whether a tool
/// succeeded; on `false` the caller falls back to OSC 52.
fn copy_via_tool(text: &str) -> bool {
    clipboard_commands()
        .into_iter()
        .any(|(cmd, args)| run_clipboard_tool(cmd, args, text))
}

/// Spawn one clipboard tool, feed `text` on stdin, and report success.
///
/// A spawn error (tool not installed) or a non-zero exit both count as failure
/// so the next candidate is tried.
fn run_clipboard_tool(cmd: &str, args: &[&str], text: &str) -> bool {
    use std::process::Stdio;
    let mut child = match Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    // Write then drop stdin so the tool sees EOF before we wait on it.
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(text.as_bytes()).is_err() {
            let _ = child.wait();
            return false;
        }
    }
    matches!(child.wait(), Ok(status) if status.success())
}

/// Write the OSC 52 sequence for `text` to stdout, DCS-wrapping it for tmux
/// passthrough when running under tmux.
fn copy_via_osc52(text: &str) -> Result<()> {
    let seq = osc52_sequence(text, std::env::var_os("TMUX").is_some());
    let mut stdout = io::stdout();
    stdout.write_all(seq.as_bytes()).map_err(|e| Error::Tui {
        message: format!("Failed to write to clipboard: {e}"),
    })?;
    stdout.flush().map_err(|e| Error::Tui {
        message: format!("Failed to flush clipboard write: {e}"),
    })?;
    Ok(())
}

/// Build the OSC 52 clipboard-set sequence for `text`.
///
/// Bare form is `ESC ] 52 ; c ; <base64> BEL` (`c` = the clipboard buffer).
/// Under tmux it is wrapped in a DCS passthrough — `ESC P tmux ; <payload> ESC
/// \` with every inner `ESC` doubled — so tmux forwards it to the host terminal
/// instead of consuming it (requires `allow-passthrough on`).
fn osc52_sequence(text: &str, in_tmux: bool) -> String {
    let osc = format!("\x1b]52;c;{}\x07", base64_encode(text.as_bytes()));
    if in_tmux {
        format!("\x1bPtmux;{}\x1b\\", osc.replace('\x1b', "\x1b\x1b"))
    } else {
        osc
    }
}

/// Standard base64 (RFC 4648) encoder, `=`-padded.
///
/// Hand-rolled to keep OSC 52 dependency-free — a full `base64` crate would be
/// a runtime dependency for fifteen lines of table lookup.
fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18 & 63) as usize] as char);
        out.push(TABLE[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{base64_encode, osc52_sequence};

    #[test]
    fn osc52_bare_outside_tmux() {
        // ESC ] 52 ; c ; <base64> BEL, nothing wrapped.
        assert_eq!(osc52_sequence("foo", false), "\x1b]52;c;Zm9v\x07");
    }

    #[test]
    fn osc52_dcs_wrapped_inside_tmux() {
        // DCS passthrough: ESC P tmux ; <payload, inner ESC doubled> ESC \
        assert_eq!(
            osc52_sequence("foo", true),
            "\x1bPtmux;\x1b\x1b]52;c;Zm9v\x07\x1b\\"
        );
    }

    #[test]
    fn base64_matches_known_vectors() {
        // RFC 4648 §10 test vectors exercise every padding case.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }
}
