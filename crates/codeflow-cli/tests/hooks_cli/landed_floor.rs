//! Real hook dispatch against a tracked authority and a weaker checkout.
use std::io::Write;

use git2::{Repository, Signature};
use serde_json::{json, Value};

fn journey_path() -> std::ffi::OsString {
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_codeflow"));
    let mut paths = vec![binary.parent().unwrap().to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(paths).unwrap()
}

fn journey_git(fixture: &Fixture, args: &[&str]) -> std::process::Output {
    super::git_command()
        .current_dir(fixture.dir.path())
        .env("PATH", journey_path())
        .env_remove("CODEFLOW_HOOK_BINARY")
        .args(args)
        .output()
        .unwrap()
}

fn journey_ok(fixture: &Fixture, args: &[&str]) {
    let out = journey_git(fixture, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn journey_adopter() -> Fixture {
    let policy =
        json!({"root_branch":"task/check", "secret_scan":"block", "conflict_markers":"block"});
    let fixture = Fixture::new(policy.clone(), policy, false);
    for args in [
        ["config", "user.name", "Test"],
        ["config", "user.email", "test@example.invalid"],
    ] {
        journey_ok(&fixture, &args);
    }
    let out = super::codeflow()
        .current_dir(fixture.dir.path())
        .env("PATH", journey_path())
        .env_remove("CODEFLOW_HOOK_BINARY")
        .args(["init", "--minimal", "--yes"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "init: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(fixture
        .dir
        .path()
        .join(".codeflow/git-hooks/pre-merge-commit")
        .is_file());
    journey_ok(&fixture, &["add", "."]);
    journey_ok(
        &fixture,
        &[
            "commit",
            "--allow-empty",
            "-m",
            "chore: seed scratch adopter",
        ],
    );
    fixture
}

fn journey_head(fixture: &Fixture) -> String {
    let out = journey_git(fixture, &["rev-parse", "HEAD"]);
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn lower_journey_policy(fixture: &Fixture, level: &str) {
    super::write_policy(
        fixture.dir.path(),
        &json!({"schema_version":1,
        "git":{"root_branch":"task/check", "secret_scan":level, "conflict_markers":level}})
        .to_string(),
    );
}

#[test]
fn landed_floor_real_commit_journey_refuses_content_and_keeps_head() {
    for level in ["off", "allow"] {
        let fixture = journey_adopter();
        lower_journey_policy(&fixture, level);
        std::fs::write(
            fixture.dir.path().join("payload.txt"),
            format!("key = {}{}\n", "AKIA", "Q3ZWJ2X5LMN8RT4V"),
        )
        .unwrap();
        journey_ok(&fixture, &["add", "payload.txt"]);
        let before = journey_head(&fixture);
        let out = journey_git(
            &fixture,
            &[
                "commit",
                "-m",
                "fix: refuse staged content",
                "--",
                "payload.txt",
            ],
        );
        let after = journey_head(&fixture);
        println!(
            "commit journey: root={} level={level} before={before} after={after} exit={}\n{}",
            fixture.dir.path().display(),
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        refused(&out, "git.secret_scan");
        assert!(String::from_utf8_lossy(&out.stderr).contains("codeflow pre-commit"));
        assert_eq!(before, after);
    }
}

#[test]
fn landed_floor_real_merge_journey_refuses_content_and_keeps_head() {
    let mut failures = Vec::new();
    for level in ["off", "allow"] {
        for (kind, content, rule) in [
            (
                "content",
                format!("key = {}{}\n", "AKIA", "Q3ZWJ2X5LMN8RT4V"),
                "git.secret_scan",
            ),
            (
                "markers",
                "<<<<<<< ours\na\n=======\nb\n>>>>>>> theirs\n".to_owned(),
                "git.conflict_markers",
            ),
        ] {
            let fixture = journey_adopter();
            // Seed held topic history as fixture data. The operation under
            // test is the real Git merge through the hooks installed by init.
            let base = fixture.repo.head().unwrap().peel_to_commit().unwrap();
            let mut index = fixture.repo.index().unwrap();
            index.read(true).unwrap();
            index.read_tree(&base.tree().unwrap()).unwrap();
            let payload = fixture.dir.path().join("payload.txt");
            std::fs::write(&payload, content).unwrap();
            index.add_path(std::path::Path::new("payload.txt")).unwrap();
            let tree = fixture.repo.find_tree(index.write_tree().unwrap()).unwrap();
            let sig = Signature::now("Test", "test@example.invalid").unwrap();
            fixture
                .repo
                .commit(
                    Some("refs/heads/task/topic"),
                    &sig,
                    &sig,
                    "feat: held topic payload",
                    &tree,
                    &[&base],
                )
                .unwrap();
            std::fs::remove_file(payload).unwrap();
            lower_journey_policy(&fixture, level);
            let before = journey_head(&fixture);
            let out = journey_git(
                &fixture,
                &[
                    "merge",
                    "--no-ff",
                    "-m",
                    "Merge topic fixture",
                    "task/topic",
                ],
            );
            let after = journey_head(&fixture);
            let case = format!(
                "merge journey: root={} level={level} kind={kind} before={before} after={after}",
                fixture.dir.path().display()
            );
            println!(
                "{case} exit={}\n{}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            );
            collect_refusal(&out, rule, &case, &mut failures);
            if !String::from_utf8_lossy(&out.stderr).contains("codeflow pre-merge-commit") {
                failures.push(format!("{case}: did not refuse at pre-merge-commit"));
            }
            if before != after {
                failures.push(format!("{case}: HEAD moved"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

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
            for stage in ["pre-commit", "pre-merge-commit"] {
                let out = fixture.hook(stage, "fix: check", "");
                let case = format!("stage={stage}, stale={stale}, level={level}");
                collect_refusal(&out, "git.secret_scan", &case, &mut failures);
                collect_refusal(&out, "git.conflict_markers", &case, &mut failures);
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn landed_floor_inactive_authority_and_checkout_do_not_scan_on_either_hook() {
    for stale in [false, true] {
        for level in ["off", "allow"] {
            let policy = json!({"secret_scan":level, "conflict_markers":level});
            let fixture = Fixture::new(policy.clone(), policy, stale);
            fixture.stage(&format!(
                "key = {}{}\n<<<<<<< ours\na\n=======\nb\n>>>>>>> theirs\n",
                "AKIA", "Q3ZWJ2X5LMN8RT4V"
            ));
            for stage in ["pre-commit", "pre-merge-commit"] {
                let out = fixture.hook(stage, "fix: check", "");
                assert!(
                    out.status.success(),
                    "stage={stage}, stale={stale}, level={level}: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
    }
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

/// A landing candidate name admits a branch at pre-push (TSK-1351), so the
/// checkout's `git.candidate_prefix` must not admit a name the landed
/// prefix does not.
#[test]
fn landed_floor_pre_push_rejects_checkout_candidate_prefix() {
    let mut failures = Vec::new();
    for stale in [false, true] {
        let fixture = Fixture::new(
            json!({"branch_prefixes":["task/"]}),
            json!({"branch_prefixes":["task/"], "candidate_prefix":"loose/"}),
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
    println!(
        "binary={}\nexit={}\n{}\n{}",
        env!("CARGO_BIN_EXE_codeflow"),
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    refused(&out, "git.secret_scan");
}

/// AC-2: a checked-out `block` over a landed `warn` blocks, as a stale tree
/// and as an uncommitted file.
#[test]
fn landed_floor_checkout_block_over_landed_warn_blocks() {
    let mut failures = Vec::new();
    for stale in [false, true] {
        let fixture = Fixture::new(
            json!({"secret_scan":"warn"}),
            json!({"secret_scan":"block"}),
            stale,
        );
        fixture.stage(&format!("key = {}{}\n", "AKIA", "Q3ZWJ2X5LMN8RT4V"));
        collect_refusal(
            &fixture.hook("pre-commit", "fix: check", ""),
            "git.secret_scan",
            &format!("stale={stale}"),
            &mut failures,
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
