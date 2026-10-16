//! Range spelling and context regressions for issues 130, 141 and 165.
use super::line_adoption::fixture::{output, passes, Line, LINE};
use std::process::Output;

fn ci(f: &Line, args: &[&str], env: Option<(&str, &str)>) -> Output {
    let mut command = f.command(env!("CARGO_BIN_EXE_codeflow"));
    for key in [
        "CI_MERGE_REQUEST_DIFF_BASE_SHA",
        "CI_COMMIT_SHA",
        "BITBUCKET_PR_DESTINATION_COMMIT",
        "BITBUCKET_COMMIT",
        "GITHUB_BASE_REF",
        "CODEFLOW_DEFAULT_BRANCH",
        "GITHUB_HEAD_REF",
        "CI_MERGE_REQUEST_SOURCE_BRANCH_NAME",
        "CI_COMMIT_REF_NAME",
        "BITBUCKET_BRANCH",
        "CI_MERGE_REQUEST_TARGET_BRANCH_NAME",
        "BITBUCKET_PR_DESTINATION_BRANCH",
    ] {
        command.env_remove(key);
    }
    if let Some((key, value)) = env {
        command.env(key, value);
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
}

#[test]
fn unknown_commit_target_refuses_and_explicit_into_resolves_it() {
    let f = Line::new();
    let base = f.git(&["rev-parse", "HEAD"]);
    f.write("notes.md", "Line notes.\n");
    f.commit("docs: line notes");
    f.git(&["switch", "-q", "main"]);
    f.write("main.md", "Main notes.\n");
    f.commit("docs: main notes");
    let out = ci(&f, &["--base", &base, "--branch", "feat/unbound"], None);
    let text = output(&out);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(
        text.contains("--into") && text.contains("--branch"),
        "{text}"
    );
    assert!(!text.contains("reopen"), "{text}");
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
fn explicit_head_reports_ci_and_checkout_context_before_binding_findings() {
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
        if let Some(finding) = text.find("reopen") {
            assert!(guidance < finding, "{text}");
        }
    }
}

#[test]
fn task_target_selects_base_from_flag_ci_or_checkout() {
    let f = Line::new();
    f.write("line.md", "Line notes.\n");
    let base = f.commit("docs: line notes");
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
    f.git(&["switch", "-qc", "task/TSK-001-work"]);
    let path = "project-management/tasks/TSK-001.md";
    let record = std::fs::read_to_string(f.root().join(path))
        .unwrap()
        .replace(LINE, "integration/EPC-001-missing");
    f.write(path, &record);
    f.commit("docs: change target");
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
            output(&out)
                .contains("acceptance target 'main' (branches at the resolved base commit)"),
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
fn branches_sharing_a_base_refuse_both_spellings_until_into_is_given() {
    let f = Line::new();
    let base = f.git(&["rev-parse", "main"]);
    f.git(&["switch", "-qc", "feat/unbound"]);
    f.write("notes.md", "Notes.\n");
    f.commit("docs: write notes");
    for spelling in [LINE, &base] {
        let out = ci(&f, &["--base", spelling], None);
        assert_eq!(out.status.code(), Some(2), "{}", output(&out));
        assert!(
            output(&out).contains("several branches"),
            "{}",
            output(&out)
        );
        assert!(output(&out).contains("--into"), "{}", output(&out));
        passes(&ci(&f, &["--base", spelling, "--into", LINE], None));
    }
}
