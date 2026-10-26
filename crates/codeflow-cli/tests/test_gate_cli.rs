//! `codeflow test` gate guards and progress, driven through the real binary
//! (TSK-134, TSK-094): a start line per target on stderr, one full gate at a
//! time on a machine, and a warning for a cargo target directory outside the
//! worktree.

use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::Receiver;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

#[path = "support/drain_pipe.rs"]
mod drain_pipe;
#[path = "test_gate_cli/evidence.rs"]
mod evidence;
#[path = "support/own_repository.rs"]
mod own_repository;
use drain_pipe::{drain_lines, drain_pipe};

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
    git(dir.path(), &["config", "user.name", "Test"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    std::fs::write(dir.path().join("README.md"), "hello\n").unwrap();
    git(dir.path(), &["add", "README.md"]);
    git(dir.path(), &["commit", "-q", "-m", "chore: initial commit"]);
    std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
    std::fs::write(dir.path().join(".codeflow/test-config.json"), config).unwrap();
    dir
}

fn command(dir: &Path, home: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(
        std::env::var_os("CODEFLOW_TEST_GATE_BIN")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_codeflow").into()),
    );
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

#[test]
fn default_comparison_names_the_task_line_and_explicit_controls_override_it() {
    let dir = repo(
        r#"{"schema_version":"1.0","targets":[{"name":"check","runner":"custom","modes":{"quick":{"command":"echo checked"}}}]}"#,
    );
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("project-management/tasks")).unwrap();
    std::fs::write(
        dir.path().join("project-management/tasks/TSK-001.md"),
        "---\nid: TSK-001\nintegration_target: integration/test-line\n---\n",
    )
    .unwrap();
    std::fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "test: declare task line"]);
    git(dir.path(), &["branch", "integration/test-line"]);
    git(dir.path(), &["switch", "-c", "task/TSK-001-selection"]);
    let base = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let base = String::from_utf8(base.stdout).unwrap();
    let output = run(dir.path(), home.path(), &["test", "--mode", "quick"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains(base.trim()), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("TSK-001 integration target integration/test-line"),
        "{}",
        stderr(&output)
    );
    let output = run(
        dir.path(),
        home.path(),
        &["test", "--mode", "quick", "--since", "HEAD"],
    );
    assert!(
        stderr(&output).contains("explicit --since HEAD"),
        "{}",
        stderr(&output)
    );
    let output = run(
        dir.path(),
        home.path(),
        &["test", "--mode", "quick", "--all"],
    );
    assert!(
        stderr(&output).contains("--all: every target"),
        "{}",
        stderr(&output)
    );
}

/// What a spawned gate has written to stdout, read to the end on its own
/// thread (`join` returns the bytes once the gate has exited).
type Stdout = std::thread::JoinHandle<Vec<u8>>;

/// Spawn with both outputs piped and drained while the gate runs, so a gate
/// whose output outgrows a small pipe buffer is never stalled by the test
/// (issue 100). Returns the child, a receiver of its stderr lines and its
/// stdout.
fn spawn(mut cmd: Command) -> (Child, Receiver<String>, Stdout) {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("codeflow spawns");
    let stdout = drain_pipe(child.stdout.take());
    let rx = drain_lines(child.stderr.take());
    (child, rx, stdout)
}

/// The helper that every gate test spawns through reads both streams while
/// the child runs: a child that writes a mebibyte to each, far past any pipe
/// buffer, still exits. With a reader missing, it blocks in `write` and the
/// bound below fails the test (issue 100).
#[cfg(unix)]
#[test]
fn the_spawn_helper_drains_both_streams_past_a_pipe_buffer() {
    let mut cmd = Command::new("sh");
    cmd.args([
        "-c",
        "yes out | head -c 1048576; yes err | head -c 1048576 >&2",
    ]);
    let (mut child, rx, stdout) = spawn(cmd);
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > Duration::from_secs(10) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("the child blocked on a pipe: a stream is not drained while the test waits");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success());
    assert_eq!(stdout.join().unwrap().len(), 1_048_576);
    assert_eq!(rx.iter().count(), 1_048_576 / 4);
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

#[cfg(unix)]
const TWO_SLEEPERS: &str = r#"{"schema_version": "1.0", "targets": [
  {"name": "first", "runner": "custom", "modes": {"full": {"command": "sleep 1"}}},
  {"name": "second", "runner": "custom", "modes": {"full": {"command": "sleep 1"}}}
]}"#;

#[cfg(unix)]
#[test]
fn sequential_start_lines_precede_each_target_and_stdout_is_unchanged() {
    let dir = repo(TWO_SLEEPERS);
    let home = tempfile::tempdir().unwrap();
    let (mut child, rx, out) = spawn(command(dir.path(), home.path(), &["test"]));

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

    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.join().unwrap()).into_owned();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "stdout: {stdout}");
    assert!(
        lines[0].starts_with("target first: ok (exit 0, "),
        "{stdout}"
    );
    assert!(
        lines[1].starts_with("target second: ok (exit 0, "),
        "{stdout}"
    );
    assert_eq!(lines[2], "test gate: passed (executed 2; skipped 0)");
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
    let (mut child, rx, _stdout) = spawn(command(dir.path(), home.path(), &["test"]));
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

