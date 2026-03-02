//! Inline metadata editor — edits description, tags, and core flag for a spec.
//!
//! Opened by pressing `e` in the list view. Unlike `akm skills edit` which
//! opens $EDITOR on raw JSON, this provides a structured form-like interface.
//! Triggers editing is deferred (read-only structured data).

use crate::error::Result;
use crate::tui::app::App;
use crate::tui::event::{self, Event};
use crate::tui::theme;
use crate::tui::{self, Term};

use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Which field is currently focused in the editor.
#[derive(Debug, Clone, Copy, PartialEq)]
enum EditField {
    Description,
    Tags,
    Core,
}

impl EditField {
    fn next(self) -> Self {
        match self {
            EditField::Description => EditField::Tags,
            EditField::Tags => EditField::Core,
            EditField::Core => EditField::Description,
        }
    }

    fn prev(self) -> Self {
        match self {
            EditField::Description => EditField::Core,
            EditField::Tags => EditField::Description,
            EditField::Core => EditField::Tags,
        }
    }
}

/// State for the editor view.
struct EditView {
    spec_id: String,
    description: String,
    tags_text: String,
    core: bool,
    focused: EditField,
}

impl EditView {
    fn from_spec(app: &App, spec_id: &str) -> Option<Self> {
        let spec = app.library.get(spec_id)?;
        Some(Self {
            spec_id: spec_id.to_string(),
            description: spec.description.clone(),
            tags_text: spec.tags.join(", "),
            core: spec.core,
            focused: EditField::Description,
        })
    }

    /// Apply the edits back to the app state.
    fn apply(&self, app: &mut App) {
        if let Some(spec) = app.library.get_mut(&self.spec_id) {
            spec.description = self.description.clone();
            spec.tags = self
                .tags_text
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();
            spec.core = self.core;
            app.library_dirty = true;
        }
    }
}

/// Run the inline editor. Returns when user saves (Enter) or cancels (Esc).
pub fn run_inline(terminal: &mut Term, app: &mut App, spec_id: &str) -> Result<()> {
    let mut view = match EditView::from_spec(app, spec_id) {
        Some(v) => v,
        None => return Ok(()), // Spec not found — silently return
    };

    loop {
        tui::draw(terminal, |frame| render_edit(frame, &view))?;

        match event::poll_event()? {
            Event::Key(key) => {
                if event::is_ctrl_c(&key) {
                    return Ok(()); // Cancel without saving
                }
                match key.code {
                    KeyCode::Esc => return Ok(()), // Cancel
                    KeyCode::Enter => {
                        view.apply(app);
                        return Ok(());
                    }
                    KeyCode::Tab => {
                        view.focused = view.focused.next();
                    }
                    KeyCode::BackTab => {
                        view.focused = view.focused.prev();
                    }
                    KeyCode::Char(' ') if view.focused == EditField::Core => {
                        view.core = !view.core;
                    }
                    KeyCode::Char(c) => match view.focused {
                        EditField::Description => view.description.push(c),
                        EditField::Tags => view.tags_text.push(c),
                        EditField::Core => {}
                    },
                    KeyCode::Backspace => match view.focused {
                        EditField::Description => {
                            view.description.pop();
                        }
                        EditField::Tags => {
                            view.tags_text.pop();
                        }
                        EditField::Core => {}
                    },
                    _ => {}
                }
            }
            Event::Tick | Event::Resize(_, _) => {}
        }
    }
}

/// Render the edit form as a centered popup.
fn render_edit(frame: &mut Frame, view: &EditView) {
    let area = centered_rect(60, 40, frame.area());

    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(" Edit: {} ", view.spec_id))
        .borders(Borders::ALL)
        .style(theme::HEADER);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let field_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Description
            Constraint::Length(2), // Tags
            Constraint::Length(2), // Core
            Constraint::Length(1), // spacer
            Constraint::Length(1), // help
        ])
        .split(inner);

    render_field(
        frame,
        field_chunks[0],
        "Description",
        &view.description,
        view.focused == EditField::Description,
    );

    render_field(
        frame,
        field_chunks[1],
        "Tags",
        &view.tags_text,
        view.focused == EditField::Tags,
    );

    let core_text = if view.core { "[✓] Core" } else { "[ ] Core" };
    let core_style = if view.focused == EditField::Core {
        theme::SELECTED
    } else {
        ratatui::style::Style::default()
    };
    let core_para = Paragraph::new(core_text).style(core_style);
    frame.render_widget(core_para, field_chunks[2]);

    let help =
        Paragraph::new(" Tab next  Space toggle  Enter save  Esc cancel").style(theme::HELP_BAR);
    frame.render_widget(help, field_chunks[4]);
}

/// Render a single text input field.
fn render_field(frame: &mut Frame, area: Rect, label: &str, value: &str, focused: bool) {
    let style = if focused {
        theme::SELECTED
    } else {
        ratatui::style::Style::default()
    };

    let display = if focused {
        format!("{label}: {value}█")
    } else {
        format!("{label}: {value}")
    };

    let para = Paragraph::new(display).style(style);
    frame.render_widget(para, area);
}

/// Create a centered rectangle of the given percentage size.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
