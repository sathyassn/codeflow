//! Integration test for INF-TSK-050-014 — worktree session latch bug.
//!
//! Verifies the end-to-end fix: a tracked session that spawns its first
//! teammate before `TeamCreate` is observable by the post-tool-use
//! pipeline no longer produces a duplicate worktree when a later
//! teammate fires its `SessionStart`. Specifically, after the simulated
//! sequence:
//!
//! 1. Lead writes `pathflow-session-status.json` with `lead_pid`.
//! 2. Lead's `WorktreeRegistry` is populated with one active entry for
//!    the session.
//! 3. First teammate's `Task` post-tool-use fires
//!    `handle_teammate_spawn` against missing `pathflow-team.json`
//!    (Layer 1 creates it).
//! 4. A second teammate's `SessionStart` fires.
//!
//! `WorktreeRegistry::list_active()` MUST still return exactly one
//! worktree for the session (AC-07). Pre-fix the second teammate was
//! misclassified as NEW LEAD and the CLI's lead-mode path added a
//! second registry entry; post-fix Layer 2 classifies it as TEAMMATE
//! MODE and `InitResult::is_teammate` is `true`, so the CLI's teammate
//! path does NOT register an additional worktree.

use std::fs;

use codeflow_core::SessionId;
use codeflow_core::hooks::session_start::SessionStartInit;
use codeflow_core::hooks::{HookEvent, HookInput};
use codeflow_core::worktree::{WorktreeEntry, WorktreeStatus};
use codeflow_core::{hooks::post_tool_use, session, worktree};
use serial_test::serial;

fn fixed_now() -> String {
    "2026-05-07T00:00:00Z".to_string()
}

fn make_session_start_input(project_dir: &str, claude_sid: &str) -> HookInput {
    HookInput {
        tool_name: None,
        tool_input: None,
        event: HookEvent::SessionStart,
        session_id: Some(claude_sid.into()),
        project_dir: Some(project_dir.into()),
        source: Some("startup".into()),
        transcript_path: None,
        ..Default::default()
    }
}

/// RAII guard that removes CODEFLOW_* environment variables for the
/// duration of a test and restores them on drop. The integration test
/// runs inside a `codeflow -i` session whose env vars (CODEFLOW_MANAGED,
/// CODEFLOW_SESSION_ID, CODEFLOW_WORKTREE_PATH, CF_PROJECT_ROOT) would
/// otherwise hijack the SessionStartInit::run() fast path and prevent
/// the AC-07 scenario from exercising the teammate-detection code path.
struct CodeflowEnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl CodeflowEnvGuard {
    fn new() -> Self {
        const KEYS: &[&str] = &[
            "CODEFLOW_MANAGED",
            "CODEFLOW_SESSION_ID",
            "CODEFLOW_WORKTREE_PATH",
            "CF_PROJECT_ROOT",
            "AUTORUN_SESSION_ID",
            "AUTORUN_BATCH_ID",
        ];
        let saved: Vec<_> = KEYS.iter().map(|k| (*k, std::env::var(k).ok())).collect();
        // SAFETY: env mutation is guarded by #[serial(env_vars)]; no
        // other thread sees racing reads/writes.
        unsafe {
            for (k, _) in &saved {
                std::env::remove_var(k);
            }
        }
        Self { saved }
    }
}

impl Drop for CodeflowEnvGuard {
    fn drop(&mut self) {
        // SAFETY: env mutation is guarded by #[serial(env_vars)].
        unsafe {
            for (k, v) in &self.saved {
                if let Some(val) = v {
                    std::env::set_var(k, val);
                } else {
                    std::env::remove_var(k);
                }
            }
        }
    }
}

