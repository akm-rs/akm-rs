//! Artifacts explorer TUI view — a two-pane browser over the per-project
//! artifacts tree.
//!
//! Left pane: a lazily-expanding directory tree. Right pane: a live plain-text
//! preview of the selected file. `Enter`/`o` on a file drops to `$EDITOR` and
//! comes back (see [`crate::tui::edit_file_in_terminal`]). There are no git
//! side effects here — committing is owned by session exit / `akm artifacts
//! sync`.
//!
//! Structure mirrors [`crate::tui::list`]: a pure state machine
//! ([`Explorer`] and its methods) that is unit-tested with synthetic key
//! events, and a render/loop shell that is verified manually.

use crate::artifacts::{scan_dir, Entry};
use crate::config::Config;
use crate::error::Result;
use crate::git::Git;
use crate::paths::Paths;
use crate::tui::event::{self, Event};
use crate::tui::theme;
use crate::tui::{self, Term};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use std::path::{Path, PathBuf};

/// One visible row: a filesystem entry at a tree depth, and whether a directory
/// row is currently expanded.
struct TreeNode {
    entry: Entry,
    depth: usize,
    expanded: bool,
}

/// State for the artifacts explorer.
struct Explorer {
    /// The artifacts directory being browsed (tree root).
    root: PathBuf,
    /// Flattened visible rows. Children exist only under expanded dirs (lazy).
    rows: Vec<TreeNode>,
    /// Index of the highlighted row.
    selected: usize,
    /// Inline status (e.g. an editor spawn failure), cleared on the next key.
    status: Option<String>,
}

/// What a handled key asks the loop to do. Pure — no terminal, so the state
/// machine is testable with synthetic events.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    /// Stay in the loop.
    Continue,
    /// Leave the explorer.
    Exit,
    /// Suspend and open this file in `$EDITOR`.
    OpenFile(PathBuf),
}

impl Explorer {
    /// Build an explorer over `root`, listing only its top level (depth 0).
    fn new(root: PathBuf) -> Result<Self> {
        let rows = scan_dir(&root)?
            .into_iter()
            .map(|entry| TreeNode {
                entry,
                depth: 0,
                expanded: false,
            })
            .collect();
        Ok(Self {
            root,
            rows,
            selected: 0,
            status: None,
        })
    }

    /// Expand a top-level project directory and place the cursor on it.
    ///
    /// Called on open with the current git project's basename, so the tree
    /// lands focused on "where the LLM just wrote". A no-op if no depth-0 dir
    /// matches (the project has no artifacts yet, or we are not in one).
    fn focus_project(&mut self, name: &str) {
        if let Some(idx) = self
            .rows
            .iter()
            .position(|n| n.depth == 0 && n.entry.is_dir && n.entry.name == name)
        {
            let _ = self.expand(idx); // best-effort; selection is the point
            self.selected = idx;
        }
    }

    /// The highlighted row, if any.
    fn selected_node(&self) -> Option<&TreeNode> {
        self.rows.get(self.selected)
    }

    /// Move the cursor up one row.
    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move the cursor down one row.
    fn move_down(&mut self) {
        if self.selected + 1 < self.rows.len() {
            self.selected += 1;
        }
    }

    /// Expand the directory at `idx`, splicing its children in right after it.
    ///
    /// A no-op on a leaf or an already-expanded row. Children are scanned only
    /// here, so an unexpanded subtree costs nothing — the launch stays instant.
    fn expand(&mut self, idx: usize) -> Result<()> {
        let Some(node) = self.rows.get(idx) else {
            return Ok(());
        };
        if !node.entry.is_dir || node.expanded {
            return Ok(());
        }
        let depth = node.depth;
        let path = node.entry.path.clone();
        let children: Vec<TreeNode> = scan_dir(&path)?
            .into_iter()
            .map(|entry| TreeNode {
                entry,
                depth: depth + 1,
                expanded: false,
            })
            .collect();
        let at = idx + 1;
        self.rows.splice(at..at, children);
        self.rows[idx].expanded = true;
        Ok(())
    }

