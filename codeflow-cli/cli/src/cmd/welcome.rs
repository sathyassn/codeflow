//! Welcome command: display formatted project overview.
//!
//! Renders a bordered ratatui panel when running in a TTY, falling back to
//! plain text output when piped or redirected.

use std::io::IsTerminal;
use std::path::Path;

use anyhow::Result;

use crate::helpers;

/// Run the welcome command.
///
/// Detects the project root and active session count, then renders
/// either a TUI panel (TTY) or plain text (non-TTY).
#[allow(clippy::unnecessary_wraps)]
pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_root().ok();
    let version = env!("CARGO_PKG_VERSION");
    let active_sessions = project_dir.as_deref().map_or(0, count_active_sessions);

    if std::io::stdout().is_terminal() {
        render_tui(version, active_sessions);
    } else {
        render_plain(version, active_sessions);
    }
    Ok(())
}

/// Count active interactive sessions by scanning heartbeat files.
///
/// Returns 0 on any error (best-effort).
fn count_active_sessions(project_dir: &Path) -> usize {
    let hb_dir = project_dir.join(".state/interactive");
    let Ok(entries) = std::fs::read_dir(&hb_dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("heartbeat-"))
        .count()
}

/// Render the welcome panel using ratatui inline rendering.
fn render_tui(version: &str, active_sessions: usize) {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;
    use ratatui::widgets::{Block, Borders, Paragraph, Widget};

    use codeflow_core::tui::theme;

    let lines = build_lines(version, active_sessions);

    // Height: content lines + 2 (top/bottom border). Truncation is safe
    // because line count is bounded by build_lines (~9 lines).
    #[allow(clippy::cast_possible_truncation)]
    let height = (lines.len() + 2) as u16;
    let width = 60;
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme::BORDER_TYPE)
        .title(" CodeFlow ")
        .style(Style::new().fg(theme::WHITE_TEXT));

    let paragraph = Paragraph::new(lines).block(block);
    paragraph.render(area, &mut buf);

    // Write buffer to stdout line by line.
    for y in 0..height {
        let line: String = (0..width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect();
        println!("{line}");
    }
}

/// Render plain-text fallback for non-TTY output.
fn render_plain(version: &str, active_sessions: usize) {
    println!("CodeFlow v{version}");
    println!();
    println!("AI-native development framework for structured, traceable work.");
    println!();
    println!("Active sessions: {active_sessions}");
    println!();
    println!("Quick start:");
    println!("  codeflow -i             Launch interactive session");
    println!("  codeflow interactive status --watch  Live dashboard");
    println!("  codeflow --help         Show all commands");
}

/// Build styled lines for both TUI and test assertions.
fn build_lines(version: &str, active_sessions: usize) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::style::Style;
    use ratatui::text::{Line, Span};

    use codeflow_core::tui::theme;

    vec![
        Line::from(vec![
            Span::styled("CodeFlow ", theme::header()),
            Span::styled(format!("v{version}"), Style::new().fg(theme::BLUE_ACCENT)),
        ]),
        Line::raw(""),
        Line::styled(
            "AI-native development framework",
            Style::new().fg(theme::DIM_PENDING),
        ),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                format!("{} ", theme::BULLET),
                if active_sessions > 0 {
                    Style::new().fg(theme::GREEN_SUCCESS)
                } else {
                    Style::new().fg(theme::DIM_PENDING)
                },
            ),
            Span::raw(format!("{active_sessions} active session(s)")),
        ]),
        Line::raw(""),
        Line::styled("Quick start:", Style::new().fg(theme::WHITE_TEXT)),
        Line::from(vec![
            Span::styled(
                format!("  {} ", theme::TRIANGLE),
                Style::new().fg(theme::BLUE_ACCENT),
            ),
            Span::raw("codeflow -i"),
            Span::styled("  Launch interactive session", theme::dim()),
        ]),
        Line::from(vec![
            Span::styled(
                format!("  {} ", theme::TRIANGLE),
                Style::new().fg(theme::BLUE_ACCENT),
            ),
            Span::raw("codeflow --help"),
            Span::styled("  Show all commands", theme::dim()),
        ]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_active_sessions_no_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(count_active_sessions(tmp.path()), 0);
    }

    #[test]
    fn test_count_active_sessions_with_heartbeats() {
        let tmp = tempfile::tempdir().unwrap();
        let hb_dir = tmp.path().join(".state/interactive");
        std::fs::create_dir_all(&hb_dir).unwrap();
        std::fs::write(hb_dir.join("heartbeat-ses-001"), "").unwrap();
        std::fs::write(hb_dir.join("heartbeat-ses-002"), "").unwrap();
        std::fs::write(hb_dir.join("other-file"), "").unwrap(); // not a heartbeat
        assert_eq!(count_active_sessions(tmp.path()), 2);
    }

    #[test]
    fn test_build_lines_has_version() {
        let lines = build_lines("0.1.0", 3);
        let text: String = lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("v0.1.0"), "should contain version: {text}");
        assert!(
            text.contains("3 active session(s)"),
            "should show session count: {text}"
        );
    }

    #[test]
    fn test_build_lines_zero_sessions() {
        let lines = build_lines("0.1.0", 0);
        let text: String = lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("0 active session(s)"));
    }

    #[test]
    fn test_build_lines_contains_quick_start() {
        let lines = build_lines("0.1.0", 0);
        let text: String = lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("Quick start:"),
            "should contain quick start section"
        );
        assert!(
            text.contains("codeflow -i"),
            "should contain launch command"
        );
    }

    #[test]
    fn test_render_tui_succeeds() {
        // Verify the TUI render path doesn't panic.
        render_tui("0.1.0", 0);
    }

    #[test]
    fn test_render_plain_produces_output() {
        // Verify plain render doesn't panic.
        render_plain("0.1.0", 2);
    }
}
