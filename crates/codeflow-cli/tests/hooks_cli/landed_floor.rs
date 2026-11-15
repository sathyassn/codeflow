//! Real hook dispatch against a tracked authority and a weaker checkout.
use std::io::Write;

use git2::{Repository, Signature};
use serde_json::{json, Value};

struct Fixture {
    dir: tempfile::TempDir,
    _remote: tempfile::TempDir,
    repo: Repository,
}

impl Fixture {
    fn new(landed: Value, working: Value, stale: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        Repository::init_bare(remote.path()).unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        repo.set_head("refs/heads/task/check").unwrap();
        std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
        let policy = dir.path().join(".codeflow/policy.json");
        let write = |mut value: Value| {
            value
                .as_object_mut()
                .unwrap()
                .entry("root_branch")
                .or_insert(json!("task/check"));
            std::fs::write(
                &policy,
                json!({"schema_version": 1, "git": value}).to_string(),
            )
            .unwrap();
        };
        let commit = |repo: &Repository| {
            let mut index = repo.index().unwrap();
            index
                .add_path(std::path::Path::new(".codeflow/policy.json"))
                .unwrap();
            index.write().unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let sig = Signature::now("Test", "test@example.invalid").unwrap();
            let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
            let parents: Vec<_> = parent.iter().collect();
            repo.commit(Some("HEAD"), &sig, &sig, "test: seed", &tree, &parents)
                .unwrap()
        };
        write(landed);
        let authority = commit(&repo);
        repo.remote("origin", remote.path().to_str().unwrap())
            .unwrap();
        repo.reference("refs/remotes/origin/main", authority, true, "fixture")
            .unwrap();
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            true,
            "fixture",
        )
        .unwrap();
        write(working);
        if stale {
            commit(&repo);
        }
        Self {
            dir,
            _remote: remote,
            repo,
        }
    }

    fn stage(&self, text: &str) {
        std::fs::write(self.dir.path().join("payload.txt"), text).unwrap();
        let mut index = self.repo.index().unwrap();
        index.add_path(std::path::Path::new("payload.txt")).unwrap();
        index.write().unwrap();
    }

    fn hook(&self, stage: &str, message: &str, input: &str) -> std::process::Output {
        let path = self.repo.path().join("COMMIT_EDITMSG");
        std::fs::write(&path, message).unwrap();
        let mut command = super::codeflow();
        command.env("CODEX_CI", "1");
        command
            .current_dir(self.dir.path())
            .args(["git-hook", stage]);
        if stage == "commit-msg" {
            command.arg(&path);
        }
        let mut child = command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
}

fn refused(out: &std::process::Output, rule: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success() && stderr.contains(rule),
        "expected {rule}: {stderr}"
    );
}

fn collect_refusal(out: &std::process::Output, rule: &str, case: &str, failures: &mut Vec<String>) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    if out.status.success() || !stderr.contains(rule) {
        failures.push(format!("{case}: expected {rule}: {stderr}"));
    }
}

