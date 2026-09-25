//! Real-binary checks in fictional consumer repositories using shipped assets.
use super::*;
use codeflow_core::scaffold::{
    self, AssetSource, DirSource, InitAnswers, InitOptions, Tier, UpdateOptions,
};
use serde_json::{json, Value};

const BODY: &str = "## Summary\nMake the command easier to use.\n\n## Changes\n- Explain the command's result.\n\n## Testing\nAt fixture HEAD, ran python -m unittest:\n```text\nRan 3 tests\nOK\n```\nCoverage: not measured; this fixture has no coverage tool.\nNot tested: Windows.\n\n## Reviews\nNone: awaiting the maintainer's review.\n\n## Release impact\n- Impact: patch\n- Breaking: no\n- Rationale: Clarify output; this project has no release automation.\n- Migration: none\n- Package: fictional-tool\n";

fn source() -> DirSource {
    DirSource::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"))
}

fn install(root: &Path, tier: Tier, source: &dyn AssetSource) {
    scaffold::init(
        source,
        root,
        &InitOptions {
            tier: Some(tier),
            force: false,
            binary_version: "3.0.0".into(),
            answers: InitAnswers::default(),
        },
    )
    .unwrap();
}

fn update(root: &Path, source: &dyn AssetSource) {
    scaffold::update(
        source,
        root,
        &UpdateOptions {
            force: false,
            binary_version: "3.0.1".into(),
            diff_out: None,
        },
    )
    .unwrap();
}

fn read_policy(root: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(root.join(".codeflow/policy.json")).unwrap()).unwrap()
}

fn write_policy(root: &Path, policy: &Value) {
    std::fs::create_dir_all(root.join(".codeflow")).unwrap();
    std::fs::write(
        root.join(".codeflow/policy.json"),
        serde_json::to_vec_pretty(policy).unwrap(),
    )
    .unwrap();
}

