//! The scaffolded CI binary pin (SPC-013 R-113) and the enforcing job on
//! `pull_request_target`: the install step verifies the target-pinned release
//! against its published `sha256.sum` and fails closed, every copy of the
//! step is the same script, and a pull request that edits the workflow, the
//! pin and the policy changes neither the enforcing job's pinned version nor
//! its verdict.
//!
//! The install script is executed from the shipped YAML against a local
//! `file://` release, so the test runs the exact bytes adopters receive.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CI: &str = include_str!("../../../assets/base/ci/codeflow-ci.yml");
const POLICY: &str = include_str!("../../../assets/base/ci/codeflow-policy.yml");
const ASSET: &str = "codeflow-cli-x86_64-unknown-linux-gnu";

/// The `run:` script of every step whose name starts with `prefix`: the
/// block of a `run: |`, or the inline command.
fn run_blocks(workflow: &str, prefix: &str) -> Vec<String> {
    let lines: Vec<&str> = workflow.lines().collect();
    let mut blocks = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(name) = line.trim_start().strip_prefix("- name: ") else {
            continue;
        };
        if !name.starts_with(prefix) {
            continue;
        }
        let step_end = lines[i + 1..]
            .iter()
            .position(|l| l.trim_start().starts_with("- "))
            .map_or(lines.len(), |p| i + 1 + p);
        let Some(run) = (i + 1..step_end).find(|&j| lines[j].trim_start().starts_with("run:"))
        else {
            continue;
        };
        let inline = lines[run].trim_start().trim_start_matches("run:").trim();
        if inline != "|" {
            blocks.push(format!("{inline}\n"));
            continue;
        }
        let indent = lines[run].len() - lines[run].trim_start().len() + 2;
        let body: Vec<String> = lines[run + 1..step_end]
            .iter()
            .take_while(|l| l.trim().is_empty() || l.len() - l.trim_start().len() >= indent)
            .map(|l| l.get(indent..).unwrap_or("").to_string())
            .collect();
        blocks.push(body.join("\n").trim_end().to_string() + "\n");
    }
    blocks
}

fn install_script(workflow: &str) -> String {
    run_blocks(workflow, "Install codeflow")
        .into_iter()
        .next()
        .expect("install step")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("CODEFLOW_INTEGRATE_TOKEN", "test")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn sha256(path: &Path) -> String {
    let out = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .expect("shasum runs");
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

/// A local release for `version`: the archive holding a fake `codeflow`
/// that prints its version, and `sha256.sum` listing it.
fn publish(releases: &Path, version: &str) -> PathBuf {
    let dir = releases.join(format!("v{version}"));
    let stage = releases.join(format!("stage-{version}")).join(ASSET);
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let bin = stage.join("codeflow");
    std::fs::write(&bin, format!("#!/bin/sh\necho \"codeflow {version}\"\n")).unwrap();
    Command::new("chmod").arg("755").arg(&bin).status().unwrap();
    let archive = dir.join(format!("{ASSET}.tar.xz"));
    let status = Command::new("tar")
        .arg("-cJf")
        .arg(&archive)
        .arg("-C")
        .arg(stage.parent().unwrap())
        .arg(ASSET)
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::write(
        dir.join("sha256.sum"),
        format!(
            "{}  {ASSET}.tar.xz\n0000  codeflow-cli-installer.sh\n",
            sha256(&archive)
        ),
    )
    .unwrap();
    dir
}

fn project_toml(version: &str) -> String {
    format!(
        "schema_version = 1\ntier = \"minimal\"\nscaffold_version = \"{version}\"\nstack = \"unset\"\nareas = [\"core\"]\npolicy_armed = true\ngit_hooks = \"unwired\"\npermission_preset = \"default\"\n"
    )
}

/// A repository whose `main` pins `version`.
fn repo(dir: &Path, version: &str) {
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
    std::fs::create_dir_all(dir.join(".codeflow")).unwrap();
    std::fs::write(dir.join(".codeflow/project.toml"), project_toml(version)).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "chore: init"]);
}

