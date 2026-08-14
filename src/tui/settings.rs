//! Settings panel TUI view — a single flat, scrolling list over [`Config`].
//!
//! Every write goes straight through [`Config::save`]; there is no Submit
//! button and no second source of truth. The model (the [`Row`] list and the
//! pure `apply_*` transitions) is unit-tested; the render + event loop are
//! verified manually, matching the house style.
//!
//! This retires the old shared-registries menu: adding, removing and verifying
//! a shared registry all live here now, alongside the personal registry,
//! artifacts and feature settings.
//!
//! A `Shared(name)` row carries only the registry name today. It is a dedicated
//! variant rather than a bare string so it can grow to `{url, path}` (issue #47)
//! without reshaping the row list.

use crate::commands::skills::shared;
use crate::config::{Config, ConfigKey, Feature};
use crate::error::Result;
use crate::paths::Paths;
use crate::tui::event::{self, Event};
use crate::tui::input::TextInput;
use crate::tui::theme;
use crate::tui::{self, EventOutcome, Term};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

/// One line in the settings panel.
///
/// `Header` rows are captions and never take the cursor; everything else is
/// selectable. The order the panel builds them in is local-first: the personal
/// registry, then artifacts, then features, then the shared registries.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    /// A dim, non-selectable section caption.
    Header(&'static str),
    /// `registry.url` — editable text.
    RegistryUrl,
    /// `artifacts.remote` — editable text.
    ArtifactsRemote,
    /// `artifacts.dir` — editable text.
    ArtifactsDir,
    /// `artifacts.auto-push` — a checkbox.
    ArtifactsAutoPush,
    /// One feature toggle — a checkbox.
    Feature(Feature),
    /// One configured shared registry, addressed by name — editable URL,
    /// cleared to remove.
    Shared(String),
    /// The "+ add a shared registry" affordance.
    AddShared,
}

/// The three features, in the fixed display order the panel uses.
const FEATURE_ORDER: [Feature; 3] = [Feature::Skills, Feature::Artifacts, Feature::Instructions];

/// Column at which a text row's value starts, so labels line up.
const LABEL_WIDTH: usize = 24;

impl Row {
    /// The config key this row edits, if it maps to a single one.
    ///
    /// `Feature` rows toggle a set membership rather than a keyed scalar, so
    /// they have none (use [`toggle_feature`]); headers and `AddShared` have
    /// none either.
    fn config_key(&self) -> Option<ConfigKey> {
        match self {
            Row::RegistryUrl => Some(ConfigKey::RegistryUrl),
            Row::ArtifactsRemote => Some(ConfigKey::ArtifactsRemote),
            Row::ArtifactsDir => Some(ConfigKey::ArtifactsDir),
            Row::ArtifactsAutoPush => Some(ConfigKey::ArtifactsAutoPush),
            Row::Shared(name) => Some(ConfigKey::Shared(name.clone())),
            Row::Header(_) | Row::Feature(_) | Row::AddShared => None,
        }
    }

    /// Whether the cursor can land on this row. Headers cannot.
    fn is_selectable(&self) -> bool {
        !matches!(self, Row::Header(_))
    }

    /// Whether this row is a checkbox that `space`/`Enter` toggles.
    fn is_checkbox(&self) -> bool {
        matches!(self, Row::ArtifactsAutoPush | Row::Feature(_))
    }

    /// Whether this row edits free text (registry URLs, artifacts dir/remote).
    fn is_text(&self) -> bool {
        matches!(
            self,
            Row::RegistryUrl | Row::ArtifactsRemote | Row::ArtifactsDir | Row::Shared(_)
        )
    }
}

