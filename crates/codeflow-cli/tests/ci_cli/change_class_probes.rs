//! TSK-135 review round 1: every probe of the change-class exemption, run
//! through the real binary. Each fixture requires the shipped section list
//! with Release impact at block, and offers a body with only Summary and
//! Changes: exit 0 means the range was classified light, exit 1 with a
//! Reviews finding means it was checked in full. Release impact follows the base and commits.

use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::process::{Command, Output};

use super::{codeflow, git};

const LIGHT: &str =
    "Task: TSK-001\n## Summary\n\nUpdate the contract.\n\n- one file\n\n## Changes\n\n- Update one file.\n";

/// A repository on `main` with the shipped required sections, Release
/// impact at block, the `extra` policy keys and `files`, then a
/// `feat/probe` branch.
fn repo(dir: &Path, extra: &str, files: &[(&str, &str)]) {
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
    write(
        dir,
        ".codeflow/policy.json",
        &format!(
            r#"{{"schema_version":1,"git":{{"pr_required_sections":["Summary","Changes","Reviews","Release impact"],"pr_code_sections":["Testing"],"pr_release_impact":"block","commit_format":"warn"{extra}}}}}"#
        ),
    );
    write(dir, "base.txt", "base\n");
    for (path, text) in files {
        write(dir, path, text);
    }
    commit(dir, "chore: initialize fixture");
    git(dir, &["branch", "integration/line", "main"]);
    git(dir, &["switch", "-q", "-c", "feat/probe"]);
}

fn write(dir: &Path, path: &str, text: &str) {
    let file = dir.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", message]);
}

fn ci(dir: &Path, body: &str) -> Output {
    ci_with(dir, body, None)
}

