//! `codeflow test` gate guards and progress, driven through the real binary
//! (TSK-134, TSK-094): a start line per target on stderr, one full gate at a
//! time on a machine, and no cargo target directory outside the worktree.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git {args:?} failed");
}

/// A temp git repository with `config` as its test config.
fn repo(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(dir.path().join("README.md"), "hello\n").unwrap();
    git(dir.path(), &["add", "README.md"]);
    git(dir.path(), &["commit", "-q", "-m", "chore: initial commit"]);
    std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
    std::fs::write(dir.path().join(".codeflow/test-config.json"), config).unwrap();
    dir
}

fn command(dir: &Path, home: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_codeflow"));
    cmd.args(args)
        .current_dir(dir)
        .env("CODEFLOW_HOME", home)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CI");
    cmd
}

fn run(dir: &Path, home: &Path, args: &[&str]) -> Output {
    command(dir, home, args).output().expect("codeflow runs")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

/// Spawn with piped stderr, returning the child and a line receiver.
fn spawn(mut cmd: Command) -> (Child, Receiver<String>) {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("codeflow spawns");
    let err = child.stderr.take().unwrap();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    (child, rx)
}

/// Wait for a stderr line containing `needle`; panics after 20 s.
fn wait_for(rx: &Receiver<String>, needle: &str) -> String {
    loop {
        match rx.recv_timeout(Duration::from_secs(20)) {
            Ok(line) if line.contains(needle) => return line,
            Ok(_) => {}
            Err(e) => panic!("no stderr line containing {needle:?}: {e}"),
        }
    }
}

fn start(name: &str, mode: &str) -> String {
    format!("[codeflow test] starting target '{name}' ({mode} mode)")
}

const TWO_SLEEPERS: &str = r#"{"schema_version": "1.0", "targets": [
  {"name": "first", "runner": "custom", "modes": {"full": {"command": "sleep 1"}}},
  {"name": "second", "runner": "custom", "modes": {"full": {"command": "sleep 1"}}}
]}"#;

#[cfg(unix)]
#[test]
fn sequential_start_lines_precede_each_target_and_stdout_is_unchanged() {
    let dir = repo(TWO_SLEEPERS);
    let home = tempfile::tempdir().unwrap();
    let (mut child, rx) = spawn(command(dir.path(), home.path(), &["test"]));

    assert_eq!(wait_for(&rx, "'first'"), start("first", "full"));
    assert!(
        child.try_wait().unwrap().is_none(),
        "the start line arrived while the first target was still running"
    );
    assert_eq!(wait_for(&rx, "'second'"), start("second", "full"));
    assert!(
        child.try_wait().unwrap().is_none(),
        "the start line arrived before the second target finished"
    );

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "stdout: {stdout}");
    assert!(lines[0].starts_with("first: ok (exit 0, "), "{stdout}");
    assert!(lines[1].starts_with("second: ok (exit 0, "), "{stdout}");
    assert_eq!(lines[2], "test gate: passed (2 target(s))");
    assert!(
        !stdout.contains("[codeflow test]"),
        "start lines stay off stdout"
    );
}

#[cfg(unix)]
#[test]
fn parallel_start_lines_are_whole_lines() {
    let config = TWO_SLEEPERS.replace(
        r#""schema_version": "1.0","#,
        r#""schema_version": "1.0", "execution": {"parallel": true},"#,
    );
    let dir = repo(&config);
    let home = tempfile::tempdir().unwrap();
    let output = run(dir.path(), home.path(), &["test"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let err = stderr(&output);
    let mut starts: Vec<&str> = err
        .lines()
        .filter(|l| l.contains("starting target"))
        .collect();
    starts.sort_unstable();
    assert_eq!(
        starts,
        vec![start("first", "full"), start("second", "full")],
        "{err}"
    );
}

#[cfg(unix)]
#[test]
fn a_killed_gate_has_already_named_its_running_target() {
    let dir = repo(
        r#"{"schema_version": "1.0", "targets": [
  {"name": "sleeper", "runner": "custom", "modes": {"full": {"command": "sleep 5"}}}
]}"#,
    );
    let home = tempfile::tempdir().unwrap();
    let (mut child, rx) = spawn(command(dir.path(), home.path(), &["test"]));
    let named = wait_for(&rx, "starting target");
    assert_eq!(named, start("sleeper", "full"));
    assert!(child.try_wait().unwrap().is_none(), "still running");
    child.kill().unwrap();
    child.wait().unwrap();
    let rest: Vec<String> = rx.try_iter().collect();
    // The line was read from the partial stderr of a killed process.
    assert!(rest.iter().all(|l| !l.contains("test gate:")), "{rest:?}");
}

