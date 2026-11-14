//! TSK-450: style findings are advisory while semantic checks still block.
use codeflow_core::hooks::{
    git_hook,
    policy::{GitPolicy, PolicyLevel},
};

#[test]
fn style_defaults_warn_without_refusing() {
    let cases = [
        (
            format!("feat: {}", "x".repeat(60)),
            "git.commit_description",
        ),
        ("feat: x\n\nA prose body.".into(), "git.commit_body"),
        (
            "feat: x\n\n- one\n- two\n- three\n- four".into(),
            "git.commit_body",
        ),
        ("feat: x\n\nRefs: TASK-1".into(), "git.commit_body"),
        (
            "feat: x\n\nA range 1\u{2013}3 and a pause\u{2014}here".into(),
            "git.policy_characters",
        ),
    ];
    for (message, rule) in cases {
        let report = git_hook::commit_msg(&GitPolicy::default(), &message, false);
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule == rule && v.level == PolicyLevel::Warn));
        assert!(
            !report
                .violations
                .iter()
                .any(|v| v.level == PolicyLevel::Block),
            "{message}"
        );
    }
}

#[test]
fn semantic_defaults_still_block() {
    for (message, rule) in [
        ("oops: x".to_string(), "git.commit_format"),
        (
            "feat: x\n\nbreaking change: incompatible".into(),
            "git.commit_format",
        ),
        (format!("feat: {}", "x".repeat(73)), "git.commit_format"),
        (
            "feat: x\n\nCo-Authored-By: Claude <bot@example.com>".into(),
            "git.ai_attribution",
        ),
        ("feat: launch \u{1f680}".into(), "git.commit_emoji"),
    ] {
        let report = git_hook::commit_msg(&GitPolicy::default(), &message, false);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.rule == rule && v.level == PolicyLevel::Block),
            "{message}"
        );
    }
}

#[test]
fn attribution_refusal_names_the_host_settings() {
    let report = git_hook::commit_msg(
        &GitPolicy::default(),
        "feat: x\n\nCo-Authored-By: Claude <bot@example.com>",
        false,
    );
    let finding = report
        .violations
        .iter()
        .find(|v| v.rule == "git.ai_attribution")
        .unwrap();
    let remedy = finding.remedy.to_string();
    assert!(remedy.contains("includeCoAuthoredBy"));
    assert!(remedy.contains("attribution"));
    for source in [
        include_str!("../../../assets/base/settings/default.json"),
        include_str!("../../../assets/base/settings/acceptEdits.json"),
        include_str!("../../../assets/base/settings/bypass-sandboxed.json"),
    ] {
        let settings: serde_json::Value = serde_json::from_str(source).unwrap();
        assert_eq!(settings["includeCoAuthoredBy"], false);
        assert_eq!(settings["attribution"]["commit"], "");
        assert_eq!(settings["attribution"]["pr"], "");
    }
}

#[test]
fn retired_controls_load_without_entering_the_effective_schema() {
    use codeflow_core::hooks::{policy::Policy, policy_schema};
    let released = include_str!("fixtures/policy-last-release.json");
    policy_schema::validate_policy_str(released).unwrap();
    let policy: Policy = serde_json::from_str(released).unwrap();
    let effective = serde_json::to_value(policy).unwrap();
    let raw: serde_json::Value = serde_json::from_str(released).unwrap();
    let paths = policy_schema::unsupported_control_paths(&raw);
    assert_eq!(paths.len(), 7);
    for path in paths {
        assert!(policy_schema::lookup(&effective, path).is_none());
        assert!(policy_schema::schema().iter().all(|spec| spec.path != path));
    }
    assert_eq!(policy_schema::unsupported_control_warnings(&raw).len(), 7);
}

#[test]
fn required_footers_block_independently_of_body_style() {
    for body in [PolicyLevel::Warn, PolicyLevel::Off] {
        let policy = GitPolicy {
            commit_body: body,
            commit_required_footers: vec!["Signed-off-by".into()],
            ..GitPolicy::default()
        };
        let missing = git_hook::commit_msg(&policy, "feat: x", false);
        assert!(missing
            .violations
            .iter()
            .any(|v| v.rule == "git.commit_footer_required" && v.level == PolicyLevel::Block));
        let present = git_hook::commit_msg(
            &policy,
            "feat: x\n\nSigned-off-by: Example <example@example.com>",
            false,
        );
        assert!(!present
            .violations
            .iter()
            .any(|v| v.level == PolicyLevel::Block));
    }
}
