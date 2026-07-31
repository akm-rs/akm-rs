//! A single-line text field with a real cursor, rendered wrapped.
//!
//! The previous editor appended to a `String` and popped from the end, drawn
//! into a two-line box with no wrapping. A description longer than the popup
//! ran off the edge and took the cursor block with it, which made the field
//! look broken even though the text was technically going in. This widget
//! fixes both halves: the cursor can move, and the text wraps.
//!
//! Everything here is measured in *characters*, not bytes, so multi-byte input
//! cannot split a character or push the cursor into the middle of one.

/// An editable line of text plus a cursor position.
#[derive(Debug, Clone, Default)]
pub struct TextInput {
    chars: Vec<char>,
    /// Cursor position, in characters, in `0..=chars.len()`.
    cursor: usize,
}

impl TextInput {
    /// Create an input holding `value`, with the cursor at the end — where
    /// someone who opened a field to extend it expects to start typing.
    pub fn new(value: &str) -> Self {
        let chars: Vec<char> = value.chars().collect();
        let cursor = chars.len();
        Self { chars, cursor }
    }

    /// The current text.
    pub fn value(&self) -> String {
        self.chars.iter().collect()
    }

    /// Cursor position in characters.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Number of characters held.
    pub fn len(&self) -> usize {
        self.chars.len()
    }

    /// Whether the field is empty.
    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// Insert a character at the cursor and step over it.
    pub fn insert(&mut self, c: char) {
        self.chars.insert(self.cursor, c);
        self.cursor += 1;
    }

