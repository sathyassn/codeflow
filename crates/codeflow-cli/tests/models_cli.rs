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

    /// A folder outside the project, written the way the platform writes an
    /// absolute path (a drive letter on Windows).
    fn far(&self, name: &str) -> PathBuf {
        self.dir.path().join("far").join(name)
    }

    fn far_text(&self, name: &str) -> String {
        self.far(name).display().to_string()
    }

    /// The standard profile: a folder per family, and a Codex API fallback.
    /// TOML literal strings, so a Windows path needs no escapes.
    fn standard_profile(&self) -> String {
        format!(
            "schema_version = 1\n\n[families.claude]\nconfig_home = '{}'\n\n\
             [families.codex]\nconfig_home = '{}'\n\n\
             [families.codex.api_fallback]\nkey_env = \"CODEX_FALLBACK_API_KEY\"\nspend_cap_usd = 25\n\n\
             [families.grok]\nconfig_home = '{}'\n",
            self.far_text("claude-work"),
            self.far_text("codex-second"),
            self.far_text("grok-second"),
        )
    }

    /// A profile that maps one family to one folder outside the project.
    fn one_home(&self, family: &str, name: &str) -> String {
        format!(
            "schema_version = 1\n[families.{family}]\nconfig_home = '{}'\n",
            self.far_text(name)
        )
    }

    /// Record `payload` as the seat of `family` that the standard profile maps
    /// (its config home passed explicitly).
    fn record(&self, family: &str, payload: &str) -> Output {
        let home = self.far_text(match family {
            "claude" => "claude-work",
            "codex" => "codex-second",
            _ => "grok-second",
        });
        self.record_in(family, payload, Some(&home))
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
            .env("USERPROFILE", self.home())
            .env("CODEFLOW_HOME", self.home().join(".codeflow"))
            .arg("models")
            .args(args);
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    fn record_in(&self, family: &str, payload: &str, home: Option<&str>) -> Output {
        use std::io::Write;
        let mut args = vec!["record-limits", "--family", family];
        if let Some(home) = home {
            args.extend(["--config-home", home]);
        }
        let mut child = self
            .command(&args)
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
fn the_managed_ignore_keeps_the_local_folder_untracked() {
    let ignore = include_str!("../../../assets/base/gitignore");
    assert!(ignore.lines().any(|line| line == ".codeflow/local/"));
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
        let output = seats.record_in(family, payload, None);
        assert_eq!(output.status.code(), Some(2), "{family} {payload}");
    }
    let empty = seats.record_in("claude", r#"{"model":{"id":"x"}}"#, None);
    assert_eq!(empty.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&empty.stdout).contains("nothing recorded"));
    assert!(!seats.project().join(".codeflow/local/limits").exists());
    // Grok has no payload form, and only Grok takes --limit-hit.
    assert_eq!(seats.record_in("grok", "{}", None).status.code(), Some(2));
    let wrong = seats.run(&["record-limits", "--family", "claude", "--limit-hit"]);
    assert_eq!(wrong.status.code(), Some(2));
}

#[test]
fn payload_fields_of_the_wrong_type_or_range_are_refused_and_stored_files_are_checked() {
    let seats = Seats::new();
    for (family, payload) in [
        (
            "codex",
            r#"{"rateLimits":{"primary":{"usedPercent":5,"windowDurationMins":"weekly"}}}"#,
        ),
        (
            "codex",
            r#"{"rateLimits":{"primary":{"usedPercent":5,"windowDurationMins":-5}}}"#,
        ),
        (
            "claude",
            r#"{"rate_limits":{"five_hour":{"used_percentage":5,"resets_at":18446744073709551615}}}"#,
        ),
        ("claude", r#"{"rate_limits":{"five_hour":"oops"}}"#),
        ("claude", r#"{"rate_limits":"oops"}"#),
        (
            "claude",
            r#"{"rate_limits":{"five_hour":{"used_percentage":-5}}}"#,
        ),
    ] {
        let output = seats.record_in(family, payload, None);
        assert_eq!(output.status.code(), Some(2), "{payload}: {output:?}");
    }
    assert!(!seats.project().join(".codeflow/local/limits").exists());
    // Grok reset time and the stored file get the same range check.
    let output = seats.run(&[
        "record-limits",
        "--family",
        "grok",
        "--limit-hit",
        "--resets-at",
        "18446744073709551615",
    ]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    write(
        &seats.project(),
        ".codeflow/local/limits/claude.json",
        r#"{"schema_version":2,"family":"claude","homes":[{"source":"x","recorded_at":1,
           "five_hour":{"used_percent":-123.0}}]}"#,
    );
    let output = seats.run(&["limits"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("0 to 100"));
}

const CLAUDE_STATUSLINE: &str = r#"{"model":{"id":"claude-opus-5-5"},"rate_limits":{
  "five_hour":{"used_percentage":23.5,"resets_at":4102444800},
  "seven_day":{"used_percentage":41.2,"resets_at":4102531200}}}"#;

