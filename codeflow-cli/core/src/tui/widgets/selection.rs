//! Selection list widget for wizard choice screens.
//!
//! Renders a vertical list of options with a highlight indicator on the selected item.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::tui::theme;

/// A single selectable option.
#[derive(Debug, Clone)]
pub struct SelectOption {
    /// Display label.
    pub label: String,
    /// Optional description shown below the label.
    pub description: Option<String>,
}

/// Renders a selection list with one highlighted item.
pub struct SelectionList<'a> {
    options: &'a [SelectOption],
    selected: usize,
    title: &'a str,
}

impl<'a> SelectionList<'a> {
    #[must_use]
    pub fn new(options: &'a [SelectOption], selected: usize, title: &'a str) -> Self {
        Self {
            options,
            selected,
            title,
        }
    }

    fn option_lines(index: usize, option: &SelectOption, is_selected: bool) -> Vec<Line<'static>> {
        let pointer = if is_selected { theme::TRIANGLE } else { ' ' };

        let label_style = if is_selected {
            theme::header()
        } else {
            Style::new().fg(theme::WHITE_TEXT)
        };

        let pointer_style = if is_selected {
            Style::new().fg(theme::BLUE_ACCENT)
        } else {
            Style::default()
        };

        let mut lines = vec![Line::from(vec![
            Span::styled(format!("  {pointer} "), pointer_style),
            Span::styled(format!("{}. {}", index + 1, option.label), label_style),
        ])];

        if let Some(ref desc) = option.description {
            lines.push(Line::from(vec![
                Span::raw("      "),
                Span::styled(desc.clone(), theme::dim()),
            ]));
        }

        lines
    }
}

impl Widget for SelectionList<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines: Vec<Line<'static>> = self
            .options
            .iter()
            .enumerate()
            .flat_map(|(i, opt)| Self::option_lines(i, opt, i == self.selected))
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme::BORDER_TYPE)
            .title(Span::styled(format!(" {} ", self.title), theme::header()));

        let paragraph = Paragraph::new(lines).block(block);
        paragraph.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn make_options() -> Vec<SelectOption> {
        vec![
            SelectOption {
                label: "New project".to_string(),
                description: Some("Create a new CodeFlow project".to_string()),
            },
            SelectOption {
                label: "Existing project".to_string(),
                description: Some("Initialize CodeFlow in existing directory".to_string()),
            },
            SelectOption {
                label: "Join project".to_string(),
                description: None,
            },
        ]
    }

    #[test]
    fn test_selected_item_has_triangle() {
        let options = make_options();
        let lines = SelectionList::option_lines(0, &options[0], true);
        let content: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains(theme::TRIANGLE));
        assert!(content.contains("1. New project"));
    }

    #[test]
    fn test_unselected_item_has_space() {
        let options = make_options();
        let lines = SelectionList::option_lines(1, &options[1], false);
        let content: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(!content.contains(theme::TRIANGLE));
        assert!(content.contains("2. Existing project"));
    }

    #[test]
    fn test_option_with_description_has_two_lines() {
        let options = make_options();
        let lines = SelectionList::option_lines(0, &options[0], true);
        assert_eq!(
            lines.len(),
            2,
            "option with description should have 2 lines"
        );
    }

    #[test]
    fn test_option_without_description_has_one_line() {
        let options = make_options();
        let lines = SelectionList::option_lines(2, &options[2], false);
        assert_eq!(
            lines.len(),
            1,
            "option without description should have 1 line"
        );
    }

    #[test]
    fn test_selection_list_renders_without_panic() {
        let options = make_options();
        let widget = SelectionList::new(&options, 1, "Choose");
        let area = Rect::new(0, 0, 50, 12);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
    }

    #[test]
    fn test_selection_list_empty() {
        let options: Vec<SelectOption> = vec![];
        let widget = SelectionList::new(&options, 0, "Empty");
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
    }

    #[test]
    fn test_selection_list_selected_beyond_bounds() {
        let options = make_options();
        // selected=99 is beyond bounds, should not panic.
        let widget = SelectionList::new(&options, 99, "Test");
        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
    }
}
