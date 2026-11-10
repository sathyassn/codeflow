//! TSK-1396 AC-2 (ADR-0081 as amended): the shipped CI and policy
//! workflows run for a merge queue entry (`merge_group`) and run every job
//! that reports a check, never skipped; the policy job judges from the
//! default branch's commit, never the queue commit; the installed release
//! and the trusted inputs come from the queue's base; and a queue entry's
//! concurrency never joins a pull request's.
//!
//! The shipped templates and the installed baseline carry this now. This
//! repository's own copies under `.github/workflows/` change through an
//! operator patch, so they are judged by the same properties once they
//! trigger on `merge_group`, and are otherwise named as not yet wired.
//! `CODEFLOW_OWN_WORKFLOWS_ROOT` names another tree whose own copies are
//! judged instead, so the operator patch can be tested before it is
//! committed.
use serde_yaml::Value;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// The tree a path is read from: this repository, or for this
/// repository's own copies the tree `CODEFLOW_OWN_WORKFLOWS_ROOT` names.
fn root_for(path: &str) -> PathBuf {
    match std::env::var_os("CODEFLOW_OWN_WORKFLOWS_ROOT") {
        Some(root) if path == OWN_POLICY || path == OWN_CI => PathBuf::from(root),
        _ => repo_root(),
    }
}

fn read(path: &str) -> (String, Value) {
    let text = std::fs::read_to_string(root_for(path).join(path))
        .unwrap_or_else(|error| panic!("{path}: {error}"));
    let value = serde_yaml::from_str(&text).unwrap_or_else(|error| panic!("{path}: {error}"));
    (text, value)
}

const POLICY: [&str; 2] = [
    "assets/base/ci/codeflow-policy.yml",
    ".codeflow/.baseline/.github/workflows/codeflow-policy.yml",
];
const CI: [&str; 2] = [
    "assets/base/ci/codeflow-ci.yml",
    ".codeflow/.baseline/.github/workflows/codeflow-ci.yml",
];
const OWN_POLICY: &str = ".github/workflows/codeflow-policy.yml";
const OWN_CI: &str = ".github/workflows/codeflow-ci.yml";

/// The policy and CI copies to judge: the shipped ones always, and this
/// repository's own once they trigger on `merge_group`.
fn copies(shipped: [&'static str; 2], own: &'static str) -> Vec<&'static str> {
    let mut paths = shipped.to_vec();
    let (_, workflow) = read(own);
    if workflow["on"].get("merge_group").is_some() {
        paths.push(own);
    }
    paths
}

/// The `merge_group` trigger asks for the checks of a queue entry.
fn assert_queue_trigger(path: &str, workflow: &Value) {
    let trigger = workflow["on"]
        .get("merge_group")
        .unwrap_or_else(|| panic!("{path}: no merge_group trigger"));
    let types: Vec<&str> = trigger["types"]
        .as_sequence()
        .unwrap_or_else(|| panic!("{path}: merge_group names no types"))
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    assert_eq!(types, ["checks_requested"], "{path}");
}

/// Whether a job runs for a queue entry: no `if`, an `if` that names
/// `merge_group` as an event it admits, or one that names no event apart
/// from leaving out a draft pull request.
fn runs_for_queue(job: &Value) -> bool {
    match job.get("if").and_then(Value::as_str) {
        None => true,
        Some(condition) => {
            condition.contains("github.event_name == 'merge_group'")
                || !condition
                    .replace(
                        "!(github.event_name == 'pull_request' && github.event.pull_request.draft)",
                        "",
                    )
                    .contains("github.event_name")
        }
    }
}

#[test]
fn the_shipped_workflows_trigger_on_merge_group() {
    for path in POLICY.iter().chain(CI.iter()) {
        let (_, workflow) = read(path);
        assert_queue_trigger(path, &workflow);
    }
}

/// The steps that only a pull request event runs, because they read its
/// body.
const PULL_REQUEST_BODY_STEPS: [&str; 2] = [
    "Read the pull request's current body",
    "the pull request is unchanged since it was judged",
];

/// The helper jobs of this repository's CI copy that a pull request or a
/// push alone needs; none reports a required check, and each verdict job
/// accepts it skipped.
const OWN_HELPER_JOBS: [&str; 6] = [
    "ubuntu-image",
    "windows-image",
    "reuse",
    "provenance",
    "windows-tier",
    "windows-tier-journeys",
];

