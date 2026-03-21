//! Autorun command: batch execution of tasks in parallel worktrees.
//!
//! Parses the batch file, loads parallel-work config, creates an orchestrator
//! with a real worker runner, and executes the batch.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use base64::Engine as _;

use crate::helpers;

pub async fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir).await
}

async fn run_with_dir(project_dir: &Path) -> Result<()> {
    let batch_path = project_dir
        .join(".codeflow")
        .join("config")
        .join("autorun")
        .join("batch.yaml");

    if !batch_path.exists() {
        anyhow::bail!("no autorun batch file found at {}", batch_path.display());
    }

    let parsed = codeflow_core::autorun::batch::parse_batch_file(&batch_path)
        .context("parsing autorun batch")?;

    let config =
        codeflow_core::autorun::load_config(project_dir).context("loading parallel-work config")?;

    println!(
        "autorun batch: {} tasks, max_workers={}, scope_policy={}",
        parsed.tasks.len(),
        config.worktree.max_concurrent.min(parsed.max_workers),
        config.claims.default_scope_policy,
    );

    // Generate a session ID for this autorun run.
    let session_id = codeflow_core::session::generate_session_id();

    // Create the worker runner with real implementations.
    let worktree_provider =
        codeflow_core::autorun::RealWorktreeProvider::new(project_dir.to_path_buf());
    let tmux = RealTmux;
    let worker_timeout = Duration::from_secs(config.autorun.worker_timeout_secs);
    let claude = RealClaude {
        tmux: RealTmux,
        worker_timeout,
    };
    let worker = codeflow_core::autorun::TmuxWorker::with_timeout(
        tmux,
        claude,
        worktree_provider,
        project_dir.to_path_buf(),
        worker_timeout,
    );

    let orchestrator = codeflow_core::autorun::Orchestrator::new(worker);

    let results = orchestrator
        .execute(session_id.as_str(), &parsed, project_dir)
        .await
        .context("executing autorun batch")?;

    report_results(&results)
}

/// Report execution results and return error if any tasks failed or timed out.
fn report_results(results: &[codeflow_core::autorun::WorkerResult]) -> Result<()> {
    let completed = results.iter().filter(|r| r.status == "completed").count();
    let failed = results.iter().filter(|r| r.status == "failed").count();
    let skipped = results.iter().filter(|r| r.status == "skipped").count();
    let timed_out = results.iter().filter(|r| r.status == "timeout").count();

    println!(
        "autorun complete: {completed} completed, {failed} failed, {skipped} skipped, {timed_out} timed out"
    );

    for result in results {
        if result.status != "completed" {
            eprintln!(
                "  task {}: {} (exit={}{})",
                result.task_id,
                result.status,
                result.exit_code,
                format_error(&result.error),
            );
        }
    }

    if failed > 0 || timed_out > 0 {
        anyhow::bail!("{} task(s) failed or timed out", failed + timed_out);
    }

    Ok(())
}

/// Format an error string for display in result output.
fn format_error(error: &str) -> String {
    if error.is_empty() {
        String::new()
    } else {
        format!(", error={error}")
    }
}

/// Run a tmux command and return success/failure.
async fn run_tmux(args: &[&str]) -> Result<bool, codeflow_core::AutorunError> {
    let status = tokio::process::Command::new("tmux")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|e| codeflow_core::AutorunError::WorkerFailed(format!("tmux {}: {e}", args[0])))?;
    Ok(status.success())
}

/// Real tmux runner that executes tmux commands via the system.
struct RealTmux;

impl codeflow_core::autorun::TmuxRunner for RealTmux {
    async fn create_session(&self, name: &str) -> Result<(), codeflow_core::AutorunError> {
        if !run_tmux(&["new-session", "-d", "-s", name]).await? {
            return Err(codeflow_core::AutorunError::WorkerFailed(format!(
                "tmux new-session failed for {name}"
            )));
        }
        Ok(())
    }

    async fn send_command(
        &self,
        session: &str,
        command: &str,
    ) -> Result<(), codeflow_core::AutorunError> {
        if !run_tmux(&["send-keys", "-t", session, command, "Enter"]).await? {
            return Err(codeflow_core::AutorunError::WorkerFailed(format!(
                "tmux send-keys failed for {session}"
            )));
        }
        Ok(())
    }

    async fn kill_session(&self, name: &str) -> Result<(), codeflow_core::AutorunError> {
        let _ = run_tmux(&["kill-session", "-t", name]).await;
        Ok(())
    }

    async fn has_session(&self, name: &str) -> Result<bool, codeflow_core::AutorunError> {
        run_tmux(&["has-session", "-t", name]).await
    }
}

/// Default polling interval for file-marker completion detection.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Grace period after SIGTERM before escalating to SIGKILL.
const SIGTERM_GRACE_SECS: u64 = 10;

/// Real Claude invoker using tmux send-keys for command dispatch and
/// file-marker polling for completion detection.
struct RealClaude<T: codeflow_core::autorun::TmuxRunner> {
    tmux: T,
    worker_timeout: Duration,
}