struct Install {
    out: Output,
    home: tempfile::TempDir,
    path_file: PathBuf,
}

impl Install {
    fn installed_version(&self) -> Option<String> {
        let bin = self.home.path().join(".codeflow/bin/codeflow");
        let out = Command::new(bin).output().ok()?;
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
    fn stderr(&self) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(&self.out.stdout),
            String::from_utf8_lossy(&self.out.stderr)
        )
    }
}

fn run_install(script: &str, repo: &Path, pin_ref: &str, releases: &Path) -> Install {
    let home = tempfile::tempdir().unwrap();
    let path_file = home.path().join("github_path");
    std::fs::write(&path_file, "").unwrap();
    let out = Command::new("bash")
        .arg("-c")
        .arg(script)
        .current_dir(repo)
        .env("PIN_REF", pin_ref)
        .env(
            "CODEFLOW_RELEASE_URL",
            format!("file://{}", releases.display()),
        )
        .env("HOME", home.path())
        .env("GITHUB_PATH", &path_file)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("bash runs");
    Install {
        out,
        home,
        path_file,
    }
}

#[test]
fn every_install_step_is_the_same_verified_script() {
    let mut blocks = run_blocks(CI, "Install codeflow");
    blocks.extend(run_blocks(CI, "Install candidate codeflow"));
    blocks.extend(run_blocks(POLICY, "Install codeflow"));
    assert_eq!(blocks.len(), 3, "gates, candidate and the enforcing job");
    assert!(blocks.iter().all(|b| b == &blocks[0]));
    assert!(blocks[0].contains("sha256.sum"));
    for workflow in [CI, POLICY] {
        assert!(!workflow.contains("PLACEHOLDER"));
    }
    // The enforcing job reads the pin from the target checkout.
    assert!(POLICY.contains("PIN_REF: HEAD"));
}

#[test]
fn pinned_install_verifies_the_release_and_fails_closed() {
    let script = install_script(POLICY);
    let dir = tempfile::tempdir().unwrap();
    let releases = dir.path().join("releases");
    let work = dir.path().join("repo");
    std::fs::create_dir_all(&work).unwrap();
    repo(&work, "1.2.3");
    let release = publish(&releases, "1.2.3");

    // A verified release installs and joins the PATH.
    let ok = run_install(&script, &work, "HEAD", &releases);
    assert!(ok.out.status.success(), "{}", ok.stderr());
    assert_eq!(ok.installed_version().as_deref(), Some("codeflow 1.2.3"));
    let path = std::fs::read_to_string(&ok.path_file).unwrap();
    assert!(path.contains(".codeflow/bin"), "{path}");

    // A tampered asset fails closed and installs nothing.
    let archive = release.join(format!("{ASSET}.tar.xz"));
    let original = std::fs::read(&archive).unwrap();
    let mut tampered = original.clone();
    tampered.extend_from_slice(b"tampered");
    std::fs::write(&archive, &tampered).unwrap();
    let bad = run_install(&script, &work, "HEAD", &releases);
    assert!(!bad.out.status.success());
    assert!(
        bad.stderr().contains("checksum mismatch"),
        "{}",
        bad.stderr()
    );
    assert!(bad.installed_version().is_none());
    std::fs::write(&archive, &original).unwrap();

    // A missing checksum file fails closed.
    let sums = release.join("sha256.sum");
    let listed = std::fs::read_to_string(&sums).unwrap();
    std::fs::remove_file(&sums).unwrap();
    let missing = run_install(&script, &work, "HEAD", &releases);
    assert!(!missing.out.status.success());
    assert!(
        missing.stderr().contains("no published checksum file"),
        "{}",
        missing.stderr()
    );
    assert!(missing.installed_version().is_none());

    // A checksum file without this asset fails closed.
    std::fs::write(&sums, "0000  codeflow-cli-installer.sh\n").unwrap();
    let unlisted = run_install(&script, &work, "HEAD", &releases);
    assert!(!unlisted.out.status.success());
    assert!(
        unlisted.stderr().contains("lists no checksum"),
        "{}",
        unlisted.stderr()
    );
    std::fs::write(&sums, listed).unwrap();

    // No pin fails closed.
    std::fs::write(work.join(".codeflow/project.toml"), "tier = \"minimal\"\n").unwrap();
    git(&work, &["commit", "-qam", "chore: drop the pin"]);
    let unpinned = run_install(&script, &work, "HEAD", &releases);
    assert!(!unpinned.out.status.success());
    assert!(
        unpinned.stderr().contains("no scaffold_version pinned"),
        "{}",
        unpinned.stderr()
    );
}

