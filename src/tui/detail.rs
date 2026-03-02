//! Detail view — displays the full SKILL.md / agent .md content.
//!
//! Opened by pressing `Enter` in the list view.
//! Supports scrolling with ↑/↓ and PageUp/PageDown.
//! `q`, `Esc`, or `Backspace` returns to the list view.

use crate::error::Result;
use crate::tui::app::App;
use crate::tui::event::{self, Event};
use crate::tui::theme;
use crate::tui::{self, Term};

use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

/// State for the detail view.
struct DetailView {
    /// Spec ID being viewed.
    spec_id: String,
    /// Full markdown content.
    content: String,
    /// Current scroll offset (line number).
    scroll: u16,
    /// Total number of lines.
    total_lines: u16,
}

/// Run the detail view inline (without creating a new terminal).
///
/// Called from the list view's event loop. Returns when the user presses
/// `q`, `Esc`, or `Backspace` to go back.
pub fn run_inline(terminal: &mut Term, app: &App, spec_id: &str) -> Result<()> {
    let content = app.read_spec_content(spec_id)?;
    let total_lines = content.lines().count().min(u16::MAX as usize) as u16;

    let mut view = DetailView {
        spec_id: spec_id.to_string(),
        content,
        scroll: 0,
        total_lines,
    };

    loop {
        tui::draw(terminal, |frame| render_detail(frame, &view))?;

        match event::poll_event()? {
            Event::Key(key) => {
                if event::is_ctrl_c(&key) || event::is_escape(&key) {
                    return Ok(());
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Backspace => return Ok(()),
                    KeyCode::Up | KeyCode::Char('k') => {
                        view.scroll = view.scroll.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if view.scroll < view.total_lines.saturating_sub(1) {
                            view.scroll += 1;
                        }
                    }
                    KeyCode::PageUp => {
                        view.scroll = view.scroll.saturating_sub(20);
                    }
                    KeyCode::PageDown => {
                        view.scroll = (view.scroll + 20).min(view.total_lines.saturating_sub(1));
                    }
                    KeyCode::Home => {
                        view.scroll = 0;
                    }
                    KeyCode::End => {
                        view.scroll = view.total_lines.saturating_sub(1);
                    }
                    _ => {}
                }
            }
            Event::Tick | Event::Resize(_, _) => {}
        }
    }
}

/// Render the detail view.
fn render_detail(frame: &mut Frame, view: &DetailView) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // content
            Constraint::Length(1), // help bar
        ])
        .split(frame.area());

    let title = format!(" {} ", view.spec_id);
    let content = Paragraph::new(view.content.as_str())
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: false })
        .scroll((view.scroll, 0));

    frame.render_widget(content, chunks[0]);

    let help = Paragraph::new(" ↑↓ scroll  PageUp/PageDown  q/Esc back").style(theme::HELP_BAR);
    frame.render_widget(help, chunks[1]);
}
