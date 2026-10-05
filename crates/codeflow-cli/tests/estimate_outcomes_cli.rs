//! `codeflow estimate outcomes` and the estimates status line (TSK-239,
//! issue 77): timings derived from a fixture repository built here, with
//! fixed author and committer times, joined to forecasts written here.
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The fixture's clock origin, in epoch seconds.
const T0: i64 = 1_800_000_000;
const HOUR: i64 = 3600;

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        let home = dir.path().join("home");
        std::fs::create_dir_all(root.join("project-management/tasks")).unwrap();
        std::fs::create_dir_all(dir.path().join("no-hooks")).unwrap();
        let fixture = Self {
            root,
            home,
            _dir: dir,
        };
        fixture.git(&["init", "-q", "-b", "main"], T0);
        let hooks = fixture.root.parent().unwrap().join("no-hooks");
        fixture.git(&["config", "core.hooksPath", hooks.to_str().unwrap()], T0);
        std::fs::write(fixture.root.join("README.md"), "fixture\n").unwrap();
        fixture.commit("chore: start", T0);
        fixture
    }

    fn git_out(&self, args: &[&str], at: i64) -> String {
        let date = format!("{at} +0000");
        let out = Command::new("git")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=f@example.invalid",
            ])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(&self.root)
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn git(&self, args: &[&str], at: i64) {
        self.git_out(args, at);
    }

    /// Commit everything and return the commit id.
    fn commit(&self, message: &str, at: i64) -> String {
        self.git(&["add", "-A"], at);
        self.git(&["commit", "-q", "--allow-empty", "-m", message], at);
        self.git_out(&["rev-parse", "HEAD"], at)
    }

    fn merge(&self, branch: &str, at: i64) -> String {
        self.git(&["checkout", "-q", "main"], at);
        self.git(
            &[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                &format!("merge {branch}"),
                branch,
            ],
            at,
        );
        self.git_out(&["rev-parse", "HEAD"], at)
    }

    fn record(&self, id: &str, status: &str, reviewed: Option<&str>) {
        let closeout = reviewed.map_or_else(String::new, |sha| {
            format!(
                "\n```yaml\nacceptance:\n  reviewed: {sha}\n  review: fixture review\n  criteria:\n    AC-1: verified | fixture\n  journey: verified | fixture\n  not_verified: none\n  follow_ups: none\n  verdict: approved\n```\n"
            )
        });
        let text = format!(
            "---\nid: {id}\nepic_id: null\nstandalone_reason: \"fixture\"\ntitle: \"Fixture task {id}\"\nstatus: {status}\nwork_type: feat\nspecs: []\ndepends_on: []\nintegration_target: \"main\"\nexternal_refs: []\ncreated: 2027-01-15\n---\n\n# {id}: Fixture task\n\n## Description\n\nFixture.\n\n## Acceptance Criteria\n\n- AC-1 Fixture.\n\n## Closeout\n{closeout}"
        );
        std::fs::write(
            self.root.join(format!("project-management/tasks/{id}.md")),
            text,
        )
        .unwrap();
    }

    /// Plan `id` by a no-fast-forward planning merge at `at`; returns the
    /// merge.
    fn plan(&self, id: &str, at: i64) -> String {
        let branch = format!("plan/{id}");
        self.git(&["checkout", "-q", "-b", &branch, "main"], at);
        self.record(id, "todo", None);
        self.commit(&format!("docs: plan {id}"), at - 100);
        self.merge(&branch, at)
    }

    fn code(&self, name: &str, at: i64) -> String {
        std::fs::write(self.root.join(name), format!("{at}\n")).unwrap();
        self.commit(&format!("feat: {name}"), at)
    }

    /// Work `id` from `start`: a first commit, the reviewed commit at
    /// `start + active / 2`, completion at `start + active`, landing an hour
    /// later. Returns (first, reviewed, completion, landing).
    fn deliver(&self, id: &str, start: i64, active: i64) -> [String; 4] {
        let branch = format!("task/{id}-fixture");
        self.git(&["checkout", "-q", "-b", &branch, "main"], start);
        let first = self.code(&format!("{id}-a.txt"), start);
        let reviewed = self.code(&format!("{id}-b.txt"), start + active / 2);
        self.record(id, "complete", Some(&reviewed));
        let completion = self.commit(&format!("docs: complete {id}"), start + active);
        let landing = self.merge(&branch, start + active + HOUR);
        [first, reviewed, completion, landing]
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_codeflow"))
            .args(args)
            .current_dir(&self.root)
            .env("CODEFLOW_HOME", &self.home)
            .env("HOME", &self.home)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap()
    }

    fn report(&self, extra: &[&str]) -> (Option<i32>, Value) {
        let mut args = vec!["estimate", "outcomes", "--json"];
        args.extend_from_slice(extra);
        let out = self.run(&args);
        let value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
            panic!(
                "not JSON:\n{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        });
        (out.status.code(), value)
    }

    fn adopt(&self, root: &str) {
        std::fs::create_dir_all(self.root.join(".codeflow")).unwrap();
        std::fs::write(
            self.root.join(".codeflow/estimate.json"),
            json!({
                "schema_version": 1,
                "status": "adopted",
                "rationale": "fixture",
                "method_version": "codeflow-agentic-1",
                "root": root,
                "re_offer_when": "never"
            })
            .to_string(),
        )
        .unwrap();
    }
}