#[cfg(unix)]
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
    let (mut child, rx, _stdout) = spawn(command(
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
    assert!(err.contains("another heavy gate is running"), "{err}");
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

    let done = child.wait().unwrap();
    assert_eq!(done.code(), Some(0));
    let after = run(other.path(), home.path(), &["test", "--mode", "full"]);
    assert_eq!(after.status.code(), Some(0), "{}", stderr(&after));
}

#[cfg(unix)]
#[test]
fn a_killed_gate_keeps_its_lock_until_its_target_exits() {
    killed_gate_keeps_its_lock_until_its_target_exits("full");
    killed_gate_keeps_its_lock_until_its_target_exits("essential");
}

#[cfg(unix)]
fn killed_gate_keeps_its_lock_until_its_target_exits(mode: &str) {
    let home = tempfile::tempdir().unwrap();
    // The target records its process group (the shell's pid leads it), then
    // outlives the gate that started it.
    let first = repo(
        &r#"{"schema_version": "1.0", "targets": [
  {"name": "survivor", "runner": "custom",
   "modes": {"full": {"command": "echo $$ > group.txt; sleep 30"}}}
]}"#
        .replace("full", mode),
    );
    let (mut gate, rx, _stdout) = spawn(command(
        first.path(),
        home.path(),
        &["test", "--mode", mode],
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
    // The gate records the group in its holder record just after the target
    // starts, and the target can write its file first: wait for the record
    // before killing the gate, or the kill can land between the two.
    let lock_file = home.path().join("locks/full-gate.record");
    let recorded = (0..200).any(|_| {
        let text = std::fs::read_to_string(&lock_file).unwrap_or_default();
        let found = text.lines().any(|l| l == format!("group={group}"));
        if !found {
            std::thread::sleep(Duration::from_millis(50));
        }
        found
    });
    assert!(recorded, "the gate recorded the target's group");

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
    let other = repo(&QUICK_PASS.replace("full", mode));
    for dir in [other.path(), first.path()] {
        let refused = run(dir, home.path(), &["test", "--mode", mode]);
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
    let after = run(other.path(), home.path(), &["test", "--mode", mode]);
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
    own_repository::own_repository(&root);
    let init = run(&root, home.path(), &["init", "--yes", "--minimal"]);
    assert!(init.status.success(), "init: {}", stderr(&init));
    std::fs::write(root.join(".codeflow/test-config.json"), LONG_TARGET).unwrap();

    let (mut gate, rx, _stdout) = spawn(command(&root, home.path(), &["test", "--mode", "full"]));
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

#[cfg(unix)]
#[test]
fn prerequisite_failure_never_starts_its_consumer() {
    let dir = repo(
        r#"{"schema_version":"1.0","execution":{"parallel":true,"max_parallel":2},"targets":[
      {"name":"producer","runner":"custom","modes":{"full":{"command":"exit 1"}}},
      {"name":"consumer","runner":"custom","requires":["producer"],"modes":{"full":{"command":"touch consumed"}}}] }"#,
    );
    let home = tempfile::tempdir().unwrap();
    let output = run(dir.path(), home.path(), &["test", "--all"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("not run: prerequisite failed"),
        "{}",
        stderr(&output)
    );
    assert!(!dir.path().join("consumed").exists());
}

#[cfg(unix)]
#[test]
fn delayed_producer_precedes_consumer_and_evidence_survives_cleanup() {
    let dir = repo(
        r#"{"schema_version":"1.0","execution":{"parallel":true,"max_parallel":2},"targets":[
      {"name":"consumer","runner":"custom","requires":["producer"],"modes":{"full":{"command":"test -f generated"}}},
      {"name":"producer","runner":"custom","outputs":["generated"],"exclusive":true,"modes":{"full":{"command":"sleep 0.1; touch generated"}}}] }"#,
    );
    let home = tempfile::tempdir().unwrap();
    let output = run(dir.path(), home.path(), &["test", "--all"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let artifact = stderr(&output)
        .lines()
        .find_map(|line| line.strip_prefix("[codeflow test] durable artifact: "))
        .unwrap()
        .to_owned();
    let raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&artifact).unwrap()).unwrap();
    assert_eq!(raw["passed"], true);
    assert!(!raw["revision"].as_str().unwrap().is_empty());
    assert!(!raw["tree"].as_str().unwrap().is_empty());
    drop(dir);
    assert!(Path::new(&artifact).exists());
}

#[cfg(unix)]
#[test]
fn producer_changing_tracked_bytes_refuses_candidate() {
    let dir = repo(
        r#"{"schema_version":"1.0","targets":[
      {"name":"producer","runner":"custom","outputs":["README.md"],"modes":{"full":{"command":"echo changed > README.md"}}},
      {"name":"consumer","runner":"custom","requires":["producer"],"modes":{"full":{"command":"touch consumed"}}}] }"#,
    );
    let home = tempfile::tempdir().unwrap();
    let output = run(dir.path(), home.path(), &["test"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("generation changed the candidate"),
        "{}",
        stderr(&output)
    );
    assert!(!dir.path().join("consumed").exists());
}

#[cfg(unix)]
#[test]
fn unavailable_gate_lock_refuses_before_start() {
    let dir = repo(TWO_SLEEPERS);
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("locks"), "blocked").unwrap();
    let output = run(dir.path(), home.path(), &["test"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("gate lock unavailable"),
        "{}",
        stderr(&output)
    );
    // TSK-216 AC-2: the refusal points at the check that names the fix.
    assert!(
        stderr(&output).contains("codeflow doctor --check permissions"),
        "{}",
        stderr(&output)
    );
    assert!(!stderr(&output).contains("starting target"));
}

#[cfg(unix)]
struct SelectionHome {
    path: std::path::PathBuf,
    evidence: std::path::PathBuf,
}

#[cfg(unix)]
impl SelectionHome {
    fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(unix)]
impl Drop for SelectionHome {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.evidence).unwrap();
    }
}

