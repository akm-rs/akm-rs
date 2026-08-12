//! Interactive list view for browsing, filtering, and acting on library specs.
//!
//! Columns: ID | Type | Description | Tags | Core | Manifest | Sync
//!
//! The Sync column carries the spec's drift marker — `*` edited here and not
//! yet published, `v` the remote is ahead, `!` both sides moved. It is computed
//! once at startup; see [`App::drift`](crate::tui::app::App::drift).
//!
//! The view is modal. It opens in [`Mode::Normal`], where letters are commands
//! and the search filter (if any) stays applied, so actions can be used on a
//! filtered list. `/` switches to [`Mode::Search`], where letters are text.
//!
//! Normal mode key bindings:
//! - `/` → start editing the search filter
//! - `Enter` → view full SKILL.md content in detail pane
//! - `c` → toggle core flag on/off
//! - `e` → edit metadata (tags, triggers)
//! - `a` → add to current project manifest
//! - `r` → remove from current project manifest
//! - `R` → rename the spec's id (its slug)
//! - `D` → delete the spec from the library
//! - `q` → quit
//! - `Esc` → clear the search filter (never quits)
//! - `↑`/`↓` or `j`/`k` → navigate
//! - any other key → ignored
//! - `Ctrl+C` → exit immediately
//!
//! Search mode key bindings:
//! - any character → appended to the search filter
//! - `Backspace` → delete last char from search
//! - `Enter` or `Esc` → back to normal mode, keeping the filter
//! - `↑`/`↓` → navigate
//! - `Ctrl+C` → exit immediately

use crate::config::Config;
use crate::error::Result;
use crate::library::spec::SpecType;
use crate::library::tool_dirs::ToolDirs;
use crate::paths::Paths;
use crate::tui::app::{AddResult, App, DeleteOutcome, RemoveResult, RenameOutcome};
use crate::tui::detail;
use crate::tui::edit as tui_edit;
use crate::tui::event::{self, Event};
use crate::tui::input::TextInput;
use crate::tui::theme;
use crate::tui::{self, EventOutcome, Term, ViewSwitch};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};
use ratatui::Frame;

/// Input mode for the list view.
///
/// Determines whether printable keys are commands or search text. The search
/// filter is independent of the mode — it stays applied in [`Mode::Normal`],
/// which is what lets actions operate on a filtered list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Letters are commands. The search filter (if any) remains applied.
    Normal,
    /// Letters are appended to the search query.
    Search,
    /// Typing the new id for the spec being renamed.
    Rename,
    /// Awaiting `y`/`n` to confirm deleting the spec.
    ConfirmDelete,
}

/// State for the list view.
struct ListView {
    /// Current input mode.
    mode: Mode,
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
    /// The id targeted by an in-progress rename/delete (the modal target).
    pending_id: Option<String>,
    /// Text field for the new id while renaming.
    rename_input: TextInput,
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
            // Both `skills list` and `skills search <query>` open in normal
            // mode: a query passed on the command line is already committed.
            mode: Mode::Normal,
            search_query: initial_query.unwrap_or_default(),
            table_state: state,
            visible_ids: Vec::new(),
            tag_filter,
            type_filter,
            status_message: None,
            pending_id: None,
            rename_input: TextInput::default(),
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

    // Eager rename/delete changes could not be published inside the raw-mode
    // TUI. Now that the terminal is restored, offer them on the normal terminal.
    if result.is_ok() && !app.pending_publish.is_empty() {
        offer_pending_publishes(paths, &app);
    }

    result
}

