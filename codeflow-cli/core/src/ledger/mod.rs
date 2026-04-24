pub mod compact;
mod jsonl;
pub mod migrate;
pub mod rebuild;
mod routing;

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::LedgerError;

pub use jsonl::JsonlWriter;
pub use routing::route_event_type;

use crate::worktree::WorktreePaths;

/// Resolve the canonical base file path for a ledger type.
///
/// Returns `<state_dir>/ledger/<type_name>/<type_name>.jsonl`, where
/// `<state_dir>` is resolved as follows:
///
/// - `worktree_aware = true` and `CODEFLOW_WORKTREE_PATH` is set to a
///   non-empty value: `<worktree_root>/.state`.
/// - `worktree_aware = true` and the env var is unset or empty: the main-repo
///   `./.state` directory (relative to the current working directory).
/// - `worktree_aware = false`: the main-repo `./.state` directory.
///
/// This is the single authoritative path resolver for ledger consumers.
/// Callers that need to read a ledger file must use this helper (or the
/// explicit-state-dir companion [`resolve_path_in`]) instead of constructing
/// paths from string literals, so the regression test in this module keeps
/// the source tree free of legacy `.state/logs/pathflow-events` references.
///
/// # Errors
///
/// Returns [`LedgerError::UnknownType`] when `type_name` is not listed in
/// [`files::ALL`].
pub fn resolve_path(type_name: &str, worktree_aware: bool) -> Result<PathBuf, LedgerError> {
    validate_type(type_name)?;
    let state_dir = if worktree_aware {
        WorktreePaths::from_env()
            .map_or_else(|| PathBuf::from(".").join(".state"), |wp| wp.state_dir())
    } else {
        PathBuf::from(".").join(".state")
    };
    Ok(ledger_file_path(&state_dir, type_name))
}

/// Resolve the canonical base file path for a ledger type under an explicit
/// state directory.
///
/// Returns `<state_dir>/ledger/<type_name>/<type_name>.jsonl`. Callers that
/// already know the state directory (for example, `codeflow doctor` which is
/// parameterised by `--state-dir`) use this variant to avoid re-reading
/// `CODEFLOW_WORKTREE_PATH`.
///
/// # Errors
///
/// Returns [`LedgerError::UnknownType`] when `type_name` is not listed in
/// [`files::ALL`].
pub fn resolve_path_in(
    state_dir: &std::path::Path,
    type_name: &str,
) -> Result<PathBuf, LedgerError> {
    validate_type(type_name)?;
    Ok(ledger_file_path(state_dir, type_name))
}

fn validate_type(type_name: &str) -> Result<(), LedgerError> {
    if files::ALL.contains(&type_name) {
        Ok(())
    } else {
        Err(LedgerError::UnknownType(type_name.to_string()))
    }
}

fn ledger_file_path(state_dir: &std::path::Path, type_name: &str) -> PathBuf {
    state_dir
        .join("ledger")
        .join(type_name)
        .join(format!("{type_name}.jsonl"))
}

/// Canonical JSONL ledger type names (directory names in subdirectory layout).
///
/// Each type maps to a subdirectory under `.state/ledger/`. Within each
/// subdirectory, the base file is `{type}.jsonl` and session fragments are
/// `{type}-ses-{session_id}.jsonl`.
pub mod files {
    pub const WORK_GRAPH: &str = "work-graph";
    pub const MEMORY_EVENTS: &str = "memory-events";
    pub const SESSIONS: &str = "sessions";
    pub const CONFIG: &str = "config";
    pub const PATHFLOW_EVENTS: &str = "pathflow-events";
    pub const COORDINATION_EVENTS: &str = "coordination-events";
    pub const AUTORUN_EVENTS: &str = "autorun-events";

    /// Types synced to the database (excludes pathflow-events, coordination-events,
    /// and autorun-events).
    pub const CANONICAL: &[&str] = &[WORK_GRAPH, MEMORY_EVENTS, SESSIONS, CONFIG];

    /// All 7 canonical ledger type names.
    pub const ALL: &[&str] = &[
        WORK_GRAPH,
        MEMORY_EVENTS,
        SESSIONS,
        CONFIG,
        PATHFLOW_EVENTS,
        COORDINATION_EVENTS,
        AUTORUN_EVENTS,
    ];
}

