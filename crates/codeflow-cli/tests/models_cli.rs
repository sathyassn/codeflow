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
                area: None,
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
    assert!(text.starts_with("participant: designer: open [required]:"));
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
        (
            "--area",
            "Review area for a project standing review; unit-review and body-review only",
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
    assert!(text.starts_with("participant: host: orchid-seat"));
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

// TSK-431: the account profile and the limit readings. Disposable folders
// only; the profile holds paths, and no test touches a real config home,
// keychain or credential helper.

/// A project folder with an isolated home and `CodeFlow` home.
struct Seats {
    dir: tempfile::TempDir,
}

impl Seats {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("project")).unwrap();
        std::fs::create_dir_all(dir.path().join("home")).unwrap();
        Self { dir }
    }

    fn project(&self) -> PathBuf {
        self.dir.path().join("project")
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn profile(&self, text: &str) {
        write(&self.project(), ".codeflow/local/accounts.toml", text);
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codeflow"));
        command
            .current_dir(self.project())
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .env_remove("GROK_HOME")
            .env("HOME", self.home())
            .env("CODEFLOW_HOME", self.home().join(".codeflow"))
            .arg("models")
            .args(args);
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    fn record(&self, family: &str, payload: &str) -> Output {
        use std::io::Write;
        let mut child = self
            .command(&["record-limits", "--family", family])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    /// The environment a shell has after it evals the launch line for
    /// `family`, in a cleared environment.
    fn launched_environment(&self, family: &str) -> String {
        let script = format!(
            "home_line=$({bin} models launch-env --family {family}) && eval \"$home_line\"; env",
            bin = env!("CARGO_BIN_EXE_codeflow")
        );
        let output = Command::new("sh")
            .current_dir(self.project())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home())
            .env("CODEFLOW_HOME", self.home().join(".codeflow"))
            .args(["-c", &script])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    }
}

const PROFILE: &str = r#"
schema_version = 1

[families.claude]
config_home = "~/claude-work"

[families.codex]
config_home = "/srv/codex-second"

[families.codex.api_fallback]
key_env = "CODEX_FALLBACK_API_KEY"
spend_cap_usd = 25

[families.grok]
config_home = "/srv/grok-second"
"#;

const CLAUDE_STATUSLINE: &str = r#"{"model":{"id":"claude-opus-5-5"},"rate_limits":{
  "five_hour":{"used_percentage":23.5,"resets_at":4102444800},
  "seven_day":{"used_percentage":41.2,"resets_at":4102531200}}}"#;

const CODEX_READ: &str = r#"{"rateLimits":{"limitId":"codex","planType":"pro",
  "primary":{"usedPercent":12,"windowDurationMins":300,"resetsAt":4102444800},
  "secondary":{"usedPercent":60,"windowDurationMins":10080,"resetsAt":4102531200}}}"#;

fn env_lines(environment: &str, variable: &str) -> Vec<String> {
    let prefix = format!("{variable}=");
    environment
        .lines()
        .filter(|line| line.starts_with(&prefix))
        .map(str::to_string)
        .collect()
}

#[test]
fn launch_sets_only_the_launched_familys_config_home_variable() {
    let seats = Seats::new();
    seats.profile(PROFILE);
    let expected = [
        (
            "claude",
            "CLAUDE_CONFIG_DIR",
            format!("{}/claude-work", seats.home().display()),
        ),
        ("codex", "CODEX_HOME", "/srv/codex-second".to_string()),
        ("grok", "GROK_HOME", "/srv/grok-second".to_string()),
    ];
    for (family, variable, path) in &expected {
        let environment = seats.launched_environment(family);
        assert_eq!(
            env_lines(&environment, variable),
            [format!("{variable}={path}")],
            "{family}"
        );
        for (_, other, _) in expected.iter().filter(|(f, _, _)| f != family) {
            assert!(
                env_lines(&environment, other).is_empty(),
                "{family} set {other}"
            );
        }
    }
}

