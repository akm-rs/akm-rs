//! Color and style constants for the TUI.
//!
//! Maps the Bash ANSI color scheme to ratatui styles:
//! - Bash `$CYAN` (type "skill") → `Color::Cyan`
//! - Bash `$BLUE` (type "agent") → `Color::Blue`
//! - Bash `$GREEN` (checkmark, [CORE]) → `Color::Green`
//! - Bash `$YELLOW` (warnings, "?") → `Color::Yellow`
//! - Bash `$RED` (errors) → `Color::Red`
//! - Bash `$BOLD` → `Modifier::BOLD`
//! - Bash `$DIM` → `Modifier::DIM`

use ratatui::style::{Color, Modifier, Style};

/// Style for skill type labels. Bash: `$CYAN`.
pub const SKILL_TYPE: Style = Style::new().fg(Color::Cyan);

/// Style for agent type labels. Bash: `$BLUE`.
pub const AGENT_TYPE: Style = Style::new().fg(Color::Blue);

/// Style for success indicators (✓, [CORE]). Bash: `$GREEN`.
pub const SUCCESS: Style = Style::new().fg(Color::Green);

/// Style for warnings (?). Bash: `$YELLOW`.
pub const WARNING: Style = Style::new().fg(Color::Yellow);

/// Style for section headers. Bash: `$BOLD`.
pub const HEADER: Style = Style::new().add_modifier(Modifier::BOLD);

/// Style for dimmed text. Bash: `$DIM`.
pub const DIM: Style = Style::new().add_modifier(Modifier::DIM);

/// Style for the selected row in a list.
pub const SELECTED: Style = Style::new()
    .fg(Color::Black)
    .bg(Color::Cyan)
    .add_modifier(Modifier::BOLD);

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
