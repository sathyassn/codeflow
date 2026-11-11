//! Session fragment identity: the one check every reader of a
//! `{type}-{key}.jsonl` session fragment makes before it trusts the file.
//!
//! A fragment's name claims a session; its records name one too. The
//! compactor merges and deletes a fragment by its name, and a
//! `session_end` in it marks that session complete, so a file whose
//! records belong to another session, to two sessions or to none is
//! refused and left where it is.

use std::ffi::OsStr;
use std::path::Path;

use super::{session_file_key, Event, LedgerError, SESSION_KEY_PREFIX};

/// The prefix of a fragment key releases before 4.0.0 wrote: the key is
/// the recorded `session_id` itself.
pub const LEGACY_KEY_PREFIX: &str = "ses-";

/// The session key of fragment file `file_name` of ledger type
/// `type_name` (`{type}-{key}.jsonl`), when the key is in either
/// namespace: `sid-…` or the pre-4.0.0 `ses-…`. Any other name, such as
/// the base file, a lock or a temporary file, is not a session fragment.
///
/// # Errors
///
/// A name in either namespace that is not UTF-8, so its key cannot be
/// read: the fragment cannot be checked, so it is refused.
pub fn fragment_key<'a>(
    type_name: &str,
    file_name: &'a OsStr,
) -> Result<Option<&'a str>, LedgerError> {
    let bytes = file_name.as_encoded_bytes();
    let in_namespace = [SESSION_KEY_PREFIX, LEGACY_KEY_PREFIX]
        .iter()
        .any(|prefix| bytes.starts_with(format!("{type_name}-{prefix}").as_bytes()));
    let jsonl = Path::new(file_name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"));
    if !in_namespace || !jsonl {
        return Ok(None);
    }
    let name = file_name.to_str().ok_or_else(|| {
        LedgerError::Corrupt("cannot read a session fragment's name as UTF-8".into())
    })?;
    let key = &name[type_name.len() + 1..name.len() - ".jsonl".len()];
    Ok(Some(key))
}

/// Check that every record of fragment `path`, named by `key`, belongs to
/// the session the name claims, and return that session's recorded id
/// (`None` for a fragment with no records).
///
/// Each record must carry a `session_id`, all records must carry the same
/// one, and the name must be that id's: [`session_file_key`] of it for a
/// `sid-` key, the id itself for a pre-4.0.0 `ses-` key.
///
/// # Errors
///
/// [`LedgerError::Corrupt`] naming the file when a record has no
/// `session_id`, two records name different sessions, or the records'
/// session is not the one the name claims.
pub fn verify_fragment(
    path: &Path,
    key: &str,
    events: &[Event],
) -> Result<Option<String>, LedgerError> {
    let refused = |reason: String| {
        LedgerError::Corrupt(format!(
            "{}: session fragment refused, {reason}; the file is kept and nothing is merged",
            path.display()
        ))
    };
    let mut recorded: Option<&str> = None;
    for (index, event) in events.iter().enumerate() {
        let Some(id) = event.session_id.as_deref() else {
            return Err(refused(format!("record {} has no session_id", index + 1)));
        };
        match recorded {
            Some(first) if first != id => {
                return Err(refused(format!(
                    "its records name two sessions, {first} and {id}"
                )));
            }
            Some(_) => {}
            None => recorded = Some(id),
        }
    }
    let Some(id) = recorded else {
        return Ok(None);
    };
    let expected = if key.starts_with(SESSION_KEY_PREFIX) {
        session_file_key(id)
    } else {
        id.to_string()
    };
    if expected != key {
        return Err(refused(format!(
            "its records are session {id}, whose file key is {expected}, not {key}"
        )));
    }
    Ok(Some(id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(sid: Option<&str>) -> Event {
        Event {
            event_type: "task_created".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            session_id: sid.map(str::to_string),
            worktree: None,
            data: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn only_names_in_either_namespace_are_fragments() {
        fn key(name: &str) -> Option<&str> {
            fragment_key("work-graph", OsStr::new(name)).unwrap()
        }
        assert_eq!(key("work-graph-ses-a.jsonl"), Some("ses-a"));
        assert_eq!(key("work-graph-sid-00ff.jsonl"), Some("sid-00ff"));
        for other in [
            "work-graph.jsonl",
            "work-graph-ses-a.jsonl.lock",
            "work-graph-other.jsonl",
            ".compaction.lock",
            "sessions-ses-a.jsonl",
        ] {
            assert_eq!(key(other), None, "{other}");
        }
    }

    #[test]
    fn a_fragment_verifies_only_when_every_record_is_the_named_session() {
        let path = Path::new("f.jsonl");
        let sid_key = session_file_key("ses-A");
        let good = [event(Some("ses-A")), event(Some("ses-A"))];
        assert_eq!(
            verify_fragment(path, &sid_key, &good).unwrap().as_deref(),
            Some("ses-A")
        );
        assert_eq!(
            verify_fragment(path, "ses-A", &good).unwrap().as_deref(),
            Some("ses-A")
        );
        assert_eq!(verify_fragment(path, &sid_key, &[]).unwrap(), None);
        for key in [sid_key.as_str(), "ses-A"] {
            let other = [event(Some("ses-B"))];
            let missing = [event(Some("ses-A")), event(None)];
            let mixed = [event(Some("ses-A")), event(Some("ses-B"))];
            for (case, events) in [
                ("mismatched", &other[..]),
                ("missing", &missing[..]),
                ("mixed", &mixed[..]),
            ] {
                let err = verify_fragment(path, key, events).unwrap_err();
                assert!(matches!(err, LedgerError::Corrupt(_)), "{key} {case}");
            }
        }
    }
}
