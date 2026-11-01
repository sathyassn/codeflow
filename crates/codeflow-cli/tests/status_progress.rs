//! `codeflow status --task/--epic/--line/--json`: percent complete,
//! location, order, lanes, windows, issues and pace over fixture
//! repositories (TSK-1095). The forge is a fake `gh` on `PATH`.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const AS_OF: &str = "2026-11-01T12:00:00Z";

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().into()
}

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    bin: PathBuf,
    log: PathBuf,
}

impl Fixture {
    fn cli(&self, args: &[&str]) -> Output {
        self.cli_in(&self.root, args)
    }

    fn cli_in(&self, dir: &Path, args: &[&str]) -> Output {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_codeflow"))
            .current_dir(dir)
            .args(args)
            .env("PATH", path)
            .env("CODEFLOW_STATUS_AS_OF", AS_OF)
            .env("FAKE_GH_LOG", &self.log)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let mut all = vec!["status", "--json"];
        all.extend_from_slice(args);
        let output = self.cli(&all);
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// Replace the fake forge's answer.
    fn gh(&self, body: &str) {
        let script = self.bin.join("gh");
        std::fs::write(
            &script,
            format!("#!/bin/sh\necho \"$@\" >> \"$FAKE_GH_LOG\"\n{body}\n"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn task_record(id: &str, status: &str, depends: &str, extra: &str) -> String {
    format!(
        "---\nid: {id}\nepic_id: EPC-001\nintegration_target: main\ntitle: work {id}\nstatus: {status}\nwork_type: feat\nspecs: []\ndepends_on: [{depends}]\ncreated: 2026-10-30\n{extra}---\n\n## Outcome\n\nAfter this merges, {id} works.\n\n## Description\n\nWork for {id}.\n\n## Acceptance Criteria\n\n- AC-1 When run, the command shall succeed.\n"
    )
}

const EPIC: &str = "---\nid: EPC-001\ntitle: Demo epic\nstatus: active\ncreated: 2026-10-29\n---\n\n## Outcome\n\nThe demo ships.\n\n## Core work\n\n- TSK-001, TSK-002 and TSK-003.\n\n## Follow-on\n\n- TSK-004 and TSK-005.\n";

const PULLS: &str = r#"cat <<'JSON'
[{"number":7,"title":"TSK-003 work","headRefName":"task/TSK-003-work","headRefOid":"0000000000000000000000000000000000000003","baseRefName":"main","isDraft":false,"statusCheckRollup":[{"conclusion":"FAILURE"}]}]
JSON"#;

/// One epic with a task at each stage: TSK-001 not started, TSK-002 in
/// progress in its own worktree, TSK-003 in review with a red check,
/// TSK-004 landed and TSK-005 cancelled; TSK-002 waits on nothing, and
/// TSK-003 depends on TSK-001.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let root = base.join("repo");
    let origin = base.join("origin.git");
    let bin = base.join("bin");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    git(&base, &["init", "-q", "--bare", "origin.git"]);
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@example.test"]);
    write(&root, "project-management/epics/EPC-001.md", EPIC);
    write(
        &root,
        "project-management/tasks/TSK-001.md",
        &task_record("TSK-001", "todo", "", ""),
    );
    write(
        &root,
        "project-management/tasks/TSK-002.md",
        &task_record("TSK-002", "todo", "", "theme: Speed\n"),
    );
    write(
        &root,
        "project-management/tasks/TSK-003.md",
        &task_record("TSK-003", "todo", "TSK-001", ""),
    );
    write(
        &root,
        "project-management/tasks/TSK-004.md",
        &task_record("TSK-004", "complete", "", ""),
    );
    write(
        &root,
        "project-management/tasks/TSK-005.md",
        &task_record("TSK-005", "cancelled", "", ""),
    );
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "docs: plan the epic"]);
    git(
        &root,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&root, &["push", "-q", "origin", "main"]);
    git(&root, &["fetch", "-q", "origin"]);
    for (id, worktree) in [("TSK-002", true), ("TSK-003", false)] {
        let branch = format!("task/{id}-work");
        git(&root, &["switch", "-qc", &branch]);
        write(&root, &format!("src/{id}.rs"), "pub fn work() {}\n");
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "feat: build the outcome"]);
        git(&root, &["switch", "-q", "main"]);
        if worktree {
            let path = base.join(format!("wt-{id}"));
            git(
                &root,
                &["worktree", "add", "-q", path.to_str().unwrap(), &branch],
            );
        }
    }
    git(
        &root,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/acme/demo.git",
        ],
    );
    let fixture = Fixture {
        log: base.join("gh.log"),
        _dir: dir,
        root,
        bin,
    };
    fixture.gh(PULLS);
    fixture
}