fn assert_clean(root: &Path, body: &str) {
    let output = ci_with_body(root, body);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("git.pr_"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn fresh_all_tiers_ship_template_and_accept_portable_terminal_body() {
    for tier in [Tier::Minimal, Tier::Standard, Tier::Full] {
        let dir = tempfile::tempdir().unwrap();
        repo_with_range(dir.path(), "docs");
        // Non-Rust, no task records or release tooling, ordinary terminal output.
        std::fs::write(dir.path().join("tool.py"), "print('result')\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "fix: clarify result"]);
        install(dir.path(), tier, &source());
        let template = std::fs::read(dir.path().join(".github/pull_request_template.md")).unwrap();
        assert_eq!(
            template,
            source().read("base/ci/pull_request_template.md").unwrap()
        );
        let policy = read_policy(dir.path());
        assert_eq!(policy["git"]["pr_release_impact"], "warn");
        assert_eq!(policy["git"]["pr_breaking_level"], "major");
        assert_eq!(
            policy["git"]["pr_required_sections"],
            json!(["Summary", "Changes", "Reviews", "Release impact"])
        );
        let headings: Vec<_> = std::str::from_utf8(&template)
            .unwrap()
            .lines()
            .filter_map(|line| line.strip_prefix("## "))
            .collect();
        assert_eq!(
            headings,
            ["Summary", "Changes", "Testing", "Reviews", "Release impact"]
        );
        for section in policy["git"]["pr_required_sections"].as_array().unwrap() {
            assert!(headings.contains(&section.as_str().unwrap()));
        }
        assert_clean(
            dir.path(),
            &fill_installed_template(std::str::from_utf8(&template).unwrap()),
        );
        update(dir.path(), &source());
        update(dir.path(), &source());
        assert_eq!(
            std::fs::read(dir.path().join(".github/pull_request_template.md")).unwrap(),
            template
        );
        assert_eq!(read_policy(dir.path()), policy);
        assert_clean(dir.path(), BODY);
    }
}

/// The prior shipped defaults, with only the two new keys absent. The existing
/// template is intentionally unchanged here: its prose belongs to another task.
struct PreviousAssets;
impl AssetSource for PreviousAssets {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let bytes = source().read(path)?;
        if path == "base/scaffold-manifest.toml" {
            let text = String::from_utf8(bytes).unwrap();
            let (before, template) = text
                .split_once("src = \"ci/pull_request_template.md\"")
                .unwrap();
            return Some(
                format!(
                    "{before}src = \"ci/pull_request_template.md\"{}",
                    template.replacen(
                        "tiers = [\"minimal\", \"standard\", \"full\"]",
                        "tiers = [\"standard\", \"full\"]",
                        1
                    )
                )
                .into_bytes(),
            );
        }
        if path != "base/policy.json" {
            return Some(bytes);
        }
        let mut policy: Value = serde_json::from_slice(&bytes).unwrap();
        let git = policy["git"].as_object_mut().unwrap();
        git.remove("pr_release_impact");
        git.remove("pr_breaking_level");
        git.insert("pr_required_sections".into(), json!(["Summary", "Changes"]));
        Some(serde_json::to_vec_pretty(&policy).unwrap())
    }
}

#[test]
fn update_adds_keys_preserves_custom_and_default_equal_values_and_template() {
    for custom in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        repo_with_range(dir.path(), "code");
        install(dir.path(), Tier::Minimal, &PreviousAssets);
        let template_path = dir.path().join(".github/pull_request_template.md");
        let custom_template = "## Overview\nProject-owned template instructions.\n## Checks\nProject-specific evidence.\n";
        std::fs::write(&template_path, custom_template).unwrap();
        let mut policy = read_policy(dir.path());
        policy["git"]["pr_sections"] = if custom {
            json!("warn")
        } else {
            json!("block")
        };
        if custom {
            policy["git"]["pr_required_sections"] = json!(["Overview"]);
            policy["git"]["pr_code_sections"] = json!(["Checks"]);
        }
        // An explicitly deleted older key stays deleted too.
        policy["git"]
            .as_object_mut()
            .unwrap()
            .remove("test_gate_on_push");
        write_policy(dir.path(), &policy);
        update(dir.path(), &source());
        let after = read_policy(dir.path());
        let mut expected = policy;
        expected["git"]["pr_release_impact"] = json!("warn");
        expected["git"]["pr_breaking_level"] = json!("major");
        assert_eq!(after, expected);
        assert_eq!(
            std::fs::read_to_string(&template_path).unwrap(),
            custom_template
        );
        update(dir.path(), &source());
        assert_eq!(read_policy(dir.path()), expected);
        assert_eq!(
            std::fs::read_to_string(&template_path).unwrap(),
            custom_template
        );
        let body = if custom {
            BODY.replace("## Summary", "## Overview")
                .replace("## Testing", "## Checks")
        } else {
            BODY.into()
        };
        assert_clean(dir.path(), &body);
    }
}

#[test]
fn updates_preserve_explicit_release_defaults_off_warn_and_pre_one_mapping() {
    for (setting, mapping) in [("off", "major"), ("warn", "major"), ("block", "minor")] {
        let dir = tempfile::tempdir().unwrap();
        repo_with_range(dir.path(), "code");
        install(dir.path(), Tier::Minimal, &PreviousAssets);
        let mut policy = read_policy(dir.path());
        policy["git"]["pr_release_impact"] = json!(setting);
        policy["git"]["pr_breaking_level"] = json!(mapping);
        policy["git"]["pr_sections"] = json!("off");
        write_policy(dir.path(), &policy);
        update(dir.path(), &source());
        update(dir.path(), &source());
        assert_eq!(read_policy(dir.path()), policy);
        let invalid = BODY.replace("Impact: patch", "Impact: huge");
        let result = ci_with_body(dir.path(), &invalid);
        assert_eq!(result.status.code(), Some(i32::from(setting == "block")));
        assert_eq!(
            String::from_utf8_lossy(&result.stderr).contains("git.pr_release_impact"),
            setting != "off"
        );
        if mapping == "minor" {
            let pre_one = BODY
                .replace("Impact: patch", "Impact: minor")
                .replace("Breaking: no", "Breaking: yes")
                .replace("Migration: none", "Migration: Rename the removed option.");
            assert_clean(dir.path(), &pre_one);
        }
    }
}

#[test]
fn pr_events_require_a_body_but_push_and_local_runs_can_omit_it() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "docs");
    for (key, value) in [
        ("GITHUB_EVENT_NAME", "pull_request"),
        ("GITHUB_EVENT_NAME", "pull_request_target"),
        ("CI_PIPELINE_SOURCE", "merge_request_event"),
    ] {
        for body in [None, Some(""), Some("  \n<!-- comment -->")] {
            let mut command = codeflow();
            command
                .current_dir(dir.path())
                .args([
                    "ci", "--base", "main", "--head", "HEAD", "--branch", "feat/x",
                ])
                .env(key, value);
            if let Some(body) = body {
                command.env("CODEFLOW_PR_BODY", body);
            }
            let output = command.output().unwrap();
            assert_eq!(output.status.code(), Some(1));
            assert!(String::from_utf8_lossy(&output.stderr).contains("PR body is missing or empty"));
        }
    }
    let output = codeflow()
        .current_dir(dir.path())
        .args(["ci", "--base", "main", "--head", "HEAD"])
        .env("GITHUB_EVENT_NAME", "push")
        .env("CODEFLOW_PR_BODY", "")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PR-body"));
}