/// Build the panel's rows from a config, local-first.
fn build_rows(config: &Config) -> Vec<Row> {
    let mut rows = vec![
        Row::Header("Personal registry"),
        Row::RegistryUrl,
        Row::Header("Artifacts"),
        Row::ArtifactsRemote,
        Row::ArtifactsDir,
        Row::ArtifactsAutoPush,
        Row::Header("Features"),
    ];
    rows.extend(FEATURE_ORDER.map(Row::Feature));
    rows.push(Row::Header("Shared registries"));
    rows.extend(config.shared.keys().map(|name| Row::Shared(name.clone())));
    rows.push(Row::AddShared);
    rows
}

/// Toggle `artifacts.auto-push`, routed through [`ConfigKey::set`] so it shares
/// the command path's validation. The value is generated, so `set` cannot fail.
fn toggle_auto_push(config: &mut Config) {
    let next = if config.artifacts.auto_push {
        "false"
    } else {
        "true"
    };
    let _ = ConfigKey::ArtifactsAutoPush.set(config, next);
}

/// Toggle a feature's membership in the enabled set.
fn toggle_feature(config: &mut Config, feature: Feature) {
    if !config.features.remove(&feature) {
        config.features.insert(feature);
    }
}

/// Input mode for the settings panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Letters are commands; the cursor moves over selectable rows.
    Normal,
    /// Editing a text config key (the row under the cursor).
    Edit,
    /// Entering the name for a new shared registry.
    AddName,
    /// Entering the URL for the new shared registry named in `add_name`.
    AddUrl,
}

/// State for the settings panel.
struct Settings {
    /// The live config, mutated and saved in place on every change.
    config: Config,
    /// Flattened rows rebuilt from `config` after each change.
    rows: Vec<Row>,
    /// Index of the highlighted row (always a selectable one).
    selected: usize,
    /// Current input mode.
    mode: Mode,
    /// Text field for the active edit / add prompt.
    input: TextInput,
    /// While in [`Mode::AddUrl`], the name captured in [`Mode::AddName`].
    add_name: Option<String>,
    /// Inline status (verify results, add confirmations, errors).
    status: Option<String>,
}

impl Settings {
    fn new(config: Config) -> Self {
        let rows = build_rows(&config);
        let mut s = Self {
            config,
            rows,
            selected: 0,
            mode: Mode::Normal,
            input: TextInput::default(),
            add_name: None,
            status: None,
        };
        s.ensure_selectable();
        s
    }

    /// Rebuild rows from the current config and keep the cursor on something
    /// selectable (rows shift when a shared registry is added or removed).
    fn rebuild(&mut self) {
        self.rows = build_rows(&self.config);
        self.ensure_selectable();
    }

    /// Move `selected` onto a selectable row, forward then backward.
    fn ensure_selectable(&mut self) {
        if self.rows.is_empty() {
            self.selected = 0;
            return;
        }
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len() - 1;
        }
        if self.rows[self.selected].is_selectable() {
            return;
        }
        for i in self.selected..self.rows.len() {
            if self.rows[i].is_selectable() {
                self.selected = i;
                return;
            }
        }
        for i in (0..self.selected).rev() {
            if self.rows[i].is_selectable() {
                self.selected = i;
                return;
            }
        }
    }

    fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected)
    }

    /// Move the cursor to the previous selectable row.
    fn select_prev(&mut self) {
        let mut i = self.selected;
        while i > 0 {
            i -= 1;
            if self.rows[i].is_selectable() {
                self.selected = i;
                return;
            }
        }
    }

    /// Move the cursor to the next selectable row.
    fn select_next(&mut self) {
        let mut i = self.selected;
        while i + 1 < self.rows.len() {
            i += 1;
            if self.rows[i].is_selectable() {
                self.selected = i;
                return;
            }
        }
    }

    /// The value shown for a text/checkbox row, straight from the config.
    fn value_of(&self, row: &Row) -> String {
        match row {
            Row::ArtifactsAutoPush => checkbox(self.config.artifacts.auto_push),
            Row::Feature(f) => checkbox(self.config.features.contains(f)),
            _ => match row.config_key() {
                Some(key) => {
                    let v = key.get(&self.config);
                    if v.is_empty() {
                        "(not set)".to_string()
                    } else {
                        v
                    }
                }
                None => String::new(),
            },
        }
    }
}

