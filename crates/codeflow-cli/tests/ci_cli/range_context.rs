//! Range spelling and context regressions for issues 130, 141 and 165.
use super::line_adoption::fixture::{output, passes, Line, LINE};
use std::process::Output;

fn ci(f: &Line, args: &[&str], env: Option<(&str, &str)>) -> Output {
    let mut command = f.command(env!("CARGO_BIN_EXE_codeflow"));
    command
        .env_remove("CI_MERGE_REQUEST_DIFF_BASE_SHA")
        .env_remove("CI_COMMIT_SHA")
        .env_remove("BITBUCKET_PR_DESTINATION_COMMIT")
        .env_remove("BITBUCKET_COMMIT")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("CODEFLOW_DEFAULT_BRANCH")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("CI_MERGE_REQUEST_SOURCE_BRANCH_NAME")
        .env_remove("CI_COMMIT_REF_NAME")
        .env_remove("BITBUCKET_BRANCH")
        .env_remove("CI_MERGE_REQUEST_TARGET_BRANCH_NAME")
        .env_remove("BITBUCKET_PR_DESTINATION_BRANCH");
    if let Some((key, value)) = env {
        match key {
            "GITHUB_HEAD_REF" => command.env("GITHUB_HEAD_REF", value),
            "CI_COMMIT_REF_NAME" => command.env("CI_COMMIT_REF_NAME", value),
            "CI_MERGE_REQUEST_TARGET_BRANCH_NAME" => {
                command.env("CI_MERGE_REQUEST_TARGET_BRANCH_NAME", value)
            }
            "CI_MERGE_REQUEST_DIFF_BASE_SHA" => {
                command.env("CI_MERGE_REQUEST_DIFF_BASE_SHA", value)
            }
            "GITHUB_BASE_REF" => command.env("GITHUB_BASE_REF", value),
            "CODEFLOW_DEFAULT_BRANCH" => command.env("CODEFLOW_DEFAULT_BRANCH", value),
            _ => panic!("unsupported fixture CI variable: {key}"),
        };
    }
    command.arg("ci").args(args).output().unwrap()
}

fn sync_merge() -> (Line, String, String) {
    let f = Line::new();
    let path = "project-management/tasks/TSK-002.md";
    let task = super::planned_task("TSK-002", "");
    f.git(&["switch", "-q", "main"]);
    f.write(path, &task);
    f.commit("docs: plan main work");
    f.git(&["switch", "-q", LINE]);
    f.merge("main");
    f.write("line-only.rs", "// belongs only to the integration line\n");
    let direct = f.commit("feat: add line content");
    f.land_adoption(&direct);
    let base = f.git(&["rev-parse", "HEAD"]);
    f.git(&["update-ref", &format!("refs/remotes/origin/{LINE}"), &base]);
    f.git(&["switch", "-q", "main"]);
    f.git(&["switch", "-qc", "task/TSK-002-work"]);
    f.write("main-only.rs", "// reviewed on main\n");
    let reviewed = f.commit("feat: implement main work");
    f.write(path, &format!("{}\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/review/2\n  criteria:\n    AC-1: verified | unit\n    AC-2: verified | journey\n  journey: verified | command ran\n  not_verified: none\n  follow_ups: none: fixture\n  verdict: approved\n```\n", task.replace("status: todo", "status: complete")));
    f.commit("docs: complete main work");
    f.git(&["switch", "-q", "main"]);
    f.merge("task/TSK-002-work");
    f.git(&["switch", "-q", LINE]);
    f.merge("main");
    let head = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "--detach", "-q", &head]);
    (f, base, head)
}

