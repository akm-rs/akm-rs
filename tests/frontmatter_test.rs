//! Frontmatter parser tests.
//!
//! Tests the YAML frontmatter extraction from markdown content.
//! These are pure unit tests — no filesystem access needed.

use akm::library::frontmatter::Frontmatter;

#[test]
fn parse_standard_frontmatter() {
    let content = "---\nname: Test Skill\ndescription: Use when testing\n---\n# Content";
    let fm = Frontmatter::parse(content).unwrap();
    assert_eq!(fm.name.as_deref(), Some("Test Skill"));
    assert_eq!(fm.description.as_deref(), Some("Use when testing"));
}

#[test]
fn parse_quoted_values() {
    let content = "---\nname: \"Quoted Name\"\ndescription: 'Single quoted'\n---\n";
    let fm = Frontmatter::parse(content).unwrap();
    assert_eq!(fm.name.as_deref(), Some("Quoted Name"));
    assert_eq!(fm.description.as_deref(), Some("Single quoted"));
}

#[test]
fn parse_multiline_description() {
    let content = "---\nname: Editor\ndescription: |\n  Professional editing and proofreading.\n  Use when: editing text.\n---\n";
    let fm = Frontmatter::parse(content).unwrap();
    assert_eq!(fm.name.as_deref(), Some("Editor"));
    assert!(fm
        .description
        .as_deref()
        .unwrap()
        .contains("Professional editing"));
    assert!(fm.description.as_deref().unwrap().contains("Use when"));
}

#[test]
fn parse_folded_scalar_description() {
    let content =
        "---\nname: Editor\ndescription: >\n  Professional editing\n  and proofreading.\n---\n";
    let fm = Frontmatter::parse(content).unwrap();
    assert_eq!(fm.name.as_deref(), Some("Editor"));
    assert!(fm
        .description
        .as_deref()
        .unwrap()
        .contains("Professional editing"));
}

#[test]
fn parse_no_frontmatter() {
    let content = "# Just a markdown file\n\nNo frontmatter here.";
    let fm = Frontmatter::parse(content).unwrap();
    assert_eq!(fm.name, None);
    assert_eq!(fm.description, None);
}

#[test]
fn parse_empty_frontmatter() {
    let content = "---\n---\n# Content";
    let fm = Frontmatter::parse(content).unwrap();
    assert_eq!(fm.name, None);
    assert_eq!(fm.description, None);
}

#[test]
fn parse_windows_line_endings() {
    let content = "---\r\nname: Windows Skill\r\ndescription: Handles CRLF\r\n---\r\n# Content";
    let fm = Frontmatter::parse(content).unwrap();
    assert_eq!(fm.name.as_deref(), Some("Windows Skill"));
    assert_eq!(fm.description.as_deref(), Some("Handles CRLF"));
}

#[test]
fn parse_extra_fields_ignored() {
    let content = "---\nname: Test\ndescription: Desc\nlicense: MIT\nmetadata:\n  author: test\n  version: 1.0\n---\n";
    let fm = Frontmatter::parse(content).unwrap();
    assert_eq!(fm.name.as_deref(), Some("Test"));
    assert_eq!(fm.description.as_deref(), Some("Desc"));
}

#[test]
fn parse_name_fallback_not_applied_at_parse() {
    let content = "---\ndescription: Only description\n---\n";
    let fm = Frontmatter::parse(content).unwrap();
    assert_eq!(fm.name, None);
    assert_eq!(fm.description.as_deref(), Some("Only description"));
}

#[test]
fn parse_unclosed_frontmatter_returns_default() {
    let content = "---\nname: Missing end marker\n# Content";
    let fm = Frontmatter::parse(content).unwrap();
    assert_eq!(fm.name, None);
}

#[test]
fn parse_description_with_colons() {
    let content = "---\nname: Skill\ndescription: Use when: something happens\n---\n";
    let fm = Frontmatter::parse(content).unwrap();
    assert_eq!(
        fm.description.as_deref(),
        Some("Use when: something happens")
    );
}

#[test]
fn require_name_and_description_both_present() {
    let fm = Frontmatter {
        name: Some("Test".into()),
        description: Some("Desc".into()),
    };
    assert!(fm
        .require_name_and_description(std::path::Path::new("test.md"))
        .is_ok());
}

#[test]
fn require_name_and_description_missing_name() {
    let fm = Frontmatter {
        name: None,
        description: Some("Desc".into()),
    };
    assert!(fm
        .require_name_and_description(std::path::Path::new("test.md"))
        .is_err());
}

#[test]
fn parse_single_quote_char_does_not_panic() {
    let content = "---\nname: \"\n---\n";
    let fm = Frontmatter::parse(content).unwrap();
    // Single quote char is not stripped — returned as-is
    assert_eq!(fm.name.as_deref(), Some("\""));
}