fn task<'a>(value: &'a Value, id: &str) -> &'a Value {
    value["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == id)
        .unwrap_or_else(|| panic!("{id}: {value}"))
}

#[test]
fn the_epic_view_gives_every_stage_location_and_issue() {
    let fx = fixture();
    let value = fx.json(&["--epic", "EPC-001"]);
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["snapshot"]["at"], AS_OF);
    assert_eq!(value["repository"], "github.com/acme/demo");
    let stages: BTreeMap<&str, &str> = ["TSK-001", "TSK-002", "TSK-003", "TSK-004", "TSK-005"]
        .iter()
        .map(|id| (*id, task(&value, id)["stage"].as_str().unwrap()))
        .collect();
    assert_eq!(stages["TSK-001"], "not started");
    assert_eq!(stages["TSK-002"], "in progress");
    assert_eq!(stages["TSK-003"], "in review");
    assert_eq!(stages["TSK-004"], "landed");
    assert_eq!(stages["TSK-005"], "cancelled");
    // (0 + 20 + 50 + 100) / 4, the cancelled task excluded and listed.
    assert_eq!(value["scope"]["percent"], 42.5);
    assert_eq!(value["scope"]["cancelled"], serde_json::json!(["TSK-005"]));
    assert_eq!(task(&value, "TSK-004")["location"]["kind"], "main");
    assert_eq!(task(&value, "TSK-003")["location"]["kind"], "pull request");
    assert_eq!(task(&value, "TSK-002")["location"]["kind"], "branch");
    assert_eq!(
        task(&value, "TSK-001")["location"]["note"],
        "record on line"
    );
    assert_eq!(task(&value, "TSK-002")["theme"], "Speed");
    assert_eq!(task(&value, "TSK-001")["theme"], "Core work");
    assert_eq!(value["scope"]["deliverable"], "The demo ships.");
    let red = value["issues"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["kind"] == "required check red")
        .expect("the red check is an issue");
    assert_eq!(red["subject"], "TSK-003");
    assert_eq!(red["unblocker"]["kind"], "ci run");
    assert_eq!(value["lanes"]["source"], "observed");
    assert_eq!(value["lanes"]["note"], "lanes: observed only");
    assert_eq!(value["issues"]["operator"]["state"], "none owed");
}

#[test]
fn the_order_agrees_with_work_next() {
    let fx = fixture();
    let value = fx.json(&["--epic", "EPC-001"]);
    let output = fx.cli(&["work", "next", "--json"]);
    assert!(output.status.success(), "{}", text(&output));
    let next: Value = serde_json::from_slice(&output.stdout).unwrap();
    let states: BTreeMap<String, String> = next["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| {
            (
                task["id"].as_str().unwrap().into(),
                task["state"].as_str().unwrap().into(),
            )
        })
        .collect();
    let rows = value["order"]["rows"].as_array().unwrap();
    assert!(!rows.is_empty());
    for row in rows {
        assert_eq!(
            Some(row["state"].as_str().unwrap()),
            states.get(row["id"].as_str().unwrap()).map(String::as_str),
            "{row}"
        );
    }
    let at = |id: &str| rows.iter().position(|row| row["id"] == id).unwrap();
    assert!(at("TSK-001") < at("TSK-003"));
}

#[test]
fn an_offline_forge_is_unverified_and_never_fails_the_command() {
    let fx = fixture();
    fx.gh("exit 1");
    let value = fx.json(&["--epic", "EPC-001"]);
    assert_eq!(value["pulls_into_main"], "unverified");
    assert_eq!(value["issues"]["operator"]["state"], "not established");
    assert_eq!(
        value["issues"]["operator"]["unchecked"],
        serde_json::json!(["forge"])
    );
    assert_eq!(value["issues"]["complete"], false);
    assert_eq!(task(&value, "TSK-002")["location"]["state"], "unverified");
}

