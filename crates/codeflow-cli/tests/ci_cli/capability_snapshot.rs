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

#[test]
fn capability_hosted_moved_target_keeps_the_evaluated_base() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let base = repo
        .commit(
            Some("refs/heads/target"),
            &signature,
            &signature,
            "base",
            &tree,
            &[],
        )
        .unwrap();
    let parent = repo.find_commit(base).unwrap();
    let tip = repo
        .commit(
            Some("refs/heads/target"),
            &signature,
            &signature,
            "advance target",
            &tree,
            &[&parent],
        )
        .unwrap();
    assert!(repo.graph_descendant_of(tip, base).unwrap());
    assert_ne!(base, tip);
    let base = base.to_string();
    let tip = tip.to_string();
    let snapshot = serde_json::json!({"number":42,"head":{"sha":"head"},"base":{"sha":base,"ref":"target","repo":{"full_name":"owner/repository"}},"body":"unchanged body","state":"open"});
    let before = dir.path().join("before.json");
    let after = dir.path().join("after.json");
    std::fs::write(&before, snapshot.to_string()).unwrap();
    for workflow in [
        include_str!("../../../../.github/workflows/codeflow-policy.yml"),
        include_str!("../../../../assets/base/ci/codeflow-policy.yml"),
    ] {
        // The run must keep its event base even after the actual target advances.
        assert!(workflow.contains("BASE_SHA: ${{ github.event.pull_request.base.sha }}"));
        assert!(workflow.contains("codeflow ci --base \"$BASE_SHA\" --head \"$HEAD_SHA\""));
        let filter = workflow
            .lines()
            .find(|line| line.contains("'def snapshot:"))
            .unwrap()
            .trim()
            .trim_end_matches('\\')
            .trim()
            .trim_matches('\'');
        for api_tracks_tip in [false, true] {
            let mut current = snapshot.clone();
            if api_tracks_tip {
                current["base"]["sha"] = serde_json::json!(tip);
            }
            std::fs::write(&after, current.to_string()).unwrap();
            let out = Command::new("jq")
                .args(["-e", "--slurpfile", "before"])
                .arg(&before)
                .arg(filter)
                .arg(&after)
                .output()
                .unwrap();
            // Observed API behavior: branch-only movement keeps the snapshot.
            // If the API starts tracking the tip, equality fails closed. Any
            // future relaxation must prove base ancestry, not just drop equality.
            assert_eq!(out.status.success(), !api_tracks_tip);
        }
    }
}