    /// Collapse the directory at `idx`, removing the contiguous deeper block
    /// that follows it (so nested expansions vanish too).
    fn collapse(&mut self, idx: usize) {
        let Some(node) = self.rows.get(idx) else {
            return;
        };
        if !node.entry.is_dir || !node.expanded {
            return;
        }
        let depth = node.depth;
        let mut end = idx + 1;
        while end < self.rows.len() && self.rows[end].depth > depth {
            end += 1;
        }
        self.rows.drain(idx + 1..end);
        self.rows[idx].expanded = false;
    }

    /// Move the cursor to the nearest shallower row (the parent directory).
    fn jump_to_parent(&mut self, idx: usize) {
        let Some(node) = self.rows.get(idx) else {
            return;
        };
        let depth = node.depth;
        if depth == 0 {
            return;
        }
        for i in (0..idx).rev() {
            if self.rows[i].depth == depth - 1 {
                self.selected = i;
                return;
            }
        }
    }

    /// Clamp `selected` into range after the row set changes.
    fn clamp_selection(&mut self) {
        if self.rows.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.rows.len() {
            self.selected = self.rows.len() - 1;
        }
    }
}

/// Entry point for the interactive artifacts explorer.
///
/// The caller ([`crate::commands::artifacts::explorer`]) has already confirmed
/// `root` exists and that stdout is a TTY.
pub fn run(paths: &Paths, config: &Config) -> Result<()> {
    let root = config.artifacts_dir(paths);
    let mut explorer = Explorer::new(root)?;

    // Auto-focus the current project, mirroring how the list view finds it.
    if let Some(name) = Git::toplevel(None)
        .ok()
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
    {
        explorer.focus_project(&name);
    }

    let mut terminal = tui::init_terminal()?;
    let result = run_explorer_loop(&mut terminal, &mut explorer);
    tui::restore_terminal();
    result
}

/// The main event loop for the explorer.
fn run_explorer_loop(terminal: &mut Term, explorer: &mut Explorer) -> Result<()> {
    loop {
        explorer.clamp_selection();
        tui::draw(terminal, |frame| render(frame, explorer))?;

        match event::poll_event()? {
            Event::Key(key) => match handle_key(key, explorer)? {
                Action::Continue => {}
                Action::Exit => return Ok(()),
                Action::OpenFile(path) => {
                    if let Err(e) = tui::edit_file_in_terminal(terminal, &path) {
                        explorer.status = Some(e.to_string());
                    }
                }
            },
            Event::Tick => {}
            Event::Resize(_, _) => {}
        }
    }
}

/// Handle one key. Pure: navigation and expand/collapse mutate `explorer`,
/// opening a file is returned to the loop (which owns the terminal).
fn handle_key(key: KeyEvent, explorer: &mut Explorer) -> Result<Action> {
    explorer.status = None;

    if event::is_ctrl_c(&key) {
        return Ok(Action::Exit);
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return Ok(Action::Exit),
        KeyCode::Up | KeyCode::Char('k') => explorer.move_up(),
        KeyCode::Down | KeyCode::Char('j') => explorer.move_down(),

        KeyCode::Right => {
            let idx = explorer.selected;
            let expandable = explorer
                .rows
                .get(idx)
                .map(|n| n.entry.is_dir && !n.expanded)
                .unwrap_or(false);
            if expandable {
                if let Err(e) = explorer.expand(idx) {
                    explorer.status = Some(e.to_string());
                }
            }
        }

        KeyCode::Left => {
            let idx = explorer.selected;
            let dir_expanded = explorer.rows.get(idx).map(|n| (n.entry.is_dir, n.expanded));
            match dir_expanded {
                // Expanded dir: collapse it in place.
                Some((true, true)) => explorer.collapse(idx),
                // Collapsed dir or a file: step out to the parent.
                Some(_) => explorer.jump_to_parent(idx),
                None => {}
            }
        }

        KeyCode::Enter | KeyCode::Char('o') => {
            let idx = explorer.selected;
            let node = explorer
                .rows
                .get(idx)
                .map(|n| (n.entry.is_dir, n.expanded, n.entry.path.clone()));
            if let Some((is_dir, expanded, path)) = node {
                if is_dir {
                    if expanded {
                        explorer.collapse(idx);
                    } else if let Err(e) = explorer.expand(idx) {
                        explorer.status = Some(e.to_string());
                    }
                } else {
                    return Ok(Action::OpenFile(path));
                }
            }
        }

        KeyCode::Char('g') | KeyCode::Backspace => explorer.selected = 0,

        _ => {}
    }

    Ok(Action::Continue)
}

