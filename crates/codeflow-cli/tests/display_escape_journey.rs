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

#[path = "support/process_folders.rs"]
mod process_folders;
use process_folders::isolated_home;

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
    let forged = text.lines().find(|line| {
        // A line the fixtures plant: `codeflow <plane>: allowed`, a fake
        // gate result, or a fake progress line, starting at column 0.
        (line.starts_with("codeflow")
            && line
                .split_once(": ")
                .is_some_and(|(_, rest)| rest.starts_with("allowed")))
            || line.starts_with("codeflow test: passed")
            || line.starts_with("[codeflow test] completed target 'x'")
    });
    assert!(
        forged.is_none(),
        "{what}: a forged line {forged:?} in {text:?}"
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
    // Round 3 (Grok F-2): a claim branch decodes to the name git stores;
    // the text line shows its display form.
    let claimed = value["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == "TSK-001")
        .unwrap();
    assert_eq!(claimed["branches"][0], format!("task/TSK-001-{C1}"));
}

/// Round 3 (security S2): `codeflow test` relays a target's progress lines,
/// which a checkout's test command writes; they are shown escaped.
#[test]
fn the_test_runner_relays_progress_escaped() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(
        root,
        ".codeflow/test-config.json",
        "{\"schema_version\": \"1.0\", \"targets\": [{\"name\": \"hostile\", \"runner\": \"custom\", \"modes\": {\"quick\": {\"command\": \"printf 'Summary ev\\\\342\\\\200\\\\256lim \\\\302\\\\233x\\\\n'\"}}}]}\n",
    );
    let out = codeflow(root)
        .args(["test", "--mode", "quick"])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    expect(
        "codeflow test",
        &err,
        &["[codeflow test] hostile: Summary ev\\u{202e}lim \\x9bx"],
    );
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

/// Codex F-1: an error `main` returns is printed at the CLI boundary with
/// its whole chain escaped; the exit stays nonzero.
#[test]
fn a_returned_error_chain_is_shown_escaped() {
    // Windows forbids C0 characters in a file name, so the OSC and BEL case
    // runs where the platform allows it; the bidi and C1 case runs on every
    // platform (Codex round 2, F-5).
    let mut names = vec![("ev\u{202e}\u{9b}lim", "ev\\u{202e}\\x9blim")];
    if cfg!(unix) {
        names.push((
            "ev\u{1b}]0;owned\u{7}\u{202e}",
            "ev\\x1b]0;owned\\x07\\u{202e}",
        ));
    }
    for (name, shown) in names {
        for command in [
            &["remote", "protect", "--dry-run"][..],
            &["recall", "x"][..],
        ] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join(name);
            std::fs::create_dir(&root).unwrap();
            // A regular file where the state folder belongs: discovery fails
            // and the returned error chain names the path.
            std::fs::write(root.join(".codeflow"), "fixture: not a directory\n").unwrap();
            let out = codeflow(&root).args(command).output().unwrap();
            assert_ne!(out.status.code(), Some(0), "{command:?}");
            let err = String::from_utf8_lossy(&out.stderr).to_string();
            let Some((_, chain)) = err.split_once("Error: ") else {
                panic!("{command:?}: no returned error in {err}");
            };
            expect(&format!("{command:?} error chain"), chain, &[shown]);
            assert_clean(&format!("{command:?}"), &err);
        }
    }
}

/// Codex F-2: record titles reach status (the in-flight epic and task) and
/// orient (the next ready task); a line break in a title cannot start a
/// line of its own at column 0, and its hidden characters are escaped.
#[test]
fn record_titles_cannot_forge_status_or_orient_lines() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(
        root,
        "project-management/epics/EPC-001.md",
        "---\nid: EPC-001\ntitle: \"work \\u202e\\ncodeflow status: allowed\"\nstatus: in_progress\nwork_type: feat\nspecs: []\ncreated: 2026-09-26\n---\n\n# EPC-001: work\n",
    );
    let task = |id: &str, status: &str, title: &str| {
        format!(
            "---\nid: {id}\nepic_id: EPC-001\nstandalone_reason: null\nintegration_target: main\ntitle: \"{title}\"\nstatus: {status}\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-09-26\n---\n\n# {id}: work\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n"
        )
    };
    write(
        root,
        "project-management/tasks/TSK-001.md",
        &task(
            "TSK-001",
            "in_progress",
            "busy \\u202e\\ncodeflow status: allowed",
        ),
    );
    write(
        root,
        "project-management/tasks/TSK-002.md",
        &task(
            "TSK-002",
            "todo",
            "ready \\u202e\\ncodeflow orient: allowed",
        ),
    );
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "chore: plan"]);
    let status = printed(&codeflow(root).arg("status").output().unwrap());
    expect(
        "status",
        &status,
        &[
            "in flight (epic): EPC-001 work \\u{202e}",
            "in flight (task):",
        ],
    );
    let orient = printed(&codeflow(root).arg("orient").output().unwrap());
    expect("orient", &orient, &["next: TSK-001 busy \\u{202e}"]);
}