#[test]
#[serial(env_vars)]
fn test_worktree_count_stays_one_through_teammate_session_start() {
    // Permissive PID validator: the test runner is `cargo`, not
    // `claude`, so the production chokepoint
    // (`validate_claude_pid`) returns 0. Substitute a permissive
    // validator that just checks the process is alive — recovers the
    // pre-Phase-3 semantic for tests that fake a teammate scenario.
    let _validator_guard = codeflow_core::session::liveness::override_pid_validator_for_tests(
        codeflow_core::session::process::is_process_alive,
    );
    // Strip CODEFLOW_* env vars so SessionStartInit::run() doesn't take
    // the CODEFLOW_MANAGED fast path (which would hijack session_id
    // from the parent codeflow -i session that runs cargo test).
    let _env_guard = CodeflowEnvGuard::new();

    let project_dir_handle = tempfile::tempdir().unwrap();
    let project_dir = project_dir_handle.path();
    let home_handle = tempfile::tempdir().unwrap();
    let home = home_handle.path();

    let sid = SessionId::new_unchecked("ses-01jq7integration0000000");
    let runtime_dir = project_dir.join(".state").join("runtime");
    session::write_env_file(&runtime_dir, &sid, "codeflow").unwrap();

    // Step 1: lead writes pathflow-session-status.json with alive
    // lead_pid (use the test process pid so kill(pid, 0) succeeds).
    let pathflow_dir = project_dir
        .join(".state")
        .join("session")
        .join(sid.as_str())
        .join("pathflow");
    fs::create_dir_all(&pathflow_dir).unwrap();
    let lead_pid = std::process::id();
    fs::write(
        pathflow_dir.join("pathflow-session-status.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "session_id": sid.as_str(),
            "status": "pf-in-progress",
            "team_name": "integration-team",
            "lead_pid": lead_pid,
            "updated_at": fixed_now(),
        }))
        .unwrap(),
    )
    .unwrap();

    let team_config_dir = home.join(".claude").join("teams").join("integration-team");
    fs::create_dir_all(&team_config_dir).unwrap();
    fs::write(team_config_dir.join("config.json"), r#"{"members":[]}"#).unwrap();

    // Step 2: register the lead's worktree entry. We simulate the CLI
    // path that `codeflow -i` follows for a tracked lead session.
    let registry_dir = project_dir.join(".state").join("worktrees");
    fs::create_dir_all(&registry_dir).unwrap();
    let registry_path = registry_dir.join("worktrees.yaml");
    let lead_entry = WorktreeEntry {
        name: format!("worktree-{}", sid.as_str()),
        path: format!(
            "{}/.git-worktrees/worktree-{}",
            project_dir.display(),
            sid.as_str()
        ),
        branch: Some("fix/integration-test".to_string()),
        created_at: fixed_now(),
        status: WorktreeStatus::Active,
        session_id: Some(sid.as_str().to_string()),
        task_id: None,
        source: Some("interactive".to_string()),
    };
    worktree::locked_register_with_limit(&registry_path, &lead_entry, 5).unwrap();

    // Sanity: exactly one active worktree before teammate spawn.
    let registry = worktree::read_registry(&registry_path).unwrap();
    assert_eq!(
        worktree::list_active(&registry).len(),
        1,
        "precondition: exactly one active worktree for the lead"
    );

    // Step 3: first teammate spawn (Layer 1 creates pathflow-team.json
    // even though TeamCreate has not been observed by post-tool-use).
    post_tool_use::handle_teammate_spawn("cf-security", &pathflow_dir, None).unwrap();
    assert!(
        pathflow_dir.join("pathflow-team.json").exists(),
        "Layer 1: pathflow-team.json must exist after first teammate spawn"
    );

    // Step 4: second teammate's SessionStart fires through the public
    // `run()` entry point. The result's `is_teammate` flag tells the
    // CLI whether to take the lead-mode path (which registers a new
    // worktree) or the teammate-mode path (which does not).
    let init = SessionStartInit {
        lead_pid: 99_999,
        home_dir: home.to_path_buf(),
        now: fixed_now,
    };
    let input =
        make_session_start_input(project_dir.to_str().unwrap(), "claude-second-teammate-uuid");
    let mut buf = Vec::new();
    let init_result = init.run(&input, project_dir, &mut buf).unwrap();

    assert_eq!(
        init_result.session_id, sid,
        "AC-07: second teammate must reuse the lead's session ID"
    );
    assert!(
        init_result.is_teammate,
        "AC-07: second teammate must be classified as TEAMMATE MODE \
         (is_teammate=true so the CLI's teammate path runs and no \
         duplicate worktree is registered)"
    );

    // Step 5: simulate the CLI's behaviour. In TEAMMATE MODE, the CLI
    // does NOT call `register_worktree` again. The registry stays at
    // one entry. We assert this directly.
    let registry = worktree::read_registry(&registry_path).unwrap();
    let active = worktree::list_active(&registry);
    assert_eq!(
        active.len(),
        1,
        "AC-07: WorktreeRegistry must still have exactly 1 active worktree, found {}",
        active.len()
    );
    assert_eq!(
        active[0].session_id.as_deref(),
        Some(sid.as_str()),
        "the single active entry must belong to the original session"
    );
}

