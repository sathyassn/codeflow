//! Color palette, unicode symbols, border styles, and text style presets for TUI rendering.

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::BorderType;

// ---------------------------------------------------------------------------
// Color constants
// ---------------------------------------------------------------------------

/// Green for success/completed states.
pub const GREEN_SUCCESS: Color = Color::Green;

/// Red for failure/error states.
pub const RED_FAILURE: Color = Color::Red;

/// Yellow for running/in-progress states.
pub const YELLOW_RUNNING: Color = Color::Yellow;

/// Dim gray for pending/inactive states.
pub const DIM_PENDING: Color = Color::DarkGray;

/// Blue for accent/informational elements.
pub const BLUE_ACCENT: Color = Color::Cyan;

/// White for standard text.
pub const WHITE_TEXT: Color = Color::White;

// ---------------------------------------------------------------------------
// Unicode symbol constants (zero emoji)
// ---------------------------------------------------------------------------

/// Checkmark (U+2713) — success indicator.
pub const CHECKMARK: char = '\u{2713}';

/// Cross mark (U+2717) — failure indicator.
pub const CROSS: char = '\u{2717}';

/// Bullet (U+25CF) — active/running indicator.
pub const BULLET: char = '\u{25CF}';

/// Circle (U+25CB) — pending/waiting indicator.
pub const CIRCLE: char = '\u{25CB}';

/// Right arrow (U+2192) — pipeline separator.
pub const ARROW: char = '\u{2192}';

/// Right-pointing triangle (U+25B8) — list pointer.
pub const TRIANGLE: char = '\u{25B8}';

/// Caution sign (U+26A0) — warning indicator.
pub const CAUTION: char = '\u{26A0}';

// ---------------------------------------------------------------------------
// Border style
// ---------------------------------------------------------------------------

/// Standard border type for all TUI panels.
pub const BORDER_TYPE: BorderType = BorderType::Rounded;

// ---------------------------------------------------------------------------
// Text style presets
// ---------------------------------------------------------------------------

/// Header style: bold white.
#[must_use]
pub fn header() -> Style {
    Style::new().fg(WHITE_TEXT).add_modifier(Modifier::BOLD)
}

/// Selected row highlight style.
#[must_use]
pub fn selected() -> Style {
    Style::new()
        .fg(Color::Black)
        .bg(BLUE_ACCENT)
        .add_modifier(Modifier::BOLD)
}

/// Dim text style for de-emphasized content.
#[must_use]
pub fn dim() -> Style {
    Style::new().fg(DIM_PENDING)
}

/// Error text style.
#[must_use]
pub fn error() -> Style {
    Style::new().fg(RED_FAILURE).add_modifier(Modifier::BOLD)
}

/// Success text style.
#[must_use]
pub fn success() -> Style {
    Style::new().fg(GREEN_SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_constants_are_distinct() {
        let colors = [
            GREEN_SUCCESS,
            RED_FAILURE,
            YELLOW_RUNNING,
            DIM_PENDING,
            BLUE_ACCENT,
            WHITE_TEXT,
        ];
        // Each color should be unique (no duplicates).
        for (i, a) in colors.iter().enumerate() {
            for (j, b) in colors.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "colors at index {i} and {j} should differ");
                }
            }
        }
    }

    #[test]
    fn test_unicode_symbol_values() {
        assert_eq!(CHECKMARK, '\u{2713}');
        assert_eq!(CROSS, '\u{2717}');
        assert_eq!(BULLET, '\u{25CF}');
        assert_eq!(CIRCLE, '\u{25CB}');
        assert_eq!(ARROW, '\u{2192}');
        assert_eq!(TRIANGLE, '\u{25B8}');
        assert_eq!(CAUTION, '\u{26A0}');
    }

    #[test]
    fn test_symbols_are_not_emoji() {
        // All symbols should be in the BMP (Basic Multilingual Plane), not emoji.
        for ch in [CHECKMARK, CROSS, BULLET, CIRCLE, ARROW, TRIANGLE, CAUTION] {
            assert!(
                (ch as u32) < 0x1F000,
                "symbol U+{:04X} is in emoji range",
                ch as u32
            );
        }
    }

    #[test]
    fn test_border_type_is_rounded() {
        assert_eq!(BORDER_TYPE, BorderType::Rounded);
    }

    #[test]
    fn test_header_style_is_bold_white() {
        let style = header();
        assert_eq!(style.fg, Some(WHITE_TEXT));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_selected_style_has_accent_bg() {
        let style = selected();
        assert_eq!(style.bg, Some(BLUE_ACCENT));
        assert_eq!(style.fg, Some(Color::Black));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_dim_style_uses_dark_gray() {
        let style = dim();
        assert_eq!(style.fg, Some(DIM_PENDING));
    }

    #[test]
    fn test_error_style_is_bold_red() {
        let style = error();
        assert_eq!(style.fg, Some(RED_FAILURE));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_success_style_is_green() {
        let style = success();
        assert_eq!(style.fg, Some(GREEN_SUCCESS));
    }
}