/// `[x]` or `[ ]`.
fn checkbox(on: bool) -> String {
    if on {
        "[x]".to_string()
    } else {
        "[ ]".to_string()
    }
}

/// Human label for a row (the thing left of its value).
fn row_label(row: &Row) -> String {
    match row {
        Row::Header(t) => (*t).to_string(),
        Row::RegistryUrl => "registry.url".to_string(),
        Row::ArtifactsRemote => "artifacts.remote".to_string(),
        Row::ArtifactsDir => "artifacts.dir".to_string(),
        Row::ArtifactsAutoPush => "artifacts.auto-push".to_string(),
        Row::Feature(f) => f.to_string(),
        Row::Shared(name) => format!("shared.{name}"),
        Row::AddShared => "+ add a shared registry".to_string(),
    }
}

/// Entry point for the interactive settings panel.
pub fn run(paths: &Paths, config: Config) -> Result<()> {
    let mut settings = Settings::new(config);
    let mut terminal = tui::init_terminal()?;
    let result = run_settings_loop(&mut terminal, &mut settings, paths);
    tui::restore_terminal();
    result
}

/// The main event loop for the settings panel.
fn run_settings_loop(terminal: &mut Term, settings: &mut Settings, paths: &Paths) -> Result<()> {
    loop {
        tui::draw(terminal, |frame| render(frame, settings))?;
        match event::poll_event()? {
            Event::Key(key) => {
                if handle_key(key, settings, paths)? == EventOutcome::Exit {
                    return Ok(());
                }
            }
            Event::Tick => {}
            Event::Resize(_, _) => {}
        }
    }
}

/// Dispatch a key on the current mode. `Ctrl+C` always exits.
fn handle_key(key: KeyEvent, settings: &mut Settings, paths: &Paths) -> Result<EventOutcome> {
    if event::is_ctrl_c(&key) {
        return Ok(EventOutcome::Exit);
    }
    match settings.mode {
        Mode::Normal => handle_normal_key(key, settings, paths),
        Mode::Edit => handle_edit_key(key, settings, paths),
        Mode::AddName => handle_add_name_key(key, settings),
        Mode::AddUrl => handle_add_url_key(key, settings, paths),
    }
}

/// Normal mode: navigate, toggle, edit, add, verify, quit.
fn handle_normal_key(
    key: KeyEvent,
    settings: &mut Settings,
    paths: &Paths,
) -> Result<EventOutcome> {
    settings.status = None;

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return Ok(EventOutcome::Exit),
        KeyCode::Up | KeyCode::Char('k') => settings.select_prev(),
        KeyCode::Down | KeyCode::Char('j') => settings.select_next(),

        KeyCode::Char(' ') => {
            if let Some(row) = settings.selected_row().cloned() {
                if row.is_checkbox() {
                    toggle_checkbox(settings, &row, paths)?;
                }
            }
        }

        KeyCode::Enter => {
            let Some(row) = settings.selected_row().cloned() else {
                return Ok(EventOutcome::Continue);
            };
            if row.is_checkbox() {
                toggle_checkbox(settings, &row, paths)?;
            } else if row.is_text() {
                start_edit(settings, &row);
            } else if row == Row::AddShared {
                settings.mode = Mode::AddName;
                settings.input = TextInput::default();
            }
        }

        KeyCode::Char('a') => {
            settings.mode = Mode::AddName;
            settings.input = TextInput::default();
        }

        KeyCode::Char('v') => {
            if let Some(Row::Shared(name)) = settings.selected_row().cloned().as_ref() {
                verify_shared(settings, name, paths);
            }
        }

        _ => {}
    }

    Ok(EventOutcome::Continue)
}