/// Result of a compaction operation on a single ledger type.
#[derive(Debug, Clone)]
pub struct CompactionResult {
    /// The ledger type that was compacted (e.g., "work-graph").
    pub type_name: String,
    /// Number of fragment files merged into the base.
    pub merged_count: usize,
    /// Paths of fragment files that were deleted after merging.
    pub deleted_files: Vec<PathBuf>,
    /// Number of active sessions skipped (no session_end found).
    pub skipped_active: usize,
}

/// Check if a filename has a `.jsonl` extension (case-insensitive).
#[must_use]
pub fn is_jsonl_file(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
}

/// Check if a filename has a `.lock` extension (case-insensitive).
#[must_use]
pub fn is_lock_file(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("lock"))
}

/// Result of migrating from flat layout to subdirectory layout.
#[derive(Debug, Clone)]
pub struct MigrationResult {
    /// Number of types successfully migrated.
    pub migrated_count: usize,
    /// Whether the migration was already complete (no flat files found).
    pub already_migrated: bool,
}

/// A single JSONL ledger event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Canonical event type (e.g., `session_start`, `task_created`).
    #[serde(rename = "event")]
    pub event_type: String,

    /// RFC 3339 timestamp.
    pub timestamp: String,

    /// Session that produced this event (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Worktree path that produced this event (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub worktree: Option<String>,

    /// Event-specific key-value pairs, flattened into the top-level object.
    #[serde(flatten)]
    pub data: HashMap<String, serde_json::Value>,
}

/// `LedgerWriter` abstracts append-only JSONL event logging.
///
/// All methods are synchronous -- JSONL appends are local file I/O.
pub trait LedgerWriter: Send + Sync {
    /// Validate the event schema, route to the correct file, and append atomically.
    ///
    /// # Errors
    ///
    /// Returns `LedgerError` on I/O failure, serialization error, or unknown event type.
    fn append_event(&self, event: Event) -> Result<(), LedgerError>;

    /// Validate and append to a specific file (bypasses routing).
    ///
    /// # Errors
    ///
    /// Returns `LedgerError::MisroutedEvent` if the event type does not belong
    /// to the target file. Returns I/O or serialization errors on write failure.
    fn append_event_to_file(&self, target_file: &str, event: Event) -> Result<(), LedgerError>;

    /// Route an event type to its canonical ledger file.
    ///
    /// # Errors
    ///
    /// Returns `LedgerError::UnknownEventType` if the event type is not recognized.
    fn route_event(&self, event_type: &str) -> Result<String, LedgerError>;

    /// Return the ledger directory path.
    fn dir(&self) -> &std::path::Path;
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn make_event(event_type: &str, session_id: Option<&str>) -> Event {
        let mut data = HashMap::new();
        data.insert("key".to_string(), serde_json::json!("value"));
        Event {
            event_type: event_type.to_string(),
            timestamp: "2026-03-07T12:00:00Z".to_string(),
            session_id: session_id.map(String::from),
            worktree: None,
            data,
        }
    }