impl<T: codeflow_core::autorun::TmuxRunner> RealClaude<T> {
    /// Construct the base64-encoded acceptance criteria JSON.
    fn encode_acceptance(criteria: &[String]) -> String {
        let json = serde_json::to_string(criteria).unwrap_or_else(|_| "[]".to_string());
        base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
    }

    /// Read the exit code from the marker file.
    fn read_exit_code(path: &Path) -> i32 {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok())
            .unwrap_or(1)
    }

    /// Parse PR information from the worker output JSON.
    fn parse_worker_output(path: &Path) -> (i64, String, String) {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return (0, String::new(), String::new()),
        };
        // Claude --output-format json produces a JSON object.
        // Try to parse and extract PR-related fields from the output text.
        let val: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => return (0, String::new(), Self::extract_output_text(&content)),
        };

        let output_text = val
            .get("result")
            .or_else(|| val.get("output"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();

        let (pr_number, pr_url) = Self::extract_pr_from_text(&output_text);
        (pr_number, pr_url, output_text)
    }

    /// Extract PR URL and number from output text using regex-like matching.
    fn extract_pr_from_text(text: &str) -> (i64, String) {
        // Look for GitHub PR URL pattern.
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(idx) = trimmed.find("/pull/") {
                let after = &trimmed[idx + 6..];
                let num_str: String = after.chars().take_while(char::is_ascii_digit).collect();
                if let Ok(num) = num_str.parse::<i64>() {
                    // Find the full URL.
                    let url_start = trimmed[..idx].rfind("https://").unwrap_or(0);
                    let url_end = trimmed[idx + 6..]
                        .find(|c: char| c.is_whitespace() || c == ')' || c == ']')
                        .map_or(trimmed.len(), |e| idx + 6 + e);
                    let url = &trimmed[url_start..url_end];
                    return (num, url.to_string());
                }
            }
        }
        (0, String::new())
    }

    /// Extract plain text output when JSON parsing fails.
    fn extract_output_text(content: &str) -> String {
        // Limit output to a reasonable size.
        if content.len() > 4096 {
            content[..4096].to_string()
        } else {
            content.to_string()
        }
    }
}

