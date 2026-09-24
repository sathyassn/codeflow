//! Fictional catalogs and disposable planning repositories only.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

#[path = "../../codeflow-core/tests/support/catalog_fixture.rs"]
mod fixture_data;

const CATALOG: &str = ".agents/skills/cf-model-orchestrator/resources/current-ensemble.json";
const ROUTE: &str = "orchid-support@claude-code";

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

struct Fixture {
    dir: tempfile::TempDir,
    home: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("personal");
        write(dir.path(), CATALOG, &fixture_data::fixture().to_string());
        Self { dir, home }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn catalog(&self, value: &Value) {
        write(self.root(), CATALOG, &value.to_string());
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codeflow"));
        command
            .current_dir(self.root())
            .env("CODEFLOW_HOME", &self.home)
            .args(["models", "resolve"]);
        command
    }

    fn run(&self, duty: &str, author: &str, extra: &[&str]) -> Output {
        self.command()
            .args([
                "--duty",
                duty,
                "--host",
                "claude-code",
                "--author",
                author,
                "--json",
            ])
            .args(extra)
            .output()
            .unwrap()
    }

    fn resolved(&self, duty: &str, author: &str, extra: &[&str], success: bool) -> Value {
        let output = self.run(duty, author, extra);
        assert_eq!(
            output.status.code(),
            Some(i32::from(!success)),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn input_error(&self, duty: &str, extra: &[&str], reason: &str) {
        let output = self.run(duty, "none", extra);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!("models resolve: {reason}\n")
        );
    }
}

#[test]
fn models_resolve_every_duty_and_author_and_required_exit_status() {
    let fixture = Fixture::new();
    for author in ["claude", "codex", "grok", "none"] {
        for duty in fixture_data::fixture()["duties"]
            .as_object()
            .unwrap()
            .keys()
        {
            let output = fixture.run(duty, author, &[]);
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            let required_open = value["open"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["label"] == "required");
            assert_eq!(output.status.success(), !required_open, "{duty} {author}");
            assert!(!value["participants"].as_array().unwrap().is_empty() || required_open);
            for participant in value["participants"].as_array().unwrap() {
                for field in [
                    "line",
                    "version",
                    "pinned_id",
                    "effort",
                    "remaining_alternatives",
                    "label",
                ] {
                    assert!(!participant[field].is_null(), "{duty}: missing {field}");
                }
            }
        }
    }
    let planning = fixture.resolved("independent-plan", "none", &[], true);
    assert_eq!(planning["participants"].as_array().unwrap().len(), 2);
    let optional = fixture.resolved("general-review", "codex", &[], true);
    assert!(optional["open"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["label"] == "second-opinion"));
    fixture.input_error(
        "test-authoring",
        &[],
        "test-authoring is not resolved separately; use the unit's implementation duty",
    );
    fixture.input_error("unknown-duty", &[], "unknown duty unknown-duty");
}

#[test]
fn models_catalog_only_changes_pin_and_floor_and_prints_text() {
    let fixture = Fixture::new();
    let before = fixture.resolved("orchestrate", "none", &[], true);
    let mut catalog = fixture_data::fixture();
    let mut newer = catalog["lines"][0]["versions"][0].clone();
    newer["id"] = json!("orchid-next");
    newer["pinned_id"] = json!("orchid-next-pin");
    newer["alias"] = json!("orchid-next-alias");
    newer["selectors"]["claude-code"] = json!("orchid-next-pin");
    catalog["lines"][0]["versions"]
        .as_array_mut()
        .unwrap()
        .push(newer);
    catalog["duties"]["bounded-execution"]["required"][0]["alternatives"][0]["effort"] =
        json!("high");
    fixture.catalog(&catalog);
    let after = fixture.resolved("orchestrate", "none", &[], true);
    assert_eq!(before["participants"][0]["pinned_id"], "orchid-one-pin");
    assert_eq!(after["participants"][0]["pinned_id"], "orchid-next-pin");
    assert_eq!(
        fixture.resolved("bounded-execution", "none", &[], true)["participants"][0]["effort"],
        "high"
    );
    let text = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .current_dir(fixture.root())
        .env("CODEFLOW_HOME", &fixture.home)
        .args([
            "models",
            "resolve",
            "--duty",
            "orchestrate",
            "--host",
            "claude-code",
        ])
        .output()
        .unwrap();
    assert!(text.status.success());
    let text = String::from_utf8(text.stdout).unwrap();
    for expected in [
        "orchid-seat",
        "orchid-main",
        "version=orchid-next",
        "pinned=orchid-next-pin",
        "effort=high",
        "alternative:",
    ] {
        assert!(text.contains(expected), "{text}");
    }
}

#[test]
fn models_exclusions_drift_triggers_and_optional_gap() {
    let fixture = Fixture::new();
    let result = fixture.resolved(
        "engineering-implementation",
        "none",
        &["--exclude", "bucket:quartz-bucket"],
        true,
    );
    assert_eq!(result["participants"][0]["line"], "orchid-main");
    fixture.resolved(
        "independent-plan",
        "none",
        &["--exclude", "bucket:quartz-bucket"],
        false,
    );
    let result = fixture.resolved(
        "orchestrate",
        "none",
        &["--observed", "orchid-one-pin=unexpected-pin"],
        true,
    );
    assert_eq!(result["participants"][0]["pinned_id"], "orchid-two-pin");
    assert_eq!(result["participants"][0]["reduced_assurance"], true);
    fixture.resolved(
        "design",
        "none",
        &["--observed", "orchid-one-pin=unexpected-pin"],
        false,
    );
    let result = fixture.resolved(
        "light-execution",
        "none",
        &["--observed", "quartz-two-pin=unknown-worker"],
        true,
    );
    assert_eq!(result["participants"][0]["pinned_id"], "unknown-worker");
    assert_eq!(result["participants"][0]["eligibility"], "candidate");
    let result = fixture.resolved("orchestrate", "none", &["--trigger", "deep"], true);
    assert_eq!(result["obligations"][0]["effort"], "xhigh");
    assert_eq!(result["xhigh_trigger_met"], true);
    let review = fixture.resolved("unit-review", "codex", &["--trigger", "material"], true);
    assert_eq!(review["participants"].as_array().unwrap().len(), 2);
    let optional = fixture.resolved("general-review", "codex", &[], true);
    assert_eq!(optional["open"][0]["label"], "second-opinion");
    for invalid in [
        vec!["--exclude", "bucket:unknown"],
        vec!["--exclude", "selector:unknown"],
        vec!["--observed", "bad"],
        vec!["--observed", "unknown=pin"],
        vec![
            "--observed",
            "orchid-one-pin=a",
            "--observed",
            "orchid-one-pin=b",
        ],
        vec!["--host", "unknown"],
    ] {
        assert!(!fixture.run("design", "none", &invalid).status.success());
    }
}

fn git(root: &Path, args: &[&str]) {
    let result = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn task(id: &str, block: &str) -> String {
    format!("---\nid: {id}\ntitle: fictional design\nstatus: todo\nwork_type: feat\nepic_id: null\nstandalone_reason: isolated fixture\nintegration_target: integration/fixture\nspecs: []\ndepends_on: []\ncreated: 2026-09-23\n---\n\n## Execution contract\n\nPlan v3.4.\n\n```text\n{block}\n```\n\n## Closeout\n")
}

fn block() -> String {
    format!("OPERATOR_OVERRIDE\ntask: TSK-900\nduty: design\nroute: {ROUTE}\neffort: high\nplan: Plan v3.4\ninstruction: 2026-09-23 operator conversation fixture")
}

fn planning_fixture(record: &str) -> Fixture {
    let fixture = Fixture::new();
    git(fixture.root(), &["init", "-b", "integration/fixture"]);
    git(
        fixture.root(),
        &["config", "user.email", "fixture@example.test"],
    );
    git(fixture.root(), &["config", "user.name", "Fixture"]);
    write(
        fixture.root(),
        "project-management/tasks/TSK-900.md",
        &task("TSK-900", record),
    );
    write(
        fixture.root(),
        "project-management/tasks/TSK-901.md",
        &task("TSK-901", ""),
    );
    git(fixture.root(), &["add", "."]);
    git(
        fixture.root(),
        &["commit", "-m", "test: establish fictional planning anchor"],
    );
    git(fixture.root(), &["checkout", "-b", "task/TSK-900-fixture"]);
    fixture
}

fn override_args<'a>(task: &'a str, id: &'a str) -> Vec<&'a str> {
    vec![
        "--task",
        task,
        "--override",
        id,
        "--route",
        ROUTE,
        "--effort",
        "high",
        "--exclude",
        "selector:orchid-one-alias",
    ]
}

#[test]
fn models_override_matches_anchored_plan_and_never_working_tree() {
    let fixture = planning_fixture(&block());
    let result = fixture.resolved("design", "none", &override_args("TSK-900", "TSK-900"), true);
    assert_eq!(result["participants"][0]["pinned_id"], "orchid-two-pin");
    assert_eq!(
        result["participants"][0]["operator_override"]["plan_version"],
        "Plan v3.4"
    );
    write(
        fixture.root(),
        "project-management/tasks/TSK-900.md",
        "unapproved working tree corruption",
    );
    fixture.resolved("design", "none", &override_args("TSK-900", "TSK-900"), true);
    for (task, id, reason) in [
        ("TSK-900", "../record.md", "OPERATOR_OVERRIDE task mismatch"),
        ("TSK-900", "anything", "OPERATOR_OVERRIDE task mismatch"),
        ("TSK-900", "TSK-901", "OPERATOR_OVERRIDE task mismatch"),
        ("TSK-901", "TSK-900", "OPERATOR_OVERRIDE task mismatch"),
        ("TSK-999", "TSK-999", "task TSK-999 not committed"),
    ] {
        fixture.input_error("design", &override_args(task, id), reason);
    }
    fixture.input_error(
        "orchestrate",
        &override_args("TSK-900", "TSK-900"),
        "OPERATOR_OVERRIDE is only valid for design",
    );
}

#[test]
fn models_override_rejects_each_missing_or_mismatched_field() {
    let original = block();
    let mut records = vec![
        (
            String::new(),
            "Execution contract requires exactly one OPERATOR_OVERRIDE block".to_owned(),
        ),
        (
            format!("{original}\n\n{original}"),
            "Execution contract requires exactly one OPERATOR_OVERRIDE block".into(),
        ),
        (
            format!("{original}\n```\n\n## Execution contract\n\n```text\n{original}"),
            "task requires exactly one Execution contract".into(),
        ),
        (
            original.replace("TSK-900", "TSK-901"),
            "OPERATOR_OVERRIDE task mismatch".into(),
        ),
        (
            original.replace("duty: design", "duty: orchestrate"),
            "OPERATOR_OVERRIDE duty mismatch".into(),
        ),
        (
            original.replace(ROUTE, "orchid-main@claude-code"),
            "OPERATOR_OVERRIDE route mismatch".into(),
        ),
        (
            original.replace("effort: high", "effort: xhigh"),
            "OPERATOR_OVERRIDE effort mismatch".into(),
        ),
        (
            original.replace("Plan v3.4", "not-a-plan"),
            "override plan must name Plan vN".into(),
        ),
    ];
    for field in ["task", "duty", "route", "effort", "plan", "instruction"] {
        records.push((
            original
                .lines()
                .filter(|line| !line.starts_with(&format!("{field}:")))
                .collect::<Vec<_>>()
                .join("\n"),
            format!("missing OPERATOR_OVERRIDE {field}"),
        ));
    }
    for (record, reason) in records {
        let fixture = planning_fixture(&record);
        fixture.input_error("design", &override_args("TSK-900", "TSK-900"), &reason);
    }
}

#[test]
fn models_override_rejects_unanchored_blocks_and_records() {
    let fixture = planning_fixture("");
    write(
        fixture.root(),
        "project-management/tasks/TSK-900.md",
        &task("TSK-900", &block()),
    );
    fixture.input_error(
        "design",
        &override_args("TSK-900", "TSK-900"),
        "Execution contract requires exactly one OPERATOR_OVERRIDE block",
    );
    git(fixture.root(), &["add", "."]);
    git(
        fixture.root(),
        &["commit", "-m", "test: unapproved task branch override"],
    );
    fixture.input_error(
        "design",
        &override_args("TSK-900", "TSK-900"),
        "Execution contract requires exactly one OPERATOR_OVERRIDE block",
    );
    write(
        fixture.root(),
        "project-management/tasks/TSK-999.md",
        &task("TSK-999", &block().replace("TSK-900", "TSK-999")),
    );
    fixture.input_error(
        "design",
        &override_args("TSK-999", "TSK-999"),
        "task TSK-999 not committed",
    );
}

fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut result = Vec::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            result.extend(snapshot(&path));
        } else {
            result.push((path.clone(), std::fs::read(&path).unwrap()));
        }
    }
    result.sort();
    result
}

