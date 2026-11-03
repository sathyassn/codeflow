//! Run the actual workflow predicates, including a retarget to the same SHA.
use std::process::Command;

#[test]
fn capability_hosted_snapshot_refuses_each_changed_input() {
    for workflow in [
        include_str!("../../../../.github/workflows/codeflow-policy.yml"),
        include_str!("../../../../assets/base/ci/codeflow-policy.yml"),
    ] {
        let before_filter = workflow
            .lines()
            .find(|line| line.contains("'.head.sha == $head"))
            .unwrap()
            .trim()
            .trim_end_matches('\\')
            .trim()
            .trim_matches('\'');
        let after_filter = workflow
            .lines()
            .find(|line| line.contains("'def snapshot:"))
            .unwrap()
            .trim()
            .trim_end_matches('\\')
            .trim()
            .trim_matches('\'');
        let dir = tempfile::tempdir().unwrap();
        let snapshot = serde_json::json!({"number":42,"head":{"sha":"head"},"base":{"sha":"base","ref":"main","repo":{"full_name":"owner/repository"}},"body":"The body\nwith literal $() text.","state":"open"});
        let before = dir.path().join("before.json");
        let after = dir.path().join("after.json");
        std::fs::write(&before, snapshot.to_string()).unwrap();
        for pointer in [
            None,
            Some("/head/sha"),
            Some("/base/sha"),
            Some("/base/ref"),
            Some("/body"),
            Some("/state"),
            Some("/number"),
            Some("/base/repo/full_name"),
        ] {
            let mut current = snapshot.clone();
            if let Some(pointer) = pointer {
                *current.pointer_mut(pointer).unwrap() = serde_json::json!("changed");
            }
            std::fs::write(&after, current.to_string()).unwrap();
            let out = Command::new("jq")
                .args(["-e", "--slurpfile", "before"])
                .arg(&before)
                .arg(after_filter)
                .arg(&after)
                .output()
                .unwrap();
            assert_eq!(
                out.status.success(),
                pointer.is_none(),
                "post-check {pointer:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let out = Command::new("jq")
                .args([
                    "-e",
                    "--arg",
                    "head",
                    "head",
                    "--arg",
                    "base",
                    "base",
                    "--arg",
                    "target",
                    "main",
                    "--arg",
                    "number",
                    "42",
                    "--arg",
                    "repository",
                    "owner/repository",
                    before_filter,
                ])
                .arg(&after)
                .output()
                .unwrap();
            assert_eq!(
                out.status.success(),
                matches!(pointer, None | Some("/body")),
                "pre-check {pointer:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}
