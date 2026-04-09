//! Detail pane widget rendering task details: branch, PR, stages, worktree path.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::tui::data::{StageInfo, TaskView};
use crate::tui::theme;

/// Renders a bordered detail panel for the currently selected task.
pub struct DetailPane<'a> {
    task: Option<&'a TaskView>,
}

impl<'a> DetailPane<'a> {
    #[must_use]
    pub fn new(task: Option<&'a TaskView>) -> Self {
        Self { task }
    }

    fn build_lines(&self) -> Vec<Line<'static>> {
        let Some(task) = self.task else {
            return vec![Line::styled("No task selected".to_string(), theme::dim())];
        };

        let branch_str = task.branch.as_deref().unwrap_or("--").to_string();
        let pr_str = task
            .pr_number
            .map_or_else(|| "--".to_string(), |n| format!("#{n}"));
        let exit_str = task
            .exit_code
            .map_or_else(|| "--".to_string(), |c| c.to_string());

        let branch_line = Line::from(vec![
            Span::styled("Branch: ".to_string(), Style::new().fg(theme::BLUE_ACCENT)),
            Span::raw(branch_str),
            Span::raw("  "),
            Span::styled("PR: ".to_string(), Style::new().fg(theme::BLUE_ACCENT)),
            Span::raw(pr_str),
            Span::raw("  "),
            Span::styled("Exit: ".to_string(), Style::new().fg(theme::BLUE_ACCENT)),
            Span::raw(exit_str),
        ]);

        let pipeline_line = build_pipeline_line(&task.stages);

        let wt_path = task.worktree_path.as_deref().unwrap_or("--").to_string();
        let path_line = Line::from(vec![
            Span::styled(
                "Worktree: ".to_string(),
                Style::new().fg(theme::BLUE_ACCENT),
            ),
            Span::styled(wt_path, theme::dim()),
        ]);

        let mut lines = vec![branch_line, pipeline_line, path_line];

        if let Some(ref err) = task.error_message {
            lines.push(Line::from(vec![
                Span::styled("Error: ".to_string(), theme::error()),
                Span::styled(err.clone(), theme::error()),
            ]));
        }

        lines
    }
}

impl Widget for DetailPane<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme::BORDER_TYPE)
            .title(" Details ")
            .style(Style::new().fg(theme::WHITE_TEXT));

        let lines = self.build_lines();
        let paragraph = Paragraph::new(lines).block(block);
        paragraph.render(area, buf);
    }
}