/// The steps of this repository's policy copy that only a pull request
/// runs: the merge admission reads the pull request's own codeflow-ci run,
/// and a queue entry's codeflow-ci run reports its verdicts directly.
const OWN_PULL_REQUEST_STEPS: [&str; 1] = ["the exact head's ready codeflow-ci run passed"];

/// Every job that reports a check runs for a queue entry: GitHub counts a
/// skipped required check as passing, and a missing one blocks the queue.
#[test]
fn every_job_runs_for_a_queue_entry() {
    for path in copies(POLICY, OWN_POLICY)
        .into_iter()
        .chain(copies(CI, OWN_CI))
    {
        let (_, workflow) = read(path);
        for (id, job) in workflow["jobs"].as_mapping().unwrap() {
            let id = id.as_str().unwrap();
            if path == OWN_CI && OWN_HELPER_JOBS.contains(&id) {
                continue;
            }
            assert!(
                runs_for_queue(job),
                "{path} job {id} is skipped for a merge queue entry: {:?}",
                job.get("if")
            );
            for step in job["steps"].as_sequence().into_iter().flatten() {
                let name = step.get("name").and_then(Value::as_str).unwrap_or_default();
                // A queue entry has no pull request body: the pull
                // request's own run judged it, and these two steps read
                // and re-check it there.
                if PULL_REQUEST_BODY_STEPS.contains(&name)
                    || (path == OWN_POLICY && OWN_PULL_REQUEST_STEPS.contains(&name))
                {
                    continue;
                }
                if let Some(condition) = step.get("if").and_then(Value::as_str) {
                    assert!(
                        !condition.contains("== 'pull_request_target'")
                            && !condition.contains("== 'pull_request'"),
                        "{path} job {id}: a step that only a pull request runs: {condition}"
                    );
                }
            }
        }
    }
}

/// The policy job judges from the default branch: on `merge_group` the
/// authority is the queue's base, never `github.sha` (the queue commit),
/// the base must be on the default branch, the queue commit is fetched as
/// data under a codeflow ref and never checked out, and an event without a
/// base names no commit, which the first step refuses.
#[test]
fn the_policy_job_judges_from_the_default_branch() {
    for path in copies(POLICY, OWN_POLICY) {
        let (text, workflow) = read(path);
        let jobs = workflow["jobs"].as_mapping().unwrap();
        let judging: Vec<&Value> = jobs
            .values()
            .filter(|job| {
                job.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.starts_with("trusted: "))
            })
            .collect();
        assert!(!judging.is_empty(), "{path}");
        for job in judging {
            let authority = job["env"]["AUTHORITY_SHA"].as_str().unwrap_or_default();
            assert_eq!(
                authority,
                "${{ github.event_name == 'merge_group' && github.event.merge_group.base_sha || github.event_name != 'merge_group' && github.sha || '' }}",
                "{path}"
            );
            let steps = job["steps"].as_sequence().unwrap();
            assert_eq!(
                steps[1]["with"]["ref"].as_str(),
                Some("${{ env.AUTHORITY_SHA }}"),
                "{path}"
            );
            let job_text = serde_yaml::to_string(job).unwrap();
            assert!(
                job_text.contains("QUEUE_BASE_REF"),
                "{path}: the queue's base ref is not checked"
            );
            assert!(
                job_text.contains("refs/heads/$DEFAULT_BRANCH")
                    || job_text.contains("refs/heads/${DEFAULT_BRANCH}"),
                "{path}: the queue's base ref is not compared with the default branch"
            );
            assert!(
                job_text.contains("git merge-base --is-ancestor \"$AUTHORITY_SHA\""),
                "{path}: the queue's base is not proven to be on the default branch"
            );
            assert!(
                job_text.contains(":refs/codeflow/pr-head"),
                "{path}: the queue commit is not fetched as data"
            );
            for forbidden in ["git checkout", "git switch", "git worktree", "git reset"] {
                assert!(!job_text.contains(forbidden), "{path}: {forbidden}");
            }
        }
        // The first step refuses a commit that is not a full id, which an
        // empty authority is.
        assert!(text.contains("grep -Eqx '[0-9a-f]{40}'"), "{path}");
    }
}

