//! Unit tests for the progress facts, over plain inputs.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::inputs::{
    AcceptanceIn, BlockerIn, BranchIn, EpicIn, ForecastIn, ForgeIn, Inputs, LaneIn, LedgerIn,
    PullIn, RefIn, ScopeSpec, TaskIn, UnreadableIn,
};
use super::owed::{Decision, OwedItem, OwedKind, OwedRead, OwedSource};
use super::{facts, to_json};

const NOW: i64 = 1_793_534_400; // 2026-11-01T12:00:00Z
const LINE: &str = "integration/EPC-001-demo";

fn task(id: &str, status: &str, state: Option<&str>) -> TaskIn {
    TaskIn {
        id: id.into(),
        title: format!("{id} title"),
        epic: Some("EPC-001".into()),
        status: status.into(),
        target: LINE.into(),
        depends_on: Vec::new(),
        created: Some("2026-10-30".into()),
        state: state.map(str::to_string),
        state_reason: String::new(),
        blocker: None,
        theme: None,
        outcome: Some(format!("{id} outcome")),
        about: None,
        follow_ups: None,
        acceptance: None,
        branch: None,
        on_main: Some(false),
        record_on_line: true,
        landed_at: None,
    }
}

fn branch(name: &str, ahead: usize) -> BranchIn {
    BranchIn {
        name: name.into(),
        head: "a".repeat(40),
        commits_ahead: ahead,
        head_time: NOW - 600,
        worktree: Some(format!("/work/{name}")),
    }
}

fn pull(number: u64, task: &str, draft: bool, checks: &str) -> PullIn {
    PullIn {
        number,
        title: format!("{task} pull"),
        head_ref: format!("task/{task}-x"),
        head_sha: "b".repeat(40),
        base: LINE.into(),
        draft,
        checks: checks.into(),
        task: Some(task.into()),
    }
}

/// One epic with every stage: not started, in progress, in review,
/// reviewed and landed, plus a cancelled task.
fn inputs() -> Inputs {
    let mut started = task("TSK-002", "todo", Some("active"));
    started.branch = Some(branch("task/TSK-002-x", 2));
    let mut review = task("TSK-003", "todo", Some("active"));
    review.branch = Some(branch("task/TSK-003-x", 3));
    let mut reviewed = task("TSK-004", "todo", Some("active"));
    reviewed.branch = Some(branch("task/TSK-004-x", 4));
    reviewed.acceptance = Some(AcceptanceIn {
        verified: 3,
        total: 3,
        reviewed: "c".repeat(40),
        approved: true,
        bound: true,
    });
    let mut landed = task("TSK-005", "complete", None);
    landed.landed_at = Some(NOW - 3_600);
    let mut fresh = task("TSK-001", "todo", Some("ready"));
    fresh.depends_on = Vec::new();
    Inputs {
        now: NOW,
        repository: "github.com/acme/demo".into(),
        repo_path: "/repo".into(),
        scope: ScopeSpec::Epics(vec!["EPC-001".into()]),
        current_target: LINE.into(),
        default_branch: "main".into(),
        default_ref: Some(RefIn {
            name: "origin/main".into(),
            sha: Some("d".repeat(40)),
        }),
        line_refs: BTreeMap::from([(
            LINE.to_string(),
            RefIn {
                name: format!("origin/{LINE}"),
                sha: Some("e".repeat(40)),
            },
        )]),
        fetched_at: Some(NOW - 60),
        sweep_seconds: 1_800,
        tasks: vec![
            fresh,
            started,
            review,
            reviewed,
            landed,
            task("TSK-006", "cancelled", None),
        ],
        epics: vec![EpicIn {
            id: "EPC-001".into(),
            title: "Demo".into(),
            status: "active".into(),
            outcome: Some("the demo ships".into()),
            about: Some("about the demo".into()),
            sections: BTreeMap::from([("TSK-001".into(), "Tasks".into())]),
            capabilities: vec![("CAP-001".into(), "demo".into(), "Demo prose.".into())],
        }],
        unreadable: Vec::new(),
        readiness_problem: None,
        forge: ForgeIn::Read {
            read_at: NOW - 30,
            pulls: vec![
                pull(10, "TSK-003", false, "pending"),
                pull(11, "TSK-004", false, "passed"),
            ],
        },
        ledger: LedgerIn::Absent,
        session: None,
        owed: Vec::new(),
        forecast: None,
        forecast_note: None,
        release_step: false,
        history_read: true,
    }
}

