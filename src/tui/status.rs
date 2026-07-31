//! Status dashboard — interactive version of `akm skills status`.
//!
//! Displays the same sections as the plain output (core, session, manifest,
//! cold) but in a scrollable TUI with navigation and actions.
//!
//! Key bindings match the list view's normal mode, minus `/` — the dashboard
//! has no search filter, so `Esc` has nothing to clear and is inert here:
//! - `↑`/`↓` or `j`/`k` — navigate between specs (across sections)
//! - `Enter` — view detail for selected spec
//! - `c` — toggle core for selected spec
//! - `e` — edit metadata (tags, triggers)
//! - `a` — add selected to manifest
//! - `r` — remove selected from manifest
//! - `q` — quit
//! - `Ctrl+C` — exit immediately
//!
//! Actions that move a spec between sections (`c`, `a`, `r`) leave the cursor
//! where it is instead of chasing the spec to its new section — see
//! [`StatusView::rebuild_holding_cursor`].

use crate::error::Result;
use crate::library::drift::DriftState;
use crate::library::spec::SpecType;
use crate::library::tool_dirs::ToolDirs;
use crate::paths::Paths;
use crate::tui::app::{AddResult, App, RemoveResult};
use crate::tui::detail;
use crate::tui::edit as tui_edit;
use crate::tui::event::{self, Event};
use crate::tui::theme;
use crate::tui::{self, Term};

use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;

/// A display row in the status dashboard.
#[derive(Debug, Clone)]
enum StatusRow {
    /// Section header (not selectable).
    Header(String),
    /// A spec entry (selectable).
    Spec {
        id: String,
        spec_type: SpecType,
        section: StatusSection,
        note: Option<String>,
        drift: DriftState,
    },
    /// Empty section indicator "(none loaded)", "(empty manifest)".
    Empty(String),
    /// Blank separator line.
    Blank,
}

/// Which section a spec belongs to.
#[derive(Debug, Clone, Copy)]
enum StatusSection {
    Core,
    Session,
    Manifest,
    Cold,
}

/// State for the status dashboard.
struct StatusView {
    rows: Vec<StatusRow>,
    /// Indices into `rows` that are selectable (Spec entries only).
    selectable_indices: Vec<usize>,
    /// Current position within `selectable_indices`.
    selected_pos: usize,
    /// ratatui ListState for scroll-into-view behavior.
    list_state: ListState,
    status_message: Option<String>,
}

