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
    for required in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
    ] {
        assert!(
            targets.contains(&required),
            "missing release target: {required}"
        );
    }

    let installers = dist
        .get("installers")
        .and_then(toml::Value::as_array)
        .expect("dist installers must be an array")
        .iter()
        .map(|value| value.as_str().expect("installer must be a string"))
        .collect::<Vec<_>>();
    assert!(installers.contains(&"shell"));
    assert!(installers.contains(&"powershell"));

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
    assert!(host.contains("needs.custom-release-plan-authority.result == 'skipped'"));
    assert!(host.contains("needs.custom-release-plan-authority.result == 'success'"));
    assert!(!host.contains("result == 'failure'"));
    assert!(!host.contains("result == 'cancelled'"));
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
fn release_candidate_and_authorization_workflows_keep_human_exact_head_boundary() {
    let root = workspace_root();
    let candidate = fs::read_to_string(root.join(".github/workflows/release-candidate.yml"))
        .expect("candidate workflow must be readable");
    for required in [
        "group: codeflow-release-candidate",
        "refs/heads/chore/release-codeflow",
        "cargo install git-cliff --version 2.13.1 --locked",
        "cargo install cargo-dist --version 0.32.0 --locked",
        "persist-credentials: false",
        "GIT_CONFIG_COUNT=1",
        "GIT_CONFIG_KEY_0=http.https://github.com/.extraheader",
        "unset auth GIT_CONFIG_COUNT GIT_CONFIG_KEY_0 GIT_CONFIG_VALUE_0",
        "python3 scripts/release.py guard-refresh",
        "python3 scripts/release.py finalize",
        "peter-evans/create-pull-request@5f6978faf089d4d20b00c7766989d076bb2fc7f1",
    ] {
        assert!(
            candidate.contains(required),
            "candidate workflow is missing {required}"
        );
    }

    let authorize = fs::read_to_string(root.join(".github/workflows/release-authorize.yml"))
        .expect("authorization workflow must be readable");
    for required in [
        "pull_request_target:",
        "github.event.pull_request.merged == true",
        "github.event.pull_request.head.ref == 'chore/release-codeflow'",
        "collaborators/${login}/permission",
        "select(.user.type == \"User\")",
        ".parents[1].sha == $head",
        "scripts/release.py authorize-event",
        "gh workflow run release.yml",
        "-f tag=",
        "git/ref/heads/chore/release-codeflow",
        "refs/heads/chore/release-codeflow",
    ] {
        assert!(
            authorize.contains(required),
            "authorization workflow is missing {required}"
        );
    }

    let authority = fs::read_to_string(root.join(".github/workflows/release-plan-authority.yml"))
        .expect("plan authority workflow must be readable");
    for required in [
        "scripts/release.py verify-dispatch",
        "scripts/release.py verify-review",
        "scripts/release.py verify-host-state",
        "scripts/release.py release-notes",
        "gh release create",
        "--draft",
        "github.event.inputs.tag == 'dry-run'",
        "github.event.inputs.tag != 'dry-run'",
        "plan_tag=",
        "REQUESTED_TAG:",
        "commits/${candidate}/pulls",
        "pulls/${pr_number}",
        "collaborators/${login}/permission",
        "select(.user.type == \"User\")",
        "GIT_CONFIG_COUNT=1",
        "unset auth GIT_CONFIG_COUNT GIT_CONFIG_KEY_0 GIT_CONFIG_VALUE_0",
    ] {
        assert!(
            authority.contains(required),
            "plan authority workflow is missing {required}"
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
    assert!(workflow.contains("types: [opened, synchronize, reopened, edited]"));
    let release_impact = workflow
        .split("  release-impact:\n")
        .nth(1)
        .expect("release-impact job must exist")
        .split("\n  gates:")
        .next()
        .expect("release-impact must precede gates");
    assert!(release_impact.contains("actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803"));
    let gates = workflow
        .split("\n  rust:")
        .next()
        .expect("codeflow gates job must precede the Rust job");
    let install = gates
        .find("uses: taiki-e/install-action@cargo-llvm-cov")
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
