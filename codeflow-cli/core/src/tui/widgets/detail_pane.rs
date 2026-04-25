//! Autorun task detail pane — thin adapter over the unified session detail
//! widget.
//!
//! Converts a [`TaskView`] into a [`SessionDetail`] with
//! [`AutorunExtras`] populated (PR, exit code, stage pipeline) and delegates
//! to [`render_session_detail`]. This is the ONLY renderer for the autorun
//! TUI's Details pane.

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::tui::data::{TaskView, format_task_id_for_display};
use crate::tui::widgets::session_detail_pane::{
    AutorunExtras, SessionDetail, render_session_detail,
};

/// Render the autorun Details pane for the currently selected task.
///
/// `task=None` renders an empty detail pane — matches the "No task selected"
/// affordance of the previous `DetailPane` widget, but via the unified
/// builder (all fields dashed).
pub fn render_task_detail(f: &mut Frame<'_>, area: Rect, task: Option<&TaskView>) {
    let Some(task) = task else {
        let empty = SessionDetail {
            session_id: "--",
            pid: None,
            pid_alive: None,
            worktree_path: "--",
            team_name: None,
            branch: None,
            work_type: None,
            task_id: None,
            task_id_formatted: None,
            phase: None,
            status: "--",
            duration_secs: None,
            autorun_extras: None,
        };
        render_session_detail(f, area, &empty);
        return;
    };

    let formatted = format_task_id_for_display(task.task_format_id.as_deref(), Some(&task.task_id));
    // If `format_task_id_for_display` ends up returning the raw ULID (no
    // task_format_id), treat the primary as the raw task_id so the widget
    // does not double-render it in parens.
    let task_id_formatted = if formatted == task.task_id {
        None
    } else {
        Some(formatted)
    };

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let duration_secs = task.duration_secs.filter(|&s| s >= 0).map(|s| s as u64);

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let pr_number = task.pr_number.filter(|&n| n >= 0).map(|n| n as u32);

    // Real OS exit codes fit in `i32`; clamp on the (impossible) overflow
    // path rather than silently truncating. The widget renders this as a
    // string, so the value-domain matters — no UB risk, just display
    // correctness if the DB ever stores something out-of-range.
    let exit_code = task.exit_code.map(|c| i32::try_from(c).unwrap_or(i32::MAX));

    let stages_slice = task.stages.as_slice();
    let extras = AutorunExtras {
        pr_number,
        exit_code,
        stages: stages_slice,
    };

    let detail = SessionDetail {
        session_id: task.worker_session_id.as_deref().unwrap_or("--"),
        pid: task.pid,
        pid_alive: task.pid_alive,
        worktree_path: task.worktree_path.as_deref().unwrap_or("--"),
        team_name: None, // autorun workers do not carry a team name
        branch: task.branch.as_deref(),
        work_type: task.work_type.as_deref(),
        task_id: Some(task.task_id.as_str()),
        task_id_formatted: task_id_formatted.as_deref(),
        phase: task.phase.as_deref(),
        status: &task.display_status,
        duration_secs,
        autorun_extras: Some(extras),
    };
    render_session_detail(f, area, &detail);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::data::StageInfo;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn sample_task() -> TaskView {
        TaskView {
            task_id: "01KQ09PV56V6CPVMKK4FSW9ZDA".to_string(),
            status: crate::types::AutorunTaskRunStatus::Running,
            display_status: "Running".to_string(),
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
            worker_session_id: Some("ses-worker-1".to_string()),
            work_type: Some("FEAT".to_string()),
            pid: Some(12345),
            pid_alive: Some(true),
            task_format_id: Some("INF-TSK-024-046".to_string()),
        }
    }

    fn render_to_string(task: Option<&TaskView>, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_task_detail(f, Rect::new(0, 0, width, height), task);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn renders_task_delegates_to_unified_pane() {
        let task = sample_task();
        let out = render_to_string(Some(&task), 120, 10);
        // Core fields unified pane shows.
        assert!(out.contains("Session:"), "Session label missing: {out}");
        assert!(out.contains("ses-worker-1"), "session id missing: {out}");
        assert!(out.contains("PID:"), "PID label missing: {out}");
        assert!(out.contains("12345"), "pid missing: {out}");
        assert!(out.contains("(alive)"), "liveness missing: {out}");
        assert!(out.contains("Worktree:"), "worktree label missing: {out}");
        assert!(out.contains("/tmp/wt"), "worktree path missing: {out}");
        assert!(out.contains("Branch:"), "branch label missing: {out}");
        assert!(out.contains("feat/tui"), "branch missing: {out}");
        assert!(
            out.contains("INF-TSK-024-046"),
            "formatted task missing: {out}"
        );
        assert!(out.contains("PR:"), "PR label missing: {out}");
        assert!(out.contains("#42"), "PR missing: {out}");
        assert!(out.contains("Stages:"), "Stages label missing: {out}");
        assert!(out.contains("WS-DEV"), "WS-DEV missing: {out}");
        assert!(out.contains("WS-REV"), "WS-REV missing: {out}");
    }

    #[test]
    fn renders_no_task_with_dashes() {
        let out = render_to_string(None, 80, 8);
        assert!(out.contains("Session: --"), "missing session dash: {out}");
        assert!(out.contains("PID: --"), "missing pid dash: {out}");
        assert!(out.contains("Task: --"), "missing task dash: {out}");
    }

    #[test]
    fn renders_missing_branch_pr_as_dashes() {
        let mut task = sample_task();
        task.branch = None;
        task.pr_number = None;
        let out = render_to_string(Some(&task), 120, 10);
        assert!(out.contains("Branch: --"), "branch dash missing: {out}");
        assert!(out.contains("PR: --"), "pr dash missing: {out}");
    }

    #[test]
    fn renders_dead_pid_marker() {
        let mut task = sample_task();
        task.pid_alive = Some(false);
        let out = render_to_string(Some(&task), 80, 10);
        assert!(out.contains("(DEAD)"), "DEAD marker missing: {out}");
    }

    #[test]
    fn raw_task_id_shown_when_no_format_id() {
        let mut task = sample_task();
        task.task_format_id = None;
        let out = render_to_string(Some(&task), 120, 10);
        // Primary should be the truncated ULID; raw is not duplicated in
        // parens since primary == raw after the adapter's normalization.
        assert!(out.contains("Task:"), "Task label missing: {out}");
    }

    #[test]
    fn exit_code_in_range_renders_verbatim() {
        let mut task = sample_task();
        task.exit_code = Some(125);
        let out = render_to_string(Some(&task), 120, 10);
        assert!(out.contains("Exit: 125"), "exit code missing: {out}");
    }

    #[test]
    fn exit_code_negative_renders_verbatim() {
        let mut task = sample_task();
        task.exit_code = Some(-1);
        let out = render_to_string(Some(&task), 120, 10);
        assert!(
            out.contains("Exit: -1"),
            "negative exit code missing: {out}"
        );
    }

    #[test]
    fn exit_code_above_i32_max_clamps_to_i32_max() {
        // Any i64 outside the i32 domain should clamp to i32::MAX rather
        // than silently truncate. F4 fix: replaced unchecked `as i32` with
        // `i32::try_from(c).unwrap_or(i32::MAX)`.
        let mut task = sample_task();
        task.exit_code = Some(i64::from(i32::MAX) + 1);
        let out = render_to_string(Some(&task), 120, 10);
        let expected = format!("Exit: {}", i32::MAX);
        assert!(
            out.contains(&expected),
            "out-of-range exit_code should clamp to i32::MAX: {out}"
        );
    }
}
