//! The ledger is an aid, never a control (TSK-422 AC-8, ADR-0086): `ci`,
//! the pre-push hook, `task status`, `work claim`, `work start` and
//! `integrate` give the same verdict, byte for byte, with the coordination
//! ledger absent, present and damaged; and none of their sources names the
//! coordination module, while the coordination module writes only under
//! its own directory.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use codeflow_core::coord::fixture;
use codeflow_core::coord::store::Paths;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn task(id: &str) -> String {
    let uid = if id == "TSK-001" {
        "6f0c2a1e-1d2b-4c3d-8e4f-5a6b7c8d9e01"
    } else {
        "6f0c2a1e-1d2b-4c3d-8e4f-5a6b7c8d9e02"
    };
    format!("---\nid: {id}\nuid: {uid}\nepic_id: null\nstandalone_reason: bounded outcome\nintegration_target: main\ntitle: work {id}\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-10-01\n---\n\n## Description\nWork.\n\n## Acceptance Criteria\n- AC-1 When run, the command shall succeed. (journey)\n")
}

#[derive(Clone, Copy, Debug)]
enum Ledger {
    Absent,
    Present,
    Damaged,
}

/// A fresh project with a remote, a task branch with one commit, and the
/// ledger in the given state.
fn project(state: Ledger) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    let origin = dir.path().join("o.git");
    std::fs::create_dir(&root).unwrap();
    git(
        dir.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@example.test"]);
    write(
        &root,
        "project-management/tasks/TSK-001.md",
        &task("TSK-001"),
    );
    write(
        &root,
        "project-management/tasks/TSK-002.md",
        &task("TSK-002"),
    );
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "docs: plan tasks"]);
    git(
        &root,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&root, &["push", "-q", "-u", "origin", "main"]);
    git(&root, &["checkout", "-q", "-b", "task/TSK-001-work"]);
    write(&root, "src.txt", "work\n");
    git(&root, &["add", "src.txt"]);
    git(&root, &["commit", "-qm", "feat: do the work"]);
    let paths = Paths::discover(&root).unwrap().0;
    match state {
        Ledger::Absent => {}
        Ledger::Present | Ledger::Damaged => {
            fixture::open_stream(&paths, "WS-001", "c1").unwrap();
            fixture::open_lane(&paths, "WS-001", "c1", "s1").unwrap();
        }
    }
    if let Ledger::Damaged = state {
        let mut log = std::fs::read_to_string(&paths.events).unwrap();
        log.insert_str(0, "{not json}\n");
        log.push_str("{\"torn\":");
        std::fs::write(&paths.events, log).unwrap();
    }
    (dir, root)
}

