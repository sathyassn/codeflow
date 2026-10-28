//! The review document's id rules and the work graph's id rules agree
//! (TSK-1023, issue #356). The present crate keeps its own rule because it
//! does not depend on `codeflow-core`; this test is where the two meet.

use std::{fs, path::Path};

use codeflow_core::{
    ids::{Kind, RegId},
    workgraph::{
        is_valid_spec_format_id, is_valid_task_format_id, light_paths::next_adr_id, next_spec_id,
        next_task_id,
    },
};
use codeflow_present::document::{parse_document, ParsedDocument};

const DOCUMENT_SCHEMAS: [&str; 4] = [
    "document-v1.schema.json",
    "document-v2.schema.json",
    "session-history-v1.schema.json",
    "session-history-v2.schema.json",
];

/// The fields a review document's provenance can carry, with the ids a
/// sampled set holds at three, four and five digits and the shapes the work
/// graph refuses.
const SAMPLES: [(&str, &[&str]); 3] = [
    (
        "task_id",
        &[
            "TSK-001",
            "TSK-999",
            "TSK-1000",
            "TSK-1023",
            "TSK-9999",
            "TSK-10000",
            "TSK-12345",
            "TSK-022-013",
            "TSK-999-999",
            "TSK-1000-0001",
            "TSK-12345-67890",
            "TSK-",
            "TSK-1",
            "TSK-99",
            "TSK-0x12",
            "TSK-+001",
            "TSK-001-",
            "TSK--001",
            "TSK-001-1",
            "TSK-1-001",
            "TSK-001-001-001",
            "TSK-001\n",
            "TSK-001 ",
            "tsk-001",
            "SPC-001",
        ],
    ),
    (
        "spec_id",
        &[
            "SPC-001",
            "SPC-999",
            "SPC-1000",
            "SPC-9999",
            "SPC-10000",
            "SPC-",
            "SPC-1",
            "SPC-12",
            "SPC-12a",
            "SPC-001-001",
            "SPC-001\n",
            "TSK-001",
        ],
    ),
    (
        "adr_id",
        &[
            "ADR-0001",
            "ADR-0050",
            "ADR-9999",
            "ADR-10000",
            "ADR-123456",
            "ADR-",
            "ADR-1",
            "ADR-12",
            "ADR-123",
            "ADR-12a4",
            "ADR-0001-001",
            "ADR-0001\n",
            "TSK-0001",
        ],
    ),
];

/// What the work graph says about an id of the given provenance field.
fn work_graph_accepts(field: &str, id: &str) -> bool {
    match field {
        "task_id" => is_valid_task_format_id(id),
        "spec_id" => is_valid_spec_format_id(id),
        "adr_id" => RegId::parse(id).is_some_and(|parsed| parsed.kind() == Kind::Adr),
        other => panic!("unexpected provenance field {other}"),
    }
}

/// What the review document validator says.
fn validator_accepts(field: &str, id: &str) -> bool {
    let document = serde_json::json!({
        "schema_version": 1,
        "title": "Review",
        "provenance": { field: id },
        "blocks": [{ "type": "narrative", "id": "summary", "markdown": "x" }],
    });
    matches!(
        parse_document(document.to_string().as_bytes()),
        Ok(ParsedDocument::Supported(_))
    )
}