#[test]
fn landed_floor_content_scans_survive_lowered_checkout() {
    let mut failures = Vec::new();
    for stale in [false, true] {
        for level in ["off", "allow"] {
            let fixture = Fixture::new(
                json!({"secret_scan": "block", "conflict_markers": "block"}),
                json!({"secret_scan": level, "conflict_markers": level}),
                stale,
            );
            fixture.stage(&format!(
                "key = {}{}\n<<<<<<< ours\na\n=======\nb\n>>>>>>> theirs\n",
                "AKIA", "Q3ZWJ2X5LMN8RT4V"
            ));
            let out = fixture.hook("pre-commit", "fix: check", "");
            let case = format!("stale={stale}, level={level}");
            collect_refusal(&out, "git.secret_scan", &case, &mut failures);
            collect_refusal(&out, "git.conflict_markers", &case, &mut failures);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn landed_floor_protected_branch_and_levels_on_each_hook() {
    let mut failures = Vec::new();
    for stale in [false, true] {
        for (stage, field) in [
            ("pre-commit", "commit_to_protected"),
            ("pre-merge-commit", "merge_to_protected"),
            ("pre-push", "push_to_protected"),
        ] {
            let mut landed = json!({"protected_branches": ["task/check"]});
            landed[field] = json!("block");
            let mut working = json!({"protected_branches": []});
            working[field] = json!("off");
            let fixture = Fixture::new(landed, working, stale);
            let oid = fixture.repo.head().unwrap().target().unwrap();
            let input = format!(
                "refs/heads/task/check {oid} refs/heads/task/check {}\n",
                "0".repeat(40)
            );
            collect_refusal(
                &fixture.hook(stage, "fix: check", &input),
                &format!("git.{field}"),
                &format!("stale={stale}, stage={stage}"),
                &mut failures,
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn landed_floor_commit_message_rules_and_lists() {
    let mut failures = Vec::new();
    for stale in [false, true] {
        let cases = [
            (
                json!({"commit_format":"block"}),
                json!({"commit_format":"off"}),
                "bad subject",
                "git.commit_format",
            ),
            (
                json!({"commit_required_footers":["Signed-off-by"]}),
                json!({"commit_required_footers":[]}),
                "fix: check",
                "git.commit_body",
            ),
            (
                json!({"commit_ticket_keys":["Refs"], "commit_ticket_required":"block"}),
                json!({"commit_ticket_keys":[]}),
                "fix: check",
                "git.commit_ticket_keys",
            ),
            (
                json!({"commit_ticket_keys":["Refs"], "commit_ticket_required":"block"}),
                json!({"commit_ticket_keys":["Refs", "Closes"], "commit_ticket_required":"off"}),
                "fix: check\n\nCloses: TSK-1\n",
                "git.commit_ticket",
            ),
            (
                json!({"commit_body":"block"}),
                json!({"commit_body":"off"}),
                "fix: check\n\nPlain paragraph\n",
                "git.commit_body",
            ),
            (
                json!({"ai_attribution":"block"}),
                json!({"ai_attribution":"off"}),
                "fix: check\n\n- Generated with Claude\n",
                "git.ai_attribution",
            ),
            (
                json!({"commit_emoji":"block"}),
                json!({"commit_emoji":"off"}),
                "fix: check \u{1F600}",
                "git.commit_emoji",
            ),
            (
                json!({"policy_characters":"block"}),
                json!({"policy_characters":"off"}),
                "fix: check \u{2014} content",
                "git.policy_characters",
            ),
            (
                json!({"commit_types":["fix"]}),
                json!({"commit_types":["fix", "loose"]}),
                "loose: check",
                "git.commit_format",
            ),
            (
                json!({"commit_footer_tokens":["Refs"]}),
                json!({"commit_footer_tokens":["Refs", "Closes"]}),
                "fix: check\n\nCloses: TSK-1\n",
                "git.commit_body",
            ),
            (
                json!({"commit_desc_max_len":3}),
                json!({"commit_desc_max_len":50}),
                "fix: check",
                "git.commit_format",
            ),
            (
                json!({"commit_subject_max_len":5}),
                json!({"commit_subject_max_len":72}),
                "fix: check",
                "git.commit_format",
            ),
            (
                json!({"commit_body_max_bullets":0}),
                json!({"commit_body_max_bullets":3}),
                "fix: check\n\n- one\n",
                "git.commit_body",
            ),
            (
                json!({"commit_body_bullet_max_len":3}),
                json!({"commit_body_bullet_max_len":72}),
                "fix: check\n\n- text\n",
                "git.commit_body",
            ),
            (
                json!({"commit_ticket_keys":["Refs"], "commit_ticket_pattern":"^TSK-[0-9]+$"}),
                json!({"commit_ticket_keys":["Refs"], "commit_ticket_pattern":".*"}),
                "fix: check\n\nRefs: other\n",
                "git.commit_ticket",
            ),
        ];
        for (landed, working, message, rule) in cases {
            let fixture = Fixture::new(landed, working, stale);
            collect_refusal(
                &fixture.hook("commit-msg", message, ""),
                rule,
                &format!("stale={stale}, message={message}"),
                &mut failures,
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn landed_floor_root_checkout_identity_and_level_on_both_hooks() {
    let mut failures = Vec::new();
    for stale in [false, true] {
        for stage in ["pre-commit", "pre-merge-commit"] {
            let fixture = Fixture::new(
                json!({"root_branch":"main", "root_checkout_commits":"block"}),
                json!({"root_branch":"task/check", "root_checkout_commits":"off"}),
                stale,
            );
            collect_refusal(
                &fixture.hook(stage, "fix: check", ""),
                "git.root_checkout_commits",
                &format!("stale={stale}, stage={stage}"),
                &mut failures,
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn landed_floor_pre_push_delete_force_and_naming_levels() {
    let mut failures = Vec::new();
    for stale in [false, true] {
        for field in [
            "delete_protected",
            "force_push_protected",
            "force_push_unprotected",
            "branch_naming",
        ] {
            let mut landed = json!({"protected_branches":["task/check"]});
            let mut working = landed.clone();
            landed[field] = json!("block");
            working[field] = json!("off");
            let fixture = Fixture::new(landed, working, stale);
            let head = fixture.repo.head().unwrap().peel_to_commit().unwrap();
            let sig = Signature::now("Test", "test@example.invalid").unwrap();
            let next = fixture
                .repo
                .commit(
                    None,
                    &sig,
                    &sig,
                    "test: next",
                    &head.tree().unwrap(),
                    &[&head],
                )
                .unwrap();
            let target = match field {
                "force_push_unprotected" => "task/free",
                "branch_naming" => "loose/check",
                _ => "task/check",
            };
            let oid = if field == "delete_protected" {
                "0".repeat(40)
            } else {
                head.id().to_string()
            };
            let input = format!("refs/heads/task/check {oid} refs/heads/{target} {next}\n");
            collect_refusal(
                &fixture.hook("pre-push", "fix: check", &input),
                &format!("git.{field}"),
                &format!("stale={stale}, field={field}"),
                &mut failures,
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn landed_floor_pre_push_rejects_added_prefix() {
    let mut failures = Vec::new();
    for stale in [false, true] {
        let fixture = Fixture::new(
            json!({"branch_prefixes":["task/"]}),
            json!({"branch_prefixes":["task/", "loose/"]}),
            stale,
        );
        let oid = fixture.repo.head().unwrap().target().unwrap();
        let input = format!(
            "refs/heads/task/check {oid} refs/heads/loose/check {}\n",
            "0".repeat(40)
        );
        collect_refusal(
            &fixture.hook("pre-push", "fix: check", &input),
            "git.branch_naming",
            &format!("stale={stale}"),
            &mut failures,
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn landed_floor_unreadable_authority_refuses_every_hook() {
    let mut failures = Vec::new();
    let fixture = Fixture::new(json!({}), json!({}), false);
    std::fs::write(
        fixture.repo.path().join("refs/remotes/origin/main"),
        format!("{}\n", "1".repeat(40)),
    )
    .unwrap();
    for stage in ["pre-commit", "pre-merge-commit", "commit-msg", "pre-push"] {
        collect_refusal(
            &fixture.hook(stage, "fix: check", ""),
            "cannot read",
            stage,
            &mut failures,
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn landed_floor_staged_content_refuses_planted_content() {
    let fixture = Fixture::new(
        json!({"secret_scan":"block"}),
        json!({"secret_scan":"off"}),
        false,
    );
    fixture.stage(&format!("key = {}{}\n", "AKIA", "Q3ZWJ2X5LMN8RT4V"));
    // Direct verification of the worktree binary. Repository git hooks use
    // the installed binary under the operator's hook-binary rule.
    let out = fixture.hook("pre-commit", "fix: check", "");
    let evidence = format!(
        "binary={}\nexit={}\n{}\n{}",
        env!("CARGO_BIN_EXE_codeflow"),
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::write("/private/tmp/TSK-1078-staged-content-output.txt", &evidence).unwrap();
    println!("{evidence}");
    refused(&out, "git.secret_scan");
}
