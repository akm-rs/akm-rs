//! Inline metadata editor — edits description, tags, and core flag for a spec.
//!
//! Opened by pressing `e` in the list view. Unlike `akm skills edit --meta`,
//! which opens $EDITOR on the raw sidecar, this is a structured form.
//!
//! The text fields are [`TextInput`]s drawn wrapped across several lines, so a
//! description longer than the popup stays readable and the cursor stays on
//! screen instead of running off the right edge.

use crate::error::Result;
use crate::tui::app::App;
use crate::tui::event::{self, Event};
use crate::tui::input::TextInput;
use crate::tui::theme;
use crate::tui::{self, Term};

use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Lines given to each wrapped text field in the popup.
const FIELD_HEIGHT: usize = 3;

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
    description: TextInput,
    tags: TextInput,
    core: bool,
    focused: EditField,
}

impl EditView {
    fn from_spec(app: &App, spec_id: &str) -> Option<Self> {
        let spec = app.library.get(spec_id)?;
        Some(Self {
            spec_id: spec_id.to_string(),
            description: TextInput::new(&spec.description),
            tags: TextInput::new(&spec.tags.join(", ")),
            core: spec.core,
            focused: EditField::Description,
        })
    }

    /// The input the focused field is editing, if it is a text field.
    fn focused_input(&mut self) -> Option<&mut TextInput> {
        match self.focused {
            EditField::Description => Some(&mut self.description),
            EditField::Tags => Some(&mut self.tags),
            EditField::Core => None,
        }
    }

    /// Apply the edits back to the app state.
    fn apply(&self, app: &mut App) {
        if let Some(spec) = app.library.get_mut(&self.spec_id) {
            spec.description = self.description.value();
            spec.tags = self
                .tags
                .value()
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();
            spec.core = self.core;
            app.library_dirty = true;
            app.edited_meta.insert(self.spec_id.clone());
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
                    code => {
                        if let Some(input) = view.focused_input() {
                            match code {
                                KeyCode::Char(c) => input.insert(c),
                                KeyCode::Backspace => input.backspace(),
                                KeyCode::Delete => input.delete(),
                                KeyCode::Left => input.left(),
                                KeyCode::Right => input.right(),
                                KeyCode::Home => input.home(),
                                KeyCode::End => input.end(),
                                _ => {}
                            }
                        }
                    }
                }
            }
            Event::Tick | Event::Resize(_, _) => {}
        }
    }
}

/// Render the edit form as a centered popup.
fn render_edit(frame: &mut Frame, view: &EditView) {
    // Tall enough for two three-line fields, their labels, the core toggle
    // and the help line.
    let area = centered_rect(60, 55, frame.area());

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
            Constraint::Length(1),                   // Description label
            Constraint::Length(FIELD_HEIGHT as u16), // Description
            Constraint::Length(1),                   // Tags label
            Constraint::Length(FIELD_HEIGHT as u16), // Tags
            Constraint::Length(1),                   // Core
            Constraint::Min(0),                      // spacer
            Constraint::Length(1),                   // help
        ])
        .split(inner);

    render_label(
        frame,
        field_chunks[0],
        "Description",
        view.focused == EditField::Description,
    );
    render_field(
        frame,
        field_chunks[1],
        &view.description,
        view.focused == EditField::Description,
    );

    render_label(
        frame,
        field_chunks[2],
        "Tags (comma separated)",
        view.focused == EditField::Tags,
    );
    render_field(
        frame,
        field_chunks[3],
        &view.tags,
        view.focused == EditField::Tags,
    );

    let core_text = if view.core { "[✓] Core" } else { "[ ] Core" };
    let core_style = if view.focused == EditField::Core {
        theme::SELECTED
    } else {
        ratatui::style::Style::default()
    };
    let core_para = Paragraph::new(core_text).style(core_style);
    frame.render_widget(core_para, field_chunks[4]);

    let help = Paragraph::new(" Tab next  ←/→ move  Space toggle  Enter save  Esc cancel")
        .style(theme::HELP_BAR);
    frame.render_widget(help, field_chunks[6]);
}

/// Render a field label, bolded while its field has focus.
fn render_label(frame: &mut Frame, area: Rect, label: &str, focused: bool) {
    let style = if focused {
        theme::HEADER
    } else {
        theme::FIELD_BLURRED
    };
    frame.render_widget(Paragraph::new(format!("{label}:")).style(style), area);
}

/// Render one wrapped text field, drawing the cursor into the text itself.
///
/// The cursor is a styled cell rather than the terminal's own, so it stays
/// correct wherever the field happens to have scrolled to.
fn render_field(frame: &mut Frame, area: Rect, input: &TextInput, focused: bool) {
    // Only the cursor cell is highlighted: styling the whole focused field
    // would swallow it.
    let style = if focused {
        ratatui::style::Style::default()
    } else {
        theme::FIELD_BLURRED
    };

    let width = area.width.max(1) as usize;
    let (rows, (cursor_row, cursor_col)) = input.visible(width, area.height.max(1) as usize);

    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .map(|(row, text)| {
            if !focused || row != cursor_row {
                return Line::from(Span::styled(text.clone(), style));
            }

            // Split the line around the cursor so it can be highlighted.
            let chars: Vec<char> = text.chars().collect();
            let before: String = chars.iter().take(cursor_col).collect();
            let under: String = chars
                .get(cursor_col)
                .map(|c| c.to_string())
                .unwrap_or_else(|| " ".into());
            let after: String = chars.iter().skip(cursor_col + 1).collect();

            Line::from(vec![
                Span::styled(before, style),
                Span::styled(under, theme::CURSOR),
                Span::styled(after, style),
            ])
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), area);
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