#[test]
fn sync_merge_has_the_same_clean_verdict_for_ref_and_commit_base() {
    let (f, base, head) = sync_merge();
    let named = ci(
        &f,
        &[
            "--base",
            &format!("origin/{LINE}"),
            "--head",
            &head,
            "--branch",
            LINE,
        ],
        None,
    );
    let commit = ci(
        &f,
        &["--base", &base, "--head", &head, "--branch", LINE],
        None,
    );
    passes(&named);
    passes(&commit);
    let findings = |out: &Output| {
        output(out)
            .lines()
            .filter(|line| line.contains("work."))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(findings(&named), findings(&commit));
}

#[test]
fn detached_sync_merge_names_missing_branch_context_without_reopen_advice() {
    let (f, base, head) = sync_merge();
    let out = ci(&f, &["--base", &base, "--head", &head], None);
    let text = output(&out);
    assert!(
        out.status.success() || out.status.code() == Some(2),
        "{text}"
    );
    assert!(
        text.contains("detached checkout") && text.contains("--branch"),
        "{text}"
    );
    assert!(!text.contains("reopen"), "{text}");
}

#[test]
fn planning_branch_uses_its_epic_line_before_protected_fallback() {
    let f = Line::new();
    f.land_task();
    f.git(&["update-ref", &format!("refs/remotes/origin/{LINE}"), LINE]);
    f.git(&["switch", "-qc", "plan/EPC-001-amend"]);
    f.write("project-management/notes.md", "Planning notes.\n");
    let head = f.commit("docs: record plan");
    f.git(&["switch", "--detach", "-q", &head]);
    let automatic = ci(&f, &["--branch", "plan/EPC-001-amend"], None);
    let explicit = ci(
        &f,
        &["--branch", "plan/EPC-001-amend", "--base", LINE],
        None,
    );
    passes(&explicit);
    passes(&automatic);
    let text = output(&automatic);
    assert!(
        text.contains("declared target") && text.contains(LINE),
        "{text}"
    );
    assert!(!text.contains("work.criteria_frozen"), "{text}");
    let findings = |out: &Output| {
        output(out)
            .lines()
            .filter(|line| line.contains("work."))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(findings(&automatic), findings(&explicit));
}

#[test]
fn help_explains_base_selection_and_acceptance_mode() {
    let f = Line::new();
    let out = ci(&f, &["--help"], None);
    let text = output(&out);
    assert!(
        text.contains("frozen or amendable") && text.contains("declared target"),
        "{text}"
    );
    for phrase in [
        "including protected names",
        "standalone task that declares the line",
        "protected_branches",
        "local remote-tracking ref",
        "update-ref of refs/remotes/*",
    ] {
        assert!(text.contains(phrase), "{text}");
    }
}

#[test]
fn unknown_commit_target_uses_repository_default() {
    let f = Line::new();
    let base = f.git(&["rev-parse", "HEAD"]);
    f.write("notes.md", "Line notes.\n");
    f.commit("docs: line notes");
    f.git(&["switch", "-q", "main"]);
    f.write("main.md", "Main notes.\n");
    f.commit("docs: main notes");
    let out = ci(&f, &["--base", &base, "--branch", "feat/unbound"], None);
    let text = output(&out);
    passes(&out);
    assert!(
        text.contains("target 'main' (repository default)"),
        "{text}"
    );
    passes(&ci(
        &f,
        &[
            "--base",
            &base,
            "--branch",
            "feat/unbound",
            "--into",
            "main",
        ],
        None,
    ));
}

#[test]
fn explicit_head_reports_context_without_blame_for_imported_work() {
    let (f, base, head) = sync_merge();
    for source in ["GITHUB_HEAD_REF", "checkout HEAD"] {
        let env = if source == "checkout HEAD" {
            f.git(&["switch", "-q", "main"]);
            None
        } else {
            Some((source, "main"))
        };
        let out = ci(&f, &["--base", &base, "--head", &head], env);
        let text = output(&out);
        assert!(
            text.contains("branch context 'main'") && text.contains(source),
            "{text}"
        );
        let guidance = text.find("pass --branch").expect(&text);
        assert!(!text.contains("reopen"), "{text}");
        assert!(!text.contains("work.acceptance_binding"), "{text}");
        if let Some(finding) = text.find("BLOCKED") {
            assert!(guidance < finding, "{text}");
        }
    }
}

#[test]
fn task_target_selects_base_from_flag_ci_or_checkout() {
    let f = Line::new();
    f.write("line.md", "Line notes.\n");
    let base = f.commit("docs: line notes");
    f.git(&["update-ref", &format!("refs/remotes/origin/{LINE}"), &base]);
    let branch = "task/TSK-001-work";
    f.git(&["switch", "-qc", branch]);
    f.write("work.md", "Task notes.\n");
    f.commit("docs: task notes");
    for (args, env) in [
        (vec!["--branch", branch], None),
        (vec![], Some(("CI_COMMIT_REF_NAME", branch))),
        (vec![], None),
    ] {
        let out = ci(&f, &args, env);
        passes(&out);
        let text = output(&out);
        assert!(text.contains(&format!("range {}..", &base[..8])), "{text}");
        assert!(
            text.contains("declared target") && text.contains(LINE),
            "{text}"
        );
    }
}

#[test]
fn unresolved_declared_target_warns_before_findings() {
    let f = Line::new();
    // The protected branch binds the missing line, so only resolution fails.
    f.git(&["switch", "-q", "main"]);
    let path = "project-management/tasks/TSK-001.md";
    let record = std::fs::read_to_string(f.root().join(path))
        .unwrap()
        .replace(LINE, "integration/EPC-001-missing");
    f.write(path, &record);
    f.commit("docs: change target");
    f.git(&["switch", "-qc", "task/TSK-001-work"]);
    f.write("notes.md", "Notes.\n");
    f.commit("docs: add notes");
    let out = ci(&f, &[], None);
    let text = output(&out);
    let warning = text
        .find("declared target 'integration/EPC-001-missing' does not resolve")
        .expect(&text);
    assert!(text[warning..].contains("--base"), "{text}");
    assert!(
        text.contains("policy protected branches (fallback)"),
        "{text}"
    );
    if let Some(finding) = text.find("[block]") {
        assert!(warning < finding, "{text}");
    }
}

#[test]
fn integration_branch_uses_default_target_and_ci_base_keeps_precedence() {
    let f = Line::new();
    let main = f.git(&["rev-parse", "main"]);
    f.land_task();
    let out = ci(&f, &["--branch", LINE], None);
    passes(&out);
    let text = output(&out);
    assert!(text.contains(&format!("range {}..", &main[..8])), "{text}");
    assert!(text.contains("declared target 'main'"), "{text}");

    let tip = f.git(&["rev-parse", "HEAD"]);
    let out = ci(
        &f,
        &["--branch", LINE],
        Some(("CI_MERGE_REQUEST_DIFF_BASE_SHA", &tip)),
    );
    passes(&out);
    let text = output(&out);
    assert!(text.contains(&format!("range {}..", &tip[..8])), "{text}");
    assert!(
        text.contains("GitLab CI_MERGE_REQUEST_DIFF_BASE_SHA"),
        "{text}"
    );
}

#[test]
fn commit_base_can_infer_the_same_target_as_named_base() {
    let f = Line::new();
    let base = f.git(&["rev-parse", "main"]);
    f.write("line.md", "Line notes.\n");
    f.commit("docs: line notes");
    f.git(&["switch", "-qc", "feat/unbound"]);
    f.write("notes.md", "Notes.\n");
    f.commit("docs: write notes");
    for spelling in ["main", &base] {
        let out = ci(&f, &["--base", spelling], None);
        passes(&out);
        assert!(
            output(&out).contains("acceptance target 'main'"),
            "{}",
            output(&out)
        );
    }
}

#[test]
fn several_planning_lines_require_an_explicit_target() {
    let f = Line::new();
    f.git(&["branch", "integration/EPC-001-other"]);
    let out = ci(&f, &["--branch", "plan/EPC-001-amend"], None);
    assert_eq!(out.status.code(), Some(2), "{}", output(&out));
    assert!(
        output(&out).contains("several epic lines"),
        "{}",
        output(&out)
    );
    passes(&ci(
        &f,
        &["--branch", "plan/EPC-001-amend", "--into", LINE],
        None,
    ));
}

#[test]
fn branches_sharing_a_base_accept_named_base_and_refuse_ambiguous_commit() {
    let f = Line::new();
    let base = f.git(&["rev-parse", "main"]);
    f.git(&["switch", "-qc", "feat/unbound"]);
    f.write("notes.md", "Notes.\n");
    f.commit("docs: write notes");
    let named = ci(&f, &["--base", LINE], None);
    passes(&named);
    assert!(
        output(&named).contains(&format!("acceptance target '{LINE}' (named --base)")),
        "{}",
        output(&named)
    );
    let out = ci(&f, &["--base", &base], None);
    assert_eq!(out.status.code(), Some(2), "{}", output(&out));
    assert!(
        output(&out).contains("several branches"),
        "{}",
        output(&out)
    );
    assert!(output(&out).contains("--into"), "{}", output(&out));
    passes(&ci(&f, &["--base", &base, "--into", LINE], None));
}

#[test]
fn a_protected_target_wins_over_an_ordinary_branch_alias() {
    let f = Line::new();
    f.write("line.md", "Line notes.\n");
    f.commit("docs: line notes");
    f.git(&["branch", "feat/alias", "main"]);
    f.git(&["switch", "-qc", "feat/unbound"]);
    let base = f.git(&["rev-parse", "main"]);
    for spelling in ["main", base.as_str()] {
        let out = ci(&f, &["--base", spelling], None);
        passes(&out);
        assert!(
            output(&out).contains("acceptance target 'main'"),
            "{}",
            output(&out)
        );
    }
}

#[test]
fn ordinary_alias_at_base_cannot_become_the_acceptance_target() {
    let f = Line::new();
    let base = f.git(&["rev-parse", "HEAD"]);
    f.git(&["branch", "docs/unrelated", &base]);
    f.write("line.md", "Line advances.\n");
    f.commit("docs: advance line");
    f.git(&["switch", "-q", "main"]);
    f.write("main.md", "Main advances.\n");
    f.commit("docs: advance main");
    f.git(&["switch", "-qc", "docs/unbound"]);
    f.write("notes.md", "Notes.\n");
    f.commit("docs: add notes");
    let out = ci(&f, &["--base", &base], None);
    passes(&out);
    assert!(
        output(&out).contains("target 'main' (repository default)"),
        "{}",
        output(&out)
    );
}

#[test]
fn named_base_wins_over_an_unrelated_integration_tip() {
    let f = Line::new();
    f.git(&["switch", "-qc", "plan/EPC-002-new"]);
    f.write("notes.md", "Notes.\n");
    f.commit("docs: add notes");
    let out = ci(&f, &["--base", "main"], None);
    passes(&out);
    assert!(
        output(&out).contains("acceptance target 'main' (named --base)"),
        "{}",
        output(&out)
    );
}

#[test]
fn named_protected_base_still_requires_release_impact_with_declared_line() {
    let f = Line::new();
    f.git(&["switch", "-qc", "task/TSK-001-work"]);
    f.write("src/lib.rs", "pub fn changed() {}\n");
    f.commit("fix: change work");
    let body = "Task: TSK-001\n\n## Summary\nFix work.\n\n- Repair work\n\n## Changes\n- Repair work\n\n## Testing\nTests pass.\nNot tested: hosted CI.\n\n## Reviews\nNone: independent review follows\n";
    let out = ci(&f, &["--base", "main", "--pr-body", body], None);
    assert!(
        output(&out).contains("git.pr_release_impact"),
        "{}",
        output(&out)
    );
}

#[test]
fn declared_target_cannot_hide_the_head_or_its_criteria_change() {
    for variant in ["self", "equal", "descendant"] {
        let f = Line::new();
        let branch = "fix/TSK-002-self";
        f.git(&["switch", "-qc", branch]);
        // A bound line (TSK-001 targets it on main) reaches the head check.
        let target = if variant == "self" { branch } else { LINE };
        f.write(
            "project-management/tasks/TSK-002.md",
            &super::planned_task("TSK-002", "")
                .replace(
                    "epic_id: null\nstandalone_reason: bounded work",
                    "epic_id: EPC-001\nstandalone_reason: null",
                )
                .replace(
                    "integration_target: main",
                    &format!("integration_target: {target}"),
                ),
        );
        let path = "project-management/tasks/TSK-001.md";
        let record = std::fs::read_to_string(f.root().join(path))
            .unwrap()
            .replace("AC-1 Work runs.", "AC-1 Changed another task.");
        f.write(path, &record);
        let head = f.commit("docs: change another criterion");
        if variant != "self" {
            f.git(&["branch", "-f", target, &head]);
            f.git(&["switch", "-q", target]);
            if variant == "descendant" {
                f.write("later.md", "Later.\n");
                f.commit("docs: later notes");
            }
            f.git(&[
                "update-ref",
                &format!("refs/remotes/origin/{target}"),
                target,
            ]);
            f.git(&["switch", "-q", branch]);
        }
        let explicit = ci(&f, &["--base", "main"], None);
        assert!(
            output(&explicit).contains("work.criteria_frozen"),
            "{}",
            output(&explicit)
        );
        let out = ci(&f, &[], None);
        if variant == "self" {
            assert_eq!(out.status.code(), Some(1), "{}", output(&out));
            assert!(
                output(&out).contains("base falls back to the policy protected branches"),
                "{}",
                output(&out)
            );
            assert!(
                output(&out).contains("work.criteria_frozen"),
                "{}",
                output(&out)
            );
            continue;
        }
        assert_eq!(out.status.code(), Some(2), "{variant}: {}", output(&out));
        assert!(
            output(&out).contains("declared target")
                && output(&out).contains("--base")
                && output(&out).contains(&head[..8]),
            "{}",
            output(&out)
        );
    }
}

#[test]
fn unresolved_explicit_and_ci_targets_name_their_source() {
    let f = Line::new();
    f.git(&["switch", "-qc", "docs/unbound"]);
    f.write("notes.md", "Notes.\n");
    f.commit("docs: add notes");
    for (args, env, source) in [
        (vec!["--into", "integration/missing"], None, "--into"),
        (
            vec![],
            Some(("CI_MERGE_REQUEST_TARGET_BRANCH_NAME", "integration/missing")),
            "CI target metadata",
        ),
    ] {
        let out = ci(&f, &args, env);
        passes(&out);
        let text = output(&out);
        assert!(
            text.contains(&format!("{source} 'integration/missing' does not resolve")),
            "{text}"
        );
        assert!(!text.contains("declared target"), "{text}");
    }
}

#[test]
fn bare_undeclared_branch_uses_default_when_base_has_several_destinations() {
    let f = Line::new();
    f.git(&["switch", "-qc", "docs/unbound"]);
    f.write("notes.md", "Notes.\n");
    f.commit("docs: add notes");
    let out = ci(&f, &[], None);
    passes(&out);
    assert!(
        output(&out).contains("target 'main' (repository default)"),
        "{}",
        output(&out)
    );
}

#[test]
fn round_two_default_target_keeps_release_impact_checks() {
    let f = Line::new();
    f.git(&["switch", "-qc", "docs/unbound"]);
    f.write("src/lib.rs", "pub fn changed() {}\n");
    f.commit("fix: change work");
    let body = "Task: TSK-001\n\n## Summary\nFix work.\n\n- Repair work\n\n## Changes\n- Repair work\n\n## Testing\nNot tested: hosted CI.\n\n## Reviews\nNone: independent review follows\n";
    let out = ci(&f, &["--pr-body", body], None);
    let text = output(&out);
    assert!(
        text.contains("target 'main' (repository default)"),
        "{text}"
    );
    assert!(text.contains("git.pr_release_impact"), "{text}");
}

#[test]
fn round_two_declared_decoy_cannot_hide_an_earlier_change() {
    for target in ["feat/decoy", "integration/decoy"] {
        let f = Line::new();
        f.git(&["switch", "-qc", "fix/TSK-002-prior"]);
        f.write(
            "project-management/tasks/TSK-002.md",
            &super::planned_task("TSK-002", "").replace(
                "integration_target: main",
                &format!("integration_target: {target}"),
            ),
        );
        let path = "project-management/tasks/TSK-001.md";
        let record = std::fs::read_to_string(f.root().join(path))
            .unwrap()
            .replace("AC-1 Work runs.", "AC-1 Changed another task.");
        f.write(path, &record);
        f.commit("docs: change another criterion");
        f.git(&["branch", target]);
        f.write("src/lib.rs", "pub fn changed() {}\n");
        f.commit("fix: change work");
        let out = ci(&f, &[], None);
        let text = output(&out);
        assert!(text.contains("work.criteria_frozen"), "{target}: {text}");
        assert!(
            text.contains("warning") && text.contains("protected branches"),
            "{text}"
        );
        assert!(
            !text.contains(&format!("acceptance target '{target}'")),
            "{target}: {text}"
        );
    }
}

#[test]
fn round_three_ineligible_declared_target_keeps_the_default_target() {
    let f = Line::new();
    f.git(&["switch", "-qc", "task/TSK-001-work"]);
    let path = "project-management/tasks/TSK-001.md";
    let record = std::fs::read_to_string(f.root().join(path))
        .unwrap()
        .replace(LINE, "feat/decoy");
    f.write(path, &record);
    f.write("src/lib.rs", "pub fn work() {}\n");
    f.commit("feat: implement work");
    f.git(&["branch", "feat/decoy"]);
    let body = "Task: TSK-001\n\n## Summary\nWork.\n\n- Do work\n\n## Changes\n- Work\n\n## Testing\nFixture.\nNot tested: hosted CI.\n\n## Reviews\nNone: independent review follows\n";
    let out = ci(&f, &["--pr-body", body], None);
    let text = output(&out);
    assert!(
        text.contains("target 'main' (repository default)"),
        "{text}"
    );
    assert!(!text.contains("acceptance target 'feat/decoy'"), "{text}");
    assert!(text.contains("git.pr_release_impact"), "{text}");
}

#[test]
fn round_three_epic_shaped_decoy_cannot_hide_an_earlier_change() {
    for target in ["integration/EPC-001-decoy", "integration/EPC-777-decoy"] {
        let f = Line::new();
        f.git(&["switch", "-qc", "fix/TSK-002-prior"]);
        f.write(
            "project-management/tasks/TSK-002.md",
            &super::planned_task("TSK-002", "")
                .replace(
                    "epic_id: null\nstandalone_reason: bounded work",
                    "epic_id: EPC-001\nstandalone_reason: null",
                )
                .replace(
                    "integration_target: main",
                    &format!("integration_target: {target}"),
                ),
        );
        let path = "project-management/tasks/TSK-001.md";
        let record = std::fs::read_to_string(f.root().join(path))
            .unwrap()
            .replace("AC-1 Work runs.", "AC-1 Changed another task.");
        f.write(path, &record);
        f.commit("docs: change another criterion");
        f.git(&["branch", target]);
        f.write("src/lib.rs", "pub fn changed() {}\n");
        f.commit("fix: change work");
        let out = ci(&f, &[], None);
        let text = output(&out);
        assert!(text.contains("work.criteria_frozen"), "{target}: {text}");
        assert!(
            text.contains(&format!("declared target '{target}'"))
                && text.contains("base falls back to the policy protected branches"),
            "{target}: {text}"
        );
        assert!(
            !text.contains(&format!("acceptance target '{target}'")),
            "{target}: {text}"
        );
    }
}

#[test]
fn round_three_cancelled_epic_line_is_not_bound() {
    let f = Line::new();
    f.git(&["switch", "-q", "main"]);
    let epic = super::line_adoption::fixture::EPIC_PATH;
    let record = std::fs::read_to_string(f.root().join(epic))
        .unwrap()
        .replace("status: planning", "status: cancelled");
    f.write(epic, &record);
    let main = f.commit("docs: cancel the epic");
    f.git(&["switch", "-qc", "task/TSK-001-work"]);
    f.write("notes.md", "Notes.\n");
    f.commit("docs: add notes");
    let text = output(&ci(&f, &[], None));
    assert!(
        text.contains(&format!(
            "declared target '{LINE}' names EPC-001, which is cancelled"
        )),
        "{text}"
    );
    assert!(text.contains(&format!("range {}..", &main[..8])), "{text}");
    assert!(
        !text.contains(&format!("acceptance target '{LINE}'")),
        "{text}"
    );
}

/// The real workflow: the epic and its tasks are recorded only on the line,
/// which exists on the remote, never on the protected branch.
fn records_only_on_the_remote_line() -> (Line, String) {
    let f = Line::new();
    f.git(&["switch", "-q", "main"]);
    f.git(&["rm", "-q", "-r", "project-management"]);
    f.write("project-management/README.md", "Records.\n");
    f.commit("docs: keep line records on the line");
    f.git(&["switch", "-q", LINE]);
    f.write("line.md", "Line notes.\n");
    let tip = f.commit("docs: line notes");
    f.git(&["update-ref", &format!("refs/remotes/origin/{LINE}"), &tip]);
    f.git(&["switch", "-qc", "task/TSK-001-work"]);
    f.write("work.md", "Task notes.\n");
    f.commit("docs: task notes");
    (f, tip)
}

#[test]
fn round_four_remote_line_records_bind_the_declared_line() {
    let (f, tip) = records_only_on_the_remote_line();
    let out = ci(&f, &[], None);
    passes(&out);
    let text = output(&out);
    assert!(text.contains(&format!("range {}..", &tip[..8])), "{text}");
    assert!(
        text.contains(&format!(
            "acceptance target '{LINE}' (branch declared target)"
        )),
        "{text}"
    );
    assert!(!text.contains("warning: branch declared target"), "{text}");

    // A stale local branch of the same name never replaces the remote ref.
    f.git(&["branch", "-f", LINE, &format!("{tip}~1")]);
    let out = ci(&f, &[], None);
    passes(&out);
    let text = output(&out);
    assert!(text.contains(&format!("range {}..", &tip[..8])), "{text}");
    assert!(
        text.contains(&format!(
            "local branch '{LINE}' differs from 'origin/{LINE}'"
        )),
        "{text}"
    );
}

#[test]
fn round_four_local_only_line_never_counts() {
    let (f, _) = records_only_on_the_remote_line();
    f.git(&["update-ref", "-d", &format!("refs/remotes/origin/{LINE}")]);
    let text = output(&ci(&f, &[], None));
    assert!(
        text.contains(&format!("declared target '{LINE}'"))
            && text.contains("local-only line never counts")
            && text.contains("base falls back to the policy protected branches"),
        "{text}"
    );
    assert!(
        !text.contains(&format!("acceptance target '{LINE}'")),
        "{text}"
    );
}

#[test]
fn round_four_remote_decoy_without_binding_records_falls_back() {
    let f = Line::new();
    f.git(&["switch", "-qc", "fix/TSK-002-prior"]);
    let path = "project-management/tasks/TSK-001.md";
    let record = std::fs::read_to_string(f.root().join(path))
        .unwrap()
        .replace("AC-1 Work runs.", "AC-1 Changed another task.");
    f.write(path, &record);
    let earlier = f.commit("docs: change another criterion");
    let decoy = "integration/EPC-001-decoy";
    f.git(&[
        "update-ref",
        &format!("refs/remotes/origin/{decoy}"),
        &earlier,
    ]);
    f.write(
        "project-management/tasks/TSK-002.md",
        &super::planned_task("TSK-002", "")
            .replace(
                "epic_id: null\nstandalone_reason: bounded work",
                "epic_id: EPC-001\nstandalone_reason: null",
            )
            .replace(
                "integration_target: main",
                &format!("integration_target: {decoy}"),
            ),
    );
    f.write("src/lib.rs", "pub fn changed() {}\n");
    f.commit("fix: change work");
    let text = output(&ci(&f, &[], None));
    assert!(text.contains("work.criteria_frozen"), "{text}");
    assert!(
        text.contains(&format!("declared target '{decoy}'"))
            && text.contains(&format!("on 'origin/{decoy}'"))
            && text.contains("base falls back to the policy protected branches"),
        "{text}"
    );
    assert!(
        !text.contains(&format!("acceptance target '{decoy}'")),
        "{text}"
    );
}

#[test]
fn round_two_planning_inference_ignores_another_epics_line() {
    let f = Line::new();
    let base = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-q", "main"]);
    f.write("later.md", "Later.\n");
    f.commit("docs: advance main");
    f.git(&["switch", "-qc", "plan/EPC-002-new", &base]);
    f.write("notes.md", "Notes.\n");
    f.commit("docs: add notes");
    let out = ci(&f, &["--base", &base], None);
    let text = output(&out);
    assert!(
        text.contains("target 'main' (repository default)"),
        "{text}"
    );
    assert!(
        !text.contains("acceptance target 'integration/EPC-001"),
        "{text}"
    );
}

#[test]
fn round_two_named_base_disagreement_is_visible() {
    let f = Line::new();
    f.git(&["switch", "-qc", "task/TSK-001-work"]);
    f.write("src/lib.rs", "pub fn changed() {}\n");
    f.commit("fix: change work");
    let out = ci(&f, &["--base", "main"], None);
    let text = output(&out);
    assert!(
        text.contains("declared target") && text.contains("differs from named --base 'main'"),
        "{text}"
    );
}

fn declared_target_push(target: &str) -> (Line, Output) {
    use std::io::Write;
    use std::process::Stdio;
    let f = Line::new();
    f.git(&["switch", "-q", "main"]);
    f.write(".codeflow/policy.json", "{\"schema_version\":1,\"git\":{\"product_paths\":[\"src/**\"],\"test_gate_on_push\":\"warn\"}}\n");
    f.write(
        "project-management/tasks/TSK-002.md",
        &super::planned_task("TSK-002", ""),
    );
    f.commit("docs: plan successor");
    f.git(&["push", "-q", "origin", "main"]);
    let branch = "fix/TSK-002-work";
    f.git(&["switch", "-qc", branch]);
    f.write(
        "project-management/tasks/TSK-002.md",
        &super::planned_task("TSK-002", "").replace(
            "integration_target: main",
            &format!("integration_target: '{target}'"),
        ),
    );
    let path = "project-management/tasks/TSK-001.md";
    let record = std::fs::read_to_string(f.root().join(path))
        .unwrap()
        .replace("AC-1 Work runs.", "AC-1 Changed another task.");
    f.write(path, &record);
    let head = f.commit("docs: change another criterion");
    let url = f.git(&["remote", "get-url", "origin"]);
    let mut child = f
        .command(env!("CARGO_BIN_EXE_codeflow"))
        .args(["git-hook", "pre-push", "origin", &url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "refs/heads/{branch} {head} refs/heads/{branch} {}",
        super::line_adoption::fixture::ZERO
    )
    .unwrap();
    let out = child.wait_with_output().unwrap();
    (f, out)
}

#[test]
fn round_two_option_shaped_target_cannot_downgrade_the_real_push_hook() {
    for target in ["main", "-x", "--help", "bad..branch", "task/TSK-002-self"] {
        let (_, out) = declared_target_push(target);
        assert_eq!(out.status.code(), Some(1), "{target}: {}", output(&out));
        assert!(
            !output(&out).contains("unexpected argument"),
            "{}",
            output(&out)
        );
    }
}

#[test]
fn round_two_docs_validation_rejects_option_shaped_targets() {
    for target in ["-x", "refs/heads/-x", "origin/-x"] {
        let (f, _) = declared_target_push(target);
        let out = f
            .command(env!("CARGO_BIN_EXE_codeflow"))
            .args(["validate", "--docs"])
            .output()
            .unwrap();
        let text = output(&out);
        assert!(
            !out.status.success()
                && text.contains("integration_target")
                && text.contains("stable non-task branch"),
            "{target}: {text}"
        );
    }
}

#[test]
fn protected_local_target_cannot_hide_a_criterion_without_a_remote_ref() {
    let f = Line::new();
    f.protected_target_shadow();
    let out = ci(&f, &[], None);
    let text = output(&out);
    assert!(text.contains("work.criteria_frozen"), "{text}");
    assert!(text.contains("exists only as a local branch"), "{text}");
    assert!(
        text.contains("base falls back to the policy protected branches"),
        "{text}"
    );
    assert_eq!(out.status.code(), Some(1), "{text}");
    let base = f.git(&["rev-parse", "main"]);
    f.git(&["update-ref", "refs/remotes/origin/master", &base]);
    let text = output(&ci(&f, &[], None));
    assert!(text.contains("work.criteria_frozen"), "{text}");
    assert!(
        text.contains("local branch 'master' differs from 'origin/master'"),
        "{text}"
    );
}

#[test]
fn standalone_task_uses_remote_bound_line_in_ci_and_work_start() {
    let (f, tip) = Line::standalone_on_remote_line();
    let start = f
        .command(env!("CARGO_BIN_EXE_codeflow"))
        .args(["work", "start", "TSK-002"])
        .output()
        .unwrap();
    passes(&start);
    assert!(output(&start).contains(LINE), "{}", output(&start));
    let out = ci(&f, &[], None);
    let text = output(&out);
    assert!(text.contains(&format!("range {}..", &tip[..8])), "{text}");
    assert!(
        text.contains(&format!(
            "acceptance target '{LINE}' (branch declared target)"
        )),
        "{text}"
    );
    assert!(!text.contains("warning: branch declared target"), "{text}");
    passes(&out);
}

#[test]
fn standalone_task_does_not_trust_decoy_lines_or_another_epic() {
    for case in [
        "local_only",
        "wrong_line",
        "missing_reason",
        "blank_reason",
        "missing_epic",
        "forged_epic",
        "other_epic",
        "cancelled_epic",
    ] {
        let (f, tip) = Line::standalone_on_remote_line();
        let path = "project-management/tasks/TSK-002.md";
        let record = std::fs::read_to_string(f.root().join(path)).unwrap();
        let target = if case == "wrong_line" {
            "integration/EPC-001-decoy"
        } else {
            LINE
        };
        let expected = match case {
            "local_only" => {
                f.git(&["update-ref", "-d", &format!("refs/remotes/origin/{LINE}")]);
                "local-only line never counts"
            }
            "wrong_line" => {
                f.git(&["update-ref", &format!("refs/remotes/origin/{target}"), &tip]);
                f.write(path, &record.replace(LINE, target));
                f.commit("docs: declare decoy target");
                "is not the target of any task of EPC-001"
            }
            "missing_reason" | "blank_reason" => {
                let replacement = if case == "missing_reason" {
                    ""
                } else {
                    "standalone_reason: ' '\n"
                };
                f.write(
                    path,
                    &record.replace("standalone_reason: Bounded repair\n", replacement),
                );
                f.commit("docs: remove standalone reason");
                "standalone task on branch 'task/TSK-002-repair' has no nonblank standalone_reason"
            }
            "other_epic" => {
                f.write(
                    path,
                    &record.replace("epic_id: null", "epic_id: EPC-002").replace(
                        "standalone_reason: Bounded repair",
                        "standalone_reason: null",
                    ),
                );
                f.commit("docs: assign another epic");
                "which is not the epic of branch"
            }
            _ => {
                f.git(&["switch", "-qc", "fixture/anchor", &tip]);
                if case == "cancelled_epic" {
                    let epic = std::fs::read_to_string(
                        f.root().join(super::line_adoption::fixture::EPIC_PATH),
                    )
                    .unwrap();
                    f.write(
                        super::line_adoption::fixture::EPIC_PATH,
                        &epic.replace("status: planning", "status: cancelled"),
                    );
                } else {
                    f.git(&["rm", super::line_adoption::fixture::EPIC_PATH]);
                }
                let anchor = f.commit("docs: change remote records");
                f.git(&[
                    "update-ref",
                    &format!("refs/remotes/origin/{LINE}"),
                    &anchor,
                ]);
                f.git(&["switch", "-q", "task/TSK-002-repair"]);
                if case == "missing_epic" {
                    f.git(&["rm", super::line_adoption::fixture::EPIC_PATH]);
                    f.commit("docs: remove local epic");
                }
                // In forged_epic the head still carries an epic and binding task:
                // the author's own records cannot substitute for a trusted anchor.
                if case == "cancelled_epic" {
                    "which is cancelled"
                } else {
                    "has no epic record"
                }
            }
        };
        let text = output(&ci(&f, &[], None));
        assert!(text.contains(expected), "{case}: {text}");
        assert!(
            text.contains("base falls back to the policy protected branches"),
            "{case}: {text}"
        );
        assert!(
            !text.contains(&format!("acceptance target '{target}'")),
            "{case}: {text}"
        );
        let main = f.git(&["rev-parse", "origin/main"]);
        let fallback_range = format!("range {}..", &main[..8]);
        assert!(text.contains(&fallback_range), "{case}: {text}");
    }
}

/// TSK-398: a local branch named `origin/main` (`refs/heads/origin/main`)
/// at an author commit never stands in for the remote-tracking
/// `refs/remotes/origin/main`, whichever source names the base branch. The
/// shadowed run gets the clean run's verdict: the same range base, and a
/// local-only line still never binds.
#[test]
fn local_origin_main_branch_never_replaces_the_remote_tracking_base() {
    for source in [
        None,
        Some(("GITHUB_BASE_REF", "main")),
        Some(("CODEFLOW_DEFAULT_BRANCH", "main")),
    ] {
        let (f, tip) = Line::standalone_on_remote_line();
        f.git(&["push", "-q", "origin", &format!(":{LINE}")]);
        let main = f.git(&["rev-parse", "refs/remotes/origin/main"]);
        let range = format!("range {}..", &main[..8]);
        let clean = ci(&f, &[], source);
        f.git(&["branch", "origin/main", &tip]);
        assert_eq!(f.git(&["rev-parse", "refs/heads/origin/main"]), tip);
        let shadowed = ci(&f, &[], source);
        let (clean_text, text) = (output(&clean), output(&shadowed));
        assert!(clean_text.contains(&range), "{source:?}: {clean_text}");
        assert!(text.contains(&range), "{source:?}: {text}");
        assert!(
            !text.contains(&format!("range {}..", &tip[..8])),
            "{source:?}: {text}"
        );
        assert!(
            !text.contains(&format!("acceptance target '{LINE}'")),
            "{source:?}: {text}"
        );
        if source.is_none() {
            assert!(
                text.contains("local-only line never counts"),
                "{source:?}: {text}"
            );
        }
        assert_eq!(
            shadowed.status.code(),
            clean.status.code(),
            "{source:?}: clean {clean_text}\nshadowed {text}"
        );
    }
}

/// A pushed `main` with later work, and an undeclared branch with one
/// commit on it. Returns the fixture and the protected tip.
fn feature_on_pushed_main() -> (Line, String) {
    let f = Line::new();
    f.git(&["switch", "-q", "main"]);
    f.write("later.md", "Later main work.\n");
    f.commit("docs: later main work");
    f.git(&["push", "-q", "origin", "main"]);
    let main = f.git(&["rev-parse", "refs/heads/main"]);
    f.git(&["switch", "-qc", "fix/feature"]);
    f.write("src/lib.rs", "pub fn feature() {}\n");
    f.commit("fix: add the feature");
    (f, main)
}

/// TSK-398 review F2: with the exact `refs/remotes/origin/main` absent, a
/// local branch or tag whose full name is `refs/remotes/origin/main` never
/// answers for it; the base stays the local `refs/heads/main`.
#[test]
fn a_ref_named_like_the_tracking_ref_never_answers_for_an_absent_one() {
    for shadow in ["branch", "tag"] {
        let (f, main) = feature_on_pushed_main();
        f.git(&["update-ref", "-d", "refs/remotes/origin/main"]);
        let tip = f.git(&["rev-parse", "HEAD"]);
        if shadow == "branch" {
            f.git(&["branch", "refs/remotes/origin/main", &tip]);
            assert_eq!(
                f.git(&["rev-parse", "refs/heads/refs/remotes/origin/main"]),
                tip
            );
        } else {
            f.git(&["tag", "refs/remotes/origin/main", &tip]);
            assert_eq!(
                f.git(&["rev-parse", "refs/tags/refs/remotes/origin/main"]),
                tip
            );
        }
        let text = output(&ci(&f, &[], None));
        assert!(
            text.contains(&format!("range {}..", &main[..8])),
            "{shadow}: {text}"
        );
        assert!(
            !text.contains(&format!("range {}..", &tip[..8])),
            "{shadow}: {text}"
        );
    }
}

/// TSK-398 review N3: a base variable carrying revision syntax is ignored
/// with a warning, so the base is the protected tip, never a walk from it.
#[test]
fn base_variables_with_revision_syntax_are_ignored() {
    for key in ["GITHUB_BASE_REF", "CODEFLOW_DEFAULT_BRANCH"] {
        for value in ["main~1", "main@{0}"] {
            let (f, main) = feature_on_pushed_main();
            let text = output(&ci(&f, &[], Some((key, value))));
            assert!(
                text.contains(&format!("{key} '{value}' is not a valid branch name")),
                "{key}={value}: {text}"
            );
            assert!(
                text.contains(&format!("range {}..", &main[..8])),
                "{key}={value}: {text}"
            );
            assert!(
                !text.contains(&format!("acceptance target '{value}'")),
                "{key}={value}: {text}"
            );
        }
    }
}

/// Codex review F2 and F3 fixture: target tip A on its own line, a malformed
/// commit B on `origin/main`, and the change C on B. Returns (f, A, B).
fn malformed_main_beside_a_target() -> (Line, String, String) {
    let f = Line::new();
    f.git(&["switch", "-q", "main"]);
    f.git(&["switch", "-qc", "release-line"]);
    f.write("release.md", "Release work.\n");
    let target = f.commit("docs: add release work");
    f.git(&["switch", "-q", "main"]);
    f.write("main.md", "Main work.\n");
    let malformed = f.commit("invalid message");
    f.git(&["update-ref", "refs/remotes/origin/main", &malformed]);
    f.git(&["switch", "-qc", "fix/review"]);
    f.write("src/lib.rs", "pub fn review() {}\n");
    f.commit("fix: add the review change");
    (f, target, malformed)
}

/// Codex review F2: `release@2026`, `deadbeef` and `@` are valid branch names, so
/// CI metadata naming them selects that target's tip and acceptance target,
/// and the malformed commit between the target and `main` is judged.
#[test]
fn valid_branch_names_with_at_or_hex_select_their_own_target() {
    for name in ["release@2026", "deadbeef", "@"] {
        for key in ["GITHUB_BASE_REF", "CODEFLOW_DEFAULT_BRANCH"] {
            let (f, target, malformed) = malformed_main_beside_a_target();
            f.git(&[
                "update-ref",
                &format!("refs/remotes/origin/{name}"),
                &target,
            ]);
            let out = ci(&f, &[], Some((key, name)));
            let text = output(&out);
            assert!(
                text.contains(&format!("range {}..", &target[..8])),
                "{key}={name}: {text}"
            );
            assert!(
                text.contains(&format!("commit {}", &malformed[..8]))
                    && text.contains("git.commit_format"),
                "{key}={name}: {text}"
            );
            assert!(!text.contains("is not a valid branch name"), "{text}");
            if key == "GITHUB_BASE_REF" {
                assert!(
                    text.contains(&format!("acceptance target '{name}' (CI target metadata)")),
                    "{key}={name}: {text}"
                );
            }
            assert_eq!(out.status.code(), Some(1), "{key}={name}: {text}");
        }
    }
}

/// Codex review F3: a full `refs/remotes/upstream/main` is read exactly as
/// written, never rewritten to origin, so the fork's own tip bounds the range.
#[test]
fn a_full_upstream_ref_is_read_exactly_as_written() {
    let (f, upstream, malformed) = malformed_main_beside_a_target();
    f.git(&["update-ref", "refs/remotes/upstream/main", &upstream]);
    let out = ci(
        &f,
        &[],
        Some(("CODEFLOW_DEFAULT_BRANCH", "refs/remotes/upstream/main")),
    );
    let text = output(&out);
    assert!(
        text.contains(&format!("range {}..", &upstream[..8])),
        "{text}"
    );
    assert!(
        text.contains(&format!("commit {}", &malformed[..8])) && text.contains("git.commit_format"),
        "{text}"
    );
    assert_eq!(out.status.code(), Some(1), "{text}");
}
