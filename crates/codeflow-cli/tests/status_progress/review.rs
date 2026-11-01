//! The review round's fail-closed cases, each through the real gatherer:
//! lost coverage, acceptance against the current criteria, location
//! proofs, windows, check runs, pace, `--since` and quoted commands.
use super::*;

/// A one-commit repository whose `origin/<branch>` refs are set by hand
/// and whose refs read as fetched a minute before the snapshot.
struct Repo {
    _dir: tempfile::TempDir,
    base: PathBuf,
    root: PathBuf,
    bin: PathBuf,
}

const DATE: &str = "2026-11-01T10:00:00Z";

impl Repo {
    fn new(name: &str, records: &[(&str, String)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let root = base.join(name);
        let bin = base.join("bin");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let repo = Self {
            _dir: dir,
            base,
            root,
            bin,
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/demo.git",
        ]);
        write(
            &repo.root,
            "project-management/epics/EPC-001.md",
            "---\nid: EPC-001\ntitle: Demo\nstatus: active\n---\n\n## Outcome\n\nDemo ships.\n\n## Core work\n\nThe first slice of the demo.\n\n- TSK-001\n",
        );
        for (id, text) in records {
            repo.record(id, text);
        }
        repo.commit();
        repo.sync("main");
        repo.gh("printf '%s\\n' '[]'");
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.test")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.test")
            .env("GIT_AUTHOR_DATE", DATE)
            .env("GIT_COMMITTER_DATE", DATE)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().into()
    }

    fn record(&self, id: &str, text: &str) {
        write(
            &self.root,
            &format!("project-management/tasks/{id}.md"),
            text,
        );
    }

    fn commit(&self) -> String {
        self.git(&["add", "."]);
        self.git(&["commit", "-qm", "docs: fixture"]);
        self.git(&["rev-parse", "HEAD"])
    }

    /// Point `origin/<branch>` at the local branch, fetched at 11:59.
    fn sync(&self, branch: &str) {
        let tip = self.git(&["rev-parse", branch]);
        self.git(&["update-ref", &format!("refs/remotes/origin/{branch}"), &tip]);
        let fetch = self.root.join(".git/FETCH_HEAD");
        std::fs::write(&fetch, "").unwrap();
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_793_534_340);
        std::fs::File::options()
            .write(true)
            .open(&fetch)
            .unwrap()
            .set_modified(at)
            .unwrap();
    }

    fn gh(&self, body: &str) {
        let script = self.bin.join("gh");
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn pulls(&self, pulls: &Value) {
        self.gh(&format!("cat <<'JSON'\n{pulls}\nJSON"));
    }

    /// A forge that lists `pulls` and, for `main`, requires `required`
    /// through one ruleset; any other read fails.
    fn forge(&self, pulls: &Value, required: &[&str]) {
        let checks: Vec<Value> = required
            .iter()
            .map(|name| serde_json::json!({"context": name}))
            .collect();
        let rules = serde_json::json!([{"type": "required_status_checks",
            "parameters": {"required_status_checks": checks}}]);
        self.gh(&format!(
            "case \"$1 $2\" in\n\
             \"api repos/acme/demo/rules/branches/main?per_page=100\") echo '{rules}' ;;\n\
             \"api repos/acme/demo/branches/main\") echo '{{\"name\":\"main\",\"protected\":false}}' ;;\n\
             \"pr list\") cat <<'JSON'\n{pulls}\nJSON\n;;\n\
             *) exit 1 ;;\nesac"
        ));
    }

    fn json(&self, args: &[&str]) -> Value {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let output = Command::new(env!("CARGO_BIN_EXE_codeflow"))
            .current_dir(&self.root)
            .args(["status", "--json"])
            .args(args)
            .env("PATH", path)
            .env("CODEFLOW_STATUS_AS_OF", AS_OF)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

fn record(id: &str, status: &str, target: &str, deps: &str) -> String {
    format!(
        "---\nid: {id}\nepic_id: EPC-001\nintegration_target: {target}\ntitle: work {id}\nstatus: {status}\nwork_type: feat\nspecs: []\ndepends_on: [{deps}]\ncreated: 2026-10-30\n---\n\n## Outcome\n\nThe work succeeds.\n\n## Description\n\nWork.\n\n## Acceptance Criteria\n\n- AC-1 When run, the command shall succeed.\n"
    )
}

fn records_source(value: &Value) -> &Value {
    value["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["source"] == "records")
        .unwrap()
}

/// F1: a task whose home target cannot be read, or whose status the
/// ladder does not know, makes every percent over it unknown and the
/// records source unchecked; it never reads as done or not started.
#[test]
fn lost_coverage_and_unknown_statuses_make_percents_unknown() {
    let repo = Repo::new(
        "missing-target",
        &[
            ("TSK-001", record("TSK-001", "complete", "main", "")),
            (
                "TSK-002",
                record("TSK-002", "todo", "integration/missing", ""),
            ),
        ],
    );
    let value = repo.json(&["--epic", "EPC-001"]);
    assert_eq!(value["scope"]["percent"], "unknown", "{}", value["scope"]);
    assert_eq!(value["epics"][0]["percent"], "unknown");
    assert_eq!(task(&value, "TSK-002")["stage"], "unknown");
    assert_eq!(records_source(&value)["checked"], false);
    assert_eq!(value["issues"]["operator"]["state"], "not established");

    let repo = Repo::new(
        "unknown-status",
        &[("TSK-001", record("TSK-001", "potato", "main", ""))],
    );
    let value = repo.json(&["--task", "TSK-001"]);
    assert_eq!(value["scope"]["percent"], "unknown");
    assert_eq!(value["tasks"][0]["stage"], "unknown");
    assert_eq!(value["tasks"][0]["percent"], "unknown");
}

/// F1: a forge answer at the read limit may hide pull requests, so it is
/// unverified, never the whole set.
#[test]
fn a_forge_read_at_its_limit_is_unverified() {
    let repo = Repo::new(
        "truncated",
        &[("TSK-001", record("TSK-001", "todo", "main", ""))],
    );
    let pulls: Vec<Value> = (1..=200)
        .map(|number| {
            serde_json::json!({"number": number, "title": "x", "headRefName": format!("fix/{number}"),
                "headRefOid": "a".repeat(40), "baseRefName": "main", "isDraft": false,
                "statusCheckRollup": []})
        })
        .collect();
    repo.pulls(&Value::Array(pulls));
    let value = repo.json(&["--epic", "EPC-001"]);
    assert_eq!(value["pulls_into_main"], "unverified");
    let forge = value["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["source"] == "forge")
        .unwrap();
    assert_eq!(forge["checked"], false);
    assert!(
        forge["note"].as_str().unwrap().contains("read limit"),
        "{forge}"
    );
}

/// F2: a block that still lists a criterion the record removed is invalid
/// and never earns `reviewed`; its count is over the current criteria.
#[test]
fn an_acceptance_block_is_judged_against_the_current_criteria() {
    let two = format!(
        "{}- AC-2 When run twice, the command shall succeed twice.\n",
        record("TSK-001", "todo", "main", "")
    );
    let repo = Repo::new("acceptance", &[("TSK-001", two)]);
    repo.git(&["switch", "-qc", "task/TSK-001-work"]);
    write(&repo.root, "src/new.rs", "fn new() {}\n");
    let reviewed = repo.commit();
    let block = format!(
        "\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: target/reviews/review.md\n  criteria:\n    AC-1: verified | test passed\n    AC-2: verified | test passed\n  journey: fixture\n  not_verified: none\n  follow_ups: none\n  verdict: approved\n```\n"
    );
    repo.record(
        "TSK-001",
        &format!("{}{block}", record("TSK-001", "todo", "main", "")),
    );
    repo.commit();
    let value = repo.json(&["--task", "TSK-001"]);
    let fact = &value["tasks"][0];
    assert_ne!(fact["stage"], "reviewed", "{fact}");
    assert_eq!(fact["criteria"]["state"], "invalid", "{fact}");
    assert_eq!(fact["criteria"]["total"], 1);
    assert_eq!(fact["criteria"]["verified"], 1);
    assert_ne!(fact["criteria"]["note"], "none");
}

/// F3: a completion on the line reads `line`, never `main`, until the
/// default branch's remote ref reaches its landing commit.
#[test]
fn a_line_completion_is_on_main_only_when_main_reaches_it() {
    const LINE: &str = "integration/EPC-001-demo";
    let repo = Repo::new(
        "main-stale",
        &[("TSK-001", record("TSK-001", "todo", LINE, ""))],
    );
    repo.git(&["switch", "-qc", LINE]);
    write(&repo.root, "src/changed.rs", "fn changed() {}\n");
    repo.record("TSK-001", &record("TSK-001", "complete", LINE, ""));
    repo.commit();
    repo.sync(LINE);
    let value = repo.json(&["--task", "TSK-001"]);
    let location = &value["tasks"][0]["location"];
    assert_eq!(location["kind"], "line", "{location}");
    assert_eq!(location["state"], "checked", "{location}");
}

/// F4: every window starts at or before each of its ends, and a
/// predecessor outside the scope still delays its successor.
#[test]
fn windows_start_before_they_end_and_outside_predecessors_count() {
    let repo = Repo::new(
        "windows",
        &[
            ("TSK-001", record("TSK-001", "todo", "main", "")),
            ("TSK-002", record("TSK-002", "todo", "main", "TSK-001")),
        ],
    );
    let value = repo.json(&["--epic", "EPC-001"]);
    for fact in value["tasks"].as_array().unwrap() {
        let window = &fact["window"];
        let start = window["start_seconds"].as_i64().unwrap();
        for end in ["earliest", "planning", "latest"] {
            let end = window[format!("{end}_end_seconds")].as_i64().unwrap();
            assert!(start <= end, "{}: {window}", fact["id"]);
        }
    }
    let alone = repo.json(&["--task", "TSK-002"]);
    let window = &alone["tasks"][0]["window"];
    assert!(window["start_seconds"].as_i64().unwrap() > 0, "{window}");
}

/// F5: a skipped run is never a pass; each run is kept with its state,
/// and a skipped required check is an issue.
#[test]
fn a_skipped_check_is_not_a_pass() {
    let repo = Repo::new(
        "checks",
        &[("TSK-001", record("TSK-001", "todo", "main", ""))],
    );
    repo.forge(
        &serde_json::json!([{"number": 1, "title": "Task", "headRefName": "task/TSK-001-work",
        "headRefOid": "a".repeat(40), "baseRefName": "main", "isDraft": true,
        "statusCheckRollup": [
            {"name": "gates", "conclusion": "SUCCESS", "completedAt": "2026-11-01T11:00:00Z"},
            {"name": "windows", "conclusion": "SKIPPED"}]}]),
        &["gates", "windows"],
    );
    let value = repo.json(&["--epic", "EPC-001"]);
    let pull = &value["epics"][0]["landing_chain"]["open"][0];
    assert_eq!(pull["checks"], "skipped", "{pull}");
    assert_eq!(pull["required"], "skipped", "{pull}");
    assert_eq!(pull["runs"][0]["state"], "success");
    assert_eq!(pull["runs"][1]["name"], "windows");
    assert_eq!(pull["runs"][1]["state"], "skipped");
    assert!(
        value["issues"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["kind"] == "required check skipped without reuse evidence"),
        "{}",
        value["issues"]
    );
}

/// F6: the status is read with the record parser, so a trailing comment
/// does not hide a landing, and the totals are the week's events.
#[test]
fn pace_reads_statuses_with_the_record_parser() {
    let repo = Repo::new(
        "pace",
        &[("TSK-001", record("TSK-001", "todo", "main", ""))],
    );
    repo.record(
        "TSK-001",
        &record(
            "TSK-001",
            "complete # todo | blocked | complete | cancelled",
            "main",
            "",
        ),
    );
    repo.commit();
    repo.sync("main");
    let value = repo.json(&["--epic", "EPC-001"]);
    assert_eq!(value["tasks"][0]["stage"], "landed");
    assert_eq!(value["pace"]["landed_total"], 1, "{}", value["pace"]);
    // Created on 2026-10-30, inside the week.
    assert_eq!(value["pace"]["added_total"], 1, "{}", value["pace"]);
    let rate = value["pace"]["observed_landing_rate_per_day"]
        .as_f64()
        .unwrap();
    assert!((rate - 1.0 / 7.0).abs() < 1e-9, "{rate}");
}

/// F7: `--since` keeps a session lane and a nested fact the current
/// snapshot did not observe, never dropping them.
#[test]
fn since_keeps_session_lanes_and_nested_facts() {
    let repo = Repo::new(
        "since",
        &[("TSK-001", record("TSK-001", "todo", "main", ""))],
    );
    let session = repo.base.join("session.json");
    std::fs::write(
        &session,
        r#"{"schema_version":1,"facts":[{"observed_at":"2026-11-01T11:00:00Z","lane":"L-1","task":"TSK-001","builder":"fixture","blocker":"vendor unavailable"}]}"#,
    )
    .unwrap();
    let before = repo.json(&["--epic", "EPC-001", "--session", session.to_str().unwrap()]);
    let previous = repo.base.join("before.json");
    std::fs::write(&previous, before.to_string()).unwrap();
    let after = repo.json(&["--epic", "EPC-001", "--since", previous.to_str().unwrap()]);
    let kept = after["since"]["not_observed"].as_array().unwrap();
    assert!(
        kept.iter()
            .any(|item| item["family"] == "session lane" && item["subject"] == "L-1 TSK-001"),
        "{}",
        after["since"]
    );

    let mut before = repo.json(&["--task", "TSK-001"]);
    before["tasks"][0]["context"]["last_moved"] = "2026-11-01T09:00:00Z".into();
    std::fs::write(&previous, before.to_string()).unwrap();
    let after = repo.json(&["--task", "TSK-001", "--since", previous.to_str().unwrap()]);
    assert!(
        after["since"]["not_observed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["field"] == "context.last_moved"
                && item["earlier"] == "2026-11-01T09:00:00Z"),
        "{}",
        after["since"]
    );
}

/// N1: a cited command quotes the repository path, so it runs from
/// another folder when the path holds a space.
#[test]
fn a_cited_command_quotes_a_path_with_spaces() {
    let blocked = format!(
        "{}\n## Blocker\n\n- reason: waits on vendor\n- owner: operator\n- revisit: when vendor replies\n",
        record("TSK-001", "blocked", "main", "")
    );
    let repo = Repo::new("blocked path with spaces", &[("TSK-001", blocked)]);
    let value = repo.json(&["--task", "TSK-001"]);
    let command = value["issues"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["kind"] == "blocked")
        .and_then(|issue| issue["command"].as_str())
        .unwrap()
        .to_string();
    let (cd, rest) = command.split_once(" && ").unwrap();
    assert!(
        rest.starts_with("codeflow task status TSK-001"),
        "{command}"
    );
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("{cd} && pwd -P"))
        .current_dir(&repo.base)
        .output()
        .unwrap();
    assert!(output.status.success(), "{command}: {}", text(&output));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        repo.root.to_str().unwrap()
    );
}

/// The fail-closed classes of review round 02, each through the real CLI.
fn block(reviewed: &str) -> String {
    format!(
        "\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: target/reviews/review.md\n  criteria:\n    AC-1: verified | test passed\n  journey: fixture\n  not_verified: none\n  follow_ups: \"none: no follow-ups\"\n  verdict: approved\n```\n"
    )
}

fn evidence<'a>(value: &'a Value, measure: &str, source: &str) -> &'a Value {
    value["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["measure"] == measure && entry["source"] == source)
        .unwrap_or_else(|| panic!("no {measure} {source} evidence: {}", value["evidence"]))
}

/// R2 F1: records the readiness core judges invalid (a missing
/// predecessor) give an unknown stage and percent, unchecked records and
/// an incomplete issues view, never a numeric zero.
#[test]
fn invalid_readiness_is_unknown_never_zero() {
    let repo = Repo::new(
        "invalid",
        &[("TSK-001", record("TSK-001", "todo", "main", "TSK-999"))],
    );
    let value = repo.json(&["--task", "TSK-001"]);
    let fact = &value["tasks"][0];
    assert_eq!(fact["state"], "invalid");
    assert_eq!(fact["stage"], "unknown", "{fact}");
    assert_eq!(fact["percent"], "unknown");
    assert_eq!(value["scope"]["percent"], "unknown");
    assert_eq!(evidence(&value, "percents", "records")["checked"], false);
    assert_eq!(value["issues"]["complete"], false);
}

/// R2 F1: an unread forge cannot rule out an open pull request, so an open
/// task's stage and its open pull request read unknown.
#[test]
fn an_unread_forge_leaves_an_open_task_unknown() {
    let repo = Repo::new(
        "forge-lost",
        &[("TSK-001", record("TSK-001", "todo", "main", ""))],
    );
    repo.gh("exit 1");
    let value = repo.json(&["--task", "TSK-001"]);
    let fact = &value["tasks"][0];
    assert_eq!(fact["stage"], "unknown", "{fact}");
    assert_eq!(fact["percent"], "unknown");
    assert_eq!(fact["context"]["open_pull_request"], "unknown");
}

/// R2 F2: a reviewed commit with the same tree but outside the head's
/// history never binds, and neither does a review whose record body
/// changed afterwards.
#[test]
fn a_review_binds_only_through_ancestry_and_an_unchanged_record() {
    let repo = Repo::new(
        "unrelated",
        &[("TSK-001", record("TSK-001", "todo", "main", ""))],
    );
    repo.git(&["switch", "-qc", "task/TSK-001-work"]);
    write(&repo.root, "src/work.rs", "fn work() {}\n");
    let head = repo.commit();
    let tree = repo.git(&["rev-parse", "HEAD^{tree}"]);
    let unrelated = repo.git(&["commit-tree", &tree, "-m", "same tree, no ancestry"]);
    repo.record(
        "TSK-001",
        &format!(
            "{}{}",
            record("TSK-001", "todo", "main", ""),
            block(&unrelated)
        ),
    );
    repo.commit();
    let fact = &repo.json(&["--task", "TSK-001"])["tasks"][0];
    assert_ne!(fact["stage"], "reviewed", "{fact}");
    assert!(
        fact["criteria"]["note"]
            .as_str()
            .unwrap()
            .contains("ancestor"),
        "{fact}"
    );

    // The real head binds.
    repo.record(
        "TSK-001",
        &format!("{}{}", record("TSK-001", "todo", "main", ""), block(&head)),
    );
    repo.commit();
    let fact = &repo.json(&["--task", "TSK-001"])["tasks"][0];
    assert_eq!(fact["stage"], "reviewed", "{fact}");

    // A later change to the record's description unbinds it.
    let changed = record("TSK-001", "todo", "main", "").replace("Work.", "Different work.");
    repo.record("TSK-001", &format!("{changed}{}", block(&head)));
    repo.commit();
    let fact = &repo.json(&["--task", "TSK-001"])["tasks"][0];
    assert_ne!(fact["stage"], "reviewed", "{fact}");
}

/// R2 F2: acceptance binds to the pull request the report shows: the
/// lowest-numbered one into the task's home target.
#[test]
fn acceptance_binds_to_the_pull_request_the_report_shows() {
    let repo = Repo::new(
        "competing",
        &[("TSK-001", record("TSK-001", "todo", "main", ""))],
    );
    repo.git(&["switch", "-qc", "task/TSK-001-work"]);
    write(&repo.root, "src/work.rs", "fn work() {}\n");
    let head = repo.commit();
    repo.record(
        "TSK-001",
        &format!("{}{}", record("TSK-001", "todo", "main", ""), block(&head)),
    );
    let bound = repo.commit();
    let pull = |number: u64, sha: &str, base: &str| {
        serde_json::json!({"number": number, "title": "Task", "headRefName": "task/TSK-001-work",
            "headRefOid": sha, "baseRefName": base, "isDraft": false, "statusCheckRollup": []})
    };
    // #1 targets another base, so #2 into main is shown, and its head is
    // not here: acceptance cannot bind.
    repo.forge(
        &serde_json::json!([
            pull(1, &bound, "elsewhere"),
            pull(2, &"c".repeat(40), "main")
        ]),
        &[],
    );
    let fact = &repo.json(&["--task", "TSK-001"])["tasks"][0];
    assert_eq!(fact["context"]["open_pull_request"], 2, "{fact}");
    assert_ne!(fact["stage"], "reviewed", "{fact}");
}

/// R2 F4: a successor starts at its predecessor's latest end, and every
/// scenario's end runs from that start (AC-5).
#[test]
fn a_successor_starts_at_the_predecessors_latest_end() {
    let repo = Repo::new(
        "start-rule",
        &[
            ("TSK-001", record("TSK-001", "todo", "main", "")),
            ("TSK-002", record("TSK-002", "todo", "main", "TSK-001")),
        ],
    );
    let value = repo.json(&["--epic", "EPC-001"]);
    let window = |id: &str| {
        value["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == id)
            .unwrap()["window"]
            .clone()
    };
    let (first, second) = (window("TSK-001"), window("TSK-002"));
    let latest = first["latest_end_seconds"].as_i64().unwrap();
    assert_eq!(
        second["start_seconds"].as_i64().unwrap(),
        latest,
        "{second}"
    );
    for end in ["earliest", "planning", "latest"] {
        assert!(
            second[format!("{end}_end_seconds")].as_i64().unwrap() > latest,
            "{second}"
        );
    }
}

/// R2 F5: an optional success never hides a required check with no run;
/// an unread required set makes the issues view incomplete; run times
/// are normalized to UTC and carry their observation time.
#[test]
fn required_checks_are_read_and_never_assumed() {
    let repo = Repo::new(
        "required",
        &[("TSK-001", record("TSK-001", "todo", "main", ""))],
    );
    let pulls = serde_json::json!([{"number": 1, "title": "Task", "headRefName": "task/TSK-001-work",
        "headRefOid": "a".repeat(40), "baseRefName": "main", "isDraft": false,
        "statusCheckRollup": [{"name": "optional-lint", "conclusion": "SUCCESS",
            "completedAt": "2026-11-01T07:00:00-04:00"}]}]);
    repo.forge(&pulls, &["build"]);
    let value = repo.json(&["--task", "TSK-001"]);
    let pull = &value["epics"][0]["landing_chain"]["open"][0];
    assert_eq!(pull["checks"], "passed", "{pull}");
    assert_eq!(pull["required"], "missing", "{pull}");
    assert_eq!(pull["runs"][0]["required"], false);
    assert_eq!(pull["runs"][0]["completed_at"], "2026-11-01T11:00:00Z");
    assert_eq!(pull["runs"][0]["observed_at"], "2026-11-01T12:00:00Z");
    assert!(
        value["issues"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["kind"] == "required check missing"),
        "{}",
        value["issues"]
    );
    assert_eq!(value["issues"]["complete"], true);

    // The pull request list alone: the required set is unread.
    repo.pulls(&pulls);
    let value = repo.json(&["--task", "TSK-001"]);
    let pull = &value["epics"][0]["landing_chain"]["open"][0];
    assert_eq!(pull["required"], "unverified", "{pull}");
    assert_eq!(pull["runs"][0]["required"], "unknown");
    assert_eq!(value["issues"]["complete"], false);
    assert_eq!(
        evidence(&value, "issues", "required checks")["checked"],
        false
    );
}

/// R2 F6: a historical status that cannot be read is no transition: no
/// landing is counted, the pace is not established and the location is
/// unverified.
#[test]
fn an_unreadable_historical_status_invents_no_landing() {
    let repo = Repo::new(
        "bad-prior",
        &[("TSK-001", record("TSK-001", "todo", "main", ""))],
    );
    std::fs::write(
        repo.root.join("project-management/tasks/TSK-001.md"),
        b"\xff unreadable former status\n",
    )
    .unwrap();
    repo.commit();
    repo.record("TSK-001", &record("TSK-001", "complete", "main", ""));
    repo.commit();
    repo.sync("main");
    let value = repo.json(&["--task", "TSK-001"]);
    assert_eq!(
        value["pace"]["landed_total"], "unknown",
        "{}",
        value["pace"]
    );
    assert_eq!(value["tasks"][0]["location"]["state"], "unverified");
    let pace = evidence(&value, "pace", "git history");
    assert_eq!(pace["checked"], false);
    assert!(pace["note"].as_str().unwrap().contains("TSK-001"), "{pace}");
}

/// R2 F7: a lost observation keeps its own observation time, and a pull
/// request with its check runs is kept when the forge is lost.
#[test]
fn since_keeps_lost_observations_with_their_own_times() {
    let repo = Repo::new(
        "since-times",
        &[("TSK-001", record("TSK-001", "todo", "main", ""))],
    );
    let session = repo.base.join("session.json");
    std::fs::write(
        &session,
        r#"{"schema_version":1,"facts":[{"observed_at":"2026-11-01T11:00:00Z","lane":"L-1","task":"TSK-001","builder":"fixture","blocker":"vendor unavailable"}]}"#,
    )
    .unwrap();
    let pulls = serde_json::json!([{"number": 1, "title": "Task", "headRefName": "task/TSK-001-work",
        "headRefOid": "a".repeat(40), "baseRefName": "main", "isDraft": false,
        "statusCheckRollup": [{"name": "build", "conclusion": "SUCCESS"}]}]);
    repo.forge(&pulls, &["build"]);
    let before = repo.json(&["--epic", "EPC-001", "--session", session.to_str().unwrap()]);
    let previous = repo.base.join("before.json");
    std::fs::write(&previous, before.to_string()).unwrap();
    repo.gh("exit 1");
    let after = repo.json(&["--epic", "EPC-001", "--since", previous.to_str().unwrap()]);
    let kept = after["since"]["not_observed"].as_array().unwrap();
    let lane = kept
        .iter()
        .find(|item| item["family"] == "session lane")
        .unwrap_or_else(|| panic!("{}", after["since"]));
    assert_eq!(lane["earlier_observed_at"], "2026-11-01T11:00:00Z");
    let pull = kept
        .iter()
        .find(|item| item["family"] == "pull request" && item["subject"] == "#1")
        .unwrap_or_else(|| panic!("{}", after["since"]));
    assert_eq!(pull["earlier"]["runs"][0]["name"], "build");
    assert_eq!(pull["earlier_observed_at"], "2026-11-01T12:00:00Z");
}