    #[test]
    fn test_event_serde_roundtrip() {
        let event = make_event("task_created", Some("ses-123"));
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["event"], "task_created");
        assert_eq!(parsed["timestamp"], "2026-03-07T12:00:00Z");
        assert_eq!(parsed["session_id"], "ses-123");
        assert_eq!(parsed["key"], "value");
    }

    #[test]
    fn test_event_no_session_id_omits_field() {
        let event = make_event("epic_created", None);
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // skip_serializing_if = Option::is_none means session_id absent
        assert!(
            parsed.get("session_id").is_none(),
            "session_id should be absent when None, but got: {json}",
        );
    }

    #[test]
    fn test_event_rename_event_field() {
        // event_type field serializes as "event"
        let event = make_event("begin_work", None);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"begin_work""#));
        assert!(!json.contains("event_type"));
    }

    #[test]
    fn test_event_flatten_data() {
        let mut data = HashMap::new();
        data.insert(
            "format_id".to_string(),
            serde_json::json!("INF-TSK-022-019"),
        );
        data.insert("status".to_string(), serde_json::json!("todo"));
        let event = Event {
            event_type: "task_created".to_string(),
            timestamp: "2026-03-07T00:00:00Z".to_string(),
            session_id: None,
            worktree: None,
            data,
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Flattened fields appear at top level
        assert_eq!(parsed["format_id"], "INF-TSK-022-019");
        assert_eq!(parsed["status"], "todo");
        assert_eq!(parsed["event"], "task_created");
    }

    #[test]
    fn test_event_deserialize_from_jsonl_line() {
        let line = r#"{"event":"session_start","timestamp":"2026-03-07T00:00:00Z","session_id":"ses-abc","source":"startup"}"#;
        let event: Event = serde_json::from_str(line).unwrap();
        assert_eq!(event.event_type, "session_start");
        assert_eq!(event.timestamp, "2026-03-07T00:00:00Z");
        assert_eq!(event.session_id.as_deref(), Some("ses-abc"));
        assert_eq!(event.data["source"], "startup");
    }

    #[test]
    fn test_ledger_files_constants() {
        assert_eq!(files::WORK_GRAPH, "work-graph");
        assert_eq!(files::MEMORY_EVENTS, "memory-events");
        assert_eq!(files::SESSIONS, "sessions");
        assert_eq!(files::CONFIG, "config");
        assert_eq!(files::PATHFLOW_EVENTS, "pathflow-events");
        assert_eq!(files::COORDINATION_EVENTS, "coordination-events");
        assert_eq!(files::AUTORUN_EVENTS, "autorun-events");
        assert_eq!(files::CANONICAL.len(), 4);
        assert!(!files::CANONICAL.contains(&files::PATHFLOW_EVENTS));
        assert!(!files::CANONICAL.contains(&files::COORDINATION_EVENTS));
        assert!(!files::CANONICAL.contains(&files::AUTORUN_EVENTS));
        assert_eq!(files::ALL.len(), 7);
    }

    #[test]
    fn test_event_snapshot() {
        let event = make_event("task_status_changed", Some("ses-177137202131769e89b2d5688"));
        insta::assert_json_snapshot!(event);
    }

    // -- resolve_path / resolve_path_in --

    #[test]
    fn test_resolve_path_in_known_pathflow_events() {
        let state_dir = PathBuf::from("/tmp/test-state");
        let p = resolve_path_in(&state_dir, files::PATHFLOW_EVENTS).unwrap();
        assert_eq!(
            p,
            PathBuf::from("/tmp/test-state/ledger/pathflow-events/pathflow-events.jsonl")
        );
    }

    #[test]
    fn test_resolve_path_in_known_work_graph() {
        let state_dir = PathBuf::from("/tmp/test-state");
        let p = resolve_path_in(&state_dir, files::WORK_GRAPH).unwrap();
        assert_eq!(
            p,
            PathBuf::from("/tmp/test-state/ledger/work-graph/work-graph.jsonl")
        );
    }

    #[test]
    fn test_resolve_path_in_all_canonical_types() {
        // Every entry in files::ALL must be accepted.
        let state_dir = PathBuf::from("/s");
        for ty in files::ALL {
            let p = resolve_path_in(&state_dir, ty).expect("every files::ALL entry must resolve");
            assert_eq!(p, PathBuf::from(format!("/s/ledger/{ty}/{ty}.jsonl")));
        }
    }

    #[test]
    fn test_resolve_path_in_unknown_type_returns_err() {
        let state_dir = PathBuf::from("/tmp/test-state");
        let err = resolve_path_in(&state_dir, "unknown-type").unwrap_err();
        match err {
            LedgerError::UnknownType(name) => assert_eq!(name, "unknown-type"),
            other => panic!("expected UnknownType, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_path_unknown_type_returns_err() {
        let err = resolve_path("bogus", false).unwrap_err();
        assert!(matches!(err, LedgerError::UnknownType(s) if s == "bogus"));
    }

    #[test]
    #[serial_test::serial]
    fn test_resolve_path_worktree_aware_uses_env() {
        let td = tempfile::tempdir().unwrap();
        let wt_root = td.path();
        // SAFETY: serial test exclusivity over env.
        unsafe { std::env::set_var("CODEFLOW_WORKTREE_PATH", wt_root) };
        let p = resolve_path(files::PATHFLOW_EVENTS, true).unwrap();
        // SAFETY: serial test exclusivity over env.
        unsafe { std::env::remove_var("CODEFLOW_WORKTREE_PATH") };
        assert_eq!(
            p,
            wt_root
                .join(".state")
                .join("ledger")
                .join("pathflow-events")
                .join("pathflow-events.jsonl"),
        );
    }

    #[test]
    #[serial_test::serial]
    fn test_resolve_path_worktree_aware_unset_falls_back_to_main_state() {
        // SAFETY: serial test exclusivity over env.
        unsafe { std::env::remove_var("CODEFLOW_WORKTREE_PATH") };
        let p = resolve_path(files::PATHFLOW_EVENTS, true).unwrap();
        assert_eq!(
            p,
            PathBuf::from("./.state/ledger/pathflow-events/pathflow-events.jsonl"),
        );
    }

    #[test]
    #[serial_test::serial]
    fn test_resolve_path_worktree_aware_empty_env_falls_back() {
        // SAFETY: serial test exclusivity over env.
        unsafe { std::env::set_var("CODEFLOW_WORKTREE_PATH", "") };
        let p = resolve_path(files::PATHFLOW_EVENTS, true).unwrap();
        // SAFETY: serial test exclusivity over env.
        unsafe { std::env::remove_var("CODEFLOW_WORKTREE_PATH") };
        assert_eq!(
            p,
            PathBuf::from("./.state/ledger/pathflow-events/pathflow-events.jsonl"),
        );
    }

    #[test]
    #[serial_test::serial]
    fn test_resolve_path_non_worktree_ignores_env() {
        let td = tempfile::tempdir().unwrap();
        // SAFETY: serial test exclusivity over env.
        unsafe { std::env::set_var("CODEFLOW_WORKTREE_PATH", td.path()) };
        let p = resolve_path(files::PATHFLOW_EVENTS, false).unwrap();
        // SAFETY: serial test exclusivity over env.
        unsafe { std::env::remove_var("CODEFLOW_WORKTREE_PATH") };
        // worktree_aware=false MUST ignore the env var and use the main-repo path.
        assert_eq!(
            p,
            PathBuf::from("./.state/ledger/pathflow-events/pathflow-events.jsonl"),
        );
    }

    #[test]
    fn test_event_serialization_with_worktree() {
        let mut data = HashMap::new();
        data.insert("key".to_string(), serde_json::json!("value"));
        let event = Event {
            event_type: "phase_transition".to_string(),
            timestamp: "2026-03-07T12:00:00Z".to_string(),
            session_id: Some("ses-123".to_string()),
            worktree: Some("/tmp/worktree/abc".to_string()),
            data,
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["worktree"], "/tmp/worktree/abc");
        assert_eq!(parsed["session_id"], "ses-123");
    }

    #[test]
    fn test_event_deserialization_without_worktree() {
        // Old events without a worktree field should deserialize with worktree = None
        let line = r#"{"event":"phase_transition","timestamp":"2026-03-07T00:00:00Z","session_id":"ses-old","phase":"PF1-INIT"}"#;
        let event: Event = serde_json::from_str(line).unwrap();
        assert!(event.worktree.is_none());
        assert_eq!(event.event_type, "phase_transition");
        assert_eq!(event.session_id.as_deref(), Some("ses-old"));
    }

    #[test]
    fn test_event_worktree_skip_serializing_none() {
        let event = make_event("session_start", Some("ses-1"));
        // worktree is None in make_event
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // skip_serializing_if = Option::is_none means worktree is absent
        assert!(
            parsed.get("worktree").is_none(),
            "worktree should be absent when None, but got: {json}",
        );
    }

    // -- Regression lint: no hardcoded pathflow-events path literals outside
    //    (a) ledger/mod.rs resolve_path (this helper), and
    //    (b) session_start.rs migrate_pathflow_events_from_logs migration function,
    //    (c) lines carrying `// EXEMPT:` comments,
    //    (d) test code (#[cfg(test)] modules and files under /tests/).
    //
    // This prevents future consumers from silently reading the legacy
    // `.state/logs/pathflow-events.jsonl` path after the INF-TSK-024-035
    // migration and closes gap G1 from the 2026-04-22 chain audit.
    mod regression_lint {
        use std::path::{Path, PathBuf};

        fn workspace_src_roots() -> Vec<PathBuf> {
            // CARGO_MANIFEST_DIR for this crate = codeflow-cli/core
            let core_manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
            let workspace_root = core_manifest
                .parent()
                .expect("core crate must have a parent")
                .to_path_buf();
            vec![
                workspace_root.join("core").join("src"),
                workspace_root.join("cli").join("src"),
            ]
        }

        fn collect_rs_files(root: &Path) -> Vec<PathBuf> {
            let pattern = root.join("**").join("*.rs");
            glob::glob(&pattern.to_string_lossy())
                .expect("glob pattern compiles")
                .filter_map(Result::ok)
                .collect()
        }

        /// Return the indices of lines that live inside a `#[cfg(test)]` module
        /// (or nested module within one). Uses brace-counting to track scope.
        fn test_line_mask(lines: &[&str]) -> Vec<bool> {
            let mut mask = vec![false; lines.len()];
            let mut i = 0;
            while i < lines.len() {
                let trimmed = lines[i].trim_start();
                if trimmed.starts_with("#[cfg(test)]") {
                    // Find the opening `{` of the following module.
                    let mut j = i + 1;
                    while j < lines.len() && !lines[j].contains('{') {
                        j += 1;
                    }
                    if j >= lines.len() {
                        break;
                    }
                    // Count braces starting from line j until depth returns to 0.
                    let mut depth: i32 = 0;
                    let mut started = false;
                    let mut k = j;
                    while k < lines.len() {
                        for ch in lines[k].chars() {
                            if ch == '{' {
                                depth += 1;
                                started = true;
                            } else if ch == '}' {
                                depth -= 1;
                            }
                        }
                        mask[k] = true;
                        if started && depth <= 0 {
                            k += 1;
                            break;
                        }
                        k += 1;
                    }
                    i = k;
                    continue;
                }
                i += 1;
            }
            mask
        }

        /// Path is exempt from the regression lint because the file legitimately
        /// references the legacy location (helper definitions or migration code).
        fn is_exempt_file(path: &Path) -> bool {
            let s = path.to_string_lossy();
            // This file (ledger/mod.rs) contains the resolve_path helper and
            // this very lint's documentation strings.
            if s.ends_with("ledger/mod.rs") || s.ends_with("ledger\\mod.rs") {
                return true;
            }
            // session_start.rs contains the one-shot migration from
            // .state/logs/pathflow-events.jsonl -> ledger subdir.
            if s.ends_with("hooks/session_start.rs") || s.ends_with("hooks\\session_start.rs") {
                return true;
            }
            false
        }

        fn line_is_exempt(line: &str) -> bool {
            line.contains("// EXEMPT:")
        }

        #[test]
        fn no_hardcoded_legacy_pathflow_events_literals() {
            let mut offenders: Vec<String> = Vec::new();
            for root in workspace_src_roots() {
                if !root.exists() {
                    continue;
                }
                for file in collect_rs_files(&root) {
                    if is_exempt_file(&file) {
                        continue;
                    }
                    let Ok(content) = std::fs::read_to_string(&file) else {
                        continue;
                    };
                    let lines: Vec<&str> = content.lines().collect();
                    let test_mask = test_line_mask(&lines);
                    for (idx, line) in lines.iter().enumerate() {
                        if test_mask[idx] {
                            continue;
                        }
                        if line_is_exempt(line) {
                            continue;
                        }
                        if line.contains(".state/logs/pathflow-events")
                            || line.contains("pathflow-events.jsonl")
                        {
                            offenders.push(format!(
                                "{}:{}: {}",
                                file.display(),
                                idx + 1,
                                line.trim(),
                            ));
                        }
                    }
                }
            }
            assert!(
                offenders.is_empty(),
                "hardcoded legacy pathflow-events path literals found in non-test code \
                 (use ledger::resolve_path / resolve_path_in or add // EXEMPT: <reason>):\n{}",
                offenders.join("\n"),
            );
        }

        // -- Meta tests: verify the lint has teeth.

        #[test]
        fn test_line_mask_marks_cfg_test_module() {
            let src = r"fn a() {}

#[cfg(test)]
mod tests {
    #[test]
    fn t() {}
}

fn b() {}
";
            let lines: Vec<&str> = src.lines().collect();
            let mask = test_line_mask(&lines);
            // Non-test code outside the module.
            assert!(!mask[0]); // fn a() {}
            assert!(!mask[1]); // blank
            assert!(!mask[2]); // #[cfg(test)]
            // Lines inside the test module.
            assert!(mask[3]); // mod tests {
            assert!(mask[4]); //     #[test]
            assert!(mask[5]); //     fn t() {}
            assert!(mask[6]); // }
            // Non-test code after the module.
            assert!(!mask[7]); // blank
            assert!(!mask[8]); // fn b() {}
        }

        #[test]
        fn test_line_mask_handles_no_test_module() {
            let src = "fn a() {}\nfn b() {}\n";
            let lines: Vec<&str> = src.lines().collect();
            let mask = test_line_mask(&lines);
            assert!(mask.iter().all(|m| !m));
        }

        #[test]
        fn test_line_mask_ignores_non_test_cfg() {
            // Sanity: #[cfg(feature = "x")] must NOT be treated as a test module.
            let src = r#"#[cfg(feature = "x")]
fn gated() {}
"#;
            let lines: Vec<&str> = src.lines().collect();
            let mask = test_line_mask(&lines);
            assert!(mask.iter().all(|m| !m));
        }

        #[test]
        fn line_is_exempt_detects_marker() {
            assert!(line_is_exempt(
                "let p = \".state/logs/pathflow-events.jsonl\"; // EXEMPT: historical migration"
            ));
            assert!(!line_is_exempt(
                "let p = \".state/logs/pathflow-events.jsonl\";"
            ));
        }

        #[test]
        fn regression_lint_flags_offending_literal_when_unmasked() {
            // Verify the lint has teeth: when we simulate a non-test, non-exempt
            // file containing the legacy string, the detection logic triggers.
            let src = "pub fn bad() -> &'static str { \"pathflow-events.jsonl\" }\n";
            let lines: Vec<&str> = src.lines().collect();
            let mask = test_line_mask(&lines);
            let mut hit = false;
            for (idx, line) in lines.iter().enumerate() {
                if mask[idx] {
                    continue;
                }
                if line_is_exempt(line) {
                    continue;
                }
                if line.contains(".state/logs/pathflow-events")
                    || line.contains("pathflow-events.jsonl")
                {
                    hit = true;
                    break;
                }
            }
            assert!(hit, "lint must flag literal when not masked/exempt");
        }
    }

    mod proptests {
        use proptest::prelude::*;

        use super::*;

        fn arb_event_type() -> impl Strategy<Value = String> {
            prop_oneof![
                Just("session_start".to_string()),
                Just("task_created".to_string()),
                Just("epic_created".to_string()),
                Just("begin_work".to_string()),
                Just("config_set".to_string()),
            ]
        }

        proptest! {
            #[test]
            fn event_serde_roundtrip(
                event_type in arb_event_type(),
                has_session in any::<bool>(),
            ) {
                let session_id = if has_session {
                    Some("ses-test".to_string())
                } else {
                    None
                };
                let mut data = HashMap::new();
                data.insert("key".to_string(), serde_json::json!("value"));
                let event = Event {
                    event_type: event_type.clone(),
                    timestamp: "2026-03-07T00:00:00Z".to_string(),
                    session_id,
                    worktree: None,
                    data,
                };
                let json = serde_json::to_string(&event).unwrap();
                let reparsed: Event = serde_json::from_str(&json).unwrap();
                prop_assert_eq!(&event.event_type, &reparsed.event_type);
                prop_assert_eq!(&event.timestamp, &reparsed.timestamp);
                prop_assert_eq!(&event.session_id, &reparsed.session_id);
            }
        }
    }
}