#[test]
fn two_runs_on_one_state_are_byte_identical() {
    let fx = fixture();
    let first = fx.cli(&["status", "--json", "--epic", "EPC-001"]);
    let second = fx.cli(&["status", "--json", "--epic", "EPC-001"]);
    assert!(first.status.success(), "{}", text(&first));
    assert_eq!(first.stdout, second.stdout);
}

/// Every file under the repository with its bytes, `.git` included.
fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.insert(path.clone(), std::fs::read(&path).unwrap_or_default());
            }
        }
    }
    out
}

#[test]
fn status_writes_nothing_and_never_fetches() {
    let fx = fixture();
    let before = tree(&fx.root);
    for args in [
        vec!["status", "--json", "--epic", "EPC-001"],
        vec!["status", "--task", "TSK-002"],
        vec!["status", "--json"],
    ] {
        let output = fx.cli(&args);
        assert!(output.status.success(), "{}", text(&output));
    }
    assert!(before == tree(&fx.root), "the repository changed");
    let log = std::fs::read_to_string(&fx.log).unwrap();
    assert!(
        log.lines().all(|line| line.starts_with("pr list ")),
        "the forge was only read: {log}"
    );
}

#[test]
fn the_text_view_prints_the_ladder_order_lanes_and_windows() {
    let fx = fixture();
    let output = fx.cli(&["status", "--epic", "EPC-001"]);
    let out = text(&output);
    assert!(output.status.success(), "{out}");
    for needle in [
        "Ladder: not started 0%, in progress 20%",
        "Overall: 42.5%",
        "path to main:",
        "Order (critical path",
        "Lanes (observed)",
        "intake stops:",
        "TSK-003 required check red",
    ] {
        assert!(out.contains(needle), "{needle}: {out}");
    }
    assert!(!out.contains("2026-11-0") || out.contains("as of 2026-11-01T12:00:00Z"));
}

#[test]
fn the_default_text_view_is_unchanged() {
    let fx = fixture();
    let output = fx.cli(&["status"]);
    let out = text(&output);
    assert!(output.status.success(), "{out}");
    assert!(!out.contains("Ladder:"), "{out}");
}

#[test]
fn output_flags_are_mutually_exclusive_by_name() {
    let fx = fixture();
    let output = fx.cli(&["status", "--json", "--md"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        text(&output).contains("--json and --md cannot be used together"),
        "{}",
        text(&output)
    );
    let output = fx.cli(&["status", "--json", "--present", "--md"]);
    assert!(
        text(&output).contains("--json and --present and --md"),
        "{}",
        text(&output)
    );
}

#[test]
fn a_malformed_session_file_is_refused_by_name() {
    let fx = fixture();
    let path = fx.root.parent().unwrap().join("session.json");
    std::fs::write(
        &path,
        r#"{"schema_version":1,"facts":[{"observed_at":"2026-11-01T11:00:00Z","lane":"L-1","owner":"x"}]}"#,
    )
    .unwrap();
    let output = fx.cli(&["status", "--json", "--session", path.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output).contains("owner"), "{}", text(&output));

    std::fs::write(
        &path,
        r#"{"schema_version":1,"facts":[{"observed_at":"2026-11-01T11:00:00Z","lane":"L-1","task":"TSK-002","builder":"codex"}]}"#,
    )
    .unwrap();
    let value = fx.json(&["--epic", "EPC-001", "--session", path.to_str().unwrap()]);
    assert_eq!(value["lanes"]["session"][0]["builder"], "codex");
    assert_eq!(value["scope"]["percent"], 42.5);
}

#[test]
fn since_refuses_a_snapshot_of_another_scope() {
    let fx = fixture();
    let previous = fx.root.parent().unwrap().join("previous.json");
    let output = fx.cli(&["status", "--json", "--task", "TSK-001"]);
    std::fs::write(&previous, &output.stdout).unwrap();
    let output = fx.cli(&[
        "status",
        "--json",
        "--epic",
        "EPC-001",
        "--since",
        previous.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output).contains("scope kind"), "{}", text(&output));
    let value = fx.json(&["--task", "TSK-001", "--since", previous.to_str().unwrap()]);
    assert_eq!(value["since"]["changes"], serde_json::json!([]));
}

#[test]
fn cited_commands_carry_the_repository_path_from_another_folder() {
    let fx = fixture();
    write(
        &fx.root,
        "project-management/tasks/TSK-001.md",
        &task_record("TSK-001", "blocked", "", "")
            .replace("## Description", "## Blocker\n\n- reason: waits on a vendor\n- owner: operator\n- revisit: when the vendor replies\n\n## Description"),
    );
    git(&fx.root, &["commit", "-qam", "docs: block TSK-001"]);
    git(
        &fx.root,
        &[
            "remote",
            "set-url",
            "origin",
            fx.root
                .parent()
                .unwrap()
                .join("origin.git")
                .to_str()
                .unwrap(),
        ],
    );
    git(&fx.root, &["push", "-q", "origin", "main"]);
    git(&fx.root, &["fetch", "-q", "origin"]);
    git(
        &fx.root,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/acme/demo.git",
        ],
    );
    let elsewhere = fx.root.parent().unwrap().join("wt-TSK-002");
    let output = fx.cli_in(&elsewhere, &["status", "--json", "--epic", "EPC-001"]);
    assert!(output.status.success(), "{}", text(&output));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let blocked = value["issues"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["kind"] == "blocked")
        .expect("the blocked task is an issue");
    let command = blocked["command"].as_str().unwrap();
    assert!(command.starts_with("cd /"), "{command}");
    assert_eq!(
        blocked["recommended_action"],
        "revisit: when the vendor replies"
    );
    assert_eq!(value["issues"]["operator"]["state"], "owed");
}