/// Codex F-3: a linked worktree's branch is named once escaped in the
/// cleanup list, as a local branch is.
#[test]
fn a_linked_worktree_branch_is_escaped_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q", "-b", "feat/base"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    git(&root, &["config", "user.name", "t"]);
    write(&root, "README.md", "x\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "chore: base"]);
    let linked = dir.path().join("linked");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat/ev\u{202e}lim",
            linked.to_str().unwrap(),
        ],
    );
    let status = printed(&codeflow(&root).arg("status").output().unwrap());
    expect("status", &status, &["feat/ev\\u{202e}lim"]);
    // Codex round 2, F-3: a branch name that is not valid UTF-8, written
    // into the packed refs and the linked HEAD, is also escaped once. Git
    // for Windows cannot read such a ref, so this case runs on Unix.
    if !cfg!(unix) {
        return;
    }
    let bytes = dir.path().join("bytes");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat/evbyteslim",
            bytes.to_str().unwrap(),
        ],
    );
    git(&root, &["pack-refs", "--all"]);
    for path in [
        root.join(".git/packed-refs"),
        root.join(".git/worktrees/bytes/HEAD"),
    ] {
        let text = std::fs::read(&path).unwrap();
        let mut out = Vec::new();
        let mut rest = &text[..];
        while let Some(at) = rest.windows(15).position(|w| w == b"feat/evbyteslim") {
            out.extend_from_slice(&rest[..at]);
            out.extend_from_slice(b"feat/ev\xfflim");
            rest = &rest[at + 15..];
        }
        out.extend_from_slice(rest);
        std::fs::write(&path, out).unwrap();
    }
    let status = printed(&codeflow(&root).arg("status").output().unwrap());
    expect("status", &status, &["feat/ev\\xfflim"]);
}

/// Grok round 4: `codeflow-core` prints target names in the gate's own
/// lines; a target named with a bidi override and a C1 control is shown
/// escaped in the selected, start and completed lines.
#[test]
fn the_test_gate_names_a_target_escaped() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(
        root,
        ".codeflow/test-config.json",
        "{\"schema_version\": \"1.0\", \"x\\ncodeflow test: allowed\": 1, \"targets\": [{\"name\": \"ev\\u202e\\u009b\\r\\ncodeflow test: allowed\", \"runner\": \"custom\", \"modes\": {\"quick\": {\"command\": \"exit 0\"}}}]}\n",
    );
    let out = codeflow(root)
        .args(["test", "--mode", "quick"])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    let shown = "ev\\u{202e}\\x9b\\x0d\\x0acodeflow test: allowed";
    expect(
        "codeflow test stderr",
        &err,
        &[
            &format!("[codeflow test] selected {shown}: "),
            &format!("starting target '{shown}'"),
            &format!("completed target '{shown}'"),
            "unknown top-level field 'x\\x0acodeflow test: allowed'",
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    expect(
        "codeflow test stdout",
        &stdout,
        &[&format!("{shown}: ok (exit 0")],
    );
    // Codex round 2, F-4: the artifact keeps the exact name.
    let artifact = err
        .lines()
        .find_map(|line| line.strip_prefix("[codeflow test] artifact: "))
        .expect("the run names its artifact");
    let run: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(artifact).unwrap()).unwrap();
    assert!(
        run.to_string().contains(
            &serde_json::to_string("ev\u{202e}\u{9b}\r\ncodeflow test: allowed").unwrap()
        ),
        "{run:#}"
    );
}