#[cfg(unix)]
fn selection_repo() -> (tempfile::TempDir, SelectionHome, String) {
    let dir = repo(
        r#"{"schema_version":"1.0","execution":{"parallel":true,"max_parallel":2,"run_everything":["crates/**","assets/**","Cargo.*",".codeflow/**"]},"targets":[
      {"name":"producer","runner":"custom","narrow":["crates/**"],"modes":{"full":{"command":"echo producer >> runs"}}},
      {"name":"docs","runner":"custom","requires":["producer"],"narrow":["docs/**"],"modes":{"full":{"command":"echo docs >> runs"}}},
      {"name":"present","runner":"custom","narrow":["crates/**"],"modes":{"full":{"command":"echo present >> runs"}}},
      {"name":"release","runner":"custom","narrow":["scripts/**"],"modes":{"full":{"command":"echo release >> runs"}}}] }"#,
    );
    let path = dir.path().join(".codeflow/test-config.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for target in config["targets"].as_array_mut().unwrap() {
        target["input_class"] = serde_json::json!("content");
        target["evidence"] = serde_json::json!({"tools":{},"environment":{},"environment_complete":true,"membership":[],"covers_modes":["quick"]});
    }
    config["targets"][0]["outputs"] = serde_json::json!(["target/producer"]);
    config["targets"][0]["modes"]["full"]["command"] =
        serde_json::json!("echo producer >> runs; echo produced > target/producer");
    for target in config["targets"].as_array_mut().unwrap() {
        target["modes"]["quick"] = target["modes"]["full"].clone();
    }
    std::fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
    for folder in ["crates", "scripts"] {
        std::fs::create_dir_all(dir.path().join(folder)).unwrap();
        std::fs::write(dir.path().join(folder).join("input"), "base").unwrap();
    }
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/guide.md"), "base\n").unwrap();
    std::fs::write(dir.path().join(".gitignore"), "target/\nruns\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "test: selection base"]);
    let base = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let base = String::from_utf8(base.stdout).unwrap().trim().to_owned();
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    git(dir.path(), &["branch", "fixture-base"]);
    let output = evidence::run(
        dir.path(),
        &home,
        &["integrate", "main", "--into", "fixture-base"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let err = stderr(&output);
    let artifact = err
        .lines()
        .find_map(|line| line.strip_prefix("[codeflow test] durable artifact: "))
        .unwrap();
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(artifact).unwrap()).unwrap();
    assert_eq!(raw["clean"], true, "{raw}");
    assert_eq!(raw["complete"], true, "{raw}");
    std::fs::remove_file(dir.path().join("runs")).unwrap();
    let evidence = Path::new(artifact)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    (
        dir,
        SelectionHome {
            path: home,
            evidence,
        },
        base,
    )
}

#[cfg(unix)]
#[test]
fn selection_narrows_only_with_green_base_and_keeps_prerequisite_closure() {
    let (dir, home, base) = selection_repo();
    std::fs::write(dir.path().join("docs/guide.md"), "changed\n").unwrap();
    let output = evidence::run(
        dir.path(),
        home.path(),
        &["test", "--mode", "quick", "--since", &base],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let runs = std::fs::read_to_string(dir.path().join("runs")).unwrap();
    assert_eq!(runs, "producer\ndocs\n", "{}", stderr(&output));
    assert!(stderr(&output).contains("skipped present"));
    let isolated = tempfile::tempdir().unwrap();
    let output = evidence::run(dir.path(), isolated.path(), &["test", "--all"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("selected present"));
}

#[cfg(unix)]
#[test]
fn selection_runs_everything_for_shared_unmatched_rename_deletion_and_unproven_base() {
    for case in [
        "core",
        "asset",
        "new",
        "rename",
        "delete",
        "unknown-base",
        "config",
    ] {
        let (dir, home, base) = selection_repo();
        let mut comparison = base.clone();
        match case {
            "core" | "asset" => {
                let folder = if case == "core" { "crates" } else { "assets" };
                std::fs::create_dir_all(dir.path().join(folder)).unwrap();
                std::fs::write(dir.path().join(folder).join("new.rs"), "input").unwrap();
            }
            "new" => {
                std::fs::write(dir.path().join("unmatched-new-file"), "input").unwrap();
            }
            "rename" => {
                git(dir.path(), &["mv", "docs/guide.md", "docs/renamed.md"]);
            }
            "delete" => {
                std::fs::remove_file(dir.path().join("docs/guide.md")).unwrap();
            }
            "unknown-base" => {
                comparison = "unknown".into();
            }
            "config" => {
                let path = dir.path().join(".codeflow/test-config.json");
                let mut config: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                config["execution"]["max_parallel"] = 1.into();
                for target in config["targets"].as_array_mut().unwrap() {
                    target["modes"]["quick"] = target["modes"]["full"].clone();
                }
                std::fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
            }
            _ => unreachable!(),
        }
        let output = evidence::run(
            dir.path(),
            home.path(),
            &["test", "--mode", "quick", "--since", &comparison],
        );
        assert!(output.status.success(), "{case}: {}", stderr(&output));
        let runs = std::fs::read_to_string(dir.path().join("runs")).unwrap();
        for target in ["producer", "docs", "present", "release"] {
            assert!(runs.lines().any(|line| line == target), "{case}: {runs}");
        }
    }
}

#[test]
fn every_tracked_path_is_classified_by_the_conservative_partition() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(".codeflow/test-config.json")).unwrap())
            .unwrap();
    let mut patterns: Vec<String> = config["execution"]["run_everything"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_owned())
        .collect();
    for target in config["targets"].as_array().unwrap() {
        if let Some(narrow) = target["narrow"].as_array() {
            patterns.extend(narrow.iter().map(|p| p.as_str().unwrap().to_owned()));
        }
    }
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .unwrap();
    let paths = String::from_utf8(output.stdout).unwrap();
    let unclassified: Vec<_> = paths
        .split('\0')
        .filter(|p| !p.is_empty())
        .filter(|p| !codeflow_core::testing::gate::inputs_match(&patterns, p))
        .collect();
    assert!(unclassified.is_empty(), "unclassified: {unclassified:?}");
}

