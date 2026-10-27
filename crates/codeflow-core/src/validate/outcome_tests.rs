//! The Outcome section and epic Deliverables (TSK-435): a record created
//! after the project adopted the section must state its Outcome, a record
//! present at adoption warns, a project with no marker only warns, and a
//! closed record is never flagged.

use std::path::{Path, PathBuf};

use super::{validate_epic, validate_task, ValidateOptions};

const OLD: &str = "11111111-1111-4111-8111-111111111111";
const NEW: &str = "22222222-2222-4222-8222-222222222222";

/// The marker of a project that adopted the section with `uids` present.
fn marked(uids: &[&str]) -> String {
    let list = uids
        .iter()
        .map(|uid| format!("\"{uid}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "outcome_required_since = \"{}\"\noutcome_present_uids = [{list}]\n",
        "a".repeat(40)
    )
}

/// A project whose `.codeflow/project.toml` ends with `marker`.
fn project(marker: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
    let state = format!(
        "schema_version = 1\ntier = \"full\"\n{}",
        marker.unwrap_or_default()
    );
    std::fs::write(dir.path().join(".codeflow/project.toml"), state).unwrap();
    dir
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Task,
    Epic,
}

/// What a record carries besides its frontmatter.
#[derive(Clone, Copy)]
struct Body {
    outcome: Option<&'static str>,
    deliverables: bool,
}

const WITHOUT: Body = Body {
    outcome: None,
    deliverables: true,
};

const WITH: Body = Body {
    outcome: Some("after this merges, an adopter can do the thing"),
    deliverables: true,
};

fn write_record(root: &Path, kind: Kind, status: &str, uid: &str, body: Body) -> PathBuf {
    let outcome = body
        .outcome
        .map(|line| format!("## Outcome\n\n{line}\n\n"))
        .unwrap_or_default();
    let deliverables = if body.deliverables {
        "## Deliverables\n\n- the change: `src/lib.rs`\n\n"
    } else {
        ""
    };
    let (path, text) = match kind {
        Kind::Task => (
            root.join("project-management/tasks/TSK-001.md"),
            format!(
                "---\nid: TSK-001\nuid: {uid}\nepic_id: EPC-001\nstandalone_reason: null\n\
                 title: \"A task\"\nstatus: {status}\nwork_type: feat\nspecs: []\ndepends_on: []\n\
                 integration_target: main\ncreated: 2026-10-27\n---\n\n# TSK-001: A task\n\n\
                 ## Description\n\nSome change.\n\n{outcome}{deliverables}\
                 ## Acceptance Criteria\n\n- AC-1 it works\n"
            ),
        ),
        Kind::Epic => (
            root.join("project-management/epics/EPC-001.md"),
            format!(
                "---\nid: EPC-001\nuid: {uid}\ntitle: \"An epic\"\nstatus: {status}\n\
                 work_type: feat\ncapabilities: []\nadrs: []\nspecs: []\ncreated: 2026-10-27\n---\n\n\
                 # EPC-001: An epic\n\n## Summary\n\nSome outcome.\n\n{outcome}{deliverables}\
                 ## Acceptance Criteria\n\n- AC-1 it works\n"
            ),
        ),
    };
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    path
}

/// The Outcome and Deliverables errors and warnings of one record.
fn findings(
    marker: Option<&str>,
    kind: Kind,
    status: &str,
    uid: &str,
    body: Body,
) -> (Vec<String>, Vec<String>) {
    let dir = project(marker);
    let path = write_record(dir.path(), kind, status, uid, body);
    let opts = ValidateOptions::default();
    let (errors, warnings) = match kind {
        Kind::Task => validate_task(&path, &opts),
        Kind::Epic => validate_epic(&path, &opts),
    }
    .unwrap();
    let ours = |field: &str| field == "outcome" || field == "deliverables";
    (
        errors
            .iter()
            .filter(|e| ours(&e.field))
            .map(ToString::to_string)
            .collect(),
        warnings
            .iter()
            .filter(|w| ours(&w.field))
            .map(ToString::to_string)
            .collect(),
    )
}

const OPEN_TASK: &[&str] = &["todo", "blocked", "in_progress"];
const OPEN_EPIC: &[&str] = &["draft", "planning", "in_progress", "blocked"];

fn open(kind: Kind) -> &'static [&'static str] {
    if kind == Kind::Task {
        OPEN_TASK
    } else {
        OPEN_EPIC
    }
}

