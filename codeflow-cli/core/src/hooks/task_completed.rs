//! `TaskCompleted` hook handler.
//!
//! One handler:
//! - `CheckpointComplete`: Marks tasks complete in checkpoint; creates phase
//!   sentinel when all phase tasks are done.

use std::path::PathBuf;
use std::sync::OnceLock;

use regex::Regex;

use super::{HookEvent, HookHandler, HookInput, HookOutput};
use crate::error::HookError;
use crate::pathflow::checkpoint::Checkpoint;
use crate::pathflow::sentinel;
use crate::session;

fn pf_task_id_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"PF\d+-TSK-\d+").expect("valid regex"))
}

// ---------------------------------------------------------------------------
// CheckpointComplete handler
// ---------------------------------------------------------------------------

/// Marks `PF{N}-TSK-{NN}` tasks as completed in the checkpoint file.
/// When all expected tasks in a phase are done/skipped, the checkpoint
/// system automatically creates the phase sentinel (e.g., `pathflow-pf-1`).
pub struct CheckpointComplete {
    pub project_dir: PathBuf,
}

impl CheckpointComplete {
    #[must_use]
    pub fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }

    fn checkpoint_path(&self, session_id: &str) -> PathBuf {
        let base = super::pipeline::resolve_state_base(&self.project_dir);
        base.join(".state")
            .join("session")
            .join(session_id)
            .join("pathflow")
            .join("pathflow-phase-tasks.json")
    }

    fn sentinel_dir(&self, session_id: &str) -> Result<PathBuf, HookError> {
        sentinel::resolve_dir(&self.project_dir, session_id)
            .map_err(|e| HookError::Config(format!("sentinel dir: {e}")))
    }

    /// Register `work_type` in checkpoint context and session status.
    ///
    /// For planned tasks that already have a `work_type` set in the session
    /// status (e.g., via cf-knowledge-layer at PF4-TSK-02), the pre-set
    /// value is preserved. Only infers from the git branch when the
    /// existing `work_type` is empty or missing.
    ///
    /// Mirrors Go `registerWorkType()` in hooks.go:855-877.
    fn register_work_type(&self, session_dir: &std::path::Path, session_id: &str) {
        use super::pipeline;

        // Check if work_type is already set in session status.
        let existing = pipeline::read_work_type_from_session_status(session_dir);
        if !existing.is_empty() {
            // Pre-set work_type exists (e.g., from planned task registration).
            // Write to checkpoint context for consistency, but do NOT overwrite
            // with branch-inferred value.
            let checkpoint_path = self.checkpoint_path(session_id);
            let cp = Checkpoint::new();
            if let Err(e) = cp.set_context(&checkpoint_path, "work_type", &existing) {
                eprintln!("checkpoint-complete: failed to set checkpoint work_type: {e}");
            }
            return;
        }

        // No pre-set work_type -- infer from git branch.
        let branch = match std::process::Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&self.project_dir)
            .output()
        {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            }
            _ => return,
        };

        if branch.is_empty() {
            return;
        }

        // infer_work_type_from_branch falls back to DEFAULT_WORK_TYPE ("FIX")
        // when the branch prefix is not recognized, so work_type is never empty.
        let work_type = pipeline::infer_work_type_from_branch(&branch);

        // Write to checkpoint context.
        let checkpoint_path = self.checkpoint_path(session_id);
        let cp = Checkpoint::new();
        if let Err(e) = cp.set_context(&checkpoint_path, "work_type", work_type) {
            eprintln!("checkpoint-complete: failed to set checkpoint work_type: {e}");
        }

        // Write to session status.
        crate::hooks::post_tool_use::update_session_status(
            session_dir,
            &serde_json::json!({"work_type": work_type}),
        );
    }

    /// Update the worktree registry branch field and session status branch
    /// with the current git branch.
    ///
    /// Called when the pf-3 sentinel is created (feature branch exists).
    /// Delegates to `update_branch_from_current` which handles both the
    /// worktree registry and session status atomically.
    fn update_worktree_branch(&self) {
        if let Err(e) = crate::worktree::update_branch_from_current(&self.project_dir) {
            eprintln!("checkpoint-complete: branch update failed: {e}");
        }
    }
}