impl StatusView {
    fn build(app: &App) -> Self {
        let mut rows = Vec::new();
        let mut selectable_indices = Vec::new();

        // Section 1: Project info
        if let Some(ref name) = app.project_name {
            let root_display = app
                .project_root
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            rows.push(StatusRow::Header(format!(
                "Project: {name} ({root_display})"
            )));
        } else {
            rows.push(StatusRow::Header("Project: (none)".to_string()));
        }
        rows.push(StatusRow::Blank);

        // Section 2: Core specs
        rows.push(StatusRow::Header(
            "Core specs (globally symlinked):".to_string(),
        ));
        let core_specs = app.library.core_specs();
        let core_ids: HashSet<&str> = core_specs.iter().map(|s| s.id.as_str()).collect();
        if core_specs.is_empty() {
            rows.push(StatusRow::Empty("  (none)".to_string()));
        } else {
            for spec in &core_specs {
                let idx = rows.len();
                selectable_indices.push(idx);
                rows.push(StatusRow::Spec {
                    id: spec.id.clone(),
                    spec_type: spec.spec_type,
                    section: StatusSection::Core,
                    note: None,
                    drift: app.drift.state_of(&spec.id),
                });
            }
        }
        rows.push(StatusRow::Blank);

        // Section 3: Session specs (if AKM_SESSION active)
        let session_dir = env::var("AKM_SESSION").ok().map(PathBuf::from);
        if let Some(ref staging) = session_dir {
            if staging.is_dir() {
                rows.push(StatusRow::Header(
                    "Session specs (staging dir):".to_string(),
                ));
                let session_specs =
                    crate::commands::skills::status::scan_session_dir(staging, &app.tool_dirs);
                if session_specs.is_empty() {
                    rows.push(StatusRow::Empty("  (none loaded)".to_string()));
                } else {
                    for (id, spec_type) in &session_specs {
                        let idx = rows.len();
                        selectable_indices.push(idx);
                        rows.push(StatusRow::Spec {
                            id: id.clone(),
                            spec_type: *spec_type,
                            section: StatusSection::Session,
                            note: None,
                            drift: app.drift.state_of(id),
                        });
                    }
                }
                rows.push(StatusRow::Blank);
            }
        }

        // Section 4: Manifest specs
        if app.project_root.is_some() {
            if let Some(manifest) = &app.manifest {
                rows.push(StatusRow::Header(
                    "Manifest specs (.agents/akm.json):".to_string(),
                ));
                let mut has_entries = false;

                for id in manifest.skill_ids() {
                    let idx = rows.len();
                    selectable_indices.push(idx);
                    let note = if !app.library.contains(id) {
                        Some("(not in library)".to_string())
                    } else {
                        None
                    };
                    rows.push(StatusRow::Spec {
                        id: id.clone(),
                        spec_type: SpecType::Skill,
                        section: StatusSection::Manifest,
                        note,
                        drift: app.drift.state_of(id),
                    });
                    has_entries = true;
                }
                for id in manifest.agent_ids() {
                    let idx = rows.len();
                    selectable_indices.push(idx);
                    let note = if !app.library.contains(id) {
                        Some("(not in library)".to_string())
                    } else {
                        None
                    };
                    rows.push(StatusRow::Spec {
                        id: id.clone(),
                        spec_type: SpecType::Agent,
                        section: StatusSection::Manifest,
                        note,
                        drift: app.drift.state_of(id),
                    });
                    has_entries = true;
                }

                if !has_entries {
                    rows.push(StatusRow::Empty("  (empty manifest)".to_string()));
                }
                rows.push(StatusRow::Blank);
            }
        }

        // Section 5: Cold (available) — not core, not in manifest
        rows.push(StatusRow::Header("Cold (available):".to_string()));
        for spec in &app.library.specs {
            if core_ids.contains(spec.id.as_str()) {
                continue;
            }
            if app.manifest_ids.contains(&spec.id) {
                continue;
            }
            let idx = rows.len();
            selectable_indices.push(idx);
            rows.push(StatusRow::Spec {
                id: spec.id.clone(),
                spec_type: spec.spec_type,
                section: StatusSection::Cold,
                note: None,
                drift: app.drift.state_of(&spec.id),
            });
        }

        let initial_list_pos = selectable_indices.first().copied();
        Self {
            rows,
            selectable_indices,
            selected_pos: 0,
            list_state: ListState::default().with_selected(initial_list_pos),
            status_message: None,
        }
    }

    fn selected_id(&self) -> Option<&str> {
        self.id_at(self.selected_pos)
    }

    /// Spec id at a position within `selectable_indices`.
    fn id_at(&self, pos: usize) -> Option<&str> {
        self.selectable_indices
            .get(pos)
            .and_then(|&idx| match &self.rows[idx] {
                StatusRow::Spec { id, .. } => Some(id.as_str()),
                _ => None,
            })
    }

    /// Position of a spec id within `selectable_indices`, preferring the
    /// occurrence closest to `anchor`.
    ///
    /// An id can show up in more than one section — a core spec that is also
    /// in the manifest is listed twice — so a plain first-match would pull the
    /// cursor into a section the user was not working in.
    fn position_nearest(&self, id: &str, anchor: usize) -> Option<usize> {
        (0..self.selectable_indices.len())
            .filter(|&pos| self.id_at(pos) == Some(id))
            .min_by_key(|&pos| pos.abs_diff(anchor))
    }

    fn selected_row_index(&self) -> Option<usize> {
        self.selectable_indices.get(self.selected_pos).copied()
    }