#[test]
fn real_commit_range_floors_release_impact_at_project_breaking_level() {
    for message in [
        "feat!: replace option",
        "feat: replace option\n\nBREAKING CHANGE: Rename the old option.",
        "feat: replace option\n\nBREAKING-CHANGE: Rename the old option.",
    ] {
        let dir = tempfile::tempdir().unwrap();
        repo_with_range(dir.path(), "code");
        git(dir.path(), &["commit", "--allow-empty", "-m", message]);
        write_policy(
            dir.path(),
            &json!({"git": {"pr_release_impact": "block", "pr_breaking_level": "minor"}}),
        );
        let output = ci_with_body(dir.path(), BODY);
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains("breaking commit marker requires Impact of at least minor"));
        let above_floor = BODY.replace("Impact: patch", "Impact: MAJOR");
        let rejected = ci_with_body(dir.path(), &above_floor);
        assert_eq!(rejected.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&rejected.stderr)
            .contains("breaking commit marker requires Breaking: yes"));
        let fixed = BODY
            .replace("Impact: patch", "Impact: MAJOR")
            .replace("Breaking: no", "Breaking: YES")
            .replace("Migration: none", "Migration: Rename the old option.");
        assert_clean(dir.path(), &fixed);
    }
}

#[test]
fn invalid_breaking_policy_level_fails_with_key_and_values() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "docs");
    write_policy(dir.path(), &json!({"git": {"pr_breaking_level": "none"}}));
    let output = ci_with_body(dir.path(), BODY);
    assert_eq!(output.status.code(), Some(2));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("git.pr_breaking_level") && error.contains("major"),
        "{error}"
    );
}

/// A future managed prose edit, isolated from the other agent's real template.
struct NextTemplate;
impl AssetSource for NextTemplate {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let bytes = source().read(path)?;
        if path == "base/ci/pull_request_template.md" {
            Some(
                [
                    b"<!-- Fictional new upstream guidance. -->\n".as_slice(),
                    &bytes,
                ]
                .concat(),
            )
        } else {
            Some(bytes)
        }
    }
}

#[test]
fn template_update_merges_custom_prose_and_adds_template_to_old_minimal() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    install(dir.path(), Tier::Minimal, &PreviousAssets);
    let path = dir.path().join(".github/pull_request_template.md");
    assert!(!path.exists(), "old minimal did not ship the template");
    update(dir.path(), &source());
    let original = std::fs::read_to_string(&path).unwrap();
    let custom = "\n## Project notes\nKeep this project's release checklist.\n";
    std::fs::write(&path, format!("{original}{custom}")).unwrap();
    update(dir.path(), &NextTemplate);
    let merged = std::fs::read_to_string(&path).unwrap();
    assert!(merged.starts_with("<!-- Fictional new upstream guidance. -->"));
    assert!(merged.ends_with(custom));
    update(dir.path(), &NextTemplate);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), merged);
    assert!(!path.with_extension("md.new").exists());
    assert_clean(dir.path(), BODY);
}

#[test]
fn rendered_budget_detects_epic_target_and_markdown_failures_reach_cli() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    let body = format!("{BODY}\n{}", "record\n".repeat(45));
    let task = ci_with_body(dir.path(), &body);
    assert!(String::from_utf8_lossy(&task.stderr).contains("aim for 65"));
    let epic = run_in(
        dir.path(),
        &[
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            "integration/epic",
            "--pr-body",
            &body,
        ],
    );
    assert_eq!(epic.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&epic.stderr).contains("rendered rows"));
    for replacement in [
        "```\n## Summary\nexample\n```",
        "<!--\n## Summary\nexample\n-->",
        "## Summary\n<!-- empty -->",
        "## Summary\nfirst\n## Summary\nsecond",
    ] {
        let bad = BODY.replace("## Summary\nMake the command easier to use.", replacement);
        let output = ci_with_body(dir.path(), &bad);
        assert_eq!(output.status.code(), Some(1), "{replacement}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("'## Summary'"));
    }
}

/// Fill only fields that actually exist in the installed template. Never add a
/// missing heading: that would hide a template/default-policy mismatch.
fn fill_installed_template(template: &str) -> String {
    template
        .replace(
            "## Summary\n",
            "## Summary\n\nClarify the command's result.\n",
        )
        .replace("\n-\n", "\n- Explain the result.\n")
        .replace(
            "- Revision and command:",
            "- Revision and command: fixture HEAD, python -m unittest",
        )
        .replace(
            "(paste the real test summary output here)",
            "Ran 3 tests\nOK",
        )
        .replace("- Coverage:", "- Coverage: not measured for this fixture")
        .replace("- New tests:", "- New tests: three command tests")
        .replace("- Not tested:", "- Not tested: Windows")
        .replace("|  |  |  |", "| Maintainer | fixture HEAD | approved |")
        .replace(
            "- Impact: `none | patch | minor | major`",
            "- Impact: patch",
        )
        .replace("- Breaking: `yes | no`", "- Breaking: no")
        .replace(
            "- Rationale:",
            "- Rationale: Clarify output without changing behavior.",
        )
        .replace(
            "- Migration: `none`, steps, or \"see Breaking change\"",
            "- Migration: none",
        )
}