#[test]
fn models_loads_overlay_and_rejects_project_violation_without_writes() {
    let fixture = Fixture::new();
    write(fixture.root(), ".codeflow/project.toml", "tier='full'\n");
    write(
        &fixture.home,
        "model-catalog.local.json",
        &json!({"schema_version":1,"additions":[],"exclusions":["orchid-one"]}).to_string(),
    );
    let before = snapshot(fixture.root());
    let result = fixture.resolved("orchestrate", "none", &[], true);
    assert_eq!(result["participants"][0]["pinned_id"], "orchid-two-pin");
    assert_eq!(snapshot(fixture.root()), before);
    write(
        fixture.root(),
        ".codeflow/model-selection.json",
        r#"{"schema_version":1,"bindings":[{"role":"claude-judgment-primary","binding_id":"missing"}]}"#,
    );
    let before = snapshot(fixture.root());
    fixture.input_error("orchestrate", &[], "missing binding record");
    assert_eq!(snapshot(fixture.root()), before);
}

#[test]
fn models_project_binding_selects_exact_tuple_without_bypassing_exclusions() {
    let fixture = Fixture::new();
    let digest = format!("sha256:{}", "a".repeat(64));
    let binding = json!({
        "schema_version":1,"binding_id":"orchid-approved","provider":"anthropic","lineage":"claude",
        "eligible_roles":["claude-judgment-primary"],"qualified_at":"2026-09-23T10:00:00Z",
        "requested":{"model":"orchid-two-pin","effort":"high","harness":"claude-code","harness_version":"1.0","settings_digest":digest},
        "observed":{"model":"orchid-two-pin","effort":"high","evidence":[{"kind":"session","digest":digest}]},
        "qualification":{"run_id":"fictional-run","suite":"full","suite_digest":digest,"result_digest":digest,"codeflow_revision":"abc123"},
        "settings_sources":[],"approval":{"reviewer":"operator","reviewed_at":"2026-09-23T10:00:00Z"}
    });
    write(
        &fixture.home,
        "qualified-bindings/orchid.json",
        &binding.to_string(),
    );
    write(
        fixture.root(),
        ".codeflow/model-selection.json",
        r#"{"schema_version":1,"bindings":[{"role":"claude-judgment-primary","binding_id":"orchid-approved"}]}"#,
    );
    let result = fixture.resolved("orchestrate", "none", &[], true);
    assert_eq!(result["participants"][0]["pinned_id"], "orchid-two-pin");
    assert_eq!(result["participants"][0]["eligibility"], "qualified");
    fixture.resolved("design", "none", &[], false);
    fixture.resolved(
        "orchestrate",
        "none",
        &["--exclude", "selector:orchid-two-pin"],
        false,
    );
    fixture.resolved(
        "orchestrate",
        "none",
        &["--observed", "orchid-two-pin=unexpected"],
        false,
    );
    let mut catalog = fixture_data::fixture();
    catalog["duties"]["orchestrate"]["required"][0]["alternatives"][0]["effort"] = json!("xhigh");
    fixture.catalog(&catalog);
    fixture.resolved("orchestrate", "none", &[], false);
}

