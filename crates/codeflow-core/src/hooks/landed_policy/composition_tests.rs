use super::*;
use crate::hooks::policy::{GitPolicy, SecretScanEngine};
use serde_json::json;

fn git(value: serde_json::Value) -> GitPolicy {
    serde_json::from_value(value).unwrap()
}

#[test]
fn hook_floor_each_level_uses_the_stricter_input() {
    let fields = [
        "commit_to_protected",
        "push_to_protected",
        "force_push_protected",
        "force_push_unprotected",
        "delete_protected",
        "merge_to_protected",
        "root_checkout_commits",
        "commit_format",
        "commit_body",
        "commit_ticket_required",
        "commit_ticket_merges",
        "ai_attribution",
        "commit_emoji",
        "policy_characters",
        "branch_naming",
        "secret_scan",
        "conflict_markers",
        "test_gate_on_push",
    ];
    let levels = ["off", "allow", "warn", "block"];
    for field in fields {
        for (a, landed) in levels.iter().enumerate() {
            for (b, working) in levels.iter().enumerate() {
                let mut authority = json!({});
                authority[field] = json!(landed);
                let mut local = json!({});
                local[field] = json!(working);
                let judged =
                    serde_json::to_value(compose_git(&git(authority), &git(local)).unwrap())
                        .unwrap();
                assert_eq!(
                    judged[field],
                    levels[a.max(b)],
                    "{field}, {landed}, {working}"
                );
            }
        }
    }
}

#[test]
fn hook_floor_lists_limits_and_unordered_fields() {
    let landed = git(
        json!({"protected_branches":["main"], "branch_prefixes":["task/"],
        "commit_types":["fix", "feat"], "commit_required_footers":["Signed-off-by"],
        "commit_footer_tokens":["Refs", "Acked-by"], "commit_ticket_keys":["Refs"],
        "commit_ticket_pattern":"^TSK-[0-9]+$", "root_branch":"main",
        "commit_desc_max_len":50, "commit_subject_max_len":72,
        "commit_body_max_bullets":3, "commit_body_bullet_max_len":72,
        "breaking_watch_paths":["api/**"], "push_budget_seconds":60}),
    );
    let local = git(
        json!({"protected_branches":["release/*"], "branch_prefixes":["task/", "loose/"],
        "commit_types":["fix", "loose"], "commit_required_footers":[],
        "commit_footer_tokens":["Refs", "New"], "commit_ticket_keys":["Refs", "Closes"],
        "commit_ticket_pattern":".*", "root_branch":"task/escape",
        "commit_desc_max_len":80, "commit_subject_max_len":60,
        "commit_body_max_bullets":9, "commit_body_bullet_max_len":50,
        "breaking_watch_paths":["cli/**"], "push_budget_seconds":600}),
    );
    let judged = compose_git(&landed, &local).unwrap();
    assert_eq!(judged.protected_branches, ["main", "release/*"]);
    assert_eq!(judged.branch_prefixes, ["task/"]);
    assert_eq!(judged.commit_types, ["fix"]);
    assert_eq!(judged.commit_ticket_keys, ["Refs"]);
    assert_eq!(judged.commit_required_footers, ["Signed-off-by"]);
    assert_eq!(judged.allowed_footer_tokens(), ["Refs", "Signed-off-by"]);
    assert_eq!(judged.breaking_watch_paths, ["api/**", "cli/**"]);
    assert_eq!(
        (
            judged.commit_desc_max_len,
            judged.commit_subject_max_len,
            judged.commit_body_max_bullets,
            judged.commit_body_bullet_max_len
        ),
        (50, 60, 3, 50)
    );
    assert_eq!(judged.commit_ticket_pattern, landed.commit_ticket_pattern);
    assert_eq!(judged.root_branch, "main");
    assert_eq!(judged.push_budget_seconds, 60);
}

/// A candidate name is an allowed branch name (TSK-1351), so the
/// checkout's candidate prefix cannot widen what the landed policy admits.
#[test]
fn hook_floor_candidate_naming_keeps_the_landed_value() {
    let landed = git(json!({"branch_prefixes":["task/"]}));
    let local = git(
        json!({"branch_prefixes":["task/"], "candidate_prefix":"loose/",
        "candidate_search_depth":5000}),
    );
    let judged = compose_git(&landed, &local).unwrap();
    assert_eq!(judged.candidate_prefix, landed.candidate_prefix);
    assert_eq!(judged.candidate_search_depth, landed.candidate_search_depth);
    assert!(!judged.branch_name_ok("loose/x"));
    assert!(judged.branch_name_ok("candidate/x"));
}

