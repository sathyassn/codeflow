//! Unified session detail pane shared by the autorun and interactive TUIs.
//!
//! Both `codeflow autorun status` and `codeflow interactive status` need to
//! render the same per-session fields (session ID, PID + liveness, worktree,
//! team, branch, work type, task, status, phase, duration). Autorun adds a
//! second row for PR number, worker exit code, and the stage pipeline.
//!
//! Previously these were two independent render paths that drifted apart —
//! the autorun pane hid the PID/liveness that operators need to debug the
//! worktree reaper. This widget is the single source of truth.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::data::StageInfo;
use crate::tui::theme;

/// Autorun-only detail fields: PR number, worker exit code, stage pipeline.
/// When `None`, the widget renders the interactive-session layout.
pub struct AutorunExtras<'a> {
    pub pr_number: Option<u32>,
    pub exit_code: Option<i32>,
    pub stages: &'a [StageInfo],
}

/// All fields rendered by [`render_session_detail`]. Callers populate the
/// subset they know about — unknown fields render as `--`.
pub struct SessionDetail<'a> {
    pub session_id: &'a str,
    pub pid: Option<i32>,
    pub pid_alive: Option<bool>,
    pub worktree_path: &'a str,
    pub team_name: Option<&'a str>,
    pub branch: Option<&'a str>,
    pub work_type: Option<&'a str>,
    /// Raw task ID (e.g. ULID). Shown in parentheses after
    /// `task_id_formatted` only when the two differ.
    pub task_id: Option<&'a str>,
    /// Human-readable task ID (e.g. `INF-TSK-024-046`). Preferred for
    /// display; `task_id` is shown as a fallback when this is `None`.
    pub task_id_formatted: Option<&'a str>,
    pub phase: Option<&'a str>,
    pub status: &'a str,
    pub duration_secs: Option<u64>,
    pub autorun_extras: Option<AutorunExtras<'a>>,
}

