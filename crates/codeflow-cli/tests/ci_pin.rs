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
    // The enforcing jobs read the pin from the target checkout.
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

#[test]
fn the_enforcing_job_runs_the_target_workflow_and_reads_the_head_as_data() {
    // Only pull_request_target: GitHub runs this file from the target branch.
    let on = POLICY
        .split("\non:\n")
        .nth(1)
        .and_then(|rest| rest.split("\n\n").next())
        .unwrap();
    assert!(on.contains("pull_request_target:"), "{on}");
    assert!(
        !on.contains("  pull_request:") && !on.contains("push:"),
        "{on}"
    );
    assert!(POLICY.contains("\npermissions:\n  contents: read\n"));
    assert!(
        !POLICY.contains("secrets."),
        "no secret reaches the enforcing job"
    );

    let enforcing = job(POLICY, "commit-lint");
    assert!(enforcing.contains("name: commit standards"));
    // The checkout is the target: no ref or repository points at the head.
    let checkout = enforcing
        .split("- uses: actions/checkout@")
        .nth(1)
        .and_then(|rest| rest.split("\n\n").next())
        .unwrap();
    assert!(
        !checkout.contains("ref:") && !checkout.contains("repository:"),
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
    let dir = tempfile::tempdir().unwrap();
    let releases = dir.path().join("releases");
    publish(&releases, "1.2.3");
    publish(&releases, "9.9.9");
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

    // The head: an always-green workflow, a raised pin, a relaxed policy and
    // a commit that breaks the target's format rule.
    git(&origin, &["checkout", "-b", "feat/x"]);
    let forged = POLICY.replace(
        "run: codeflow ci --base \"$BASE_SHA\" --head \"$HEAD_SHA\" --branch \"$HEAD_REF\" --actor \"$ACTOR\"",
        "run: exit 0",
    );
    assert_ne!(forged, POLICY);
    std::fs::write(origin.join(".github/workflows/codeflow-policy.yml"), forged).unwrap();
    std::fs::write(origin.join(".codeflow/project.toml"), project_toml("9.9.9")).unwrap();
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

    // Control: judged from the head's own tree, the relaxed policy would pass.
    git(&origin, &["checkout", "-q", "feat/x"]);
    let forged_run = verdict(&origin);
    assert_eq!(
        forged_run.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&forged_run.stderr)
    );
}