const CODEX_READ: &str = r#"{"rateLimits":{"limitId":"codex","planType":"pro",
  "primary":{"usedPercent":12,"windowDurationMins":300,"resetsAt":4102444800},
  "secondary":{"usedPercent":60,"windowDurationMins":10080,"resetsAt":4102531200}}}"#;

#[test]
fn launch_with_no_profile_or_no_mapping_unsets_the_family_variable() {
    let seats = Seats::new();
    let output = seats.run(&["launch-env", "--family", "codex"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "unset CODEX_HOME\n"
    );
    seats.profile(&seats.one_home("claude", "c"));
    let output = seats.run(&["launch-env", "--family", "codex"]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "unset CODEX_HOME\n"
    );
    let json = seats.run(&["launch-env", "--family", "codex", "--json"]);
    let value: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(
        value,
        json!({"family": "codex", "variable": "CODEX_HOME", "config_home": null})
    );
}

#[test]
fn check_fails_unless_the_environment_matches_the_profile() {
    let seats = Seats::new();
    seats.profile(&seats.standard_profile());
    let check = |inherited: Option<&str>| {
        let mut command = seats.command(&["launch-env", "--family", "codex", "--check"]);
        if let Some(value) = inherited {
            command.env("CODEX_HOME", value);
        }
        command.output().unwrap()
    };
    let mapped = seats.far_text("codex-second");
    let below = seats.far("codex-second").join("sub").display().to_string();
    assert_eq!(check(Some(&mapped)).status.code(), Some(0));
    for wrong in [None, Some("/inherited"), Some(below.as_str())] {
        let output = check(wrong);
        assert_eq!(output.status.code(), Some(2), "{wrong:?}");
        assert!(output.stdout.is_empty());
    }
    seats.profile(&seats.one_home("claude", "c"));
    assert_eq!(check(None).status.code(), Some(0));
    assert_eq!(check(Some("/inherited")).status.code(), Some(2));
    // A refused profile refuses the check as well.
    seats.profile("schema_version = 1\ntoken = \"x\"\n");
    assert_eq!(check(None).status.code(), Some(2));
}

#[test]
fn launch_json_names_the_variable_and_folder() {
    let seats = Seats::new();
    seats.profile(&seats.standard_profile());
    let output = seats.run(&["launch-env", "--family", "codex", "--json"]);
    assert_eq!(output.status.code(), Some(0));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value,
        json!({"family": "codex", "variable": "CODEX_HOME", "config_home": seats.far_text("codex-second")})
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
    seats.profile(&seats.standard_profile());
    let recorded = seats.record("claude", CLAUDE_STATUSLINE);
    assert_eq!(recorded.status.code(), Some(0), "{recorded:?}");
    assert_eq!(seats.record("codex", CODEX_READ).status.code(), Some(0));
    let grok_home = seats.far_text("grok-second");
    let hit = seats.run(&[
        "record-limits",
        "--family",
        "grok",
        "--limit-hit",
        "--resets-at",
        "4102444800",
        "--config-home",
        &grok_home,
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
    assert_eq!(
        codex["account"]["config_home"],
        seats.far_text("codex-second")
    );
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
    assert!(text.contains(&format!(
        "account: config home {} (via CODEX_HOME)",
        seats.far_text("codex-second")
    )));
    assert!(text.contains("api fallback: key in $CODEX_FALLBACK_API_KEY, spend cap $25"));
    assert!(text.contains("5-hour: not exposed"));
    assert!(text.contains("limit hit reported"));
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
    seats.profile(&seats.standard_profile());
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
        format!("export CODEX_HOME='{}'\n", seats.far_text("codex-second"))
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

fn launch_in(seats: &Seats, home: &Path) -> Output {
    seats.profile(&format!(
        "schema_version = 1\n[families.codex]\nconfig_home = '{}'\n",
        home.display()
    ));
    seats.run(&["launch-env", "--family", "codex"])
}

fn refusal(seats: &Seats, home: &Path) -> String {
    let output = launch_in(seats, home);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{} {output:?}",
        home.display()
    );
    assert!(output.stdout.is_empty(), "nothing for a shell to eval");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_config_home_inside_a_linked_worktree_or_under_tracked_local_files_is_refused() {
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
    let linked = seats.dir.path().join("linked");
    git(
        &project,
        &["worktree", "add", "--quiet", "-b", "feature", "../linked"],
    );
    std::fs::create_dir_all(linked.join("state")).unwrap();
    // The worktree is outside the main checkout, yet a commit there carries
    // its files.
    refusal(&seats, &linked.join("state"));
    // A tracked file under the local exception makes the exception void.
    let local_home = project.join(".codeflow/local/home");
    std::fs::create_dir_all(&local_home).unwrap();
    assert_eq!(launch_in(&seats, &local_home).status.code(), Some(0));
    write(&project, ".codeflow/local/keep.txt", "x");
    git(&project, &["add", "-f", ".codeflow/local/keep.txt"]);
    refusal(&seats, &local_home);
}

#[test]
fn escaped_secret_shapes_are_refused_and_diagnostics_never_quote_the_text() {
    let seats = Seats::new();
    let key = "a1".repeat(20);
    // The secret-shaped text is written with TOML escapes, so the raw line
    // does not match any pattern but the decoded value does.
    seats.profile(&format!(
        "schema_version = 1\n[families.codex]\nconfig_home = \"/srv/\\u0073k-{key}\"\n"
    ));
    let output = seats.run(&["launch-env", "--family", "codex"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&key));
    // A malformed line neither echoes its value nor the source line.
    seats.profile("schema_version = 1\nsecret = \"SYNTHETIC_PRIVATE_VALUE\n");
    for args in [
        vec!["launch-env", "--family", "codex"],
        vec!["limits"],
        vec!["limits", "--json"],
    ] {
        let output = seats.run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("SYNTHETIC_PRIVATE_VALUE"), "{stderr}");
        assert!(stderr.contains("accounts.toml"), "{stderr}");
    }
}

#[test]
fn a_reading_is_not_shown_for_a_different_account_after_the_profile_changes() {
    let seats = Seats::new();
    let (home_a, home_b) = (seats.far_text("home-a"), seats.far_text("home-b"));
    seats.profile(&seats.one_home("codex", "home-a"));
    // The seat that produced the reading ran under home A.
    let recorded = seats.record_in("codex", CODEX_READ, Some(&home_a));
    assert_eq!(recorded.status.code(), Some(0), "{recorded:?}");
    let shown = |seats: &Seats| -> Value {
        let output = seats.run(&["limits", "--json"]);
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        value["families"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["family"] == "codex")
            .cloned()
            .unwrap()
    };
    assert_eq!(shown(&seats)["five_hour"]["state"], "used");
    // The profile now names home B: A's windows must not be shown for it.
    seats.profile(&seats.one_home("codex", "home-b"));
    let codex = shown(&seats);
    assert_eq!(codex["five_hour"], json!({"state": "not_exposed"}));
    assert_eq!(codex["weekly"], json!({"state": "not_exposed"}));
    assert_eq!(codex["exposed"], false);
    assert_eq!(codex["other_homes"], json!([home_a]));
    let text = String::from_utf8(seats.run(&["limits"]).stdout).unwrap();
    assert!(
        text.contains(&format!(
            "unknown for this account: readings exist only for {home_a}"
        )),
        "{text}"
    );
    assert!(!text.contains("12% used"), "{text}");
    // A recorder running without the flag takes its identity from its own
    // config-home variable.
    let mut command = seats.command(&["record-limits", "--family", "codex"]);
    command
        .env("CODEX_HOME", &home_b)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(CODEX_READ.as_bytes())
            .unwrap();
    }
    assert_eq!(child.wait_with_output().unwrap().status.code(), Some(0));
    assert_eq!(shown(&seats)["five_hour"]["state"], "used");
}

#[test]
fn concurrent_recorders_of_sparse_windows_never_lose_one() {
    use std::io::Write;
    let sparse_primary = r#"{"rateLimits":{"primary":{"usedPercent":1,"windowDurationMins":300,"resetsAt":4102444800}}}"#;
    let sparse_secondary = r#"{"rateLimits":{"secondary":{"usedPercent":2,"windowDurationMins":10080,"resetsAt":4102444800}}}"#;
    for round in 0..12 {
        let seats = Seats::new();
        let home = seats.far_text("a");
        let spawn = |payload: &'static str| {
            let mut child = seats
                .command(&["record-limits", "--family", "codex", "--config-home", &home])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            (child.stdin.take().unwrap(), child, payload)
        };
        let (stdin_a, child_a, payload_a) = spawn(sparse_primary);
        let (stdin_b, child_b, payload_b) = spawn(sparse_secondary);
        let (mut a, mut b) = (stdin_a, stdin_b);
        a.write_all(payload_a.as_bytes()).unwrap();
        b.write_all(payload_b.as_bytes()).unwrap();
        drop((a, b));
        for child in [child_a, child_b] {
            let output = child.wait_with_output().unwrap();
            assert_eq!(output.status.code(), Some(0), "round {round}: {output:?}");
        }
        seats.profile(&seats.one_home("codex", "a"));
        let output = seats.run(&["limits", "--json"]);
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        let codex = value["families"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["family"] == "codex")
            .unwrap();
        assert_eq!(codex["five_hour"]["state"], "used", "round {round}");
        assert_eq!(codex["weekly"]["state"], "used", "round {round}");
    }
}

#[test]
fn a_credential_named_or_secret_shaped_key_is_not_echoed_by_any_reader() {
    let seats = Seats::new();
    let secret = format!("sk-{}", "a1".repeat(20));
    for profile in [
        format!("schema_version = 1\n\"token {secret}\" = 1\n"),
        format!("schema_version = 1\n[families.codex]\nconfig_home = '/srv/c'\n\"api key {secret}\" = 1\n"),
        format!("schema_version = 1\n[families.\"{secret}\"]\nconfig_home = '/srv/c'\n"),
        // The shape is hidden by an escape, so only the decoded key shows it.
        format!(
            "schema_version = 1\n\"token \\u0073k-{}\" = 1\n",
            "a1".repeat(20)
        ),
    ] {
        seats.profile(&profile);
        for args in [
            vec!["launch-env", "--family", "codex"],
            vec!["launch-env", "--family", "codex", "--check"],
            vec!["limits"],
            vec!["limits", "--json"],
        ] {
            let output = seats.run(&args);
            assert_eq!(output.status.code(), Some(2), "{args:?}");
            assert!(output.stdout.is_empty());
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(!stderr.contains(&"a1".repeat(20)), "{args:?}: {stderr}");
            assert!(stderr.contains("accounts.toml"), "{stderr}");
        }
    }
}

#[test]
fn an_unreadable_index_does_not_open_the_local_exception() {
    let seats = Seats::new();
    let project = seats.project();
    git(&project, &["init", "--quiet", "-b", "main"]);
    let home = project.join(".codeflow/local/home");
    std::fs::create_dir_all(&home).unwrap();
    // A repository with no commits and an empty index: the exception holds.
    assert_eq!(launch_in(&seats, &home).status.code(), Some(0));
    write(&project, ".codeflow/local/keep.txt", "x");
    git(&project, &["add", "-f", ".codeflow/local/keep.txt"]);
    assert!(refusal(&seats, &home).contains("tracked"));
    // The index is damaged: the tracked file is invisible, and the home that
    // was just refused for it must not be accepted.
    std::fs::write(project.join(".git/index"), b"not an index").unwrap();
    let stderr = refusal(&seats, &home);
    assert!(
        stderr.contains("cannot read the repository index"),
        "{stderr}"
    );
    // A home outside the project does not depend on the index.
    let outside = seats.dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    assert_eq!(launch_in(&seats, &outside).status.code(), Some(0));
}

#[test]
fn a_file_from_the_previous_build_is_replaced_by_the_next_recording() {
    let seats = Seats::new();
    seats.profile(&seats.one_home("codex", "home-a"));
    // The shape the earlier build stored: one reading, no producing home.
    write(
        &seats.project(),
        ".codeflow/local/limits/codex.json",
        r#"{"schema_version":1,"family":"codex","source":"codex-rate-limits",
           "recorded_at":100,"five_hour":{"used_percent":77.0,"resets_at":4102444800}}"#,
    );
    let shown = |seats: &Seats| -> Value {
        let output = seats.run(&["limits", "--json"]);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        value["families"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["family"] == "codex")
            .cloned()
            .unwrap()
    };
    // Reading: no error, and the old number belongs to no account.
    let before = shown(&seats);
    assert_eq!(before["five_hour"], json!({"state": "not_exposed"}));
    assert_eq!(before["legacy_unbound"], true);
    let text = String::from_utf8(seats.run(&["limits"]).stdout).unwrap();
    assert!(text.contains("earlier-format reading"), "{text}");
    assert!(!text.contains("77"), "{text}");
    // Writing: a fresh reading is accepted, replaces the file, and is shown
    // for the home that produced it only.
    let home_a = seats.far_text("home-a");
    let recorded = seats.record_in("codex", CODEX_READ, Some(&home_a));
    assert_eq!(recorded.status.code(), Some(0), "{recorded:?}");
    let after = shown(&seats);
    assert_eq!(after["five_hour"]["state"], "used");
    assert_eq!(after["five_hour"]["used_percent"], 12.0);
    assert_eq!(after["legacy_unbound"], false);
    let stored =
        std::fs::read_to_string(seats.project().join(".codeflow/local/limits/codex.json")).unwrap();
    assert!(
        stored.contains("\"schema_version\": 2") && !stored.contains("77"),
        "{stored}"
    );
}

