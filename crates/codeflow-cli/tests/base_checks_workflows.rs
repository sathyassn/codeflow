//! TSK-1103 in the shipped workflows: the trusted-base project checks run
//! in the `pull_request_target` verdict job and nowhere in the head-run
//! workflow (AC-9), every job resolves the base branch's tip once instead of
//! the event's base commit (AC-15), the gates job reads its test config and
//! policy from that tip (AC-16), and the GitLab, Bitbucket and generic
//! pinned run carries the same step before any of the change's code (AC-4,
//! issue 510).
//!
//! The steps run from the shipped YAML's own bytes against local fixture
//! repositories. The last test drives the whole verdict job step by step
//! against a local `origin`, with the test binary standing in for the
//! pinned install and a `curl` stand-in serving the pull request; GitHub's
//! choice of the default branch's workflow file for `pull_request_target`
//! is the boundary it cannot reach, so it reads the workflow from the
//! default branch's checkout as GitHub does.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const CI: &str = include_str!("../../../assets/base/ci/codeflow-ci.yml");
const POLICY: &str = include_str!("../../../assets/base/ci/codeflow-policy.yml");
const REGISTRY: &str = include_str!("../../../assets/base/ci/codeflow-registry.yml");
const GITLAB: &str = include_str!("../../../assets/base/ci/.gitlab-ci.yml");
const BITBUCKET: &str = include_str!("../../../assets/base/ci/bitbucket-pipelines.yml");
const GENERIC: &str = include_str!("../../../assets/base/ci/ci-generic.sh");

/// The `run:` script of every step whose name starts with `prefix`.
fn run_blocks(workflow: &str, prefix: &str) -> Vec<String> {
    steps(workflow)
        .into_iter()
        .filter(|step| step.name.starts_with(prefix))
        .filter_map(|step| step.run)
        .collect()
}

#[derive(Debug, Clone)]
struct Step {
    job: String,
    name: String,
    id: Option<String>,
    uses: Option<String>,
    run: Option<String>,
    condition: Option<String>,
    env: BTreeMap<String, String>,
}

