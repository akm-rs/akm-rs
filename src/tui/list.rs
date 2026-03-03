//! Interactive list view for browsing, filtering, and acting on library specs.
//!
//! Columns: ID | Type | Description | Tags | Core
//!
//! Key bindings:
//! - Type to filter/search (search bar at top)
//! - `Enter` → view full SKILL.md content in detail pane
//! - `c` → toggle core flag on/off
//! - `e` → edit metadata (tags, triggers)
//! - `a` → add to current project manifest
//! - `r` → remove from current project manifest
//! - `q` or `Esc` → quit
//! - `↑`/`↓` or `j`/`k` → navigate
//! - `Backspace` → delete last char from search
//! - `Ctrl+C` → exit immediately

use crate::error::Result;
use crate::library::spec::SpecType;
use crate::library::tool_dirs::ToolDirs;
use crate::paths::Paths;
use crate::tui::app::{AddResult, App, RemoveResult};
use crate::tui::detail;
use crate::tui::edit as tui_edit;
use crate::tui::event::{self, Event};
use crate::tui::theme;
use crate::tui::{self, EventOutcome, Term, ViewSwitch};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};
use ratatui::Frame;

/// State for the list view.
struct ListView {
    /// Current search/filter query (typed by user).
    search_query: String,
    /// Table selection state (tracks which row is highlighted).
    table_state: TableState,
    /// IDs of currently visible (filtered) specs, in display order.
    visible_ids: Vec<String>,
    /// CLI-level tag filter (from --tag).
    tag_filter: Option<String>,
    /// CLI-level type filter (from --type), pre-parsed as SpecType.
    type_filter: Option<SpecType>,
    /// Status message shown briefly after an action (e.g., "✓ Added to manifest").
    status_message: Option<String>,
}

impl ListView {
    fn new(
        tag_filter: Option<String>,
        type_filter: Option<SpecType>,
        initial_query: Option<String>,
    ) -> Self {
        let mut state = TableState::default();
        state.select(Some(0));
        Self {
            search_query: initial_query.unwrap_or_default(),
            table_state: state,
            visible_ids: Vec::new(),
            tag_filter,
            type_filter,
            status_message: None,
        }
    }

    /// Recompute the visible IDs based on current filters and search query.
    fn update_visible(&mut self, app: &App) {
        let base_specs = app.filtered_specs(self.tag_filter.as_deref(), self.type_filter);
        let filtered = App::search_filter(&base_specs, &self.search_query);
        self.visible_ids = filtered.iter().map(|s| s.id.clone()).collect();

        if self.visible_ids.is_empty() {
            self.table_state.select(None);
        } else {
            let max = self.visible_ids.len().saturating_sub(1);
            if let Some(selected) = self.table_state.selected() {
                if selected > max {
                    self.table_state.select(Some(max));
                }
            }
            if self.table_state.selected().is_none() {
                self.table_state.select(Some(0));
            }
        }
    }

    /// Get the currently selected spec ID.
    fn selected_id(&self) -> Option<&str> {
        self.table_state
            .selected()
            .and_then(|i| self.visible_ids.get(i))
            .map(|s| s.as_str())
    }

    /// Move selection up.
    fn select_prev(&mut self) {
        if let Some(selected) = self.table_state.selected() {
            if selected > 0 {
                self.table_state.select(Some(selected - 1));
            }
        }
    }

    /// Move selection down.
    fn select_next(&mut self) {
        if let Some(selected) = self.table_state.selected() {
            let max = self.visible_ids.len().saturating_sub(1);
            if selected < max {
                self.table_state.select(Some(selected + 1));
            }
        }
    }
}

/// Entry point for the interactive list view.
///
/// Called by `commands::skills::list::run()` and `commands::skills::search::run()`.
///
/// # Arguments
/// * `paths` — Resolved XDG paths
/// * `tag` — Optional CLI-level tag filter
/// * `type_filter` — Optional CLI-level type filter (pre-parsed as SpecType)
/// * `initial_query` — Pre-populated search query (used by `skills search <query>`)
/// * `tool_dirs` — Tool directory configuration
pub fn run(
    paths: &Paths,
    tag: Option<&str>,
    type_filter: Option<SpecType>,
    initial_query: Option<&str>,
    tool_dirs: &ToolDirs,
) -> Result<()> {
    let mut app = App::new(paths.clone(), tool_dirs.clone())?;
    let mut view = ListView::new(
        tag.map(|s| s.to_string()),
        type_filter,
        initial_query.map(|s| s.to_string()),
    );

    let mut terminal = tui::init_terminal()?;

    // Main event loop
    let result = run_list_loop(&mut terminal, &mut app, &mut view);

    // Always restore terminal, even on error
    tui::restore_terminal();

    // Save any mutations (core toggle, manifest add/remove).
    if let Err(save_err) = app.save_if_dirty() {
        if result.is_ok() {
            return Err(save_err);
        }
        eprintln!("Warning: failed to save changes: {save_err}");
    }

    result
}