/// The installed release and every trusted input of the CI workflow come
/// from the queue's base, never from the queue commit, and an event
/// without a base names no commit ('unset'), which each step refuses.
#[test]
fn the_ci_trusted_inputs_come_from_the_queue_base() {
    let trusted = "${{ github.event_name == 'merge_group' && (github.event.merge_group.base_sha || 'unset') || github.event.pull_request.base.sha || github.sha }}";
    for path in copies(CI, OWN_CI) {
        let (_, workflow) = read(path);
        let mut pins = 0;
        let mut trusted_inputs = 0;
        for (id, job) in workflow["jobs"].as_mapping().unwrap() {
            let id = id.as_str().unwrap();
            for step in job["steps"].as_sequence().into_iter().flatten() {
                let env = &step["env"];
                let name = step.get("name").and_then(Value::as_str).unwrap_or_default();
                if name.starts_with("Install codeflow") {
                    pins += 1;
                    assert_eq!(env["PIN_REF"].as_str(), Some(trusted), "{path} {id}");
                }
                if let Some(value) = env.get("TRUSTED_SHA").and_then(Value::as_str) {
                    trusted_inputs += 1;
                    assert_eq!(value, trusted, "{path} {id} {name}");
                }
                for key in ["PIN_REF", "TRUSTED_SHA", "SCAN_FROM"] {
                    if let Some(value) = env.get(key).and_then(Value::as_str) {
                        if name.starts_with("Install candidate") {
                            continue;
                        }
                        assert!(
                            value.contains("merge_group.base_sha") || !value.contains("github.sha"),
                            "{path} {id} {name}: {key} falls back to the queue commit: {value}"
                        );
                    }
                }
            }
        }
        // This repository builds codeflow from source and installs none.
        if path != OWN_CI {
            assert!(pins >= 1, "{path}: no base-pinned install");
        }
        assert!(trusted_inputs >= 2, "{path}: {trusted_inputs}");
    }
}

/// A queue entry never shares a concurrency group with a pull request or
/// another entry: either the workflow has no group, or its group names
/// the entry's own queue commit.
#[test]
fn each_queue_entry_has_its_own_concurrency() {
    for path in copies(POLICY, OWN_POLICY)
        .into_iter()
        .chain(copies(CI, OWN_CI))
    {
        let (_, workflow) = read(path);
        let mut groups = Vec::new();
        if let Some(group) = workflow
            .get("concurrency")
            .map(|c| c.get("group").unwrap_or(c))
        {
            groups.push(group.as_str().unwrap().to_owned());
        }
        for (_, job) in workflow["jobs"].as_mapping().unwrap() {
            if let Some(group) = job.get("concurrency").map(|c| c.get("group").unwrap_or(c)) {
                groups.push(group.as_str().unwrap().to_owned());
            }
        }
        for group in groups {
            assert!(
                group.contains("github.event.merge_group.head_sha")
                    || group.contains("github.ref }}")
                    || group.contains("github.ref)")
                    || group.contains("github.ref,"),
                "{path}: concurrency group '{group}' can join two queue entries"
            );
        }
    }
    // The policy template's group keys queue entries apart from pull
    // requests.
    let (_, workflow) = read(POLICY[0]);
    let group = workflow["concurrency"]["group"].as_str().unwrap();
    assert!(
        group.contains("'{0}-queue-{1}', github.workflow, github.event.merge_group.head_sha"),
        "{group}"
    );
    assert!(group.contains("'{0}-pr-{1}'"), "{group}");
}

/// The gate runs in full for a queue entry: only a pull request event
/// selects the essential mode.
#[test]
fn the_gate_runs_full_for_a_queue_entry() {
    for path in copies(CI, OWN_CI) {
        let (text, _) = read(path);
        assert!(
            !text.contains("= merge_group ] && python3"),
            "{path}: a queue entry selects a lighter mode"
        );
    }
    let (text, _) = read(CI[0]);
    assert!(
        text.contains("if [ \"${EVENT:-}\" = pull_request ] && python3"),
        "the essential-mode switch moved; check that a queue entry still runs the full gate"
    );
}

/// This repository's copies are wired by an operator patch; until it is
/// applied they keep the earlier triggers, and this says so.
#[test]
fn this_repository_copies_are_named_when_not_wired() {
    for own in [OWN_POLICY, OWN_CI] {
        let (_, workflow) = read(own);
        if workflow["on"].get("merge_group").is_none() {
            eprintln!(
                "note: {own} does not trigger on merge_group yet (operator patch for TSK-1396)"
            );
        } else {
            assert_queue_trigger(own, &workflow);
        }
    }
}