#[cfg(unix)]
#[test]
fn parallel_execution_honors_its_bound_and_exclusive_targets_run_alone() {
    let bounded = r#"{"schema_version":"1.0","execution":{"parallel":true,"max_parallel":2},"targets":[
      {"name":"a","runner":"custom","modes":{"full":{"command":"if mkdir slot1 2>/dev/null; then sleep 0.3; rmdir slot1; else mkdir slot2 && sleep 0.3 && rmdir slot2; fi"}}},
      {"name":"b","runner":"custom","modes":{"full":{"command":"if mkdir slot1 2>/dev/null; then sleep 0.3; rmdir slot1; else mkdir slot2 && sleep 0.3 && rmdir slot2; fi"}}},
      {"name":"c","runner":"custom","modes":{"full":{"command":"if mkdir slot1 2>/dev/null; then sleep 0.3; rmdir slot1; else mkdir slot2 && sleep 0.3 && rmdir slot2; fi"}}}] }"#;
    let dir = repo(bounded);
    let home = tempfile::tempdir().unwrap();
    let output = run(dir.path(), home.path(), &["test"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let exclusive = bounded.replace("\"name\":\"b\",", "\"name\":\"b\",\"exclusive\":true,")
        .replace("if mkdir slot1 2>/dev/null; then sleep 0.3; rmdir slot1; else mkdir slot2 && sleep 0.3 && rmdir slot2; fi", "mkdir active && sleep 0.1 && rmdir active");
    let dir = repo(&exclusive);
    let output = run(dir.path(), home.path(), &["test"]);
    assert!(output.status.success(), "{}", stderr(&output));
}

#[cfg(unix)]
#[test]
fn unwritable_temporary_storage_refuses_before_targets_start() {
    let dir = repo(
        r#"{"schema_version":"1.0","targets":[{"name":"unit","runner":"custom","modes":{"full":{"command":"touch started"}}}]}"#,
    );
    let home = tempfile::tempdir().unwrap();
    let blocked = dir.path().join("not-a-directory");
    std::fs::write(&blocked, "blocked").unwrap();
    let output = command(dir.path(), home.path(), &["test"])
        .env("TMPDIR", &blocked)
        .env("TMP", &blocked)
        .env("TEMP", &blocked)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("temporary directory is not writable in this sandbox"),
        "{}",
        stderr(&output)
    );
    assert!(!dir.path().join("started").exists());
}

#[cfg(unix)]
#[test]
fn coordinator_sets_absolute_binary_paths_from_the_declared_output() {
    let dir = repo(
        r#"{"schema_version":"1.0","targets":[
      {"name":"codeflow-bin","runner":"custom","outputs":["target/debug/codeflow"],"modes":{"full":{"command":"mkdir -p target/debug; touch target/debug/codeflow"}}},
      {"name":"consumer","runner":"custom","requires":["codeflow-bin"],"env":{"CODEFLOW_BIN":"wrong","CF_PRESENT_CODEFLOW":"wrong"},"modes":{"full":{"command":"test \"$CODEFLOW_BIN\" = \"$PWD/target/debug/codeflow\" && test \"$CF_PRESENT_CODEFLOW\" = \"$CODEFLOW_BIN\""}}}] }"#,
    );
    let home = tempfile::tempdir().unwrap();
    let output = run(dir.path(), home.path(), &["test"]);
    assert!(output.status.success(), "{}", stderr(&output));
}

#[cfg(unix)]
#[test]
fn evidence_records_the_resolved_tool_path_and_version() {
    let dir = repo(CARGO_TARGET);
    let home = tempfile::tempdir().unwrap();
    let output = run(dir.path(), home.path(), &["test"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let err = stderr(&output);
    let artifact = err
        .lines()
        .find_map(|line| line.strip_prefix("[codeflow test] artifact: "))
        .unwrap();
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(artifact).unwrap()).unwrap();
    let cargo = raw["tools"]["cargo"].as_str().unwrap();
    assert!(
        cargo.starts_with('/'),
        "resolved absolute tool path: {cargo}"
    );
    assert!(cargo.contains("cargo "), "tool version: {cargo}");
}

/// TSK-184 AC-5 (Astra's A184-2): a target CI skips is still owed locally.
/// The CI run's artifact is not complete, so a local `--since` run does not
/// take it as a green base for that target and runs it.
#[cfg(unix)]
#[test]
fn a_ci_skipped_target_is_never_proved_by_the_ci_run() {
    let dir = repo(
        r#"{"schema_version":"1.0","execution":{"parallel":true,"max_parallel":2},"targets":[
      {"name":"always","runner":"custom","modes":{"full":{"command":"echo control"}}},
      {"name":"local-required","runner":"custom","ci_skip":true,"ci_skip_reason":"local hardware check","narrow":["src/**"],"modes":{"full":{"command":"exit 19"}}}] }"#,
    );
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/input.txt"), "unchanged\n").unwrap();
    std::fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "test: fixture"]);
    let base = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let base = String::from_utf8(base.stdout).unwrap().trim().to_owned();
    let home = tempfile::tempdir().unwrap();

    // CI: the skipped target is omitted, and the artifact says so.
    let output = command(
        dir.path(),
        home.path(),
        &["test", "--mode", "full", "--all"],
    )
    .env("CI", "true")
    .output()
    .unwrap();
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    assert!(err.contains("still owed locally: local-required"), "{err}");
    let artifact = err
        .lines()
        .find_map(|line| line.strip_prefix("[codeflow test] durable artifact: "))
        .unwrap();
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(artifact).unwrap()).unwrap();
    assert_eq!(raw["complete"], false, "{raw}");
    assert_eq!(
        raw["ci_skipped"],
        serde_json::json!(["local-required"]),
        "{raw}"
    );

    // Local: the CI run is no green base, so the owed target runs and fails.
    let output = run(
        dir.path(),
        home.path(),
        &["test", "--mode", "full", "--since", &base],
    );
    let err = stderr(&output);
    assert_eq!(output.status.code(), Some(1), "{err}");
    assert!(!err.contains("skipped local-required"), "{err}");
}