#[test]
fn a_leading_tilde_expands_to_the_home_folder() {
    let seats = Seats::new();
    seats.profile("schema_version = 1\n[families.claude]\nconfig_home = '~/claude-work'\n");
    let output = seats.run(&["launch-env", "--family", "claude"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!(
            "export CLAUDE_CONFIG_DIR='{}'\n",
            seats.home().join("claude-work").display()
        )
    );
}

// Unix-only seat tests: the launch sequences run in `sh`, the Herdr stub needs
// POSIX permissions, and the alias tests make symlinks. Everything that checks
// profile parsing and refusal, launch output, per-home attribution, migration
// or the boundary through the CLI is above this module and runs on every
// platform.
#[cfg(unix)]
mod unix_seats {
    use super::*;

    impl Seats {
        /// The environment a shell has after the published launch sequence
        /// (from the shipped `transport.md`) for `family`, with no inherited
        /// config-home variable.
        fn launched_environment(&self, family: &str) -> String {
            let output = self.sequence(family, &[], "env");
            assert!(output.status.success(), "{output:?}");
            String::from_utf8(output.stdout).unwrap()
        }

        /// Run the published launch text for `family` in `sh` with `inherited`
        /// variables set, then print a marker and run `after`. `codeflow` on
        /// `PATH` is this build.
        fn sequence(&self, family: &str, inherited: &[(&str, &str)], after: &str) -> Output {
            let script = format!(
                "{}\necho STARTED\n{after}\n",
                published_launch_block().replace("<claude|codex|grok>", family)
            );
            let mut command = Command::new("sh");
            command
                .current_dir(self.project())
                .env("PATH", self.bin_path())
                .env_remove("CLAUDE_CONFIG_DIR")
                .env_remove("CODEX_HOME")
                .env_remove("GROK_HOME")
                .env("HOME", self.home())
                .env("CODEFLOW_HOME", self.home().join(".codeflow"))
                .args(["-c", &script]);
            apply_inherited(&mut command, inherited);
            command.output().unwrap()
        }

        /// A `PATH` whose first folder holds a `codeflow` that is this build.
        fn bin_path(&self) -> std::ffi::OsString {
            let bin = self.dir.path().join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let link = bin.join("codeflow");
            if !link.exists() {
                std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_codeflow"), &link).unwrap();
            }
            let mut folders = vec![bin];
            folders.extend(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            ));
            std::env::join_paths(folders).unwrap()
        }
    }

    /// Set the named variables, which are only the three config homes and the
    /// stub's two switches, on `command`.
    fn apply_inherited(command: &mut Command, variables: &[(&str, &str)]) {
        for (name, value) in variables {
            match *name {
                "CLAUDE_CONFIG_DIR" => command.env("CLAUDE_CONFIG_DIR", value),
                "CODEX_HOME" => command.env("CODEX_HOME", value),
                "GROK_HOME" => command.env("GROK_HOME", value),
                "STUB_FAIL" => command.env("STUB_FAIL", value),
                "STUB_SWALLOW" => command.env("STUB_SWALLOW", value),
                other => panic!("unexpected variable {other}"),
            };
        }
    }

    /// The first `bash` block under "Account profiles" in the shipped transport
    /// rule: the exact text an agent runs before a seat starts.
    fn published_launch_block() -> String {
        let text = include_str!(
        "../../../assets/base/agents/skills/cf-model-orchestrator/resources/routing/transport.md"
    );
        let section = &text[text.find("### Account profiles").unwrap()..];
        let start = section.find("```bash\n").unwrap() + "```bash\n".len();
        let end = start + section[start..].find("```").unwrap();
        section[start..end].to_string()
    }

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
        seats.profile(&seats.standard_profile());
        let expected = [
            ("claude", "CLAUDE_CONFIG_DIR", seats.far_text("claude-work")),
            ("codex", "CODEX_HOME", seats.far_text("codex-second")),
            ("grok", "GROK_HOME", seats.far_text("grok-second")),
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

    const VARIABLES: [(&str, &str); 3] = [
        ("claude", "CLAUDE_CONFIG_DIR"),
        ("codex", "CODEX_HOME"),
        ("grok", "GROK_HOME"),
    ];

    #[test]
    fn the_published_sequence_replaces_or_clears_an_inherited_home_and_leaves_other_families_alone()
    {
        let seats = Seats::new();
        seats.profile(&seats.standard_profile());
        for (family, variable) in VARIABLES {
            let inherited: Vec<(&str, &str)> =
                VARIABLES.iter().map(|(_, v)| (*v, "/inherited")).collect();
            let output = seats.sequence(family, &inherited, "env");
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(
                output.status.success() && text.contains("STARTED"),
                "{text}"
            );
            let mapped = seats.far_text(match family {
                "claude" => "claude-work",
                "codex" => "codex-second",
                _ => "grok-second",
            });
            assert_eq!(
                env_lines(&text, variable),
                [format!("{variable}={mapped}")],
                "{family}"
            );
            for (_, other) in VARIABLES.iter().filter(|(f, _)| *f != family) {
                assert_eq!(
                    env_lines(&text, other),
                    [format!("{other}=/inherited")],
                    "{family} must not touch {other}"
                );
            }
        }
        // A family the profile does not map: its own inherited home is cleared.
        seats.profile(&seats.one_home("claude", "c"));
        for (family, variable) in VARIABLES.iter().filter(|(f, _)| *f != "claude") {
            let output = seats.sequence(family, &[(variable, "/inherited")], "env");
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(text.contains("STARTED"), "{text}");
            assert!(env_lines(&text, variable).is_empty(), "{family}: {text}");
        }
        // No profile at all behaves the same.
        std::fs::remove_file(seats.project().join(".codeflow/local/accounts.toml")).unwrap();
        let output = seats.sequence("codex", &[("CODEX_HOME", "/inherited")], "env");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("STARTED") && env_lines(&text, "CODEX_HOME").is_empty());
    }

    #[test]
    fn a_refused_profile_stops_the_published_sequence_with_an_inherited_home_in_place() {
        let seats = Seats::new();
        seats.profile(
            "schema_version = 1\n[families.codex]\nconfig_home = \"/srv/c\"\ntoken = \"x\"\n",
        );
        for (family, variable) in VARIABLES {
            let output = seats.sequence(family, &[(variable, "/inherited")], "env");
            let text = String::from_utf8(output.stdout).unwrap();
            assert_eq!(output.status.code(), Some(1), "{family}: {text}");
            assert!(!text.contains("STARTED"), "{family}: {text}");
            // Not even the last line of the sequence ran: nothing printed.
            assert!(text.is_empty(), "{family}: {text}");
        }
        // The same text under `sh -e` stops too.
        let script = format!(
            "{}\necho STARTED\n",
            published_launch_block().replace("<claude|codex|grok>", "codex")
        );
        let output = Command::new("sh")
            .current_dir(seats.project())
            .env("PATH", seats.bin_path())
            .env("HOME", seats.home())
            .env("CODEFLOW_HOME", seats.home().join(".codeflow"))
            .env("CODEX_HOME", "/inherited")
            .args(["-e", "-c", &script])
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&output.stdout).contains("STARTED"));
        assert!(!output.status.success());
    }

    /// The lines of the shipped `cf-herdr` launch that handle the account
    /// profile, up to the line before `herdr agent start`.
    fn published_herdr_block() -> String {
        let text = include_str!("../../../assets/base/agents/skills/cf-herdr/SKILL.md");
        let start = text.find("home_line=$(codeflow models launch-env").unwrap();
        let end = start + text[start..].find("herdr agent start").unwrap();
        text[start..end].to_string()
    }

    /// A stand-in `herdr` that logs each call and, for `pane run`, runs the
    /// command in a fresh `sh` that inherits the caller's environment and appends
    /// the command text and its output to `pane.out`, as a pane echoes what it is
    /// given (earlier commands are replayed so the pane keeps its state). `STUB_FAIL` names a call to fail (`run1`, `run2` or `wait`);
    /// `STUB_SWALLOW` names a `pane run` whose command is not executed.
    const HERDR_STUB: &str = r#"#!/bin/sh