#[test]
fn models_override_rejects_target_only_commit_and_unstable_target() {
    let fixture = planning_fixture("");
    // Diverge the implementation first, then update the target without merging
    // it. The new block is on the target but outside the shared planning base.
    write(fixture.root(), "work.txt", "fixture work");
    git(fixture.root(), &["add", "."]);
    git(
        fixture.root(),
        &["commit", "-m", "test: diverge fixture task"],
    );
    git(fixture.root(), &["checkout", "integration/fixture"]);
    write(
        fixture.root(),
        "project-management/tasks/TSK-900.md",
        &task("TSK-900", &block()),
    );
    git(fixture.root(), &["add", "."]);
    git(
        fixture.root(),
        &["commit", "-m", "test: update fixture target plan"],
    );
    git(fixture.root(), &["checkout", "task/TSK-900-fixture"]);
    fixture.input_error(
        "design",
        &override_args("TSK-900", "TSK-900"),
        "Execution contract requires exactly one OPERATOR_OVERRIDE block",
    );
    for target in ["HEAD", "task/TSK-900-fixture", "integration/fixture~0"] {
        write(
            fixture.root(),
            "project-management/tasks/TSK-900.md",
            &task("TSK-900", &block()).replace(
                "integration_target: integration/fixture",
                &format!("integration_target: {target}"),
            ),
        );
        git(fixture.root(), &["add", "."]);
        git(
            fixture.root(),
            &["commit", "-m", "test: invalid fixture target"],
        );
        fixture.input_error(
            "design",
            &override_args("TSK-900", "TSK-900"),
            "override target must be a stable non-task branch",
        );
    }
}