/// Every step of every job, in order.
fn steps(workflow: &str) -> Vec<Step> {
    let doc: serde_yaml::Value = serde_yaml::from_str(workflow).unwrap();
    let mut out = Vec::new();
    for (job, body) in doc["jobs"].as_mapping().unwrap() {
        for step in body["steps"].as_sequence().unwrap() {
            let text = |key: &str| step.get(key).and_then(|v| v.as_str()).map(str::to_string);
            let env = step
                .get("env")
                .and_then(|v| v.as_mapping())
                .map(|map| {
                    map.iter()
                        .map(|(k, v)| {
                            (
                                k.as_str().unwrap().to_string(),
                                v.as_str().map_or_else(|| serde_yaml::to_string(v).unwrap().trim().to_string(), str::to_string),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            out.push(Step {
                job: job.as_str().unwrap().to_string(),
                name: text("name").unwrap_or_default(),
                id: text("id"),
                uses: text("uses"),
                run: text("run"),
                condition: text("if"),
                env,
            });
        }
    }
    out
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=Fixture", "-c", "user.email=fixture@example.com", "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null", "-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let _ = std::fs::remove_file(&file);
    std::fs::write(file, text).unwrap();
}

fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "--allow-empty", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

/// Run `script` with bash as a GitHub `run:` step runs it, in `dir`, with
/// `env`; returns the status, the log and the step's outputs.
fn run_step(script: &str, dir: &Path, env: &[(&str, &str)]) -> (bool, String, BTreeMap<String, String>) {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("output");
    let summary = temp.path().join("summary");
    std::fs::write(&output, "").unwrap();
    let out = Command::new("bash")
        .args(["--noprofile", "--norc", "-eo", "pipefail", "-c", script])
        .current_dir(dir)
        .env("GITHUB_OUTPUT", &output)
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .envs(env.iter().copied())
        .output()
        .unwrap();
    let outputs = std::fs::read_to_string(&output)
        .unwrap()
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    (
        out.status.success(),
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)),
        outputs,
    )
}

/// AC-15: no shipped workflow reads the event's base commit any more.
#[test]
fn no_managed_workflow_reads_the_event_base_commit() {
    for (name, text) in [("codeflow-ci.yml", CI), ("codeflow-policy.yml", POLICY), ("codeflow-registry.yml", REGISTRY)] {
        assert!(!text.contains("pull_request.base.sha"), "{name} still reads pull_request.base.sha");
    }
}

/// AC-15: each job of the head-run workflow that judges against the base
/// resolves it with the same step, first after its checkout, and every
/// former site reads that step's output.
#[test]
fn every_job_resolves_the_base_once_with_the_same_step() {
    let all = steps(CI);
    let resolves: Vec<&Step> = all.iter().filter(|s| s.name == "Resolve the base").collect();
    let jobs: Vec<&str> = resolves.iter().map(|s| s.job.as_str()).collect();
    assert_eq!(jobs, ["gates", "candidate", "secret-scan", "security-review"]);
    for step in &resolves {
        assert_eq!(step.run, resolves[0].run, "{} differs", step.job);
        assert_eq!(step.id.as_deref(), Some("base"));
        let in_job: Vec<&Step> = all.iter().filter(|s| s.job == step.job).collect();
        assert!(in_job[0].uses.as_deref().unwrap_or_default().starts_with("actions/checkout@"));
        assert_eq!(in_job[1].name, "Resolve the base", "{}", step.job);
    }
    let site = |job: &str, name: &str, key: &str| {
        all.iter()
            .find(|s| s.job == job && s.name.starts_with(name))
            .unwrap_or_else(|| panic!("{job}: {name}"))
            .env[key]
            .clone()
    };
    assert_eq!(site("gates", "Install codeflow", "PIN_REF"), "${{ steps.base.outputs.tip || github.sha }}");
    assert_eq!(site("candidate", "Compare the target and head pins", "BASE_SHA"), "${{ steps.base.outputs.tip }}");
    assert_eq!(site("secret-scan", "gitleaks", "TRUSTED_SHA"), "${{ steps.base.outputs.tip || github.sha }}");
    assert_eq!(site("secret-scan", "gitleaks", "SCAN_FROM"), "${{ steps.base.outputs.tip || github.event.before || '' }}");
    assert_eq!(site("security-review", "dependency audit", "TRUSTED_SHA"), "${{ steps.base.outputs.tip || github.sha }}");
    let policy = steps(POLICY);
    for name in ["codeflow ci (commit", "codeflow ids check"] {
        let step = policy.iter().find(|s| s.name.starts_with(name)).unwrap();
        assert_eq!(step.env["BASE_SHA"], "${{ steps.range.outputs.base }}", "{name}");
    }
}

/// A local `origin` with `main`, a pull request head branched from it, and
/// `main` moved on after the branch; returns (origin, head, old main, new main).
fn moved_base() -> (tempfile::TempDir, PathBuf, String, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    write(&work, "a.txt", "a\n");
    let old = commit(&work, "chore: seed");
    git(&work, &["switch", "-q", "-c", "feat/x"]);
    write(&work, "b.txt", "b\n");
    let head = commit(&work, "feat: b");
    git(&work, &["switch", "-q", "main"]);
    write(&work, "c.txt", "c\n");
    let new = commit(&work, "chore: the base moves");
    let origin = dir.path().join("origin.git");
    git(dir.path(), &["clone", "-q", "--bare", work.to_str().unwrap(), origin.to_str().unwrap()]);
    git(&work, &["push", "-q", origin.to_str().unwrap(), &format!("{head}:refs/pull/1/head")]);
    // The runner's checkout knows only the event's old base.
    let runner = dir.path().join("runner");
    git(dir.path(), &["clone", "-q", "--no-checkout", origin.to_str().unwrap(), runner.to_str().unwrap()]);
    git(&runner, &["fetch", "-q", "origin", &format!("refs/pull/1/head:refs/pr-head")]);
    git(&runner, &["update-ref", "refs/remotes/origin/main", &old]);
    (dir, runner, head, old, new)
}

/// AC-15: a base that moved after the pull request opened is judged at its
/// current tip, and the range is merge-base(head, tip)..head; a run that is
/// not a pull request leaves both empty; a base branch that cannot be
/// fetched fails.
#[test]
fn the_base_is_the_current_tip_resolved_once() {
    let (_dir, runner, head, old, new) = moved_base();
    let script = run_blocks(CI, "Resolve the base").remove(0);
    let (ok, log, outputs) = run_step(&script, &runner, &[("EVENT", "pull_request"), ("BASE_REF", "main"), ("HEAD_SHA", &head)]);
    assert!(ok, "{log}");
    assert_eq!(outputs["tip"], new, "{log}");
    assert_eq!(outputs["range"], old, "{log}");
    assert!(log.contains(&format!("base main resolved once at {new}; the change is {old}..{head}")), "{log}");

    let (ok, log, outputs) = run_step(&script, &runner, &[("EVENT", "push"), ("BASE_REF", ""), ("HEAD_SHA", "")]);
    assert!(ok, "{log}");
    assert_eq!((outputs["tip"].as_str(), outputs["range"].as_str()), ("", ""));

    let (ok, log, _) = run_step(&script, &runner, &[("EVENT", "pull_request"), ("BASE_REF", "gone"), ("HEAD_SHA", &head)]);
    assert!(!ok);
    assert!(log.contains("cannot fetch the pull request's base branch gone; nothing is judged"), "{log}");
}

/// AC-15 in the verdict job: the fetch step resolves the base branch's tip,
/// passes the merge-base as the range base, and hands on the test merge only
/// when its parents are that tip and the head.
#[test]
fn the_verdict_job_fetches_the_tip_and_checks_the_merge() {
    let (_dir, runner, head, old, new) = moved_base();
    let origin = runner.parent().unwrap().join("origin.git");
    let work = runner.parent().unwrap().join("work");
    let script = run_blocks(POLICY, "Fetch the pull request's base and head as data").remove(0);
    let env = [("PR_NUMBER", "1"), ("BASE_REF", "main"), ("HEAD_SHA", head.as_str())];

    // A test merge built on the old base is not handed on.
    git(&work, &["switch", "-q", "--detach", &old]);
    git(&work, &["merge", "-q", "--no-ff", "-m", "Merge", &head]);
    let stale = git(&work, &["rev-parse", "HEAD"]);
    git(&work, &["push", "-q", origin.to_str().unwrap(), &format!("{stale}:refs/pull/1/merge")]);
    let (ok, log, outputs) = run_step(&script, &runner, &env);
    assert!(ok, "{log}");
    assert_eq!(outputs["tip"], new);
    assert_eq!(outputs["base"], old);
    assert_eq!(outputs["merge"], "", "{log}");

    // One built on the current tip is.
    git(&work, &["switch", "-q", "--detach", &new]);
    git(&work, &["merge", "-q", "--no-ff", "-m", "Merge", &head]);
    let current = git(&work, &["rev-parse", "HEAD"]);
    git(&work, &["push", "-q", "-f", origin.to_str().unwrap(), &format!("{current}:refs/pull/1/merge")]);
    let (ok, log, outputs) = run_step(&script, &runner, &env);
    assert!(ok, "{log}");
    assert_eq!(outputs["merge"], current, "{log}");
    assert_eq!(git(&runner, &["rev-parse", "refs/codeflow/pr-base"]), old);

    // A head that is not the event's fails.
    let (ok, log, _) = run_step(&script, &runner, &[("PR_NUMBER", "1"), ("BASE_REF", "main"), ("HEAD_SHA", &old)]);
    assert!(!ok);
    assert!(log.contains("the pull request head moved"), "{log}");
}

/// AC-16: the gates job reads the test config and the policy from the
/// base, so a head that drops a target, lowers a floor or edits the policy
/// is judged by the base's copies; a file the base lacks is removed; a base
/// copy that is not a regular file or cannot be read fails the job.
#[test]
fn the_gate_reads_its_config_and_policy_from_the_base() {
    let script = run_blocks(CI, "Read the gate's config and policy from the base").remove(0);
    let base_config = r#"{"schema_version": 1, "execution": {"quick_default_class": "heavy"}, "targets": [{"name": "unit", "command": "true", "modes": {"essential": {}}}, {"name": "lint", "command": "true"}]}"#;
    let base_policy = r#"{"git": {"security_review": "block"}}"#;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(root, ".codeflow/test-config.json", base_config);
    write(root, ".codeflow/policy.json", base_policy);
    let base = commit(root, "chore: seed");
    // The head drops a target, lowers the execution floor and the policy.
    write(root, ".codeflow/test-config.json", r#"{"schema_version": 1, "targets": [{"name": "lint", "command": "true"}]}"#);
    write(root, ".codeflow/policy.json", r#"{"git": {"security_review": "off"}}"#);
    commit(root, "feat: weaken the gate");
    let (ok, log, _) = run_step(&script, root, &[("BASE_TIP", &base)]);
    assert!(ok, "{log}");
    assert_eq!(std::fs::read_to_string(root.join(".codeflow/test-config.json")).unwrap(), base_config);
    assert_eq!(std::fs::read_to_string(root.join(".codeflow/policy.json")).unwrap(), base_policy);
    assert!(log.contains(&format!("the gate reads .codeflow/test-config.json from the base {base}")), "{log}");

    // A base without the files: the head's copies are removed.
    git(root, &["rm", "-q", "-f", ".codeflow/test-config.json", ".codeflow/policy.json"]);
    let bare = commit(root, "chore: no config");
    write(root, ".codeflow/test-config.json", "{}");
    write(root, ".codeflow/policy.json", "{}");
    let (ok, log, _) = run_step(&script, root, &[("BASE_TIP", &bare)]);
    assert!(ok, "{log}");
    assert!(!root.join(".codeflow/test-config.json").exists());
    assert!(!root.join(".codeflow/policy.json").exists());
    assert!(log.contains("holds no .codeflow/policy.json; the gate runs without it, as the base would"), "{log}");

    // A base whose policy is a symbolic link fails.
    std::os::unix::fs::symlink("/etc/hosts", root.join(".codeflow/policy.json")).unwrap();
    let linked = commit(root, "chore: a linked policy");
    let (ok, log, _) = run_step(&script, root, &[("BASE_TIP", &linked)]);
    assert!(!ok);
    assert!(log.contains(&format!(".codeflow/policy.json at the base {linked} is not a regular file; nothing is judged")), "{log}");

    // A base copy that cannot be read fails.
    git(root, &["rm", "-q", ".codeflow/policy.json"]);
    write(root, ".codeflow/policy.json", base_policy);
    let unreadable = commit(root, "chore: restore the policy");
    let blob = git(root, &["rev-parse", &format!("{unreadable}:.codeflow/policy.json")]);
    std::fs::remove_file(root.join(format!(".git/objects/{}/{}", &blob[..2], &blob[2..]))).unwrap();
    let (ok, log, _) = run_step(&script, root, &[("BASE_TIP", &unreadable)]);
    assert!(!ok);
    assert!(log.contains("nothing is judged"), "{log}");

    // No base at all fails.
    let (ok, log, _) = run_step(&script, root, &[("BASE_TIP", "")]);
    assert!(!ok);
    assert!(log.contains("no base commit to read the gate's config and policy from"), "{log}");
}

/// AC-16: in the gates job the base read comes after the base is resolved
/// and before anything reads the config or the policy.
#[test]
fn the_gates_job_reads_the_base_before_the_gate() {
    let gates: Vec<String> = steps(CI).into_iter().filter(|s| s.job == "gates").map(|s| s.name).collect();
    let at = |name: &str| gates.iter().position(|n| n.starts_with(name)).unwrap_or_else(|| panic!("{name}: {gates:?}"));
    assert!(at("Resolve the base") < at("Read the gate's config and policy from the base"));
    assert!(at("Read the gate's config and policy from the base") < at("codeflow test"));
    assert!(at("codeflow test") < at("codeflow validate --docs"));
    let read = steps(CI).into_iter().find(|s| s.name.starts_with("Read the gate's config")).unwrap();
    assert_eq!(read.condition.as_deref(), Some("github.event_name == 'pull_request'"));
}

/// AC-9: the step lives in the verdict job, after the head is fetched as
/// data and before the pull request is judged, and the head-run workflow
/// carries no such slot.
#[test]
fn the_checks_run_in_the_verdict_job_only() {
    assert!(!CI.contains("base-checks"), "codeflow-ci.yml must not carry a base-checks slot");
    let policy: Vec<Step> = steps(POLICY);
    assert!(policy.iter().all(|s| s.job == "commit-lint"));
    let at = |name: &str| policy.iter().position(|s| s.name.starts_with(name)).unwrap_or_else(|| panic!("{name}"));
    let checks = &policy[at("Project checks from the default branch")];
    assert_eq!(
        checks.run.as_deref().map(str::trim),
        Some(r#"codeflow ci base-checks --trusted "$AUTHORITY_SHA" --base "$RANGE_BASE" --head "$HEAD_SHA" --merge "$MERGE_SHA" --head-repo "$HEAD_REPO""#)
    );
    assert_eq!(checks.env["AUTHORITY_SHA"], "${{ github.sha }}");
    assert_eq!(checks.env["HEAD_REPO"], "${{ github.event.pull_request.head.repo.full_name }}");
    assert!(at("Fetch the pull request's base and head as data") < at("Project checks from the default branch"));
    assert!(at("Project checks from the default branch") < at("codeflow ci (commit"));
    // Each step is followed by the authority assertion.
    let after = &policy[at("Project checks from the default branch") + 1];
    assert_eq!(after.name, "Assert the authority checkout");
    // No expression is interpolated into the step's script.
    assert!(!checks.run.as_deref().unwrap().contains("${{"));
}

/// AC-4, issue 510: the GitLab, Bitbucket and generic pinned run carries
/// the step, from the target commit, after the judgment and before the
/// project setup hook and the test gate.
#[test]
fn the_pinned_run_checks_before_any_candidate_code() {
    for (name, text) in [("ci-generic.sh", GENERIC), (".gitlab-ci.yml", GITLAB), ("bitbucket-pipelines.yml", BITBUCKET)] {
        let checks = text.find("codeflow ci base-checks --trusted \"$target\"").unwrap_or_else(|| panic!("{name}"));
        let judged = text.find("codeflow ci --base \"$target\"").unwrap();
        let setup = text.find(". ./.codeflow/ci-setup.sh").unwrap();
        let gate = text.find("\"$codeflow_bin\" test --strict").unwrap();
        assert!(judged < checks && checks < setup && setup < gate, "{name}");
    }
}

/// The verdict job of `codeflow-policy.yml`, run step by step from the
/// default branch's copy against a local `origin`.
struct Job {
    _dir: tempfile::TempDir,
    origin: PathBuf,
    work: PathBuf,
    bin: PathBuf,
}

/// The pull request body for task `TSK-00<number>`.
fn body(number: u32) -> String {
    format!("## Summary\nA change.\n\n- one change\n\nTask: TSK-00{number}\n\n## Changes\n- one\n\n## Testing\n- test\n- Not tested: nothing else\n")
}

impl Job {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        git(&work, &["init", "-q", "-b", "main"]);
        write(&work, ".codeflow/policy.json", "{}\n");
        write(&work, ".codeflow/base-checks.json", r#"{"version": 1, "checks": [{"name": "boundary", "script": ".codeflow/checks/boundary.sh", "interpreter": "bash", "inputs": ["docs/boundary.txt"]}]}"#);
        write(
            &work,
            ".codeflow/checks/boundary.sh",
            "set -eu\nif git diff --name-only \"$1\" \"$2\" | grep -qf docs/boundary.txt; then echo \"boundary: refused\"; exit 1; fi\necho boundary: holds\n",
        );
        write(&work, "docs/boundary.txt", "^vendor/\n");
        write(&work, ".github/workflows/codeflow-policy.yml", POLICY);
        write(&work, "src/lib.rs", "// lib\n");
        commit(&work, "chore: seed");
        let origin = dir.path().join("origin.git");
        git(dir.path(), &["clone", "-q", "--bare", work.to_str().unwrap(), origin.to_str().unwrap()]);
        // The pinned install's stand-ins: the binary under test, and a curl
        // that serves the pull request from PULL_JSON.
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_codeflow"), bin.join("codeflow")).unwrap();
        write(
            &bin,
            "curl",
            "#!/bin/sh\nwhile [ $# -gt 0 ]; do if [ \"$1\" = -o ]; then cp \"$PULL_JSON\" \"$2\"; exit 0; fi; shift; done\nexit 1\n",
        );
        Command::new("chmod").args(["755", bin.join("curl").to_str().unwrap()]).status().unwrap();
        Self { _dir: dir, origin, work, bin }
    }

    /// Push a pull request head built by `change` from main as
    /// `refs/pull/<n>/head`; returns its commit.
    fn pull_request(&self, number: u32, branch: &str, change: impl Fn(&Path)) -> String {
        git(&self.work, &["switch", "-q", "-C", branch, "main"]);
        change(&self.work);
        let head = commit(&self.work, "feat: the change");
        git(&self.work, &["push", "-q", self.origin.to_str().unwrap(), &format!("{head}:refs/pull/{number}/head")]);
        head
    }

    /// Run the verdict job for pull request `number` as GitHub runs it from
    /// the default branch; returns the step that failed, if any, and the log.
    fn run(&self, number: u32, branch: &str, head: &str) -> (Option<String>, String) {
        let runner = self._dir.path().join(format!("runner-{number}"));
        let _ = std::fs::remove_dir_all(&runner);
        git(self._dir.path(), &["clone", "-q", self.origin.to_str().unwrap(), runner.to_str().unwrap()]);
        let authority = git(&runner, &["rev-parse", "HEAD"]);
        // GitHub reads the workflow from the default branch's commit.
        let workflow = git(&runner, &["show", "HEAD:.github/workflows/codeflow-policy.yml"]);
        let temp = self._dir.path().join(format!("temp-{number}"));
        std::fs::create_dir_all(&temp).unwrap();
        let pull = temp.join("pull-served.json");
        std::fs::write(
            &pull,
            serde_json::json!({"head": {"sha": head}, "base": {"ref": "main"}, "body": body(number)}).to_string(),
        )
        .unwrap();
        let mut log = String::new();
        let mut outputs: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let path = format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap());
        for step in steps(&workflow) {
            if step.uses.is_some() || step.name.starts_with("Install codeflow") {
                continue; // the checkout above and the binary under test
            }
            if let Some(condition) = &step.condition {
                assert_eq!(condition, "github.event_name == 'pull_request_target'");
            }
            let mut env: Vec<(String, String)> = Vec::new();
            for (key, value) in &step.env {
                env.push((key.clone(), expand(value, number, head, branch, &authority, &temp, &outputs)));
            }
            let mut fixed = vec![
                ("GITHUB_REF", "refs/heads/main".to_string()),
                ("GITHUB_API_URL", "http://api.invalid".to_string()),
                ("GITHUB_REPOSITORY", "owner/repo".to_string()),
                ("RUNNER_TEMP", temp.display().to_string()),
                ("PULL_JSON", pull.display().to_string()),
                ("PATH", path.clone()),
                ("CODEFLOW_HOME", temp.join("home").display().to_string()),
            ];
            fixed.extend(env.iter().map(|(k, v)| (k.as_str(), v.clone())));
            let pairs: Vec<(&str, &str)> = fixed.iter().map(|(k, v)| (*k, v.as_str())).collect();
            let (ok, text, step_outputs) = run_step(step.run.as_deref().unwrap(), &runner, &pairs);
            log.push_str(&format!("== {}\n{text}", step.name));
            if let Some(id) = &step.id {
                outputs.insert(id.clone(), step_outputs);
            }
            if !ok {
                return (Some(step.name), log);
            }
        }
        (None, log)
    }
}

/// The value GitHub would give each expression the verdict job uses.
fn expand(
    value: &str,
    number: u32,
    head: &str,
    branch: &str,
    authority: &str,
    temp: &Path,
    outputs: &BTreeMap<String, BTreeMap<String, String>>,
) -> String {
    if let Some(open) = value.find("${{ ") {
        let close = open + value[open..].find(" }}").unwrap();
        let inner = &value[open + 4..close];
        let rest = &value[close + 3..];
        return format!(
            "{}{}{}",
            &value[..open],
            expression(inner, number, head, branch, authority, temp, outputs),
            expand(rest, number, head, branch, authority, temp, outputs)
        );
    }
    value.to_string()
}

fn expression(
    inner: &str,
    number: u32,
    head: &str,
    branch: &str,
    authority: &str,
    temp: &Path,
    outputs: &BTreeMap<String, BTreeMap<String, String>>,
) -> String {
    if let Some(rest) = inner.strip_prefix("steps.") {
        let (id, key) = rest.split_once(".outputs.").unwrap();
        return outputs.get(id).and_then(|o| o.get(key)).cloned().unwrap_or_default();
    }
    match inner {
        "github.event_name" => "pull_request_target".into(),
        "github.event.repository.default_branch" => "main".into(),
        "github.sha" => authority.into(),
        "github.event.pull_request.number" => number.to_string(),
        "github.event.pull_request.head.sha" => head.into(),
        "github.event.pull_request.head.ref" => branch.into(),
        "github.event.pull_request.base.ref" => "main".into(),
        "github.event.pull_request.head.repo.full_name" => "owner/repo".into(),
        "github.token" => "ghs_fixture_token".into(),
        "runner.temp" => temp.display().to_string(),
        "github.event.pull_request.head.repo.full_name == github.repository && github.actor || 'unknown'" => "unknown".into(),
        other => panic!("the fixture does not model the expression {other:?}"),
    }
}

/// TSK-1103 AC-1, AC-9 and partial AC-8: the shipped verdict job, run from
/// the default branch against a local origin, passes a clean pull request,
/// refuses one that adds a file the boundary forbids, and refuses the same
/// change when its head rewrites the check to pass, empties the boundary
/// file and deletes the step from its own copy of the workflow.
#[test]
fn the_verdict_job_runs_the_default_branch_checks() {
    let job = Job::new();
    let clean = job.pull_request(1, "task/TSK-001-clean", |root| write(root, "src/more.rs", "// more\n"));
    let (failed, log) = job.run(1, "task/TSK-001-clean", &clean);
    assert_eq!(failed, None, "{log}");
    assert!(log.contains("codeflow ci base-checks: boundary passed"), "{log}");

    let vendor = job.pull_request(2, "task/TSK-002-vendor", |root| write(root, "vendor/lib.rs", "// copied\n"));
    let (failed, log) = job.run(2, "task/TSK-002-vendor", &vendor);
    assert_eq!(failed.as_deref(), Some("Project checks from the default branch"), "{log}");
    assert!(log.contains("boundary: refused"), "{log}");

    let rewritten = job.pull_request(3, "task/TSK-003-rewrite", |root| {
        write(root, "vendor/lib.rs", "// copied\n");
        write(root, ".codeflow/checks/boundary.sh", "exit 0\n");
        write(root, "docs/boundary.txt", "^nothing$\n");
        write(root, ".codeflow/base-checks.json", "{\"version\": 1}\n");
        let workflow = std::fs::read_to_string(root.join(".github/workflows/codeflow-policy.yml")).unwrap();
        write(root, ".github/workflows/codeflow-policy.yml", &workflow.replace("codeflow ci base-checks", "true"));
    });
    let (failed, log) = job.run(3, "task/TSK-003-rewrite", &rewritten);
    assert_eq!(failed.as_deref(), Some("Project checks from the default branch"), "{log}");
    assert!(log.contains("boundary: refused"), "{log}");
}