/// The job-level text of `id` in a workflow, up to the next job.
fn job<'w>(workflow: &'w str, id: &str) -> &'w str {
    let start = workflow
        .find(&format!("\n  {id}:\n"))
        .unwrap_or_else(|| panic!("job {id}"))
        + 1;
    let rest = &workflow[start..];
    let mut offset = rest.find('\n').unwrap() + 1;
    for line in rest[offset..].split_inclusive('\n') {
        if line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':') {
            break;
        }
        offset += line.len();
    }
    &rest[..offset]
}

/// The expression that selects the pull request's base commit.
const BASE_SHA_REF: &str = "${{ github.event.pull_request.base.sha }}";

/// The checkout of the enforcing job: the pull request's base commit, and
/// the event's own commit for the registry's push, schedule and dispatch
/// runs.
const TARGET_REF: &str =
    "${{ github.event_name == 'pull_request_target' && github.event.pull_request.base.sha || '' }}";

/// The `with:` block of the first checkout step in `job_text`.
fn checkout_block(job_text: &str) -> &str {
    job_text
        .split("- uses: actions/checkout@")
        .nth(1)
        .and_then(|rest| rest.split("\n\n").next())
        .expect("a checkout step")
}

/// The checkout step's `ref:` value, when it sets one.
fn checkout_ref(checkout: &str) -> Option<String> {
    checkout
        .lines()
        .find_map(|l| l.trim().strip_prefix("ref: "))
        .map(str::to_string)
}

/// What a workflow's checkout `ref:` resolves to for `event`: the commit a
/// run checks out, or `None` for GitHub's default checkout. Only the forms
/// the shipped workflows use are understood; anything else fails the test.
fn resolve_ref(expr: Option<&str>, event: &str, base_sha: &str) -> Option<String> {
    match expr {
        None => None,
        Some(e) if e == BASE_SHA_REF => Some(base_sha.to_string()),
        Some(e) if e == TARGET_REF => {
            (event == "pull_request_target").then(|| base_sha.to_string())
        }
        Some(other) => panic!("unrecognised checkout ref {other}"),
    }
}