/// First `max_lines` of a text file, or a marker for a binary or unreadable one.
///
/// Pure and terminal-free — the preview pane calls it fresh each frame off the
/// selected path, so returning from `$EDITOR` shows the new contents with no
/// extra bookkeeping.
fn file_preview(path: &Path, max_lines: usize) -> String {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return format!("‹cannot read: {e}›"),
    };
    if bytes.contains(&0) {
        return format!("‹binary file, {} KB›", bytes.len() / 1024);
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.lines().take(max_lines).collect();
    if text.lines().count() > max_lines {
        lines.push("… (truncated)");
    }
    lines.join("\n")
}

/// Render the whole view: tree | preview, then status and legend.
fn render(frame: &mut Frame, explorer: &Explorer) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // panes
            Constraint::Length(1), // status
            Constraint::Length(1), // legend
        ])
        .split(frame.area());

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(chunks[0]);

    render_tree_pane(frame, panes[0], explorer);
    render_preview_pane(frame, panes[1], explorer);

    if let Some(ref msg) = explorer.status {
        frame.render_widget(Paragraph::new(msg.as_str()).style(theme::ERROR), chunks[1]);
    }
    render_legend(frame, chunks[2]);
}

/// Render the left tree pane with indent + expand/leaf glyphs.
fn render_tree_pane(frame: &mut Frame, area: Rect, explorer: &Explorer) {
    let title = format!(
        " {} ",
        explorer
            .root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "artifacts".to_string())
    );

    let items: Vec<ListItem> = explorer
        .rows
        .iter()
        .map(|node| {
            let indent = "  ".repeat(node.depth);
            let glyph = if node.entry.is_dir {
                if node.expanded {
                    "▾ "
                } else {
                    "▸ "
                }
            } else if node.entry.is_symlink {
                "↳ "
            } else {
                "  "
            };
            let suffix = if node.entry.is_dir { "/" } else { "" };
            ListItem::new(format!("{indent}{glyph}{}{suffix}", node.entry.name))
        })
        .collect();

    let mut state = ListState::default();
    if !explorer.rows.is_empty() {
        state.select(Some(explorer.selected));
    }

    let list = List::new(items)
        .block(Block::default().borders(Borders::RIGHT).title(title))
        .highlight_style(theme::SELECTED);
    frame.render_stateful_widget(list, area, &mut state);
}

/// Render the right preview pane for the selected entry.
fn render_preview_pane(frame: &mut Frame, area: Rect, explorer: &Explorer) {
    let text = match explorer.selected_node() {
        Some(node) if !node.entry.is_dir => {
            file_preview(&node.entry.path, area.height.max(1) as usize)
        }
        Some(node) => format!("‹{}/ — Enter to expand›", node.entry.name),
        None => "‹empty›".to_string(),
    };
    frame.render_widget(Paragraph::new(text), area);
}