#[test]
#[serial(env_vars)]
fn test_worktree_count_stays_one_when_team_file_missing_layer2() {
    // INF-TSK-050-014 Layer 2 path: even if Layer 1 had failed to
    // create pathflow-team.json (defense-in-depth), Layer 2's reader
    // still classifies the second teammate as TEAMMATE MODE on the
    // basis of an alive lead and a session config dir — preventing the
    // worktree-count-becomes-2 bug.
    let _validator_guard = codeflow_core::session::liveness::override_pid_validator_for_tests(
        codeflow_core::session::process::is_process_alive,
    );
    let _env_guard = CodeflowEnvGuard::new();

    let project_dir_handle = tempfile::tempdir().unwrap();
    let project_dir = project_dir_handle.path();
    let home_handle = tempfile::tempdir().unwrap();
    let home = home_handle.path();

    let sid = SessionId::new_unchecked("ses-01jq7integnoteam000000");
    let runtime_dir = project_dir.join(".state").join("runtime");
    session::write_env_file(&runtime_dir, &sid, "codeflow").unwrap();

    let pathflow_dir = project_dir
        .join(".state")
        .join("session")
        .join(sid.as_str())
        .join("pathflow");
    fs::create_dir_all(&pathflow_dir).unwrap();
    let lead_pid = std::process::id();
    fs::write(
        pathflow_dir.join("pathflow-session-status.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "session_id": sid.as_str(),
            "status": "pf-in-progress",
            "team_name": "noteam-team",
            "lead_pid": lead_pid,
            "updated_at": fixed_now(),
        }))
        .unwrap(),
    )
    .unwrap();
    // pathflow-team.json INTENTIONALLY MISSING — Layer 1 either failed
    // or hasn't fired yet.

    let team_config_dir = home.join(".claude").join("teams").join("noteam-team");
    fs::create_dir_all(&team_config_dir).unwrap();
    fs::write(team_config_dir.join("config.json"), r#"{"members":[]}"#).unwrap();

    let registry_dir = project_dir.join(".state").join("worktrees");
    fs::create_dir_all(&registry_dir).unwrap();
    let registry_path = registry_dir.join("worktrees.yaml");
    let lead_entry = WorktreeEntry {
        name: format!("worktree-{}", sid.as_str()),
        path: format!(
            "{}/.git-worktrees/worktree-{}",
            project_dir.display(),
            sid.as_str()
        ),
        branch: None,
        created_at: fixed_now(),
        status: WorktreeStatus::Active,
        session_id: Some(sid.as_str().to_string()),
        task_id: None,
        source: Some("interactive".to_string()),
    };
    worktree::locked_register_with_limit(&registry_path, &lead_entry, 5).unwrap();

    let init = SessionStartInit {
        lead_pid: 99_999,
        home_dir: home.to_path_buf(),
        now: fixed_now,
    };
    let input =
        make_session_start_input(project_dir.to_str().unwrap(), "claude-second-teammate-uuid");
    let mut buf = Vec::new();
    let init_result = init.run(&input, project_dir, &mut buf).unwrap();

    assert_eq!(
        init_result.session_id, sid,
        "AC-07 Layer 2: second teammate reuses lead SID even with missing team file"
    );
    assert!(
        init_result.is_teammate,
        "AC-07 Layer 2: is_teammate=true even with missing team file"
    );

    let registry = worktree::read_registry(&registry_path).unwrap();
    let active = worktree::list_active(&registry);
    assert_eq!(
        active.len(),
        1,
        "AC-07: registry stays at 1 active worktree when Layer 2 fires"
    );
}