/// The `pattern` each shipped schema holds for the field, found wherever the
/// schema keeps its provenance object.
fn schema_patterns(schema: &serde_json::Value, field: &str) -> Vec<String> {
    fn walk(value: &serde_json::Value, field: &str, found: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(pattern) = map
                    .get("properties")
                    .and_then(|properties| properties.get(field))
                    .and_then(|property| property.get("pattern"))
                    .and_then(serde_json::Value::as_str)
                {
                    found.push(pattern.to_string());
                }
                for child in map.values() {
                    walk(child, field, found);
                }
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    walk(child, field, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    walk(schema, field, &mut found);
    found
}

fn schemas_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/present/schemas")
}

fn assert_all_agree(field: &str, id: &str) {
    let expected = work_graph_accepts(field, id);
    assert_eq!(
        validator_accepts(field, id),
        expected,
        "the review document validator disagrees with the work graph on {field} {id:?} (work graph: {expected})"
    );
    for name in DOCUMENT_SCHEMAS {
        let schema: serde_json::Value =
            serde_json::from_slice(&fs::read(schemas_dir().join(name)).unwrap()).unwrap();
        let patterns = schema_patterns(&schema, field);
        assert_eq!(patterns.len(), 1, "{name} should hold one {field} pattern");
        let pattern = regex::Regex::new(&patterns[0]).unwrap();
        assert_eq!(
            pattern.is_match(id),
            expected,
            "{name} pattern {} disagrees with the work graph on {field} {id:?} (work graph: {expected})",
            patterns[0]
        );
    }
}

#[test]
fn the_validator_and_every_schema_agree_with_the_work_graph_on_sampled_ids() {
    for (field, ids) in SAMPLES {
        for id in ids {
            assert_all_agree(field, id);
        }
    }
}

#[test]
fn every_id_the_allocators_issue_is_accepted_by_the_validator_and_every_schema() {
    let temp = tempfile::tempdir().unwrap();
    let pm = temp.path().join("project-management");
    let decisions = temp.path().join("docs/decisions");
    fs::create_dir_all(pm.join("tasks")).unwrap();
    fs::create_dir_all(pm.join("specs")).unwrap();
    fs::create_dir_all(&decisions).unwrap();

    let mut issued = Vec::new();
    for top in ["998", "999", "1000", "9999", "10000", "99999"] {
        fs::write(pm.join(format!("tasks/TSK-{top}.md")), "task").unwrap();
        fs::write(pm.join(format!("specs/SPC-{top}.md")), "spec").unwrap();
        issued.push(("task_id", next_task_id(&pm).unwrap()));
        issued.push(("spec_id", next_spec_id(&pm).unwrap()));
    }
    for top in ["0001", "0999", "9998", "9999", "10000", "10001", "123456"] {
        fs::write(decisions.join(format!("ADR-{top}-decision.md")), "adr").unwrap();
        issued.push(("adr_id", next_adr_id(temp.path()).unwrap()));
    }

    for (field, id) in &issued {
        assert!(
            work_graph_accepts(field, id),
            "the work graph rejects its own {field} {id}"
        );
        assert_all_agree(field, id);
    }
    // The widths the allocators reach: a fourth and a fifth digit.
    for expected in [
        "TSK-999",
        "TSK-1000",
        "TSK-10000",
        "TSK-100000",
        "SPC-1000",
        "SPC-10000",
        "ADR-10000",
        "ADR-10001",
        "ADR-123457",
    ] {
        assert!(
            issued.iter().any(|(_, id)| id == expected),
            "no allocator issued {expected}: {issued:?}"
        );
    }
}

#[cfg(unix)]
mod commands {
    use std::{
        fs,
        path::Path,
        process::{Command, Output},
    };

    fn codeflow(project: &Path, home: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_codeflow"))
            .args(args)
            .current_dir(project)
            .env("HOME", home)
            .env("XDG_STATE_HOME", home.join("state"))
            .env(
                codeflow_present::service::OWNER_PID_ENV,
                std::process::id().to_string(),
            )
            .output()
            .unwrap()
    }

    fn succeeded(output: &Output) -> String {
        assert!(
            output.status.success(),
            "command failed\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout.clone()).unwrap()
    }

    fn document(title: &str, provenance: &serde_json::Value) -> String {
        serde_json::json!({
            "schema_version": 1,
            "title": title,
            "provenance": provenance,
            "blocks": [{ "type": "narrative", "id": "summary", "markdown": "x" }],
        })
        .to_string()
    }

    #[test]
    fn check_open_and_update_accept_four_and_five_digit_ids_and_still_refuse_short_ones() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let home = temp.path().join("home");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&home).unwrap();
        assert!(Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&project)
            .status()
            .unwrap()
            .success());

        let provenance = serde_json::json!({
            "task_id": "TSK-1000",
            "spec_id": "SPC-1000",
            "adr_id": "ADR-10000",
        });
        let first = project.join("first.json");
        let second = project.join("second.json");
        fs::write(&first, document("First review", &provenance)).unwrap();
        let later = serde_json::json!({
            "task_id": "TSK-12345",
            "spec_id": "SPC-12345",
            "adr_id": "ADR-123456",
        });
        fs::write(&second, document("Second review", &later)).unwrap();

        for path in [&first, &second] {
            let checked = succeeded(&codeflow(
                &project,
                &home,
                &["present", "check", "--file", path.to_str().unwrap()],
            ));
            assert!(checked.contains("\"valid\":true"), "{checked}");
        }

        let opened = succeeded(&codeflow(
            &project,
            &home,
            &["present", "open", first.to_str().unwrap(), "--no-launch"],
        ));
        let session_id = opened.split_whitespace().nth(1).unwrap().to_string();
        succeeded(&codeflow(
            &project,
            &home,
            &["present", "update", &session_id, second.to_str().unwrap()],
        ));
        let history: serde_json::Value = serde_json::from_str(&succeeded(&codeflow(
            &project,
            &home,
            &["present", "history", &session_id],
        )))
        .unwrap();
        assert_eq!(history["session"]["current_revision"], 2);
        assert_eq!(history["session"]["provenance"], later);
        succeeded(&codeflow(
            &project,
            &home,
            &["present", "close", &session_id],
        ));

        for (field, id) in [
            ("task_id", "TSK-99"),
            ("spec_id", "SPC-12"),
            ("adr_id", "ADR-123"),
        ] {
            let bad = project.join("bad.json");
            fs::write(&bad, document("Bad", &serde_json::json!({ field: id }))).unwrap();
            let output = codeflow(
                &project,
                &home,
                &["present", "check", "--file", bad.to_str().unwrap()],
            );
            assert_eq!(output.status.code(), Some(9), "{field} {id} was accepted");
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.contains(&format!("provenance {field}")), "{stdout}");
        }
    }
}
