//! `codeflow test` gate guards and progress, driven through the real binary
//! (TSK-134, TSK-094): a start line per target on stderr, one full gate at a
//! time on a machine, and a warning for a cargo target directory outside the
//! worktree.

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

#[cfg(unix)]
#[test]
fn a_killed_gate_keeps_its_lock_until_its_target_exits() {
    let home = tempfile::tempdir().unwrap();
    // The target records its process group (the shell's pid leads it), then
    // outlives the gate that started it.
    let first = repo(
        r#"{"schema_version": "1.0", "targets": [
  {"name": "survivor", "runner": "custom",
   "modes": {"full": {"command": "echo $$ > group.txt; sleep 30"}}}
]}"#,
    );
    let (mut gate, rx) = spawn(command(
        first.path(),
        home.path(),
        &["test", "--mode", "full"],
    ));
    wait_for(&rx, "starting target 'survivor'");
    let group_file = first.path().join("group.txt");
    let group = (0..200)
        .find_map(|_| {
            let text = std::fs::read_to_string(&group_file).unwrap_or_default();
            let parsed = text.trim().parse::<u32>().ok();
            if parsed.is_none() {
                std::thread::sleep(Duration::from_millis(50));
            }
            parsed
        })
        .expect("the target wrote its group");

    gate.kill().unwrap();
    gate.wait().unwrap();
    let alive = |g: u32| {
        Command::new("kill")
            .args(["-0", "--", &format!("-{g}")])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    };
    assert!(alive(group), "the target outlived its gate");

    // Its gate process is gone, so the OS released the lock, but its target
    // still runs: a second full gate refuses, from any repository.
    let other = repo(QUICK_PASS);
    for dir in [other.path(), first.path()] {
        let refused = run(dir, home.path(), &["test", "--mode", "full"]);
        let err = stderr(&refused);
        assert_eq!(refused.status.code(), Some(1), "{err}");
        assert!(err.contains("targets are still running"), "{err}");
        assert!(err.contains(&format!("process group {group}")), "{err}");
        assert!(!err.contains("starting target"), "no target ran: {err}");
    }

    // Once the group exits, the lock is stale and is reclaimed.
    Command::new("kill")
        .args(["-KILL", "--", &format!("-{group}")])
        .status()
        .unwrap();
    let gone = (0..100).any(|_| {
        let alive_now = alive(group);
        if alive_now {
            std::thread::sleep(Duration::from_millis(50));
        }
        !alive_now
    });
    assert!(gone, "the target group exited");
    let after = run(other.path(), home.path(), &["test", "--mode", "full"]);
    let err = stderr(&after);
    assert_eq!(after.status.code(), Some(0), "{err}");
    assert!(err.contains("reclaimed a stale gate lock"), "{err}");
}

/// A long target whose recorded pid is the one to watch: on Unix the shell
/// that leads the target's process group, on Windows the target's
/// grandchild (`cmd.exe` runs `powershell`), so its end shows the tree ended.
#[cfg(unix)]
const LONG_TARGET: &str = r#"{"schema_version": "1.0", "targets": [
  {"name": "long", "runner": "custom",
   "modes": {"full": {"command": "echo $$ > pid.txt; sleep 30"}}}
]}"#;
#[cfg(windows)]
const LONG_TARGET: &str = r#"{"schema_version": "1.0", "targets": [
  {"name": "long", "runner": "custom",
   "modes": {"full": {"command": "powershell -NoProfile -NonInteractive -Command \"Set-Content -Path pid.txt -Value $PID; Start-Sleep -Seconds 120\""}}}
]}"#;

/// Whether the process (Windows) or process group (Unix) `pid` still runs.
fn still_runs(pid: u32) -> bool {
    if cfg!(windows) {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .expect("tasklist runs");
        String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
    } else {
        Command::new("kill")
            .args(["-0", "--", &format!("-{pid}")])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    }
}

