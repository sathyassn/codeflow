//! TSK-336 (ADR-0091): the policy and release verdicts are produced only by
//! the verdict jobs of the policy workflow, which GitHub runs from the
//! default branch on `pull_request_target`. A verdict is trusted by the
//! explicit list `codeflow_core::remote::TRUSTED_VERDICTS`, never by its
//! spelling: the commit verdict is named `commit standards`, the name the
//! rulesets require (TSK-1368). No workflow that runs on `pull_request` or
//! `push` emits a trusted verdict's name or a `trusted:` name. These read
//! every workflow the repository ships or runs.
use codeflow_core::remote::{is_trusted_verdict, TRUSTED_VERDICTS};
use serde_yaml::Value;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// A repo-relative path with forward slashes. Windows displays `\\`, and
/// every lookup, comparison and message in this file uses `/`.
fn slashed(path: &str) -> String {
    path.replace('\\', "/")
}

#[test]
fn paths_are_compared_with_forward_slashes() {
    assert_eq!(
        slashed(r"assets\base\ci\codeflow-policy.yml"),
        "assets/base/ci/codeflow-policy.yml"
    );
    assert_eq!(
        slashed(r".codeflow\.baseline\.github\workflows\a.yml"),
        ".codeflow/.baseline/.github/workflows/a.yml"
    );
    assert_eq!(slashed("a/b.yml"), "a/b.yml");
}

/// Every workflow file: this repository's, the shipped templates and the
/// installed baseline.
fn workflows() -> Vec<(String, Value)> {
    let root = repo_root();
    let mut found = Vec::new();
    for dir in [
        ".github/workflows",
        "assets/base/ci",
        ".codeflow/.baseline/.github/workflows",
    ] {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(root.join(dir))
            .unwrap_or_else(|error| panic!("{dir}: {error}"))
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension().is_some_and(|ext| ext == "yml")
                    && !path
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with('.'))
            })
            .collect();
        entries.sort();
        for path in entries {
            let text = std::fs::read_to_string(&path).unwrap();
            let value: Value = serde_yaml::from_str(&text)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            if value.get("jobs").is_some() {
                found.push((
                    slashed(&path.strip_prefix(&root).unwrap().display().to_string()),
                    value,
                ));
            }
        }
    }
    assert!(found.len() >= 6, "{}", found.len());
    found
}

/// The `(job id, check name)` pairs of a workflow.
fn jobs(workflow: &Value) -> Vec<(String, String, Value)> {
    workflow["jobs"]
        .as_mapping()
        .unwrap()
        .iter()
        .map(|(id, job)| {
            (
                id.as_str().unwrap().to_owned(),
                job.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                job.clone(),
            )
        })
        .collect()
}