fn run(root: &Path, args: &[&str], stdin: Option<String>) -> (Option<i32>, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_codeflow"));
    cmd.current_dir(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.env_remove("CLAUDECODE")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_CI")
        .env_remove("GROK_SESSION_ID");
    let mut child = cmd.spawn().unwrap();
    if let Some(stdin) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
    } else {
        drop(child.stdin.take());
    }
    let out: Output = child.wait_with_output().unwrap();
    // Paths differ between fixtures; the verdict and messages must not.
    let parent = root.parent().unwrap().to_string_lossy().into_owned();
    let canonical = root
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let text = format!(
        "{}\n--stderr--\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .replace(&canonical, "<dir>")
    .replace(&parent, "<dir>");
    // Windows prints a path with forward slashes, and canonical paths there
    // carry the verbatim prefix: compare those spellings too.
    let slashed = |path: &str| path.trim_start_matches(r"\\?\").replace('\\', "/");
    let text = text
        .replace(&slashed(&canonical), "<dir>")
        .replace(&slashed(&parent), "<dir>");
    let text = regex::Regex::new("[0-9a-f]{40}")
        .unwrap()
        .replace_all(&text, "<sha>")
        .into_owned();
    let text = regex::Regex::new("[0-9]+(\\.[0-9]+)?m?s\\b")
        .unwrap()
        .replace_all(&text, "<time>")
        .into_owned();
    let text = regex::Regex::new("\\b[0-9a-f]{7,12}\\b")
        .unwrap()
        .replace_all(&text, "<short>")
        .into_owned();
    (out.status.code(), text)
}

type Step = fn(&Path) -> (Option<i32>, String);

fn steps() -> Vec<(&'static str, Step)> {
    vec![
        ("ci", |root| run(root, &["ci"], None)),
        ("pre-push", |root| {
            let head = git(root, &["rev-parse", "HEAD"]);
            let line = format!(
                "refs/heads/task/TSK-001-work {head} refs/heads/task/TSK-001-work {}\n",
                "0".repeat(40)
            );
            let url = git(root, &["remote", "get-url", "origin"]);
            run(root, &["git-hook", "pre-push", "origin", &url], Some(line))
        }),
        ("task status", |root| {
            run(
                root,
                &[
                    "task",
                    "status",
                    "TSK-002",
                    "blocked",
                    "--reason",
                    "waits",
                    "--owner",
                    "op",
                    "--revisit",
                    "later",
                ],
                None,
            )
        }),
        ("work claim", |root| {
            run(root, &["work", "claim", "TSK-002"], None)
        }),
        ("work start", |root| {
            run(root, &["work", "start", "TSK-001"], None)
        }),
        ("integrate", |root| {
            git(root, &["checkout", "-q", "main"]);
            run(
                root,
                &["integrate", "task/TSK-001-work", "--into", "main"],
                None,
            )
        }),
    ]
}

#[test]
fn every_gate_gives_the_same_verdict_with_the_ledger_absent_present_or_damaged() {
    for (name, step) in steps() {
        let verdicts: Vec<(Option<i32>, String)> =
            [Ledger::Absent, Ledger::Present, Ledger::Damaged]
                .into_iter()
                .map(|state| {
                    let (_dir, root) = project(state);
                    step(&root)
                })
                .collect();
        assert_eq!(
            verdicts[0], verdicts[1],
            "{name}: absent and present differ"
        );
        assert_eq!(
            verdicts[0], verdicts[2],
            "{name}: absent and damaged differ"
        );
    }
}

fn source(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel)).unwrap()
}

/// The code search behind the guarantee: the gate commands never name the
/// coordination module, and the coordination module's only writer is its
/// store, whose paths are all under the ledger directory.
#[test]
fn no_gate_reads_the_ledger_and_the_ledger_writes_only_its_own_files() {
    for gate in [
        "codeflow-cli/src/cmd/ci.rs",
        "codeflow-cli/src/cmd/git_hook.rs",
        "codeflow-cli/src/cmd/integrate.rs",
        "codeflow-cli/src/cmd/new.rs",
        "codeflow-core/src/hooks/git_hook.rs",
        "codeflow-core/src/integrate.rs",
        "codeflow-core/src/workgraph/work_start.rs",
        "codeflow-core/src/workgraph/readiness.rs",
        "codeflow-core/src/workgraph/status_verb.rs",
    ] {
        assert!(
            !source(gate).contains("coord::"),
            "{gate} names the coordination module"
        );
    }
    let work = source("codeflow-cli/src/cmd/work.rs");
    assert!(
        !work.contains("coord::"),
        "work.rs names the coordination module"
    );
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codeflow-core/src/coord");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let writes = [
            "fs::write",
            "atomic_write",
            "OpenOptions",
            "create_dir",
            "remove_file",
            "rename(",
        ]
        .iter()
        .any(|call| text.contains(call));
        if name == "store.rs" {
            assert!(
                !text.contains("\"project-management") && !text.contains("\"docs"),
                "store.rs names a record path"
            );
        } else {
            assert!(!writes, "{name} writes; only store.rs may");
        }
    }
}