fn ci_with(dir: &Path, body: &str, path_env: Option<&std::ffi::OsStr>) -> Output {
    let mut cmd = codeflow();
    cmd.args([
        "ci",
        "--base",
        "integration/line",
        "--head",
        "HEAD",
        "--branch",
        "feat/probe",
        "--pr-body",
        body,
    ])
    .current_dir(dir);
    if let Some(path) = path_env {
        cmd.env("PATH", path);
    }
    cmd.output().expect("binary runs")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn assert_light(out: &Output, what: &str) {
    let err = stderr(out);
    assert_eq!(out.status.code(), Some(0), "{what} should be light: {err}");
    assert!(!err.contains("git.pr_"), "{what}: {err}");
}

fn assert_full(out: &Output, what: &str) {
    let err = stderr(out);
    assert_eq!(out.status.code(), Some(1), "{what} should be full: {err}");
    assert!(
        err.contains("'## Reviews'"),
        "{what} was not checked in full: {err}"
    );
}

/// R1: only plain Markdown under `docs/` or `project-management/`, outside
/// every shared path set, is light; the target policy's product paths, the
/// shipped templates, the record schema, dependency manifests, executables
/// in planning directories and hidden instruction trees are all full. R2:
/// names Git would quote are read as they are.
#[test]
fn only_plain_docs_and_records_are_light() {
    let extra = r#","product_paths":["src/**","docs/runtime/**"]"#;
    for (path, light) in [
        ("docs/guide.md", true),
        ("docs/a b.md", true),
        ("docs/a\"b.md", true),
        ("docs/café.md", true),
        ("docs/plan/roadmap.md", true),
        ("src/lib.rs", false),
        ("src/prompt.md", false),
        ("docs/runtime/prompt.md", false),
        ("docs/decisions/template.md", false),
        ("project-management/templates/spec.md", false),
        (".github/pull_request_template.md", false),
        ("requirements.txt", false),
        ("docs/plan/runner.py", false),
        ("project-management/tools/run.py", false),
        ("docs/plan/.agents/guide.md", false),
        ("assets/guide.md", false),
        (".agents/skills/explain/SKILL.md", false),
    ] {
        // Windows file names cannot hold a quote; `café` still makes git
        // quote the path there.
        if cfg!(windows) && path.contains('"') {
            continue;
        }
        let dir = tempfile::tempdir().unwrap();
        repo(dir.path(), extra, &[]);
        write(dir.path(), path, "changed\n");
        commit(dir.path(), "docs: update the guide");
        let out = ci(dir.path(), LIGHT);
        if light {
            assert_light(&out, path);
        } else {
            assert_full(&out, path);
        }
    }
}

/// R1: a watched contract path is full, and a head that drops the target's
/// product path from its own policy does not make the path light.
#[test]
fn target_contract_paths_hold_against_the_head_policy() {
    let dir = tempfile::tempdir().unwrap();
    repo(
        dir.path(),
        r#","breaking_watch_paths":["docs/api/**"]"#,
        &[],
    );
    write(dir.path(), "docs/api/contract.md", "v2\n");
    commit(dir.path(), "docs: change the contract");
    assert_full(&ci(dir.path(), LIGHT), "watched contract path");

    let dir = tempfile::tempdir().unwrap();
    repo(
        dir.path(),
        r#","product_paths":["docs/runtime/**"]"#,
        &[
            (
                "app.py",
                "from pathlib import Path\nprint(Path('docs/runtime/prompt.md').read_text())\n",
            ),
            ("docs/runtime/prompt.md", "original\n"),
        ],
    );
    write(
        dir.path(),
        "docs/runtime/prompt.md",
        "replacement behaviour\n",
    );
    let policy = std::fs::read_to_string(dir.path().join(".codeflow/policy.json"))
        .unwrap()
        .replace(r#","product_paths":["docs/runtime/**"]"#, "");
    write(dir.path(), ".codeflow/policy.json", &policy);
    git(dir.path(), &["add", "-A"]);
    git(
        dir.path(),
        &["commit", "-q", "-m", "docs: replace the prompt"],
    );
    // The checkout policy no longer names it; the target's still does.
    git(
        dir.path(),
        &["checkout", "-q", "main", "--", ".codeflow/policy.json"],
    );
    assert_full(
        &ci(dir.path(), LIGHT),
        "runtime Markdown under a target product path",
    );
}

/// R1: tracked work gets no exemption for a product path either.
#[test]
fn a_tracked_product_markdown_change_is_full() {
    let task = "---\nid: TSK-001\nepic_id: null\nstandalone_reason: bounded work\n\
integration_target: main\ntitle: update behaviour\nstatus: todo\nwork_type: feat\n\
specs: []\ndepends_on: []\ncreated: 2026-09-27\n---\n\n## Description\nUpdate the runtime.\n\n\
## Acceptance Criteria\n- AC-1 When run, the runtime shall use the new prompt. (journey)\n";
    let dir = tempfile::tempdir().unwrap();
    repo(
        dir.path(),
        r#","product_paths":["docs/runtime/**"]"#,
        &[
            ("project-management/tasks/TSK-001.md", task),
            ("docs/runtime/prompt.md", "original\n"),
        ],
    );
    write(
        dir.path(),
        "docs/runtime/prompt.md",
        "replacement behaviour\n",
    );
    commit(dir.path(), "docs: replace the prompt");
    let out = ci(dir.path(), &format!("Task: TSK-001\n\n{LIGHT}"));
    assert_full(&out, "tracked product Markdown");
}

/// R2: a rename is both its sides, a deletion counts, and a gitlink or a
/// symlink named like prose is not prose.
#[test]
fn renames_deletions_and_modes_are_classified_by_both_sides() {
    let dir = tempfile::tempdir().unwrap();
    repo(dir.path(), "", &[("src/app.rs", "fn app() {}\n")]);
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    git(dir.path(), &["mv", "src/app.rs", "docs/app.md"]);
    commit(dir.path(), "docs: move the app");
    assert_full(&ci(dir.path(), LIGHT), "code renamed to docs");

    let dir = tempfile::tempdir().unwrap();
    repo(dir.path(), "", &[("src/app.rs", "fn app() {}\n")]);
    git(dir.path(), &["rm", "-q", "src/app.rs"]);
    write(dir.path(), "docs/guide.md", "Guide\n");
    commit(dir.path(), "docs: replace the app with a guide");
    assert_full(&ci(dir.path(), LIGHT), "deleted code");

    let dir = tempfile::tempdir().unwrap();
    repo(dir.path(), "", &[("docs/old.md", "Old\n")]);
    git(dir.path(), &["rm", "-q", "docs/old.md"]);
    commit(dir.path(), "docs: drop the old guide");
    assert_light(&ci(dir.path(), LIGHT), "deleted docs");

    let dir = tempfile::tempdir().unwrap();
    repo(dir.path(), "", &[]);
    let base = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    git(
        dir.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},docs/module.md", base.trim()),
        ],
    );
    git(dir.path(), &["commit", "-q", "-m", "docs: add a module"]);
    assert_full(&ci(dir.path(), LIGHT), "gitlink named .md");
}