#[test]
fn the_enforcing_job_runs_the_target_workflow_and_reads_the_head_as_data() {
    // pull_request_target, never pull_request: GitHub runs this file from
    // the default branch, never from the pull request. Push, schedule and
    // dispatch are the registry check's own events, on the event's commit.
    let on = POLICY
        .split("\non:\n")
        .nth(1)
        .and_then(|rest| rest.split("\n\n").next())
        .unwrap();
    assert!(on.contains("pull_request_target:"), "{on}");
    assert!(!on.contains("  pull_request:"), "{on}");
    for event in ["push:", "schedule:", "workflow_dispatch:"] {
        assert!(on.contains(event), "{on}");
    }
    assert!(
        !POLICY.contains("concurrency:"),
        "a registry run is never replaced by a queued event"
    );
    assert!(POLICY.contains("\npermissions:\n  contents: read\n"));
    assert!(
        !POLICY.contains("secrets."),
        "no secret reaches the enforcing job"
    );

    let enforcing = job(POLICY, "commit-lint");
    assert!(enforcing.contains("name: commit standards"));
    // GitHub's default checkout for pull_request_target is the default
    // branch, whatever the pull request targets, so the job names the pull
    // request's base commit; nothing points the checkout at the head.
    let checkout = checkout_block(enforcing);
    assert_eq!(
        checkout_ref(checkout).as_deref(),
        Some(TARGET_REF),
        "{checkout}"
    );
    assert!(
        !checkout.contains("head") && !checkout.contains("repository:"),
        "{checkout}"
    );
    // The pin is read from the target checkout.
    assert!(enforcing.contains("PIN_REF: HEAD"));
    // The head is fetched as a ref and only named by sha to codeflow ci.
    assert!(enforcing.contains("refs/pull/${PR_NUMBER}/head:refs/codeflow/pr-head"));
    for forbidden in [
        "git checkout",
        "git switch",
        "./",
        "bash ",
        "sh ",
        "npm ",
        "cargo ",
    ] {
        let runs: Vec<String> = run_blocks(enforcing, "");
        assert!(
            runs.iter().all(|r| !r.contains(forbidden)),
            "the enforcing job never runs head content ({forbidden})"
        );
    }
    // No skipped twin: no other workflow defines the enforcing check name.
    assert!(!CI.contains("name: commit standards"));
    assert!(!CI.contains("  pull_request_target:"));
}

/// A pull request that edits the enforcing workflow to pass, raises the pin
/// and relaxes the policy changes nothing: the `pull_request_target` run
/// reads the workflow, the pin and the policy from the target.
#[test]
fn a_pr_editing_the_workflow_changes_neither_the_pin_nor_the_verdict() {
    judge_a_forged_head("9.9.9");
}

/// SPC-013 R-104 (TSK-110): a head that lowers the pin while it touches the
/// workflow and the policy is judged by the target's binary, as a raise is.
#[test]
fn a_pr_lowering_the_pin_is_still_judged_by_the_target_binary() {
    judge_a_forged_head("1.0.0");
}