/// Flip a checkbox row and persist immediately.
fn toggle_checkbox(settings: &mut Settings, row: &Row, paths: &Paths) -> Result<()> {
    match row {
        Row::ArtifactsAutoPush => toggle_auto_push(&mut settings.config),
        Row::Feature(f) => toggle_feature(&mut settings.config, *f),
        _ => return Ok(()),
    }
    settings.config.save(paths)?;
    Ok(())
}

/// Open the text editor for a text row, prefilled with its current value.
fn start_edit(settings: &mut Settings, row: &Row) {
    let current = row
        .config_key()
        .map(|k| k.get(&settings.config))
        .unwrap_or_default();
    settings.input = TextInput::new(&current);
    settings.mode = Mode::Edit;
}

/// Verify a shared registry is reachable (explicit, blocking network call).
fn verify_shared(settings: &mut Settings, name: &str, paths: &Paths) {
    match shared::open(paths, &settings.config, name) {
        Ok(registry) => {
            let outcome = shared::refresh(&registry);
            settings.status = Some(format!("{name}: {outcome}"));
        }
        Err(e) => settings.status = Some(format!("{name}: {e}")),
    }
}

/// Edit mode: commit on Enter, cancel on Esc.
fn handle_edit_key(key: KeyEvent, settings: &mut Settings, paths: &Paths) -> Result<EventOutcome> {
    if event::is_escape(&key) {
        settings.mode = Mode::Normal;
        return Ok(EventOutcome::Continue);
    }
    match key.code {
        KeyCode::Enter => {
            let row = settings.selected_row().cloned();
            let value = settings.input.value().to_string();
            settings.mode = Mode::Normal;
            if let Some(row) = row {
                if let Some(key) = row.config_key() {
                    let is_shared = matches!(row, Row::Shared(_));
                    let cleared = value.is_empty();
                    key.set(&mut settings.config, &value)?;
                    settings.config.save(paths)?;
                    // Emptying a shared URL removes it — reap the cache checkout,
                    // the same reconciliation the scriptable removal path gets.
                    if is_shared && cleared {
                        shared::sweep_orphans(paths, &settings.config);
                    }
                    settings.rebuild();
                }
            }
        }
        KeyCode::Backspace => settings.input.backspace(),
        KeyCode::Left => settings.input.left(),
        KeyCode::Right => settings.input.right(),
        KeyCode::Char(c) => settings.input.insert(c),
        _ => {}
    }
    Ok(EventOutcome::Continue)
}

/// Add-name mode: validate the name, then move to the URL prompt.
fn handle_add_name_key(key: KeyEvent, settings: &mut Settings) -> Result<EventOutcome> {
    if event::is_escape(&key) {
        settings.mode = Mode::Normal;
        return Ok(EventOutcome::Continue);
    }
    match key.code {
        KeyCode::Enter => {
            let name = settings.input.value().trim().to_string();
            if name.is_empty() {
                settings.mode = Mode::Normal;
            } else if !crate::config::is_valid_shared_name(&name) {
                settings.status = Some("Name must be a single segment (no '.')".to_string());
            } else if settings.config.shared.contains_key(&name) {
                settings.status = Some(format!("'{name}' is already configured"));
            } else {
                settings.add_name = Some(name);
                settings.input = TextInput::default();
                settings.mode = Mode::AddUrl;
                settings.status = None;
            }
        }
        KeyCode::Backspace => settings.input.backspace(),
        KeyCode::Left => settings.input.left(),
        KeyCode::Right => settings.input.right(),
        KeyCode::Char(c) => settings.input.insert(c),
        _ => {}
    }
    Ok(EventOutcome::Continue)
}

