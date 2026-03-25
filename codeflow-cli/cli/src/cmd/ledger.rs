//! Ledger command: status, compact, rebuild, migrate, append.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use clap::Subcommand;
use codeflow_core::ledger::{Event, JsonlWriter, LedgerWriter, compact, files, migrate, rebuild};

use crate::helpers;

#[derive(Debug, Subcommand)]
pub enum LedgerCommand {
    /// Show fragment count and event count per type
    Status,
    /// Compact completed fragments into base files
    Compact {
        /// Compact a specific type only
        #[arg(long)]
        r#type: Option<String>,
    },
    /// Rebuild event stream from base + fragments
    Rebuild {
        /// Rebuild a specific type only
        #[arg(long)]
        r#type: Option<String>,
    },
    /// Migrate from flat layout to subdirectory layout
    Migrate,
    /// Append an event to the ledger via the proper routing pipeline
    Append {
        /// Event type (e.g., task_created, session_start, begin_work)
        #[arg(long)]
        event_type: String,
        /// Event data as JSON object (merged into top-level fields)
        #[arg(long)]
        data: String,
    },
}

pub fn run(command: Option<LedgerCommand>) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    let ledger_dir = project_dir.join(".state").join("ledger");

    match command {
        None | Some(LedgerCommand::Status) => run_status(&ledger_dir),
        Some(LedgerCommand::Compact { r#type }) => run_compact(&ledger_dir, r#type.as_deref()),
        Some(LedgerCommand::Rebuild { r#type }) => run_rebuild(&ledger_dir, r#type.as_deref()),
        Some(LedgerCommand::Migrate) => run_migrate(&ledger_dir),
        Some(LedgerCommand::Append { event_type, data }) => {
            run_append(&project_dir, &event_type, &data)
        }
    }
}

fn run_status(ledger_dir: &Path) -> Result<()> {
    if !ledger_dir.is_dir() {
        println!("no ledger directory");
        return Ok(());
    }

    println!(
        "{:<24} {:>6} {:>10} {:>10}",
        "TYPE", "FRAGS", "BASE", "TOTAL"
    );
    println!("{}", "-".repeat(54));

    for type_name in files::ALL {
        let subdir = ledger_dir.join(type_name);
        if !subdir.is_dir() {
            println!("{type_name:<24} {:>6} {:>10} {:>10}", 0, "-", 0);
            continue;
        }

        let base_file = subdir.join(format!("{type_name}.jsonl"));
        let base_lines = count_lines(&base_file);

        let mut frag_count = 0usize;
        let mut frag_lines = 0usize;
        let prefix = format!("{type_name}-ses-");
        if let Ok(entries) = fs::read_dir(&subdir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_str().unwrap_or("");
                if name.starts_with(&prefix) && name.ends_with(".jsonl") && !name.ends_with(".lock")
                {
                    frag_count += 1;
                    frag_lines += count_lines(&entry.path());
                }
            }
        }

        let total = base_lines + frag_lines;
        let base_str = if base_file.exists() {
            base_lines.to_string()
        } else {
            "-".to_string()
        };
        println!("{type_name:<24} {frag_count:>6} {base_str:>10} {total:>10}");
    }

    Ok(())
}

fn run_compact(ledger_dir: &Path, type_filter: Option<&str>) -> Result<()> {
    if let Some(type_name) = type_filter {
        let result =
            compact::compact_ledger_type(ledger_dir, type_name, None).context("compacting")?;
        println!(
            "{}: merged {} fragments, skipped {} active",
            result.type_name, result.merged_count, result.skipped_active
        );
    } else {
        let results = compact::compact_all(ledger_dir, None).context("compacting all")?;
        for result in &results {
            if result.merged_count > 0 || result.skipped_active > 0 {
                println!(
                    "{}: merged {} fragments, skipped {} active",
                    result.type_name, result.merged_count, result.skipped_active
                );
            }
        }
        let total: usize = results.iter().map(|r| r.merged_count).sum();
        println!("total: {total} fragments merged");
    }
    Ok(())
}

fn run_rebuild(ledger_dir: &Path, type_filter: Option<&str>) -> Result<()> {
    if let Some(type_name) = type_filter {
        let events = rebuild::rebuild_ledger_type(ledger_dir, type_name).context("rebuilding")?;
        println!("{type_name}: {} events", events.len());
    } else {
        let all = rebuild::rebuild_all(ledger_dir).context("rebuilding all")?;
        for (type_name, events) in &all {
            println!("{type_name}: {} events", events.len());
        }
        let total: usize = all.values().map(Vec::len).sum();
        println!("total: {total} events");
    }
    Ok(())
}

/// Resolve session ID and ledger directory from environment or codeflow-env.sh.
///
/// Checks env vars first (`CODEFLOW_SESSION_ID`, `CODEFLOW_WORKTREE_PATH`),
/// then falls back to reading `.state/runtime/codeflow-env.sh`.
fn resolve_session_context(project_dir: &Path) -> Result<(String, PathBuf)> {
    // Try env vars first (set by hooks at runtime).
    let session_id = std::env::var("CODEFLOW_SESSION_ID").ok();
    let worktree_path = std::env::var("CODEFLOW_WORKTREE_PATH").ok();

    // Fall back to codeflow-env.sh if env vars are not set.
    let (session_id, worktree_path) = match session_id {
        Some(sid) if !sid.is_empty() => (sid, worktree_path),
        _ => {
            let runtime_dir = project_dir.join(".state").join("runtime");
            match codeflow_core::session::read_env_file(&runtime_dir) {
                Ok(Some(env)) => (
                    env.session_id.as_str().to_string(),
                    env.worktree_path,
                ),
                Ok(None) => bail!("no session ID found: set CODEFLOW_SESSION_ID or ensure .state/runtime/codeflow-env.sh exists"),
                Err(e) => bail!("failed to read codeflow-env.sh: {e}"),
            }
        }
    };

    // Resolve ledger directory: worktree-local if worktree path is set.
    let ledger_dir = match &worktree_path {
        Some(wt) if !wt.is_empty() => PathBuf::from(wt).join(".state").join("ledger"),
        _ => project_dir.join(".state").join("ledger"),
    };

    Ok((session_id, ledger_dir))
}

fn run_append(project_dir: &Path, event_type: &str, data_json: &str) -> Result<()> {
    let (session_id, ledger_dir) = resolve_session_context(project_dir)?;

    // Parse user-provided data as a JSON object.
    let data_value: serde_json::Value =
        serde_json::from_str(data_json).context("--data must be valid JSON")?;
    let data: HashMap<String, serde_json::Value> = match data_value {
        serde_json::Value::Object(map) => map.into_iter().collect(),
        _ => bail!("--data must be a JSON object ({{...}}), not an array or scalar"),
    };

    // Construct the writer with session-scoped fragment files.
    let writer = JsonlWriter::new_with_session(&ledger_dir, Some(session_id.clone()))
        .context("opening ledger writer")?;

    // Validate event type is routable before constructing the event.
    writer
        .route_event(event_type)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let event = Event {
        event_type: event_type.to_string(),
        timestamp: Utc::now().to_rfc3339(),
        session_id: Some(session_id),
        worktree: std::env::var("CODEFLOW_WORKTREE_PATH").ok().filter(|s| !s.is_empty()),
        data,
    };

    writer
        .append_event(event.clone())
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Print the written event to stdout for verification.
    let json = serde_json::to_string(&event).context("serializing event for output")?;
    println!("{json}");

    Ok(())
}

fn run_migrate(ledger_dir: &Path) -> Result<()> {
    let result = migrate::migrate_flat_to_subdirs(ledger_dir).context("migrating")?;
    if result.already_migrated {
        println!("already migrated (subdirectory layout)");
    } else {
        println!(
            "migrated {} files to subdirectory layout",
            result.migrated_count
        );
    }
    Ok(())
}

/// Count non-empty lines in a JSONL file.
fn count_lines(path: &Path) -> usize {
    fs::read_to_string(path)
        .map(|c| c.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- count_lines ---

    #[test]
    fn test_count_lines_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(count_lines(&dir.path().join("nope.jsonl")), 0);
    }

    #[test]
    fn test_count_lines_with_content() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("test.jsonl");
        fs::write(&f, "line1\nline2\n\nline3\n").unwrap();
        assert_eq!(count_lines(&f), 3);
    }

    #[test]
    fn test_count_lines_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("empty.jsonl");
        fs::write(&f, "").unwrap();
        assert_eq!(count_lines(&f), 0);
    }

    #[test]
    fn test_count_lines_whitespace_only() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("ws.jsonl");
        fs::write(&f, "   \n  \n\n").unwrap();
        assert_eq!(count_lines(&f), 0);
    }

    // --- run_status ---

    #[test]
    fn test_status_no_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_status(&dir.path().join("nonexistent"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_status_empty_ledger() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path()).unwrap();
        let result = run_status(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_status_with_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path();
        let sessions = ledger.join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("sessions.jsonl"),
            "{\"event\":\"session_start\"}\n",
        )
        .unwrap();
        fs::write(
            sessions.join("sessions-ses-abc.jsonl"),
            "{\"event\":\"session_end\"}\n",
        )
        .unwrap();
        // Lock file should be ignored.
        fs::write(sessions.join("sessions-ses-abc.jsonl.lock"), "").unwrap();
        let result = run_status(ledger);
        assert!(result.is_ok());
    }

    #[test]
    fn test_status_subdir_no_base_file() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path();
        let wg = ledger.join("work-graph");
        fs::create_dir_all(&wg).unwrap();
        // Fragment but no base file -- should show "-" for base.
        fs::write(
            wg.join("work-graph-ses-x.jsonl"),
            "{\"event\":\"task_created\"}\n",
        )
        .unwrap();
        let result = run_status(ledger);
        assert!(result.is_ok());
    }

    // --- run_compact ---

    #[test]
    fn test_compact_single_type_empty() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_compact(dir.path(), Some("work-graph"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_compact_all_empty() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_compact(dir.path(), None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compact_single_type_with_fragment() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path();

        // Create sessions with session_end so fragment is compactable.
        let sessions = ledger.join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("sessions.jsonl"),
            "{\"event\":\"session_end\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"session_id\":\"ses-done\"}\n",
        ).unwrap();

        let wg = ledger.join("work-graph");
        fs::create_dir_all(&wg).unwrap();
        fs::write(
            wg.join("work-graph-ses-done.jsonl"),
            "{\"event\":\"task_created\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"session_id\":\"ses-done\"}\n",
        ).unwrap();

        let result = run_compact(ledger, Some("work-graph"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_compact_all_with_activity() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path();

        // Create a type with an active (non-compactable) fragment.
        let cfg = ledger.join("config");
        fs::create_dir_all(&cfg).unwrap();
        fs::write(
            cfg.join("config-ses-active.jsonl"),
            "{\"event\":\"config_set\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"session_id\":\"ses-active\"}\n",
        ).unwrap();

        let result = run_compact(ledger, None);
        assert!(result.is_ok());
    }

    // --- run_rebuild ---

    #[test]
    fn test_rebuild_single_type_empty() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_rebuild(dir.path(), Some("sessions"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_rebuild_all_empty() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_rebuild(dir.path(), None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_rebuild_single_type_with_events() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path();
        let sessions = ledger.join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("sessions.jsonl"),
            "{\"event\":\"session_start\",\"timestamp\":\"2026-01-01T00:00:00Z\"}\n",
        )
        .unwrap();

        let result = run_rebuild(ledger, Some("sessions"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_rebuild_all_with_events() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path();
        let wg = ledger.join("work-graph");
        fs::create_dir_all(&wg).unwrap();
        fs::write(
            wg.join("work-graph.jsonl"),
            "{\"event\":\"task_created\",\"timestamp\":\"2026-01-01T00:00:00Z\"}\n",
        )
        .unwrap();
        fs::write(
            wg.join("work-graph-ses-a.jsonl"),
            "{\"event\":\"task_status_changed\",\"timestamp\":\"2026-01-02T00:00:00Z\"}\n",
        )
        .unwrap();

        let result = run_rebuild(ledger, None);
        assert!(result.is_ok());
    }

    // --- run_migrate ---

    #[test]
    fn test_migrate_flat_layout() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path();
        fs::create_dir_all(ledger).unwrap();
        fs::write(ledger.join("sessions.jsonl"), "data\n").unwrap();

        let result = run_migrate(ledger);
        assert!(result.is_ok());
        assert!(ledger.join("sessions/sessions.jsonl").exists());
    }

    #[test]
    fn test_migrate_already_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path();
        // Create subdirectory layout.
        for type_name in files::ALL {
            let subdir = ledger.join(type_name);
            fs::create_dir_all(&subdir).unwrap();
            fs::write(subdir.join(format!("{type_name}.jsonl")), "").unwrap();
        }

        let result = run_migrate(ledger);
        assert!(result.is_ok());
    }

    #[test]
    fn test_migrate_nonexistent_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_migrate(&dir.path().join("nope"));
        assert!(result.is_ok());
    }

    // --- run_append ---

    #[test]
    fn test_append_writes_to_correct_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let ledger_dir = project.join(".state").join("ledger");
        fs::create_dir_all(&ledger_dir).unwrap();

        // Create env file so resolve_session_context can find it.
        let runtime_dir = project.join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::write(
            runtime_dir.join("codeflow-env.sh"),
            "export CODEFLOW_SESSION_ID='ses-test-append'\nexport CF_PROJECT_ROOT='codeflow'\n",
        )
        .unwrap();

        let result = run_append(
            project,
            "task_created",
            r#"{"format_id":"TEST-001"}"#,
        );
        assert!(result.is_ok(), "run_append failed: {result:?}");

        // Should write to work-graph/work-graph-ses-test-append.jsonl
        let fragment = ledger_dir
            .join("work-graph")
            .join("work-graph-ses-test-append.jsonl");
        assert!(fragment.exists(), "fragment file should exist at {}", fragment.display());

        let content = fs::read_to_string(&fragment).unwrap();
        assert!(content.contains("\"event\":\"task_created\""));
        assert!(content.contains("\"format_id\":\"TEST-001\""));
        assert!(content.contains("\"session_id\":\"ses-test-append\""));
        assert!(content.contains("\"timestamp\""));
    }

    #[test]
    fn test_append_rejects_unknown_event_type() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let runtime_dir = project.join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::write(
            runtime_dir.join("codeflow-env.sh"),
            "export CODEFLOW_SESSION_ID='ses-test-bad'\nexport CF_PROJECT_ROOT='codeflow'\n",
        )
        .unwrap();

        let result = run_append(project, "totally_bogus_event", r#"{"key":"val"}"#);
        assert!(result.is_err(), "unknown event type should fail");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("totally_bogus_event"),
            "error should mention the unknown event type: {err_msg}"
        );
    }

    #[test]
    fn test_append_rejects_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let runtime_dir = project.join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::write(
            runtime_dir.join("codeflow-env.sh"),
            "export CODEFLOW_SESSION_ID='ses-test-json'\nexport CF_PROJECT_ROOT='codeflow'\n",
        )
        .unwrap();

        let result = run_append(project, "task_created", "not json");
        assert!(result.is_err(), "invalid JSON should fail");
    }

    #[test]
    fn test_append_rejects_non_object_json() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let runtime_dir = project.join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::write(
            runtime_dir.join("codeflow-env.sh"),
            "export CODEFLOW_SESSION_ID='ses-test-arr'\nexport CF_PROJECT_ROOT='codeflow'\n",
        )
        .unwrap();

        let result = run_append(project, "task_created", r#"["not","an","object"]"#);
        assert!(result.is_err(), "JSON array should fail");
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("object"), "error should mention object: {err_msg}");
    }

    #[test]
    fn test_append_routes_session_events_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let ledger_dir = project.join(".state").join("ledger");
        fs::create_dir_all(&ledger_dir).unwrap();
        let runtime_dir = project.join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::write(
            runtime_dir.join("codeflow-env.sh"),
            "export CODEFLOW_SESSION_ID='ses-route-test'\nexport CF_PROJECT_ROOT='codeflow'\n",
        )
        .unwrap();

        // session_start should route to sessions/ subdirectory.
        let result = run_append(project, "session_start", r#"{"source":"startup"}"#);
        assert!(result.is_ok(), "session_start append failed: {result:?}");

        let fragment = ledger_dir
            .join("sessions")
            .join("sessions-ses-route-test.jsonl");
        assert!(fragment.exists(), "sessions fragment should exist");
        let content = fs::read_to_string(&fragment).unwrap();
        assert!(content.contains("\"event\":\"session_start\""));
        assert!(content.contains("\"source\":\"startup\""));
    }

    #[test]
    fn test_append_no_session_id_fails() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        // No env file, no env var — should fail.
        let result = run_append(project, "task_created", r#"{"key":"val"}"#);
        assert!(result.is_err(), "missing session ID should fail");
    }

    // --- resolve_session_context ---

    #[test]
    fn test_resolve_session_context_from_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let runtime_dir = project.join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::write(
            runtime_dir.join("codeflow-env.sh"),
            "export CODEFLOW_SESSION_ID='ses-ctx-test'\nexport CF_PROJECT_ROOT='codeflow'\n",
        )
        .unwrap();

        let (sid, ledger_dir) = resolve_session_context(project).unwrap();
        assert_eq!(sid, "ses-ctx-test");
        assert_eq!(ledger_dir, project.join(".state").join("ledger"));
    }

    #[test]
    fn test_resolve_session_context_with_worktree_in_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let wt_path = dir.path().join("my-worktree");
        fs::create_dir_all(wt_path.join(".state").join("runtime")).unwrap();

        let runtime_dir = project.join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::write(
            runtime_dir.join("codeflow-env.sh"),
            format!(
                "export CODEFLOW_SESSION_ID='ses-wt-test'\nexport CF_PROJECT_ROOT='codeflow'\nexport CODEFLOW_WORKTREE_PATH='{}'\n",
                wt_path.display()
            ),
        )
        .unwrap();

        let (sid, ledger_dir) = resolve_session_context(project).unwrap();
        assert_eq!(sid, "ses-wt-test");
        assert_eq!(
            ledger_dir,
            wt_path.join(".state").join("ledger"),
            "ledger should resolve to worktree-local path"
        );
    }
}