#[test]
fn skipped_and_disabled_targets_print_no_start_line() {
    let dir = repo(
        r#"{"schema_version": "1.0", "targets": [
  {"name": "runs", "runner": "custom", "modes": {"full": {"command": "exit 0"}}},
  {"name": "off", "runner": "custom", "enabled": false, "modes": {"full": {"command": "exit 0"}}},
  {"name": "ci-only-skip", "runner": "custom", "ci_skip": true, "ci_skip_reason": "local only",
   "modes": {"full": {"command": "exit 0"}}},
  {"name": "quick-only", "runner": "custom", "modes": {"quick": {"command": "exit 0"}}}
]}"#,
    );
    let home = tempfile::tempdir().unwrap();
    let output = command(dir.path(), home.path(), &["test"])
        .env("CI", "true")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let err = stderr(&output);
    let starts: Vec<&str> = err
        .lines()
        .filter(|l| l.contains("starting target"))
        .collect();
    assert_eq!(starts, vec![start("runs", "full")], "{err}");
}

const HOLDER: &str = r#"{"schema_version": "1.0", "targets": [
  {"name": "holder", "runner": "custom", "modes": {"full": {"command": "sleep 3"}}}
]}"#;
const QUICK_PASS: &str = r#"{"schema_version": "1.0", "targets": [
  {"name": "pass", "runner": "custom", "modes": {"full": {"command": "exit 0"}}}
]}"#;

#[cfg(unix)]
#[test]
fn a_second_full_gate_is_refused_naming_the_holder() {
    let home = tempfile::tempdir().unwrap();
    let first = repo(HOLDER);
    let (child, rx) = spawn(command(
        first.path(),
        home.path(),
        &["test", "--mode", "full"],
    ));
    // The lock is taken before any target starts.
    wait_for(&rx, "starting target 'holder'");

    // Another repository on the same machine: the machine-wide lock refuses.
    let other = repo(QUICK_PASS);
    let refused = run(other.path(), home.path(), &["test", "--mode", "full"]);
    let err = stderr(&refused);
    assert_eq!(refused.status.code(), Some(1), "{err}");
    assert!(err.contains("another full gate is running"), "{err}");
    assert!(err.contains(&format!("pid {}", child.id())), "{err}");
    assert!(!err.contains("starting target"), "no target ran: {err}");

    // The same repository from a sandbox without the home lock: the
    // repository lock under the git common dir still refuses.
    let other_home = tempfile::tempdir().unwrap();
    let same_repo = run(first.path(), other_home.path(), &["test"]);
    assert_eq!(same_repo.status.code(), Some(1), "{}", stderr(&same_repo));

    // Quick mode takes no lock.
    let quick = repo(
        r#"{"schema_version": "1.0", "targets": [
  {"name": "q", "runner": "custom", "modes": {"quick": {"command": "exit 0"}}}]}"#,
    );
    let q = run(quick.path(), home.path(), &["test", "--mode", "quick"]);
    assert_eq!(q.status.code(), Some(0), "{}", stderr(&q));

    let done = child.wait_with_output().unwrap();
    assert_eq!(done.status.code(), Some(0));
    let after = run(other.path(), home.path(), &["test", "--mode", "full"]);
    assert_eq!(after.status.code(), Some(0), "{}", stderr(&after));
}

#[test]
fn a_stale_lock_from_a_dead_gate_is_reclaimed() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join("locks")).unwrap();
    std::fs::write(
        home.path().join("locks/full-gate.lock"),
        "pid=999999\nstarted=1\ndir=/gone\n",
    )
    .unwrap();
    let dir = repo(QUICK_PASS);
    let output = run(dir.path(), home.path(), &["test"]);
    let err = stderr(&output);
    assert_eq!(output.status.code(), Some(0), "{err}");
    assert!(
        err.contains("reclaimed a stale gate lock") && err.contains("pid 999999 in /gone"),
        "{err}"
    );
}

const CARGO_TARGET: &str = r#"{"schema_version": "1.0", "targets": [
  {"name": "rust", "runner": "custom", "modes": {"full": {"command": "cargo --version"},
   "quick": {"command": "cargo --version"}}}
]}"#;

#[test]
fn a_cargo_target_dir_outside_the_worktree_is_refused() {
    let dir = repo(CARGO_TARGET);
    let home = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    for mode in ["full", "quick"] {
        let refused = command(dir.path(), home.path(), &["test", "--mode", mode])
            .env("CARGO_TARGET_DIR", elsewhere.path())
            .output()
            .unwrap();
        let err = stderr(&refused);
        assert_eq!(refused.status.code(), Some(1), "{mode}: {err}");
        assert!(err.contains("never shared between worktrees"), "{err}");
        assert!(err.contains("CARGO_TARGET_DIR="), "{err}");
        assert!(!err.contains("starting target"), "no target ran: {err}");
    }

    let inside = command(dir.path(), home.path(), &["test"])
        .env("CARGO_TARGET_DIR", dir.path().join("target"))
        .output()
        .unwrap();
    assert_eq!(inside.status.code(), Some(0), "{}", stderr(&inside));

    // A gate that runs no cargo is not refused.
    let no_cargo = repo(QUICK_PASS);
    let other = command(no_cargo.path(), home.path(), &["test"])
        .env("CARGO_TARGET_DIR", elsewhere.path())
        .output()
        .unwrap();
    assert_eq!(other.status.code(), Some(0), "{}", stderr(&other));
}