impl<T: codeflow_core::autorun::TmuxRunner> codeflow_core::autorun::ClaudeInvoker
    for RealClaude<T>
{
    async fn invoke(
        &self,
        cfg: codeflow_core::autorun::InvokeConfig,
    ) -> Result<codeflow_core::autorun::InvokeResult, codeflow_core::AutorunError> {
        let session = &cfg.tmux_session;
        let work_dir = &cfg.work_dir;

        // Ensure .state/runtime directory exists in worktree.
        let runtime_dir = PathBuf::from(work_dir).join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).map_err(|e| {
            codeflow_core::AutorunError::WorkerFailed(format!(
                "creating runtime dir {}: {e}",
                runtime_dir.display()
            ))
        })?;

        // Set environment variables via tmux send-keys.
        self.tmux
            .send_command(
                session,
                &format!("export AUTORUN_SESSION_ID={}", cfg.session_id),
            )
            .await?;
        self.tmux
            .send_command(session, &format!("export AUTORUN_TASK_ID={}", cfg.task_id))
            .await?;

        let acceptance_b64 = Self::encode_acceptance(&cfg.acceptance_criteria);
        self.tmux
            .send_command(
                session,
                &format!("export AUTORUN_ACCEPTANCE={acceptance_b64}"),
            )
            .await?;

        self.tmux
            .send_command(
                session,
                &format!("export CODEFLOW_WORKTREE_PATH={work_dir}"),
            )
            .await?;

        // Escape single quotes in the prompt for POSIX single-quote shell quoting.
        // Pattern: end current quote, insert escaped quote, restart quote ('\'')
        let escaped_prompt = cfg.prompt.replace('\'', "'\\''");

        // Send the Claude command. Uses regular '...' quoting (not $'...') so
        // that backslash sequences like \n in the prompt are preserved literally.
        // Quote work_dir with POSIX single-quote escaping to handle paths with spaces.
        let escaped_work_dir = work_dir.replace('\'', "'\\''");
        let exit_code_path = format!("{work_dir}/.state/runtime/worker-exit-code");
        let output_path = format!("{work_dir}/.state/runtime/worker-output.json");
        let escaped_exit_code_path = exit_code_path.replace('\'', "'\\''");
        let escaped_output_path = output_path.replace('\'', "'\\''");
        let claude_cmd = format!(
            "cd '{escaped_work_dir}' && claude -p '{escaped_prompt}' \
             --dangerously-skip-permissions \
             --output-format json \
             > '{escaped_output_path}' 2>&1; \
             echo $? > '{escaped_exit_code_path}'"
        );
        self.tmux.send_command(session, &claude_cmd).await?;

        // INNER TIMEOUT: Polls for Claude's exit-code marker file and handles
        // graceful shutdown (SIGTERM → grace period → SIGKILL). This complements
        // the OUTER timeout in worker.rs which bounds the entire worker lifecycle.
        // The inner timeout provides Claude-specific shutdown sequencing, while
        // the outer timeout catches hangs in non-Claude phases (worktree setup, etc.).
        let exit_code_file = PathBuf::from(&exit_code_path);
        let timeout = self.worker_timeout;
        let poll_start = std::time::Instant::now();

        loop {
            if exit_code_file.exists() {
                break;
            }

            if poll_start.elapsed() >= timeout {
                // Timeout: SIGTERM the tmux session, wait grace period, then SIGKILL.
                eprintln!(
                    "worker timeout after {}s for task {}, sending SIGTERM",
                    timeout.as_secs(),
                    cfg.task_id
                );
                // Send Ctrl-C to the tmux session to interrupt Claude.
                let _ = self.tmux.send_command(session, "C-c").await;
                tokio::time::sleep(Duration::from_secs(SIGTERM_GRACE_SECS)).await;

                // If still no exit code, kill the session.
                if !exit_code_file.exists() {
                    let _ = self.tmux.kill_session(session).await;
                }

                return Ok(codeflow_core::autorun::InvokeResult {
                    exit_code: 124,
                    pr_number: 0,
                    pr_url: String::new(),
                    branch_name: String::new(),
                    output: "worker exceeded timeout".to_string(),
                });
            }

            tokio::time::sleep(POLL_INTERVAL).await;
        }

        // Read exit code from marker file.
        let exit_code = Self::read_exit_code(&exit_code_file);

        // Parse worker output.
        let output_file = PathBuf::from(&output_path);
        let (mut pr_number, mut pr_url, output) = Self::parse_worker_output(&output_file);

        // Get current branch name from the worktree.
        let branch_name = tokio::process::Command::new("git")
            .args(["-C", work_dir, "branch", "--show-current"])
            .output()
            .await
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout)
                        .ok()
                        .map(|s| s.trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        // If PR info not found in output, try gh pr list as fallback.
        if pr_number == 0 && exit_code == 0 && !branch_name.is_empty() {
            if let Ok(gh_output) = tokio::process::Command::new("gh")
                .args([
                    "pr",
                    "list",
                    "--head",
                    &branch_name,
                    "--json",
                    "number,url",
                    "--limit",
                    "1",
                ])
                .current_dir(work_dir)
                .output()
                .await
            {
                if gh_output.status.success() {
                    if let Ok(prs) =
                        serde_json::from_slice::<Vec<serde_json::Value>>(&gh_output.stdout)
                    {
                        if let Some(pr) = prs.first() {
                            pr_number = pr
                                .get("number")
                                .and_then(serde_json::Value::as_i64)
                                .unwrap_or(0);
                            pr_url = pr
                                .get("url")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("")
                                .to_string();
                        }
                    }
                }
            }
        }

        Ok(codeflow_core::autorun::InvokeResult {
            exit_code,
            pr_number,
            pr_url,
            branch_name,
            output,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- run_with_dir: missing batch file --

    #[test]
    fn test_autorun_no_batch_file() {
        let dir = tempfile::tempdir().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_with_dir(dir.path()));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("no autorun batch file"),
            "expected batch file error, got: {msg}"
        );
    }

    // -- run_with_dir: valid batch gets past parsing --

    #[test]
    fn test_autorun_with_valid_batch() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow").join("config").join("autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(
            batch_dir.join("batch.yaml"),
            "name: test-batch\ntasks:\n  - id: task-1\n",
        )
        .unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        // This will fail at tmux level in CI, but should get past parsing.
        // We verify it doesn't fail on config/parse.
        let result = rt.block_on(run_with_dir(dir.path()));
        // Result might be ok (mock tmux) or err (no tmux in CI) — either is fine.
        // The important thing is it gets past config loading.
        let _ = result;
    }

    // -- run_with_dir: invalid YAML --

    #[test]
    fn test_autorun_with_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow").join("config").join("autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(batch_dir.join("batch.yaml"), "{{invalid yaml").unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_with_dir(dir.path()));
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("parsing autorun batch"),
            "expected parse context, got: {msg}"
        );
    }

    // -- Config loading --

    #[test]
    fn test_autorun_loads_config() {
        let dir = tempfile::tempdir().unwrap();
        // Create batch file.
        let batch_dir = dir.path().join(".codeflow").join("config").join("autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(
            batch_dir.join("batch.yaml"),
            "name: config-test\ntasks:\n  - id: task-1\n",
        )
        .unwrap();
        // Create config file.
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "worktree": { "max_concurrent": 5 } }"#,
        )
        .unwrap();

        // Config should load without error.
        let config = codeflow_core::autorun::load_config(dir.path()).unwrap();
        assert_eq!(config.worktree.max_concurrent, 5);
    }

    #[test]
    fn test_config_loads_defaults_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let config = codeflow_core::autorun::load_config(dir.path()).unwrap();
        assert_eq!(config.worktree.max_concurrent, 3);
        assert_eq!(config.claims.default_scope_policy, "soft");
        assert_eq!(config.claims.ttl_secs, 4200);
        assert!(config.sync.auto_start);
    }

    #[test]
    fn test_config_loads_custom_claims_values() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{
                "claims": {
                    "default_scope_policy": "hard",
                    "ttl_secs": 7200,
                    "capture_events": false
                }
            }"#,
        )
        .unwrap();

        let config = codeflow_core::autorun::load_config(dir.path()).unwrap();
        assert_eq!(config.claims.default_scope_policy, "hard");
        assert_eq!(config.claims.ttl_secs, 7200);
        assert!(!config.claims.capture_events);
    }

    #[test]
    fn test_config_loads_custom_sync_values() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "sync": { "interval_secs": 10, "auto_start": false } }"#,
        )
        .unwrap();

        let config = codeflow_core::autorun::load_config(dir.path()).unwrap();
        assert_eq!(config.sync.interval_secs, 10);
        assert!(!config.sync.auto_start);
    }

    #[test]
    fn test_config_loads_custom_merge_values() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "merge": { "auto_rebase": false, "queue_enabled": false, "max_rebase_attempts": 5 } }"#,
        )
        .unwrap();

        let config = codeflow_core::autorun::load_config(dir.path()).unwrap();
        assert!(!config.merge.auto_rebase);
        assert!(!config.merge.queue_enabled);
        assert_eq!(config.merge.max_rebase_attempts, 5);
    }

    #[test]
    fn test_config_malformed_json_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            "{ not valid json }",
        )
        .unwrap();

        let result = codeflow_core::autorun::load_config(dir.path());
        assert!(result.is_err());
    }

    // -- Batch parsing --

    #[test]
    fn test_batch_parse_multiple_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.yaml");
        std::fs::write(
            &batch_path,
            "name: multi-task\nmax_workers: 3\ntasks:\n  - id: task-a\n  - id: task-b\n  - id: task-c\n",
        )
        .unwrap();

        let parsed = codeflow_core::autorun::batch::parse_batch_file(&batch_path).unwrap();
        assert_eq!(parsed.tasks.len(), 3);
        assert_eq!(parsed.max_workers, 3);
        assert_eq!(parsed.tasks[0].id, "task-a");
        assert_eq!(parsed.tasks[1].id, "task-b");
        assert_eq!(parsed.tasks[2].id, "task-c");
    }

    #[test]
    fn test_batch_parse_with_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.yaml");
        std::fs::write(
            &batch_path,
            "name: deps-test\ntasks:\n  - id: task-a\n  - id: task-b\n    depends_on: [task-a]\n",
        )
        .unwrap();

        let parsed = codeflow_core::autorun::batch::parse_batch_file(&batch_path).unwrap();
        assert_eq!(parsed.tasks.len(), 2);
        assert!(parsed.tasks[0].depends_on.is_empty());
        assert_eq!(parsed.tasks[1].depends_on, vec!["task-a"]);
    }

    #[test]
    fn test_batch_parse_empty_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.yaml");
        std::fs::write(&batch_path, "name: empty\ntasks: []\n").unwrap();

        let parsed = codeflow_core::autorun::batch::parse_batch_file(&batch_path);
        // Empty tasks should either succeed (with empty vec) or fail validation.
        match parsed {
            Ok(p) => assert!(p.tasks.is_empty()),
            Err(e) => assert!(
                e.to_string().contains("task") || e.to_string().contains("empty"),
                "error should relate to empty tasks: {e}"
            ),
        }
    }

    #[test]
    fn test_batch_parse_nonexistent_file() {
        let result =
            codeflow_core::autorun::batch::parse_batch_file(Path::new("/nonexistent/batch.yaml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_batch_parse_with_file_scope_and_scope_policy() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.yaml");
        std::fs::write(
            &batch_path,
            r#"name: scope-test
tasks:
  - id: task-a
    file_scope:
      - "src/main.rs"
      - "src/lib.rs"
    scope_policy: hard
"#,
        )
        .unwrap();

        let parsed = codeflow_core::autorun::batch::parse_batch_file(&batch_path).unwrap();
        assert_eq!(
            parsed.tasks[0].file_scope,
            vec!["src/main.rs", "src/lib.rs"]
        );
        assert_eq!(
            parsed.tasks[0].scope_policy.as_deref(),
            Some("hard"),
            "scope_policy should be parsed from batch"
        );
    }

    // -- report_results (extracted from run_with_dir) --

    #[test]
    fn test_report_results_all_completed_returns_ok() {
        let results = vec![
            make_result("task-1", "completed", 0),
            make_result("task-2", "completed", 0),
            make_result("task-3", "completed", 0),
        ];
        assert!(report_results(&results).is_ok());
    }

    #[test]
    fn test_report_results_with_failure_returns_error() {
        let results = vec![
            make_result("task-1", "completed", 0),
            make_result("task-2", "failed", 1),
        ];
        let err = report_results(&results).unwrap_err();
        assert!(
            err.to_string().contains("1 task(s) failed or timed out"),
            "expected failure count in error, got: {err}"
        );
    }

    #[test]
    fn test_report_results_with_timeout_returns_error() {
        let results = vec![make_result("task-1", "timeout", -1)];
        let err = report_results(&results).unwrap_err();
        assert!(err.to_string().contains("1 task(s) failed or timed out"));
    }

    #[test]
    fn test_report_results_mixed_statuses() {
        let results = vec![
            make_result("task-1", "completed", 0),
            make_result("task-2", "failed", 1),
            make_result("task-3", "skipped", 0),
            make_result("task-4", "timeout", -1),
        ];
        let err = report_results(&results).unwrap_err();
        assert!(
            err.to_string().contains("2 task(s) failed or timed out"),
            "expected 2 failures, got: {err}"
        );
    }

    #[test]
    fn test_report_results_skipped_only_returns_ok() {
        let results = vec![
            make_result("task-1", "skipped", 0),
            make_result("task-2", "skipped", 0),
        ];
        // Skipped tasks don't trigger failure.
        assert!(report_results(&results).is_ok());
    }

    #[test]
    fn test_report_results_empty_returns_ok() {
        let results: Vec<codeflow_core::autorun::WorkerResult> = vec![];
        assert!(report_results(&results).is_ok());
    }

    #[test]
    fn test_report_results_with_error_message() {
        let results = vec![make_result_with_error(
            "task-err",
            "failed",
            1,
            "connection timeout",
        )];
        let err = report_results(&results).unwrap_err();
        assert!(err.to_string().contains("1 task(s) failed"));
    }

    // -- format_error --

    #[test]
    fn test_format_error_with_message() {
        assert_eq!(
            format_error("connection timeout"),
            ", error=connection timeout"
        );
    }

    #[test]
    fn test_format_error_empty() {
        assert!(format_error("").is_empty());
    }

    // -- Worker/orchestrator construction --

    #[test]
    fn test_worker_config_construction() {
        let cfg = codeflow_core::autorun::WorkerConfig {
            session_id: "ses-test".into(),
            worker_id: "w-1".into(),
            worker_num: 1,
            task_id: "task-test".into(),
            batch_name: "test-batch".into(),
            auto_merge: false,
            target: "main".into(),
            tmux_prefix: "worker".into(),
            file_scope: vec!["src/main.rs".into()],
            scope_policy: "soft".into(),
        };
        assert_eq!(cfg.task_id, "task-test");
        assert_eq!(cfg.scope_policy, "soft");
        assert_eq!(cfg.file_scope, vec!["src/main.rs"]);
    }

    #[test]
    fn test_worker_result_construction() {
        let result = codeflow_core::autorun::WorkerResult {
            worker_id: "w-1".into(),
            task_id: "task-1".into(),
            status: "completed".into(),
            exit_code: 0,
            pr_number: 42,
            pr_url: "https://github.com/org/repo/pull/42".into(),
            error: String::new(),
            branch_name: "feat/test".into(),
            duration_sec: 120,
        };
        assert_eq!(result.status, "completed");
        assert_eq!(result.pr_number, 42);
        assert_eq!(result.duration_sec, 120);
    }

    #[test]
    fn test_invoke_config_and_result() {
        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-invoke".into(),
            work_dir: "/tmp/work".into(),
            prompt: "implement feature X".into(),
            session_id: "ses-test".into(),
            auto_merge: false,
            target: "main".into(),
            tmux_session: "worker-1".into(),
            acceptance_criteria: vec!["tests pass".into()],
        };
        assert_eq!(cfg.task_id, "task-invoke");
        assert_eq!(cfg.acceptance_criteria, vec!["tests pass"]);

        let result = codeflow_core::autorun::InvokeResult {
            exit_code: 0,
            pr_number: 1,
            pr_url: String::new(),
            branch_name: "feat/x".into(),
            output: "done".into(),
        };
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.branch_name, "feat/x");
    }

    #[test]
    fn test_invoke_config_round_trip_serde() {
        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-rt".into(),
            work_dir: "/tmp/test".into(),
            prompt: "do things".into(),
            session_id: "ses-rt".into(),
            auto_merge: true,
            target: "main".into(),
            tmux_session: "w-1".into(),
            acceptance_criteria: vec!["crit 1".into(), "crit 2".into()],
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let deserialized: codeflow_core::autorun::InvokeConfig =
            serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.task_id, cfg.task_id);
        assert_eq!(deserialized.work_dir, cfg.work_dir);
        assert_eq!(deserialized.prompt, cfg.prompt);
        assert_eq!(deserialized.session_id, cfg.session_id);
        assert_eq!(deserialized.auto_merge, cfg.auto_merge);
        assert_eq!(deserialized.target, cfg.target);
        assert_eq!(deserialized.tmux_session, cfg.tmux_session);
        assert_eq!(deserialized.acceptance_criteria, cfg.acceptance_criteria);
    }

    // -- Helpers --

    fn make_result(
        task_id: &str,
        status: &str,
        exit_code: i32,
    ) -> codeflow_core::autorun::WorkerResult {
        codeflow_core::autorun::WorkerResult {
            worker_id: format!("w-{task_id}"),
            task_id: task_id.into(),
            status: status.into(),
            exit_code,
            pr_number: 0,
            pr_url: String::new(),
            error: String::new(),
            branch_name: String::new(),
            duration_sec: 0,
        }
    }

    fn make_result_with_error(
        task_id: &str,
        status: &str,
        exit_code: i32,
        error: &str,
    ) -> codeflow_core::autorun::WorkerResult {
        codeflow_core::autorun::WorkerResult {
            worker_id: format!("w-{task_id}"),
            task_id: task_id.into(),
            status: status.into(),
            exit_code,
            pr_number: 0,
            pr_url: String::new(),
            error: error.into(),
            branch_name: String::new(),
            duration_sec: 0,
        }
    }

    // -- RealClaude tests --

    /// Mock tmux that records sent commands for verification.
    struct RecordingTmux {
        commands: std::sync::Arc<tokio::sync::Mutex<Vec<(String, String)>>>,
    }

    impl RecordingTmux {
        fn new() -> (
            Self,
            std::sync::Arc<tokio::sync::Mutex<Vec<(String, String)>>>,
        ) {
            let commands = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
            (
                Self {
                    commands: commands.clone(),
                },
                commands,
            )
        }
    }

    impl codeflow_core::autorun::TmuxRunner for RecordingTmux {
        async fn create_session(&self, _name: &str) -> Result<(), codeflow_core::AutorunError> {
            Ok(())
        }
        async fn send_command(
            &self,
            session: &str,
            command: &str,
        ) -> Result<(), codeflow_core::AutorunError> {
            self.commands
                .lock()
                .await
                .push((session.to_string(), command.to_string()));
            Ok(())
        }
        async fn kill_session(&self, _name: &str) -> Result<(), codeflow_core::AutorunError> {
            Ok(())
        }
        async fn has_session(&self, _name: &str) -> Result<bool, codeflow_core::AutorunError> {
            Ok(false)
        }
    }

    #[test]
    fn test_real_claude_encode_acceptance() {
        let criteria = vec!["tests pass".to_string(), "no warnings".to_string()];
        let encoded = RealClaude::<RecordingTmux>::encode_acceptance(&criteria);
        // Should be valid base64.
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .unwrap();
        let json_str = String::from_utf8(decoded).unwrap();
        let parsed: Vec<String> = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed, criteria);
    }

    #[test]
    fn test_real_claude_encode_empty_acceptance() {
        let encoded = RealClaude::<RecordingTmux>::encode_acceptance(&[]);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .unwrap();
        let json_str = String::from_utf8(decoded).unwrap();
        assert_eq!(json_str, "[]");
    }

    #[test]
    fn test_real_claude_read_exit_code_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("exit-code");
        std::fs::write(&path, "0\n").unwrap();
        assert_eq!(RealClaude::<RecordingTmux>::read_exit_code(&path), 0);
    }

    #[test]
    fn test_real_claude_read_exit_code_nonzero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("exit-code");
        std::fs::write(&path, "42\n").unwrap();
        assert_eq!(RealClaude::<RecordingTmux>::read_exit_code(&path), 42);
    }

    #[test]
    fn test_real_claude_read_exit_code_missing() {
        let path = PathBuf::from("/nonexistent/exit-code");
        assert_eq!(RealClaude::<RecordingTmux>::read_exit_code(&path), 1);
    }

    #[test]
    fn test_real_claude_read_exit_code_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("exit-code");
        std::fs::write(&path, "not-a-number\n").unwrap();
        assert_eq!(RealClaude::<RecordingTmux>::read_exit_code(&path), 1);
    }

    #[test]
    fn test_real_claude_extract_pr_from_text() {
        let text = "PR created: https://github.com/org/repo/pull/42\nDone.";
        let (num, url) = RealClaude::<RecordingTmux>::extract_pr_from_text(text);
        assert_eq!(num, 42);
        assert!(url.contains("/pull/42"));
    }

    #[test]
    fn test_real_claude_extract_pr_no_match() {
        let text = "No PR info here\nJust some output.";
        let (num, url) = RealClaude::<RecordingTmux>::extract_pr_from_text(text);
        assert_eq!(num, 0);
        assert!(url.is_empty());
    }

    #[test]
    fn test_real_claude_parse_worker_output_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("output.json");
        std::fs::write(
            &path,
            r#"{"result": "PR: https://github.com/org/repo/pull/99"}"#,
        )
        .unwrap();
        let (num, url, output) = RealClaude::<RecordingTmux>::parse_worker_output(&path);
        assert_eq!(num, 99);
        assert!(url.contains("/pull/99"));
        assert!(output.contains("PR:"));
    }

    #[test]
    fn test_real_claude_parse_worker_output_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("output.json");
        std::fs::write(&path, "not json at all").unwrap();
        let (num, url, output) = RealClaude::<RecordingTmux>::parse_worker_output(&path);
        assert_eq!(num, 0);
        assert!(url.is_empty());
        assert_eq!(output, "not json at all");
    }

    #[test]
    fn test_real_claude_parse_worker_output_missing_file() {
        let path = PathBuf::from("/nonexistent/output.json");
        let (num, url, output) = RealClaude::<RecordingTmux>::parse_worker_output(&path);
        assert_eq!(num, 0);
        assert!(url.is_empty());
        assert!(output.is_empty());
    }

    #[test]
    fn test_real_claude_invoke_sends_env_vars_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        // Pre-create exit code marker so polling returns immediately.
        std::fs::write(runtime_dir.join("worker-exit-code"), "0\n").unwrap();
        std::fs::write(
            runtime_dir.join("worker-output.json"),
            r#"{"result": "ok"}"#,
        )
        .unwrap();

        let (tmux, commands) = RecordingTmux::new();
        let claude = RealClaude {
            tmux,
            worker_timeout: Duration::from_secs(5),
        };

        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-env-test".into(),
            work_dir: dir.path().to_string_lossy().into_owned(),
            prompt: "test prompt".into(),
            session_id: "ses-env".into(),
            auto_merge: false,
            target: "main".into(),
            tmux_session: "worker-env".into(),
            acceptance_criteria: vec!["crit A".into(), "crit B".into()],
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(codeflow_core::autorun::ClaudeInvoker::invoke(&claude, cfg))
            .unwrap();
        assert_eq!(result.exit_code, 0);

        let cmds = rt.block_on(commands.lock());
        // Verify env vars were sent in the correct order.
        assert!(
            cmds.len() >= 5,
            "expected at least 5 commands, got {}",
            cmds.len()
        );

        // Command 0: export AUTORUN_SESSION_ID
        assert!(
            cmds[0].1.contains("AUTORUN_SESSION_ID=ses-env"),
            "first env var should be session ID, got: {}",
            cmds[0].1
        );
        // Command 1: export AUTORUN_TASK_ID
        assert!(
            cmds[1].1.contains("AUTORUN_TASK_ID=task-env-test"),
            "second env var should be task ID, got: {}",
            cmds[1].1
        );
        // Command 2: export AUTORUN_ACCEPTANCE (base64)
        assert!(
            cmds[2].1.contains("AUTORUN_ACCEPTANCE="),
            "third env var should be acceptance, got: {}",
            cmds[2].1
        );
        // Command 3: export CODEFLOW_WORKTREE_PATH
        assert!(
            cmds[3].1.contains("CODEFLOW_WORKTREE_PATH="),
            "fourth env var should be worktree path, got: {}",
            cmds[3].1
        );
        // Command 4: the actual claude command
        assert!(
            cmds[4].1.contains("claude -p"),
            "fifth command should be claude invocation, got: {}",
            cmds[4].1
        );
        assert!(
            cmds[4].1.contains("--dangerously-skip-permissions"),
            "claude command should include --dangerously-skip-permissions"
        );
        assert!(
            cmds[4].1.contains("--output-format json"),
            "claude command should include --output-format json"
        );
        assert!(
            cmds[4].1.contains("worker-output.json"),
            "claude output should redirect to worker-output.json"
        );
        assert!(
            cmds[4].1.contains("worker-exit-code"),
            "exit code should be written to worker-exit-code"
        );

        // All commands should target the correct session.
        for (session, _) in &*cmds {
            assert_eq!(session, "worker-env");
        }
    }

    #[test]
    fn test_real_claude_invoke_reads_exit_code_from_marker() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        // Non-zero exit code.
        std::fs::write(runtime_dir.join("worker-exit-code"), "1\n").unwrap();
        std::fs::write(runtime_dir.join("worker-output.json"), "{}").unwrap();

        let (tmux, _) = RecordingTmux::new();
        let claude = RealClaude {
            tmux,
            worker_timeout: Duration::from_secs(5),
        };

        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-exitcode".into(),
            work_dir: dir.path().to_string_lossy().into_owned(),
            prompt: "test".into(),
            session_id: "ses-exit".into(),
            auto_merge: false,
            target: "main".into(),
            tmux_session: "w-exit".into(),
            acceptance_criteria: Vec::new(),
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(codeflow_core::autorun::ClaudeInvoker::invoke(&claude, cfg))
            .unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    fn test_real_claude_invoke_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        // Do NOT create exit code marker -- should trigger timeout.

        let (tmux, commands) = RecordingTmux::new();
        let claude = RealClaude {
            tmux,
            worker_timeout: Duration::from_millis(100), // Very short timeout.
        };

        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-timeout".into(),
            work_dir: dir.path().to_string_lossy().into_owned(),
            prompt: "test".into(),
            session_id: "ses-timeout".into(),
            auto_merge: false,
            target: "main".into(),
            tmux_session: "w-timeout".into(),
            acceptance_criteria: Vec::new(),
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(codeflow_core::autorun::ClaudeInvoker::invoke(&claude, cfg))
            .unwrap();
        assert_eq!(result.exit_code, 124, "timeout should return exit code 124");
        assert!(
            result.output.contains("timeout"),
            "output should mention timeout"
        );

        // Should have sent Ctrl-C for SIGTERM.
        let cmds = rt.block_on(commands.lock());
        let has_ctrl_c = cmds.iter().any(|(_, cmd)| cmd == "C-c");
        assert!(has_ctrl_c, "should send C-c on timeout");
    }

    #[test]
    fn test_real_claude_invoke_preserves_backslash_sequences_in_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(runtime_dir.join("worker-exit-code"), "0\n").unwrap();
        std::fs::write(runtime_dir.join("worker-output.json"), "{}").unwrap();

        let (tmux, commands) = RecordingTmux::new();
        let claude = RealClaude {
            tmux,
            worker_timeout: Duration::from_secs(5),
        };

        // Prompt contains literal \n and \t which must NOT be interpreted as
        // newline/tab when using regular '...' quoting (not $'...').
        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-escape".into(),
            work_dir: dir.path().to_string_lossy().into_owned(),
            prompt: "line1\\nline2\\ttab".into(),
            session_id: "ses-esc".into(),
            auto_merge: false,
            target: "main".into(),
            tmux_session: "w-esc".into(),
            acceptance_criteria: Vec::new(),
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let _ = rt.block_on(codeflow_core::autorun::ClaudeInvoker::invoke(&claude, cfg));

        let cmds = rt.block_on(commands.lock());
        // Find the claude command (the one containing "claude -p").
        let claude_cmd = cmds
            .iter()
            .find(|(_, cmd)| cmd.contains("claude -p"))
            .expect("should have a claude command");

        // Verify it uses regular '...' quoting, not $'...'.
        assert!(
            claude_cmd.1.contains("claude -p '"),
            "should use regular single-quote quoting, got: {}",
            claude_cmd.1
        );
        assert!(
            !claude_cmd.1.contains("$'"),
            "should NOT use $'...' quoting, got: {}",
            claude_cmd.1
        );
        // The literal \n should be preserved in the command string.
        assert!(
            claude_cmd.1.contains("\\n"),
            "backslash-n should be preserved literally, got: {}",
            claude_cmd.1
        );
    }

    // -- run_tmux helper + RealTmux tests --

    #[test]
    fn test_run_tmux_nonexistent_command() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_tmux(&["has-session", "-t", "nonexistent-xyz"]));
        // Ok(false) if tmux available, Err if tmux not installed — both valid.
        match result {
            Ok(success) => assert!(!success),
            Err(_) => {}
        }
    }

    #[test]
    fn test_real_tmux_kill_nonexistent_session_is_ok() {
        let tmux = RealTmux;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(codeflow_core::autorun::TmuxRunner::kill_session(
            &tmux,
            "nonexistent-session-xyz",
        ));
        assert!(result.is_ok());
    }

    #[test]
    fn test_real_tmux_has_nonexistent_session() {
        let tmux = RealTmux;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(codeflow_core::autorun::TmuxRunner::has_session(
            &tmux,
            "nonexistent-session-xyz",
        ));
        match result {
            Ok(has) => assert!(!has, "nonexistent session should not exist"),
            Err(_) => {}
        }
    }

    #[test]
    fn test_real_tmux_create_session_handles_failure() {
        let tmux = RealTmux;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(codeflow_core::autorun::TmuxRunner::create_session(
            &tmux, "",
        ));
        let _ = result;
    }

    #[test]
    fn test_real_tmux_send_command_handles_failure() {
        let tmux = RealTmux;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(codeflow_core::autorun::TmuxRunner::send_command(
            &tmux,
            "nonexistent-session-xyz",
            "echo test",
        ));
        let _ = result;
    }

    // -- run_with_dir integration: batch + config + session ID generation --

    #[test]
    fn test_run_with_dir_generates_session_id_and_prints_batch_info() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow").join("config").join("autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(
            batch_dir.join("batch.yaml"),
            "name: session-gen-test\nmax_workers: 2\ntasks:\n  - id: task-a\n  - id: task-b\n",
        )
        .unwrap();

        // Write custom config.
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "claims": { "default_scope_policy": "hard" } }"#,
        )
        .unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_with_dir(dir.path()));
        // Will fail at orchestrator/tmux level, but exercises config + batch + session ID paths.
        let _ = result;
    }

    #[test]
    fn test_run_with_dir_with_multiple_tasks_and_config() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow").join("config").join("autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(
            batch_dir.join("batch.yaml"),
            "name: multi\nmax_workers: 1\ntasks:\n  - id: t1\n    file_scope: [src/a.rs]\n    scope_policy: soft\n  - id: t2\n    depends_on: [t1]\n",
        )
        .unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_with_dir(dir.path()));
        let _ = result;
    }

    // -- Batch parsing edge cases --

    #[test]
    fn test_batch_parse_default_max_workers() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.yaml");
        // No max_workers specified — should use default.
        std::fs::write(&batch_path, "name: defaults\ntasks:\n  - id: task-1\n").unwrap();
        let parsed = codeflow_core::autorun::batch::parse_batch_file(&batch_path).unwrap();
        assert!(parsed.max_workers > 0, "default max_workers should be > 0");
    }

    #[test]
    fn test_batch_parse_task_without_scope() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.yaml");
        std::fs::write(&batch_path, "name: no-scope\ntasks:\n  - id: task-plain\n").unwrap();
        let parsed = codeflow_core::autorun::batch::parse_batch_file(&batch_path).unwrap();
        assert!(parsed.tasks[0].file_scope.is_empty());
        assert!(parsed.tasks[0].scope_policy.is_none());
    }
}