/// TSK-142 AC-5 (journey): in a freshly scaffolded project driven by the
/// real binary, a full gate killed during a long target never lets a second
/// full gate run beside that target. On Windows the target's tree ends with
/// the gate; on Unix the lock stays held until the target's process group
/// exits (TSK-134). The next full gate then takes the lock.
#[test]
fn a_killed_full_gate_never_runs_beside_its_target_in_a_fresh_scaffold() {
    let home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    let init = run(&root, home.path(), &["init", "--yes", "--minimal"]);
    assert!(init.status.success(), "init: {}", stderr(&init));
    std::fs::write(root.join(".codeflow/test-config.json"), LONG_TARGET).unwrap();

    let (mut gate, rx) = spawn(command(&root, home.path(), &["test", "--mode", "full"]));
    wait_for(&rx, "starting target 'long'");
    let pid_file = root.join("pid.txt");
    let pid = (0..1200)
        .find_map(|_| {
            let text = std::fs::read_to_string(&pid_file).unwrap_or_default();
            let parsed = text.trim().parse::<u32>().ok();
            if parsed.is_none() {
                std::thread::sleep(Duration::from_millis(50));
            }
            parsed
        })
        .expect("the target recorded its pid");
    assert!(still_runs(pid), "the target runs under its gate");
    gate.kill().unwrap();
    gate.wait().unwrap();

    let other = repo(QUICK_PASS);
    if cfg!(unix) {
        // The target outlives its gate, and no second gate runs beside it.
        assert!(still_runs(pid), "the target outlived its gate");
        let refused = run(other.path(), home.path(), &["test", "--mode", "full"]);
        let err = stderr(&refused);
        assert_eq!(refused.status.code(), Some(1), "{err}");
        assert!(err.contains("targets are still running"), "{err}");
        assert!(!err.contains("starting target"), "no target ran: {err}");
        Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .status()
            .unwrap();
    }
    // Windows: the killed gate took its target's tree with it. Unix: the
    // group was just stopped. Either way it ends.
    let gone = (0..200).any(|_| {
        let alive = still_runs(pid);
        if alive {
            std::thread::sleep(Duration::from_millis(50));
        }
        !alive
    });
    assert!(gone, "the target ended");

    // The next full gate takes the lock, from the scaffold and elsewhere.
    std::fs::write(root.join(".codeflow/test-config.json"), QUICK_PASS).unwrap();
    for dir in [root.as_path(), other.path()] {
        let after = run(dir, home.path(), &["test", "--mode", "full"]);
        let err = stderr(&after);
        assert_eq!(after.status.code(), Some(0), "{err}");
        assert!(err.contains("starting target 'pass'"), "{err}");
    }
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
fn a_cargo_target_dir_outside_the_worktree_warns_and_runs() {
    let dir = repo(CARGO_TARGET);
    let home = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    for mode in ["full", "quick"] {
        let warned = command(dir.path(), home.path(), &["test", "--mode", mode])
            .env("CARGO_TARGET_DIR", elsewhere.path())
            .output()
            .unwrap();
        let err = stderr(&warned);
        assert_eq!(warned.status.code(), Some(0), "{mode}: {err}");
        assert!(
            err.contains("codeflow test: warning: CARGO_TARGET_DIR="),
            "{err}"
        );
        assert!(err.contains("in parallel"), "{err}");
        assert!(
            err.contains("starting target 'rust'"),
            "the gate still ran: {err}"
        );
    }

    let inside = command(dir.path(), home.path(), &["test"])
        .env("CARGO_TARGET_DIR", dir.path().join("target"))
        .output()
        .unwrap();
    assert_eq!(inside.status.code(), Some(0), "{}", stderr(&inside));
    assert!(!stderr(&inside).contains("warning: CARGO_TARGET_DIR"));

    // A gate that runs no cargo does not warn.
    let no_cargo = repo(QUICK_PASS);
    let other = command(no_cargo.path(), home.path(), &["test"])
        .env("CARGO_TARGET_DIR", elsewhere.path())
        .output()
        .unwrap();
    assert_eq!(other.status.code(), Some(0), "{}", stderr(&other));
    assert!(!stderr(&other).contains("warning: CARGO_TARGET_DIR"));
}
