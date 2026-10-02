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
    // 3.0.0 publishes macOS and Linux only (operator decision 2026-10-01).
    // TSK-197 restores `x86_64-pc-windows-msvc` and the PowerShell installer.
    assert_eq!(
        targets,
        [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
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
    assert_eq!(installers, ["shell"]);

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