/// Render the bottom key legend.
fn render_legend(frame: &mut Frame, area: Rect) {
    let pairs: &[(&str, &str)] = &[
        (" ↑↓/jk", " navigate  "),
        ("→", " expand  "),
        ("←", " collapse/up  "),
        ("Enter/o", " open  "),
        ("g", " top  "),
        ("q", " quit"),
    ];
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
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// A temp artifacts tree:
    ///   other/            (empty dir)
    ///   proj/
    ///     plans/
    ///       a.md
    ///     note.md
    fn temp_tree() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join("proj/plans")).unwrap();
        std::fs::create_dir_all(root.join("other")).unwrap();
        std::fs::write(root.join("proj/plans/a.md"), "hi").unwrap();
        std::fs::write(root.join("proj/note.md"), "hi").unwrap();
        (tmp, root)
    }

    fn names(explorer: &Explorer) -> Vec<(String, usize)> {
        explorer
            .rows
            .iter()
            .map(|n| (n.entry.name.clone(), n.depth))
            .collect()
    }

    #[test]
    fn new_lists_top_level_dirs_first() {
        let (_tmp, root) = temp_tree();
        let ex = Explorer::new(root).unwrap();
        assert_eq!(names(&ex), vec![("other".into(), 0), ("proj".into(), 0)]);
    }

    #[test]
    fn expand_splices_children_and_collapse_removes_nested() {
        let (_tmp, root) = temp_tree();
        let mut ex = Explorer::new(root).unwrap();

        // Expand `proj` (idx 1): plans/ (dir) then note.md (file, reverse order).
        ex.expand(1).unwrap();
        assert_eq!(
            names(&ex),
            vec![
                ("other".into(), 0),
                ("proj".into(), 0),
                ("plans".into(), 1),
                ("note.md".into(), 1),
            ]
        );

        // Expand `plans` (idx 2) → a.md at depth 2.
        ex.expand(2).unwrap();
        assert_eq!(
            names(&ex),
            vec![
                ("other".into(), 0),
                ("proj".into(), 0),
                ("plans".into(), 1),
                ("a.md".into(), 2),
                ("note.md".into(), 1),
            ]
        );

        // Collapsing `proj` drops the whole nested block, grandchild included.
        ex.collapse(1);
        assert_eq!(names(&ex), vec![("other".into(), 0), ("proj".into(), 0)]);
        assert!(!ex.rows[1].expanded);
    }

    #[test]
    fn right_expands_and_enter_toggles_a_dir() {
        let (_tmp, root) = temp_tree();
        let mut ex = Explorer::new(root).unwrap();
        ex.selected = 1; // proj

        handle_key(key(KeyCode::Right), &mut ex).unwrap();
        assert!(ex.rows[1].expanded);
        assert_eq!(ex.rows.len(), 4);

        // Enter on the expanded dir collapses it.
        handle_key(key(KeyCode::Enter), &mut ex).unwrap();
        assert!(!ex.rows[1].expanded);
        assert_eq!(ex.rows.len(), 2);
    }

    #[test]
    fn enter_on_a_file_asks_the_loop_to_open_it() {
        let (_tmp, root) = temp_tree();
        let mut ex = Explorer::new(root).unwrap();
        ex.expand(1).unwrap(); // reveal note.md at idx 3
        ex.selected = 3;

        let action = handle_key(key(KeyCode::Enter), &mut ex).unwrap();
        assert_eq!(action, Action::OpenFile(root_join(&ex, "proj/note.md")));
    }

    /// Absolute path of `rel` under the explorer root, for asserting OpenFile.
    fn root_join(ex: &Explorer, rel: &str) -> PathBuf {
        ex.root.join(rel)
    }

    #[test]
    fn left_on_a_file_jumps_to_its_parent_dir() {
        let (_tmp, root) = temp_tree();
        let mut ex = Explorer::new(root).unwrap();
        ex.expand(1).unwrap(); // proj → plans, note.md
        ex.expand(2).unwrap(); // plans → a.md (idx 3, depth 2)
        ex.selected = 3;

        handle_key(key(KeyCode::Left), &mut ex).unwrap();
        assert_eq!(ex.selected, 2); // parent `plans`
    }

    #[test]
    fn focus_project_expands_and_selects_it() {
        let (_tmp, root) = temp_tree();
        let mut ex = Explorer::new(root).unwrap();
        ex.focus_project("proj");
        assert_eq!(ex.selected, 1);
        assert!(ex.rows[1].expanded);
    }

    #[test]
    fn q_and_ctrl_c_exit() {
        let (_tmp, root) = temp_tree();
        let mut ex = Explorer::new(root).unwrap();
        assert_eq!(
            handle_key(key(KeyCode::Char('q')), &mut ex).unwrap(),
            Action::Exit
        );
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(handle_key(ctrl_c, &mut ex).unwrap(), Action::Exit);
    }

    #[test]
    fn file_preview_returns_short_text_verbatim() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.md");
        std::fs::write(&f, "line one\nline two").unwrap();
        assert_eq!(file_preview(&f, 10), "line one\nline two");
    }

    #[test]
    fn file_preview_truncates_over_the_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.md");
        std::fs::write(&f, "1\n2\n3\n4\n5").unwrap();
        let out = file_preview(&f, 2);
        assert_eq!(out, "1\n2\n… (truncated)");
    }

    #[test]
    fn file_preview_flags_a_binary_file() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("blob.bin");
        std::fs::write(&f, [0u8, 1, 2, 3]).unwrap();
        assert!(file_preview(&f, 10).starts_with("‹binary file,"));
    }
}
