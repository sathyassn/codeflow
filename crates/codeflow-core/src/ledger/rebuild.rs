//! Ledger rebuild: read base + all fragments for a type, return sorted events.
//!
//! Rebuildable-cache support (charter D17): any index built over the ledger
//! (e.g., recall's FTS index) reconstructs from these events.

use std::fs;
use std::path::Path;

use super::files;
use super::Event;
use super::LedgerError;

/// Read all events for a single ledger type (base + fragments), sorted by timestamp.
///
/// Algorithm:
/// 1. Read base file: `{type}/{type}.jsonl`
/// 2. Glob fragments: `{type}/{type}-ses-*.jsonl`
/// 3. Parse all events from all files
/// 4. Sort by timestamp (stable sort preserves intra-file order)
///
/// Unreadable files and corrupt lines refuse the rebuild.
///
/// # Errors
///
/// Returns `LedgerError::Io` if the directory cannot be read.
pub fn rebuild_ledger_type(ledger_dir: &Path, type_name: &str) -> Result<Vec<Event>, LedgerError> {
    let subdir = ledger_dir.join(type_name);
    if !subdir.try_exists()? {
        return Ok(Vec::new());
    }

    let mut events = Vec::new();

    // Read base file.
    let base_file = subdir.join(format!("{type_name}.jsonl"));
    if base_file.try_exists()? {
        read_events_from_file(&base_file, &mut events)?;
    }

    // Read fragment files.
    let prefix = format!("{type_name}-ses-");
    for entry in fs::read_dir(&subdir)? {
        let entry = entry?;
        let path = entry.path();
        if entry
            .file_name()
            .as_encoded_bytes()
            .starts_with(prefix.as_bytes())
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
        {
            read_events_from_file(&path, &mut events)?;
        }
    }

    // Stable sort by timestamp preserves intra-file order for equal timestamps.
    events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

    Ok(events)
}

/// Read all events across all ledger types.
///
/// # Errors
///
/// Returns `LedgerError::Io` on directory read failure.
pub fn rebuild_all(
    ledger_dir: &Path,
) -> Result<std::collections::HashMap<String, Vec<Event>>, LedgerError> {
    let mut result = std::collections::HashMap::new();
    for type_name in files::ALL {
        let events = rebuild_ledger_type(ledger_dir, type_name)?;
        if !events.is_empty() {
            result.insert(type_name.to_string(), events);
        }
    }
    Ok(result)
}

/// Parse JSONL events from a file, appending to `events`.
///
/// Unreadable files and corrupt lines refuse the rebuild.
fn read_events_from_file(path: &Path, events: &mut Vec<Event>) -> Result<(), LedgerError> {
    let content = fs::read_to_string(path)?;
    for (i, line) in content.split_terminator('\n').enumerate() {
        if line.trim_matches([' ', '\t', '\r']).is_empty() {
            continue;
        }
        let event = serde_json::from_str::<Event>(line).map_err(|error| {
            LedgerError::Corrupt(format!("{}:{}: {error}", path.display(), i + 1))
        })?;
        events.push(event);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_event(path: &Path, event_type: &str, ts: &str, sid: Option<&str>) {
        use std::io::Write;
        let sid_json = sid
            .map(|s| format!(r#","session_id":"{s}""#))
            .unwrap_or_default();
        let line = format!(r#"{{"event":"{event_type}","timestamp":"{ts}"{sid_json}}}"#);
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(f, "{line}").unwrap();
    }

    #[test]
    fn test_rebuild_empty_type() {
        let dir = tempfile::tempdir().unwrap();
        let events = rebuild_ledger_type(dir.path(), "work-graph").unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_rebuild_base_only() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("sessions");
        fs::create_dir_all(&subdir).unwrap();

        let base = subdir.join("sessions.jsonl");
        write_event(
            &base,
            "session_start",
            "2026-01-01T00:00:00Z",
            Some("ses-1"),
        );
        write_event(&base, "session_end", "2026-01-01T01:00:00Z", Some("ses-1"));

        let events = rebuild_ledger_type(dir.path(), "sessions").unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "session_start");
        assert_eq!(events[1].event_type, "session_end");
    }

    #[test]
    fn test_rebuild_base_plus_fragments() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("work-graph");
        fs::create_dir_all(&subdir).unwrap();

        // Base file with older event.
        let base = subdir.join("work-graph.jsonl");
        write_event(&base, "task_created", "2026-01-01T00:00:00Z", None);

        // Fragment with newer event.
        let frag = subdir.join("work-graph-ses-abc.jsonl");
        write_event(
            &frag,
            "task_status_changed",
            "2026-01-02T00:00:00Z",
            Some("ses-abc"),
        );

        let events = rebuild_ledger_type(dir.path(), "work-graph").unwrap();
        assert_eq!(events.len(), 2);
        // Should be sorted by timestamp.
        assert_eq!(events[0].timestamp, "2026-01-01T00:00:00Z");
        assert_eq!(events[1].timestamp, "2026-01-02T00:00:00Z");
    }

    #[test]
    fn test_rebuild_refuses_corrupt_lines() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("config");
        fs::create_dir_all(&subdir).unwrap();

        let base = subdir.join("config.jsonl");
        fs::write(
            &base,
            r#"{"event":"config_set","timestamp":"2026-01-01T00:00:00Z"}
NOT VALID JSON
{"event":"config_updated","timestamp":"2026-01-02T00:00:00Z"}
"#,
        )
        .unwrap();

        assert!(rebuild_ledger_type(dir.path(), "config").is_err());
    }

    #[test]
    fn test_rebuild_ignores_lock_files() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("sessions");
        fs::create_dir_all(&subdir).unwrap();

        let base = subdir.join("sessions.jsonl");
        write_event(&base, "session_start", "2026-01-01T00:00:00Z", None);

        // Lock file should not be treated as a fragment.
        fs::write(subdir.join("sessions-ses-abc.jsonl.lock"), "").unwrap();

        let events = rebuild_ledger_type(dir.path(), "sessions").unwrap();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_rebuild_all_multiple_types() {
        let dir = tempfile::tempdir().unwrap();

        let sessions_dir = dir.path().join("sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        write_event(
            &sessions_dir.join("sessions.jsonl"),
            "session_start",
            "2026-01-01T00:00:00Z",
            None,
        );

        let wg_dir = dir.path().join("work-graph");
        fs::create_dir_all(&wg_dir).unwrap();
        write_event(
            &wg_dir.join("work-graph.jsonl"),
            "task_created",
            "2026-01-01T00:00:00Z",
            None,
        );

        let result = rebuild_all(dir.path()).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains_key("sessions"));
        assert!(result.contains_key("work-graph"));
    }
}