fn task<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["task_id"] == id)
        .unwrap_or_else(|| panic!("{id} missing from {report:#}"))
}

fn at(point: &Value) -> (String, i64) {
    (
        point["commit"].as_str().unwrap().to_string(),
        point["epoch_seconds"].as_i64().unwrap(),
    )
}

/// A forecast whose packages name `tasks`, each planned at 4 hours over
/// implement, review and verify, with discovery and integration outside.
fn forecast(tasks: &[&str]) -> String {
    let pin = json!({"path": "profile.md", "sha256": "0".repeat(64)});
    let packages: Vec<Value> = tasks
        .iter()
        .map(|id| {
            json!({
                "id": format!("pkg-{id}"),
                "source": {"kind": "codeflow_task", "task_id": id, "sha256": "0".repeat(64)},
                "context_pins": [],
                "grade": "medium",
                "grade_evidence": ["fixture"],
                "duration_basis": "judgment",
                "stage_exclusions": []
            })
        })
        .collect();
    let activity = |id: &str, package: Option<String>, stage: &str, start: i64, duration: i64| {
        json!({
            "id": id, "package_id": package, "stage": stage, "start_seconds": start,
            "duration_seconds": duration, "demands": [{"resource_id": "producer", "units": 1}],
            "after": [], "after_boundaries": [], "basis": "fixture"
        })
    };
    let scenario = |name: &str, scale: i64| {
        let mut activities = Vec::new();
        for id in tasks {
            let package = Some(format!("pkg-{id}"));
            activities.push(activity(
                &format!("{id}-discover"),
                package.clone(),
                "discovery",
                0,
                2 * HOUR,
            ));
            activities.push(activity(
                &format!("{id}-implement"),
                package.clone(),
                "implement",
                2 * HOUR,
                2 * HOUR * scale,
            ));
            activities.push(activity(
                &format!("{id}-review"),
                package.clone(),
                "review",
                2 * HOUR + 2 * HOUR * scale,
                HOUR * scale,
            ));
            activities.push(activity(
                &format!("{id}-verify"),
                package.clone(),
                "verify",
                2 * HOUR + 3 * HOUR * scale,
                HOUR * scale,
            ));
        }
        activities.push(activity("integrate", None, "integrate", 20 * HOUR, HOUR));
        json!({"name": name, "assumptions": ["fixture"], "activities": activities, "milestones": []})
    };
    json!({
        "schema_version": 1, "id": "fixture", "anchor_epoch_seconds": T0, "horizon_seconds": 30 * 24 * HOUR,
        "profile": pin, "rubric": pin, "packages": packages, "boundaries": [],
        "resources": [{"id": "producer", "capacity": 1, "windows": [{"start_seconds": 0, "end_seconds": 30 * 24 * HOUR}]}],
        "scenarios": [scenario("favorable", 1), scenario("planning", 1), scenario("adverse", 2)]
    })
    .to_string()
}

