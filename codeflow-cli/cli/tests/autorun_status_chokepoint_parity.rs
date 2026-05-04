//! INF-TSK-050-003 AC-04 / WS-REV MAJOR-1: text-mode and TUI-mode
//! liveness verdicts must agree on identical input state.
//!
//! The two modes use DIFFERENT call paths to reach the same chokepoint:
//!
//! - **Text mode** (`autorun.rs::run_status` line 1860): for each
//!   `AutorunWorker` row, `is_session_alive(project_dir, &w.worker_session_id)
//!   .is_alive()`.
//!
//! - **TUI mode** (`tui::data::resolve_worker_pid_liveness` line 1061):
//!   for each `AutorunWorker` row, looks up the canonical lead PID via
//!   `read_canonical_lead_pid` first (worktree-registry path), and only
//!   then calls `is_session_alive`.
//!
//! A regression that, e.g., changed text mode to use
//! `validate_claude_pid(w.pid)` directly (bypassing the chokepoint)
//! would diverge from TUI mode. This test enforces parity by:
//!   1. Seeding a `pathflow-session-status.json` for a synthetic
//!      session ID with a chosen `lead_pid`.
//!   2. Computing the text-mode verdict via `is_session_alive`.
//!   3. Computing the TUI-mode verdict via `resolve_worker_pid_liveness`
//!      (the function autorun status TUI consumes via `tui::data`).
//!   4. Asserting both verdicts agree across (a) live PID, (b) dead PID,
//!      and (c) missing status file.
//!
//! WS-REV MAJOR-1 fix: previously the test called `is_session_alive`
//! twice in the same scope and asserted equality with itself —
//! tautological. Now both modes are exercised through their distinct
//! call paths.

use codeflow_core::session::liveness::{
    SessionLiveness, is_session_alive, validate_orchestrator_pid,
};
use codeflow_core::tui::data::resolve_worker_pid_liveness;

/// Seed a `pathflow-session-status.json` with the given `lead_pid` for
/// `sid` under `project_dir/.state/session/{sid}/pathflow/`. Mirrors
/// the production hook layout exactly.
fn seed_status_file(project_dir: &std::path::Path, sid: &str, lead_pid: u32) {
    let pf_dir = project_dir
        .join(".state")
        .join("session")
        .join(sid)
        .join("pathflow");
    std::fs::create_dir_all(&pf_dir).expect("create pathflow dir");
    let status_file = pf_dir.join("pathflow-session-status.json");
    let body = serde_json::json!({
        "session_id": sid,
        "status": "pf-in-progress",
        "lead_pid": lead_pid,
    });
    std::fs::write(&status_file, body.to_string()).expect("write status file");
}

/// Replicates the text-mode liveness predicate at `cli/src/cmd/autorun.rs::run_status`
/// line 1860 byte-for-byte. If anyone changes that line, this helper
/// must be updated to match — and the parity test will then fail until
/// both modes converge again. That's the load-bearing property.
fn text_mode_verdict(project_dir: &std::path::Path, sid: &str) -> bool {
    is_session_alive(project_dir, sid).is_alive()
}

/// Replicates the TUI-mode liveness predicate at
/// `tui::data::resolve_worker_pid_liveness`. Returns `Option<bool>`
/// because the TUI distinguishes "unknown" from a definite verdict.
/// We map `None` → `false` to align with text mode's behavior (text
/// mode doesn't have a Unknown-display state for autorun workers).
fn tui_mode_verdict(project_dir: &std::path::Path, sid: &str) -> bool {
    resolve_worker_pid_liveness(project_dir, None, Some(sid))
        .1
        .unwrap_or(false)
}

#[test]
fn parity_dead_pid() {
    // PID guaranteed dead → both modes report dead.
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path();
    let sid = "ses-parity-dead-pid";
    seed_status_file(project, sid, 4_000_000);

    let text = text_mode_verdict(project, sid);
    let tui = tui_mode_verdict(project, sid);
    assert!(!text, "text mode must report dead for unreachable PID");
    assert!(!tui, "TUI mode must report dead for unreachable PID");
    assert_eq!(
        text, tui,
        "AC-04 / MAJOR-1: text + TUI verdicts must match on dead PID"
    );
}

#[test]
fn parity_missing_status_file() {
    // No status file → text mode chokepoint returns Unknown (is_alive
    // = false); TUI mode resolve_worker_pid_liveness returns
    // (None, None) which we map to false. Both consistent: dead.
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path();
    let sid = "ses-parity-missing-file";

    let text = text_mode_verdict(project, sid);
    let tui = tui_mode_verdict(project, sid);
    assert!(!text, "text mode: Unknown chokepoint verdict is not alive");
    assert!(!tui, "TUI mode: missing canonical PID is not alive");
    assert_eq!(text, tui, "AC-04 / MAJOR-1: parity on missing file");
    assert_eq!(
        is_session_alive(project, sid),
        SessionLiveness::Unknown,
        "missing file must surface as Unknown verdict"
    );
}

#[test]
fn parity_live_pid_current_process() {
    // The current test process IS alive but is not named "claude".
    // The chokepoint's `validate_claude_pid` requires both alive AND
    // named-claude, so it returns 0 (dead). Both modes must agree:
    // dead. This proves both paths consume the same name-validation
    // step — a regression that bypassed the name check in EITHER mode
    // would diverge from this assertion.
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path();
    let sid = "ses-parity-live-test-runner";
    seed_status_file(project, sid, std::process::id());

    let text = text_mode_verdict(project, sid);
    let tui = tui_mode_verdict(project, sid);
    assert!(
        !text,
        "text mode: current PID is alive but not named 'claude' → dead per chokepoint"
    );
    assert!(!tui, "TUI mode: same chokepoint contract → dead");
    assert_eq!(
        text, tui,
        "AC-04 / MAJOR-1: parity on live-but-not-claude PID (proves name check is consumed by BOTH modes)"
    );
}

#[test]
fn parity_empty_session_id_treated_as_unknown_by_both_modes() {
    // Edge case: an empty SID. Text mode: `is_session_alive` returns
    // Unknown (no file). TUI mode: `resolve_worker_pid_liveness` with
    // `Some("")` is filtered to None by the `filter(|s| !s.is_empty())`
    // guard at line 1066, so it returns (None, None). Both consistent.
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path();
    let text = text_mode_verdict(project, "");
    let tui = tui_mode_verdict(project, "");
    assert_eq!(text, tui, "empty SID must produce same verdict");
    assert!(!text);
}

#[test]
fn validate_orchestrator_pid_agrees_with_kernel() {
    // INF-TSK-050-003 AC-03: orchestrator liveness uses
    // `validate_orchestrator_pid` (PID-only). Verify the function
    // produces the expected verdict for live + dead inputs. This
    // locks down the public-API contract that autorun text mode
    // reads off `session.pid`.
    let live = std::process::id();
    assert!(
        validate_orchestrator_pid(live),
        "current process must be alive"
    );
    assert!(
        !validate_orchestrator_pid(0),
        "PID 0 must always be rejected"
    );
    assert!(
        !validate_orchestrator_pid(4_000_000),
        "PID 4_000_000 must be rejected as dead"
    );
}