/// Add-url mode: an opaque URL, inserted and saved on Enter.
fn handle_add_url_key(
    key: KeyEvent,
    settings: &mut Settings,
    paths: &Paths,
) -> Result<EventOutcome> {
    if event::is_escape(&key) {
        settings.mode = Mode::Normal;
        settings.add_name = None;
        return Ok(EventOutcome::Continue);
    }
    match key.code {
        KeyCode::Enter => {
            let url = settings.input.value().trim().to_string();
            let name = settings.add_name.take();
            settings.mode = Mode::Normal;
            if let Some(name) = name {
                if !url.is_empty() {
                    // Opaque URL: handed to git clone later, so no host-sniffing.
                    settings.config.shared.insert(name.clone(), url);
                    settings.config.save(paths)?;
                    settings.status = Some(format!("Added '{name}'"));
                    settings.rebuild();
                }
            }
        }
        KeyCode::Backspace => settings.input.backspace(),
        KeyCode::Left => settings.input.left(),
        KeyCode::Right => settings.input.right(),
        KeyCode::Char(c) => settings.input.insert(c),
        _ => {}
    }
    Ok(EventOutcome::Continue)
}

/// Render the panel: prompt bar, row list, status, legend.
fn render(frame: &mut Frame, settings: &Settings) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // prompt bar
            Constraint::Min(5),    // rows
            Constraint::Length(1), // status
            Constraint::Length(1), // legend
        ])
        .split(frame.area());

    render_prompt_bar(frame, chunks[0], settings);
    render_rows(frame, chunks[1], settings);

    if let Some(ref msg) = settings.status {
        frame.render_widget(
            Paragraph::new(msg.as_str()).style(theme::WARNING),
            chunks[2],
        );
    }
    render_legend(frame, chunks[3], settings.mode);
}

/// Top bar: the active edit / add prompt, or a hint in normal mode.
fn render_prompt_bar(frame: &mut Frame, area: Rect, settings: &Settings) {
    let prefix = match settings.mode {
        Mode::Normal => {
            let hint = Line::from(vec![Span::styled(
                " akm settings — changes save as you go",
                theme::DIM,
            )]);
            frame.render_widget(Paragraph::new(hint), area);
            return;
        }
        Mode::Edit => format!(
            " {} ▸ ",
            row_label(settings.selected_row().unwrap_or(&Row::RegistryUrl))
        ),
        Mode::AddName => " New shared registry name ▸ ".to_string(),
        Mode::AddUrl => format!(
            " URL for '{}' ▸ ",
            settings.add_name.as_deref().unwrap_or("")
        ),
    };
    let line = Line::from(vec![
        Span::styled(prefix, theme::SEARCH_BAR),
        Span::raw(settings.input.value()),
        Span::styled("█", theme::SEARCH_BAR),
    ]);
    frame.render_widget(Paragraph::new(line).style(theme::SEARCH_BAR), area);
}

