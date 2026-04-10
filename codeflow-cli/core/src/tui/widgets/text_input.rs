//! Text input field widget for wizard forms.
//!
//! Renders a labeled input field with cursor position and placeholder text.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::tui::theme;

/// A single-line text input widget.
pub struct TextInput<'a> {
    /// Current input value.
    value: &'a str,
    /// Placeholder shown when value is empty.
    placeholder: &'a str,
    /// Label shown above the input.
    label: &'a str,
    /// Cursor position (byte offset in value).
    cursor: usize,
    /// Whether this input is currently focused.
    focused: bool,
}

impl<'a> TextInput<'a> {
    #[must_use]
    pub fn new(value: &'a str, label: &'a str) -> Self {
        Self {
            value,
            placeholder: "",
            label,
            cursor: value.len(),
            focused: false,
        }
    }

    #[must_use]
    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    #[must_use]
    pub fn cursor(mut self, cursor: usize) -> Self {
        self.cursor = cursor;
        self
    }

    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
}

impl Widget for TextInput<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_style = if self.focused {
            Style::new().fg(theme::BLUE_ACCENT)
        } else {
            Style::default()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme::BORDER_TYPE)
            .border_style(border_style)
            .title(Span::styled(format!(" {} ", self.label), theme::header()));

        let line = if self.value.is_empty() {
            Line::from(Span::styled(self.placeholder, theme::dim()))
        } else if self.focused {
            // Show cursor as a highlighted character.
            let (before, after) = if self.cursor <= self.value.len() {
                (&self.value[..self.cursor], &self.value[self.cursor..])
            } else {
                (self.value, "")
            };
            let cursor_char = after.chars().next().map_or(' ', |c| c);
            let rest = if after.len() > cursor_char.len_utf8() {
                &after[cursor_char.len_utf8()..]
            } else {
                ""
            };
            Line::from(vec![
                Span::raw(before.to_string()),
                Span::styled(
                    cursor_char.to_string(),
                    Style::new()
                        .fg(theme::WHITE_TEXT)
                        .add_modifier(Modifier::REVERSED),
                ),
                Span::raw(rest.to_string()),
            ])
        } else {
            Line::from(Span::raw(self.value))
        };

        let paragraph = Paragraph::new(line).block(block);
        paragraph.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    #[test]
    fn test_text_input_empty_shows_placeholder() {
        let widget = TextInput::new("", "Name").placeholder("Enter name...");
        let area = Rect::new(0, 0, 30, 3);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        // Should not panic.
    }

    #[test]
    fn test_text_input_with_value() {
        let widget = TextInput::new("hello", "Name");
        let area = Rect::new(0, 0, 30, 3);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
    }

    #[test]
    fn test_text_input_focused_cursor() {
        let widget = TextInput::new("hello", "Name").focused(true).cursor(3);
        let area = Rect::new(0, 0, 30, 3);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
    }

    #[test]
    fn test_text_input_cursor_at_end() {
        let widget = TextInput::new("abc", "Label").focused(true).cursor(3);
        let area = Rect::new(0, 0, 30, 3);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
    }

    #[test]
    fn test_text_input_cursor_beyond_length() {
        let widget = TextInput::new("ab", "Label").focused(true).cursor(99);
        let area = Rect::new(0, 0, 30, 3);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
    }

    #[test]
    fn test_text_input_unfocused_no_cursor() {
        let widget = TextInput::new("hello", "Label").focused(false);
        let area = Rect::new(0, 0, 30, 3);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
    }
}