/// The main event loop for the list view.
fn run_list_loop(terminal: &mut Term, app: &mut App, view: &mut ListView) -> Result<()> {
    loop {
        view.update_visible(app);

        tui::draw(terminal, |frame| render_list(frame, app, view))?;

        match event::poll_event()? {
            Event::Key(key) => match handle_list_key(key, app, view)? {
                EventOutcome::Continue => {}
                EventOutcome::Exit => return Ok(()),
                EventOutcome::SwitchTo(ViewSwitch::Detail { spec_id }) => {
                    detail::run_inline(terminal, app, &spec_id)?;
                }
                EventOutcome::SwitchTo(ViewSwitch::Edit { spec_id }) => {
                    tui_edit::run_inline(terminal, app, &spec_id)?;
                }
            },
            Event::Tick => {}
            Event::Resize(_, _) => {}
        }
    }
}

/// Handle a key event in the list view.
///
/// Guard conditions:
/// - `Ctrl+C` always exits (highest priority, checked first)
/// - `q` exits only if search bar is empty (otherwise it's a search character)
/// - Letter keys are treated as search input unless they match a keybinding
///   AND the search bar is empty.
fn handle_list_key(key: KeyEvent, app: &mut App, view: &mut ListView) -> Result<EventOutcome> {
    view.status_message = None;

    if event::is_ctrl_c(&key) {
        return Ok(EventOutcome::Exit);
    }

    if event::is_escape(&key) {
        if !view.search_query.is_empty() {
            view.search_query.clear();
            return Ok(EventOutcome::Continue);
        }
        return Ok(EventOutcome::Exit);
    }

    match key.code {
        // Navigation — arrow keys always work
        KeyCode::Up => view.select_prev(),
        KeyCode::Down => view.select_next(),
        // j/k only navigate when search is empty
        KeyCode::Char('k') if view.search_query.is_empty() => view.select_prev(),
        KeyCode::Char('j') if view.search_query.is_empty() => view.select_next(),

        KeyCode::Backspace => {
            view.search_query.pop();
        }

        KeyCode::Enter => {
            if let Some(id) = view.selected_id() {
                return Ok(EventOutcome::SwitchTo(ViewSwitch::Detail {
                    spec_id: id.to_string(),
                }));
            }
        }

        // Action keybindings — only active when search bar is empty
        KeyCode::Char('q') if view.search_query.is_empty() => {
            return Ok(EventOutcome::Exit);
        }
        KeyCode::Char('c') if view.search_query.is_empty() => {
            if let Some(id) = view.selected_id() {
                let id = id.to_string();
                if let Some(new_core) = app.toggle_core(&id) {
                    let state = if new_core { "on" } else { "off" };
                    view.status_message = Some(format!("Core {state}: {id}"));
                }
            }
        }
        KeyCode::Char('e') if view.search_query.is_empty() => {
            if let Some(id) = view.selected_id() {
                return Ok(EventOutcome::SwitchTo(ViewSwitch::Edit {
                    spec_id: id.to_string(),
                }));
            }
        }
        KeyCode::Char('a') if view.search_query.is_empty() => {
            if let Some(id) = view.selected_id() {
                let id = id.to_string();
                match app.add_to_manifest(&id)? {
                    AddResult::Added => {
                        view.status_message = Some(format!("✓ Added to manifest: {id}"))
                    }
                    AddResult::AlreadyPresent => {
                        view.status_message = Some(format!("Already in manifest: {id}"))
                    }
                    AddResult::NoProject => {
                        view.status_message = Some("No project detected".to_string())
                    }
                    AddResult::SpecNotFound => {
                        view.status_message = Some(format!("Spec not found: {id}"))
                    }
                }
            }
        }
        KeyCode::Char('r') if view.search_query.is_empty() => {
            if let Some(id) = view.selected_id() {
                let id = id.to_string();
                match app.remove_from_manifest(&id)? {
                    RemoveResult::Removed => {
                        view.status_message = Some(format!("✓ Removed from manifest: {id}"))
                    }
                    RemoveResult::NotPresent => {
                        view.status_message = Some(format!("Not in manifest: {id}"))
                    }
                    RemoveResult::NoManifest => {
                        view.status_message = Some("No manifest found".to_string())
                    }
                }
            }
        }

        // All other characters go to search
        KeyCode::Char(c) => {
            view.search_query.push(c);
        }

        _ => {}
    }

    Ok(EventOutcome::Continue)
}