    fn select_prev(&mut self) {
        if self.selected_pos > 0 {
            self.selected_pos -= 1;
            self.list_state.select(self.selected_row_index());
        }
    }

    fn select_next(&mut self) {
        if self.selected_pos + 1 < self.selectable_indices.len() {
            self.selected_pos += 1;
            self.list_state.select(self.selected_row_index());
        }
    }

    /// Screen line the cursor currently sits on, counted from the top of the
    /// visible window.
    fn cursor_screen_line(&self) -> usize {
        self.selected_row_index()
            .unwrap_or(0)
            .saturating_sub(self.list_state.offset())
    }

    /// Point the list state at `selected_pos`, scrolling so the cursor lands
    /// back on `screen_line`.
    fn restore_cursor(&mut self, screen_line: usize) {
        let row = self.selected_row_index();
        self.list_state.select(row);
        *self.list_state.offset_mut() = row.unwrap_or(0).saturating_sub(screen_line);
    }

    /// Rebuild the view from fresh app data, preserving the selected spec.
    ///
    /// For actions that leave the ordering alone (detail, metadata edit).
    fn rebuild_preserving_selection(&mut self, app: &App) {
        let screen_line = self.cursor_screen_line();
        let anchor = self.selected_pos;
        let saved_id = self.selected_id().map(|s| s.to_string());
        *self = StatusView::build(app);
        if let Some(pos) = saved_id.and_then(|id| self.position_nearest(&id, anchor)) {
            self.selected_pos = pos;
        }
        self.restore_cursor(screen_line);
    }

    /// Rebuild the view from fresh app data, holding the cursor still.
    ///
    /// For actions that move the selected spec to another section (core
    /// toggle, manifest add/remove). Following the spec to its new home would
    /// drag the window across the dashboard, so instead the cursor keeps its
    /// index and its screen line, landing on the neighbour that slid up into
    /// the vacated slot — the same feel as working down a list in the list
    /// view.
    fn rebuild_holding_cursor(&mut self, app: &App) {
        let screen_line = self.cursor_screen_line();
        let pos = self.selected_pos;
        let next_id = self.id_at(pos + 1).map(str::to_string);
        *self = StatusView::build(app);
        self.selected_pos = next_id
            .and_then(|id| self.position_nearest(&id, pos))
            .unwrap_or(pos)
            .min(self.selectable_indices.len().saturating_sub(1));
        self.restore_cursor(screen_line);
    }
}

/// Entry point for the status dashboard.
pub fn run(paths: &Paths, tool_dirs: &ToolDirs) -> Result<()> {
    let mut app = App::new(paths.clone(), tool_dirs.clone())?;
    let mut terminal = tui::init_terminal()?;

    let result = run_status_loop(&mut terminal, &mut app);

    tui::restore_terminal();

    if let Err(save_err) = app.save_if_dirty() {
        if result.is_ok() {
            return Err(save_err);
        }
        eprintln!("Warning: failed to save changes: {save_err}");
    }

    result
}