/// TSK-203 AC-4: `--only` runs the named targets and their prerequisites,
/// skips and names the rest, and records the run as not complete, so a
/// split gate is judged by all of its parts together and no part alone
/// can serve as a green base.
#[cfg(unix)]
#[test]
fn only_runs_the_named_targets_with_prerequisites_and_is_never_complete() {
    let (dir, home, _) = selection_repo();
    let output = run(
        dir.path(),
        home.path(),
        &[
            "test",
            "--mode",
            "full",
            "--strict",
            "--all",
            "--only",
            "docs,release",
        ],
    );
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("executed 3; skipped 0; not full-gate evidence"));
    let runs = std::fs::read_to_string(dir.path().join("runs")).unwrap();
    let mut ran: Vec<&str> = runs.lines().collect();
    ran.sort_unstable();
    assert_eq!(ran, ["docs", "producer", "release"], "{err}");
    assert!(
        err.contains("excluded present: --only: the named targets and their prerequisites"),
        "{err}"
    );
    let artifact = err
        .lines()
        .find_map(|line| line.strip_prefix("[codeflow test] durable artifact: "))
        .unwrap();
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(artifact).unwrap()).unwrap();
    assert_eq!(raw["complete"], false, "{raw}");
    assert_eq!(raw["selection"]["skipped"], serde_json::json!([]));
    assert_eq!(raw["passed"], true, "{raw}");

    // A limited run is never complete evidence, even when its names cover
    // every target: only a run without `--only` stands for the whole gate.
    std::fs::remove_file(dir.path().join("runs")).unwrap();
    let output = run(
        dir.path(),
        home.path(),
        &[
            "test",
            "--only",
            "docs",
            "--only",
            "present,release,producer",
        ],
    );
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    let artifact = err
        .lines()
        .find_map(|line| line.strip_prefix("[codeflow test] durable artifact: "))
        .unwrap();
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(artifact).unwrap()).unwrap();
    assert_eq!(raw["complete"], false, "{raw}");
    assert_eq!(raw["passed"], true, "{raw}");

    // The same targets run without `--only` are the whole gate.
    let output = run(dir.path(), home.path(), &["test"]);
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    let artifact = err
        .lines()
        .find_map(|line| line.strip_prefix("[codeflow test] durable artifact: "))
        .unwrap();
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(artifact).unwrap()).unwrap();
    assert_eq!(raw["complete"], true, "{raw}");
}

/// TSK-203 AC-4: a name that is not an enabled target of the mode is
/// refused before any target starts, so a typo cannot shrink a CI part.
#[test]
fn only_refuses_an_unknown_or_disabled_name_before_any_target_starts() {
    let dir = repo(
        r#"{"schema_version":"1.0","targets":[
      {"name":"unit","runner":"custom","modes":{"full":{"command":"echo unit > started"}}},
      {"name":"off","runner":"custom","enabled":false,"modes":{"full":{"command":"echo off > started"}}},
      {"name":"light","runner":"custom","modes":{"quick":{"command":"echo light > started"}}}]}"#,
    );
    let home = tempfile::tempdir().unwrap();
    for name in ["unti", "off", "light"] {
        let output = run(
            dir.path(),
            home.path(),
            &["test", "--only", &format!("unit,{name}")],
        );
        let err = stderr(&output);
        assert_eq!(output.status.code(), Some(1), "{name}: {err}");
        assert!(
            err.contains(&format!(
                "--only names {name}, which is no enabled target with a full mode"
            )),
            "{name}: {err}"
        );
        assert!(!err.contains("starting target"), "{name}: {err}");
        assert!(!dir.path().join("started").exists(), "{name}");
    }
    let output = run(
        dir.path(),
        home.path(),
        &["test", "--only", "unit", "--since", "HEAD"],
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("cannot be used with"),
        "{}",
        stderr(&output)
    );
}