/// After the TUI exits, offer to publish the rename/delete changes it made.
fn offer_pending_publishes(paths: &Paths, app: &App) {
    let config = Config::load(paths).unwrap_or_default();
    for change in &app.pending_publish {
        println!("{}", change.summary);
        crate::commands::skills::publish::offer_pathspecs(
            paths,
            &config,
            &change.pathspecs,
            &change.message,
        );
    }
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
/// `Ctrl+C` exits from either mode (highest priority, checked first).
/// Everything else is dispatched on [`ListView::mode`].
fn handle_list_key(key: KeyEvent, app: &mut App, view: &mut ListView) -> Result<EventOutcome> {
    view.status_message = None;

    if event::is_ctrl_c(&key) {
        return Ok(EventOutcome::Exit);
    }

    match view.mode {
        Mode::Normal => handle_normal_key(key, app, view),
        Mode::Search => {
            handle_search_key(key, view);
            Ok(EventOutcome::Continue)
        }
        Mode::Rename => handle_rename_key(key, app, view),
        Mode::ConfirmDelete => handle_confirm_delete_key(key, app, view),
    }
}

/// Handle a key event in normal mode, where letters are commands.
///
/// Keys with no binding are ignored — they never fall through to the search
/// query. `/` is the only way into [`Mode::Search`].
fn handle_normal_key(key: KeyEvent, app: &mut App, view: &mut ListView) -> Result<EventOutcome> {
    if event::is_escape(&key) {
        // Esc clears the filter but never quits — only `q` and Ctrl+C do.
        view.search_query.clear();
        return Ok(EventOutcome::Continue);
    }

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => view.select_prev(),
        KeyCode::Down | KeyCode::Char('j') => view.select_next(),

        KeyCode::Char('/') => view.mode = Mode::Search,

        KeyCode::Enter => {
            if let Some(id) = view.selected_id() {
                return Ok(EventOutcome::SwitchTo(ViewSwitch::Detail {
                    spec_id: id.to_string(),
                }));
            }
        }

        KeyCode::Char('q') => return Ok(EventOutcome::Exit),

        KeyCode::Char('c') => {
            if let Some(id) = view.selected_id() {
                let id = id.to_string();
                if let Some(new_core) = app.toggle_core(&id) {
                    let state = if new_core { "on" } else { "off" };
                    view.status_message = Some(format!("Core {state}: {id}"));
                }
            }
        }
        KeyCode::Char('e') => {
            if let Some(id) = view.selected_id() {
                return Ok(EventOutcome::SwitchTo(ViewSwitch::Edit {
                    spec_id: id.to_string(),
                }));
            }
        }
        KeyCode::Char('a') => {
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
        KeyCode::Char('r') => {
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

        KeyCode::Char('R') => {
            if let Some(id) = view.selected_id() {
                let id = id.to_string();
                view.rename_input = TextInput::new(&id);
                view.pending_id = Some(id);
                view.mode = Mode::Rename;
            }
        }
        KeyCode::Char('D') => {
            if let Some(id) = view.selected_id() {
                view.pending_id = Some(id.to_string());
                view.mode = Mode::ConfirmDelete;
            }
        }

        _ => {}
    }

    Ok(EventOutcome::Continue)
}

/// Handle a key event while typing the new id for a rename.
///
/// `Enter` commits, `Esc` cancels. A rejected id (bad slug or collision) keeps
/// the field open so it can be corrected; success returns to normal mode.
fn handle_rename_key(key: KeyEvent, app: &mut App, view: &mut ListView) -> Result<EventOutcome> {
    if event::is_escape(&key) {
        view.mode = Mode::Normal;
        view.pending_id = None;
        return Ok(EventOutcome::Continue);
    }

    match key.code {
        KeyCode::Enter => {
            let (Some(old), new) = (view.pending_id.clone(), view.rename_input.value()) else {
                view.mode = Mode::Normal;
                return Ok(EventOutcome::Continue);
            };
            if new == old {
                view.mode = Mode::Normal;
                view.pending_id = None;
                return Ok(EventOutcome::Continue);
            }
            match app.rename_spec(&old, &new)? {
                RenameOutcome::Renamed => {
                    view.status_message = Some(format!("Renamed '{old}' -> '{new}'"));
                    view.mode = Mode::Normal;
                    view.pending_id = None;
                }
                RenameOutcome::InvalidId(reason) => {
                    view.status_message = Some(format!("Invalid id: {reason}"));
                }
                RenameOutcome::Collision => {
                    view.status_message = Some(format!("'{new}' already exists"));
                }
                RenameOutcome::NotFound => {
                    view.status_message = Some(format!("Spec not found: {old}"));
                    view.mode = Mode::Normal;
                    view.pending_id = None;
                }
            }
        }
        KeyCode::Backspace => view.rename_input.backspace(),
        KeyCode::Left => view.rename_input.left(),
        KeyCode::Right => view.rename_input.right(),
        KeyCode::Char(c) => view.rename_input.insert(c),
        _ => {}
    }

    Ok(EventOutcome::Continue)
}

/// Handle the `y`/`n` confirmation before deleting a spec.
fn handle_confirm_delete_key(
    key: KeyEvent,
    app: &mut App,
    view: &mut ListView,
) -> Result<EventOutcome> {
    let id = view.pending_id.clone();
    view.mode = Mode::Normal;
    view.pending_id = None;

    if let (KeyCode::Char('y') | KeyCode::Char('Y'), Some(id)) = (key.code, id) {
        match app.delete_spec(&id)? {
            DeleteOutcome::Deleted => view.status_message = Some(format!("Deleted '{id}'")),
            DeleteOutcome::NotFound => view.status_message = Some(format!("Spec not found: {id}")),
        }
    }

    Ok(EventOutcome::Continue)
}

/// Handle a key event in search mode, where characters are query text.
///
/// `Enter` and `Esc` both return to [`Mode::Normal`] keeping the query, so the
/// action keys become available on the filtered list.
fn handle_search_key(key: KeyEvent, view: &mut ListView) {
    if event::is_escape(&key) {
        view.mode = Mode::Normal;
        return;
    }

    match key.code {
        KeyCode::Enter => view.mode = Mode::Normal,
        KeyCode::Up => view.select_prev(),
        KeyCode::Down => view.select_next(),
        KeyCode::Backspace => {
            view.search_query.pop();
        }
        KeyCode::Char(c) => view.search_query.push(c),
        _ => {}
    }
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

    render_prompt_bar(frame, chunks[0], view);
    render_table(frame, chunks[1], app, view);

    if let Some(ref msg) = view.status_message {
        let status = Paragraph::new(msg.as_str()).style(theme::SUCCESS);
        frame.render_widget(status, chunks[2]);
    }

    render_help_bar(frame, chunks[3], view.mode, !app.drift.is_clean());
}

/// Render the top bar: the search field, or the rename/confirm-delete prompt.
fn render_prompt_bar(frame: &mut Frame, area: Rect, view: &ListView) {
    match view.mode {
        Mode::Rename => {
            let target = view.pending_id.as_deref().unwrap_or("");
            let line = Line::from(vec![
                Span::styled(format!(" Rename {target} -> "), theme::SEARCH_BAR),
                Span::raw(view.rename_input.value()),
                Span::styled("█", theme::SEARCH_BAR),
            ]);
            frame.render_widget(Paragraph::new(line).style(theme::SEARCH_BAR), area);
        }
        Mode::ConfirmDelete => {
            let target = view.pending_id.as_deref().unwrap_or("");
            let line = Line::from(vec![Span::styled(
                format!(" Delete '{target}' from the library? [y/N]"),
                theme::WARNING,
            )]);
            frame.render_widget(Paragraph::new(line), area);
        }
        Mode::Normal | Mode::Search => {
            render_search_bar(frame, area, view.mode, &view.search_query)
        }
    }
}

/// Render the search/filter bar.
///
/// The cursor block is shown only in search mode, so the bar also indicates
/// which mode the view is in.
fn render_search_bar(frame: &mut Frame, area: Rect, mode: Mode, query: &str) {
    let text = match (mode, query.is_empty()) {
        (Mode::Search, _) => Line::from(vec![
            Span::styled(" 🔍 ", theme::SEARCH_BAR),
            Span::raw(query),
            Span::styled("█", theme::SEARCH_BAR),
        ]),
        (Mode::Normal, true) => Line::from(vec![
            Span::styled(" 🔍 ", theme::DIM),
            Span::styled("Press / to search/filter...", theme::DIM),
        ]),
        (Mode::Normal, false) => Line::from(vec![
            Span::styled(" 🔍 ", theme::SEARCH_BAR),
            Span::raw(query),
            Span::styled("  [filtered]", theme::DIM),
        ]),
        // The modal modes render their own prompt via `render_prompt_bar`.
        (Mode::Rename, _) | (Mode::ConfirmDelete, _) => Line::from(""),
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
        Cell::from("Sync").style(theme::HEADER),
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
            let drift = app.drift.state_of(&spec.id);

            Row::new(vec![
                Cell::from(spec.id.as_str()),
                Cell::from(spec.spec_type.to_string()).style(type_style),
                Cell::from(spec.description.as_str()),
                Cell::from(tags_text).style(theme::DIM),
                Cell::from(core_text).style(theme::CORE_BADGE),
                Cell::from(manifest_text).style(theme::SUCCESS),
                Cell::from(drift.marker()).style(theme::drift_style(drift)),
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
            Constraint::Length(4),      // Sync
        ],
    )
    .header(header)
    .row_highlight_style(theme::SELECTED)
    .block(Block::default().borders(Borders::NONE));

    frame.render_stateful_widget(table, area, &mut view.table_state);
}

/// Render the help bar showing the key bindings for the current mode.
///
/// A second line explains the Sync column, shown only when something has
/// actually drifted — on a level library the markers are all blank.
fn render_help_bar(frame: &mut Frame, area: Rect, mode: Mode, show_drift_legend: bool) {
    let pairs: &[(&str, &str)] = match mode {
        Mode::Normal => &[
            (" ↑↓/jk", " navigate  "),
            ("Enter", " view  "),
            ("c", " core  "),
            ("e", " edit  "),
            ("a", " add  "),
            ("r", " remove  "),
            ("R", " rename  "),
            ("D", " delete  "),
            ("/", " search  "),
            ("Esc", " clear filter  "),
            ("q", " quit"),
        ],
        Mode::Search => &[
            (" type", " to filter  "),
            ("Backspace", " delete  "),
            ("↑↓", " navigate  "),
            ("Enter/Esc", " done  "),
            ("Ctrl+C", " quit"),
        ],
        Mode::Rename => &[
            (" type", " new id  "),
            ("Enter", " confirm  "),
            ("Esc", " cancel"),
        ],
        Mode::ConfirmDelete => &[("y", " delete  "), ("n/Esc", " cancel")],
    };
    let help_text = Line::from(
        pairs
            .iter()
            .flat_map(|(key, label)| {
                [
                    Span::styled(*key, theme::HEADER),
                    Span::styled(*label, theme::HELP_BAR),
                ]
            })
            .collect::<Vec<_>>(),
    );

    let mut lines = vec![help_text];
    if show_drift_legend {
        lines.push(Line::from(vec![
            Span::styled(" Sync:", theme::HEADER),
            Span::styled(" * unpublished  ", theme::WARNING),
            Span::styled("v remote ahead  ", theme::DIM),
            Span::styled("! diverged", theme::ERROR),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::spec::Spec;
    use crate::library::Library;
    use crossterm::event::KeyModifiers;

    /// Build a key event with no modifiers.
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// Build a printable-character key event.
    fn ch(c: char) -> KeyEvent {
        key(KeyCode::Char(c))
    }

    /// Feed a sequence of characters through the key handler.
    fn type_chars(app: &mut App, view: &mut ListView, s: &str) {
        for c in s.chars() {
            handle_list_key(ch(c), app, view).unwrap();
            view.update_visible(app);
        }
    }

    /// An App backed by a temp dir, plus the guard keeping that dir alive.
    fn test_app() -> (App, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(tmp.path(), tmp.path(), tmp.path(), tmp.path());
        let library = Library {
            version: 1,
            specs: vec![
                Spec::new(
                    "git-commit",
                    SpecType::Skill,
                    "Git Commit",
                    "Structured commits",
                ),
                Spec::new(
                    "git-worktrees",
                    SpecType::Skill,
                    "Worktrees",
                    "Isolated worktrees",
                ),
                Spec::new(
                    "code-review",
                    SpecType::Agent,
                    "Code Review",
                    "Reviews changes",
                ),
            ],
        };
        library.save(&paths).unwrap();
        let tool_dirs = ToolDirs::builtin(tmp.path());
        let app = App::new(paths, tool_dirs).unwrap();
        (app, tmp)
    }

    /// An App whose specs actually exist on disk, so rename/delete (which move
    /// and remove files) can run. Returns the App and its temp-dir guard.
    fn test_app_on_disk() -> (App, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(tmp.path(), tmp.path(), tmp.path(), tmp.path());
        let library_dir = paths.library_dir();

        for id in ["git-commit", "git-worktrees"] {
            let dir = library_dir.join("skills").join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {id}\ndescription: a test skill\n---\nBody"),
            )
            .unwrap();
        }
        crate::library::libgen::generate(&library_dir, &paths.library_json()).unwrap();

        let tool_dirs = ToolDirs::builtin(tmp.path());
        let app = App::new(paths, tool_dirs).unwrap();
        (app, tmp)
    }

    /// A normal-mode view over `app`, cursor on the first row.
    fn normal_view(app: &mut App) -> ListView {
        let mut view = ListView::new(None, None, None);
        view.update_visible(app);
        view
    }

    #[test]
    fn capital_r_opens_rename_prefilled_with_the_id() {
        let (mut app, _tmp) = test_app_on_disk();
        let mut view = normal_view(&mut app);
        handle_list_key(ch('R'), &mut app, &mut view).unwrap();
        assert_eq!(view.mode, Mode::Rename);
        assert_eq!(view.pending_id.as_deref(), Some("git-commit"));
        assert_eq!(view.rename_input.value(), "git-commit");
    }

    #[test]
    fn committing_a_rename_moves_the_spec_and_queues_a_publish() {
        let (mut app, _tmp) = test_app_on_disk();
        let mut view = normal_view(&mut app);
        handle_list_key(ch('R'), &mut app, &mut view).unwrap();
        view.rename_input = TextInput::new("git-renamed");
        handle_list_key(key(KeyCode::Enter), &mut app, &mut view).unwrap();

        assert_eq!(view.mode, Mode::Normal);
        assert!(app.library.get("git-commit").is_none());
        assert!(app.library.get("git-renamed").is_some());
        assert_eq!(app.pending_publish.len(), 1);
    }

    #[test]
    fn rename_collision_keeps_the_field_open() {
        let (mut app, _tmp) = test_app_on_disk();
        let mut view = normal_view(&mut app);
        handle_list_key(ch('R'), &mut app, &mut view).unwrap();
        view.rename_input = TextInput::new("git-worktrees");
        handle_list_key(key(KeyCode::Enter), &mut app, &mut view).unwrap();

        // Still renaming, both specs intact.
        assert_eq!(view.mode, Mode::Rename);
        assert!(app.library.get("git-commit").is_some());
        assert!(app.pending_publish.is_empty());
    }

    #[test]
    fn capital_d_then_y_deletes_and_queues_a_publish() {
        let (mut app, _tmp) = test_app_on_disk();
        let mut view = normal_view(&mut app);
        handle_list_key(ch('D'), &mut app, &mut view).unwrap();
        assert_eq!(view.mode, Mode::ConfirmDelete);

        handle_list_key(ch('y'), &mut app, &mut view).unwrap();
        assert_eq!(view.mode, Mode::Normal);
        assert!(app.library.get("git-commit").is_none());
        assert_eq!(app.pending_publish.len(), 1);
    }

    #[test]
    fn delete_cancelled_leaves_the_spec() {
        let (mut app, _tmp) = test_app_on_disk();
        let mut view = normal_view(&mut app);
        handle_list_key(ch('D'), &mut app, &mut view).unwrap();
        handle_list_key(ch('n'), &mut app, &mut view).unwrap();
        assert_eq!(view.mode, Mode::Normal);
        assert!(app.library.get("git-commit").is_some());
        assert!(app.pending_publish.is_empty());
    }

    /// A view already filtered to "git" and back in normal mode.
    fn filtered_view(app: &mut App) -> ListView {
        let mut view = ListView::new(None, None, None);
        view.update_visible(app);
        handle_list_key(ch('/'), app, &mut view).unwrap();
        type_chars(app, &mut view, "git");
        handle_list_key(key(KeyCode::Esc), app, &mut view).unwrap();
        view.update_visible(app);
        view
    }

    #[test]
    fn opens_in_normal_mode() {
        let view = ListView::new(None, None, None);
        assert_eq!(view.mode, Mode::Normal);
        assert!(view.search_query.is_empty());
    }

    #[test]
    fn cli_query_opens_in_normal_mode() {
        let view = ListView::new(None, None, Some("tdd".to_string()));
        assert_eq!(view.mode, Mode::Normal);
        assert_eq!(view.search_query, "tdd");
    }

    #[test]
    fn slash_enters_search_mode() {
        let (mut app, _tmp) = test_app();
        let mut view = ListView::new(None, None, None);
        handle_list_key(ch('/'), &mut app, &mut view).unwrap();
        assert_eq!(view.mode, Mode::Search);
        // The `/` itself is not part of the query.
        assert!(view.search_query.is_empty());
    }

    #[test]
    fn search_mode_types_chars_into_query() {
        let (mut app, _tmp) = test_app();
        let mut view = ListView::new(None, None, None);
        handle_list_key(ch('/'), &mut app, &mut view).unwrap();
        type_chars(&mut app, &mut view, "git");
        assert_eq!(view.search_query, "git");
        assert_eq!(view.visible_ids, vec!["git-commit", "git-worktrees"]);
    }

    #[test]
    fn search_mode_treats_action_keys_as_text() {
        let (mut app, _tmp) = test_app();
        let mut view = ListView::new(None, None, None);
        handle_list_key(ch('/'), &mut app, &mut view).unwrap();
        // Every key bound to an action in normal mode is plain text here.
        type_chars(&mut app, &mut view, "arceqjk/");
        assert_eq!(view.search_query, "arceqjk/");
        assert_eq!(view.mode, Mode::Search);
        assert!(!app.library_dirty);
        assert!(!app.manifest_dirty);
    }

    #[test]
    fn escape_leaves_search_mode_keeping_the_filter() {
        let (mut app, _tmp) = test_app();
        let view = filtered_view(&mut app);
        assert_eq!(view.mode, Mode::Normal);
        assert_eq!(view.search_query, "git");
        assert_eq!(view.visible_ids, vec!["git-commit", "git-worktrees"]);
    }

    #[test]
    fn enter_leaves_search_mode_keeping_the_filter() {
        let (mut app, _tmp) = test_app();
        let mut view = ListView::new(None, None, None);
        handle_list_key(ch('/'), &mut app, &mut view).unwrap();
        type_chars(&mut app, &mut view, "git");
        let outcome = handle_list_key(key(KeyCode::Enter), &mut app, &mut view).unwrap();
        assert_eq!(outcome, EventOutcome::Continue);
        assert_eq!(view.mode, Mode::Normal);
        assert_eq!(view.search_query, "git");
    }

    /// The bug from issue #19: actions must work on a list that is still
    /// filtered, and leaving search must not throw the filter away.
    #[test]
    fn actions_work_while_filter_is_active() {
        let (mut app, _tmp) = test_app();
        let mut view = filtered_view(&mut app);

        handle_list_key(ch('c'), &mut app, &mut view).unwrap();

        assert!(app.library.get("git-commit").unwrap().core);
        assert!(app.library_dirty);
        // The filter survived the action.
        assert_eq!(view.search_query, "git");
        assert_eq!(view.visible_ids, vec!["git-commit", "git-worktrees"]);
    }

    #[test]
    fn normal_mode_ignores_unbound_chars() {
        let (mut app, _tmp) = test_app();
        let mut view = filtered_view(&mut app);
        for c in ['g', 't', 'v', 'Z', '1'] {
            let outcome = handle_list_key(ch(c), &mut app, &mut view).unwrap();
            assert_eq!(outcome, EventOutcome::Continue);
        }
        assert_eq!(view.mode, Mode::Normal);
        assert_eq!(view.search_query, "git");
    }

    #[test]
    fn normal_mode_escape_clears_filter_without_quitting() {
        let (mut app, _tmp) = test_app();
        let mut view = filtered_view(&mut app);

        let outcome = handle_list_key(key(KeyCode::Esc), &mut app, &mut view).unwrap();
        assert_eq!(outcome, EventOutcome::Continue);
        assert!(view.search_query.is_empty());

        // A second Esc on an unfiltered list is inert — only q/Ctrl+C quit.
        let outcome = handle_list_key(key(KeyCode::Esc), &mut app, &mut view).unwrap();
        assert_eq!(outcome, EventOutcome::Continue);
    }

    #[test]
    fn normal_mode_ignores_backspace() {
        let (mut app, _tmp) = test_app();
        let mut view = filtered_view(&mut app);
        handle_list_key(key(KeyCode::Backspace), &mut app, &mut view).unwrap();
        assert_eq!(view.search_query, "git");
    }

    #[test]
    fn search_mode_backspace_pops_a_char() {
        let (mut app, _tmp) = test_app();
        let mut view = ListView::new(None, None, None);
        handle_list_key(ch('/'), &mut app, &mut view).unwrap();
        type_chars(&mut app, &mut view, "git");
        handle_list_key(key(KeyCode::Backspace), &mut app, &mut view).unwrap();
        assert_eq!(view.search_query, "gi");
    }

    #[test]
    fn q_quits_only_in_normal_mode() {
        let (mut app, _tmp) = test_app();
        let mut view = filtered_view(&mut app);
        assert_eq!(
            handle_list_key(ch('q'), &mut app, &mut view).unwrap(),
            EventOutcome::Exit
        );
    }

    #[test]
    fn ctrl_c_exits_from_search_mode() {
        let (mut app, _tmp) = test_app();
        let mut view = ListView::new(None, None, None);
        handle_list_key(ch('/'), &mut app, &mut view).unwrap();
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(
            handle_list_key(ctrl_c, &mut app, &mut view).unwrap(),
            EventOutcome::Exit
        );
    }

    #[test]
    fn enter_opens_detail_in_normal_mode() {
        let (mut app, _tmp) = test_app();
        let mut view = filtered_view(&mut app);
        let outcome = handle_list_key(key(KeyCode::Enter), &mut app, &mut view).unwrap();
        assert_eq!(
            outcome,
            EventOutcome::SwitchTo(ViewSwitch::Detail {
                spec_id: "git-commit".to_string()
            })
        );
    }

    #[test]
    fn jk_navigate_in_normal_mode() {
        let (mut app, _tmp) = test_app();
        let mut view = filtered_view(&mut app);
        assert_eq!(view.selected_id(), Some("git-commit"));
        handle_list_key(ch('j'), &mut app, &mut view).unwrap();
        assert_eq!(view.selected_id(), Some("git-worktrees"));
        handle_list_key(ch('k'), &mut app, &mut view).unwrap();
        assert_eq!(view.selected_id(), Some("git-commit"));
    }

    #[test]
    fn arrows_navigate_in_search_mode() {
        let (mut app, _tmp) = test_app();
        let mut view = ListView::new(None, None, None);
        handle_list_key(ch('/'), &mut app, &mut view).unwrap();
        type_chars(&mut app, &mut view, "git");
        assert_eq!(view.selected_id(), Some("git-commit"));
        handle_list_key(key(KeyCode::Down), &mut app, &mut view).unwrap();
        assert_eq!(view.selected_id(), Some("git-worktrees"));
        assert_eq!(view.search_query, "git");
    }
}