fn events(workflow: &Value) -> Vec<String> {
    match &workflow["on"] {
        Value::Mapping(map) => map
            .keys()
            .map(|key| key.as_str().unwrap().to_owned())
            .collect(),
        Value::String(event) => vec![event.clone()],
        Value::Sequence(list) => list
            .iter()
            .map(|event| event.as_str().unwrap().to_owned())
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether a job's check name must come only from the trusted workflow: a
/// listed verdict, or anything spelled like one.
fn looks_trusted(name: &str) -> bool {
    is_trusted_verdict(name) || name.starts_with("trusted: ")
}

/// TSK-1368: the list is reviewed here, so a change to it is a change to
/// this test.
#[test]
fn the_trusted_verdicts_are_the_listed_names() {
    assert_eq!(
        TRUSTED_VERDICTS,
        ["commit standards", "trusted: release impact"]
    );
    for (path, workflow) in workflows() {
        for (id, name, _) in jobs(&workflow) {
            if name.starts_with("trusted: ") {
                assert!(
                    is_trusted_verdict(&name),
                    "{path} job {id} ('{name}') is spelled as a trusted verdict but is not on the list"
                );
            }
        }
    }
}

/// AC-2, AC-9 (review round 1, R1-1), TSK-1368: a trusted verdict's name
/// comes only from a policy workflow that `pull_request_target` alone
/// starts, and the job carries no `if`. A job-level `if` skips a job but
/// keeps its check, and GitHub counts a skipped required check as passing,
/// so a push, schedule or dispatch run of a workflow holding a trusted-named
/// job would leave a passing check under that name at the pushed commit.
/// `commit standards` is checked by name, so it keeps every property
/// without a `trusted:` prefix.
#[test]
fn only_pull_request_target_workflows_emit_a_trusted_name() {
    let mut producers = Vec::new();
    for (path, workflow) in workflows() {
        for (id, name, job) in jobs(&workflow) {
            if name == "commit standards" {
                producers.push(path.clone());
            }
            if looks_trusted(&name) {
                assert!(
                    path.ends_with("/codeflow-policy.yml"),
                    "{path} job {id} ('{name}'): only the policy workflow produces a trusted verdict"
                );
                assert_eq!(
                    events(&workflow),
                    ["pull_request_target"],
                    "{path} job {id} ('{name}'): a workflow any other event starts creates this check, skipped or not"
                );
                assert!(
                    job.get("if").is_none(),
                    "{path} job {id} ('{name}'): a skipped job still reports its check"
                );
            }
        }
    }
    assert_eq!(
        producers,
        [
            ".github/workflows/codeflow-policy.yml",
            "assets/base/ci/codeflow-policy.yml",
            ".codeflow/.baseline/.github/workflows/codeflow-policy.yml",
        ],
        "commit standards has one producer in each workflow set"
    );
}

/// AC-2: the registry check of pushes, the schedule and manual runs lives in
/// its own workflow, under its own name, and no workflow that runs on
/// `pull_request_target` carries it; the pull request's registry check is a
/// step of the verdict job.
#[test]
fn the_registry_runs_in_its_own_workflow() {
    let all = workflows();
    for path in [
        "assets/base/ci/codeflow-registry.yml",
        ".codeflow/.baseline/.github/workflows/codeflow-registry.yml",
        ".github/workflows/codeflow-registry.yml",
    ] {
        let (_, workflow) = all
            .iter()
            .find(|(found, _)| found == path)
            .unwrap_or_else(|| panic!("{path}"));
        assert_eq!(
            events(workflow),
            ["push", "schedule", "workflow_dispatch"],
            "{path}"
        );
        let names: Vec<String> = jobs(workflow).into_iter().map(|(_, n, _)| n).collect();
        // This repository's copy adds the main-only cache writer (TSK-1240):
        // a job that is no verdict and carries no `trusted:` name. The
        // shipped template and its baseline keep the one registry job.
        let expected: &[&str] = if path == ".github/workflows/codeflow-registry.yml" {
            &["registry check", "cache main build"]
        } else {
            &["registry check"]
        };
        assert_eq!(names, expected, "{path}");
    }
    for (path, workflow) in &all {
        if jobs(workflow)
            .iter()
            .any(|(_, name, _)| name == "registry check")
        {
            assert!(
                !events(workflow).contains(&"pull_request_target".to_owned()),
                "{path}"
            );
        }
    }
}

/// AC-9, amended 2026-11-03: the head-built `release impact` job stays in
/// codeflow-release.yml until the last change before main, so line pull
/// requests keep the verdict ruleset 25435656 requires; it is that name's
/// only producer, and `trusted: release impact` runs beside it.
#[test]
fn the_old_release_impact_name_has_one_producer_until_main() {
    let producers: Vec<String> = workflows()
        .into_iter()
        .flat_map(|(path, workflow)| {
            jobs(&workflow)
                .into_iter()
                .filter(|(_, name, _)| name == "release impact")
                .map(move |(id, _, _)| format!("{path}:{id}"))
        })
        .collect();
    assert_eq!(
        producers,
        [".github/workflows/codeflow-release.yml:release-impact"]
    );
}

/// The verdict jobs each workflow copy declares.
#[test]
fn the_policy_workflows_declare_their_trusted_names() {
    let declared = |path: &str| -> Vec<String> {
        workflows()
            .into_iter()
            .find(|(found, _)| found == path)
            .map_or_else(
                || panic!("{path}"),
                |(_, workflow)| {
                    jobs(&workflow)
                        .into_iter()
                        .map(|(_, name, _)| name)
                        .filter(|name| is_trusted_verdict(name))
                        .collect()
                },
            )
    };
    assert_eq!(
        declared("assets/base/ci/codeflow-policy.yml"),
        ["commit standards"]
    );
    assert_eq!(
        declared(".codeflow/.baseline/.github/workflows/codeflow-policy.yml"),
        ["commit standards"]
    );
    assert_eq!(
        declared(".github/workflows/codeflow-policy.yml"),
        ["commit standards", "trusted: release impact"]
    );
}

/// AC-1: every verdict job resolves the default branch, checks out exactly
/// the run's commit, asserts that checkout after every later step, and
/// runs nothing from the pull request: the base and head are fetched as
/// refs, and the release verdict runs the authority's scripts/release.py
/// with the authority's build.
#[test]
fn every_verdict_job_runs_only_the_default_branch_code() {
    for (path, workflow) in workflows() {
        for (id, name, job) in jobs(&workflow) {
            if !looks_trusted(&name) {
                continue;
            }
            let steps = job["steps"].as_sequence().unwrap();
            let label = |step: &Value| {
                step.get("name")
                    .or_else(|| step.get("uses"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            assert_eq!(label(&steps[0]), "Resolve the authority", "{path} {id}");
            assert!(
                label(&steps[1]).starts_with("actions/checkout@"),
                "{path} {id}"
            );
            assert_eq!(steps[1]["with"]["ref"].as_str(), Some("${{ github.sha }}"));
            let assert_text = serde_yaml::to_string(&steps[2]).unwrap();
            for (index, step) in steps.iter().enumerate().skip(1) {
                let is_assert = label(step) == "Assert the authority checkout";
                assert_eq!(is_assert, index % 2 == 0, "{path} {id} step {index}");
                if is_assert {
                    assert_eq!(serde_yaml::to_string(step).unwrap(), assert_text);
                }
                let run = step.get("run").and_then(Value::as_str).unwrap_or_default();
                for forbidden in ["git checkout", "git switch", "git worktree", "git reset"] {
                    assert!(!run.contains(forbidden), "{path} {id}: {forbidden}");
                }
                assert!(!run.contains("secrets."), "{path} {id}");
            }
            assert_eq!(steps.len() % 2, 1, "{path} {id} ends with an assertion");
            let text = serde_yaml::to_string(&job).unwrap();
            assert!(!text.contains("environment"), "{path} {id}");
            assert!(!text.contains(": write"), "{path} {id}");
            if name == "trusted: release impact" {
                assert!(text.contains("python3 -I scripts/release.py check-pr"));
                assert!(text.contains("--codeflow-bin target/release/codeflow"));
                // TSK-1040: the runner logs a step's `env:` values, so the
                // body reaches the script as a file, as in commit standards.
                assert!(text.contains("BODY_FILE: ${{ runner.temp }}/pr-body.md"));
                assert!(text.contains("--body-file \"$BODY_FILE\""));
                assert!(!text.contains("CODEFLOW_PR_BODY"));
                assert!(!text.contains("outputs.text"));
                assert!(!text.contains("github.event.pull_request.body"));
                // A queued run cannot pass on a body that changed under it.
                assert!(text.contains("the pull request is unchanged since it was judged"));
                assert!(text.contains("JUDGED_DIGEST: ${{ steps.body.outputs.digest }}"));
            }
        }
    }
}

/// Each shipped workflow (`assets/base/ci`), parsed.
fn shipped_workflows() -> Vec<(String, Value)> {
    workflows()
        .into_iter()
        .filter(|(path, _)| path.starts_with("assets/base/ci/"))
        .collect()
}

/// TSK-1252 AC-11 (was TSK-1175 AC-1): every shipped checkout disables
/// credential persistence, and the only steps that receive the run's
/// token, each in its own environment, are the fetch steps, the pull
/// request body reads, the `codeflow ci` step and TSK-1177's host-rules
/// read and producer refusal.
#[test]
fn every_shipped_checkout_keeps_no_credential() {
    let shipped = shipped_workflows();
    assert!(shipped.len() >= 3, "{}", shipped.len());
    let mut checkouts = 0;
    for (path, workflow) in shipped {
        for (id, _, job) in jobs(&workflow) {
            for step in job["steps"].as_sequence().unwrap() {
                let uses = step.get("uses").and_then(Value::as_str).unwrap_or_default();
                if uses.starts_with("actions/checkout@") {
                    checkouts += 1;
                    assert_eq!(
                        step["with"]["persist-credentials"].as_bool(),
                        Some(false),
                        "{path} {id}: a checkout persists its credential"
                    );
                }
                let text = serde_yaml::to_string(step).unwrap();
                if text.contains("github.token") {
                    let name = step.get("name").and_then(Value::as_str).unwrap_or_default();
                    assert!(
                        [
                            "Read the pull request's current body",
                            "the pull request is unchanged since it was judged",
                            "Fetch the pull request's base and head as data",
                            "Fetch the shared registry",
                            "codeflow ci (commit + branch + PR-body standards)",
                            // TSK-1177: read with the read-only token for
                            // the report's authority line and the Mode B
                            // refusal, in the verdict and authority jobs.
                            "Read the host rules for the authority line",
                            "Refuse another producer of a trusted name",
                        ]
                        .contains(&name),
                        "{path} {id}: step {name:?} receives the run's token"
                    );
                }
            }
        }
    }
    assert!(checkouts >= 6, "{checkouts}");
}

/// TSK-1252 AC-1, AC-7: the policy job's `codeflow ci` step reads the
/// run's token as `CODEFLOW_CI_TOKEN` and no long-lived credential (no
/// secret, personal token or App key), and the job's token is read-only
/// with `contents: read`.
#[test]
fn the_policy_ci_step_gets_only_the_run_token() {
    let (path, workflow) = shipped_workflows()
        .into_iter()
        .find(|(path, _)| path == "assets/base/ci/codeflow-policy.yml")
        .unwrap();
    let (_, _, job) = jobs(&workflow)
        .into_iter()
        .find(|(_, name, _)| name == "commit standards")
        .unwrap();
    let permissions = job["permissions"].as_mapping().unwrap();
    assert_eq!(
        job["permissions"]["contents"].as_str(),
        Some("read"),
        "{path}"
    );
    for (scope, level) in permissions {
        assert_eq!(
            level.as_str(),
            Some("read"),
            "{path}: {scope:?} is not read-only"
        );
    }
    let step = job["steps"]
        .as_sequence()
        .unwrap()
        .iter()
        .find(|step| {
            step.get("name").and_then(Value::as_str)
                == Some("codeflow ci (commit + branch + PR-body standards)")
        })
        .unwrap();
    assert_eq!(
        step["env"]["CODEFLOW_CI_TOKEN"].as_str(),
        Some("${{ github.token }}"),
        "{path}"
    );
    let env = serde_yaml::to_string(&step["env"]).unwrap();
    for forbidden in ["secrets.", "GH_TOKEN", "GITHUB_TOKEN", "PAT", "APP_"] {
        assert!(!env.contains(forbidden), "{path}: {forbidden} in {env}");
    }
    assert_eq!(env.matches("github.token").count(), 1, "{path}: {env}");
}