fn json(inputs: &Inputs) -> Value {
    to_json(&facts(inputs), None).expect("facts serialise")
}

fn task_json<'a>(value: &'a Value, id: &str) -> &'a Value {
    value["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == id)
        .unwrap_or_else(|| panic!("{id} in the facts"))
}

#[test]
fn every_stage_has_its_percent_and_cancelled_is_excluded() {
    let value = json(&inputs());
    let stages: Vec<(&str, &str)> = ["TSK-001", "TSK-002", "TSK-003", "TSK-004", "TSK-005"]
        .iter()
        .map(|id| (*id, task_json(&value, id)["stage"].as_str().unwrap()))
        .collect();
    assert_eq!(
        stages,
        [
            ("TSK-001", "not started"),
            ("TSK-002", "in progress"),
            ("TSK-003", "in review"),
            ("TSK-004", "reviewed"),
            ("TSK-005", "landed")
        ]
    );
    // (0 + 20 + 50 + 75 + 100) / 5
    assert_eq!(value["scope"]["percent"], 49.0);
    assert_eq!(value["scope"]["cancelled"], serde_json::json!(["TSK-006"]));
    assert_eq!(value["epics"][0]["percent"], 49.0);
    assert_eq!(value["epics"][0]["done"], serde_json::json!(["TSK-005"]));
    assert_eq!(task_json(&value, "TSK-004")["criteria"]["verified"], 3);
    assert_eq!(task_json(&value, "TSK-001")["criteria"], "not recorded");
}

#[test]
fn a_stale_acceptance_block_reads_stale() {
    let mut inputs = inputs();
    inputs.tasks[3].acceptance.as_mut().unwrap().bound = false;
    let value = json(&inputs);
    let task = task_json(&value, "TSK-004");
    assert_eq!(task["criteria"]["state"], "stale");
    assert_eq!(task["stage"], "in review");
}

#[test]
fn an_unreadable_record_or_an_empty_scope_is_unknown() {
    let mut unreadable = inputs();
    unreadable.unreadable.push(UnreadableIn {
        path: "project-management/tasks/TSK-007.md".into(),
        reason: "bad frontmatter".into(),
        task_id: Some("TSK-007".into()),
    });
    let value = json(&unreadable);
    assert_eq!(value["scope"]["percent"], "unknown");
    assert_eq!(value["epics"][0]["percent"], "unknown");

    let mut empty = inputs();
    empty.scope = ScopeSpec::Epics(vec!["EPC-404".into()]);
    let value = json(&empty);
    assert_eq!(value["scope"]["percent"], "unknown");
    assert_eq!(value["scope"]["tasks"], 0);
}

#[test]
fn locations_follow_reachability_and_pull_requests() {
    let mut inputs = inputs();
    inputs.tasks[4].on_main = Some(true);
    let value = json(&inputs);
    assert_eq!(task_json(&value, "TSK-005")["location"]["kind"], "main");
    assert_eq!(
        task_json(&value, "TSK-004")["location"]["kind"],
        "pull request"
    );
    assert_eq!(
        task_json(&value, "TSK-004")["location"]["substate"],
        format!("approved at {}", "c".repeat(40))
    );
    assert_eq!(task_json(&value, "TSK-002")["location"]["kind"], "branch");
    assert_eq!(
        task_json(&value, "TSK-001")["location"]["note"],
        "record on line"
    );

    // A stale fetch makes every ref-bound location unverified.
    inputs.fetched_at = Some(NOW - 7_200);
    let value = json(&inputs);
    assert_eq!(
        task_json(&value, "TSK-005")["location"]["state"],
        "unverified"
    );
    assert_eq!(value["refs"][0]["state"], "unverified");
}

