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
        self.project_dir
            .join(".state")
            .join("session")
            .join(session_id)
            .join("pathflow")
            .join("pathflow-phase-tasks.json")
    }

    fn sentinel_dir(&self, session_id: &str) -> Result<PathBuf, HookError> {
        sentinel::resolve_dir(&self.project_dir, session_id)
            .map_err(|e| HookError::Config(format!("sentinel dir: {e}")))
    }
}

impl HookHandler for CheckpointComplete {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        // TaskCompleted events carry task info in tool_input.
        // Extract the subject to find PF{N}-TSK-{NN}.
        let subject = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("subject"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let task_id = match pf_task_id_re().find(subject) {
            Some(m) => m.as_str().to_string(),
            None => return Ok(HookOutput::Allow),
        };

        // Resolve session ID.
        let sid = session::current_session_id(&self.project_dir.join(".state"))
            .map_err(|e| HookError::Config(format!("session ID: {e}")))?;

        let checkpoint_path = self.checkpoint_path(sid.as_ref());
        let sentinel_dir = self.sentinel_dir(sid.as_ref())?;

        let cp = Checkpoint::new();
        match cp.complete_task(&checkpoint_path, &sentinel_dir, &task_id) {
            Ok(()) => Ok(HookOutput::Allow),
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
            tool_input: Some(serde_json::json!({"subject": subject})),
            event: HookEvent::TaskCompleted,
            session_id: Some("ses-test".into()),
            project_dir: Some("/tmp/test-project".into()),
            source: None,
            transcript_path: None,
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
        let input = HookInput {
            tool_name: None,
            tool_input: Some(serde_json::json!({"subject": ""})),
            event: HookEvent::TaskCompleted,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_ignores_no_tool_input() {
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
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_ignores_missing_subject_field() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: Some(serde_json::json!({"description": "some task"})),
            event: HookEvent::TaskCompleted,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_pf_task_warns_on_checkpoint_error() {
        // A PF task subject with no session env → session ID resolution fails
        // → returns warning (non-blocking).
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        let input = make_input("PF3-TSK-01: Classify work type");
        let result = handler.handle(input);
        // Session ID lookup will fail → HookError::Config.
        assert!(result.is_err());
    }

    #[test]
    fn test_pf_task_with_session_env_warns_no_checkpoint() {
        // Setup session env but no checkpoint file → complete_task returns error → Warn.
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(state_dir.join("runtime")).unwrap();
        let sid = "ses-1234567890abc";
        std::fs::write(
            state_dir.join("codeflow-env.sh"),
            format!("export CODEFLOW_SESSION_ID=\"{sid}\"\nexport CF_PROJECT_ROOT=\"test\"\n"),
        )
        .unwrap();

        let handler = CheckpointComplete::new(dir.path().to_path_buf());
        let input = make_input("PF3-TSK-01: Classify work type");
        let result = handler.handle(input).unwrap();
        // Checkpoint file doesn't exist → complete_task returns error → Warn.
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
}
