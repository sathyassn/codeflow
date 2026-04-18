//! Phase badge widget mapping PathFlow phase to colored `Span`.

use ratatui::style::Style;
use ratatui::text::Span;

use crate::tui::theme;

/// Renders a PathFlow phase label as a colored `Span`.
pub struct PhaseBadge<'a> {
    phase: Option<&'a str>,
}

impl<'a> PhaseBadge<'a> {
    #[must_use]
    pub fn new(phase: Option<&'a str>) -> Self {
        Self { phase }
    }

    /// Convert to a styled `Span`.
    ///
    /// Accepts phase labels in several shapes:
    /// - Uppercase `PF1`..`PF7` (autorun legacy)
    /// - Lowercase `pf-1`..`pf-7` (interactive sessions,
    ///   `last_completed_phase` form)
    /// - Contextual fallbacks: `Starting`, `N/A`, `pre-pf1` (INF-TSK-047-001)
    /// - Anything else is rendered white.
    #[must_use]
    pub fn to_span(&self) -> Span<'static> {
        let Some(phase) = self.phase else {
            return Span::styled("--", theme::dim());
        };

        let style = match phase {
            "PF1" | "PF2" | "PF3" | "pf-1" | "pf-2" | "pf-3" => Style::new().fg(theme::BLUE_ACCENT),
            "PF4" | "pf-4" | "Starting" => Style::new().fg(theme::YELLOW_RUNNING),
            "PF5" | "PF6" | "pf-5" | "pf-6" => Style::new().fg(theme::GREEN_SUCCESS),
            "PF7" | "pf-7" | "N/A" | "pre-pf1" => Style::new().fg(theme::DIM_PENDING),
            _ => Style::new().fg(theme::WHITE_TEXT),
        };

        Span::styled(phase.to_string(), style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_none_phase_shows_dashes() {
        let badge = PhaseBadge::new(None);
        let span = badge.to_span();
        assert_eq!(span.content.as_ref(), "--");
    }

    #[test]
    fn test_pf1_is_blue() {
        let badge = PhaseBadge::new(Some("PF1"));
        let span = badge.to_span();
        assert_eq!(span.content.as_ref(), "PF1");
        assert_eq!(span.style.fg, Some(theme::BLUE_ACCENT));
    }

    #[test]
    fn test_pf4_is_yellow() {
        let badge = PhaseBadge::new(Some("PF4"));
        let span = badge.to_span();
        assert_eq!(span.content.as_ref(), "PF4");
        assert_eq!(span.style.fg, Some(theme::YELLOW_RUNNING));
    }

    #[test]
    fn test_pf5_is_green() {
        let badge = PhaseBadge::new(Some("PF5"));
        let span = badge.to_span();
        assert_eq!(span.content.as_ref(), "PF5");
        assert_eq!(span.style.fg, Some(theme::GREEN_SUCCESS));
    }

    #[test]
    fn test_pf6_is_green() {
        let badge = PhaseBadge::new(Some("PF6"));
        let span = badge.to_span();
        assert_eq!(span.style.fg, Some(theme::GREEN_SUCCESS));
    }

    #[test]
    fn test_pf7_is_dim() {
        let badge = PhaseBadge::new(Some("PF7"));
        let span = badge.to_span();
        assert_eq!(span.style.fg, Some(theme::DIM_PENDING));
    }

    #[test]
    fn test_unknown_phase_is_white() {
        let badge = PhaseBadge::new(Some("PF99"));
        let span = badge.to_span();
        assert_eq!(span.content.as_ref(), "PF99");
        assert_eq!(span.style.fg, Some(theme::WHITE_TEXT));
    }

    #[test]
    fn test_pf2_is_blue() {
        let badge = PhaseBadge::new(Some("PF2"));
        let span = badge.to_span();
        assert_eq!(span.style.fg, Some(theme::BLUE_ACCENT));
    }

    #[test]
    fn test_pf3_is_blue() {
        let badge = PhaseBadge::new(Some("PF3"));
        let span = badge.to_span();
        assert_eq!(span.style.fg, Some(theme::BLUE_ACCENT));
    }

    #[test]
    fn test_lowercase_pf4_is_yellow() {
        let badge = PhaseBadge::new(Some("pf-4"));
        let span = badge.to_span();
        assert_eq!(span.content.as_ref(), "pf-4");
        assert_eq!(span.style.fg, Some(theme::YELLOW_RUNNING));
    }

    #[test]
    fn test_starting_is_yellow() {
        let badge = PhaseBadge::new(Some("Starting"));
        let span = badge.to_span();
        assert_eq!(span.content.as_ref(), "Starting");
        assert_eq!(span.style.fg, Some(theme::YELLOW_RUNNING));
    }

    #[test]
    fn test_pre_pf1_is_dim() {
        let badge = PhaseBadge::new(Some("pre-pf1"));
        let span = badge.to_span();
        assert_eq!(span.content.as_ref(), "pre-pf1");
        assert_eq!(span.style.fg, Some(theme::DIM_PENDING));
    }

    #[test]
    fn test_na_is_dim() {
        let badge = PhaseBadge::new(Some("N/A"));
        let span = badge.to_span();
        assert_eq!(span.content.as_ref(), "N/A");
        assert_eq!(span.style.fg, Some(theme::DIM_PENDING));
    }

    #[test]
    fn test_lowercase_pf6_is_green() {
        let badge = PhaseBadge::new(Some("pf-6"));
        let span = badge.to_span();
        assert_eq!(span.style.fg, Some(theme::GREEN_SUCCESS));
    }
}