/// R2: a code change made while resolving a merge counts, although both
/// merged branches only changed docs.
#[test]
fn a_merge_resolution_is_part_of_the_range() {
    let dir = tempfile::tempdir().unwrap();
    repo(
        dir.path(),
        "",
        &[("src/lib.rs", "fn value() -> i32 { 1 }\n")],
    );
    write(dir.path(), "docs/left.md", "left\n");
    commit(dir.path(), "docs: add the left guide");
    git(dir.path(), &["switch", "-q", "-c", "docs/right", "main"]);
    write(dir.path(), "docs/right.md", "right\n");
    commit(dir.path(), "docs: add the right guide");
    git(dir.path(), &["switch", "-q", "feat/probe"]);
    git(
        dir.path(),
        &["merge", "-q", "--no-ff", "--no-commit", "docs/right"],
    );
    write(dir.path(), "src/lib.rs", "fn value() -> i32 { 2 }\n");
    commit(dir.path(), "Merge documentation with a code resolution");
    assert_full(&ci(dir.path(), LIGHT), "merge resolution in code");
}

/// A breaking marker in a commit or merge body keeps Release impact
/// required on a docs-only range.
#[test]
fn a_breaking_marker_keeps_release_impact_required() {
    for merge in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        repo(dir.path(), "", &[]);
        write(dir.path(), "docs/guide.md", "Changed\n");
        commit(
            dir.path(),
            "docs: change the contract\n\nBREAKING CHANGE: adopt the replacement",
        );
        if merge {
            git(dir.path(), &["switch", "-q", "-c", "docs/right", "main"]);
            write(dir.path(), "docs/right.md", "right\n");
            commit(dir.path(), "docs: add the right guide");
            git(dir.path(), &["switch", "-q", "feat/probe"]);
            git(
                dir.path(),
                &[
                    "merge",
                    "-q",
                    "--no-ff",
                    "-m",
                    "Merge the guide\n\nBREAKING CHANGE: replace the value contract",
                    "docs/right",
                ],
            );
        }
        let out = ci(dir.path(), LIGHT);
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(1), "merge {merge}: {err}");
        assert!(
            err.contains("git.pr_release_impact"),
            "merge {merge}: {err}"
        );
    }
}

/// R2: when the range's paths cannot be listed the range is code. A `git`
/// that fails only the tree diff turns a light range full.
#[cfg(unix)]
#[test]
fn an_unlisted_range_is_checked_in_full() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    repo(dir.path(), "", &[]);
    write(dir.path(), "docs/guide.md", "Guide\n");
    commit(dir.path(), "docs: add a guide");
    assert_light(&ci(dir.path(), LIGHT), "listed docs range");

    let real = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    let real = String::from_utf8(real.stdout).unwrap().trim().to_string();
    let shim_dir = tempfile::tempdir().unwrap();
    let shim: PathBuf = shim_dir.path().join("git");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = \"--raw\" ] && exit 1; done\nexec \"{real}\" \"$@\"\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(std::iter::once(shim_dir.path().to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    assert_full(
        &ci_with(dir.path(), LIGHT, Some(&path)),
        "range whose tree diff failed",
    );
}

/// The planes agree on the class: the shipped policy workflow runs the same
/// binary over the PR's base and head with full history, so the merge-base
/// tree diff is available there; `CodeFlow`'s own release check stays
/// stricter and always reads Release impact, which its PR template says.
#[test]
fn the_workflow_and_the_release_check_read_the_same_range() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |path: &str| std::fs::read_to_string(root.join(path)).unwrap();
    for workflow in [
        "assets/base/ci/codeflow-policy.yml",
        ".github/workflows/codeflow-policy.yml",
    ] {
        let text = read(workflow);
        assert!(text.contains("fetch-depth: 0"), "{workflow}");
        assert!(
            text.contains(r#"codeflow ci --base "$BASE_SHA" --head "$HEAD_SHA""#),
            "{workflow}"
        );
    }
    assert!(read("scripts/release.py").contains("exactly one '## Release impact' section"));
    assert!(read(".github/pull_request_template.md")
        .contains("on every PR,\n     docs and planning included: keep this section here"));
}