#[test]
fn models_override_without_task_reports_missing_argument() {
    let fixture = Fixture::new();
    fixture.input_error(
        "design",
        &[
            "--override",
            "TSK-900",
            "--route",
            ROUTE,
            "--effort",
            "high",
        ],
        "--override requires --task",
    );
}

#[test]
fn models_input_errors_use_stderr_and_exit_two() {
    let fixture = Fixture::new();
    for (extra, reason) in [
        (
            vec!["--observed", "bad"],
            "observed must be pinned-id=observed-id",
        ),
        (
            vec!["--observed", "unknown=pin"],
            "unknown observed pinned id unknown",
        ),
        (
            vec!["--exclude", "selector:unknown"],
            "unknown excluded selector unknown",
        ),
        (
            vec!["--task", "../record"],
            "task must be a canonical TSK id",
        ),
        (
            vec!["--route", ROUTE],
            "--route and --effort require --override",
        ),
    ] {
        fixture.input_error("design", &extra, reason);
    }
    let output = fixture
        .command()
        .args(["--duty", "design", "--host", "unknown", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "models resolve: unsupported host harness\n"
    );
    for (extra, reason) in [
        (
            vec!["--no-such-flag"],
            "unexpected argument '--no-such-flag'",
        ),
        (
            vec![
                "--task",
                "TSK-900",
                "--override",
                "TSK-900",
                "--route",
                ROUTE,
                "--effort",
                "superhigh",
            ],
            "unknown variant `superhigh`",
        ),
    ] {
        let output = fixture.run("design", "none", &extra);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr).unwrap().contains(reason));
    }
    fixture.catalog(&json!({"schema_version":5}));
    let output = fixture.run("design", "none", &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("missing field `policy_id`"));
}