/// The main event loop for the status dashboard.
fn run_status_loop(terminal: &mut Term, app: &mut App) -> Result<()> {
    let mut view = StatusView::build(app);

    loop {
        tui::draw(terminal, |frame| render_status(frame, &mut view))?;

        match event::poll_event()? {
            Event::Key(key) => {
                view.status_message = None;
                // Esc is deliberately not an exit — it only ever clears a
                // filter, and this view has none. Only `q` and Ctrl+C quit.
                if event::is_ctrl_c(&key) {
                    return Ok(());
                }
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Up | KeyCode::Char('k') => view.select_prev(),
                    KeyCode::Down | KeyCode::Char('j') => view.select_next(),
                    KeyCode::Enter => {
                        if let Some(id) = view.selected_id() {
                            let id = id.to_string();
                            detail::run_inline(terminal, app, &id)?;
                            view.rebuild_preserving_selection(app);
                        }
                    }
                    KeyCode::Char('e') => {
                        if let Some(id) = view.selected_id() {
                            let id = id.to_string();
                            tui_edit::run_inline(terminal, app, &id)?;
                            view.rebuild_preserving_selection(app);
                        }
                    }
                    KeyCode::Char('c') => {
                        if let Some(id) = view.selected_id() {
                            let id = id.to_string();
                            if let Some(new_core) = app.toggle_core(&id) {
                                let state = if new_core { "on" } else { "off" };
                                view.rebuild_holding_cursor(app);
                                view.status_message = Some(format!("Core {state}: {id}"));
                            }
                        }
                    }
                    KeyCode::Char('a') => {
                        if let Some(id) = view.selected_id() {
                            let id = id.to_string();
                            match app.add_to_manifest(&id)? {
                                AddResult::Added => {
                                    view.rebuild_holding_cursor(app);
                                    view.status_message = Some(format!("✓ Added: {id}"));
                                }
                                AddResult::AlreadyPresent => {
                                    view.status_message =
                                        Some(format!("Already in manifest: {id}"));
                                }
                                AddResult::NoProject => {
                                    view.status_message = Some("No project detected".to_string());
                                }
                                AddResult::SpecNotFound => {
                                    view.status_message = Some(format!("Spec not found: {id}"));
                                }
                            }
                        }
                    }
                    KeyCode::Char('r') => {
                        if let Some(id) = view.selected_id() {
                            let id = id.to_string();
                            match app.remove_from_manifest(&id)? {
                                RemoveResult::Removed => {
                                    view.rebuild_holding_cursor(app);
                                    view.status_message = Some(format!("✓ Removed: {id}"));
                                }
                                RemoveResult::NotPresent => {
                                    view.status_message = Some(format!("Not in manifest: {id}"));
                                }
                                RemoveResult::NoManifest => {
                                    view.status_message = Some("No manifest found".to_string());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::Tick | Event::Resize(_, _) => {}
        }
    }
}

/// Render the status dashboard.
fn render_status(frame: &mut Frame, view: &mut StatusView) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),    // status list
            Constraint::Length(1), // status message
            Constraint::Length(1), // help bar
        ])
        .split(frame.area());

    let items: Vec<ListItem> = view
        .rows
        .iter()
        .map(|row| match row {
            StatusRow::Header(text) => {
                ListItem::new(Line::from(Span::styled(text.as_str(), theme::HEADER)))
            }
            StatusRow::Spec {
                id,
                spec_type,
                section,
                note,
                drift,
            } => {
                let icon = match section {
                    StatusSection::Core | StatusSection::Session | StatusSection::Manifest => "✓",
                    StatusSection::Cold => "○",
                };
                let icon_style = match section {
                    StatusSection::Core | StatusSection::Session | StatusSection::Manifest => {
                        theme::SUCCESS
                    }
                    StatusSection::Cold => theme::DIM,
                };
                let type_style = theme::type_style(spec_type);

                let mut spans = vec![
                    Span::styled(format!("  {icon} "), icon_style),
                    Span::styled(format!("{:<6}", spec_type), type_style),
                    Span::styled(format!("  {} ", drift.marker()), theme::drift_style(*drift)),
                    Span::raw(id.as_str()),
                ];
                if let Some(note) = note {
                    spans.push(Span::styled(format!(" {note}"), theme::WARNING));
                }
                ListItem::new(Line::from(spans))
            }
            StatusRow::Empty(text) => {
                ListItem::new(Line::from(Span::styled(text.as_str(), theme::DIM)))
            }
            StatusRow::Blank => ListItem::new(Line::from("")),
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Skills Status ")
                .borders(Borders::ALL),
        )
        .highlight_style(theme::SELECTED);
    frame.render_stateful_widget(list, chunks[0], &mut view.list_state);

    if let Some(ref msg) = view.status_message {
        let status = Paragraph::new(msg.as_str()).style(theme::SUCCESS);
        frame.render_widget(status, chunks[1]);
    }

    let help_text = Line::from(
        [
            (" ↑↓/jk", " navigate  "),
            ("Enter", " view  "),
            ("c", " core  "),
            ("e", " edit  "),
            ("a", " add  "),
            ("r", " remove  "),
            ("q", " quit"),
        ]
        .iter()
        .flat_map(|(key, label)| {
            [
                Span::styled(*key, theme::HEADER),
                Span::styled(*label, theme::HELP_BAR),
            ]
        })
        .collect::<Vec<_>>(),
    );
    frame.render_widget(Paragraph::new(help_text), chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::spec::Spec;
    use crate::library::Library;

    /// An App over a library of `ids`, the ones in `core` flagged as core.
    ///
    /// The manifest is dropped so the dashboard shows only the core and cold
    /// sections, independent of whatever project the tests run in.
    fn test_app(ids: &[&str], core: &[&str]) -> (App, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::from_roots(tmp.path(), tmp.path(), tmp.path(), tmp.path());
        let library = Library {
            version: 1,
            specs: ids
                .iter()
                .map(|id| Spec {
                    core: core.contains(id),
                    ..Spec::new(*id, SpecType::Skill, *id, "desc")
                })
                .collect(),
        };
        library.save(&paths).unwrap();
        let mut app = App::new(paths, ToolDirs::builtin(tmp.path())).unwrap();
        app.manifest = None;
        app.manifest_ids.clear();
        (app, tmp)
    }

    /// Move the cursor onto `id`.
    fn select(view: &mut StatusView, id: &str) {
        view.selected_pos = view.position_nearest(id, 0).expect("id is selectable");
        view.list_state.select(view.selected_row_index());
    }

    #[test]
    fn toggling_core_leaves_the_cursor_on_the_next_neighbour() {
        let (mut app, _tmp) = test_app(&["a", "b", "c", "d"], &["a", "b", "c"]);
        let mut view = StatusView::build(&app);
        select(&mut view, "b");

        app.toggle_core("b");
        view.rebuild_holding_cursor(&app);

        assert_eq!(view.selected_id(), Some("c"));
    }

    #[test]
    fn toggling_core_holds_the_cursor_on_its_screen_line() {
        let (mut app, _tmp) = test_app(&["a", "b", "c", "d"], &["a", "b", "c"]);
        let mut view = StatusView::build(&app);
        select(&mut view, "b");
        // Pretend the last render had scrolled so the cursor sat on line 2.
        *view.list_state.offset_mut() = view.selected_row_index().unwrap() - 2;

        app.toggle_core("b");
        view.rebuild_holding_cursor(&app);

        assert_eq!(view.cursor_screen_line(), 2);
    }

    #[test]
    fn toggling_core_on_the_last_spec_keeps_the_cursor_at_the_bottom() {
        let (mut app, _tmp) = test_app(&["a", "b", "c"], &["a"]);
        let mut view = StatusView::build(&app);
        select(&mut view, "c");
        let pos = view.selected_pos;

        // "c" moves up into the core section; nothing follows it, so the
        // cursor holds its index rather than trailing the spec upwards.
        app.toggle_core("c");
        view.rebuild_holding_cursor(&app);

        assert_eq!(view.selected_pos, pos);
        assert_eq!(view.selected_id(), Some("b"));
    }

    #[test]
    fn holding_the_cursor_never_lands_back_on_the_toggled_spec() {
        let (mut app, _tmp) = test_app(&["a", "b", "c", "d"], &["a", "b"]);
        let mut view = StatusView::build(&app);
        for id in ["a", "b", "c"] {
            select(&mut view, id);
            app.toggle_core(id);
            view.rebuild_holding_cursor(&app);
            assert_ne!(view.selected_id(), Some(id));
        }
    }

    #[test]
    fn edits_keep_the_cursor_on_the_same_spec() {
        let (app, _tmp) = test_app(&["a", "b", "c"], &["a"]);
        let mut view = StatusView::build(&app);
        select(&mut view, "b");
        *view.list_state.offset_mut() = view.selected_row_index().unwrap() - 1;

        view.rebuild_preserving_selection(&app);

        assert_eq!(view.selected_id(), Some("b"));
        assert_eq!(view.cursor_screen_line(), 1);
    }
}
