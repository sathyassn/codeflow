use std::{fs, path::PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("CLI crate must live below the workspace root")
        .to_path_buf()
}

#[test]
fn distribution_config_keeps_supported_targets_and_installers() {
    let root = workspace_root();
    let raw = fs::read_to_string(root.join("dist-workspace.toml"))
        .expect("dist-workspace.toml must be readable");
    let config: toml::Value = toml::from_str(&raw).expect("distribution config must be TOML");
    let dist = config
        .get("dist")
        .and_then(toml::Value::as_table)
        .expect("[dist] table must exist");

    let targets = dist
        .get("targets")
        .and_then(toml::Value::as_array)
        .expect("dist targets must be an array")
        .iter()
        .map(|value| value.as_str().expect("target must be a string"))
        .collect::<Vec<_>>();
    // 3.1.0 restores native Windows (TSK-197).
    assert_eq!(
        targets,
        [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
        ],
        "release targets must be exactly the platforms this release supports"
    );

    let installers = dist
        .get("installers")
        .and_then(toml::Value::as_array)
        .expect("dist installers must be an array")
        .iter()
        .map(|value| value.as_str().expect("installer must be a string"))
        .collect::<Vec<_>>();
    assert_eq!(installers, ["shell", "powershell"]);

    assert_eq!(
        dist.get("dispatch-releases").and_then(toml::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        dist.get("github-release").and_then(toml::Value::as_str),
        Some("announce")
    );
    assert_eq!(
        dist.get("create-release").and_then(toml::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        dist.get("local-artifacts-jobs")
            .and_then(toml::Value::as_array)
            .and_then(|jobs| jobs.first())
            .and_then(toml::Value::as_str),
        Some("./release-plan-authority")
    );
    assert_eq!(
        dist.get("global-artifacts-jobs")
            .and_then(toml::Value::as_array)
            .and_then(|jobs| jobs.first())
            .and_then(toml::Value::as_str),
        Some("./release-main-recheck")
    );
    assert_eq!(
        dist.get("post-announce-jobs")
            .and_then(toml::Value::as_array)
            .and_then(|jobs| jobs.first())
            .and_then(toml::Value::as_str),
        Some("./release-post-announce")
    );
    assert_eq!(
        dist.get("pr-run-mode").and_then(toml::Value::as_str),
        Some("plan")
    );
    assert!(
        dist.get("host-jobs").is_none(),
        "cargo-dist 0.32 host jobs do not gate its host job; authority must be a local artifact"
    );
}

#[test]
fn custom_release_jobs_have_exact_scoped_permissions() {
    let raw = fs::read_to_string(workspace_root().join("dist-workspace.toml"))
        .expect("dist-workspace.toml must be readable");
    let config: toml::Value = toml::from_str(&raw).expect("distribution config must be TOML");
    let jobs = config["dist"]["github-custom-job-permissions"]
        .as_table()
        .expect("custom-job permissions must be explicit");
    let authority = jobs["release-plan-authority"]
        .as_table()
        .expect("authority permissions must be a table");
    assert_eq!(authority["actions"].as_str(), Some("read"));
    assert_eq!(authority["contents"].as_str(), Some("write"));
    assert_eq!(authority["pull-requests"].as_str(), Some("read"));
    assert_eq!(authority["checks"].as_str(), Some("read"));
    let recheck = jobs["release-main-recheck"]
        .as_table()
        .expect("main recheck permissions must be a table");
    assert_eq!(recheck["actions"].as_str(), Some("read"));
    assert_eq!(recheck["contents"].as_str(), Some("write"));
    assert_eq!(recheck["checks"].as_str(), Some("read"));
}

#[test]
fn generated_release_workflow_uses_cargo_dist_platform_matrix() {
    let workflow = fs::read_to_string(workspace_root().join(".github/workflows/release.yml"))
        .expect("generated release workflow must be readable");
    for required in [
        "runs-on: ${{ matrix.runner }}",
        "enable windows longpaths",
        "--output-format=json > plan-dist-manifest.json",
        "workflow_dispatch:",
        "custom-release-plan-authority:",
        "uses: ./.github/workflows/release-plan-authority.yml",
        "custom-release-main-recheck:",
        "uses: ./.github/workflows/release-main-recheck.yml",
        "plan: ${{ needs.plan.outputs.val }}",
        "gh release upload",
        "gh release edit",
        "custom-release-post-announce:",
        "uses: ./.github/workflows/release-post-announce.yml",
    ] {
        assert!(
            workflow.contains(required),
            "generated workflow is missing {required}"
        );
    }
    assert!(!workflow.contains("push:\n    tags:"));
    assert!(workflow.contains("  custom-release-plan-authority:\n"));
    let custom_call = workflow
        .split("  custom-release-plan-authority:\n")
        .nth(1)
        .expect("generated authority call must exist")
        .split("\n  # Build and package")
        .next()
        .expect("authority call must precede global packaging");
    assert!(custom_call.contains("\"contents\": \"write\""));
    assert!(custom_call.contains("\"actions\": \"read\""));
    assert!(custom_call.contains("\"pull-requests\": \"read\""));
    let recheck_call = workflow
        .split("  custom-release-main-recheck:\n")
        .nth(1)
        .expect("generated main recheck call must exist")
        .split("\n  # Determines if we should publish")
        .next()
        .expect("main recheck must precede hosting");
    assert!(recheck_call.contains("\"actions\": \"read\""));
    let post_announce = workflow
        .split("  custom-release-post-announce:\n")
        .nth(1)
        .expect("generated post-announce verifier must exist");
    assert!(post_announce.contains("- announce"));
    assert!(post_announce.contains("plan: ${{ needs.plan.outputs.val }}"));
    let host = workflow
        .split("  host:\n")
        .nth(1)
        .expect("generated host job must exist")
        .split("\n  announce:")
        .next()
        .expect("host job must precede announce");
    assert!(host.contains("- custom-release-plan-authority"));
    assert!(host.contains("- custom-release-main-recheck"));
    assert!(host.contains("needs.custom-release-plan-authority.result == 'skipped'"));
    assert!(host.contains("needs.custom-release-plan-authority.result == 'success'"));
    assert!(host.contains("needs.custom-release-main-recheck.result == 'skipped'"));
    assert!(host.contains("needs.custom-release-main-recheck.result == 'success'"));
    for blocked in ["failure", "cancelled"] {
        assert!(
            !host.contains(&format!("result == '{blocked}'")),
            "a {blocked} authority or recheck must block hosting"
        );
    }
    let announce = workflow
        .split("  announce:\n")
        .nth(1)
        .expect("generated announce job must exist")
        .split("\n  custom-release-post-announce:")
        .next()
        .expect("announce must precede post-announce verification");
    assert!(announce.contains("needs.host.result == 'success'"));
    assert!(!announce.contains("needs.host.result == 'failure'"));
    assert!(!announce.contains("needs.host.result == 'cancelled'"));
    let custom = workflow
        .split("  custom-release-plan-authority:\n")
        .nth(1)
        .expect("custom authority job must exist")
        .split("\n  # Build and package")
        .next()
        .expect("custom authority must precede global artifacts");
    assert!(
        custom.contains("needs.plan.outputs.publishing == 'true'"),
        "a publishing run cannot skip authority"
    );
    for pin in [
        "actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803",
        "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
        "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c",
    ] {
        assert!(
            workflow.contains(pin),
            "generated workflow is missing action pin {pin}"
        );
    }
}

#[test]
fn release_workflows_keep_same_pr_and_current_main_boundary() {
    let root = workspace_root();
    assert!(!root
        .join(".github/workflows/release-candidate.yml")
        .exists());
    assert!(!root
        .join(".github/workflows/release-authorize.yml")
        .exists());
    assert!(!root.join("cliff.toml").exists());

    let authority = fs::read_to_string(root.join(".github/workflows/release-plan-authority.yml"))
        .expect("plan authority workflow must be readable");
    for required in [
        "actions: read",
        "test \"$GITHUB_REF\" = refs/heads/main",
        "git/ref/heads/main",
        "test \"$GITHUB_SHA\" = \"$main_sha\"",
        "GITHUB_TRIGGERING_ACTOR",
        "collaborators/${login}/permission",
        "commits/${GITHUB_SHA}/pulls",
        "pulls/${pr_number}",
        "merge_commit_sha == env.GITHUB_SHA",
        "scripts/release.py verify-authority",
        "check-runs?filter=all&per_page=100",
        "actions/workflows/codeflow-ci.yml/runs?branch=main&event=push&head_sha=",
        "--runs-state /tmp/authority-runs.json",
        "actions/workflows/codeflow-release.yml/runs?branch=main&event=push&head_sha=",
        "--runs-state /tmp/authority-release-runs.json",
        "scripts/release.py verify-publication",
        "scripts/release.py host-state",
        "scripts/release.py verify-host-state",
        "scripts/release.py release-notes",
        "--repository \"$GITHUB_REPOSITORY\" --output /tmp/release-notes.md",
        "gh release create",
        "--draft",
        "github.event.inputs.tag == 'dry-run'",
        "github.event.inputs.tag != 'dry-run'",
        "PLAN_TAG: ${{ fromJSON(inputs.plan).announcement_tag }}",
        "test \"$PLAN_TAG\" = \"$REQUESTED_TAG\"",
        "REQUESTED_TAG:",
        "pull-requests: read",
        "checks: read",
    ] {
        assert!(
            authority.contains(required),
            "plan authority workflow is missing {required}"
        );
    }

    let recheck = fs::read_to_string(root.join(".github/workflows/release-main-recheck.yml"))
        .expect("main recheck workflow must be readable");
    assert!(recheck.contains("actions: read"));
    assert!(recheck.contains("scripts/release.py verify-publication"));
    assert!(recheck.contains("scripts/release.py verify-checks"));
    assert!(
        recheck.contains("actions/workflows/codeflow-ci.yml/runs?branch=main&event=push&head_sha=")
    );
    assert!(recheck.contains("--runs-state /tmp/authority-runs.json"));
    // TSK-106: the release-state check runs in its own workflow, whose
    // latest main-push run publication reads as well.
    assert!(recheck
        .contains("actions/workflows/codeflow-release.yml/runs?branch=main&event=push&head_sha="));
    assert!(recheck.contains("--runs-state /tmp/authority-release-runs.json"));
    assert!(recheck.contains("--source \"$GITHUB_SHA\""));
    assert!(recheck.contains("--main-source \"$main_sha\""));
    assert!(recheck.contains("GITHUB_TRIGGERING_ACTOR"));
    assert!(recheck.contains("collaborators/${login}/permission"));
    // sathyassn/codeflow#14: every platform build is read for a clean
    // version line of the release commit before the host job publishes, on
    // dry runs too, so this step carries no dry-run condition.
    let clean = recheck
        .split("- name: Download the platform builds\n")
        .nth(1)
        .and_then(|rest| rest.split("- name: Recheck selected main").next())
        .expect("main recheck downloads the platform builds before its recheck");
    assert!(clean.contains("pattern: artifacts-build-local-*"));
    assert!(clean.contains(
        "scripts/release.py verify-clean-builds --artifacts-dir /tmp/release-builds --source \"$GITHUB_SHA\""
    ));
    assert!(!clean.contains("if:"));

    // The cargo-dist plan carries the release notes, and 3.0.0's plan
    // exceeded the 128 KiB limit on one variable, so no workflow passes the
    // whole plan into the environment; each takes only the fields it reads.
    for entry in fs::read_dir(root.join(".github/workflows")).expect("workflows must be readable") {
        let path = entry.expect("workflow entry").path();
        let text = fs::read_to_string(&path).expect("workflow must be readable");
        assert!(
            !text.contains(": ${{ inputs.plan }}"),
            "{} passes the whole plan into the environment",
            path.display()
        );
    }

    let published = fs::read_to_string(root.join(".github/workflows/release-post-announce.yml"))
        .expect("post-announce verification workflow must be readable");
    for required in [
        "scripts/release.py verify-published-assets",
        "pattern: artifacts-*",
        "merge-multiple: true",
        "releases/tags/${tag}",
        "commits/tags/${tag}",
        "contents: read",
    ] {
        assert!(
            published.contains(required),
            "post-announce workflow is missing {required}"
        );
    }
}

/// Run a command in the workspace, whatever repository a calling hook points
/// git at.
fn workspace_output(program: &str, args: &[&str]) -> String {
    let output = std::process::Command::new(program)
        .args(args)
        .current_dir(workspace_root())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("command must run");
    assert!(output.status.success(), "{program} {args:?} failed");
    String::from_utf8(output.stdout).expect("output must be UTF-8")
}

#[test]
fn source_archive_leaves_out_only_retained_evidence() {
    // cargo-dist builds source.tar.gz with `git archive`. Compare what it
    // actually writes with the tracked files: only the retained evidence may
    // be left out, so an exclusion on any parent folder also fails here.
    let archive = std::env::temp_dir().join(format!("codeflow-source-{}.tar", std::process::id()));
    let path = archive.to_str().expect("temporary path must be UTF-8");
    workspace_output(
        "git",
        &[
            "archive",
            "--worktree-attributes",
            "--format=tar",
            "-o",
            path,
            "HEAD",
        ],
    );
    let listed = workspace_output("tar", &["-tf", path]);
    fs::remove_file(&archive).expect("temporary archive must be removable");
    let archived = listed
        .lines()
        .filter(|entry| !entry.ends_with('/'))
        .collect::<std::collections::BTreeSet<_>>();
    // Raw names: git quotes unusual names in its display form.
    let tracked = workspace_output(
        "git",
        &[
            "-c",
            "core.quotePath=false",
            "ls-tree",
            "-r",
            "-z",
            "--name-only",
            "HEAD",
        ],
    );
    let mut evidence = 0;
    let mut wrong = Vec::new();
    for file in tracked.split('\0').filter(|name| !name.is_empty()) {
        let is_evidence = file.starts_with("docs/verification/");
        evidence += usize::from(is_evidence);
        if is_evidence == archived.contains(file) {
            wrong.push(file);
        }
    }
    assert!(
        evidence > 0,
        "docs/verification/ must hold the retained evidence"
    );
    assert!(
        wrong.is_empty(),
        "the source archive must leave out exactly docs/verification/: {wrong:?}"
    );
}

#[test]
fn strict_repository_gate_installs_its_declared_coverage_tool() {
    let workflow = fs::read_to_string(workspace_root().join(".github/workflows/codeflow-ci.yml"))
        .expect("repository CI workflow must be readable");
    // TSK-106: the release jobs, and the body-edit trigger they need, run
    // from the CodeFlow-only release workflow with a pinned checkout.
    assert!(!workflow.contains("release-impact:"));
    let release =
        fs::read_to_string(workspace_root().join(".github/workflows/codeflow-release.yml"))
            .expect("release workflow must be readable");
    assert!(release.contains("types: [opened, synchronize, reopened, edited]"));
    let release_impact = release
        .split("  release-impact:\n")
        .nth(1)
        .expect("release-impact job must exist")
        .split("\n  release-state:")
        .next()
        .expect("release-impact must precede release-state");
    assert!(release_impact.contains("actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803"));
    let gates = workflow
        .split("\n  rust:")
        .next()
        .expect("codeflow gates job must precede the Rust job");
    // TSK-184: one pinned install step supplies nextest and llvm-cov.
    let install = gates
        .find("cargo-llvm-cov@")
        .expect("strict gate must install cargo-llvm-cov on a clean runner");
    let strict = gates
        .find("run: codeflow test --mode full --strict")
        .expect("strict aggregate gate must remain enabled");

    assert!(
        install < strict,
        "cargo-llvm-cov must be available before the strict aggregate gate"
    );
}

#[test]
fn windows_cross_check_lints_target_specific_code() {
    let config = fs::read_to_string(workspace_root().join(".cargo/config.toml"))
        .expect("Cargo configuration must be readable");

    assert!(
        config.contains(
            "cross-check-windows = \"xwin clippy --workspace --all-targets --target \
             x86_64-pc-windows-msvc -- -D warnings\""
        ),
        "the host-agnostic Windows check must lint target-specific code, not only compile it"
    );
}

/// TSK-203: the published binary reported `dirty=true`. The build script
/// marks a build dirty when `git status` lists any untracked file, and the
/// release job writes files into its checkout before `dist build` compiles:
/// it redirects the manifest to `dist-manifest.json` at the root and downloads
/// the plan into `target/distrib/`. Every path the job writes must be ignored,
/// judged by the build's own dirty check on a repository carrying this
/// `.gitignore` and nothing from the host's git configuration.
#[test]
fn the_release_build_writes_only_ignored_paths_so_its_binary_is_clean() {
    let root = workspace_root();
    let workflow: serde_yaml::Value = serde_yaml::from_str(
        &fs::read_to_string(root.join(".github/workflows/release.yml"))
            .expect("release workflow must be readable"),
    )
    .expect("release workflow must be YAML");
    let job = &workflow["jobs"]["build-local-artifacts"];
    let expression = regex::Regex::new(r"\$\{\{[^}]*\}\}").unwrap();
    let redirect = regex::Regex::new(r#">>?\s*"?([^\s"]+)"#).unwrap();
    let mut written: Vec<String> = Vec::new();
    if let Some(manifest) = job["env"]["BUILD_MANIFEST_NAME"].as_str() {
        written.push(manifest.to_string());
    }
    for step in job["steps"].as_sequence().expect("release build steps") {
        let uses = step["uses"].as_str().unwrap_or_default();
        if uses.starts_with("actions/download-artifact@") {
            written.push(step["with"]["path"].as_str().unwrap_or(".").to_string());
        }
        for capture in redirect.captures_iter(step["run"].as_str().unwrap_or_default()) {
            // Paths in the checkout; a variable, descriptor or /dev/null is not.
            let path = &capture[1];
            if !path.starts_with(['$', '&', '/']) {
                written.push(path.to_string());
            }
        }
    }
    let written: Vec<String> = written
        .iter()
        .map(|path| {
            let path = expression.replace_all(path, "x");
            // A directory the job fills gets a file inside it.
            match path.strip_suffix('/') {
                Some(directory) => format!("{directory}/downloaded"),
                None => path.into_owned(),
            }
        })
        .collect();
    assert!(
        written.iter().any(|path| path == "dist-manifest.json"),
        "the release build's root manifest must be among the written paths: {written:?}"
    );

    let repo = tempfile::tempdir().unwrap();
    let make = || {
        let mut command = std::process::Command::new("git");
        command
            .env("GIT_CONFIG_GLOBAL", repo.path().join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.test")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.test");
        command
    };
    fs::copy(root.join(".gitignore"), repo.path().join(".gitignore")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["add", ".gitignore"],
        &["commit", "-qm", "base"],
    ] {
        let out = make().args(args).current_dir(repo.path()).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    for path in &written {
        let file = repo.path().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "written by the release job").unwrap();
    }
    let (_, dirty, _) = codeflow_core::hooks::source_identity::revision(repo.path(), None, &make);
    assert_eq!(
        dirty, "false",
        "the release job writes a path .gitignore does not cover: {written:?}"
    );
}