/// Issue 79: a path in the git index that is not valid UTF-8 stopped the gate
/// at its snapshot of tracked files, so `codeflow test` failed in any project
/// with such a file name. The entry is added to the index only, so the test
/// also runs on file systems that refuse the name.
#[cfg(unix)]
#[test]
fn a_tracked_path_that_is_not_utf8_does_not_stop_the_gate() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let dir = repo(
        r#"{"schema_version":"1.0","targets":[
      {"name":"all","runner":"custom","narrow":["docs/**"],"modes":{"full":{"command":"true"}}}]}"#,
    );
    let blob = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child.stdin.take().unwrap().write_all(b"data")?;
            child.wait_with_output()
        })
        .unwrap();
    let blob = String::from_utf8(blob.stdout).unwrap().trim().to_owned();
    let entry = [
        b"100644,".as_slice(),
        blob.as_bytes(),
        b",caf\xe9.txt".as_slice(),
    ]
    .concat();
    let update = Command::new("git")
        .args(["update-index", "--add", "--cacheinfo"])
        .arg(OsStr::from_bytes(&entry))
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(update.status.success(), "{}", stderr(&update));
    let home = tempfile::tempdir().unwrap();
    let output = run(dir.path(), home.path(), &["test"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(String::from_utf8_lossy(&output.stdout).contains("test gate: passed"));
}

#[cfg(unix)]
fn held_until_released(mode: &str) -> tempfile::TempDir {
    repo(&format!(
        r#"{{"schema_version":"1.0","targets":[{{"name":"holder","runner":"custom","timeout_seconds":10,"modes":{{"{mode}":{{"command":"while ! test -f release; do sleep 0.01; done"}}}}}}]}}"#
    ))
}

#[cfg(unix)]
#[test]
fn essential_and_full_refuse_every_mode_pair_across_worktrees_and_repositories() {
    for holder in ["essential", "full"] {
        let home = tempfile::tempdir().unwrap();
        let first = held_until_released(holder);
        let other = repo(QUICK_PASS);
        let linked = first.path().join("linked");
        git(
            first.path(),
            &["worktree", "add", "--detach", linked.to_str().unwrap()],
        );
        let (mut child, rx, _stdout) = spawn(command(
            first.path(),
            home.path(),
            &["test", "--mode", holder],
        ));
        wait_for(&rx, "starting target 'holder'");
        for contender in ["essential", "full"] {
            for directory in [other.path(), linked.as_path()] {
                let output = run(directory, home.path(), &["test", "--mode", contender]);
                let error = stderr(&output);
                assert_eq!(
                    output.status.code(),
                    Some(1),
                    "{holder}/{contender}: {error}"
                );
                assert!(error.contains(&format!("pid {}", child.id())), "{error}");
                assert!(error.contains(&format!("{holder} mode")), "{error}");
                assert!(error.contains(first.path().to_str().unwrap()), "{error}");
                assert!(!error.contains("starting target"), "{error}");
            }
        }
        std::fs::write(first.path().join("release"), "done").unwrap();
        assert!(child.wait().unwrap().success());
        std::fs::write(
            first.path().join(".codeflow/test-config.json"),
            QUICK_PASS.replace("full", holder),
        )
        .unwrap();
        let after = run(first.path(), home.path(), &["test", "--mode", holder]);
        assert!(after.status.success(), "{}", stderr(&after));
    }
}

#[cfg(unix)]
#[test]
fn quick_runs_light_targets_but_refuses_declared_and_fallback_heavy_targets() {
    for fallback in [false, true] {
        let home = tempfile::tempdir().unwrap();
        let first = held_until_released("full");
        let linked = first.path().join("linked");
        git(
            first.path(),
            &["worktree", "add", "--detach", linked.to_str().unwrap()],
        );
        let mode = if fallback { "essential" } else { "quick" };
        let declaration = if fallback { "" } else { "\"heavy\":true," };
        std::fs::create_dir_all(linked.join(".codeflow")).unwrap();
        std::fs::write(linked.join(".codeflow/test-config.json"), format!(r#"{{"schema_version":"1.0","execution":{{"parallel":true,"max_parallel":2,"heavy_wait_seconds":1}},"targets":[
          {{"name":"workspace-clippy","runner":"custom",{declaration}"modes":{{"{mode}":{{"command":"touch heavy-ran"}}}}}},
          {{"name":"light","timeout_seconds":5,"heavy":false,"runner":"custom","modes":{{"{mode}":{{"command":"touch light-ran"}}}}}}
        ]}}"#)).unwrap();
        let (mut child, rx, _stdout) = spawn(command(first.path(), home.path(), &["test"]));
        wait_for(&rx, "starting target 'holder'");
        let start = Instant::now();
        let output = run(&linked, home.path(), &["test", "--mode", "quick"]);
        let error = stderr(&output);
        assert_eq!(output.status.code(), Some(1), "{error}");
        assert!(start.elapsed() >= Duration::from_secs(1));
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(linked.join("light-ran").exists());
        assert!(!linked.join("heavy-ran").exists());
        assert!(error.contains(&format!("pid {}", child.id())), "{error}");
        assert!(error.contains("full mode"), "{error}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("target light: ok"));
        std::fs::write(first.path().join("release"), "done").unwrap();
        assert!(child.wait().unwrap().success());
        assert!(run(&linked, home.path(), &["test", "--mode", "quick"])
            .status
            .success());
        assert!(linked.join("heavy-ran").exists());
    }
}

#[cfg(unix)]
#[test]
fn reserve_refuses_beside_a_gate_then_runs_exact_arguments() {
    let home = tempfile::tempdir().unwrap();
    let first = held_until_released("full");
    let linked = first.path().join("linked");
    git(
        first.path(),
        &["worktree", "add", "--detach", linked.to_str().unwrap()],
    );
    let args = [
        "reserve",
        "--",
        "sh",
        "-c",
        "printf '%s' \"$1\" > result",
        "sh",
        "literal $value; spaces",
    ];
    let (mut child, rx, _stdout) = spawn(command(first.path(), home.path(), &["test"]));
    wait_for(&rx, "starting target 'holder'");
    let refused = run(&linked, home.path(), &args);
    assert_eq!(refused.status.code(), Some(1), "{}", stderr(&refused));
    assert!(!linked.join("result").exists());
    assert!(stderr(&refused).contains(&format!("pid {}", child.id())));
    std::fs::write(first.path().join("release"), "done").unwrap();
    assert!(child.wait().unwrap().success());
    let after = run(&linked, home.path(), &args);
    assert!(after.status.success(), "{}", stderr(&after));
    assert_eq!(
        std::fs::read_to_string(linked.join("result")).unwrap(),
        "literal $value; spaces"
    );
}

#[cfg(unix)]
#[test]
fn concurrent_heavy_targets_share_the_configured_workers_while_light_work_runs() {
    let dir = repo(
        r#"{"schema_version":"1.0","execution":{"parallel":true,"max_parallel":3,"worker_budget":8},"targets":[
      {"name":"a","timeout_seconds":5,"heavy":true,"runner":"custom","modes":{"quick":{"command":"echo $CARGO_BUILD_JOBS:$NEXTEST_TEST_THREADS:$RUST_TEST_THREADS:$CODEFLOW_ORACLE_WORKERS > a; while ! test -f b; do sleep 0.01; done; test -f light || sleep 0.2"}}},
      {"name":"b","timeout_seconds":5,"heavy":true,"runner":"custom","modes":{"quick":{"command":"echo $CARGO_BUILD_JOBS:$NEXTEST_TEST_THREADS:$RUST_TEST_THREADS:$CODEFLOW_ORACLE_WORKERS > b; while ! test -f a; do sleep 0.01; done; test -f light || sleep 0.2"}}},
      {"name":"light","timeout_seconds":5,"heavy":false,"runner":"custom","modes":{"quick":{"command":"touch light"}}}
    ]}"#,
    );
    let home = tempfile::tempdir().unwrap();
    let output = run(dir.path(), home.path(), &["test", "--mode", "quick"]);
    assert!(output.status.success(), "{}", stderr(&output));
    for name in ["a", "b"] {
        assert_eq!(
            std::fs::read_to_string(dir.path().join(name))
                .unwrap()
                .trim(),
            "4:4:1:4"
        );
    }
    assert!(dir.path().join("light").exists());
    assert!(stderr(&output).contains("workers for 'a': jobs=4"));
    assert!(stderr(&output).contains("workers for 'b': jobs=4"));
}