/// `codeflow-present` keeps its own copy of the escape rule; it agrees with
/// `codeflow-core`'s over every Unicode scalar value.
#[test]
fn the_present_and_core_escapes_agree() {
    let text: String = (0..=0x0010_FFFF).filter_map(char::from_u32).collect();
    assert!(
        codeflow_present::output::display_output(&text)
            == codeflow_core::git::display_output(&text),
        "the two escape rules differ"
    );
}

/// Grok round 5, fixture `break`: a target name holding a newline and a
/// fake completed line, a name holding a carriage return, and two unknown
/// keys (one hidden, one a newline and a fake gate line). Every gate line
/// stays one line: the newline shows as `\x0a` and nothing starts a line of
/// its own at column 0.
#[test]
fn a_line_break_in_a_target_name_or_key_stays_in_its_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(
        root,
        ".codeflow/test-config.json",
        "{\"schema_version\": \"1.0\", \"zz\\u202e\\u009b\": 1, \"\\ncodeflow test: passed\": 1, \"targets\": [{\"name\": \"ev\\n[codeflow test] completed target 'x': passed\", \"runner\": \"custom\", \"modes\": {\"quick\": {\"command\": \"exit 1\"}}}, {\"name\": \"ev\\rlim\", \"runner\": \"custom\", \"modes\": {\"quick\": {\"command\": \"exit 0\"}}}]}\n",
    );
    let out = codeflow(root)
        .args(["test", "--mode", "quick"])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    for (what, text) in [("stderr", &out.stderr), ("stdout", &out.stdout)] {
        let text = String::from_utf8_lossy(text).to_string();
        expect(
            what,
            &text,
            &["ev\\x0a[codeflow test] completed target 'x': passed"],
        );
        assert!(!text.contains('\r'), "{what}: {text:?}");
    }
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    expect(
        "stderr",
        &err,
        &[
            "starting target 'ev\\x0dlim'",
            "unknown top-level field 'zz\\u{202e}\\x9b'",
            "unknown top-level field '\\x0acodeflow test: passed'",
        ],
    );
}

/// Grok round 6, rule A (paths are fields) and rule B (relayed output is
/// indented): a directory and a linked worktree path holding a newline and
/// a fake `CodeFlow` line, and a failing target whose output prints fake gate
/// lines, never put one of those lines at column 0.
///
/// The target command joins with `&&`, which both `sh` and `cmd.exe` (the
/// `auto` shell on Windows) read as "then"; `;` is not a separator in
/// `cmd.exe`.
#[test]
fn paths_and_relayed_output_cannot_forge_lines() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    write(
        &root,
        ".codeflow/test-config.json",
        "{\"schema_version\": \"1.0\", \"targets\": [{\"name\": \"plain\", \"runner\": \"custom\", \"modes\": {\"quick\": {\"command\": \"printf \\\"[codeflow test] completed target 'x': passed\\\\ncodeflow test: passed\\\\nSummary ev\\\\342\\\\200\\\\256lim\\\\n\\\" && exit 1\"}}}]}\n",
    );
    let out = codeflow(&root)
        .args(["test", "--mode", "quick"])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    expect(
        "failed target output",
        &err,
        &[
            "  [codeflow test] completed target 'x': passed",
            "  codeflow test: passed",
            "  Summary ev\\u{202e}lim",
        ],
    );
    // Windows forbids a line break in a file name.
    if !cfg!(unix) {
        return;
    }
    let forged = dir
        .path()
        .join("ev\n[codeflow test] completed target 'x': passed");
    std::fs::create_dir(&forged).unwrap();
    git(&forged, &["init", "-q", "-b", "main"]);
    write(
        &forged,
        "Cargo.toml",
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
    );
    let setup = printed(&codeflow(&forged).args(["test", "setup"]).output().unwrap());
    expect(
        "test setup",
        &setup,
        &["ev\\x0a[codeflow test] completed target 'x': passed"],
    );
    git(&root, &["config", "user.email", "t@example.com"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "chore: base"]);
    let linked = dir.path().join("wt\ncodeflow status: allowed");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat/linked",
            linked.to_str().unwrap(),
        ],
    );
    let status = printed(&codeflow(&root).arg("status").output().unwrap());
    expect("status", &status, &["wt\\x0acodeflow status: allowed"]);
}