/// Render the unified session detail pane inside a bordered Block.
pub fn render_session_detail(f: &mut Frame<'_>, area: Rect, detail: &SessionDetail<'_>) {
    let lines = build_lines(detail);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme::BORDER_TYPE)
        .title(" Details ")
        .style(Style::new().fg(theme::WHITE_TEXT));
    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn build_lines(d: &SessionDetail<'_>) -> Vec<Line<'static>> {
    let mut lines = Vec::with_capacity(6);
    lines.push(line_session_pid_worktree(d));
    lines.push(line_team_branch_type(d));
    lines.push(line_task(d));
    lines.push(line_status_phase_duration(d));
    if let Some(extras) = &d.autorun_extras {
        lines.push(line_branch_pr_exit(d.branch.unwrap_or("--"), extras));
        lines.push(line_stages(extras.stages));
    }
    lines
}

/// `Session: ses-…  PID: 12345 (alive)  Worktree: /tmp/wt`
fn line_session_pid_worktree(d: &SessionDetail<'_>) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(9);
    spans.push(accent_label("Session: "));
    spans.push(Span::raw(d.session_id.to_string()));
    spans.push(Span::raw("  "));
    spans.push(accent_label("PID: "));
    match (d.pid, d.pid_alive) {
        (Some(pid), Some(alive)) => {
            spans.push(Span::raw(pid.to_string()));
            let (label, style) = liveness_marker(alive);
            spans.push(Span::styled(format!(" ({label})"), style));
        }
        (Some(pid), None) => {
            spans.push(Span::raw(pid.to_string()));
        }
        _ => spans.push(Span::raw("--".to_string())),
    }
    spans.push(Span::raw("  "));
    spans.push(accent_label("Worktree: "));
    spans.push(Span::styled(d.worktree_path.to_string(), theme::dim()));
    Line::from(spans)
}

/// `Team: team-a  Branch: feat/x  Type: FEAT`
fn line_team_branch_type(d: &SessionDetail<'_>) -> Line<'static> {
    Line::from(vec![
        accent_label("Team: "),
        Span::raw(d.team_name.unwrap_or("--").to_string()),
        Span::raw("  "),
        accent_label("Branch: "),
        Span::raw(d.branch.unwrap_or("--").to_string()),
        Span::raw("  "),
        accent_label("Type: "),
        Span::raw(d.work_type.unwrap_or("--").to_string()),
    ])
}

/// `Task: INF-TSK-024-046 (01KQ09PV56V6CPVMKK4FSW9ZDA)`
fn line_task(d: &SessionDetail<'_>) -> Line<'static> {
    let primary = d
        .task_id_formatted
        .filter(|s| !s.is_empty())
        .or(d.task_id.filter(|s| !s.is_empty()))
        .unwrap_or("--");
    let secondary = match (d.task_id_formatted, d.task_id) {
        (Some(formatted), Some(raw))
            if !formatted.is_empty() && !raw.is_empty() && formatted != raw =>
        {
            format!(" ({raw})")
        }
        _ => String::new(),
    };
    Line::from(vec![
        accent_label("Task: "),
        Span::raw(primary.to_string()),
        Span::styled(secondary, theme::dim()),
    ])
}

/// `Status: Running  Phase: PF4  Duration: 1m23s`
fn line_status_phase_duration(d: &SessionDetail<'_>) -> Line<'static> {
    Line::from(vec![
        accent_label("Status: "),
        Span::raw(d.status.to_string()),
        Span::raw("  "),
        accent_label("Phase: "),
        Span::raw(d.phase.unwrap_or("--").to_string()),
        Span::raw("  "),
        accent_label("Duration: "),
        Span::raw(format_duration(d.duration_secs)),
    ])
}

/// `Branch: feat/x  PR: #42  Exit: 0` — autorun-only line.
fn line_branch_pr_exit(branch: &str, extras: &AutorunExtras<'_>) -> Line<'static> {
    let pr = extras
        .pr_number
        .map_or_else(|| "--".to_string(), |n| format!("#{n}"));
    let exit = extras
        .exit_code
        .map_or_else(|| "--".to_string(), |c| c.to_string());
    Line::from(vec![
        accent_label("Branch: "),
        Span::raw(branch.to_string()),
        Span::raw("  "),
        accent_label("PR: "),
        Span::raw(pr),
        Span::raw("  "),
        accent_label("Exit: "),
        Span::raw(exit),
    ])
}

/// `Stages: WS-DEV ✓ → WS-REV ○` — autorun-only pipeline line.
fn line_stages(stages: &[StageInfo]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = vec![accent_label("Stages: ")];
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

fn accent_label(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), Style::new().fg(theme::BLUE_ACCENT))
}

fn liveness_marker(alive: bool) -> (&'static str, Style) {
    if alive {
        ("alive", Style::new().fg(theme::GREEN_SUCCESS))
    } else {
        ("DEAD", Style::new().fg(theme::RED_FAILURE))
    }
}

/// Format a duration in seconds as `Hh Mm Ss` / `Mm Ss` / `Ss` / `--`.
fn format_duration(secs: Option<u64>) -> String {
    let Some(total) = secs else {
        return "--".to_string();
    };
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn stages_sample() -> Vec<StageInfo> {
        vec![
            StageInfo {
                name: "WS-DEV".to_string(),
                completed: true,
            },
            StageInfo {
                name: "WS-REV".to_string(),
                completed: false,
            },
        ]
    }

    fn rendered_buffer(detail: &SessionDetail<'_>, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_session_detail(f, Rect::new(0, 0, width, height), detail);
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

    fn autorun_detail() -> SessionDetail<'static> {
        SessionDetail {
            session_id: "ses-worker-abc",
            pid: Some(12345),
            pid_alive: Some(true),
            worktree_path: "/tmp/wt",
            team_name: Some("team-a"),
            branch: Some("feat/x"),
            work_type: Some("FEAT"),
            task_id: Some("01KQ09PV56V6CPVMKK4FSW9ZDA"),
            task_id_formatted: Some("INF-TSK-024-046"),
            phase: Some("PF4"),
            status: "Running",
            duration_secs: Some(3_723),
            autorun_extras: Some(AutorunExtras {
                pr_number: Some(42),
                exit_code: None,
                stages: &[],
            }),
        }
    }

    fn interactive_detail() -> SessionDetail<'static> {
        SessionDetail {
            session_id: "ses-lead-xyz",
            pid: Some(67_890),
            pid_alive: Some(false),
            worktree_path: "/repos/main",
            team_name: Some("team-b"),
            branch: Some("main"),
            work_type: None,
            task_id: None,
            task_id_formatted: None,
            phase: None,
            status: "stale",
            duration_secs: Some(45),
            autorun_extras: None,
        }
    }

    // ---- Autorun variant -------------------------------------------------

    #[test]
    fn renders_autorun_with_extras_six_lines() {
        let mut d = autorun_detail();
        let stages = stages_sample();
        d.autorun_extras = Some(AutorunExtras {
            pr_number: Some(42),
            exit_code: Some(0),
            stages: &stages,
        });
        let lines = build_lines(&d);
        assert_eq!(
            lines.len(),
            6,
            "autorun variant has 6 lines (4 core + 2 extras)"
        );
    }

    #[test]
    fn autorun_line_one_has_session_pid_alive_worktree() {
        let d = autorun_detail();
        let out = rendered_buffer(&d, 80, 8);
        assert!(out.contains("Session:"), "Session label missing: {out}");
        assert!(out.contains("ses-worker-abc"), "session id missing: {out}");
        assert!(out.contains("PID:"), "PID label missing: {out}");
        assert!(out.contains("12345"), "pid missing: {out}");
        assert!(out.contains("(alive)"), "alive marker missing: {out}");
        assert!(out.contains("Worktree:"), "worktree label missing: {out}");
        assert!(out.contains("/tmp/wt"), "worktree path missing: {out}");
    }

    #[test]
    fn autorun_line_two_has_team_branch_type() {
        let d = autorun_detail();
        let out = rendered_buffer(&d, 80, 8);
        assert!(out.contains("Team:"), "Team label missing: {out}");
        assert!(out.contains("team-a"), "team name missing: {out}");
        assert!(out.contains("Branch:"), "Branch label missing: {out}");
        assert!(out.contains("feat/x"), "branch missing: {out}");
        assert!(out.contains("Type:"), "Type label missing: {out}");
        assert!(out.contains("FEAT"), "work type missing: {out}");
    }

    #[test]
    fn autorun_line_three_has_task_with_raw_in_parens() {
        let d = autorun_detail();
        let out = rendered_buffer(&d, 120, 8);
        assert!(out.contains("Task:"), "Task label missing: {out}");
        assert!(
            out.contains("INF-TSK-024-046"),
            "formatted task missing: {out}"
        );
        // The ULID fallback only renders inside parens when different.
        assert!(
            out.contains("(01KQ09PV56V6CPVMKK4FSW9ZDA)"),
            "raw task_id parens missing: {out}"
        );
    }

    #[test]
    fn autorun_line_four_has_status_phase_duration() {
        let d = autorun_detail();
        let out = rendered_buffer(&d, 80, 8);
        assert!(out.contains("Status:"), "Status label missing: {out}");
        assert!(out.contains("Running"), "status value missing: {out}");
        assert!(out.contains("Phase:"), "Phase label missing: {out}");
        assert!(out.contains("PF4"), "phase value missing: {out}");
        assert!(out.contains("Duration:"), "Duration label missing: {out}");
        // 3723s = 1h 2m 3s
        assert!(out.contains("1h 2m 3s"), "duration format missing: {out}");
    }

    #[test]
    fn autorun_line_five_has_pr_and_exit() {
        let d = autorun_detail();
        let out = rendered_buffer(&d, 80, 8);
        assert!(out.contains("PR:"), "PR label missing: {out}");
        assert!(out.contains("#42"), "PR number missing: {out}");
        assert!(out.contains("Exit:"), "Exit label missing: {out}");
    }

    #[test]
    fn autorun_line_six_has_stage_pipeline() {
        let mut d = autorun_detail();
        let stages = stages_sample();
        d.autorun_extras = Some(AutorunExtras {
            pr_number: Some(42),
            exit_code: Some(0),
            stages: &stages,
        });
        let out = rendered_buffer(&d, 100, 10);
        assert!(out.contains("Stages:"), "Stages label missing: {out}");
        assert!(out.contains("WS-DEV"), "WS-DEV missing: {out}");
        assert!(out.contains("WS-REV"), "WS-REV missing: {out}");
        assert!(
            out.contains(theme::CHECKMARK),
            "completed checkmark missing: {out}"
        );
        assert!(out.contains(theme::CIRCLE), "pending circle missing: {out}");
    }

    #[test]
    fn autorun_stages_none_renders_none_placeholder() {
        let line = line_stages(&[]);
        let s = line.to_string();
        assert!(s.contains("Stages:"));
        assert!(s.contains("(none)"));
    }

    #[test]
    fn autorun_pr_exit_dashes_when_missing() {
        let mut d = autorun_detail();
        d.autorun_extras = Some(AutorunExtras {
            pr_number: None,
            exit_code: None,
            stages: &[],
        });
        let out = rendered_buffer(&d, 80, 8);
        // Both PR and Exit should render "--" once each on the extras line.
        assert!(out.contains("PR: --"), "PR dash missing: {out}");
        assert!(out.contains("Exit: --"), "Exit dash missing: {out}");
    }

    // ---- Interactive variant (autorun_extras=None) -----------------------

    #[test]
    fn renders_interactive_without_extras_four_lines() {
        let d = interactive_detail();
        let lines = build_lines(&d);
        assert_eq!(
            lines.len(),
            4,
            "interactive variant has 4 lines (no extras)"
        );
    }

    #[test]
    fn interactive_line_one_shows_dead_marker() {
        let d = interactive_detail();
        let out = rendered_buffer(&d, 80, 6);
        assert!(out.contains("67890"), "pid missing: {out}");
        assert!(out.contains("(DEAD)"), "DEAD marker missing: {out}");
    }

    #[test]
    fn interactive_missing_task_shows_dashes() {
        let d = interactive_detail();
        let out = rendered_buffer(&d, 80, 6);
        // Task primary should render "--" when both fields are None.
        assert!(out.contains("Task: --"), "task dash missing: {out}");
    }

    #[test]
    fn interactive_missing_phase_work_type_shows_dashes() {
        let d = interactive_detail();
        let out = rendered_buffer(&d, 80, 6);
        assert!(out.contains("Phase: --"), "phase dash missing: {out}");
        assert!(out.contains("Type: --"), "type dash missing: {out}");
    }

    // ---- Pure helpers ----------------------------------------------------

    #[test]
    fn format_duration_none() {
        assert_eq!(format_duration(None), "--");
    }

    #[test]
    fn format_duration_seconds_only() {
        assert_eq!(format_duration(Some(45)), "45s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(Some(125)), "2m 5s");
    }

    #[test]
    fn format_duration_hours() {
        assert_eq!(format_duration(Some(3_723)), "1h 2m 3s");
    }

    #[test]
    fn liveness_marker_alive() {
        let (label, _) = liveness_marker(true);
        assert_eq!(label, "alive");
    }

    #[test]
    fn liveness_marker_dead() {
        let (label, _) = liveness_marker(false);
        assert_eq!(label, "DEAD");
    }

    #[test]
    fn pid_without_liveness_omits_marker() {
        let d = SessionDetail {
            session_id: "s",
            pid: Some(99),
            pid_alive: None,
            worktree_path: "/x",
            team_name: None,
            branch: None,
            work_type: None,
            task_id: None,
            task_id_formatted: None,
            phase: None,
            status: "Unknown",
            duration_secs: None,
            autorun_extras: None,
        };
        let line = line_session_pid_worktree(&d);
        let rendered = line.to_string();
        assert!(rendered.contains("PID: 99"));
        // No liveness label should be present.
        assert!(!rendered.contains("(alive)"));
        assert!(!rendered.contains("(DEAD)"));
    }

    #[test]
    fn no_pid_renders_dash() {
        let d = SessionDetail {
            session_id: "s",
            pid: None,
            pid_alive: None,
            worktree_path: "/x",
            team_name: None,
            branch: None,
            work_type: None,
            task_id: None,
            task_id_formatted: None,
            phase: None,
            status: "Pending",
            duration_secs: None,
            autorun_extras: None,
        };
        let line = line_session_pid_worktree(&d);
        let rendered = line.to_string();
        assert!(rendered.contains("PID: --"));
    }

    #[test]
    fn task_formatted_equal_to_raw_omits_parens() {
        let d = SessionDetail {
            session_id: "s",
            pid: None,
            pid_alive: None,
            worktree_path: "/x",
            team_name: None,
            branch: None,
            work_type: None,
            task_id: Some("INF-TSK-024-046"),
            task_id_formatted: Some("INF-TSK-024-046"),
            phase: None,
            status: "Pending",
            duration_secs: None,
            autorun_extras: None,
        };
        let line = line_task(&d);
        let rendered = line.to_string();
        assert!(rendered.contains("INF-TSK-024-046"));
        // When formatted and raw are identical, only the primary should render.
        assert!(!rendered.contains("(INF-TSK-024-046)"));
    }

    #[test]
    fn task_raw_only_renders_as_primary() {
        let d = SessionDetail {
            session_id: "s",
            pid: None,
            pid_alive: None,
            worktree_path: "/x",
            team_name: None,
            branch: None,
            work_type: None,
            task_id: Some("raw-id-only"),
            task_id_formatted: None,
            phase: None,
            status: "Pending",
            duration_secs: None,
            autorun_extras: None,
        };
        let line = line_task(&d);
        let rendered = line.to_string();
        assert!(rendered.contains("raw-id-only"));
        assert!(!rendered.contains("(raw-id-only)"));
    }
}