#[test]
fn models_real_resolutions_preserve_engine_json_and_exit_status() {
    use codeflow_core::model_catalog::{Catalog, Exclusion, ExclusionScope, ResolveRequest};
    let fixture = Fixture::new();
    let catalog = Catalog::parse(fixture_data::fixture().to_string().as_bytes()).unwrap();
    let observations = std::collections::BTreeMap::new();
    let exclusions = [Exclusion {
        scope: ExclusionScope::Version("orchid-one".into()),
        reason: "caller supplied selector:orchid-one".into(),
        fresh_native: true,
    }];
    for (extra, exclusions) in [
        (vec![], &[][..]),
        (vec!["--exclude", "selector:orchid-one"], &exclusions[..]),
    ] {
        let expected = catalog
            .resolve(&ResolveRequest {
                duty: "design",
                task: "",
                host_harness: "claude-code",
                author_lineage: None,
                exclusions,
                observed_ids: &observations,
                trigger_facts: &[],
                requested_override: None,
                operator_override: None,
            })
            .unwrap();
        let output = fixture.run("design", "none", &extra);
        assert_eq!(output.status.code(), Some(i32::from(expected.is_open())));
        assert!(output.stderr.is_empty());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("{}\n", serde_json::to_string_pretty(&expected).unwrap())
        );
    }
}