/// AC-1: planned, started, completed and landed are the planning merge, the
/// first task commit, the completion commit and the landing merge, and
/// elapsed active is completed minus started.
#[test]
fn outcomes_derive_the_four_points_of_a_landed_task() {
    let fx = Fixture::new();
    let planning = fx.plan("TSK-001", T0 + 200);
    let [first, _, completion, landing] = fx.deliver("TSK-001", T0 + 1000, 2 * HOUR);
    let (code, report) = fx.report(&[]);
    assert_eq!(code, Some(0), "{report:#}");
    let outcome = task(&report, "TSK-001");
    assert_eq!(at(&outcome["planned"]), (planning, T0 + 200));
    assert_eq!(at(&outcome["started"]), (first, T0 + 1000));
    assert_eq!(
        at(&outcome["completed"]),
        (completion, T0 + 1000 + 2 * HOUR)
    );
    assert_eq!(at(&outcome["landed"]), (landing, T0 + 1000 + 3 * HOUR));
    assert_eq!(outcome["elapsed_active_seconds"], 2 * HOUR);
    assert_eq!(outcome["lead_seconds"], 1000 + 3 * HOUR - 200);
    assert_eq!(outcome["blocked"], json!([]));
}

/// AC-2: a blocked span on the task branch is listed with its two commits
/// and subtracted from elapsed active.
#[test]
fn a_blocked_span_is_listed_and_subtracted() {
    let fx = Fixture::new();
    fx.plan("TSK-001", T0 + 200);
    fx.git(
        &["checkout", "-q", "-b", "task/TSK-001-fixture", "main"],
        T0 + 1000,
    );
    let first = fx.code("a.txt", T0 + 1000);
    fx.record("TSK-001", "blocked", None);
    let block = fx.commit("docs: block TSK-001", T0 + 2000);
    fx.record("TSK-001", "todo", None);
    let clear = fx.commit("docs: clear TSK-001", T0 + 2600);
    let reviewed = fx.code("b.txt", T0 + 4000);
    fx.record("TSK-001", "complete", Some(&reviewed));
    fx.commit("docs: complete TSK-001", T0 + 9000);
    fx.merge("task/TSK-001-fixture", T0 + 9500);
    let (code, report) = fx.report(&[]);
    assert_eq!(code, Some(0), "{report:#}");
    let outcome = task(&report, "TSK-001");
    assert_eq!(at(&outcome["started"]), (first, T0 + 1000));
    let spans = outcome["blocked"].as_array().unwrap();
    assert_eq!(spans.len(), 1, "{outcome:#}");
    assert_eq!(at(&spans[0]["from"]), (block, T0 + 2000));
    assert_eq!(at(&spans[0]["to"]), (clear, T0 + 2600));
    assert_eq!(outcome["elapsed_active_seconds"], 9000 - 1000 - 600);
}

fn three_delivered(fx: &Fixture) {
    // Ratios 0.3, 0.3 and 0.4 of a 4 hour planning span.
    for (index, (id, active)) in [("TSK-001", 4320), ("TSK-002", 4320), ("TSK-003", 5760)]
        .into_iter()
        .enumerate()
    {
        let base = T0 + i64::try_from(index).unwrap() * 10 * HOUR;
        fx.plan(id, base + 200);
        fx.deliver(id, base + 1000, active);
    }
}

