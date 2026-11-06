//! `codeflow status --scope WS-NNN`: the work a coordination stream's scope
//! links name is the scope, and a stream that names no work is refused
//! (TSK-1095 AC-16). The ledger fixture writes real ledger events.

use codeflow_core::coord::fixture;
use codeflow_core::coord::store::Paths;

use super::*;

fn paths(fx: &Fixture) -> Paths {
    Paths::discover(&fx.root).unwrap().0
}

/// A second epic with one task, on the fetched `main`, so a stream can own
/// one epic and one task of another epic.
fn add_second_epic(fx: &Fixture) {
    write(
        &fx.root,
        "project-management/epics/EPC-002.md",
        &EPIC.replace("EPC-001", "EPC-002"),
    );
    write(
        &fx.root,
        "project-management/tasks/TSK-006.md",
        &task_record("TSK-006", "todo", "", "").replace("EPC-001", "EPC-002"),
    );
    git(&fx.root, &["add", "."]);
    git(&fx.root, &["commit", "-qm", "docs: plan the second epic"]);
    git(
        &fx.root,
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
}

fn task_ids(value: &Value) -> Vec<String> {
    value["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["id"].as_str().unwrap().to_string())
        .collect()
}

fn refused(output: &Output, needle: &str) {
    assert_eq!(output.status.code(), Some(2), "{}", text(output));
    let shown = text(output);
    assert!(shown.contains(needle), "{needle}: {shown}");
}

#[test]
fn a_stream_scope_takes_the_epic_and_task_its_links_name() {
    let fx = fixture();
    add_second_epic(&fx);
    fixture::open_stream_with_scope(
        &paths(&fx),
        "WS-001",
        "c1",
        &["EPC-002", "TSK-001", "the status report"],
    )
    .unwrap();
    let value = fx.json(&["--scope", "WS-001"]);
    assert_eq!(value["scope"]["kind"], "stream");
    assert_eq!(value["scope"]["names"], serde_json::json!(["WS-001"]));
    assert_eq!(task_ids(&value), ["TSK-001", "TSK-006"]);
    assert_eq!(value["scope"]["tasks"], 2);
    assert_eq!(value["scope"]["percent"], 0.0);
    let schema: Value = serde_json::from_str(include_str!(
        "../../../../assets/base/status/schemas/report-v1.schema.json"
    ))
    .unwrap();
    let errors = validate(&schema, &schema, &value, "$");
    assert!(errors.is_empty(), "{errors:#?}");

    let view = fx.cli(&["status", "--scope", "WS-001"]);
    assert!(view.status.success(), "{}", text(&view));
    assert!(text(&view).contains("Progress: stream WS-001"));
}

#[test]
fn an_unknown_stream_is_refused_by_name() {
    let fx = fixture();
    refused(
        &fx.cli(&["status", "--json", "--scope", "WS-009"]),
        "unknown stream WS-009",
    );
    fixture::open_stream_with_scope(&paths(&fx), "WS-001", "c1", &["TSK-001"]).unwrap();
    refused(
        &fx.cli(&["status", "--json", "--scope", "WS-009"]),
        "unknown stream WS-009",
    );
    refused(
        &fx.cli(&["status", "--json", "--scope", "EPC-001"]),
        "`EPC-001` is not a stream id",
    );
}

#[test]
fn a_stream_with_no_work_links_is_refused_by_name() {
    let fx = fixture();
    fixture::open_stream(&paths(&fx), "WS-001", "c1").unwrap();
    refused(
        &fx.cli(&["status", "--json", "--scope", "WS-001"]),
        "stream WS-001 has no work links",
    );
}

/// A link to a record that is not here must not shrink the denominator.
#[test]
fn a_link_with_no_record_leaves_the_percent_unknown() {
    let fx = fixture();
    fixture::open_stream_with_scope(&paths(&fx), "WS-001", "c1", &["TSK-001", "TSK-404"]).unwrap();
    let value = fx.json(&["--scope", "WS-001"]);
    assert_eq!(value["scope"]["percent"], "unknown");
    let reason = value["scope"]["unknown_reason"].as_str().unwrap();
    assert!(reason.contains("WS-001 links TSK-404"), "{reason}");
}

#[test]
fn the_stream_scope_excludes_the_other_scope_flags() {
    let fx = fixture();
    for other in [
        ["--task", "TSK-001"],
        ["--epic", "EPC-001"],
        ["--line", "main"],
    ] {
        let output = fx.cli(&["status", "--scope", "WS-001", other[0], other[1]]);
        refused(&output, "cannot be used with");
    }
}

/// A damaged ledger may have lost later events, so it can neither name a
/// stream's scope nor count as a checked source of owed items.
#[test]
fn a_damaged_ledger_is_never_read_as_checked() {
    let fx = fixture();
    let paths = paths(&fx);
    fixture::open_stream_with_scope(&paths, "WS-001", "c1", &["TSK-001"]).unwrap();
    let log = std::fs::read_to_string(&paths.events).unwrap();
    let mut lines: Vec<&str> = log.lines().collect();
    lines.insert(lines.len() - 1, "not json");
    std::fs::write(&paths.events, lines.join("\n") + "\n").unwrap();

    refused(
        &fx.cli(&["status", "--json", "--scope", "WS-001"]),
        "the coordination ledger is damaged",
    );
    let value = fx.json(&["--task", "TSK-001"]);
    assert_eq!(value["issues"]["operator"]["state"], "not established");
    let unchecked = value["issues"]["operator"]["unchecked"].to_string();
    assert!(unchecked.contains("ledger"), "{unchecked}");
}

/// Workers come from the ledger through the CLI when it records them.
#[test]
fn ledger_workers_reach_the_view_through_the_cli() {
    let fx = fixture();
    let paths = paths(&fx);
    fixture::open_stream_with_scope(&paths, "WS-001", "c1", &["TSK-001"]).unwrap();
    let worker = fixture::open_lane(&paths, "WS-001", "c1", "s1").unwrap();
    let value = fx.json(&["--scope", "WS-001"]);
    assert_eq!(value["workers"]["source"], "ledger");
    assert_eq!(value["workers"]["workers"][0]["id"], worker.as_str());
    assert_eq!(value["workers"]["workers"][0]["stream"], "WS-001");
}
