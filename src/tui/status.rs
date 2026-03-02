//! Status dashboard — interactive version of `akm skills status`.
//!
//! Displays the same sections as the plain output (core, session, manifest,
//! cold) but in a scrollable TUI with navigation and actions.
//!
//! Key bindings:
//! - `↑`/`↓` — navigate between specs (across sections)
//! - `Enter` — view detail for selected spec
//! - `c` — toggle core for selected spec
//! - `a` — add selected to manifest
//! - `r` — remove selected from manifest
//! - `q` / `Esc` — quit

use crate::error::Result;
use crate::library::spec::SpecType;
use crate::library::tool_dirs::ToolDirs;
use crate::paths::Paths;
use crate::tui::app::{AddResult, App, RemoveResult};
use crate::tui::detail;
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
        self.selectable_indices
            .get(self.selected_pos)
            .and_then(|&idx| match &self.rows[idx] {
                StatusRow::Spec { id, .. } => Some(id.as_str()),
                _ => None,
            })
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

    /// Rebuild the view from fresh app data, preserving the selected spec.
    fn rebuild_preserving_selection(&mut self, app: &App) {
        let saved_id = self.selected_id().map(|s| s.to_string());
        *self = StatusView::build(app);
        if let Some(ref id) = saved_id {
            if let Some(pos) = self.selectable_indices.iter().position(|&idx| {
                matches!(&self.rows[idx], StatusRow::Spec { id: row_id, .. } if row_id == id)
            }) {
                self.selected_pos = pos;
                self.list_state.select(self.selected_row_index());
            }
        }
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
                if event::is_ctrl_c(&key) || event::is_escape(&key) {
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
                    KeyCode::Char('c') => {
                        if let Some(id) = view.selected_id() {
                            let id = id.to_string();
                            if let Some(new_core) = app.toggle_core(&id) {
                                let state = if new_core { "on" } else { "off" };
                                view.rebuild_preserving_selection(app);
                                view.status_message = Some(format!("Core {state}: {id}"));
                            }
                        }
                    }
                    KeyCode::Char('a') => {
                        if let Some(id) = view.selected_id() {
                            let id = id.to_string();
                            match app.add_to_manifest(&id)? {
                                AddResult::Added => {
                                    view.rebuild_preserving_selection(app);
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
                                    view.rebuild_preserving_selection(app);
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
                    Span::raw(format!("  {id}")),
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

    let help = Paragraph::new(" ↑↓ navigate  Enter view  c core  a add  r remove  q quit")
        .style(theme::HELP_BAR);
    frame.render_widget(help, chunks[2]);
}