/// AC-3: the forecast join gives each task its ratio; three ratios at 0.3,
/// 0.3 and 0.4 print the median and the contradiction line, two print
/// "below minimum" and no verdict.
#[test]
fn ratios_join_the_planning_scenario_and_judge_only_past_the_minimum() {
    let fx = Fixture::new();
    three_delivered(&fx);
    let home = "project-management/estimates";
    fx.adopt(home);
    let dir = fx.root.join(home).join("forecasts/release");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("v1.json"),
        forecast(&["TSK-001", "TSK-002", "TSK-003"]),
    )
    .unwrap();

    let (code, report) = fx.report(&[]);
    assert_eq!(code, Some(0), "{report:#}");
    let ratio = |id: &str| task(&report, id)["ratio"].as_f64().unwrap();
    assert!((ratio("TSK-001") - 0.3).abs() < 1e-9);
    assert!((ratio("TSK-003") - 0.4).abs() < 1e-9);
    let prediction = &task(&report, "TSK-001")["forecast"];
    assert_eq!(prediction["predicted_active_seconds"], 4 * HOUR);
    assert_eq!(
        prediction["path"],
        format!("{home}/forecasts/release/v1.json")
    );
    let feat = report["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|group| group["work_type"] == "feat")
        .unwrap();
    assert_eq!(feat["verdict"], "contradicts");
    assert!((feat["median"].as_f64().unwrap() - 0.3).abs() < 1e-9);
    let text = String::from_utf8_lossy(&fx.run(&["estimate", "outcomes"]).stdout).into_owned();
    assert!(
        text.contains("group feat: 3 ratio(s), median 0.30, range 0.30 to 0.40: outcomes contradict the forecast"),
        "{text}"
    );
    assert!(
        text.contains("thresholds: a verdict needs 3 ratios"),
        "{text}"
    );

    // Two joined tasks: below the minimum, no verdict.
    let two = fx.root.join("two.json");
    std::fs::write(&two, forecast(&["TSK-001", "TSK-002"])).unwrap();
    let out = fx.run(&["estimate", "outcomes", "--forecast", "two.json"]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("below minimum (2 of 3), no verdict"),
        "{text}"
    );
    let groups: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("group "))
        .collect();
    assert!(!groups.is_empty(), "{text}");
    assert!(
        groups
            .iter()
            .all(|line| !line.contains("contradict") && !line.contains("median")),
        "{text}"
    );
}

/// AC-4: an adopted home that does not exist is reported and fails; no
/// adoption record still derives timings, exits 0 and says no forecast was
/// joined.
#[test]
fn a_missing_home_fails_and_no_adoption_still_reports() {
    let fx = Fixture::new();
    fx.plan("TSK-001", T0 + 200);
    fx.deliver("TSK-001", T0 + 1000, HOUR);

    let (code, report) = fx.report(&[]);
    assert_eq!(code, Some(0), "{report:#}");
    assert!(task(&report, "TSK-001")["elapsed_active_seconds"].is_number());
    let text = String::from_utf8_lossy(&fx.run(&["estimate", "outcomes"]).stdout).into_owned();
    assert!(text.contains("no forecast joined"), "{text}");

    fx.adopt("project-management/estimates");
    let out = fx.run(&["estimate", "outcomes"]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("estimate.json adopted; root project-management/estimates missing: no profile, no frozen forecast, no outcomes"),
        "{text}"
    );
    assert!(text.contains("TSK-001"), "{text}");
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.insert(path.clone(), std::fs::read(&path).unwrap());
            }
        }
    }
    out
}

/// AC-5: the command writes nothing in the repository, its git directory
/// or the user registry.
#[test]
fn outcomes_write_nothing() {
    let fx = Fixture::new();
    three_delivered(&fx);
    fx.adopt("project-management/estimates");
    let dir = fx
        .root
        .join("project-management/estimates/forecasts/release");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("v1.json"), forecast(&["TSK-001"])).unwrap();
    let before = snapshot(&fx.root);
    for args in [
        &["estimate", "outcomes"][..],
        &["estimate", "outcomes", "--json"],
    ] {
        assert_eq!(fx.run(args).status.code(), Some(0));
    }
    assert_eq!(before, snapshot(&fx.root), "the repository changed");
    assert!(!fx.home.exists(), "the user registry home was written");
}

