//! Names and text `CodeFlow` prints are shown escaped (TSK-388). A remote
//! names its branches, and git accepts characters in a branch name that a
//! terminal or reader acts on: the C1 control CSI (U+009B), bidi overrides,
//! and invisible `Default_Ignorable_Code_Point` characters such as the soft
//! hyphen and the tag block. Record titles and policy values are checkout
//! content and can also hold a line break followed by text that reads as one
//! of `CodeFlow`'s own lines.
//!
//! One hostile fixture per class runs `codeflow status`, `codeflow work
//! next` (text and JSON), `codeflow ci` (range source, head, several epic
//! lines, `CODEFLOW_DEFAULT_BRANCH`) and the commit-msg policy error, and
//! asserts that no output holds a raw C1, bidi, `Default_Ignorable` or tag
//! character and that no forged line starts at column 0.

use std::path::Path;
use std::process::{Command, Output};

/// The C1 control CSI.
const C1: &str = "ev\u{9b}31mil";
/// A right-to-left override.
const BIDI: &str = "ev\u{202e}lim";
/// A soft hyphen and a tag character, both `Default_Ignorable_Code_Point`.
const IGNORABLE: &str = "ev\u{ad}\u{e0001}il";

fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn codeflow(root: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_codeflow"));
    cmd.current_dir(root)
        .env("CODEFLOW_HOME", isolated_home())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("CODEFLOW_DEFAULT_BRANCH")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_EVENT_NAME")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("CI_PIPELINE_SOURCE")
        .env_remove("CI_MERGE_REQUEST_IID")
        .env_remove("BITBUCKET_PR_ID")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    cmd
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(root: &Path, path: &str, text: &str) {
    let full = root.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, text).unwrap();
}