    /// Delete the character before the cursor.
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }

    /// Delete the character under the cursor.
    pub fn delete(&mut self) {
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
        }
    }

    /// Move the cursor one character left.
    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Move the cursor one character right.
    pub fn right(&mut self) {
        if self.cursor < self.chars.len() {
            self.cursor += 1;
        }
    }

    /// Move the cursor to the start of the text.
    pub fn home(&mut self) {
        self.cursor = 0;
    }

    /// Move the cursor to the end of the text.
    pub fn end(&mut self) {
        self.cursor = self.chars.len();
    }

    /// Lay the text out into lines of at most `width` characters.
    ///
    /// Returns the lines together with the cursor's (row, column) within them.
    /// Wrapping happens on word boundaries where one exists inside the width,
    /// and mid-word otherwise, so a long unbroken token still shows rather
    /// than vanishing off the right edge.
    pub fn wrapped(&self, width: usize) -> (Vec<String>, (usize, usize)) {
        let width = width.max(1);

        let mut lines: Vec<String> = Vec::new();
        let mut cursor_pos = (0usize, 0usize);
        let mut start = 0usize;

        while start < self.chars.len() {
            let remaining = self.chars.len() - start;
            let end = if remaining <= width {
                self.chars.len()
            } else {
                // Break after the last space that fits; if there is none, the
                // word is longer than the field and has to be split.
                match self.chars[start..start + width]
                    .iter()
                    .rposition(|c| *c == ' ')
                {
                    Some(offset) => start + offset + 1,
                    None => start + width,
                }
            };

            if (start..end).contains(&self.cursor) {
                cursor_pos = (lines.len(), self.cursor - start);
            }
            lines.push(self.chars[start..end].iter().collect());
            start = end;
        }

        // The cursor sits past the last character when appending, which is one
        // column beyond the final line — or the start of a new one when that
        // line is already full.
        if self.cursor >= start {
            let last = lines.len().saturating_sub(1);
            let column = self.cursor - start;
            if lines.is_empty() {
                cursor_pos = (0, 0);
            } else if column == 0 && lines[last].chars().count() >= width {
                lines.push(String::new());
                cursor_pos = (lines.len() - 1, 0);
            } else {
                cursor_pos = (last, lines[last].chars().count());
            }
        }

        if lines.is_empty() {
            lines.push(String::new());
        }

        (lines, cursor_pos)
    }

    /// The `height` lines to draw, and the cursor within them, scrolled so the
    /// cursor is always on screen.
    pub fn visible(&self, width: usize, height: usize) -> (Vec<String>, (usize, usize)) {
        let height = height.max(1);
        let (lines, (row, column)) = self.wrapped(width);

        let first = row.saturating_sub(height - 1);
        let window = lines
            .into_iter()
            .skip(first)
            .take(height)
            .collect::<Vec<_>>();

        (window, (row - first, column))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_inserts_at_the_cursor_rather_than_the_end() {
        let mut input = TextInput::new("helo");
        input.left();
        input.insert('l');

        assert_eq!(input.value(), "hello");
        assert_eq!(input.cursor(), 4);
    }

    #[test]
    fn backspace_and_delete_act_on_opposite_sides_of_the_cursor() {
        let mut input = TextInput::new("abcd");
        input.home();
        input.right();
        input.right();

        input.backspace();
        assert_eq!(input.value(), "acd");

        input.delete();
        assert_eq!(input.value(), "ad");
    }

    #[test]
    fn the_cursor_cannot_leave_the_text() {
        let mut input = TextInput::new("ab");
        input.home();
        input.left();
        assert_eq!(input.cursor(), 0);
        input.backspace();
        assert_eq!(input.value(), "ab");

        input.end();
        input.right();
        assert_eq!(input.cursor(), 2);
        input.delete();
        assert_eq!(input.value(), "ab");
    }

    /// Editing must never split a multi-byte character.
    #[test]
    fn multi_byte_characters_are_edited_whole() {
        let mut input = TextInput::new("héllo — ok");
        input.home();
        input.right();
        input.delete();

        assert_eq!(input.value(), "hllo — ok");

        input.end();
        input.backspace();
        assert_eq!(input.value(), "hllo — o");
    }

    #[test]
    fn wrapping_breaks_on_word_boundaries() {
        let input = TextInput::new("use when writing tests");
        let (lines, _) = input.wrapped(10);

        assert_eq!(lines, vec!["use when ", "writing ", "tests"]);
    }

    /// A word longer than the field is split rather than dropped off the edge.
    #[test]
    fn wrapping_splits_a_word_that_cannot_fit() {
        let input = TextInput::new("supercalifragilistic");
        let (lines, _) = input.wrapped(8);

        assert_eq!(lines, vec!["supercal", "ifragili", "stic"]);
    }

    #[test]
    fn the_cursor_is_located_on_the_wrapped_line_that_holds_it() {
        let mut input = TextInput::new("use when writing tests");
        input.home();

        let (_, position) = input.wrapped(10);
        assert_eq!(position, (0, 0));

        for _ in 0..12 {
            input.right();
        }
        let (_, position) = input.wrapped(10);
        assert_eq!(position, (1, 3));
    }

    /// Appending at the end is the common case, and the reason the old field
    /// looked broken: the cursor has to stay visible past the last character.
    #[test]
    fn the_cursor_sits_just_past_the_last_character_when_appending() {
        let input = TextInput::new("use when");
        let (lines, position) = input.wrapped(10);

        assert_eq!(lines, vec!["use when"]);
        assert_eq!(position, (0, 8));
    }

    /// When the last line is exactly full, the cursor moves onto a fresh line
    /// instead of drawing one column past the edge.
    #[test]
    fn a_full_last_line_pushes_the_cursor_onto_the_next_one() {
        let input = TextInput::new("abcdefghij");
        let (lines, position) = input.wrapped(10);

        assert_eq!(lines, vec!["abcdefghij", ""]);
        assert_eq!(position, (1, 0));
    }

    #[test]
    fn an_empty_field_still_renders_one_line() {
        let input = TextInput::new("");
        let (lines, position) = input.wrapped(10);

        assert_eq!(lines, vec![""]);
        assert_eq!(position, (0, 0));
        assert!(input.is_empty());
        assert_eq!(input.len(), 0);
    }

    /// Text longer than the box scrolls so the cursor stays on screen.
    #[test]
    fn the_view_scrolls_to_keep_the_cursor_visible() {
        let input = TextInput::new("one two three four five six seven");

        let (window, (row, _)) = input.visible(10, 2);

        assert_eq!(window.len(), 2);
        assert_eq!(row, 1, "the cursor is on the last visible line");
        assert!(window.last().unwrap().contains("seven"));
    }

    #[test]
    fn the_view_does_not_scroll_when_everything_fits() {
        let input = TextInput::new("short");
        let (window, position) = input.visible(20, 3);

        assert_eq!(window, vec!["short"]);
        assert_eq!(position, (0, 5));
    }
}
