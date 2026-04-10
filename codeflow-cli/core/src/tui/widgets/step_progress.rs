//! Step progress sidebar widget for multi-step wizards.
//!
//! Displays a vertical list of steps with status indicators:
//! - Checkmark (complete)
//! - Circle-dot / bullet (current)
//! - Circle (pending)

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::tui::theme;

/// Status of a single wizard step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    /// Step completed successfully.
    Complete,
    /// Step is currently active.
    Current,
    /// Step has not started yet.
    Pending,
    /// Step was skipped.
    Skipped,
}

/// A single step in the wizard progress sidebar.
#[derive(Debug, Clone)]
pub struct StepEntry {
    /// Display label for the step.
    pub label: String,
    /// Current status of this step.
    pub status: StepStatus,
}

/// Renders a vertical step progress list.
pub struct StepProgress<'a> {
    steps: &'a [StepEntry],
    title: &'a str,
}

impl<'a> StepProgress<'a> {
    #[must_use]
    pub fn new(steps: &'a [StepEntry], title: &'a str) -> Self {
        Self { steps, title }
    }

    fn step_line(index: usize, entry: &StepEntry) -> Line<'static> {
        let (symbol, style) = match entry.status {
            StepStatus::Complete => (theme::CHECKMARK, Style::new().fg(theme::GREEN_SUCCESS)),
            StepStatus::Current => (theme::BULLET, Style::new().fg(theme::YELLOW_RUNNING)),
            StepStatus::Pending => (theme::CIRCLE, Style::new().fg(theme::DIM_PENDING)),
            StepStatus::Skipped => (theme::CROSS, Style::new().fg(theme::DIM_PENDING)),
        };

        let num = index + 1;
        let label_style = match entry.status {
            StepStatus::Current => theme::header(),
            StepStatus::Complete => Style::new().fg(theme::GREEN_SUCCESS),
            StepStatus::Pending | StepStatus::Skipped => theme::dim(),
        };

        Line::from(vec![
            Span::styled(format!(" {symbol} "), style),
            Span::styled(format!("{num}. {}", entry.label), label_style),
        ])
    }
}

impl Widget for StepProgress<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines: Vec<Line<'static>> = self
            .steps
            .iter()
            .enumerate()
            .map(|(i, entry)| Self::step_line(i, entry))
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

    fn make_steps() -> Vec<StepEntry> {
        vec![
            StepEntry {
                label: "Location".to_string(),
                status: StepStatus::Complete,
            },
            StepEntry {
                label: "Prerequisites".to_string(),
                status: StepStatus::Current,
            },
            StepEntry {
                label: "Auth".to_string(),
                status: StepStatus::Pending,
            },
        ]
    }

    #[test]
    fn test_step_line_complete() {
        let entry = StepEntry {
            label: "Done".to_string(),
            status: StepStatus::Complete,
        };
        let line = StepProgress::step_line(0, &entry);
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            content.contains(theme::CHECKMARK),
            "should contain checkmark"
        );
        assert!(content.contains("1. Done"));
    }

    #[test]
    fn test_step_line_current() {
        let entry = StepEntry {
            label: "Active".to_string(),
            status: StepStatus::Current,
        };
        let line = StepProgress::step_line(1, &entry);
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains(theme::BULLET), "should contain bullet");
        assert!(content.contains("2. Active"));
    }

    #[test]
    fn test_step_line_pending() {
        let entry = StepEntry {
            label: "Wait".to_string(),
            status: StepStatus::Pending,
        };
        let line = StepProgress::step_line(2, &entry);
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains(theme::CIRCLE), "should contain circle");
        assert!(content.contains("3. Wait"));
    }

    #[test]
    fn test_step_line_skipped() {
        let entry = StepEntry {
            label: "Skip".to_string(),
            status: StepStatus::Skipped,
        };
        let line = StepProgress::step_line(0, &entry);
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains(theme::CROSS), "should contain cross");
    }

    #[test]
    fn test_step_progress_renders_without_panic() {
        let steps = make_steps();
        let widget = StepProgress::new(&steps, "Steps");
        let area = Rect::new(0, 0, 30, 10);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        // Rendering should not panic.
    }

    #[test]
    fn test_step_progress_empty_steps() {
        let steps: Vec<StepEntry> = vec![];
        let widget = StepProgress::new(&steps, "Empty");
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
    }

    #[test]
    fn test_step_status_equality() {
        assert_eq!(StepStatus::Complete, StepStatus::Complete);
        assert_ne!(StepStatus::Complete, StepStatus::Pending);
        assert_ne!(StepStatus::Current, StepStatus::Skipped);
    }
}