#[test]
fn unfilled_installed_templates_fail_only_for_empty_required_sections() {
    for tier in [Tier::Minimal, Tier::Standard, Tier::Full] {
        let dir = tempfile::tempdir().unwrap();
        repo_with_range(dir.path(), "code");
        install(dir.path(), tier, &source());
        let template =
            std::fs::read_to_string(dir.path().join(".github/pull_request_template.md")).unwrap();
        let output = ci_with_body(dir.path(), &template);
        assert_eq!(output.status.code(), Some(1));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            !error.contains("missing required section"),
            "{tier:?}: {error}"
        );
        let lines: Vec<_> = error.lines().collect();
        let mut blocks = 0;
        for (index, line) in lines.iter().enumerate() {
            if line.contains("policy rule") && line.contains("(block)") {
                blocks += 1;
                assert!(line.contains("git.pr_sections"), "{error}");
                assert!(lines[index + 1].contains("present but empty"), "{error}");
            }
        }
        assert!(blocks > 0, "{error}");
    }
}

#[test]
fn builtin_default_keeps_summary_and_changes_without_explicit_list() {
    // A body written before Reviews and Release impact existed.
    let old_body = BODY.split("## Reviews").next().unwrap();
    for policy in [None, Some(json!({"git": {"pr_sections": "block"}}))] {
        let dir = tempfile::tempdir().unwrap();
        repo_with_range(dir.path(), "code");
        if let Some(policy) = &policy {
            write_policy(dir.path(), policy);
        }
        let output = ci_with_body(dir.path(), old_body);
        let error = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(0), "{policy:?}: {error}");
        assert!(!error.contains("missing required section"), "{error}");
        // The generic release check still advises, at its warn default.
        assert!(error.contains("git.pr_release_impact (warn)"), "{error}");
    }
}

#[test]
fn inline_html_and_unclosed_blocks_keep_every_section() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    for (from, to, unclosed) in [
        (
            "- Explain the command's result.",
            "- Return Vec<String> instead of a joined string.",
            None,
        ),
        (
            "Make the command easier to use.",
            "Replace <path> with the real file.",
            None,
        ),
        (
            "Make the command easier to use.",
            "<p align=\"center\">\n\nMake the command easier to use.",
            Some("p"),
        ),
    ] {
        let body = BODY.replace(from, to);
        let output = ci_with_body(dir.path(), &body);
        let error = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(0), "{body}\n{error}");
        assert!(!error.contains("required section"), "{error}");
        assert!(!error.contains("Release impact section"), "{error}");
        match unclosed {
            None => assert!(!error.contains("git.pr_"), "{error}"),
            Some(tag) => assert!(
                error.contains(&format!("HTML <{tag}> block that never closes")),
                "{error}"
            ),
        }
    }
}

#[test]
fn bitbucket_missing_channel_warns_but_explicit_empty_bodies_fail() {
    let dir = tempfile::tempdir().unwrap();
    repo_with_range(dir.path(), "code");
    let run = |env_body: Option<&str>, flags: &[&str]| {
        let mut command = codeflow();
        command
            .current_dir(dir.path())
            .args([
                "ci", "--base", "main", "--head", "HEAD", "--branch", "feat/x",
            ])
            .env("BITBUCKET_PR_ID", "42")
            .args(flags);
        if let Some(body) = env_body {
            command.env("CODEFLOW_PR_BODY", body);
        }
        command.output().unwrap()
    };
    let missing = run(None, &[]);
    assert_eq!(missing.status.code(), Some(0));
    let warning = String::from_utf8_lossy(&missing.stderr);
    for text in [
        "warning",
        "body was not supplied",
        "checks skipped",
        "CODEFLOW_PR_BODY",
        "--pr-body-file",
    ] {
        assert!(warning.contains(text), "{warning}");
    }
    assert!(!String::from_utf8_lossy(&missing.stdout).contains("PR-body"));
    assert!(warning.contains("skipped: PR-body"), "{warning}");
    let empty_file = dir.path().join("empty-body.md");
    std::fs::write(&empty_file, "").unwrap();
    for output in [
        run(Some(""), &[]),
        run(None, &["--pr-body", ""]),
        run(None, &["--pr-body-file", empty_file.to_str().unwrap()]),
    ] {
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("PR body is missing or empty"));
    }
    let supplied = run(Some(BODY), &[]);
    assert_eq!(supplied.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&supplied.stderr).contains("was not supplied"));
}