#[test]
fn the_facts_validate_against_the_shipped_schema() {
    let fx = fixture();
    let schema: Value = serde_json::from_str(include_str!(
        "../../../assets/base/status/schemas/report-v1.schema.json"
    ))
    .unwrap();
    for args in [vec!["--epic", "EPC-001"], vec!["--task", "TSK-001"], vec![]] {
        let value = fx.json(&args);
        let errors = validate(&schema, &schema, &value, "$");
        assert!(errors.is_empty(), "{args:?}: {errors:#?}");
    }
    // The schema is frozen: a field removed or added is a version change.
    let required: Vec<&str> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        required,
        [
            "schema_version",
            "kind",
            "repository",
            "snapshot",
            "refs",
            "scope",
            "ladder",
            "epics",
            "tasks",
            "pulls_into_main",
            "order",
            "lanes",
            "windows",
            "issues",
            "pace",
            "since",
            "evidence"
        ]
    );
}

/// The subset of JSON Schema the shipped schemas use: `type`, `const`,
/// `enum`, `pattern` (anchored literal prefixes are not checked),
/// `required`, `properties`, `additionalProperties: false`, `items`,
/// `anyOf` and local `$ref`.
fn validate(root: &Value, schema: &Value, value: &Value, at: &str) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let name = reference.trim_start_matches("#/$defs/");
        return validate(root, &root["$defs"][name], value, at);
    }
    if let Some(options) = schema.get("anyOf").and_then(Value::as_array) {
        if !options
            .iter()
            .any(|option| validate(root, option, value, at).is_empty())
        {
            errors.push(format!("{at}: matches no anyOf option: {value}"));
        }
    }
    if let Some(expected) = schema.get("const") {
        if expected != value {
            errors.push(format!("{at}: expected {expected}, got {value}"));
        }
    }
    if let Some(options) = schema.get("enum").and_then(Value::as_array) {
        if !options.contains(value) {
            errors.push(format!("{at}: {value} not in {options:?}"));
        }
    }
    if let Some(kind) = schema.get("type").and_then(Value::as_str) {
        let ok = match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "number" => value.is_number(),
            "integer" => value.is_i64() || value.is_u64(),
            _ => true,
        };
        if !ok {
            errors.push(format!("{at}: not {kind}: {value}"));
            return errors;
        }
        if kind == "integer"
            && schema.get("minimum").is_some()
            && value.as_i64().is_some_and(|n| n < 0)
        {
            errors.push(format!("{at}: negative"));
        }
    }
    if let Some(object) = value.as_object() {
        for field in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if !object.contains_key(field.as_str().unwrap()) {
                errors.push(format!("{at}: missing {field}"));
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        for (key, item) in object {
            match properties.and_then(|p| p.get(key)) {
                Some(sub) => errors.extend(validate(root, sub, item, &format!("{at}.{key}"))),
                None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => {
                    errors.push(format!("{at}: unexpected field {key}"));
                }
                None => {}
            }
        }
    }
    if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
        for (index, item) in array.iter().enumerate() {
            errors.extend(validate(root, items, item, &format!("{at}[{index}]")));
        }
    }
    errors
}
