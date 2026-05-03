//! Shared freeze-on-terminal elapsed-time helper used by every TUI surface.
//!
//! INF-TSK-049-001 AC #7: the autorun and interactive dashboards both need the
//! same rule for the ELAPSED / DURATION column — tick live for running rows,
//! freeze at `completed_at - created_at` (falling back to `updated_at`) for
//! terminal rows, never reference `now()` after termination.
//!
//! The logic lived inline in `tui::data::status_driven_elapsed_secs` (autorun)
//! and in `fetch_session_views_with_keep_last` (interactive). Both are now
//! thin wrappers around [`freeze_on_terminal_secs`] via the
//! [`HasLifecycleTimestamps`] trait.
//!
//! Pure functions — `now` is passed explicitly so tests pin the clock.

use chrono::{DateTime, Utc};

/// Sentinel: the interactive dashboard renders this as `"--"` (unknown
/// duration) for terminal rows whose `completed_at` / `updated_at` were both
/// lost. Autorun callers map this through the regular "0" branch because
/// their view-model type is unsigned-interpretable.
pub const UNKNOWN_DURATION_SECS: i64 = -1;

/// View of the lifecycle timestamps and status bit a row needs to decide how
/// to compute its elapsed value. Implemented for `AutorunSession` and
/// `InteractiveSession`; keeps the helper entity-agnostic.
pub trait HasLifecycleTimestamps {
    /// RFC 3339 timestamp of when the row started.
    fn created_at(&self) -> &str;
    /// Optional RFC 3339 timestamp of when the row terminated.
    fn completed_at(&self) -> Option<&str>;
    /// Optional RFC 3339 timestamp of the last update (fallback when
    /// `completed_at` is absent but we still need a terminal anchor).
    fn updated_at(&self) -> Option<&str>;
    /// True when the row is in a terminal state and ELAPSED MUST freeze.
    fn is_terminal(&self) -> bool;
    /// True when the row is not yet started (e.g. Pending). Callers render
    /// these as 0. Distinct from `is_terminal` because Pending has no
    /// meaningful "started" moment yet.
    fn is_not_started(&self) -> bool {
        false
    }
}

/// Compute the ELAPSED value for a lifecycle row.
///
/// Rules:
///
/// - `is_not_started()` → 0 (not started yet).
/// - `is_terminal()` → `completed_at - created_at` when `completed_at` is set,
///   else `updated_at - created_at`. Falls back to 0 when neither endpoint is
///   present and `allow_unknown` is `false`; returns
///   [`UNKNOWN_DURATION_SECS`] when `allow_unknown` is `true` so the caller
///   can render `"--"` for a truly unanchored row.
/// - Otherwise (live) → `now - created_at`.
///
/// Returns seconds, clamped to `>= 0` on the positive paths. Never returns a
/// negative value on the "running" path, even if the clock jumps backward.
///
/// Pure function — `now` is passed explicitly so tests can pin the clock.
#[must_use]
pub fn freeze_on_terminal_secs<E: HasLifecycleTimestamps>(
    entity: &E,
    now: DateTime<Utc>,
    allow_unknown: bool,
) -> i64 {
    if entity.is_not_started() {
        return 0;
    }
    if entity.is_terminal() {
        if let Some(completed) = entity.completed_at() {
            return secs_between(entity.created_at(), completed);
        }
        if let Some(updated) = entity.updated_at() {
            return secs_between(entity.created_at(), updated);
        }
        return if allow_unknown {
            UNKNOWN_DURATION_SECS
        } else {
            0
        };
    }
    secs_from(entity.created_at(), now)
}

/// Parse an RFC 3339 timestamp and return `max(0, now - start)`.
#[must_use]
pub fn secs_from(started_at: &str, now: DateTime<Utc>) -> i64 {
    DateTime::parse_from_rfc3339(started_at)
        .map(|start| now.signed_duration_since(start).num_seconds().max(0))
        .unwrap_or(0)
}