/// A head that sets its own pin to `head_pin`, forges the workflow, relaxes
/// the policy and breaks the commit format; the target's pinned binary
/// judges it and blocks.
fn judge_a_forged_head(head_pin: &str) {
    let dir = tempfile::tempdir().unwrap();
    let releases = dir.path().join("releases");
    publish(&releases, "1.2.3");
    publish(&releases, head_pin);
    let origin = dir.path().join("origin");
    std::fs::create_dir_all(&origin).unwrap();
    repo(&origin, "1.2.3");
    std::fs::create_dir_all(origin.join(".github/workflows")).unwrap();
    std::fs::write(origin.join(".github/workflows/codeflow-policy.yml"), POLICY).unwrap();
    std::fs::write(
        origin.join(".codeflow/policy.json"),
        include_str!("../../../assets/base/policy.json"),
    )
    .unwrap();
    git(&origin, &["add", "."]);
    git(&origin, &["commit", "-m", "ci: add the enforcing workflow"]);

    // The head: an always-green workflow, its own pin, a relaxed policy and
    // a commit that breaks the target's format rule.
    git(&origin, &["checkout", "-b", "feat/x"]);
    let forged = POLICY.replace(
        "run: codeflow ci --base \"$BASE_SHA\" --head \"$HEAD_SHA\" --branch \"$HEAD_REF\" --actor \"$ACTOR\"",
        "run: exit 0",
    );
    assert_ne!(forged, POLICY);
    std::fs::write(origin.join(".github/workflows/codeflow-policy.yml"), forged).unwrap();
    std::fs::write(
        origin.join(".codeflow/project.toml"),
        project_toml(head_pin),
    )
    .unwrap();
    let relaxed = include_str!("../../../assets/base/policy.json")
        .replace("\"commit_format\": \"block\"", "\"commit_format\": \"off\"");
    std::fs::write(origin.join(".codeflow/policy.json"), relaxed).unwrap();
    git(&origin, &["add", "."]);
    git(&origin, &["commit", "-m", "Make every check pass"]);
    let head = git(&origin, &["rev-parse", "HEAD"]);
    git(&origin, &["checkout", "main"]);
    let base = git(&origin, &["rev-parse", "main"]);

    // The pull_request_target run: the target checkout, the head as data.
    let target = dir.path().join("target");
    git(
        dir.path(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            target.to_str().unwrap(),
        ],
    );
    git(
        &target,
        &["fetch", "-q", "origin", "feat/x:refs/codeflow/pr-head"],
    );
    assert_eq!(git(&target, &["rev-parse", "refs/codeflow/pr-head"]), head);

    let workflow =
        std::fs::read_to_string(target.join(".github/workflows/codeflow-policy.yml")).unwrap();
    assert_eq!(workflow, POLICY, "the run uses the target's workflow");
    let install = run_install(&install_script(&workflow), &target, "HEAD", &releases);
    assert!(install.out.status.success(), "{}", install.stderr());
    assert_eq!(
        install.installed_version().as_deref(),
        Some("codeflow 1.2.3")
    );

    let verdict = |cwd: &Path| {
        Command::new(env!("CARGO_BIN_EXE_codeflow"))
            .args([
                "ci", "--base", &base, "--head", &head, "--branch", "feat/x", "--actor", "unknown",
            ])
            .current_dir(cwd)
            .env("CODEFLOW_HOME", dir.path().join("home"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("CODEFLOW_PR_BODY")
            .env_remove("GITHUB_EVENT_NAME")
            .env_remove("GIT_DIR")
            .output()
            .unwrap()
    };
    let enforced = verdict(&target);
    let text = String::from_utf8_lossy(&enforced.stderr);
    assert_eq!(enforced.status.code(), Some(1), "{text}");
    assert!(text.contains("git.commit_format"), "{text}");

    // Run from the head's own tree, the relaxed policy no longer judges
    // the range: the policy is read at the base as git data
    // (sathyassn/codeflow#22), so the checkout cannot choose it.
    git(&origin, &["checkout", "-q", "feat/x"]);
    let forged_run = verdict(&origin);
    let forged_text = String::from_utf8_lossy(&forged_run.stderr);
    assert_eq!(forged_run.status.code(), Some(1), "{forged_text}");
    assert!(forged_text.contains("git.commit_format"), "{forged_text}");
}

/// A repository whose default branch `main` pins 1.2.3 and allows a
/// 50-character description, an `integration/check` target pinning 1.2.4
/// with a 10-character limit, and `feat/longer` on it with a 24-character
/// description. Returns the target's commit and the head's; `main` is left
/// checked out.
fn default_and_integration_targets(origin: &Path) -> (String, String) {
    std::fs::create_dir_all(origin).unwrap();
    repo(origin, "1.2.3");
    let policy = include_str!("../../../assets/base/policy.json");
    assert!(policy.contains("\"commit_desc_max_len\": 50,"));
    std::fs::write(origin.join(".codeflow/policy.json"), policy).unwrap();
    git(origin, &["add", "."]);
    git(origin, &["commit", "-m", "chore: add the policy"]);

    git(origin, &["checkout", "-q", "-b", "integration/check"]);
    std::fs::write(
        origin.join(".codeflow/policy.json"),
        policy.replace(
            "\"commit_desc_max_len\": 50,",
            "\"commit_desc_max_len\": 10,",
        ),
    )
    .unwrap();
    std::fs::write(origin.join(".codeflow/project.toml"), project_toml("1.2.4")).unwrap();
    git(origin, &["add", "."]);
    git(origin, &["commit", "-m", "chore: tighten"]);
    let base = git(origin, &["rev-parse", "HEAD"]);

    git(origin, &["checkout", "-q", "-b", "feat/longer"]);
    std::fs::write(origin.join("change.txt"), "fixture\n").unwrap();
    git(origin, &["add", "."]);
    git(origin, &["commit", "-m", "feat: add a longer description"]);
    let head = git(origin, &["rev-parse", "HEAD"]);
    git(origin, &["checkout", "-q", "main"]);
    (base, head)
}

/// A pull request into a target that is not the default branch is judged by
/// that target. GitHub runs the enforcing workflow from the default branch
/// and its default checkout is the default branch too, so only the explicit
/// base-commit checkout makes the pin and the policy the target's. The
/// default branch allows a 50-character description, the integration target
/// 10; the head's 24-character description must fail as the target says.
#[test]
fn a_pull_request_into_a_non_default_target_is_judged_by_that_target() {
    let dir = tempfile::tempdir().unwrap();
    let releases = dir.path().join("releases");
    publish(&releases, "1.2.3");
    publish(&releases, "1.2.4");
    let origin = dir.path().join("origin");
    let (base, head) = default_and_integration_targets(&origin);

    // The run's working tree: GitHub's default checkout, the default branch.
    let target = dir.path().join("target");
    git(
        dir.path(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            target.to_str().unwrap(),
        ],
    );
    assert_eq!(git(&target, &["branch", "--show-current"]), "main");
    git(
        &target,
        &["fetch", "-q", "origin", "feat/longer:refs/codeflow/pr-head"],
    );
    let verdict = |cwd: &Path| {
        Command::new(env!("CARGO_BIN_EXE_codeflow"))
            .args([
                "ci",
                "--base",
                &base,
                "--head",
                &head,
                "--branch",
                "feat/longer",
                "--actor",
                "unknown",
            ])
            .current_dir(cwd)
            .env("CODEFLOW_HOME", dir.path().join("home"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("CODEFLOW_PR_BODY")
            .env_remove("GITHUB_ACTIONS")
            .env_remove("GITHUB_EVENT_NAME")
            .env_remove("GITHUB_EVENT_PATH")
            .env_remove("GIT_DIR")
            .output()
            .unwrap()
    };
    // From the default checkout the policy is still the base's, read as
    // git data (sathyassn/codeflow#22), so the verdict no longer depends on
    // the checkout; the pin still does, which is why the workflow checks
    // out the base: without it the default branch's pin would be installed.
    let from_default = verdict(&target);
    let from_default_text = String::from_utf8_lossy(&from_default.stderr);
    assert_eq!(from_default.status.code(), Some(1), "{from_default_text}");
    assert!(
        from_default_text.contains("over the 10-char limit"),
        "{from_default_text}"
    );
    let default_pin = run_install(&install_script(POLICY), &target, "HEAD", &releases);
    assert_eq!(
        default_pin.installed_version().as_deref(),
        Some("codeflow 1.2.3")
    );

    // The enforcing workflow selects the base commit for a pull request;
    // the registry's push, schedule and dispatch runs keep their own commit.
    let policy_ref = checkout_ref(checkout_block(POLICY));
    let selected = resolve_ref(policy_ref.as_deref(), "pull_request_target", &base);
    assert_eq!(selected.as_deref(), Some(base.as_str()));
    for event in ["push", "schedule", "workflow_dispatch"] {
        assert_eq!(resolve_ref(policy_ref.as_deref(), event, &base), None);
    }

    // The run as the workflows specify it: the base commit checked out.
    git(&target, &["checkout", "-q", "--detach", &base]);
    let install = run_install(&install_script(POLICY), &target, "HEAD", &releases);
    assert!(install.out.status.success(), "{}", install.stderr());
    assert_eq!(
        install.installed_version().as_deref(),
        Some("codeflow 1.2.4"),
        "the target's pin"
    );
    let enforced = verdict(&target);
    let text = String::from_utf8_lossy(&enforced.stderr);
    assert_eq!(enforced.status.code(), Some(1), "{text}");
    assert!(text.contains("git.commit_format"), "{text}");
    assert!(text.contains("over the 10-char limit"), "{text}");
}