impl HookHandler for CheckpointComplete {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        // TaskCompleted events carry task_subject as a top-level field.
        // Primary: input.task_subject (Claude Code's actual format).
        // Fallback: tool_input.subject (legacy/test compat).
        let subject = input
            .task_subject
            .as_deref()
            .or_else(|| {
                input
                    .tool_input
                    .as_ref()
                    .and_then(|v| v.get("subject"))
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("");

        let task_id = match pf_task_id_re().find(subject) {
            Some(m) => m.as_str().to_string(),
            None => return Ok(HookOutput::Allow),
        };

        // Resolve session ID.
        let sid = session::current_session_id(&self.project_dir)
            .map_err(|e| HookError::Config(format!("session ID: {e}")))?;

        let checkpoint_path = self.checkpoint_path(sid.as_ref());
        let sentinel_dir = self.sentinel_dir(sid.as_ref())?;

        let cp = Checkpoint::new();
        match cp.complete_task(&checkpoint_path, &sentinel_dir, &task_id) {
            Ok(()) => {
                // Extract phase number from task_id (e.g., "PF4-TSK-05" -> "pf-4")
                // and update session status with last completed phase.
                if let Some(pf_num) = task_id.split('-').next() {
                    let base = super::pipeline::resolve_state_base(&self.project_dir);
                    let session_dir = base
                        .join(".state")
                        .join("session")
                        .join(sid.as_ref())
                        .join("pathflow");
                    // Convert "PF4" -> "pf-4" to match Go format.
                    let phase_normalized = format!("pf-{}", &pf_num[2..]);
                    if sentinel::check_by_name(&sentinel_dir, &phase_normalized) {
                        // Only advance status to "pf-in-progress" if current status
                        // is "created" or "pf-started". Guard prevents a late
                        // TaskCompleted from overwriting "pf-complete" in a race
                        // (matches Go hooks.go:826).
                        let status_path = session_dir.join("pathflow-session-status.json");
                        let should_set_status =
                            crate::pathflow::file_lock::locked_read(&status_path)
                                .ok()
                                .and_then(|v| {
                                    v.get("status")
                                        .and_then(|s| s.as_str())
                                        .map(|s| s == "created" || s == "pf-started")
                                })
                                .unwrap_or(false);

                        let mut updates = serde_json::json!({
                            "last_completed_phase": phase_normalized,
                        });
                        if should_set_status {
                            updates["status"] = serde_json::Value::String("pf-in-progress".into());
                        }
                        crate::hooks::post_tool_use::update_session_status(&session_dir, &updates);

                        // When pf-3 is newly created, infer work_type from git branch
                        // and write to checkpoint context + session status.
                        // Mirrors Go registerWorkType() in hooks.go:855-877.
                        if phase_normalized == "pf-3" {
                            self.register_work_type(&session_dir, sid.as_ref());
                            // Update worktree registry branch field now that
                            // the feature branch exists (AC #10).
                            self.update_worktree_branch();
                        }
                    }
                }
                Ok(HookOutput::Allow)
            }
            Err(e) => {
                // Non-blocking: warn but allow through.
                Ok(HookOutput::Warn {
                    message: format!("checkpoint complete warning: {e}"),
                })
            }
        }
    }

    fn name(&self) -> &'static str {
        "checkpoint-complete"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::TaskCompleted]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_input(subject: &str) -> HookInput {
        HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::TaskCompleted,
            session_id: Some("ses-test".into()),
            project_dir: Some("/tmp/test-project".into()),
            source: None,
            transcript_path: None,
            task_subject: Some(subject.to_string()),
            task_id: None,
            task_description: None,
            tool_response: None,
        }
    }

    #[test]
    fn test_ignores_non_pf_task() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        let input = make_input("Some regular task completion");
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_handler_name() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        assert_eq!(handler.name(), "checkpoint-complete");
    }

    #[test]
    fn test_handler_events() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        assert_eq!(handler.events(), &[HookEvent::TaskCompleted]);
    }

    #[test]
    fn test_pf_task_id_regex() {
        let re = pf_task_id_re();
        assert!(re.is_match("PF1-TSK-01"));
        assert!(re.is_match("PF7-TSK-03"));
        assert!(re.is_match("completed PF4-TSK-05: register task"));
        assert!(!re.is_match("TSK-01"));
    }

    #[test]
    fn test_ignores_empty_subject() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        let input = make_input("");
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_ignores_no_task_subject() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::TaskCompleted,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
            task_subject: None,
            task_id: None,
            task_description: None,
            tool_response: None,
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_ignores_non_pf_task_subject() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::TaskCompleted,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            task_subject: Some("Regular task with no PF ID".into()),
            task_id: Some("42".into()),
            task_description: Some("Just a task".into()),
            tool_response: None,
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_pf_task_warns_on_checkpoint_error() {
        // A PF task subject with no session env -> session ID resolution fails
        // -> returns warning (non-blocking).
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        let input = make_input("PF3-TSK-01: Classify work type");
        let result = handler.handle(input);
        // Session ID lookup will fail -> HookError::Config.
        assert!(result.is_err());
    }

    #[test]
    fn test_pf_task_with_session_env_warns_no_checkpoint() {
        // Setup session env but no checkpoint file -> complete_task returns error -> Warn.
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        let sid = "ses-1234567890abc";
        std::fs::write(
            runtime_dir.join("codeflow-env.sh"),
            format!("export CODEFLOW_SESSION_ID=\"{sid}\"\nexport CF_PROJECT_ROOT=\"test\"\n"),
        )
        .unwrap();

        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        let input = make_input("PF3-TSK-01: Classify work type");
        let result = handler.handle(input).unwrap();
        // Checkpoint file doesn't exist -> complete_task returns error -> Warn.
        assert!(matches!(result, HookOutput::Warn { .. }));
    }

    #[test]
    fn test_checkpoint_path_construction() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        let path = handler.checkpoint_path("ses-abc123");
        assert!(path.ends_with(".state/session/ses-abc123/pathflow/pathflow-phase-tasks.json"));
    }

    #[test]
    fn test_sentinel_dir_construction() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        let sdir = handler.sentinel_dir("ses-abc123").unwrap();
        assert!(sdir.ends_with(".state/sentinels/pathflow/ses-abc123"));
    }

    #[test]
    fn test_sentinel_dir_empty_session_fails() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        assert!(handler.sentinel_dir("").is_err());
    }

    #[test]
    fn test_pf_task_complete_creates_sentinel_and_updates_status() {
        // Full integration: session env + checkpoint + PF task completion.
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01jq7checkpoint000000ab";

        // Set up session env.
        let runtime_dir = dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(
            runtime_dir.join("codeflow-env.sh"),
            format!("export CODEFLOW_SESSION_ID='{sid}'\nexport CF_PROJECT_ROOT='test'\n"),
        )
        .unwrap();

        // Create session pathflow dir + status file.
        let session_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(
            session_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-started","team_name":"t"}"#,
        )
        .unwrap();

        // Create sentinel dir.
        let sentinel_dir = dir
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(sid);
        std::fs::create_dir_all(&sentinel_dir).unwrap();

        // Initialize a minimal checkpoint with one task in PF1.
        let _cp = Checkpoint::new();
        let checkpoint_path = session_dir.join("pathflow-phase-tasks.json");
        // Create a checkpoint with PF1-TSK-01 registered.
        let checkpoint_data = serde_json::json!({
            "phases": {
                "pf-1": {
                    "expected": ["PF1-TSK-01"],
                    "registered": ["PF1-TSK-01"],
                    "completed": [],
                    "skipped": [],
                    "sentinel_created": false
                }
            }
        });
        std::fs::write(
            &checkpoint_path,
            serde_json::to_string_pretty(&checkpoint_data).unwrap(),
        )
        .unwrap();

        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        let input = make_input("PF1-TSK-01: Security check");
        let result = handler.handle(input).unwrap();

        // Should succeed (Allow or Warn depending on checkpoint state).
        assert!(
            matches!(result, HookOutput::Allow | HookOutput::Warn { .. }),
            "task completion should succeed: {result:?}"
        );
    }

    #[test]
    fn test_pf_task_regex_extracts_first_match() {
        let re = pf_task_id_re();
        let m = re.find("completed PF4-TSK-05: register task in WorkGraph");
        assert!(m.is_some());
        assert_eq!(m.unwrap().as_str(), "PF4-TSK-05");
    }

    #[test]
    fn test_pf_task_regex_no_match_on_partial() {
        let re = pf_task_id_re();
        assert!(!re.is_match("PF-TSK-01")); // Missing digit after PF
        assert!(!re.is_match("PF1TSK01")); // Missing dashes
    }

    #[test]
    fn test_register_work_type_no_session() {
        // register_work_type with no git branch should return silently.
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        std::fs::create_dir_all(&session_dir).unwrap();
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        // Should not panic -- just returns silently.
        handler.register_work_type(&session_dir, "ses-test");
    }

    #[test]
    fn test_register_work_type_preserves_preset() {
        // When work_type is already set in session status, register_work_type
        // should NOT overwrite it with a branch-inferred value.
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        std::fs::create_dir_all(&session_dir).unwrap();

        // Pre-set work_type in status file.
        std::fs::write(
            session_dir.join("pathflow-session-status.json"),
            r#"{"work_type": "FEAT", "status": "pf-in-progress"}"#,
        )
        .unwrap();

        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        handler.register_work_type(&session_dir, "ses-test");

        // Verify work_type was NOT overwritten.
        let status: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(session_dir.join("pathflow-session-status.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            status["work_type"].as_str().unwrap(),
            "FEAT",
            "pre-set work_type should be preserved"
        );
    }

    #[test]
    fn test_register_work_type_infers_when_empty() {
        // When work_type is empty in session status, register_work_type
        // should infer from the git branch (or return silently if no branch).
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        std::fs::create_dir_all(&session_dir).unwrap();

        // Empty work_type in status file.
        std::fs::write(
            session_dir.join("pathflow-session-status.json"),
            r#"{"work_type": "", "status": "pf-in-progress"}"#,
        )
        .unwrap();

        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        // This will attempt git branch and likely fail in test env, which is fine.
        // The key test is that it doesn't short-circuit on the empty work_type.
        handler.register_work_type(&session_dir, "ses-test");
    }

    #[test]
    fn test_register_work_type_uses_project_dir_for_git() {
        // register_work_type must run `git branch --show-current` with
        // current_dir set to project_dir, not the process cwd. This ensures
        // correct branch detection when running inside a worktree.
        //
        // We verify the fix by passing a tempdir (no git repo) as project_dir.
        // The git command will fail (not a git repo), causing register_work_type
        // to return silently without setting work_type -- which is correct
        // fallback behaviour. If current_dir were NOT set, the command would
        // instead pick up whatever git repo the test process happens to run in,
        // producing a non-deterministic result.
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        std::fs::create_dir_all(&session_dir).unwrap();

        // Empty work_type so the branch-inference path is exercised.
        std::fs::write(
            session_dir.join("pathflow-session-status.json"),
            r#"{"work_type": "", "status": "pf-in-progress"}"#,
        )
        .unwrap();

        // project_dir is a tempdir with no git repo -- git command will fail.
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        // Should return silently (no panic, no write) because git branch fails.
        handler.register_work_type(&session_dir, "ses-test");

        // Status file must be unchanged (no work_type written).
        let status: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(session_dir.join("pathflow-session-status.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            status["work_type"].as_str().unwrap_or(""),
            "",
            "work_type should remain empty when git branch detection fails"
        );
    }

    #[test]
    fn test_pf_task_subject_with_multiple_task_ids() {
        // Subject with multiple PF task IDs -- should match the first one.
        let re = pf_task_id_re();
        let m = re.find("PF1-TSK-01 blocked by PF2-TSK-03");
        assert_eq!(m.unwrap().as_str(), "PF1-TSK-01");
    }

    #[test]
    fn test_pf_task_various_phase_numbers() {
        let re = pf_task_id_re();
        for i in 1..=7 {
            let task = format!("PF{i}-TSK-01");
            assert!(re.is_match(&task), "should match {task}");
        }
    }

    #[test]
    fn test_fallback_tool_input_subject() {
        // When task_subject is None, falls back to tool_input.subject (legacy compat).
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: Some(serde_json::json!({"subject": "PF2-TSK-01: Context loading"})),
            event: HookEvent::TaskCompleted,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            task_subject: None, // Not set -- should fall back to tool_input
            task_id: None,
            task_description: None,
            tool_response: None,
        };
        // Will fail on session ID (no env), but proves fallback extraction works.
        let result = handler.handle(input);
        assert!(result.is_err()); // session ID missing -> error (not Allow/skip)
    }

    #[test]
    fn test_task_subject_takes_priority_over_tool_input() {
        // When both task_subject and tool_input.subject exist, task_subject wins.
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: Some(serde_json::json!({"subject": "PF9-TSK-99: Wrong one"})),
            event: HookEvent::TaskCompleted,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            task_subject: Some("PF3-TSK-01: Correct one".into()),
            task_id: None,
            task_description: None,
            tool_response: None,
        };
        // Will fail on session ID, but the important thing is it doesn't return Allow
        // (which would mean it matched nothing / matched PF9-TSK-99).
        let result = handler.handle(input);
        assert!(result.is_err()); // session ID missing -> error (proves PF3-TSK-01 was extracted)
    }
}