dir="$STUB_DIR"
printf '%s\n' "$*" >> "$dir/calls.log"
case "$1 $2" in
"pane run")
  n=$(cat "$dir/runs" 2>/dev/null || echo 0); n=$((n + 1)); echo "$n" > "$dir/runs"
  shift 3
  printf '%s\n' "$*" >> "$dir/pane.out"
  [ "$STUB_FAIL" = "run$n" ] && exit 1
  [ "$STUB_SWALLOW" = "run$n" ] && exit 0
  printf '%s\n' "$*" >> "$dir/pane.sh"
  sh -c "$(cat "$dir/pane.sh")" >> "$dir/pane.out" 2>&1
  exit 0 ;;
"pane wait-output")
  [ "$STUB_FAIL" = "wait" ] && exit 1
  match=$5
  grep -F -- "$match" "$dir/pane.out" > /dev/null && exit 0
  exit 1 ;;
esac
exit 0
"#;

    fn run_herdr_sequence(seats: &Seats, family: &str, env: &[(&str, &str)]) -> (Output, String) {
        run_herdr_sequence_after(seats, family, env, "")
    }

    /// As [`run_herdr_sequence`], with `earlier` already in the pane's output, as
    /// from a previous launch in the same pane.
    fn run_herdr_sequence_after(
        seats: &Seats,
        family: &str,
        env: &[(&str, &str)],
        earlier: &str,
    ) -> (Output, String) {
        use std::os::unix::fs::PermissionsExt;
        let stub_dir = seats.dir.path().join("stub");
        std::fs::remove_dir_all(&stub_dir).ok();
        std::fs::create_dir_all(&stub_dir).unwrap();
        std::fs::write(stub_dir.join("pane.out"), earlier).unwrap();
        let path = seats.bin_path();
        let herdr = seats.dir.path().join("bin").join("herdr");
        std::fs::write(&herdr, HERDR_STUB).unwrap();
        std::fs::set_permissions(&herdr, std::fs::Permissions::from_mode(0o755)).unwrap();
        let script = format!(
        "pane_id=p1\n{}\nherdr agent start seat --kind {family} --pane \"$pane_id\"\necho LAUNCHED\n",
        published_herdr_block().replace("<claude|codex|grok>", family)
    );
        let mut command = Command::new("sh");
        command
            .current_dir(seats.project())
            .env("PATH", path)
            .env("STUB_DIR", &stub_dir)
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .env_remove("GROK_HOME")
            .env_remove("STUB_FAIL")
            .env_remove("STUB_SWALLOW")
            .env("HOME", seats.home())
            .env("CODEFLOW_HOME", seats.home().join(".codeflow"))
            .args(["-c", &script]);
        apply_inherited(&mut command, env);
        let output = command.output().unwrap();
        let calls = std::fs::read_to_string(stub_dir.join("calls.log")).unwrap_or_default();
        (output, calls)
    }

    /// The acknowledgement text a launch waited for, from the stub's call log.
    fn waited_for(calls: &str) -> String {
        let line = calls
            .lines()
            .find(|line| line.starts_with("pane wait-output"))
            .unwrap_or_else(|| panic!("no wait in {calls}"));
        let words: Vec<&str> = line.split_whitespace().collect();
        let at = words.iter().position(|w| *w == "--match").unwrap();
        words[at + 1].to_string()
    }

    #[test]
    fn the_herdr_launch_starts_the_seat_only_after_the_pane_confirms_its_home() {
        let seats = Seats::new();
        seats.profile(&seats.standard_profile());
        let (output, calls) = run_herdr_sequence(&seats, "codex", &[("CODEX_HOME", "/inherited")]);
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("LAUNCHED"), "{text} {calls}");
        assert!(calls.contains("agent start"), "{calls}");
        assert!(
            calls.contains(&format!(
                "pane run p1 export CODEX_HOME='{}'",
                seats.far_text("codex-second")
            )),
            "{calls}"
        );
        // The wait is for a per-launch acknowledgement, not a fixed word.
        let marker = waited_for(&calls);
        assert!(marker.starts_with("CF_HOME_OK_") && marker.len() >= "CF_HOME_OK_".len() + 24);
        // The pane echoes the command it is given; the acknowledgement is not in
        // that echo (the word and the nonce are separate printf arguments), so the
        // wait cannot be satisfied by the command line itself: it appears once, as
        // the output of a passed check.
        let pane = std::fs::read_to_string(seats.dir.path().join("stub/pane.out")).unwrap();
        assert_eq!(pane.matches(&marker).count(), 1, "{pane}");
        // A second launch waits for different text.
        let (_, again) = run_herdr_sequence(&seats, "codex", &[("CODEX_HOME", "/inherited")]);
        assert_ne!(waited_for(&again), marker);
    }

    #[test]
    fn the_acknowledgement_cannot_come_from_a_path_an_echo_an_error_or_old_output() {
        let seats = Seats::new();
        // 1. The mapped home itself ends in the old fixed word. The export that
        //    was never applied makes the check fail, and its diagnostic names the
        //    expected home: the word is in the echoed commands and in the error,
        //    and still must not start the seat.
        for word in ["CF_HOME_OK", "CF_HOME_OK_", "CF_HOME_OK_deadbeef"] {
            seats.profile(&format!(
                "schema_version = 1\n[families.codex]\nconfig_home = '{}/{word}'\n",
                seats.far_text("codex-second")
            ));
            let (output, calls) = run_herdr_sequence(
                &seats,
                "codex",
                &[("STUB_SWALLOW", "run1"), ("CODEX_HOME", "/inherited")],
            );
            let pane = std::fs::read_to_string(seats.dir.path().join("stub/pane.out")).unwrap();
            assert!(pane.contains(word), "the word must be in the pane: {pane}");
            assert!(
                !String::from_utf8_lossy(&output.stdout).contains("LAUNCHED"),
                "{word}: {pane}"
            );
            assert!(!calls.contains("agent start"), "{word}: {calls}");
            assert_eq!(output.status.code(), Some(1), "{word}");
        }
        // 2. Output left in the pane by an earlier launch, a fixed word and a
        //    full acknowledgement alike, does not satisfy a later wait.
        seats.profile(&seats.standard_profile());
        let earlier = "CF_HOME_OK\nCF_HOME_OK_000000000000000000000000\n";
        let (output, calls) = run_herdr_sequence_after(
            &seats,
            "codex",
            &[("STUB_SWALLOW", "run1"), ("CODEX_HOME", "/inherited")],
            earlier,
        );
        assert!(!calls.contains("agent start"), "{calls}");
        assert_eq!(output.status.code(), Some(1));
        // 3. A mapped home with the word in its name is not a problem when the
        //    check really passes: the seat starts.
        seats.profile(&seats.one_home("codex", "CF_HOME_OK"));
        let (output, calls) = run_herdr_sequence(&seats, "codex", &[("CODEX_HOME", "/inherited")]);
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("LAUNCHED"),
            "{calls}"
        );
    }

    #[test]
    fn a_failure_in_any_herdr_step_stops_the_launch_before_agent_start() {
        let seats = Seats::new();
        seats.profile(&seats.standard_profile());
        for (label, env) in [
            ("first pane run fails", vec![("STUB_FAIL", "run1")]),
            ("check pane run fails", vec![("STUB_FAIL", "run2")]),
            ("wait fails or times out", vec![("STUB_FAIL", "wait")]),
            // The pane never applied the line, so the check there fails and the
            // confirmation never appears.
            (
                "the pane did not apply the line",
                vec![("STUB_SWALLOW", "run1"), ("CODEX_HOME", "/inherited")],
            ),
        ] {
            let (output, calls) = run_herdr_sequence(&seats, "codex", &env);
            let text = String::from_utf8_lossy(&output.stdout);
            assert!(!text.contains("LAUNCHED"), "{label}: {text} {calls}");
            assert!(!calls.contains("agent start"), "{label}: {calls}");
            assert_eq!(output.status.code(), Some(1), "{label}");
        }
        // A refused profile stops before the pane is touched at all.
        seats.profile(
            "schema_version = 1\n[families.codex]\nconfig_home = \"/srv/c\"\ntoken = \"x\"\n",
        );
        let (output, calls) = run_herdr_sequence(&seats, "codex", &[("CODEX_HOME", "/inherited")]);
        assert_eq!(output.status.code(), Some(1));
        assert!(calls.is_empty(), "{calls}");
    }

    // Round 2 of TSK-431: findings from the first security review. Each test
    // states the attack it closes. All paths are disposable folders.

    #[cfg(unix)]
    #[test]
    fn a_config_home_that_aliases_the_project_or_is_not_a_folder_is_refused() {
        use std::os::unix::fs::symlink;
        let seats = Seats::new();
        let project = seats.project();
        let outside = seats.dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(project.join("tracked")).unwrap();
        std::fs::create_dir_all(project.join(".codeflow/local")).unwrap();

        // A link outside the project that points into it.
        symlink(project.join("tracked"), outside.join("alias")).unwrap();
        refusal(&seats, &outside.join("alias"));
        // A link under the local exception that points into tracked content, and
        // one that points to a folder elsewhere (the spelling says "local").
        symlink(project.join("tracked"), project.join(".codeflow/local/in")).unwrap();
        refusal(&seats, &project.join(".codeflow/local/in"));
        symlink(&outside, project.join(".codeflow/local/away")).unwrap();
        refusal(&seats, &project.join(".codeflow/local/away"));
        // A regular file is not a config home.
        let file = outside.join("file");
        std::fs::write(&file, "").unwrap();
        refusal(&seats, &file);
        // A dangling link.
        symlink(outside.join("missing"), outside.join("dangling")).unwrap();
        refusal(&seats, &outside.join("dangling"));
        // A real folder outside the project, and a real folder under local, work.
        let plain = seats.dir.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        let output = launch_in(&seats, &plain);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        let under_local = project.join(".codeflow/local/codex-second");
        std::fs::create_dir_all(&under_local).unwrap();
        assert_eq!(launch_in(&seats, &under_local).status.code(), Some(0));
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_local_folder_is_not_followed_for_readings() {
        use std::os::unix::fs::symlink;
        let seats = Seats::new();
        let outside = seats.dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(seats.project().join(".codeflow/local")).unwrap();
        symlink(&outside, seats.project().join(".codeflow/local/limits")).unwrap();
        let output = seats.record("claude", CLAUDE_STATUSLINE);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        assert_eq!(seats.run(&["limits"]).status.code(), Some(2));

        // The profile file itself behind a link is refused.
        let seats = Seats::new();
        let stash = seats.dir.path().join("stash");
        std::fs::create_dir_all(&stash).unwrap();
        std::fs::write(
            stash.join("accounts.toml"),
            "schema_version = 1\n[families.codex]\nconfig_home = \"/srv/c\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(seats.project().join(".codeflow")).unwrap();
        symlink(&stash, seats.project().join(".codeflow/local")).unwrap();
        let output = seats.run(&["launch-env", "--family", "codex"]);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
    }

    #[test]
    fn an_unmapped_family_clears_its_inherited_variable_in_a_shell() {
        let seats = Seats::new();
        let environment = seats.launched_environment("codex");
        for variable in ["CLAUDE_CONFIG_DIR", "CODEX_HOME", "GROK_HOME"] {
            assert!(env_lines(&environment, variable).is_empty(), "{variable}");
        }
    }
}
