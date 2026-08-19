//! Color and style constants for the TUI.
//!
//! A black-and-gold scheme matching raphaelsimon.fr. Gold (`#eec35e`) is an
//! *accent*, kept sparse the way the site keeps it: it marks focus (the
//! selection bar and edit cursor), identity (the skill type, the `[CORE]`
//! badge) — the TUI's analogs of the site's links and logo. Section headers
//! stay neutral-bold, as the site's headings are white, not gold. Skills and
//! agents keep two distinguishable shades of the one family (gold vs a dimmed
//! bronze). Semantic status colours (success/warning/error) are preserved as
//! distinct signals, warmed to sit in the palette.
//!
//! Colours are truecolour (`Rgb`); a non-truecolour terminal degrades them to
//! the nearest 256-colour approximation.

use ratatui::style::{Color, Modifier, Style};

// --- Brand palette ---------------------------------------------------------

/// Primary brand accent — raphaelsimon.fr gold.
const GOLD: Color = Color::Rgb(0xee, 0xc3, 0x5e);
/// Dimmed gold — separates the second family member (agents) from skills.
const BRONZE: Color = Color::Rgb(0xb0, 0x85, 0x4a);
/// Near-black foreground for text sitting on a gold fill.
const INK: Color = Color::Rgb(0x1a, 0x17, 0x12);
/// Warm charcoal fill for the search bar.
const CHARCOAL: Color = Color::Rgb(0x2a, 0x26, 0x20);
/// Warm grey for dimmed / help text (replaces cold DarkGray).
const TAUPE: Color = Color::Rgb(0x8a, 0x81, 0x72);
/// Warm parchment for text on a dark fill.
const PARCHMENT: Color = Color::Rgb(0xed, 0xe6, 0xd6);
/// Semantic success — muted olive.
const OLIVE: Color = Color::Rgb(0x7a, 0x9a, 0x3e);
/// Semantic warning — amber.
const AMBER: Color = Color::Rgb(0xe8, 0x91, 0x2a);
/// Semantic error — warm red.
const WARM_RED: Color = Color::Rgb(0xd2, 0x41, 0x2e);

// --- Semantic styles -------------------------------------------------------

/// Style for skill type labels.
pub const SKILL_TYPE: Style = Style::new().fg(GOLD);

/// Style for agent type labels.
pub const AGENT_TYPE: Style = Style::new().fg(BRONZE);

/// Style for success indicators (✓, [CORE]).
pub const SUCCESS: Style = Style::new().fg(OLIVE);

/// Style for warnings (?).
pub const WARNING: Style = Style::new().fg(AMBER);

/// Style for errors and states that need a decision.
pub const ERROR: Style = Style::new().fg(WARM_RED);

/// Style for section headers.
pub const HEADER: Style = Style::new().add_modifier(Modifier::BOLD);

/// Style for dimmed text.
pub const DIM: Style = Style::new().add_modifier(Modifier::DIM);

/// Style for the selected row in a list — the signature gold bar.
pub const SELECTED: Style = Style::new().fg(INK).bg(GOLD).add_modifier(Modifier::BOLD);

/// Style for the text cell the edit cursor sits on.
///
/// Reversed rather than a block glyph, so it reads correctly wherever it lands
/// in a wrapped field — including past the last character. Shares the selection
/// bar's gold so focus reads the same everywhere.
pub const CURSOR: Style = Style::new().fg(INK).bg(GOLD);

/// Style for the contents of a text field that does not have focus.
pub const FIELD_BLURRED: Style = Style::new().add_modifier(Modifier::DIM);

/// Style for the search/filter bar.
pub const SEARCH_BAR: Style = Style::new().fg(PARCHMENT).bg(CHARCOAL);

/// Style for the help bar at the bottom.
pub const HELP_BAR: Style = Style::new().fg(TAUPE);

/// Style for the core flag indicator in rows.
pub const CORE_BADGE: Style = Style::new().fg(GOLD).add_modifier(Modifier::BOLD);

/// Return the style for a drift marker.
///
/// Only the states that need the user to do something are coloured; a spec the
/// remote merely moved ahead of is applied by the next fast-forward on its own.
pub fn drift_style(state: crate::library::drift::DriftState) -> Style {
    use crate::library::drift::DriftState;
    match state {
        DriftState::Clean => DIM,
        DriftState::LocalNewer => WARNING,
        DriftState::RemoteNewer => DIM,
        DriftState::Diverged => ERROR,
    }
}

/// Return the type style for a given spec type string.
pub fn type_style(spec_type: &crate::library::spec::SpecType) -> Style {
    match spec_type {
        crate::library::spec::SpecType::Skill => SKILL_TYPE,
        crate::library::spec::SpecType::Agent => AGENT_TYPE,
    }
}