#[cfg(unix)]
#[test]
fn killing_heavy_quick_fallback_and_reservation_reaps_each_command_group() {
    use std::os::unix::process::CommandExt as _;
    for mode in ["reserve", "quick", "essential"] {
        let dir = repo(QUICK_PASS);
        let home = tempfile::tempdir().unwrap();
        let args = if mode == "reserve" {
            vec![
                "reserve",
                "--",
                "sh",
                "-c",
                "echo $$ > group; sleep 30 & wait",
            ]
        } else {
            let weight = if mode == "quick" {
                "\"heavy\":true,"
            } else {
                ""
            };
            std::fs::write(dir.path().join(".codeflow/test-config.json"), format!(r#"{{"schema_version":"1.0","targets":[{{"name":"heavy","runner":"custom",{weight}"modes":{{"{mode}":{{"command":"echo $$ > group; sleep 30 & wait"}}}}}}]}}"#)).unwrap();
            vec!["test", "--mode", "quick"]
        };
        let mut launch = command(dir.path(), home.path(), &args);
        launch.process_group(0);
        let (mut supervisor, rx, _stdout) = spawn(launch);
        wait_for(
            &rx,
            if mode == "reserve" {
                "[codeflow reserve] started process"
            } else {
                "supervising heavy target"
            },
        );
        let group = (0..100)
            .find_map(|_| {
                let value = std::fs::read_to_string(dir.path().join("group"))
                    .unwrap_or_default()
                    .trim()
                    .parse::<u32>()
                    .ok();
                if value.is_none() {
                    std::thread::sleep(Duration::from_millis(20));
                }
                value
            })
            .expect("reserved command group");
        if mode == "quick" {
            assert!(Command::new("kill")
                .args(["-TERM", "--", &format!("-{}", supervisor.id())])
                .status()
                .unwrap()
                .success());
        } else {
            supervisor.kill().unwrap();
        }
        supervisor.wait().unwrap();
        let gone = (0..100).any(|_| {
            let alive = Command::new("kill")
                .args(["-0", "--", &format!("-{group}")])
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success();
            if alive {
                std::thread::sleep(Duration::from_millis(50));
            }
            !alive
        });
        assert!(gone, "reserved group {group} survived its supervisor");
        let output = run(
            dir.path(),
            home.path(),
            &["reserve", "--", "sh", "-c", "exit 0"],
        );
        assert!(output.status.success(), "{}", stderr(&output));
    }
}

#[cfg(unix)]
#[test]
fn a_heavy_target_waits_for_its_requested_share_without_blocking_light_work() {
    let dir = repo(
        r#"{"schema_version":"1.0","execution":{"parallel":true,"max_parallel":3,"worker_budget":8},"targets":[
      {"name":"first","timeout_seconds":5,"heavy":true,"workers":8,"runner":"custom","modes":{"quick":{"command":"test \"$CARGO_BUILD_JOBS\" = 8 && touch first-start && sleep 0.3 && touch first-done"}}},
      {"name":"second","timeout_seconds":5,"heavy":true,"workers":4,"runner":"custom","modes":{"quick":{"command":"test -f first-done && test \"$CARGO_BUILD_JOBS\" = 4"}}},
      {"name":"light","timeout_seconds":5,"heavy":false,"runner":"custom","modes":{"quick":{"command":"while ! test -f first-start; do sleep 0.01; done; test ! -f first-done"}}}
    ]}"#,
    );
    let home = tempfile::tempdir().unwrap();
    let output = run(dir.path(), home.path(), &["test", "--mode", "quick"]);
    assert!(output.status.success(), "{}", stderr(&output));
}