#[test]
fn hook_floor_candidate_naming_keeps_landed_eligibility() {
    let landed = git(json!({"protected_branches":["main"],
        "branch_prefixes":["candidate/work/"]}));
    let working = git(json!({"protected_branches":["main"], "branch_prefixes":[]}));
    assert!(!landed.branch_name_ok("candidate/free"));
    assert!(working.branch_name_ok("candidate/free"));
    let judged = compose_git(&landed, &working).unwrap();
    assert!(!judged.is_candidate_branch("candidate/free"));
    assert!(!judged.branch_name_ok("candidate/free"));
    let nested = compose_git(&judged, &working).unwrap();
    assert!(!nested.branch_name_ok("candidate/free"));
}

/// Protection is a classification: the union only adds prohibitions, and
/// what grants trust or exempts a branch reads the landed list (TSK-1078).
#[test]
fn hook_floor_keeps_the_landed_protected_classification_beside_the_union() {
    let landed = git(json!({"protected_branches":["main"], "branch_prefixes":["task/"]}));
    let local = git(
        json!({"protected_branches":["main", "integration/EPC-999-unbound", "loose/*"],
        "branch_prefixes":["task/"]}),
    );
    let judged = compose_git(&landed, &local).unwrap();
    assert_eq!(
        judged.protected_branches,
        ["main", "integration/EPC-999-unbound", "loose/*"]
    );
    assert_eq!(judged.trusted_protected_branches(), ["main"]);
    // The union protects the added names, so prohibitions apply to them.
    assert!(judged.branch_is_protected("integration/EPC-999-unbound"));
    assert!(judged.branch_is_protected("loose/check"));
    // The landed classification does not, so no exemption or grant follows.
    assert!(!judged.branch_is_landed_protected("integration/EPC-999-unbound"));
    assert!(!judged.branch_is_landed_protected("loose/check"));
    assert!(judged.branch_is_landed_protected("main"));
    assert!(!judged.branch_name_ok("loose/check"));
    assert!(judged.branch_name_ok("main"));
    // Outside composition the two classifications are the same.
    let plain = git(json!({"protected_branches":["main", "loose/*"]}));
    assert_eq!(plain.trusted_protected_branches(), ["main", "loose/*"]);
    assert!(plain.branch_is_landed_protected("loose/check"));
}

#[test]
fn hook_floor_the_stricter_level_is_the_higher_one() {
    use crate::hooks::policy::PolicyLevel::{Allow, Block, Off, Warn};
    for (a, b, stricter) in [
        (Off, Block, Block),
        (Block, Off, Block),
        (Warn, Allow, Warn),
        (Allow, Warn, Warn),
        (Off, Allow, Allow),
        (Warn, Warn, Warn),
    ] {
        assert_eq!(a.stricter(b), stricter, "{a} and {b}");
    }
}

#[test]
fn hook_floor_empty_ticket_intersection_refuses_instead_of_disabling() {
    let authority = git(json!({"commit_ticket_keys":["Refs"]}));
    for keys in [json!([]), json!(["Closes"])] {
        let error = compose_git(&authority, &git(json!({"commit_ticket_keys":keys}))).unwrap_err();
        assert!(error.contains("git.commit_ticket_keys"), "{error}");
    }
}

#[test]
fn hook_floor_new_required_footer_cannot_expand_landed_allowances() {
    assert!(compose_git(
        &GitPolicy::default(),
        &git(json!({"commit_required_footers":["New"]}))
    )
    .is_err());
    let authority = git(json!({"commit_footer_tokens":["Signed-off-by"]}));
    let local = git(json!({"commit_required_footers":["Signed-off-by"]}));
    assert_eq!(
        compose_git(&authority, &local)
            .unwrap()
            .commit_required_footers,
        ["Signed-off-by"]
    );
}

#[test]
fn hook_floor_cannot_remove_either_requested_scan_engine() {
    for (a, b) in [
        (SecretScanEngine::Builtin, SecretScanEngine::Gitleaks),
        (SecretScanEngine::Gitleaks, SecretScanEngine::Builtin),
    ] {
        let landed = GitPolicy {
            secret_scan_engine: a,
            ..GitPolicy::default()
        };
        let local = GitPolicy {
            secret_scan_engine: b,
            ..GitPolicy::default()
        };
        assert_eq!(
            compose_git(&landed, &local).unwrap().secret_scan_engine,
            SecretScanEngine::Gitleaks
        );
    }
}

#[test]
fn hook_floor_bootstrap_still_suspends_checks_except_scan() {
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
    std::fs::write(
        dir.path().join(".codeflow/project.toml"),
        "policy_armed = false\n",
    )
    .unwrap();
    let (policy, armed) = load_for_hooks(dir.path()).unwrap();
    assert!(!armed);
    assert_eq!(policy.git.secret_scan, PolicyLevel::Block);
    assert_eq!(policy.git.conflict_markers, PolicyLevel::Off);
    assert_eq!(policy.git.commit_format, PolicyLevel::Off);
    assert_eq!(policy.git.commit_to_protected, PolicyLevel::Off);
}

