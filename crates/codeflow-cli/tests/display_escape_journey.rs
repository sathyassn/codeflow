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
            || line.starts_with("ok  test-config: healthy")
            || line.starts_with("ok  permissions: verified")
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

/// Runs `cmd` and checks the exit code and the output: no forged line at
/// column 0, and each `expected` text.
fn row(what: &str, cmd: &mut Command, code: i32, expected: &[&str]) {
    let out = cmd.output().unwrap();
    let text = printed(&out);
    assert_eq!(
        out.status.code(),
        Some(code),
        "{what}: exit changed: {text}"
    );
    expect(what, &text, expected);
}

/// TSK-388 round 7, the sink rule at work: Codex's two doctor reproductions
/// (a newline in a checkout path and in `CODEFLOW_HOME`) and Grok's four
/// printers (the coverage note, a stale gate lock note, the shared
/// `CARGO_TARGET_DIR` warning, the scaffold version warning). Each value
/// holds a line break and a fake result line; none starts a line at column
/// 0, and each command exits as it did before.
#[test]
fn doctor_and_gate_notes_cannot_forge_lines() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();

    // Grok: the scaffold version from a checkout's project settings.
    let version = base.join("version");
    std::fs::create_dir(&version).unwrap();
    git(&version, &["init", "-q", "-b", "main"]);
    write(
        &version,
        ".codeflow/project.toml",
        "schema_version = 1\ntier = \"minimal\"\nscaffold_version = \"0.0.1\\ncodeflow test: passed\"\nstack = \"generic\"\nareas = []\npolicy_armed = true\ngit_hooks = \"unwired\"\npermission_preset = \"default\"\n",
    );
    row(
        "scaffold version",
        codeflow(&version).args(["status"]),
        0,
        &["scaffold 0.0.1\\x0acodeflow test: passed is behind"],
    );

    // Grok: the coverage note names a target whose name holds a newline.
    let coverage = base.join("coverage");
    std::fs::create_dir(&coverage).unwrap();
    git(&coverage, &["init", "-q", "-b", "main"]);
    write(
        &coverage,
        ".codeflow/test-config.json",
        "{\"schema_version\": \"1.0\", \"targets\": [{\"name\": \"ev\\ncodeflow test: passed\", \"runner\": \"custom\", \"coverage\": {\"format\": \"lcov\", \"path\": \"missing.info\"}, \"modes\": {\"full\": {\"command\": \"exit 0\"}}}]}\n",
    );
    row(
        "coverage note",
        codeflow(&coverage).args(["test", "--mode", "full"]),
        0,
        &["  ev\\x0acodeflow test: passed: "],
    );

    // Windows forbids a line break in a file name.
    if !cfg!(unix) {
        return;
    }

    // Codex: a checkout path in the test-config check, which the nested
    // error names again.
    let checkout = base.join("ev\nok  test-config: healthy\nend");
    std::fs::create_dir(&checkout).unwrap();
    git(&checkout, &["init", "-q", "-b", "main"]);
    write(&checkout, ".codeflow/test-config.json", "{");
    row(
        "doctor test-config",
        codeflow(&checkout).args(["doctor", "--check", "test-config"]),
        1,
        &["ev\\x0aok  test-config: healthy\\x0aend"],
    );

    // Codex: `CODEFLOW_HOME` a regular file whose name holds the fake line.
    let plain = base.join("plain");
    std::fs::create_dir(&plain).unwrap();
    git(&plain, &["init", "-q", "-b", "main"]);
    let home = base.join("state\nok  permissions: verified\nend");
    std::fs::write(&home, "").unwrap();
    row(
        "doctor permissions",
        codeflow(&plain)
            .args(["doctor", "--check", "permissions"])
            .env("CODEFLOW_HOME", &home),
        0,
        &["state\\x0aok  permissions: verified\\x0aend"],
    );

    // Grok: a stale gate lock in a repository whose path holds the fake
    // line, and the shared `CARGO_TARGET_DIR` warning naming that path.
    let forged =
        base.join("ev\n[codeflow test] completed target 'x': passed\ncodeflow test: passed");
    std::fs::create_dir(&forged).unwrap();
    git(&forged, &["init", "-q", "-b", "main"]);
    write(
        &forged,
        ".codeflow/test-config.json",
        "{\"schema_version\": \"1.0\", \"targets\": [{\"name\": \"build\", \"runner\": \"custom\", \"modes\": {\"quick\": {\"command\": \"cargo --version\"}, \"full\": {\"command\": \"exit 0\"}}}]}\n",
    );
    write(
        &forged,
        ".git/codeflow/full-gate.lock",
        "pid=4194305\ndir=/tmp\nstarted=1\n",
    );
    row(
        "stale gate lock",
        codeflow(&forged).args(["test", "--mode", "full"]),
        0,
        &["reclaimed a stale gate lock at "],
    );
    row(
        "shared cargo target",
        codeflow(&forged)
            .args(["test", "--mode", "quick"])
            .env("CARGO_TARGET_DIR", base.join("cargodir")),
        0,
        &["is shared outside this worktree ("],
    );
}

