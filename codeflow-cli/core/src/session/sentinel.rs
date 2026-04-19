//! PathFlow sentinel readers — non-feature-gated, pure filesystem helpers.
//!
//! Sentinels are empty marker files at
//! `.state/sentinels/pathflow/{session-id}/pathflow-pf-N` written by the
//! checkpoint hook when phase N completes. This module provides the
//! read-side helpers used by both the status TUIs (`tui::data`) and the
//! autorun worker (`autorun::worker`). It deliberately lives outside the
//! `tui` feature gate so the worker can always resolve it, regardless of
//! whether the binary is built with the `tui` feature enabled.

use std::path::Path;

/// Read the latest PathFlow phase from sentinel files.
///
/// When `session_id` is provided, only scans that session's sentinel subdir.
/// Otherwise scans all session subdirs (returns the highest phase across all).
///
/// Returns `Some("PF{n}")` for the highest `n` found (1–255 allowed by
/// `u8::parse`, PathFlow realistically only emits 1–7), or `None` when the
/// sentinel directory is absent, unreadable, or empty.
pub fn read_latest_phase(
    project_dir: &Path,
    worktree_path: Option<&str>,
    session_id: Option<&str>,
) -> Option<String> {
    let base = worktree_path.map_or_else(|| project_dir.to_path_buf(), std::path::PathBuf::from);

    let sentinel_dir = base.join(".state/sentinels/pathflow");
    let entries = match std::fs::read_dir(&sentinel_dir) {
        Ok(e) => e,
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            eprintln!(
                "warn: failed to read sentinel dir {}: {e}",
                sentinel_dir.display()
            );
            return None;
        }
    };

    // Look for session directories, then find the latest pf-N sentinel.
    let mut latest_phase: Option<u8> = None;
    for entry in entries.flatten() {
        let session_dir = entry.path();
        if !session_dir.is_dir() {
            continue;
        }
        // Filter to target session when specified.
        if let Some(sid) = session_id {
            if let Some(dir_name) = session_dir.file_name().and_then(|n| n.to_str()) {
                if dir_name != sid {
                    continue;
                }
            }
        }
        if let Ok(sentinels) = std::fs::read_dir(&session_dir) {
            for sentinel in sentinels.flatten() {
                let name = sentinel.file_name();
                let name = name.to_string_lossy();
                if let Some(rest) = name.strip_prefix("pathflow-pf-") {
                    if let Ok(n) = rest.parse::<u8>() {
                        latest_phase = Some(latest_phase.map_or(n, |cur| cur.max(n)));
                    }
                }
            }
        }
    }

    latest_phase.map(|n| format!("PF{n}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_latest_phase_no_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result = read_latest_phase(tmp.path(), None, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_read_latest_phase_with_sentinels() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = tmp.path().join(".state/sentinels/pathflow/ses-test-123");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("pathflow-pf-1"), "").unwrap();
        std::fs::write(session_dir.join("pathflow-pf-3"), "").unwrap();

        let result = read_latest_phase(tmp.path(), None, None);
        assert_eq!(result, Some("PF3".to_string()));
    }

    #[test]
    fn test_read_latest_phase_with_worktree_path() {
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("worktree");
        let session_dir = wt.join(".state/sentinels/pathflow/ses-wt-001");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("pathflow-pf-1"), "").unwrap();
        std::fs::write(session_dir.join("pathflow-pf-4"), "").unwrap();

        let result = read_latest_phase(tmp.path(), Some(wt.to_str().unwrap()), None);
        assert_eq!(result, Some("PF4".to_string()));
    }

    // Multi-session sentinel isolation — with session_id filter.
    #[test]
    fn test_read_latest_phase_session_isolation() {
        let tmp = tempfile::tempdir().unwrap();
        // Create two session sentinel dirs.
        let ses1 = tmp.path().join(".state/sentinels/pathflow/ses-AAA");
        let ses2 = tmp.path().join(".state/sentinels/pathflow/ses-BBB");
        std::fs::create_dir_all(&ses1).unwrap();
        std::fs::create_dir_all(&ses2).unwrap();
        std::fs::write(ses1.join("pathflow-pf-3"), "").unwrap();
        std::fs::write(ses2.join("pathflow-pf-6"), "").unwrap();

        // Without filter: returns highest across all (PF6).
        let all = read_latest_phase(tmp.path(), None, None);
        assert_eq!(all, Some("PF6".to_string()));

        // With filter: returns only target session's data.
        let ses1_only = read_latest_phase(tmp.path(), None, Some("ses-AAA"));
        assert_eq!(ses1_only, Some("PF3".to_string()));

        let ses2_only = read_latest_phase(tmp.path(), None, Some("ses-BBB"));
        assert_eq!(ses2_only, Some("PF6".to_string()));

        // Nonexistent session returns None.
        let none = read_latest_phase(tmp.path(), None, Some("ses-ZZZ"));
        assert!(none.is_none());
    }

    // Sentinel read with nonexistent dir returns gracefully.
    #[test]
    fn test_read_latest_phase_nonexistent_returns_none() {
        let result = read_latest_phase(std::path::Path::new("/nonexistent/path"), None, None);
        assert!(result.is_none());
    }
}