#[test]
fn hook_floor_unreadable_authority_object_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    git2::Repository::init_bare(remote.path()).unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    repo.remote("origin", remote.path().to_str().unwrap())
        .unwrap();
    let tracking = repo.path().join("refs/remotes/origin");
    std::fs::create_dir_all(&tracking).unwrap();
    std::fs::write(tracking.join("HEAD"), "ref: refs/remotes/origin/main\n").unwrap();
    std::fs::write(tracking.join("main"), format!("{}\n", "1".repeat(40))).unwrap();
    assert!(load_for_hooks(dir.path()).is_err());
}

#[test]
fn hook_floor_each_numeric_limit_only_decreases() {
    for field in [
        "commit_desc_max_len",
        "commit_subject_max_len",
        "commit_body_max_bullets",
        "commit_body_bullet_max_len",
    ] {
        for a in [0, 1, 50, 100] {
            for b in [0, 1, 50, 100] {
                let mut landed = json!({});
                let mut working = json!({});
                landed[field] = json!(a);
                working[field] = json!(b);
                let judged =
                    serde_json::to_value(compose_git(&git(landed), &git(working)).unwrap())
                        .unwrap();
                assert_eq!(judged[field], a.min(b));
            }
        }
    }
}

#[test]
fn hook_floor_removed_ticket_keys_do_not_survive_as_generic_footers() {
    let landed = git(json!({"commit_ticket_keys":["Refs", "Closes"]}));
    let working = git(json!({"commit_ticket_keys":["Refs"], "commit_footer_tokens":["Closes"]}));
    let judged = compose_git(&landed, &working).unwrap();
    assert_eq!(judged.commit_ticket_keys, ["Refs"]);
    assert!(!judged.allowed_footer_tokens().contains(&"Closes".into()));
}

#[test]
fn hook_floor_patterned_ticket_keys_cannot_lose_validation() {
    let landed = git(
        json!({"commit_ticket_keys":["Refs", "Closes"], "commit_ticket_pattern":"^TSK-[0-9]+$"}),
    );
    let working = git(json!({"commit_ticket_keys":["Refs"]}));
    assert!(compose_git(&landed, &working).is_err());
}

#[test]
fn hook_floor_required_ticket_footer_cannot_become_unchecked() {
    let landed =
        git(json!({"commit_ticket_keys":["Refs", "Closes"], "commit_required_footers":["Closes"]}));
    let working = git(json!({"commit_ticket_keys":["Refs"]}));
    assert!(compose_git(&landed, &working).is_err());
}

#[test]
fn hook_floor_both_commit_hooks_scan_at_the_composed_level() {
    for (landed_level, level, active) in [
        ("block", "off", true),
        ("block", "allow", true),
        ("off", "off", false),
        ("allow", "allow", false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        repo.set_head("refs/heads/task/check").unwrap();
        std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
        let path = dir.path().join(".codeflow/policy.json");
        let landed = git(
            json!({"root_checkout_commits":"off", "secret_scan":landed_level, "conflict_markers":landed_level}),
        );
        std::fs::write(&path, json!({"git":landed}).to_string()).unwrap();
        let mut index = repo.index().unwrap();
        index
            .add_path(std::path::Path::new(".codeflow/policy.json"))
            .unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("Test", "test@example.invalid").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "test: seed", &tree, &[])
            .unwrap();
        let working = git(
            json!({"root_checkout_commits":"off", "secret_scan":level, "conflict_markers":level}),
        );
        std::fs::write(&path, json!({"git":working}).to_string()).unwrap();
        std::fs::write(
            dir.path().join("payload.txt"),
            format!(
                "key = {}{}\n<<<<<<< ours\na\n=======\nb\n>>>>>>> theirs\n",
                "AKIA", "Q3ZWJ2X5LMN8RT4V"
            ),
        )
        .unwrap();
        index.add_path(std::path::Path::new("payload.txt")).unwrap();
        index.write().unwrap();
        let policy = compose_git(&landed, &working).unwrap();
        for stage in [
            crate::hooks::git_hook::pre_commit,
            crate::hooks::git_hook::pre_merge_commit,
        ] {
            let report = stage(dir.path(), &policy, false, false).unwrap();
            for rule in ["git.secret_scan", "git.conflict_markers"] {
                assert_eq!(
                    report
                        .violations
                        .iter()
                        .any(|v| v.rule == rule && v.level == PolicyLevel::Block),
                    active,
                    "{rule}, landed={landed_level}, checkout={level}: {report:?}"
                );
            }
        }
    }
}