#[test]
fn launch_with_no_profile_or_no_mapping_sets_nothing_and_says_so() {
    let seats = Seats::new();
    let output = seats.run(&["launch-env", "--family", "codex"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("default home"));
    seats.profile("schema_version = 1\n[families.claude]\nconfig_home = \"/srv/c\"\n");
    let environment = seats.launched_environment("codex");
    for variable in ["CLAUDE_CONFIG_DIR", "CODEX_HOME", "GROK_HOME"] {
        assert!(env_lines(&environment, variable).is_empty(), "{variable}");
    }
}

#[test]
fn a_refused_profile_stops_the_launch_snippet_before_any_variable_is_set() {
    // The documented launch line must fail closed: a refused profile exits
    // non-zero, so the seat is not started under the CLI's default account.
    let seats = Seats::new();
    seats
        .profile("schema_version = 1\n[families.codex]\nconfig_home = \"/srv/c\"\ntoken = \"x\"\n");
    let script = format!(
        "home_line=$({bin} models launch-env --family codex 2>/dev/null) || {{ echo REFUSED; exit 1; }}; \
         eval \"$home_line\"; echo STARTED; env",
        bin = env!("CARGO_BIN_EXE_codeflow")
    );
    let output = Command::new("sh")
        .current_dir(seats.project())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", seats.home())
        .env("CODEFLOW_HOME", seats.home().join(".codeflow"))
        .args(["-c", &script])
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(
        text.contains("REFUSED") && !text.contains("STARTED"),
        "{text}"
    );
    assert!(env_lines(&text, "CODEX_HOME").is_empty());
}

#[test]
fn launch_json_names_the_variable_and_folder() {
    let seats = Seats::new();
    seats.profile(PROFILE);
    let output = seats.run(&["launch-env", "--family", "codex", "--json"]);
    assert_eq!(output.status.code(), Some(0));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value,
        json!({"family": "codex", "variable": "CODEX_HOME", "config_home": "/srv/codex-second"})
    );
}

#[test]
fn a_profile_that_names_a_token_or_credential_is_refused_by_every_reader() {
    let seats = Seats::new();
    for (profile, needle) in [
        (
            "schema_version = 1\n[families.codex]\nconfig_home = \"/srv/c\"\ntoken = \"abc\"\n",
            "token",
        ),
        (
            "schema_version = 1\n[families.codex]\nconfig_home = \"/srv/c\"\n\
             [families.codex.api_fallback]\nkey_env = \"CODEX_KEY\"\nspend_cap_usd = 5\n\
             client_secret = \"abc\"\n",
            "secret",
        ),
        (
            concat!(
                "schema_version = 1\n[families.codex]\nconfig_home = \"/srv/c\"\nnote = \"ghp",
                "_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"\n"
            ),
            "GitHub token",
        ),
    ] {
        seats.profile(profile);
        for args in [
            vec!["launch-env", "--family", "codex"],
            vec!["limits"],
            vec!["limits", "--json"],
        ] {
            let output = seats.run(&args);
            assert_eq!(output.status.code(), Some(2), "{args:?}");
            assert!(
                output.stdout.is_empty(),
                "{args:?}: nothing for a shell to eval"
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains("accounts.toml") && stderr.contains(needle),
                "{stderr}"
            );
        }
    }
}

#[test]
fn each_familys_fixture_payload_shows_used_percentage_and_reset_time() {
    let seats = Seats::new();
    seats.profile(PROFILE);
    let recorded = seats.record("claude", CLAUDE_STATUSLINE);
    assert_eq!(recorded.status.code(), Some(0), "{recorded:?}");
    assert_eq!(seats.record("codex", CODEX_READ).status.code(), Some(0));
    let hit = seats.run(&[
        "record-limits",
        "--family",
        "grok",
        "--limit-hit",
        "--resets-at",
        "4102444800",
    ]);
    assert_eq!(hit.status.code(), Some(0), "{hit:?}");

    let output = seats.run(&["limits", "--json"]);
    assert_eq!(output.status.code(), Some(0));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let family = |id: &str| -> Value {
        value["families"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["family"] == id)
            .cloned()
            .unwrap()
    };
    let claude = family("claude");
    assert_eq!(
        claude["five_hour"],
        json!({"state": "used", "used_percent": 23.5, "resets_at": 4_102_444_800_u64})
    );
    assert_eq!(
        claude["weekly"],
        json!({"state": "used", "used_percent": 41.2, "resets_at": 4_102_531_200_u64})
    );
    let codex = family("codex");
    assert_eq!(codex["five_hour"]["used_percent"], 12.0);
    assert_eq!(codex["weekly"]["resets_at"], 4_102_531_200_u64);
    assert_eq!(codex["account"]["config_home"], "/srv/codex-second");
    assert_eq!(codex["account"]["api_fallback"]["spend_cap_usd"], 25.0);
    let grok = family("grok");
    assert_eq!(grok["five_hour"], json!({"state": "not_exposed"}));
    assert_eq!(grok["limit_hit_resets_at"], 4_102_444_800_u64);

    let text = String::from_utf8(seats.run(&["limits"]).stdout).unwrap();
    assert!(
        text.contains("5-hour: 23.5% used, resets 2100-01-01T00:00:00Z"),
        "{text}"
    );
    assert!(
        text.contains("weekly: 60% used, resets 2100-01-02T00:00:00Z"),
        "{text}"
    );
    assert!(text.contains("account: config home /srv/codex-second (via CODEX_HOME)"));
    assert!(text.contains("api fallback: key in $CODEX_FALLBACK_API_KEY, spend cap $25"));
    assert!(text.contains("5-hour: not exposed"));
    assert!(text.contains("limit hit reported"));
}