/// Render the flat row list.
fn render_rows(frame: &mut Frame, area: Rect, settings: &Settings) {
    let items: Vec<ListItem> = settings
        .rows
        .iter()
        .map(|row| match row {
            Row::Header(title) => ListItem::new(vec![
                Line::from(""),
                Line::from(Span::styled(format!("  {title}"), theme::HEADER)),
            ]),
            Row::ArtifactsAutoPush | Row::Feature(_) => {
                let line = format!("  {} {}", settings.value_of(row), row_label(row));
                ListItem::new(Line::from(line))
            }
            Row::AddShared => ListItem::new(Line::from(Span::styled(
                format!("  {}", row_label(row)),
                theme::SUCCESS,
            ))),
            _ => {
                let line = format!(
                    "  {:<width$}{}",
                    row_label(row),
                    settings.value_of(row),
                    width = LABEL_WIDTH
                );
                ListItem::new(Line::from(line))
            }
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(settings.selected));

    let list = List::new(items)
        .block(Block::default().borders(Borders::NONE))
        .highlight_style(theme::SELECTED);
    frame.render_stateful_widget(list, area, &mut state);
}

/// Bottom legend, per mode.
fn render_legend(frame: &mut Frame, area: Rect, mode: Mode) {
    let pairs: &[(&str, &str)] = match mode {
        Mode::Normal => &[
            (" ↑↓/jk", " navigate  "),
            ("Enter", " edit  "),
            ("space", " toggle  "),
            ("a", " add  "),
            ("v", " verify  "),
            ("q", " quit"),
        ],
        Mode::Edit | Mode::AddName | Mode::AddUrl => {
            &[(" Enter", " confirm  "), ("Esc", " cancel")]
        }
    };
    let legend = Line::from(
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
    frame.render_widget(Paragraph::new(legend), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_rows_is_local_first_with_one_row_per_shared() {
        let mut config = Config::default();
        config.features.insert(Feature::Skills);
        config
            .shared
            .insert("acme".into(), "git@example.com:acme.git".into());
        config
            .shared
            .insert("team".into(), "git@example.com:team.git".into());

        let rows = build_rows(&config);

        assert_eq!(
            rows,
            vec![
                Row::Header("Personal registry"),
                Row::RegistryUrl,
                Row::Header("Artifacts"),
                Row::ArtifactsRemote,
                Row::ArtifactsDir,
                Row::ArtifactsAutoPush,
                Row::Header("Features"),
                Row::Feature(Feature::Skills),
                Row::Feature(Feature::Artifacts),
                Row::Feature(Feature::Instructions),
                Row::Header("Shared registries"),
                Row::Shared("acme".into()), // BTreeMap order
                Row::Shared("team".into()),
                Row::AddShared,
            ]
        );
    }

    #[test]
    fn headers_are_not_selectable_but_values_are() {
        assert!(!Row::Header("x").is_selectable());
        assert!(Row::RegistryUrl.is_selectable());
        assert!(Row::AddShared.is_selectable());
        assert!(Row::Feature(Feature::Skills).is_selectable());
    }

    #[test]
    fn toggle_auto_push_flips_the_flag() {
        let mut config = Config::default();
        assert!(config.artifacts.auto_push); // default true
        toggle_auto_push(&mut config);
        assert!(!config.artifacts.auto_push);
        toggle_auto_push(&mut config);
        assert!(config.artifacts.auto_push);
    }

    #[test]
    fn toggle_feature_adds_then_removes() {
        let mut config = Config::default();
        assert!(!config.features.contains(&Feature::Skills));
        toggle_feature(&mut config, Feature::Skills);
        assert!(config.features.contains(&Feature::Skills));
        toggle_feature(&mut config, Feature::Skills);
        assert!(!config.features.contains(&Feature::Skills));
    }

    #[test]
    fn committing_registry_url_text_lands_via_the_config_key() {
        let mut config = Config::default();
        let key = Row::RegistryUrl.config_key().unwrap();
        key.set(&mut config, "https://example.com/mine.git")
            .unwrap();
        assert_eq!(config.registry_url(), Some("https://example.com/mine.git"));
    }

    #[test]
    fn committing_a_shared_url_inserts_and_emptying_it_removes() {
        let mut config = Config::default();
        let key = Row::Shared("acme".into()).config_key().unwrap();

        key.set(&mut config, "git@example.com:acme.git").unwrap();
        assert_eq!(
            config.shared.get("acme").map(String::as_str),
            Some("git@example.com:acme.git")
        );

        // The empty value is the remove verb (the config-key contract).
        key.set(&mut config, "").unwrap();
        assert!(!config.shared.contains_key("acme"));
    }

    /// The first landing spot skips the leading header, and navigation never
    /// stops on a header.
    #[test]
    fn navigation_skips_headers() {
        let settings = Settings::new(Config::default());
        // Row 0 is Header("Personal registry"); the cursor starts on RegistryUrl.
        assert_eq!(settings.selected_row(), Some(&Row::RegistryUrl));

        let mut settings = settings;
        settings.select_next(); // → ArtifactsRemote (skips the "Artifacts" header)
        assert_eq!(settings.selected_row(), Some(&Row::ArtifactsRemote));
    }
}