#[test]
fn order_respects_dependencies_and_marks_the_critical_path_apart_from_depth() {
    let mut inputs = inputs();
    for task in &mut inputs.tasks {
        task.status = "todo".into();
        task.state = Some("ready".into());
        task.branch = None;
        task.acceptance = None;
    }
    inputs.forge = ForgeIn::Read {
        read_at: NOW,
        pulls: Vec::new(),
    };
    // A chain by count (1 > 2 > 3) against one long task (4 > 5) whose
    // forecast durations are longer.
    inputs.tasks[1].depends_on = vec!["TSK-001".into()];
    inputs.tasks[2].depends_on = vec!["TSK-002".into()];
    inputs.tasks[4].depends_on = vec!["TSK-004".into()];
    inputs.tasks[5].status = "cancelled".into();
    inputs.forecast = Some(ForecastIn {
        path: "estimates/forecasts/demo/v1.json".into(),
        stale: false,
        ends: BTreeMap::from([
            ("TSK-001".into(), (60, 120, 180)),
            ("TSK-002".into(), (120, 240, 360)),
            ("TSK-003".into(), (180, 360, 540)),
            ("TSK-004".into(), (3_600, 7_200, 9_000)),
            ("TSK-005".into(), (7_200, 14_400, 18_000)),
        ]),
    });
    let value = json(&inputs);
    let rows: Vec<&str> = value["order"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    let at = |id: &str| rows.iter().position(|row| *row == id).unwrap();
    assert!(at("TSK-001") < at("TSK-002") && at("TSK-002") < at("TSK-003"));
    assert!(at("TSK-004") < at("TSK-005"));
    assert_eq!(value["order"]["depth"], 3);
    assert_eq!(
        value["order"]["critical_path"],
        serde_json::json!(["TSK-004", "TSK-005"])
    );
    assert_eq!(value["windows"]["basis_kind"], "forecast");
}

#[test]
fn a_blocked_predecessor_orders_before_its_waiting_successor() {
    let mut inputs = inputs();
    inputs.tasks[0].status = "blocked".into();
    inputs.tasks[0].state = Some("blocked".into());
    inputs.tasks[0].blocker = Some(BlockerIn {
        reason: Some("waits on a vendor".into()),
        owner: Some("operator".into()),
        revisit: Some("when the vendor replies".into()),
    });
    inputs.tasks[1].depends_on = vec!["TSK-001".into()];
    inputs.tasks[1].state = Some("waiting".into());
    let value = json(&inputs);
    let rows = value["order"]["rows"].as_array().unwrap();
    let at = |id: &str| rows.iter().position(|row| row["id"] == id).unwrap();
    assert!(at("TSK-001") < at("TSK-002"));
    let window = &task_json(&value, "TSK-001")["window"];
    assert!(window["conditional_on"]
        .as_str()
        .unwrap()
        .contains("the blocker clearing"));
    let blocked = value["issues"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["kind"] == "blocked")
        .unwrap();
    assert_eq!(blocked["unblocker"]["kind"], "operator");
    assert_eq!(
        blocked["recommended_action"],
        "revisit: when the vendor replies"
    );
    assert!(blocked["command"]
        .as_str()
        .unwrap()
        .starts_with("cd /repo && "));
    assert_eq!(value["issues"]["operator"]["state"], "owed");
}

#[test]
fn windows_without_lanes_are_assumed_one_lane_and_lanes_change_the_basis() {
    let mut inputs = inputs();
    for task in &mut inputs.tasks {
        if task.state.as_deref() == Some("active") {
            task.state = Some("ready".into());
        }
    }
    let value = json(&inputs);
    assert_eq!(value["windows"]["basis_kind"], "assumed");
    assert_eq!(value["windows"]["lanes_used"], 1);
    assert_eq!(value["lanes"]["note"], "lanes: observed only");

    let observed = json(&super::tests::inputs());
    assert_eq!(observed["windows"]["basis_kind"], "observed");
    assert_eq!(observed["windows"]["lanes_used"], 3);
    assert_eq!(observed["lanes"]["lanes"].as_array().unwrap().len(), 3);
}

#[test]
fn ledger_lanes_replace_observed_ones() {
    let mut inputs = inputs();
    inputs.ledger = LedgerIn::Read {
        lanes: vec![LaneIn {
            id: "L-1".into(),
            stream: "S-1".into(),
            scope: vec!["TSK-002".into(), "TSK-003".into()],
            attempt: 1,
            seat: None,
            reported_at: Some(NOW - 60),
        }],
        operator_items: Vec::new(),
    };
    let value = json(&inputs);
    assert_eq!(value["lanes"]["source"], "ledger");
    let lane = &value["lanes"]["lanes"][0];
    assert_eq!(lane["seat"], "not recorded");
    assert_eq!(lane["percent"], 35.0);
}

#[test]
fn intake_at_capacity_gives_no_finite_finish() {
    let mut inputs = inputs();
    for (index, task) in inputs.tasks.iter_mut().enumerate() {
        task.created = Some("2026-11-01".into());
        task.id = format!("TSK-{:03}", index + 1);
    }
    // One assumed lane gives 168 tasks a week; add that many in the window.
    for index in 0..170 {
        let mut extra = task(&format!("TSK-{:03}", 100 + index), "todo", Some("ready"));
        extra.created = Some("2026-10-31".into());
        inputs.tasks.push(extra);
    }
    for task in &mut inputs.tasks {
        if task.state.as_deref() == Some("active") {
            task.state = Some("ready".into());
            task.branch = None;
        }
    }
    let value = json(&inputs);
    let continues = &value["windows"]["scenarios"][1];
    assert_eq!(continues["finish"]["state"], "not yet estimable");
    assert!(continues["words"]
        .as_str()
        .unwrap()
        .starts_with("no finite finish"));
    // The planned rate never drops to the observed pace.
    assert_eq!(value["pace"]["planned_capacity_rate_per_day"], 24.0);
}

#[test]
fn no_history_reads_unknown_never_zero() {
    let mut inputs = inputs();
    inputs.history_read = false;
    let value = json(&inputs);
    assert_eq!(value["pace"]["observed_landing_rate_per_day"], "unknown");
    assert_eq!(value["pace"]["days"][0]["landed"], "unknown");
    assert_eq!(value["pace"]["days"].as_array().unwrap().len(), 7);
}

#[test]
fn pace_counts_landings_and_additions_per_day() {
    let value = json(&inputs());
    let days = value["pace"]["days"].as_array().unwrap();
    assert_eq!(days[6]["date"], "2026-11-01");
    assert_eq!(days[6]["landed"], serde_json::json!(["TSK-005"]));
    assert_eq!(days[4]["added"].as_array().unwrap().len(), 5);
}

struct Double(&'static str, OwedRead);

impl OwedSource for Double {
    fn name(&self) -> &str {
        self.0
    }
    fn read(&self) -> OwedRead {
        self.1.clone()
    }
}

fn owed(inputs: &mut Inputs, sources: &[Double]) {
    inputs.owed = sources
        .iter()
        .map(|source| (source.name().to_string(), source.read()))
        .collect();
}

#[test]
fn the_operator_set_is_none_owed_only_when_every_source_was_checked() {
    let mut inputs = inputs();
    owed(
        &mut inputs,
        &[
            Double("deferred results", OwedRead::Items(Vec::new())),
            Double("activation", OwedRead::Absent),
        ],
    );
    let value = json(&inputs);
    assert_eq!(value["issues"]["operator"]["state"], "none owed");
    assert_eq!(value["issues"]["complete"], true);

    inputs.forge = ForgeIn::Unverified {
        reason: "offline".into(),
    };
    let value = json(&inputs);
    assert_eq!(value["issues"]["operator"]["state"], "not established");
    assert_eq!(
        value["issues"]["operator"]["unchecked"],
        serde_json::json!(["forge"])
    );
    assert_eq!(value["pulls_into_main"], "unverified");
}

#[test]
fn owed_items_arrive_through_the_source_interface() {
    let mut inputs = inputs();
    let deferred = OwedItem {
        id: "D-1".into(),
        task: Some("TSK-002".into()),
        kind: OwedKind::DeferredResult,
        summary: "AC-3 proved after release".into(),
        owner: "lane L-1".into(),
        event: Some("the release is published".into()),
        recorded_at: None,
        decision: None,
    };
    let action = OwedItem {
        id: "A-1".into(),
        task: None,
        kind: OwedKind::OperatorAction,
        summary: "approve the release".into(),
        owner: "operator".into(),
        event: None,
        recorded_at: None,
        decision: Some(Decision {
            context: Some("3.2.0 is ready".into()),
            options: vec!["publish".into(), "hold".into()],
            recommendation: Some("publish".into()),
            reasoning: None,
        }),
    };
    owed(
        &mut inputs,
        &[
            Double("deferred results", OwedRead::Items(vec![deferred])),
            Double("activation", OwedRead::Items(vec![action])),
        ],
    );
    let value = json(&inputs);
    let issue = value["issues"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["kind"] == "deferred result owed")
        .unwrap();
    assert_eq!(issue["source"], "deferred results");
    let item = &value["issues"]["operator"]["items"][0];
    assert_eq!(item["source"], "activation");
    assert_eq!(item["decision"]["recommendation"], "publish");
    assert_eq!(item["decision"]["reasoning"], "not recorded");

    owed(
        &mut inputs,
        &[Double(
            "activation",
            OwedRead::Unreadable("torn file".into()),
        )],
    );
    let value = json(&inputs);
    assert_eq!(value["issues"]["operator"]["state"], "not established");
    assert_eq!(
        value["issues"]["operator"]["unchecked"],
        serde_json::json!(["activation"])
    );
}

#[test]
fn red_and_pending_checks_and_stale_claims_are_issues() {
    let mut inputs = inputs();
    if let ForgeIn::Read { pulls, .. } = &mut inputs.forge {
        pulls[1].checks = "failed".into();
    }
    inputs.tasks[1].branch.as_mut().unwrap().commits_ahead = 0;
    inputs.tasks[1].branch.as_mut().unwrap().head_time = NOW - 7_200;
    let value = json(&inputs);
    let kinds: BTreeSet<String> = value["issues"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|issue| {
            format!(
                "{} {}",
                issue["subject"].as_str().unwrap(),
                issue["kind"].as_str().unwrap()
            )
        })
        .collect();
    assert!(kinds.contains("TSK-003 required check pending"));
    assert!(kinds.contains("TSK-004 required check red"));
    assert!(kinds.contains("TSK-002 stale claim"));
    let red = value["issues"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["kind"] == "required check red")
        .unwrap();
    assert_eq!(red["command"], "gh pr checks 11 --repo acme/demo");
}

#[test]
fn the_facts_are_deterministic_whatever_the_record_order() {
    let first = inputs();
    let mut second = inputs();
    second.tasks.reverse();
    let a = serde_json::to_string(&json(&first)).unwrap();
    let b = serde_json::to_string(&json(&second)).unwrap();
    assert_eq!(a, b);
}

#[test]
fn since_lists_changes_superseded_and_not_observed() {
    let before = inputs();
    let mut after = inputs();
    // TSK-004 landed; its red check cleared with it.
    after.tasks[3].status = "complete".into();
    after.tasks[3].state = None;
    after.tasks.push(task("TSK-008", "todo", Some("ready")));
    let previous = json(&before);
    let value = to_json(&facts(&after), Some(&previous)).unwrap();
    let changes: Vec<String> = value["since"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            format!(
                "{} {}",
                c["change"].as_str().unwrap(),
                c["subject"].as_str().unwrap()
            )
        })
        .collect();
    assert!(
        changes.contains(&"landed TSK-004".to_string()),
        "{changes:?}"
    );
    assert!(
        changes.contains(&"task added TSK-008".to_string()),
        "{changes:?}"
    );
    assert_eq!(
        value["windows"]["scenarios"][0]["intake_since_previous"],
        serde_json::json!(["TSK-008"])
    );

    // The forge became unreachable: its pull request issue is not
    // observed, never superseded, and the retained task keeps its earlier
    // location.
    let mut offline = inputs();
    offline.forge = ForgeIn::Unverified {
        reason: "offline".into(),
    };
    let value = to_json(&facts(&offline), Some(&previous)).unwrap();
    let not_observed = value["since"]["not_observed"].as_array().unwrap();
    assert!(not_observed
        .iter()
        .any(|item| item["subject"] == "required check pending TSK-003"));
    assert!(not_observed
        .iter()
        .any(|item| item["subject"] == "TSK-003" && item["field"] == "location"));
    assert!(value["since"]["superseded"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["family"] != "issue"));
}

#[test]
fn since_refuses_another_scope_or_repository() {
    let previous = json(&inputs());
    let mut other_scope = inputs();
    other_scope.scope = ScopeSpec::Task("TSK-001".into());
    let error = to_json(&facts(&other_scope), Some(&previous)).unwrap_err();
    assert!(error.contains("scope kind"), "{error}");

    let mut other_repo = inputs();
    other_repo.repository = "github.com/other/demo".into();
    let error = to_json(&facts(&other_repo), Some(&previous)).unwrap_err();
    assert!(error.contains("repository"), "{error}");
}

#[test]
fn a_session_fact_fills_a_recommendation_and_never_a_percent() {
    let session = super::session::parse(
        br#"{"schema_version":1,"facts":[{"observed_at":"2026-11-01T11:00:00Z","lane":"L-1","task":"TSK-003","builder":"codex","no_progress":true,"recommendation":{"action":"rerun the checks","why":"a runner timed out","alternatives":["wait"]}}]}"#,
    )
    .unwrap();
    let mut inputs = inputs();
    let before = json(&inputs);
    inputs.session = Some(session);
    let value = json(&inputs);
    assert_eq!(value["scope"]["percent"], before["scope"]["percent"]);
    assert_eq!(value["lanes"]["session"][0]["label"], "session");
    let issue = value["issues"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["subject"] == "TSK-003")
        .unwrap();
    assert_eq!(issue["recommended_action"], "rerun the checks");
    assert_eq!(issue["source"], "session");
    assert!(super::session::parse(
        br#"{"schema_version":1,"facts":[{"observed_at":"x","lane":"L"}]}"#
    )
    .is_err());
    assert!(super::session::parse(br#"{"schema_version":1,"facts":[{"observed_at":"2026-11-01T11:00:00Z","lane":"L","seat_owner":"x"}]}"#).unwrap_err().contains("seat_owner"));
}

#[test]
fn features_and_themes_come_from_the_records() {
    let mut inputs = inputs();
    inputs.tasks[1].theme = Some("Speed".into());
    let value = json(&inputs);
    let epic = &value["epics"][0];
    assert_eq!(epic["features"][0]["about"], "Demo prose.");
    let themes: Vec<&str> = epic["themes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|theme| theme["name"].as_str().unwrap())
        .collect();
    assert_eq!(themes, ["Speed", "Tasks", "ungrouped"]);
    assert_eq!(value["scope"]["deliverable"], "the demo ships");
}

#[cfg(unix)]
#[test]
fn a_hung_forge_read_ends_with_its_bound_and_reads_unverified() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    repo.remote("origin", "https://github.com/acme/demo.git")
        .unwrap();
    let gh = dir.path().join("hung-gh");
    std::fs::write(&gh, "#!/bin/sh\nsleep 30\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut options = super::Options::new(ScopeSpec::Default);
    options.now = Some(NOW);
    options.gh_program = gh.into_os_string();
    options.forge_limit = std::time::Duration::from_secs(1);
    let started = std::time::Instant::now();
    let inputs = super::collect(dir.path(), &options).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert!(matches!(inputs.forge, ForgeIn::Unverified { .. }));
    let value = json(&inputs);
    assert_eq!(value["issues"]["operator"]["state"], "not established");
}