/// Parse two RFC 3339 timestamps and return `max(0, end - start)`. Returns 0
/// when either parse fails — used only on anchored terminal rows where a
/// parse failure is indistinguishable from a 0-duration row for display.
#[must_use]
pub fn secs_between(start: &str, end: &str) -> i64 {
    let start_dt = DateTime::parse_from_rfc3339(start);
    let end_dt = DateTime::parse_from_rfc3339(end);
    match (start_dt, end_dt) {
        (Ok(s), Ok(e)) => e.signed_duration_since(s).num_seconds().max(0),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// HasLifecycleTimestamps impls for the two session entities that own the
// ELAPSED / DURATION columns in their respective dashboards. The logic is
// identical; the only entity-specific bit is `is_terminal` / `is_not_started`.
// ---------------------------------------------------------------------------

impl HasLifecycleTimestamps for crate::models::AutorunSession {
    fn created_at(&self) -> &str {
        &self.created_at
    }
    fn completed_at(&self) -> Option<&str> {
        self.completed_at.as_deref()
    }
    fn updated_at(&self) -> Option<&str> {
        self.updated_at.as_deref()
    }
    fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }
    // INF-TSK-050-001 AC #20: `is_not_started` falls back to the trait
    // default (`false`) now that `Pending` is removed. No autorun status
    // produces a "not yet started" state — the orchestrator writes
    // `Running` at row creation, so ELAPSED ticks from `created_at`.
}

impl HasLifecycleTimestamps for crate::models::InteractiveSession {
    fn created_at(&self) -> &str {
        &self.created_at
    }
    fn completed_at(&self) -> Option<&str> {
        self.completed_at.as_deref()
    }
    fn updated_at(&self) -> Option<&str> {
        self.updated_at.as_deref()
    }
    fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    /// Minimal test entity so the generic helper can be exercised without a
    /// real `AutorunSession` / `InteractiveSession`.
    struct Row {
        created_at: String,
        completed_at: Option<String>,
        updated_at: Option<String>,
        terminal: bool,
        not_started: bool,
    }

    impl HasLifecycleTimestamps for Row {
        fn created_at(&self) -> &str {
            &self.created_at
        }
        fn completed_at(&self) -> Option<&str> {
            self.completed_at.as_deref()
        }
        fn updated_at(&self) -> Option<&str> {
            self.updated_at.as_deref()
        }
        fn is_terminal(&self) -> bool {
            self.terminal
        }
        fn is_not_started(&self) -> bool {
            self.not_started
        }
    }

    fn rfc(ts: DateTime<Utc>) -> String {
        ts.to_rfc3339()
    }

    #[test]
    fn live_row_uses_now_minus_created() {
        let now = Utc::now();
        let row = Row {
            created_at: rfc(now - Duration::seconds(30)),
            completed_at: None,
            updated_at: None,
            terminal: false,
            not_started: false,
        };
        let secs = freeze_on_terminal_secs(&row, now, false);
        assert_eq!(secs, 30);
    }

    #[test]
    fn terminal_row_freezes_on_completed_at() {
        let now = Utc::now();
        let row = Row {
            created_at: rfc(now - Duration::seconds(600)),
            completed_at: Some(rfc(now - Duration::seconds(300))),
            updated_at: Some(rfc(now - Duration::seconds(60))),
            terminal: true,
            not_started: false,
        };
        let secs = freeze_on_terminal_secs(&row, now, false);
        // 600 - 300 = 300 (uses completed_at, not updated_at, not now).
        assert_eq!(secs, 300);
    }

    #[test]
    fn terminal_row_falls_back_to_updated_at_when_completed_missing() {
        let now = Utc::now();
        let row = Row {
            created_at: rfc(now - Duration::seconds(100)),
            completed_at: None,
            updated_at: Some(rfc(now - Duration::seconds(40))),
            terminal: true,
            not_started: false,
        };
        let secs = freeze_on_terminal_secs(&row, now, false);
        // 100 - 40 = 60.
        assert_eq!(secs, 60);
    }

    #[test]
    fn terminal_row_no_endpoint_returns_zero_when_unknown_disallowed() {
        let now = Utc::now();
        let row = Row {
            created_at: rfc(now - Duration::seconds(500)),
            completed_at: None,
            updated_at: None,
            terminal: true,
            not_started: false,
        };
        assert_eq!(freeze_on_terminal_secs(&row, now, false), 0);
    }

    #[test]
    fn terminal_row_no_endpoint_returns_sentinel_when_unknown_allowed() {
        let now = Utc::now();
        let row = Row {
            created_at: rfc(now - Duration::seconds(500)),
            completed_at: None,
            updated_at: None,
            terminal: true,
            not_started: false,
        };
        assert_eq!(
            freeze_on_terminal_secs(&row, now, true),
            UNKNOWN_DURATION_SECS
        );
    }

    #[test]
    fn not_started_row_is_zero() {
        let now = Utc::now();
        let row = Row {
            created_at: rfc(now - Duration::seconds(120)),
            completed_at: None,
            updated_at: None,
            terminal: false,
            not_started: true,
        };
        assert_eq!(freeze_on_terminal_secs(&row, now, false), 0);
    }

    #[test]
    fn terminal_row_is_idempotent_across_calls() {
        // Re-invoking the function N times with advancing `now` must produce
        // the same frozen value for a terminal row — the "freeze" guarantee.
        let created = Utc::now() - Duration::seconds(1000);
        let completed = Utc::now() - Duration::seconds(600);
        let row = Row {
            created_at: rfc(created),
            completed_at: Some(rfc(completed)),
            updated_at: None,
            terminal: true,
            not_started: false,
        };
        let a = freeze_on_terminal_secs(&row, Utc::now(), false);
        let b = freeze_on_terminal_secs(&row, Utc::now() + Duration::seconds(60), false);
        let c = freeze_on_terminal_secs(&row, Utc::now() + Duration::seconds(3600), false);
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn live_row_clamps_negative_to_zero() {
        let now = Utc::now();
        // created_at is in the FUTURE relative to `now` (simulates clock drift).
        let row = Row {
            created_at: rfc(now + Duration::seconds(60)),
            completed_at: None,
            updated_at: None,
            terminal: false,
            not_started: false,
        };
        assert_eq!(freeze_on_terminal_secs(&row, now, false), 0);
    }

    #[test]
    fn invalid_created_at_returns_zero() {
        let now = Utc::now();
        let row = Row {
            created_at: "not-a-timestamp".to_string(),
            completed_at: None,
            updated_at: None,
            terminal: false,
            not_started: false,
        };
        assert_eq!(freeze_on_terminal_secs(&row, now, false), 0);
    }

    #[test]
    fn invalid_completed_at_falls_through_to_zero() {
        // secs_between returns 0 on parse failure. We choose NOT to fall
        // through to updated_at in that case because the parse failure is a
        // data error, not an absence — surfacing 0 signals "something's
        // wrong with the row" without a panicking path.
        let now = Utc::now();
        let row = Row {
            created_at: rfc(now - Duration::seconds(60)),
            completed_at: Some("not-a-timestamp".to_string()),
            updated_at: Some(rfc(now - Duration::seconds(30))),
            terminal: true,
            not_started: false,
        };
        assert_eq!(freeze_on_terminal_secs(&row, now, false), 0);
    }

    #[test]
    fn secs_from_parses_rfc_and_clamps() {
        let now = Utc::now();
        let past = rfc(now - Duration::seconds(42));
        assert_eq!(secs_from(&past, now), 42);
        // future
        let future = rfc(now + Duration::seconds(42));
        assert_eq!(secs_from(&future, now), 0);
        // malformed
        assert_eq!(secs_from("bogus", now), 0);
    }

    // -----------------------------------------------------------------------
    // Entity-specific impls for AutorunSession / InteractiveSession.
    //
    // INF-TSK-049-001 AC #7: the shared helper MUST produce identical output
    // for both entity types so the autorun and interactive dashboards agree.
    // Cover every status variant.
    // -----------------------------------------------------------------------

    use crate::models::{AutorunSession, InteractiveSession};
    use crate::types::{AutorunSessionStatus, InteractiveSessionStatus};

    fn make_autorun_session(
        status: AutorunSessionStatus,
        created: &str,
        completed: Option<&str>,
        updated: Option<&str>,
    ) -> AutorunSession {
        AutorunSession {
            id: "ar-test".into(),
            batch_file: "b.yaml".into(),
            batch_name: Some("test".into()),
            status,
            max_session_workers: 1,
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            pid: None,
            skipped_tasks: 0,
            tmux_session: None,
            stale_reason: None,
            target_branch: None,
            final_pr_url: None,
            current_task_id: None,
            current_task_format_id: None,
            updated_at: updated.map(String::from),
            last_heartbeat_at: None,
            created_at: created.into(),
            completed_at: completed.map(String::from),
            abort_started_at: None,
        }
    }

    fn make_interactive_session(
        status: InteractiveSessionStatus,
        created: &str,
        completed: Option<&str>,
        updated: Option<&str>,
    ) -> InteractiveSession {
        InteractiveSession {
            id: "is-test".into(),
            session_id: "ses-test".into(),
            status,
            worktree_path: None,
            branch: None,
            work_type: None,
            task_id: None,
            task_format_id: None,
            last_phase: None,
            tmux_session: None,
            team_name: None,
            source_cli: "codeflow".into(),
            managed: true,
            session_kind: "interactive".into(),
            created_at: created.into(),
            updated_at: updated.map(String::from),
            completed_at: completed.map(String::from),
        }
    }

    #[test]
    fn autorun_running_is_live() {
        let now = Utc::now();
        let s = make_autorun_session(
            AutorunSessionStatus::Running,
            &rfc(now - Duration::seconds(45)),
            None,
            None,
        );
        assert_eq!(freeze_on_terminal_secs(&s, now, false), 45);
    }

    #[test]
    fn autorun_completed_freezes() {
        let now = Utc::now();
        let s = make_autorun_session(
            AutorunSessionStatus::Completed,
            &rfc(now - Duration::seconds(500)),
            Some(&rfc(now - Duration::seconds(100))),
            None,
        );
        assert_eq!(freeze_on_terminal_secs(&s, now, false), 400);
    }

    #[test]
    fn autorun_failed_freezes() {
        let now = Utc::now();
        let s = make_autorun_session(
            AutorunSessionStatus::Failed,
            &rfc(now - Duration::seconds(300)),
            Some(&rfc(now - Duration::seconds(50))),
            None,
        );
        assert_eq!(freeze_on_terminal_secs(&s, now, false), 250);
    }

    #[test]
    fn autorun_cancelled_freezes() {
        let now = Utc::now();
        let s = make_autorun_session(
            AutorunSessionStatus::Cancelled,
            &rfc(now - Duration::seconds(1000)),
            Some(&rfc(now - Duration::seconds(10))),
            None,
        );
        assert_eq!(freeze_on_terminal_secs(&s, now, false), 990);
    }

    #[test]
    fn autorun_timeout_freezes_via_updated_at_when_completed_absent() {
        let now = Utc::now();
        let s = make_autorun_session(
            AutorunSessionStatus::Timeout,
            &rfc(now - Duration::seconds(900)),
            None,
            Some(&rfc(now - Duration::seconds(60))),
        );
        assert_eq!(freeze_on_terminal_secs(&s, now, false), 840);
    }

    #[test]
    fn autorun_aborting_is_live() {
        let now = Utc::now();
        let s = make_autorun_session(
            AutorunSessionStatus::Aborting,
            &rfc(now - Duration::seconds(120)),
            None,
            None,
        );
        assert_eq!(freeze_on_terminal_secs(&s, now, false), 120);
    }

    #[test]
    fn autorun_terminal_is_idempotent_across_now() {
        let s = make_autorun_session(
            AutorunSessionStatus::Completed,
            &rfc(Utc::now() - Duration::seconds(2000)),
            Some(&rfc(Utc::now() - Duration::seconds(1500))),
            None,
        );
        let a = freeze_on_terminal_secs(&s, Utc::now(), false);
        let b = freeze_on_terminal_secs(&s, Utc::now() + Duration::seconds(3600), false);
        assert_eq!(a, b);
    }

    #[test]
    fn interactive_active_is_live() {
        let now = Utc::now();
        let s = make_interactive_session(
            InteractiveSessionStatus::Active,
            &rfc(now - Duration::seconds(77)),
            None,
            None,
        );
        assert_eq!(freeze_on_terminal_secs(&s, now, false), 77);
    }

    #[test]
    fn interactive_stale_freezes_on_completed_at() {
        let now = Utc::now();
        let s = make_interactive_session(
            InteractiveSessionStatus::Stale,
            &rfc(now - Duration::seconds(400)),
            Some(&rfc(now - Duration::seconds(90))),
            None,
        );
        assert_eq!(freeze_on_terminal_secs(&s, now, false), 310);
    }

    #[test]
    fn interactive_complete_freezes_on_completed_at() {
        let now = Utc::now();
        let s = make_interactive_session(
            InteractiveSessionStatus::Complete,
            &rfc(now - Duration::seconds(1234)),
            Some(&rfc(now - Duration::seconds(234))),
            None,
        );
        assert_eq!(freeze_on_terminal_secs(&s, now, false), 1000);
    }

    #[test]
    fn interactive_stale_no_endpoint_returns_unknown_when_allowed() {
        let now = Utc::now();
        let s = make_interactive_session(
            InteractiveSessionStatus::Stale,
            &rfc(now - Duration::seconds(100)),
            None,
            None,
        );
        assert_eq!(
            freeze_on_terminal_secs(&s, now, true),
            UNKNOWN_DURATION_SECS
        );
    }

    #[test]
    fn interactive_stale_falls_back_to_updated_at() {
        let now = Utc::now();
        let s = make_interactive_session(
            InteractiveSessionStatus::Stale,
            &rfc(now - Duration::seconds(300)),
            None,
            Some(&rfc(now - Duration::seconds(50))),
        );
        assert_eq!(freeze_on_terminal_secs(&s, now, false), 250);
    }

    #[test]
    fn interactive_and_autorun_parity_for_same_timeline() {
        // INF-TSK-049-001 AC #7: the two entities with identical timestamps
        // and terminal-equivalent statuses MUST produce the same output.
        let now = Utc::now();
        let created = rfc(now - Duration::seconds(600));
        let completed = rfc(now - Duration::seconds(100));
        let ar = make_autorun_session(
            AutorunSessionStatus::Completed,
            &created,
            Some(&completed),
            None,
        );
        let it = make_interactive_session(
            InteractiveSessionStatus::Complete,
            &created,
            Some(&completed),
            None,
        );
        assert_eq!(
            freeze_on_terminal_secs(&ar, now, false),
            freeze_on_terminal_secs(&it, now, false)
        );
    }

    #[test]
    fn secs_between_returns_ordered_difference() {
        let now = Utc::now();
        let start = rfc(now - Duration::seconds(100));
        let end = rfc(now - Duration::seconds(40));
        assert_eq!(secs_between(&start, &end), 60);
        // Reversed = clamps to 0.
        assert_eq!(secs_between(&end, &start), 0);
        // One invalid = 0.
        assert_eq!(secs_between("bad", &end), 0);
        assert_eq!(secs_between(&start, "bad"), 0);
    }
}