#[test]
fn a_family_with_nothing_recorded_says_not_exposed() {
    let seats = Seats::new();
    let output = seats.run(&["limits"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    for family in ["claude", "codex", "grok"] {
        assert!(text.contains(&format!(
            "{family}:\n  account: the CLI's default config home"
        )));
    }
    assert_eq!(text.matches("5-hour: not exposed").count(), 3);
    assert_eq!(text.matches("weekly: not exposed").count(), 3);
    assert!(text.contains("Grok Build gives no usage reading"));
}

#[test]
fn recording_refuses_a_bad_payload_and_writes_nothing_for_an_empty_one() {
    let seats = Seats::new();
    for (family, payload) in [
        ("claude", "not json"),
        (
            "claude",
            r#"{"rate_limits":{"five_hour":{"used_percentage":250}}}"#,
        ),
        ("codex", r#"{"unrelated":true}"#),
    ] {
        let output = seats.record(family, payload);
        assert_eq!(output.status.code(), Some(2), "{family} {payload}");
    }
    let empty = seats.record("claude", r#"{"model":{"id":"x"}}"#);
    assert_eq!(empty.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&empty.stdout).contains("nothing recorded"));
    assert!(!seats.project().join(".codeflow/local/limits").exists());
    // Grok has no payload form, and only Grok takes --limit-hit.
    assert_eq!(seats.record("grok", "{}").status.code(), Some(2));
    let wrong = seats.run(&["record-limits", "--family", "claude", "--limit-hit"]);
    assert_eq!(wrong.status.code(), Some(2));
}

#[test]
fn a_linked_worktree_reads_the_main_checkouts_profile_and_readings() {
    let seats = Seats::new();
    let project = seats.project();
    git(&project, &["init", "--quiet", "-b", "main"]);
    git(
        &project,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.test",
            "commit",
            "--allow-empty",
            "-m",
            "init",
            "--quiet",
        ],
    );
    git(
        &project,
        &["worktree", "add", "--quiet", "-b", "feature", "../linked"],
    );
    seats.profile(PROFILE);
    let linked = seats.dir.path().join("linked");
    let mut command = Command::new(env!("CARGO_BIN_EXE_codeflow"));
    command
        .current_dir(&linked)
        .env("HOME", seats.home())
        .env("CODEFLOW_HOME", seats.home().join(".codeflow"))
        .args(["models", "launch-env", "--family", "codex"]);
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "export CODEX_HOME='/srv/codex-second'\n"
    );
    // A reading recorded from the worktree lands in the main checkout.
    let mut record = Command::new(env!("CARGO_BIN_EXE_codeflow"));
    record
        .current_dir(&linked)
        .env("HOME", seats.home())
        .env("CODEFLOW_HOME", seats.home().join(".codeflow"))
        .args(["models", "record-limits", "--family", "grok", "--limit-hit"]);
    assert_eq!(record.output().unwrap().status.code(), Some(0));
    assert!(project.join(".codeflow/local/limits/grok.json").is_file());
    assert!(!linked.join(".codeflow/local/limits").exists());
}

#[test]
fn the_managed_ignore_keeps_the_local_folder_untracked() {
    let ignore = include_str!("../../../assets/base/gitignore");
    assert!(ignore.lines().any(|line| line == ".codeflow/local/"));
}
