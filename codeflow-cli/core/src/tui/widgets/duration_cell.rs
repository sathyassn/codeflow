//! Duration formatting widget with color thresholds.

use ratatui::style::Style;
use ratatui::text::Span;

use crate::tui::theme;

/// Warning threshold: tasks running longer than 30 minutes show yellow.
const WARN_THRESHOLD_SECS: i64 = 30 * 60;

/// Critical threshold: tasks running longer than 60 minutes show red.
const CRITICAL_THRESHOLD_SECS: i64 = 60 * 60;

/// Renders a duration in seconds as a formatted, color-coded `Span`.
pub struct DurationCell {
    secs: Option<i64>,
}

impl DurationCell {
    #[must_use]
    pub fn new(secs: Option<i64>) -> Self {
        Self { secs }
    }

    /// Convert to a styled `Span`.
    #[must_use]
    pub fn to_span(&self) -> Span<'static> {
        let Some(secs) = self.secs else {
            return Span::styled("--:--", theme::dim());
        };

        let formatted = format_duration(secs);
        let style = duration_style(secs);
        Span::styled(formatted, style)
    }
}

/// Format seconds as MM:SS or HH:MM:SS.
#[must_use]
pub fn format_duration(secs: i64) -> String {
    let total = secs.max(0);
    let hours = total / 3600;
    let mins = (total % 3600) / 60;
    let s = total % 60;
    if hours > 0 {
        format!("{hours:02}:{mins:02}:{s:02}")
    } else {
        format!("{mins:02}:{s:02}")
    }
}

fn duration_style(secs: i64) -> Style {
    if secs >= CRITICAL_THRESHOLD_SECS {
        Style::new().fg(theme::RED_FAILURE)
    } else if secs >= WARN_THRESHOLD_SECS {
        Style::new().fg(theme::YELLOW_RUNNING)
    } else {
        Style::new().fg(theme::WHITE_TEXT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_zero() {
        assert_eq!(format_duration(0), "00:00");
    }

    #[test]
    fn test_format_duration_seconds_only() {
        assert_eq!(format_duration(45), "00:45");
    }

    #[test]
    fn test_format_duration_minutes_and_seconds() {
        assert_eq!(format_duration(125), "02:05");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3661), "01:01:01");
    }

    #[test]
    fn test_format_duration_negative_clamped() {
        assert_eq!(format_duration(-10), "00:00");
    }

    #[test]
    fn test_none_duration_shows_dashes() {
        let cell = DurationCell::new(None);
        let span = cell.to_span();
        assert_eq!(span.content.as_ref(), "--:--");
    }

    #[test]
    fn test_short_duration_is_white() {
        let cell = DurationCell::new(Some(120));
        let span = cell.to_span();
        assert_eq!(span.content.as_ref(), "02:00");
        assert_eq!(span.style.fg, Some(theme::WHITE_TEXT));
    }

    #[test]
    fn test_warn_threshold_is_yellow() {
        let cell = DurationCell::new(Some(WARN_THRESHOLD_SECS));
        let span = cell.to_span();
        assert_eq!(span.style.fg, Some(theme::YELLOW_RUNNING));
    }

    #[test]
    fn test_critical_threshold_is_red() {
        let cell = DurationCell::new(Some(CRITICAL_THRESHOLD_SECS));
        let span = cell.to_span();
        assert_eq!(span.style.fg, Some(theme::RED_FAILURE));
    }

    #[test]
    fn test_above_critical_is_red() {
        let cell = DurationCell::new(Some(CRITICAL_THRESHOLD_SECS + 100));
        let span = cell.to_span();
        assert_eq!(span.style.fg, Some(theme::RED_FAILURE));
    }

    #[test]
    fn test_just_below_warn_is_white() {
        let cell = DurationCell::new(Some(WARN_THRESHOLD_SECS - 1));
        let span = cell.to_span();
        assert_eq!(span.style.fg, Some(theme::WHITE_TEXT));
    }
}
