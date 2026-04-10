//! Checklist widget displaying items with pass/fail/pending indicators.
//!
//! Used by wizard steps that verify prerequisites or run health checks.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::tui::theme;

/// Status of a single checklist item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    /// Item passed verification.
    Pass,
    /// Item failed verification.
    Fail,
    /// Item has not been checked yet.
    Pending,
    /// Item is currently being checked.
    Checking,
}

/// A single item in a checklist.
#[derive(Debug, Clone)]
pub struct CheckItem {
    /// Display label.
    pub label: String,
    /// Verification result.
    pub status: CheckStatus,
    /// Optional detail text (e.g., version info).
    pub detail: Option<String>,
}

/// Renders a checklist of items with status indicators.
pub struct Checklist<'a> {
    items: &'a [CheckItem],
    title: &'a str,
}

impl<'a> Checklist<'a> {
    #[must_use]
    pub fn new(items: &'a [CheckItem], title: &'a str) -> Self {
        Self { items, title }
    }

    fn item_line(item: &CheckItem) -> Line<'static> {
        let (symbol, style) = match item.status {
            CheckStatus::Pass => (theme::CHECKMARK, Style::new().fg(theme::GREEN_SUCCESS)),
            CheckStatus::Fail => (theme::CROSS, Style::new().fg(theme::RED_FAILURE)),
            CheckStatus::Pending => (theme::CIRCLE, Style::new().fg(theme::DIM_PENDING)),
            CheckStatus::Checking => (theme::BULLET, Style::new().fg(theme::YELLOW_RUNNING)),
        };

        let mut spans = vec![
            Span::styled(format!("  {symbol} "), style),
            Span::styled(item.label.clone(), style),
        ];

        if let Some(ref detail) = item.detail {
            spans.push(Span::styled(format!("  ({detail})"), theme::dim()));
        }

        Line::from(spans)
    }
}

impl Widget for Checklist<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines: Vec<Line<'static>> = self
            .items
            .iter()
            .map(|item| Self::item_line(item))
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

    #[test]
    fn test_pass_item_has_checkmark() {
        let item = CheckItem {
            label: "git".to_string(),
            status: CheckStatus::Pass,
            detail: Some("2.42.0".to_string()),
        };
        let line = Checklist::item_line(&item);
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains(theme::CHECKMARK));
        assert!(content.contains("git"));
        assert!(content.contains("2.42.0"));
    }

    #[test]
    fn test_fail_item_has_cross() {
        let item = CheckItem {
            label: "claude".to_string(),
            status: CheckStatus::Fail,
            detail: None,
        };
        let line = Checklist::item_line(&item);
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains(theme::CROSS));
        assert!(content.contains("claude"));
    }

    #[test]
    fn test_pending_item_has_circle() {
        let item = CheckItem {
            label: "node".to_string(),
            status: CheckStatus::Pending,
            detail: None,
        };
        let line = Checklist::item_line(&item);
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains(theme::CIRCLE));
    }

    #[test]
    fn test_checking_item_has_bullet() {
        let item = CheckItem {
            label: "checking".to_string(),
            status: CheckStatus::Checking,
            detail: None,
        };
        let line = Checklist::item_line(&item);
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains(theme::BULLET));
    }

    #[test]
    fn test_checklist_renders_without_panic() {
        let items = vec![
            CheckItem {
                label: "git".to_string(),
                status: CheckStatus::Pass,
                detail: Some("2.42".to_string()),
            },
            CheckItem {
                label: "claude".to_string(),
                status: CheckStatus::Fail,
                detail: None,
            },
        ];
        let widget = Checklist::new(&items, "Prerequisites");
        let area = Rect::new(0, 0, 40, 8);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
    }

    #[test]
    fn test_checklist_empty() {
        let items: Vec<CheckItem> = vec![];
        let widget = Checklist::new(&items, "Empty");
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
    }

    #[test]
    fn test_check_status_equality() {
        assert_eq!(CheckStatus::Pass, CheckStatus::Pass);
        assert_ne!(CheckStatus::Pass, CheckStatus::Fail);
        assert_ne!(CheckStatus::Checking, CheckStatus::Pending);
    }
}
