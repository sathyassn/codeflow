//! Project design authority and standing reviews through the real CLI
//! (issue 43). Fictional catalogs and disposable planning repositories only.
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{json, Value};

#[path = "../../codeflow-core/tests/support/catalog_fixture.rs"]
mod fixture_data;

const CATALOG: &str = ".agents/skills/cf-model-orchestrator/resources/current-ensemble.json";
const SELECTION: &str = ".codeflow/model-selection.json";
const TARGET: &str = "integration/fixture";

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn git(root: &Path, args: &[&str]) {
    let result = Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn authority() -> Value {
    json!({
        "schema_version": 2,
        "bindings": [],
        "design_authority": {
            "seat": "orchid-seat",
            "designated": {"date": "2026-10-04", "record": "owner instruction fixture"},
            "lines": {
                "orchid-main": {"role": "owner"},
                "orchid-support": {"role": "co-owner", "approves": ["phase-exit"]}
            }
        },
        "standing_reviews": [
            {"area": "design", "seat": "cinder-seat", "mode": "read-only", "per": "phase",
             "record": "owner instruction fixture"}
        ]
    })
}

fn task(id: &str, contract: &str) -> String {
    format!(
        "---\nid: {id}\ntitle: fictional design\nstatus: todo\nwork_type: feat\nepic_id: null\n\
         standalone_reason: isolated fixture\nintegration_target: {TARGET}\nspecs: []\n\
         depends_on: []\ncreated: 2026-10-04\n---\n\n## Execution contract\n\nPlan v1.\n\n\
         ```text\n{contract}\n```\n\n## Closeout\n"
    )
}

struct Project {
    dir: tempfile::TempDir,
}

impl Project {
    /// A disposable repository whose integration target carries `target_file`
    /// (when given) and the task record, checked out on the task's branch.
    fn new(target_file: Option<&Value>, contract: &str) -> Self {
        let project = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        let root = project.root();
        write(root, CATALOG, &fixture_data::fixture().to_string());
        write(
            root,
            "project-management/tasks/TSK-900.md",
            &task("TSK-900", contract),
        );
        if let Some(file) = target_file {
            write(root, SELECTION, &file.to_string());
        }
        git(root, &["init", "-q", "-b", TARGET]);
        git(root, &["config", "user.email", "fixture@example.test"]);
        git(root, &["config", "user.name", "Fixture"]);
        git(root, &["add", "."]);
        git(
            root,
            &["commit", "-q", "-m", "test: seed the fictional target"],
        );
        git(root, &["checkout", "-q", "-b", "task/TSK-900-fixture"]);
        project
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_codeflow"))
            .current_dir(self.root())
            .env("CODEFLOW_HOME", self.root().join("personal"))
            .args(["models", "resolve", "--host", "claude-code", "--json"])
            .args(args)
            .output()
            .unwrap()
    }

    fn resolve(&self, args: &[&str], exit: i32) -> Value {
        let output = self.run(args);
        assert_eq!(
            output.status.code(),
            Some(exit),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn refused(&self, args: &[&str], reason: &str) {
        let output = self.run(args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{stderr}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert!(stderr.contains(reason), "{stderr}; expected {reason}");
    }
}

const DESIGN: &[&str] = &[
    "--duty",
    "design",
    "--task",
    "TSK-900",
    "--exclude",
    "selector:orchid-one-alias",
];

fn limitations(participant: &Value) -> Vec<&str> {
    participant["limitations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap())
        .collect()
}

/// AC-1: a block committed on the task's integration target makes its
/// co-owner design after the owner, with no override and no reduced
/// assurance; the working tree changes nothing.
#[test]
fn a_committed_co_owner_designs_without_an_override() {
    let project = Project::new(Some(&authority()), "");
    let result = project.resolve(DESIGN, 0);
    let chosen = &result["participants"][0];
    assert_eq!(chosen["line"], "orchid-support");
    assert_eq!(chosen["operator_override"], Value::Null);
    assert_eq!(chosen["reduced_assurance"], false);
    let limits = limitations(chosen);
    assert!(
        limits.iter().any(|l| l.starts_with(
            "design co-owner by project designation (.codeflow/model-selection.json, owner \
             instruction fixture; committed on integration/fixture at "
        )),
        "{limits:?}"
    );
    assert!(!limits.iter().any(|l| l.contains("reduced assurance")));
    // A working-tree edit that demotes the co-owner is not authority.
    let mut demoted = authority();
    demoted["design_authority"]["lines"]["orchid-support"]["role"] = json!("consultant");
    write(project.root(), SELECTION, &demoted.to_string());
    assert_eq!(
        project.resolve(DESIGN, 0)["participants"][0]["line"],
        "orchid-support"
    );
}

/// A block that exists only in the working tree or on the task branch is
/// not on the integration target, so it confers no design authority.
#[test]
fn a_block_off_the_integration_target_confers_nothing() {
    let project = Project::new(None, "");
    write(project.root(), SELECTION, &authority().to_string());
    let open = project.resolve(DESIGN, 1);
    assert_eq!(open["participants"], json!([]));
    let reasons = open["open"][0]["reasons"].to_string();
    assert!(
        reasons.contains("not committed on integration/fixture"),
        "{reasons}"
    );
    git(project.root(), &["add", "."]);
    git(
        project.root(),
        &["commit", "-q", "-m", "test: branch-only block"],
    );
    let open = project.resolve(DESIGN, 1);
    assert!(
        open["open"][0]["reasons"]
            .to_string()
            .contains("not committed on integration/fixture"),
        "{open}"
    );
    // Without --task nothing is anchored, and the result says so.
    let open = project.resolve(
        &["--duty", "design", "--exclude", "selector:orchid-one-alias"],
        1,
    );
    assert!(
        open["open"][0]["reasons"]
            .to_string()
            .contains("applies only with --task"),
        "{open}"
    );
}

/// AC-2: design approval is its own duty. Configured, the committed
/// co-owner is the required approver and is labelled as not the independent
/// review; unconfigured, it is an optional open gap and the exit is 0.
#[test]
fn design_approval_is_its_own_duty_and_optional_when_unconfigured() {
    let project = Project::new(Some(&authority()), "");
    let approval = project.resolve(&["--duty", "design-approval", "--task", "TSK-900"], 0);
    let approver = &approval["participants"][0];
    assert_eq!(approver["participant"], "approver:orchid-support");
    assert_eq!(approver["label"], "required");
    let limits = limitations(approver);
    assert!(limits.contains(&"same-family design approval; not the independent review"));
    assert!(limits.contains(&"approves at: phase-exit"));

    let bare = Project::new(None, "");
    let unconfigured = bare.resolve(&["--duty", "design-approval", "--task", "TSK-900"], 0);
    assert_eq!(unconfigured["participants"], json!([]));
    assert_eq!(unconfigured["open"][0]["participant"], "design-approval");
    assert_eq!(unconfigured["open"][0]["label"], "second-opinion");
}

/// AC-3: a standing review adds the catalog's extra-family participant to a
/// review run with its area; without the area, or with another, the output
/// equals the output with no project file.
#[test]
fn a_standing_review_applies_to_its_area_only() {
    let review = &["--duty", "unit-review", "--author", "claude"];
    let bare = Project::new(None, "");
    let baseline = bare.resolve(review, 0);
    let project = Project::new(Some(&authority()), "");
    assert_eq!(project.resolve(review, 0), baseline);
    let other = [review.as_slice(), &["--area", "billing"]].concat();
    assert_eq!(project.resolve(&other, 0), baseline);
    let design = [review.as_slice(), &["--area", "design"]].concat();
    let result = project.resolve(&design, 0);
    let extra = result["participants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["participant"] == "extra")
        .unwrap();
    assert_eq!(extra["seat"], "cinder-seat");
    assert_eq!(extra["label"], "required");
    assert!(limitations(extra).contains(
        &"standing assignment (area design, seat cinder-seat, read-only, per phase; owner \
          instruction fixture)"
    ));
    project.refused(
        &["--duty", "design", "--area", "design"],
        "--area applies to unit-review and body-review",
    );
}

type Change = dyn Fn(&mut Value);

/// AC-4: each invalid block rejects the whole file; every duty exits 2.
#[test]
fn an_invalid_block_rejects_the_whole_file() {
    let cases: [(&str, &Change); 6] = [
        ("is not listed by seat", &|v| {
            v["design_authority"]["lines"]["orchid-spare"] = json!({"role": "co-owner"});
        }),
        ("another family", &|v| {
            v["design_authority"]["lines"]["quartz-main"] = json!({"role": "co-owner"});
        }),
        ("first line", &|v| {
            v["design_authority"]["lines"]["orchid-main"]["role"] = json!("co-owner");
            v["design_authority"]["lines"]["orchid-support"] = json!({"role": "owner"});
        }),
        ("unknown field", &|v| {
            v["design_authority"]["lines"]["orchid-support"]["command"] = json!("x");
        }),
        ("standing-pair seat", &|v| {
            v["standing_reviews"][0]["seat"] = json!("quartz-seat");
        }),
        ("unknown variant", &|v| {
            v["standing_reviews"][0]["mode"] = json!("write");
        }),
    ];
    let project = Project::new(None, "");
    // A same-family line the design owner seat does not list.
    let mut catalog = fixture_data::fixture();
    catalog["lines"].as_array_mut().unwrap().push(json!({
        "id": "orchid-spare", "family": "orchid", "adoption": "manual",
        "adopted_version": "orchid-spare-one",
        "versions": [fixture_data::version("orchid-spare-one", &["claude-code"], None)]
    }));
    write(project.root(), CATALOG, &catalog.to_string());
    for (reason, change) in cases {
        let mut value = authority();
        change(&mut value);
        write(project.root(), SELECTION, &value.to_string());
        for duty in ["design", "design-approval", "unit-review", "orchestrate"] {
            project.refused(&["--duty", duty, "--task", "TSK-900"], reason);
        }
    }
    let duplicate = authority().to_string().replace(
        r#""orchid-main":{"role":"owner"}"#,
        r#""orchid-main":{"role":"owner"},"orchid-main":{"role":"owner"}"#,
    );
    write(project.root(), SELECTION, &duplicate);
    project.refused(&["--duty", "design"], "duplicate JSON key");
    // An invalid committed file rejects the whole file too.
    let mut bad = authority();
    bad["design_authority"]["lines"]["quartz-main"] = json!({"role": "co-owner"});
    let committed = Project::new(Some(&bad), "");
    write(committed.root(), SELECTION, &authority().to_string());
    committed.refused(DESIGN, "committed on integration/fixture");
}

/// AC-6: another family still designs only through the task's anchored
/// `OPERATOR_OVERRIDE`, which still needs its record, with a block present.
#[test]
fn another_family_still_needs_the_task_override() {
    let route = "quartz-main@codex-cli";
    let contract = format!(
        "OPERATOR_OVERRIDE\ntask: TSK-900\nduty: design\nroute: {route}\neffort: high\n\
         plan: Plan v1\ninstruction: 2026-10-04 operator conversation fixture"
    );
    let project = Project::new(Some(&authority()), &contract);
    let args = [
        "--duty",
        "design",
        "--task",
        "TSK-900",
        "--override",
        "TSK-900",
        "--route",
        route,
        "--effort",
        "high",
    ];
    let result = project.resolve(&args, 0);
    assert_eq!(result["participants"][0]["line"], "quartz-main");
    assert_eq!(
        result["participants"][0]["operator_override"]["plan_version"],
        "Plan v1"
    );
    // Without the record the route is refused, block or no block.
    let unrecorded = Project::new(Some(&authority()), "");
    unrecorded.refused(&args, "OPERATOR_OVERRIDE");
    project.refused(
        &["--duty", "design", "--route", route, "--effort", "high"],
        "--route and --effort require --override",
    );
}