/// AC-2: a record whose uid the marker does not list is refused when its
/// Outcome is missing or holds only the template comment, naming the
/// section; the validator's caller prefixes the record's path.
#[test]
fn a_later_open_record_without_an_outcome_is_refused() {
    let marker = marked(&[OLD]);
    let comment_only = Body {
        outcome: Some("<!-- One line: after this merges, X can do Y. -->"),
        deliverables: true,
    };
    for kind in [Kind::Task, Kind::Epic] {
        for status in open(kind) {
            for body in [WITHOUT, comment_only] {
                let (errors, warnings) = findings(Some(&marker), kind, status, NEW, body);
                assert_eq!(errors.len(), 1, "{status}: {errors:?}");
                assert!(errors[0].contains("`## Outcome`"), "{errors:?}");
                assert!(warnings.is_empty(), "{status}: {warnings:?}");
            }
        }
    }
}

/// AC-2: a later record that states its Outcome is clean.
#[test]
fn a_later_record_with_an_outcome_is_clean() {
    let marker = marked(&[]);
    for kind in [Kind::Task, Kind::Epic] {
        for status in open(kind) {
            let (errors, warnings) = findings(Some(&marker), kind, status, NEW, WITH);
            assert!(errors.is_empty(), "{status}: {errors:?}");
            assert!(warnings.is_empty(), "{status}: {warnings:?}");
        }
    }
}

/// AC-3: an open record listed in the marker warns and is never refused,
/// whatever its `created` date: a record the older release made on the
/// morning of the upgrade is listed, so it only warns.
#[test]
fn an_open_record_present_at_adoption_warns() {
    let marker = marked(&[OLD, NEW]);
    for kind in [Kind::Task, Kind::Epic] {
        for status in open(kind) {
            let (errors, warnings) = findings(Some(&marker), kind, status, OLD, WITHOUT);
            assert!(errors.is_empty(), "{status}: {errors:?}");
            assert_eq!(warnings.len(), 1, "{status}: {warnings:?}");
            assert!(warnings[0].contains("`## Outcome`"), "{warnings:?}");
        }
    }
}

/// AC-3: with no marker every missing Outcome warns and nothing refuses.
#[test]
fn without_a_marker_a_missing_outcome_only_warns() {
    for kind in [Kind::Task, Kind::Epic] {
        for status in open(kind) {
            let (errors, warnings) = findings(None, kind, status, NEW, WITHOUT);
            assert!(errors.is_empty(), "{status}: {errors:?}");
            assert_eq!(warnings.len(), 1, "{status}: {warnings:?}");
            assert!(warnings[0].contains("`## Outcome`"), "{warnings:?}");
        }
    }
}

/// AC-3: a complete, cancelled or archived record says nothing, with or
/// without the section, in every marker state.
#[test]
fn a_closed_record_is_never_flagged() {
    let bare = Body {
        outcome: None,
        deliverables: false,
    };
    let markers = [None, Some(marked(&[OLD])), Some(marked(&[]))];
    let closed = [
        (Kind::Task, "complete"),
        (Kind::Task, "cancelled"),
        (Kind::Epic, "complete"),
        (Kind::Epic, "cancelled"),
        (Kind::Epic, "archived"),
    ];
    for marker in &markers {
        for (kind, status) in closed {
            for uid in [OLD, NEW] {
                for body in [bare, WITHOUT, WITH] {
                    let (errors, warnings) = findings(marker.as_deref(), kind, status, uid, body);
                    assert!(errors.is_empty(), "{status}: {errors:?}");
                    assert!(warnings.is_empty(), "{status}: {warnings:?}");
                }
            }
        }
    }
}

/// An epic created after adoption names its Deliverables like a task: an
/// open one without a filled section warns, and a filled one is clean.
#[test]
fn a_later_open_epic_needs_deliverables() {
    let marker = marked(&[OLD]);
    let no_deliverables = Body {
        outcome: WITH.outcome,
        deliverables: false,
    };
    for status in OPEN_EPIC {
        let (errors, warnings) = findings(Some(&marker), Kind::Epic, status, NEW, no_deliverables);
        assert!(errors.is_empty(), "{status}: {errors:?}");
        assert_eq!(warnings.len(), 1, "{status}: {warnings:?}");
        assert!(warnings[0].contains("`## Deliverables`"), "{warnings:?}");
        let (_, warnings) = findings(Some(&marker), Kind::Epic, status, NEW, WITH);
        assert!(warnings.is_empty(), "{status}: {warnings:?}");
    }
}

/// A marker that cannot be read fails closed: a record without the
/// section is refused rather than passed.
#[test]
fn an_unreadable_marker_refuses_a_missing_outcome() {
    let broken = "outcome_present_uids = \"yesterday\"\n";
    for kind in [Kind::Task, Kind::Epic] {
        let (errors, _) = findings(Some(broken), kind, "in_progress", OLD, WITHOUT);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("outcome_present_uids"), "{errors:?}");
        let (errors, _) = findings(Some(broken), kind, "in_progress", OLD, WITH);
        assert!(errors.is_empty(), "{errors:?}");
    }
}
