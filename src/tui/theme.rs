//! Color and style constants for the TUI.
//!
//! The ANSI colour scheme, as ratatui styles:
//! - skill type → `Color::Cyan`
//! - agent type → `Color::Blue`
//! - checkmark, [CORE] → `Color::Green`
//! - warnings, "?" → `Color::Yellow`
//! - errors → `Color::Red`
//! - section headers → `Modifier::BOLD`
//! - dimmed text → `Modifier::DIM`

use ratatui::style::{Color, Modifier, Style};

/// Style for skill type labels.
pub const SKILL_TYPE: Style = Style::new().fg(Color::Cyan);

/// Style for agent type labels.
pub const AGENT_TYPE: Style = Style::new().fg(Color::Blue);

/// Style for success indicators (✓, [CORE]).
pub const SUCCESS: Style = Style::new().fg(Color::Green);

/// Style for warnings (?).
pub const WARNING: Style = Style::new().fg(Color::Yellow);

/// Style for section headers.
pub const HEADER: Style = Style::new().add_modifier(Modifier::BOLD);

/// Style for dimmed text.
pub const DIM: Style = Style::new().add_modifier(Modifier::DIM);

/// Style for the selected row in a list.
pub const SELECTED: Style = Style::new()
    .fg(Color::Black)
    .bg(Color::Cyan)
    .add_modifier(Modifier::BOLD);

/// Style for the text cell the edit cursor sits on.
///
/// Reversed rather than a block glyph, so it reads correctly wherever it lands
/// in a wrapped field — including past the last character.
pub const CURSOR: Style = Style::new().fg(Color::Black).bg(Color::Cyan);

/// Style for the contents of a text field that does not have focus.
pub const FIELD_BLURRED: Style = Style::new().add_modifier(Modifier::DIM);

/// Style for the search/filter bar.
pub const SEARCH_BAR: Style = Style::new().fg(Color::White).bg(Color::DarkGray);

/// Style for the help bar at the bottom.
pub const HELP_BAR: Style = Style::new().fg(Color::DarkGray);

/// Style for the core flag indicator in rows.
pub const CORE_BADGE: Style = Style::new().fg(Color::Green).add_modifier(Modifier::BOLD);

/// Return the type style for a given spec type string.
pub fn type_style(spec_type: &crate::library::spec::SpecType) -> Style {
    match spec_type {
        crate::library::spec::SpecType::Skill => SKILL_TYPE,
        crate::library::spec::SpecType::Agent => AGENT_TYPE,
    }
}
