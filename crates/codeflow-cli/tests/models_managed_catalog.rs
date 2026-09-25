//! `codeflow models resolve` against the switched managed catalog.
//!
//! Every model identity is read from the catalog; the test names seats, lines
//! and duties only through catalog data, never as literals.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

const CATALOG: &str = ".agents/skills/cf-model-orchestrator/resources/current-ensemble.json";

fn managed() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../assets/base/agents/skills/cf-model-orchestrator/resources/current-ensemble.json",
    );
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
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

fn seat<'a>(catalog: &'a Value, id: &str) -> &'a Value {
    catalog["seats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .unwrap()
}

fn line<'a>(catalog: &'a Value, id: &str) -> &'a Value {
    catalog["lines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["id"] == id)
        .unwrap()
}

fn family<'a>(catalog: &'a Value, id: &str) -> &'a Value {
    catalog["families"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == id)
        .unwrap()
}

/// The newest version of a line that is not retired.
fn newest(catalog: &Value, line_id: &str) -> Value {
    line(catalog, line_id)["versions"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|v| v["lifecycle"] != "retired")
        .unwrap()
        .clone()
}

fn seat_of_lineage(catalog: &Value, lineage: &str) -> Value {
    catalog["seats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| family(catalog, s["family"].as_str().unwrap())["lineage"] == lineage)
        .unwrap()
        .clone()
}

struct Repo {
    dir: tempfile::TempDir,
}

impl Repo {
    fn new(catalog: &Value) -> Self {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), CATALOG, &catalog.to_string());
        Self { dir }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn home(&self) -> PathBuf {
        self.root().join("personal")
    }

    fn run(&self, duty: &str, host: &str, author: &str, extra: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_codeflow"))
            .current_dir(self.root())
            .env("CODEFLOW_HOME", self.home())
            .args(["models", "resolve", "--duty", duty, "--host", host])
            .args(["--author", author, "--json"])
            .args(extra)
            .output()
            .unwrap()
    }

    /// Exit 0 when every required participant is filled, 1 when one is open.
    fn resolved(
        &self,
        duty: &str,
        host: &str,
        author: &str,
        extra: &[&str],
        filled: bool,
    ) -> Value {
        let output = self.run(duty, host, author, extra);
        assert_eq!(
            output.status.code(),
            Some(i32::from(!filled)),
            "{duty}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// An input error: exit 2, nothing on stdout, so nothing is filled.
    fn refused(&self, duty: &str, extra: &[&str], reason: &str) {
        let output = self.run(duty, "claude-code", "none", extra);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains(reason), "{stderr}");
    }
}

/// Launch identity: every returned participant carries a catalog pinned id and
/// the pinned selector, never a floating alias.
fn assert_pinned_launches(catalog: &Value, result: &Value) {
    let versions: Vec<&Value> = catalog["lines"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|l| l["versions"].as_array().unwrap())
        .collect();
    let aliases: Vec<&str> = versions
        .iter()
        .filter(|v| v["alias"] != v["pinned_id"])
        .map(|v| v["alias"].as_str().unwrap())
        .collect();
    for participant in result["participants"]
        .as_array()
        .unwrap()
        .iter()
        .chain(result["obligations"].as_array().unwrap())
    {
        let pinned = participant["pinned_id"].as_str().unwrap();
        assert!(
            versions.iter().any(|v| v["pinned_id"] == pinned),
            "{pinned}"
        );
        assert_eq!(participant["requested_selector"], pinned);
        assert!(!aliases.contains(&pinned), "alias launched: {pinned}");
    }
}

fn task(id: &str, block: &str) -> String {
    format!("---\nid: {id}\ntitle: fictional design\nstatus: todo\nwork_type: feat\nepic_id: null\nstandalone_reason: isolated fixture\nintegration_target: integration/fixture\nspecs: []\ndepends_on: []\ncreated: 2026-09-23\n---\n\n## Execution contract\n\nPlan v3.4.\n\n```text\n{block}\n```\n\n## Closeout\n")
}

fn override_block(task: &str, route: &str, effort: &str) -> String {
    format!("OPERATOR_OVERRIDE\ntask: {task}\nduty: design\nroute: {route}\neffort: {effort}\nplan: Plan v3.4\ninstruction: 2026-09-23 operator conversation fixture")
}

/// A disposable planning repository whose integration target carries `records`.
fn planning_repo(catalog: &Value, records: &[(&str, String)]) -> Repo {
    let repo = Repo::new(catalog);
    git(repo.root(), &["init", "-b", "integration/fixture"]);
    git(
        repo.root(),
        &["config", "user.email", "fixture@example.test"],
    );
    git(repo.root(), &["config", "user.name", "Fixture"]);
    for (id, block) in records {
        write(
            repo.root(),
            &format!("project-management/tasks/{id}.md"),
            &task(id, block),
        );
    }
    git(repo.root(), &["add", "."]);
    git(
        repo.root(),
        &["commit", "-m", "test: fictional planning anchor"],
    );
    git(repo.root(), &["checkout", "-b", "task/TSK-900-fixture"]);
    repo
}

fn with(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}

struct DesignSeat {
    owner: String,
    first: String,
    second: String,
    harness: String,
    first_pinned: String,
    second_pinned: String,
}

fn design_seat(catalog: &Value) -> DesignSeat {
    let owner = catalog["design_owner"].as_str().unwrap().to_owned();
    let seat = seat(catalog, &owner);
    let lines = seat["lines"].as_array().unwrap();
    let first = lines[0].as_str().unwrap().to_owned();
    let second = lines[1].as_str().unwrap().to_owned();
    let harness = family(catalog, seat["family"].as_str().unwrap())["harnesses"][0]
        .as_str()
        .unwrap()
        .to_owned();
    DesignSeat {
        first_pinned: newest(catalog, &first)["pinned_id"]
            .as_str()
            .unwrap()
            .into(),
        second_pinned: newest(catalog, &second)["pinned_id"]
            .as_str()
            .unwrap()
            .into(),
        owner,
        first,
        second,
        harness,
    }
}

#[test]
fn design_override_fills_the_seat_second_line_only_from_the_anchored_block() {
    let catalog = managed();
    let d = design_seat(&catalog);
    let route = format!("{}@{}", d.second, d.harness);
    let excluded = format!("selector:{}", d.first_pinned);
    let wrong_route = format!("{}@{}", d.first, d.harness);
    let repo = planning_repo(
        &catalog,
        &[
            ("TSK-900", override_block("TSK-900", &route, "high")),
            ("TSK-901", String::new()),
            ("TSK-903", override_block("TSK-903", &route, "xhigh")),
            ("TSK-904", override_block("TSK-904", &wrong_route, "high")),
        ],
    );
    let args = |task: &'static str, id: &'static str| -> Vec<String> {
        [
            "--task",
            task,
            "--override",
            id,
            "--route",
            &route,
            "--effort",
            "high",
            "--exclude",
            &excluded,
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
    };

    // Without the first line and without an override, design stays open.
    let open = repo.resolved(
        "design",
        "claude-code",
        "none",
        &["--exclude", &excluded],
        false,
    );
    assert!(open["participants"].as_array().unwrap().is_empty());
    assert_eq!(open["open"][0]["label"], "required");
    // The open reason names the one sanctioned way to fill the duty.
    let reasons = open["open"][0]["reasons"].to_string();
    assert!(
        reasons.contains("committed OPERATOR_OVERRIDE block"),
        "{reasons}"
    );

    // The matching committed block fills design with the seat's second line at high.
    let filled = repo.resolved(
        "design",
        "claude-code",
        "none",
        &with(&args("TSK-900", "TSK-900")),
        true,
    );
    let designer = &filled["participants"][0];
    assert_eq!(designer["seat"], d.owner.as_str());
    assert_eq!(designer["line"], d.second.as_str());
    assert_eq!(designer["pinned_id"], d.second_pinned.as_str());
    assert_eq!(designer["effort"], "high");
    assert_eq!(designer["operator_override"]["task"], "TSK-900");
    assert_pinned_launches(&catalog, &filled);

    // A fabricated block that exists only in the working tree grants nothing.
    write(
        repo.root(),
        "project-management/tasks/TSK-902.md",
        &task("TSK-902", &override_block("TSK-902", &route, "high")),
    );
    repo.refused(
        "design",
        &with(&args("TSK-902", "TSK-902")),
        "task TSK-902 not committed",
    );
    // Mismatched blocks: the committed effort or route differs from the invocation.
    repo.refused(
        "design",
        &with(&args("TSK-903", "TSK-903")),
        "OPERATOR_OVERRIDE effort mismatch",
    );
    repo.refused(
        "design",
        &with(&args("TSK-904", "TSK-904")),
        "OPERATOR_OVERRIDE route mismatch",
    );
    // The same block grants nothing for another task or another duty.
    repo.refused(
        "design",
        &with(&args("TSK-901", "TSK-900")),
        "OPERATOR_OVERRIDE task mismatch",
    );
    repo.refused(
        "design",
        &with(&args("TSK-901", "TSK-901")),
        "Execution contract requires exactly one OPERATOR_OVERRIDE block",
    );
    repo.refused(
        "orchestrate",
        &with(&args("TSK-900", "TSK-900")),
        "OPERATOR_OVERRIDE is only valid for design",
    );
}

/// A newer designated version for `seat_id`'s first line, and that duty's seat
/// alternative moved to `effort`: catalog data only.
fn with_newer_first_line_version(
    catalog: &mut Value,
    seat_id: &str,
    duty: &str,
    effort: &str,
) -> String {
    let first = seat(catalog, seat_id)["lines"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let mut next = newest(catalog, &first);
    let pinned = format!("fixture-next-{first}");
    next["id"] = json!(pinned);
    next["pinned_id"] = json!(pinned);
    next["alias"] = json!(format!("{pinned}-alias"));
    for selector in next["selectors"].as_object_mut().unwrap().values_mut() {
        *selector = json!(pinned);
    }
    next["efforts"] = json!(["medium", "high", "xhigh"]);
    let lines = catalog["lines"].as_array_mut().unwrap();
    let target = lines
        .iter_mut()
        .find(|l| l["id"] == first.as_str())
        .unwrap();
    target["versions"].as_array_mut().unwrap().push(next);
    for participant in catalog["duties"][duty]["required"].as_array_mut().unwrap() {
        for alternative in participant["alternatives"].as_array_mut().unwrap() {
            if alternative["target"]["id"] == seat_id {
                alternative["effort"] = json!(effort);
            }
        }
    }
    pinned
}

#[test]
fn catalog_data_alone_moves_resolution_fallback_and_open_duties() {
    let catalog = managed();
    let codex = seat_of_lineage(&catalog, "codex");
    let codex_id = codex["id"].as_str().unwrap();
    let owner = catalog["design_owner"].as_str().unwrap().to_owned();
    let repo = Repo::new(&catalog);

    let review = repo.resolved("unit-review", "claude-code", "claude", &[], true);
    let design = repo.resolved("design", "claude-code", "none", &[], true);
    for result in [&review, &design] {
        assert_pinned_launches(&catalog, result);
        assert_eq!(result["participants"][0]["effort"], "high");
    }
    assert_eq!(review["participants"][0]["seat"], codex_id);

    // A catalog-only change moves the requested selector and effort.
    let mut changed = catalog.clone();
    let review_pin = with_newer_first_line_version(&mut changed, codex_id, "unit-review", "xhigh");
    let design_pin = with_newer_first_line_version(&mut changed, &owner, "design", "xhigh");
    for version in changed["lines"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|l| l["versions"].as_array_mut().unwrap())
    {
        if version["pinned_id"] == review_pin.as_str() {
            version["designations"] =
                json!([{"seat": codex_id, "date": "2026-09-23", "record": "fixture"}]);
        }
        if version["pinned_id"] == design_pin.as_str() {
            version["designations"] =
                json!([{"seat": owner, "date": "2026-09-23", "record": "fixture"}]);
        }
    }
    let repo = Repo::new(&changed);
    for (duty, author, pin) in [
        ("unit-review", "claude", &review_pin),
        ("design", "none", &design_pin),
    ] {
        let result = repo.resolved(duty, "claude-code", author, &[], true);
        assert_ne!(
            result["participants"][0]["pinned_id"],
            (if duty == "design" { &design } else { &review })["participants"][0]["pinned_id"]
        );
        assert_eq!(result["participants"][0]["pinned_id"], pin.as_str());
        assert_eq!(
            result["participants"][0]["requested_selector"],
            pin.as_str()
        );
        assert_eq!(result["participants"][0]["effort"], "xhigh");
        assert_pinned_launches(&changed, &result);
    }
}

#[test]
fn bucket_exclusion_moves_the_fallback_and_no_codex_leaves_review_open() {
    let catalog = managed();
    let codex = seat_of_lineage(&catalog, "codex");
    // A usage-bucket exclusion moves bounded execution to its next alternative.
    let repo = Repo::new(&catalog);
    let alternatives = catalog["duties"]["bounded-execution"]["required"][0]["alternatives"]
        .as_array()
        .unwrap();
    let first_line = alternatives[0]["target"]["id"].as_str().unwrap();
    let bucket = family(
        &catalog,
        line(&catalog, first_line)["family"].as_str().unwrap(),
    )["usage_bucket"]
        .as_str()
        .unwrap();
    let before = repo.resolved("bounded-execution", "claude-code", "none", &[], true);
    assert_eq!(before["participants"][0]["line"], first_line);
    let excluded = format!("bucket:{bucket}");
    let after = repo.resolved(
        "bounded-execution",
        "claude-code",
        "none",
        &["--exclude", &excluded],
        true,
    );
    let fallback = alternatives
        .iter()
        .map(|a| a["target"]["id"].as_str().unwrap())
        .find(|id| {
            family(&catalog, line(&catalog, id)["family"].as_str().unwrap())["usage_bucket"]
                != bucket
        })
        .unwrap();
    assert_eq!(after["participants"][0]["line"], fallback);

    // With no eligible Codex version, the Claude-authored unit review stays
    // open; the Claude seat never substitutes for the opposite lineage.
    let codex_versions: Vec<String> = catalog["lines"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["family"] == codex["family"])
        .flat_map(|l| l["versions"].as_array().unwrap())
        .map(|v| v["id"].as_str().unwrap().to_owned())
        .collect();
    write(
        &repo.home(),
        "model-catalog.local.json",
        &json!({"schema_version": 1, "additions": [], "exclusions": codex_versions}).to_string(),
    );
    let open = repo.resolved("unit-review", "claude-code", "claude", &[], false);
    assert!(
        open["participants"].as_array().unwrap().is_empty(),
        "{open}"
    );
    assert_eq!(open["open"][0]["participant"], "independent");
}

#[test]
fn xhigh_trigger_keeps_the_design_seat_at_high_and_adds_a_worker() {
    let catalog = managed();
    let owner = catalog["design_owner"].as_str().unwrap().to_owned();
    let repo = Repo::new(&catalog);
    // An xhigh trigger keeps the design seat at high and adds an xhigh worker.
    let trigger = catalog["xhigh_triggers"][0].as_str().unwrap();
    let design = repo.resolved(
        "design",
        "claude-code",
        "none",
        &["--trigger", trigger],
        true,
    );
    assert_eq!(design["participants"][0]["seat"], owner.as_str());
    assert_eq!(design["participants"][0]["effort"], "high");
    let worker = &design["obligations"][0];
    assert_eq!(worker["participant"], format!("xhigh-reasoning:{owner}"));
    assert_eq!(worker["effort"], "xhigh");
    assert!(worker["seat"].is_null());
    assert_eq!(worker["approval_owner"], owner.as_str());
    assert_eq!(design["xhigh_trigger_met"], true);
    assert_pinned_launches(&catalog, &design);
}