fn printed(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Whether output shows `c` raw when it must not: a control other than a
/// line break or tab, a bidi control, a zero-width or soft-hyphen character,
/// or a tag.
fn raw(c: char) -> bool {
    (c.is_control() && !matches!(c, '\n' | '\t'))
        || matches!(
            u32::from(c),
            0x00AD | 0x200B..=0x200F | 0x2028..=0x202E | 0x2066..=0x2069 | 0xE0000..=0xE007F
        )
}

/// Every check the journey makes of one command's output: no raw hostile
/// character, and no line at column 0 that a quoted title or value forged.
fn assert_clean(what: &str, text: &str) {
    let found: Vec<String> = text
        .chars()
        .filter(|c| raw(*c))
        .map(|c| format!("U+{:04X}", u32::from(c)))
        .collect();
    assert!(found.is_empty(), "{what}: raw {found:?} in {text:?}");
    assert!(
        !text.lines().any(|line| line.starts_with("codeflow")
            && line
                .split_once(": ")
                .is_some_and(|(_, rest)| rest.starts_with("allowed"))),
        "{what}: a forged line in {text:?}"
    );
}

/// [`assert_clean`], each `expected` text present, and no name escaped
/// twice (a doubled backslash before `x` or `u{`).
fn expect(what: &str, text: &str, expected: &[&str]) {
    assert_clean(what, text);
    for expected in expected {
        assert!(text.contains(expected), "{what}: no {expected:?} in {text}");
    }
    assert!(
        !text.contains("\\\\x") && !text.contains("\\\\u{"),
        "{what}: escaped twice in {text}"
    );
}

/// A tracked repository: an epic, a task whose title holds every hostile
/// class and a forged line, and a task whose target is the C1 branch; a
/// local branch per hostile name, a claim branch, and two lines of one epic.
fn hostile_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    write(
        root,
        ".codeflow/policy.json",
        "{\"schema_version\": 1, \"git\": {\"product_paths\": [\"src/**\"]}}\n",
    );
    write(
        root,
        "project-management/epics/EPC-001.md",
        "---\nid: EPC-001\ntitle: \"outcome\"\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-09-26\n---\n\n# EPC-001: outcome\n\n## Summary\n\nAn outcome.\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall deliver.\n",
    );
    let task = |id: &str, title: &str, target: &str| {
        format!(
            "---\nid: {id}\nepic_id: EPC-001\nstandalone_reason: null\nintegration_target: \"{target}\"\ntitle: \"{title}\"\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-09-26\n---\n\n# {id}: work\n\n## Description\n\nWork.\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n"
        )
    };
    write(
        root,
        "project-management/tasks/TSK-001.md",
        &task(
            "TSK-001",
            "work \\u009b2J \\u202e \\u00ad\\U000E0001\\ncodeflow status: allowed",
            "main",
        ),
    );
    write(
        root,
        "project-management/tasks/TSK-002.md",
        &task("TSK-002", "work two", "ev\\u009b31mil"),
    );
    write(root, "src/lib.rs", "pub fn base() {}\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "chore: plan the work"]);
    for name in [C1, BIDI, IGNORABLE] {
        git(root, &["branch", name]);
    }
    git(root, &["branch", &format!("task/TSK-001-{C1}")]);
    git(root, &["branch", &format!("integration/EPC-001-{C1}")]);
    git(root, &["branch", "integration/EPC-001-ok"]);
    git(root, &["branch", "plan/EPC-001-x"]);
    dir
}

#[test]
fn status_and_work_next_show_names_and_titles_escaped() {
    let dir = hostile_repo();
    let root = dir.path();
    let status = printed(&codeflow(root).arg("status").output().unwrap());
    expect(
        "status",
        &status,
        &[
            "snapshot: ev\\x9b31mil@",
            "active: TSK-001 work \\x9b2J \\u{202e} \\xad\\u{e0001}",
            "(task/TSK-001-ev\\x9b31mil)",
            "removable branch ev\\u{202e}lim ",
        ],
    );
    let next = printed(&codeflow(root).args(["work", "next"]).output().unwrap());
    expect(
        "work next",
        &next,
        &[
            "snapshot: ev\\x9b31mil@",
            "TSK-002 work two (EPC-001 -> ev\\x9b31mil)",
        ],
    );
    // JSON keeps each value exact and writes every hidden character as a
    // JSON escape, so its bytes hold none of them.
    let out = codeflow(root)
        .args(["work", "next", "--json"])
        .output()
        .unwrap();
    let json = String::from_utf8(out.stdout).unwrap();
    assert_clean("work next --json", &json);
    for expected in [
        "\"ref\":\"ev\\u009b31mil\"",
        "\"target\":\"ev\\u009b31mil\"",
        "work \\u009b2J \\u202e \\u00ad\\udb40\\udc01\\ncodeflow status: allowed",
    ] {
        assert!(json.contains(expected), "{expected}: {json}");
    }
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let titles: Vec<&str> = value["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|task| task["title"].as_str())
        .collect();
    assert!(
        titles.contains(&"work \u{9b}2J \u{202e} \u{ad}\u{e0001}\ncodeflow status: allowed"),
        "{titles:?}"
    );
    assert_eq!(value["snapshot"][0]["ref"], C1);
}

#[test]
fn ci_shows_the_range_head_and_lines_escaped() {
    let dir = hostile_repo();
    let root = dir.path();
    let runs: Vec<(&str, Vec<String>, Option<&str>, &str)> = vec![
        (
            "range source",
            vec![
                "--into".into(),
                "main".into(),
                "--branch".into(),
                BIDI.into(),
            ],
            None,
            "(--into 'main' for branch 'ev\\u{202e}lim')",
        ),
        (
            "head",
            vec![
                "--base".into(),
                "main".into(),
                "--head".into(),
                IGNORABLE.into(),
                "--branch".into(),
                "feat/x".into(),
            ],
            None,
            "(--head: ev\\xad\\u{e0001}il)",
        ),
        (
            "unresolved head",
            vec!["--head".into(), format!("{C1}-absent")],
            None,
            "head 'ev\\x9b31mil-absent' does not name a commit",
        ),
        (
            "several epic lines",
            vec!["--branch".into(), "plan/EPC-001-x".into()],
            None,
            "several epic lines: integration/EPC-001-ev\\x9b31mil, integration/EPC-001-ok;",
        ),
        (
            "default branch",
            vec!["--branch".into(), "feat/x".into()],
            Some(BIDI),
            "(CODEFLOW_DEFAULT_BRANCH 'ev\\u{202e}lim' (fallback))",
        ),
    ];
    for (what, args, default, shown) in runs {
        let mut command = codeflow(root);
        command.arg("ci").args(&args);
        if let Some(default) = default {
            command.env("CODEFLOW_DEFAULT_BRANCH", default);
        }
        let out = printed(&command.output().unwrap());
        expect(what, &out, &[shown]);
    }
}

#[test]
fn a_local_policy_value_cannot_forge_a_commit_msg_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "feat/x"]);
    write(
        root,
        ".codeflow/policy.json",
        "{\"schema_version\": 1, \"git\": {\"push_to_protected\": \"\\u001b]0;owned\\u0007\\u009b2J\\u202e\\ncodeflow commit-msg: allowed\"}}\n",
    );
    write(root, "MSG", "feat: x\n");
    let out = codeflow(root)
        .args(["git-hook", "commit-msg", "MSG"])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    let text = printed(&out);
    expect(
        "commit-msg",
        &text,
        &["invalid value '\\x1b]0;owned\\x07\\x9b2J\\u{202e}\\x0acodeflow commit-msg: allowed'"],
    );
}