#[test]
fn models_failure_paths_never_invent_participant_names() {
    let fixture = planning_fixture(&block());
    let mut override_excluded = override_args("TSK-900", "TSK-900");
    override_excluded.extend(["--exclude", "selector:orchid-two"]);
    for extra in [
        vec!["--exclude", "selector:orchid-one"],
        vec!["--observed", "orchid-one-pin=drifted"],
        override_excluded,
    ] {
        let result = fixture.resolved("design", "none", &extra, false);
        assert_eq!(result["open"][0]["participant"], "designer");
        if extra.contains(&"--override") {
            assert_eq!(result["open"][0]["reasons"], json!(["OPERATOR_OVERRIDE route ineligible: orchid-two: native exclusion: caller supplied selector:orchid-two"]));
        }
    }
    // Rejected input has no participant result at all. Only the engine may
    // name a real open participant, including ineligible anchored overrides.
    fixture.input_error(
        "design",
        &override_args("TSK-901", "TSK-900"),
        "OPERATOR_OVERRIDE task mismatch",
    );
    let output = fixture
        .command()
        .args([
            "--duty",
            "design",
            "--host",
            "claude-code",
            "--exclude",
            "selector:orchid-one",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("designer: open [required]:"));
    assert!(!text.contains("design: open"));
}

#[test]
fn models_help_describes_context_output_and_whole_version_exclusions() {
    let fixture = Fixture::new();
    let output = fixture.command().arg("--help").output().unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for (flag, description) in [
        (
            "--duty",
            "Catalog duty whose participants and obligations must be resolved",
        ),
        (
            "--trigger",
            "Catalog trigger fact to apply to this resolution",
        ),
        (
            "--task",
            "Canonical task id for this resolution; required with --override",
        ),
        (
            "--json",
            "Print the resolution as JSON; input errors go to stderr with exit 2",
        ),
        (
            "--exclude",
            "`selector:<id>` excludes the whole version across harnesses",
        ),
    ] {
        assert!(help.contains(flag));
        assert!(help.contains(description), "{flag}: {help}");
    }
}

#[test]
fn models_text_marks_filled_and_open_obligations() {
    let fixture = Fixture::new();
    let command = || {
        fixture
            .command()
            .args([
                "--duty",
                "orchestrate",
                "--host",
                "claude-code",
                "--trigger",
                "deep",
            ])
            .output()
            .unwrap()
    };
    let output = command();
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("host: orchid-seat"));
    assert!(text.contains("\nobligation: xhigh-reasoning:orchid-seat: "));
    let mut catalog = fixture_data::fixture();
    for line in catalog["lines"].as_array_mut().unwrap() {
        if line["family"] == "orchid" {
            for version in line["versions"].as_array_mut().unwrap() {
                version["efforts"] = json!(["medium", "high"]);
            }
        }
    }
    fixture.catalog(&catalog);
    let output = command();
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("\nobligation: xhigh-reasoning:orchid-seat: open [required]:"));
    assert!(text.contains("xhigh trigger met: false"));
}