/// Render the list view.
fn render_list(frame: &mut Frame, app: &App, view: &mut ListView) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // search bar
            Constraint::Min(5),    // table
            Constraint::Length(1), // status message
            Constraint::Length(2), // help bar
        ])
        .split(frame.area());

    render_search_bar(frame, chunks[0], &view.search_query);
    render_table(frame, chunks[1], app, view);

    if let Some(ref msg) = view.status_message {
        let status = Paragraph::new(msg.as_str()).style(theme::SUCCESS);
        frame.render_widget(status, chunks[2]);
    }

    render_help_bar(frame, chunks[3]);
}

/// Render the search/filter bar.
fn render_search_bar(frame: &mut Frame, area: Rect, query: &str) {
    let text = if query.is_empty() {
        Line::from(vec![
            Span::styled(" 🔍 ", theme::DIM),
            Span::styled("Type to search/filter...", theme::DIM),
        ])
    } else {
        Line::from(vec![
            Span::styled(" 🔍 ", theme::SEARCH_BAR),
            Span::raw(query),
            Span::styled("█", theme::SEARCH_BAR),
        ])
    };
    let para = Paragraph::new(text).style(theme::SEARCH_BAR);
    frame.render_widget(para, area);
}

/// Render the spec table.
fn render_table(frame: &mut Frame, area: Rect, app: &App, view: &mut ListView) {
    let header = Row::new(vec![
        Cell::from("ID").style(theme::HEADER),
        Cell::from("Type").style(theme::HEADER),
        Cell::from("Description").style(theme::HEADER),
        Cell::from("Tags").style(theme::HEADER),
        Cell::from("Core").style(theme::HEADER),
        Cell::from("Manifest").style(theme::HEADER),
    ]);

    let rows: Vec<Row> = view
        .visible_ids
        .iter()
        .filter_map(|id| app.library.get(id))
        .map(|spec| {
            let type_style = theme::type_style(&spec.spec_type);
            let core_text = if spec.core { "✓" } else { "" };
            let manifest_text = if app.manifest_ids.contains(&spec.id) {
                "✓"
            } else {
                ""
            };
            let tags_text = spec.tags.join(", ");

            Row::new(vec![
                Cell::from(spec.id.as_str()),
                Cell::from(spec.spec_type.to_string()).style(type_style),
                Cell::from(spec.description.as_str()),
                Cell::from(tags_text).style(theme::DIM),
                Cell::from(core_text).style(theme::CORE_BADGE),
                Cell::from(manifest_text).style(theme::SUCCESS),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(25), // ID
            Constraint::Length(8),      // Type
            Constraint::Percentage(40), // Description
            Constraint::Percentage(15), // Tags
            Constraint::Length(4),      // Core
            Constraint::Length(8),      // Manifest
        ],
    )
    .header(header)
    .row_highlight_style(theme::SELECTED)
    .block(Block::default().borders(Borders::NONE));

    frame.render_stateful_widget(table, area, &mut view.table_state);
}

/// Render the help bar showing available key bindings.
fn render_help_bar(frame: &mut Frame, area: Rect) {
    let help_text = Line::from(vec![
        Span::styled(" ↑↓/jk", theme::HEADER),
        Span::styled(" navigate  ", theme::HELP_BAR),
        Span::styled("Enter", theme::HEADER),
        Span::styled(" view  ", theme::HELP_BAR),
        Span::styled("c", theme::HEADER),
        Span::styled(" core  ", theme::HELP_BAR),
        Span::styled("e", theme::HEADER),
        Span::styled(" edit  ", theme::HELP_BAR),
        Span::styled("a", theme::HEADER),
        Span::styled(" add  ", theme::HELP_BAR),
        Span::styled("r", theme::HEADER),
        Span::styled(" remove  ", theme::HELP_BAR),
        Span::styled("q", theme::HEADER),
        Span::styled(" quit  ", theme::HELP_BAR),
        Span::styled("Esc", theme::HEADER),
        Span::styled(" clear search", theme::HELP_BAR),
    ]);
    let para = Paragraph::new(help_text);
    frame.render_widget(para, area);
}