/// Build the stage pipeline display line with status indicators.
fn build_pipeline_line(stages: &[StageInfo]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = vec![Span::styled(
        "Stages: ",
        Style::new().fg(theme::BLUE_ACCENT),
    )];

    if stages.is_empty() {
        spans.push(Span::styled("(none)".to_string(), theme::dim()));
        return Line::from(spans);
    }

    for (i, stage) in stages.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(format!(" {} ", theme::ARROW), theme::dim()));
        }

        let (symbol, style) = if stage.completed {
            (theme::CHECKMARK, Style::new().fg(theme::GREEN_SUCCESS))
        } else {
            (theme::CIRCLE, theme::dim())
        };

        spans.push(Span::styled(stage.name.clone(), style));
        spans.push(Span::styled(format!(" {symbol}"), style));
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_task() -> TaskView {
        TaskView {
            task_id: "task-a".to_string(),
            status: crate::types::AutorunTaskRunStatus::Running,
            display_status: "running".to_string(),
            phase: Some("PF4".to_string()),
            branch: Some("feat/tui".to_string()),
            pr_number: Some(42),
            tmux_session: Some("tmux-1".to_string()),
            duration_secs: Some(120),
            exit_code: None,
            worktree_path: Some("/tmp/wt".to_string()),
            error_message: None,
            stages: vec![
                StageInfo {
                    name: "WS-DEV".to_string(),
                    completed: true,
                },
                StageInfo {
                    name: "WS-REV".to_string(),
                    completed: false,
                },
            ],
        }
    }

    #[test]
    fn test_no_task_selected() {
        let pane = DetailPane::new(None);
        let lines = pane.build_lines();
        assert_eq!(lines.len(), 1);
        let content = lines[0].to_string();
        assert!(content.contains("No task selected"));
    }

    #[test]
    fn test_branch_and_pr_displayed() {
        let task = sample_task();
        let pane = DetailPane::new(Some(&task));
        let lines = pane.build_lines();
        let branch_line = lines[0].to_string();
        assert!(
            branch_line.contains("feat/tui"),
            "branch missing: {branch_line}"
        );
        assert!(branch_line.contains("#42"), "PR missing: {branch_line}");
    }

    #[test]
    fn test_pipeline_shows_stages() {
        let task = sample_task();
        let pane = DetailPane::new(Some(&task));
        let lines = pane.build_lines();
        let pipeline = lines[1].to_string();
        assert!(pipeline.contains("WS-DEV"), "dev stage missing: {pipeline}");
        assert!(pipeline.contains("WS-REV"), "rev stage missing: {pipeline}");
    }

    #[test]
    fn test_worktree_path_displayed() {
        let task = sample_task();
        let pane = DetailPane::new(Some(&task));
        let lines = pane.build_lines();
        let path_line = lines[2].to_string();
        assert!(path_line.contains("/tmp/wt"), "path missing: {path_line}");
    }

    #[test]
    fn test_error_message_shown() {
        let mut task = sample_task();
        task.error_message = Some("something broke".to_string());
        let pane = DetailPane::new(Some(&task));
        let lines = pane.build_lines();
        assert_eq!(lines.len(), 4);
        let err_line = lines[3].to_string();
        assert!(err_line.contains("something broke"));
    }

    #[test]
    fn test_missing_branch_shows_dashes() {
        let mut task = sample_task();
        task.branch = None;
        task.pr_number = None;
        let pane = DetailPane::new(Some(&task));
        let lines = pane.build_lines();
        let line = lines[0].to_string();
        // Both branch and PR should show "--"
        assert!(line.matches("--").count() >= 2, "missing dashes: {line}");
    }

    #[test]
    fn test_pipeline_line_with_no_stages() {
        let line = build_pipeline_line(&[]);
        let content = line.to_string();
        assert!(content.contains("Stages:"));
        assert!(
            content.contains("(none)"),
            "empty stages should show '(none)': {content}"
        );
    }

    #[test]
    fn test_pipeline_line_completed_stage_has_checkmark() {
        let stages = vec![StageInfo {
            name: "WS-DEV".to_string(),
            completed: true,
        }];
        let line = build_pipeline_line(&stages);
        let content = line.to_string();
        assert!(content.contains(theme::CHECKMARK));
    }

    #[test]
    fn test_pipeline_line_incomplete_stage_has_circle() {
        let stages = vec![StageInfo {
            name: "WS-REV".to_string(),
            completed: false,
        }];
        let line = build_pipeline_line(&stages);
        let content = line.to_string();
        assert!(content.contains(theme::CIRCLE));
    }

    #[test]
    fn test_widget_renders_to_buffer() {
        let task = sample_task();
        let pane = DetailPane::new(Some(&task));
        let area = Rect::new(0, 0, 80, 7);
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);

        // Collect rendered text from each row (inside the border).
        let row_text =
            |y: u16| -> String { (0..80).map(|x| buf[(x, y)].symbol().to_string()).collect() };

        // Row 1 (inside top border): should contain branch and PR info.
        let r1 = row_text(1);
        assert!(r1.contains("feat/tui"), "row 1 should contain branch: {r1}");
        assert!(r1.contains("#42"), "row 1 should contain PR number: {r1}");

        // Row 2: should contain stage pipeline info.
        let r2 = row_text(2);
        assert!(
            r2.contains("WS-DEV"),
            "row 2 should contain stage name: {r2}"
        );

        // Row 3: should contain worktree path.
        let r3 = row_text(3);
        assert!(
            r3.contains("/tmp/wt"),
            "row 3 should contain worktree path: {r3}"
        );
    }

    #[test]
    fn test_widget_renders_no_task_to_buffer() {
        let pane = DetailPane::new(None);
        let area = Rect::new(0, 0, 40, 4);
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);
        let r1: String = (0..40).map(|x| buf[(x, 1)].symbol().to_string()).collect();
        assert!(
            r1.contains("No task selected"),
            "should show placeholder: {r1}"
        );
    }
}