/// AC-6: `codeflow status` has an estimates line only with an adoption
/// record, and it names a missing home.
#[test]
fn status_names_estimates_only_when_adopted() {
    let fx = Fixture::new();
    three_delivered(&fx);
    let text = |fx: &Fixture| String::from_utf8_lossy(&fx.run(&["status"]).stdout).into_owned();
    assert!(!text(&fx).contains("estimates:"), "{}", text(&fx));
    fx.adopt("project-management/estimates");
    let missing = text(&fx);
    assert!(
        missing.contains("estimates: adopted; home missing (project-management/estimates)"),
        "{missing}"
    );
    let dir = fx
        .root
        .join("project-management/estimates/forecasts/release");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("v1.json"), forecast(&["TSK-001"])).unwrap();
    let present = text(&fx);
    assert!(
        present.contains("estimates: adopted; 1 frozen forecast(s); 3 completed task(s) to compare; run `codeflow estimate outcomes`"),
        "{present}"
    );
}

/// AC-8: a squash landing and a branch with no commit before the reviewed
/// one give `started` as unknown with the reason, and no elapsed number.
#[test]
fn started_is_unknown_without_a_task_branch_history() {
    let fx = Fixture::new();
    // Squash: the task branch never reaches main.
    fx.plan("TSK-001", T0 + 200);
    fx.git(
        &["checkout", "-q", "-b", "task/TSK-001-fixture", "main"],
        T0 + 1000,
    );
    fx.code("a.txt", T0 + 1000);
    let reviewed = fx.code("b.txt", T0 + 2000);
    fx.record("TSK-001", "complete", Some(&reviewed));
    fx.commit("docs: complete TSK-001", T0 + 3000);
    fx.git(&["checkout", "-q", "main"], T0 + 4000);
    fx.git(
        &["merge", "-q", "--squash", "task/TSK-001-fixture"],
        T0 + 4000,
    );
    let squash = fx.commit("feat: TSK-001 squashed", T0 + 4000);
    // One reviewed commit, then the completion.
    fx.plan("TSK-002", T0 + 5000);
    fx.git(
        &["checkout", "-q", "-b", "task/TSK-002-fixture", "main"],
        T0 + 6000,
    );
    let only = fx.code("c.txt", T0 + 6000);
    fx.record("TSK-002", "complete", Some(&only));
    fx.commit("docs: complete TSK-002", T0 + 7000);
    fx.merge("task/TSK-002-fixture", T0 + 8000);

    let (code, report) = fx.report(&[]);
    assert_eq!(code, Some(0), "{report:#}");
    let squashed = task(&report, "TSK-001");
    assert!(
        squashed["started"]["unknown"]
            .as_str()
            .unwrap()
            .contains("landed without a merge commit"),
        "{squashed:#}"
    );
    assert_eq!(at(&squashed["landed"]), (squash, T0 + 4000));
    assert!(squashed["elapsed_active_seconds"].is_null());
    let single = task(&report, "TSK-002");
    assert_eq!(
        single["started"]["unknown"],
        "no commit before the reviewed one on the task branch"
    );
    assert!(single["elapsed_active_seconds"].is_null());
    let text = String::from_utf8_lossy(&fx.run(&["estimate", "outcomes"]).stdout).into_owned();
    assert!(
        text.contains("started    unknown: landed without a merge commit"),
        "{text}"
    );
}

/// The date filter and bad input.
#[test]
fn since_filters_and_a_bad_date_is_refused() {
    let fx = Fixture::new();
    three_delivered(&fx);
    let (_, report) = fx.report(&["--since", "2099-01-01"]);
    assert_eq!(report["tasks"], json!([]));
    let (_, report) = fx.report(&["--since", "2000-01-01"]);
    assert_eq!(report["tasks"].as_array().unwrap().len(), 3);
    let out = fx.run(&["estimate", "outcomes", "--since", "yesterday"]);
    assert_eq!(out.status.code(), Some(2));
}