#[cfg(unix)]
#[test]
fn invalid_worker_budgets_refuse_before_starting_a_target() {
    for (execution, weight) in [
        (r#"{"worker_budget":0}"#, r#""heavy":true"#),
        (r#"{"worker_budget":8}"#, r#""heavy":true,"workers":9"#),
        (r#"{"worker_budget":8}"#, r#""heavy":false,"workers":1"#),
        ("{}", r#""heavy":true,"workers":1"#),
    ] {
        let dir = repo(&format!(
            r#"{{"schema_version":"1.0","execution":{execution},"targets":[{{"name":"invalid","runner":"custom",{weight},"modes":{{"quick":{{"command":"touch ran"}}}}}}]}}"#
        ));
        let home = tempfile::tempdir().unwrap();
        let output = run(dir.path(), home.path(), &["test", "--mode", "quick"]);
        assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
        assert!(!dir.path().join("ran").exists());
        assert!(stderr(&output).contains("worker_budget"));
    }
}

#[cfg(unix)]
#[test]
fn essential_refuses_an_unavailable_lock_directory_before_any_target() {
    let dir = repo(&QUICK_PASS.replace("full", "essential"));
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("locks"), "blocked").unwrap();
    let output = run(dir.path(), home.path(), &["test", "--mode", "essential"]);
    assert_eq!(output.status.code(), Some(1));
    let error = stderr(&output);
    assert!(error.contains("gate lock unavailable"), "{error}");
    assert!(
        error.contains("codeflow doctor --check permissions"),
        "{error}"
    );
    assert!(!error.contains("starting target"), "{error}");
}

#[cfg(unix)]
#[test]
fn quick_default_class_controls_undeclared_targets_and_keeps_explicit_light_work() {
    for class in ["heavy", "light"] {
        let home = tempfile::tempdir().unwrap();
        let holder = held_until_released("full");
        let linked = holder.path().join("linked");
        git(
            holder.path(),
            &["worktree", "add", "--detach", linked.to_str().unwrap()],
        );
        std::fs::create_dir_all(linked.join(".codeflow")).unwrap();
        std::fs::write(linked.join(".codeflow/test-config.json"), format!(r#"{{"schema_version":"1.0","execution":{{"quick_default_class":"{class}","heavy_wait_seconds":1}},"targets":[
          {{"name":"undeclared","runner":"custom","modes":{{"quick":{{"command":"touch default-ran"}}}}}},
          {{"name":"light","runner":"custom","heavy":false,"modes":{{"quick":{{"command":"touch light-ran"}}}}}}
        ]}}"#)).unwrap();
        let (mut child, rx, _stdout) = spawn(command(holder.path(), home.path(), &["test"]));
        wait_for(&rx, "starting target 'holder'");
        let start = Instant::now();
        let output = run(&linked, home.path(), &["test", "--mode", "quick"]);
        let error = stderr(&output);
        assert!(linked.join("light-ran").exists());
        if class == "heavy" {
            assert_eq!(output.status.code(), Some(1), "{error}");
            assert!(start.elapsed() >= Duration::from_secs(1));
            assert!(!linked.join("default-ran").exists());
            assert!(error.contains("waiting up to 1s"), "{error}");
            assert!(error.contains(&format!("pid {}", child.id())), "{error}");
            assert!(error.contains("full mode"), "{error}");
            assert!(error.contains(holder.path().to_str().unwrap()), "{error}");
        } else {
            assert!(output.status.success(), "{error}");
            assert!(linked.join("default-ran").exists());
        }
        std::fs::write(holder.path().join("release"), "done").unwrap();
        assert!(child.wait().unwrap().success());
        let after = run(&linked, home.path(), &["test", "--mode", "quick"]);
        assert!(after.status.success(), "{}", stderr(&after));
        assert!(linked.join("default-ran").exists());
    }
}

#[cfg(unix)]
#[test]
fn lock_status_reads_both_holder_records_without_running_or_changing_them() {
    let home = tempfile::tempdir().unwrap();
    let root = held_until_released("essential");
    let path = home.path().join("locks/full-gate.record");
    let empty = run(root.path(), home.path(), &["test", "--lock-status"]);
    assert!(empty.status.success(), "{}", stderr(&empty));
    assert!(String::from_utf8_lossy(&empty.stdout).contains("no recorded holder"));
    assert!(
        !home.path().join("locks").exists(),
        "status must not create lock directories"
    );
    assert!(!root.path().join(".git/codeflow").exists());
    let (mut child, rx, _stdout) = spawn(command(
        root.path(),
        home.path(),
        &["test", "--mode", "essential"],
    ));
    wait_for(&rx, "starting target 'holder'");
    let deadline = Instant::now() + Duration::from_secs(2);
    let before = loop {
        let record = std::fs::read_to_string(&path).unwrap();
        if record.contains("group=") {
            break record;
        }
        assert!(Instant::now() < deadline, "target group was not recorded");
        std::thread::sleep(Duration::from_millis(10));
    };
    let started = before
        .lines()
        .find_map(|line| line.strip_prefix("started="))
        .unwrap();
    let status = run(root.path(), home.path(), &["test", "--lock-status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let text = String::from_utf8_lossy(&status.stdout);
    assert_eq!(text.matches("recorded holder:").count(), 2, "{text}");
    for value in [
        format!("pid {}", child.id()),
        "essential mode".into(),
        root.path().display().to_string(),
        format!("started_unix={started}"),
    ] {
        assert!(text.contains(&value), "{text}");
    }
    assert!(!stderr(&status).contains("starting target"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    assert_eq!(
        run(root.path(), home.path(), &["test"]).status.code(),
        Some(1)
    );
    std::fs::write(root.path().join("release"), "done").unwrap();
    assert!(child.wait().unwrap().success());
    let after = run(root.path(), home.path(), &["test", "--lock-status"]);
    assert!(after.status.success());
    assert_eq!(
        String::from_utf8_lossy(&after.stdout)
            .matches("no recorded holder")
            .count(),
        2
    );
}

#[cfg(unix)]
#[test]
fn reserve_keeps_the_calling_binary_for_git_hook_dispatch() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let root = repo(r#"{"schema_version":"1.0","targets":[]}"#);
    let hook = root.path().join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\nprintf '%s' \"$CODEFLOW_HOOK_BINARY\" > hook-binary\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = command(
        root.path(),
        home.path(),
        &[
            "reserve",
            "--",
            "git",
            "commit",
            "--allow-empty",
            "-m",
            "test: dispatch",
        ],
    )
    .env("CODEFLOW_HOOK_BINARY", "unrelated-codeflow")
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let expected = std::env::var_os("CODEFLOW_TEST_GATE_BIN")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_codeflow").into());
    assert_eq!(
        std::fs::read_to_string(root.path().join("hook-binary")).unwrap(),
        expected.to_str().unwrap()
    );
}

#[cfg(unix)]
#[test]
fn reserve_refuses_dangling_config_links_before_starting_the_command() {
    for ancestor in [false, true] {
        let home = tempfile::tempdir().unwrap();
        let root = repo(r#"{"schema_version":"1.0","targets":[]}"#);
        let path = if ancestor {
            root.path().join(".codeflow")
        } else {
            root.path().join(".codeflow/test-config.json")
        };
        if ancestor {
            std::fs::remove_dir_all(&path).unwrap();
        } else {
            std::fs::remove_file(&path).unwrap();
        }
        std::os::unix::fs::symlink("missing-config", &path).unwrap();
        let output = run(
            root.path(),
            home.path(),
            &["reserve", "--", "sh", "-c", "touch started"],
        );
        assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
        assert!(!root.path().join("started").exists());
    }
}

#[cfg(unix)]
#[test]
fn lock_status_refuses_dangling_record_links_instead_of_reporting_an_empty_record() {
    for ancestor in [false, true] {
        let home = tempfile::tempdir().unwrap();
        let root = repo(r#"{"schema_version":"1.0","targets":[]}"#);
        let path = if ancestor {
            home.path().join("locks")
        } else {
            std::fs::create_dir(home.path().join("locks")).unwrap();
            home.path().join("locks/full-gate.lock")
        };
        std::os::unix::fs::symlink("missing-record", &path).unwrap();
        let output = run(root.path(), home.path(), &["test", "--lock-status"]);
        assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
        assert!(stderr(&output).contains("cannot read gate lock"));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("no recorded holder"));
    }
}