/// Each Rust source file under `dir`, cut at its first inline
/// `#[cfg(test)]` module, with its path relative to the workspace.
fn sources(dir: &Path, workspace: &Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, workspace, out);
            continue;
        }
        let name = path.to_string_lossy().to_string();
        if path.extension().is_none_or(|ext| ext != "rs") || name.ends_with("tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        // Cut at the first inline test module (`mod x {`), not at a
        // `mod x;` declaration.
        let cut = text
            .match_indices("#[cfg(test)]\nmod ")
            .find_map(|(at, _)| {
                let line = text[at..].lines().nth(1).unwrap_or_default();
                line.trim_end().ends_with('{').then_some(at)
            });
        let text = cut.map_or(text.as_str(), |cut| &text[..cut]).to_string();
        let rel = path
            .strip_prefix(workspace)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        out.push((rel, text));
    }
}

/// TSK-388 round 7, the sink by construction: every print shadow applies
/// `display_print`; the standard print macros appear only inside those
/// shadows and the block printers; no CLI terminal writer uses a bare
/// `write!` or `writeln!`; and the block printers are used only where
/// listed. A new printer has to use the rule or be added here for review.
#[test]
fn print_sinks_apply_the_line_rule_by_construction() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workspace = workspace.canonicalize().unwrap();
    let mut files = Vec::new();
    for krate in ["codeflow-cli", "codeflow-core", "codeflow-present"] {
        sources(
            &workspace.join("crates").join(krate).join("src"),
            &workspace,
            &mut files,
        );
    }
    let text = |rel: &str| {
        files
            .iter()
            .find(|(path, _)| path == rel)
            .map(|(_, text)| text.clone())
            .unwrap()
    };
    for (rel, shadows) in [
        // The four shadows and `term_write!`.
        ("crates/codeflow-cli/src/main.rs", 5),
        ("crates/codeflow-core/src/lib.rs", 3),
        ("crates/codeflow-present/src/lib.rs", 1),
    ] {
        let source = text(rel);
        assert_eq!(
            source
                .matches("display_print(&::std::format!($($arg)*))")
                .count(),
            shadows,
            "{rel}: a print shadow does not apply display_print"
        );
        assert!(!source.contains("display_output(&::std::format!"), "{rel}");
    }
    let mut raw = std::collections::BTreeMap::new();
    let mut blocks = std::collections::BTreeMap::new();
    let mut bare_writers = Vec::new();
    for (rel, source) in &files {
        for macro_name in [
            "::std::print!",
            "::std::println!",
            "::std::eprint!",
            "::std::eprintln!",
        ] {
            let count = source.matches(macro_name).count();
            if count > 0 {
                *raw.entry(rel.clone()).or_insert(0) += count;
            }
        }
        let count =
            source.matches("print_block!(").count() + source.matches("println_block!(").count();
        if count > 0 {
            blocks.insert(rel.clone(), count);
        }
        if rel.starts_with("crates/codeflow-cli/") {
            for writer in ["writeln!(out", "write!(out", "writeln!(err", "write!(err"] {
                // `term_writeln!(out` is the rule's writer, not a bare one.
                let bare = source.match_indices(writer).any(|(at, _)| {
                    !source[..at]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_')
                });
                if bare {
                    bare_writers.push(format!("{rel}: {writer}"));
                }
            }
        }
    }
    let expected_raw: std::collections::BTreeMap<String, usize> = [
        ("crates/codeflow-cli/src/main.rs", 8),
        ("crates/codeflow-core/src/lib.rs", 5),
        ("crates/codeflow-present/src/lib.rs", 1),
    ]
    .into_iter()
    .map(|(rel, count)| (rel.to_string(), count))
    .collect();
    assert_eq!(
        raw, expected_raw,
        "a standard print macro outside the shadows"
    );
    assert!(
        bare_writers.is_empty(),
        "bare terminal writers: {bare_writers:?}"
    );
    // Block printers take only a `TextBlock`: a report built one line per
    // `writeln!` under the rule, or JSON from serde.
    assert_eq!(
        blocks,
        listed_block_printers(),
        "block printer sites changed; review each"
    );
}

/// The files that print a block, and how many times each does.
fn listed_block_printers() -> std::collections::BTreeMap<String, usize> {
    [
        ("crates/codeflow-cli/src/cmd/delegate.rs", 1),
        ("crates/codeflow-cli/src/cmd/estimate.rs", 3),
        ("crates/codeflow-cli/src/cmd/feedback.rs", 1),
        ("crates/codeflow-cli/src/cmd/integrate.rs", 1),
        ("crates/codeflow-cli/src/cmd/models.rs", 1),
        ("crates/codeflow-cli/src/cmd/orient.rs", 1),
        ("crates/codeflow-cli/src/cmd/portal.rs", 1),
        ("crates/codeflow-cli/src/cmd/present.rs", 5),
        ("crates/codeflow-cli/src/cmd/remote.rs", 1),
        ("crates/codeflow-cli/src/cmd/report.rs", 1),
        ("crates/codeflow-cli/src/cmd/status.rs", 2),
        ("crates/codeflow-cli/src/cmd/work.rs", 1),
        ("crates/codeflow-cli/src/main.rs", 4),
    ]
    .into_iter()
    .map(|(rel, count)| (rel.to_string(), count))
    .collect()
}
