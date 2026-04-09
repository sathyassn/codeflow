//! Status badge widget mapping `AutorunTaskRunStatus` to colored `Span`.

use ratatui::style::Style;
use ratatui::text::Span;

use crate::tui::theme;
use crate::types::AutorunTaskRunStatus;

/// Renders a task run status as a colored badge.
pub struct StatusBadge {
    status: AutorunTaskRunStatus,
    /// Optional display override (e.g., "Waiting" for pending in running session).
    display: Option<String>,
}

impl StatusBadge {
    #[must_use]
    pub fn new(status: AutorunTaskRunStatus) -> Self {
        Self {
            status,
            display: None,
        }
    }

    #[must_use]
    pub fn with_display(mut self, display: String) -> Self {
        self.display = Some(display);
        self
    }

    /// Convert to a styled `Span` for embedding in table cells.
    #[must_use]
    pub fn to_span(&self) -> Span<'static> {
        let (symbol, style) = match self.status {
            AutorunTaskRunStatus::Completed => {
                (theme::CHECKMARK, Style::new().fg(theme::GREEN_SUCCESS))
            }
            AutorunTaskRunStatus::Running => {
                (theme::BULLET, Style::new().fg(theme::YELLOW_RUNNING))
            }
            AutorunTaskRunStatus::Failed => (theme::CROSS, Style::new().fg(theme::RED_FAILURE)),
            AutorunTaskRunStatus::Timeout => (theme::CAUTION, Style::new().fg(theme::RED_FAILURE)),
            AutorunTaskRunStatus::Pending | AutorunTaskRunStatus::Skipped => {
                (theme::CIRCLE, Style::new().fg(theme::DIM_PENDING))
            }
            AutorunTaskRunStatus::Cancelled => (theme::CROSS, Style::new().fg(theme::DIM_PENDING)),
        };

        let label = self
            .display
            .as_deref()
            .unwrap_or_else(|| status_label(self.status));

        Span::styled(format!("{symbol} {label}"), style)
    }
}

fn status_label(status: AutorunTaskRunStatus) -> &'static str {
    match status {
        AutorunTaskRunStatus::Completed => "Done",
        AutorunTaskRunStatus::Running => "Running",
        AutorunTaskRunStatus::Failed => "Failed",
        AutorunTaskRunStatus::Timeout => "Timeout",
        AutorunTaskRunStatus::Pending => "Pending",
        AutorunTaskRunStatus::Skipped => "Skipped",
        AutorunTaskRunStatus::Cancelled => "Cancelled",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completed_badge_has_checkmark() {
        let badge = StatusBadge::new(AutorunTaskRunStatus::Completed);
        let span = badge.to_span();
        let content = span.content.to_string();
        assert!(
            content.contains(theme::CHECKMARK),
            "should contain checkmark: {content}"
        );
        assert!(content.contains("Done"));
    }

    #[test]
    fn test_running_badge_has_bullet() {
        let badge = StatusBadge::new(AutorunTaskRunStatus::Running);
        let span = badge.to_span();
        let content = span.content.to_string();
        assert!(content.contains(theme::BULLET));
        assert!(content.contains("Running"));
    }

    #[test]
    fn test_failed_badge_has_cross() {
        let badge = StatusBadge::new(AutorunTaskRunStatus::Failed);
        let span = badge.to_span();
        let content = span.content.to_string();
        assert!(content.contains(theme::CROSS));
        assert!(content.contains("Failed"));
    }

    #[test]
    fn test_timeout_badge_has_caution() {
        let badge = StatusBadge::new(AutorunTaskRunStatus::Timeout);
        let span = badge.to_span();
        let content = span.content.to_string();
        assert!(content.contains(theme::CAUTION));
        assert!(content.contains("Timeout"));
    }

    #[test]
    fn test_pending_badge_has_circle() {
        let badge = StatusBadge::new(AutorunTaskRunStatus::Pending);
        let span = badge.to_span();
        let content = span.content.to_string();
        assert!(content.contains(theme::CIRCLE));
        assert!(content.contains("Pending"));
    }

    #[test]
    fn test_skipped_badge() {
        let badge = StatusBadge::new(AutorunTaskRunStatus::Skipped);
        let span = badge.to_span();
        assert!(span.content.contains("Skipped"));
    }

    #[test]
    fn test_cancelled_badge() {
        let badge = StatusBadge::new(AutorunTaskRunStatus::Cancelled);
        let span = badge.to_span();
        assert!(span.content.contains("Cancelled"));
    }

    #[test]
    fn test_display_override() {
        let badge =
            StatusBadge::new(AutorunTaskRunStatus::Pending).with_display("Waiting".to_string());
        let span = badge.to_span();
        assert!(span.content.contains("Waiting"));
        assert!(!span.content.contains("Pending"));
    }

    #[test]
    fn test_completed_style_is_green() {
        let badge = StatusBadge::new(AutorunTaskRunStatus::Completed);
        let span = badge.to_span();
        assert_eq!(span.style.fg, Some(theme::GREEN_SUCCESS));
    }

    #[test]
    fn test_running_style_is_yellow() {
        let badge = StatusBadge::new(AutorunTaskRunStatus::Running);
        let span = badge.to_span();
        assert_eq!(span.style.fg, Some(theme::YELLOW_RUNNING));
    }

    #[test]
    fn test_failed_style_is_red() {
        let badge = StatusBadge::new(AutorunTaskRunStatus::Failed);
        let span = badge.to_span();
        assert_eq!(span.style.fg, Some(theme::RED_FAILURE));
    }
}
