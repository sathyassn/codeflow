//! TSK-1128: a quick run under `codeflow reserve` uses the reserve's lock,
//! a forged hand-over grants nothing, and a direct quick run can wait a
//! bounded time for the lock.

use super::{command, git, held_until_released, repo, run, spawn, stderr, wait_for};
use std::path::Path;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_codeflow");

/// A quick config whose one heavy target marks its start, waits for
/// `release` (when `hold`) and marks its end.
fn heavy_quick(hold: bool) -> String {
    let wait = if hold {
        "while ! test -f release; do sleep 0.01; done; "
    } else {
        ""
    };
    format!(
        r#"{{"schema_version":"1.0","execution":{{"heavy_wait_seconds":0}},"targets":[
          {{"name":"nested-heavy","heavy":true,"timeout_seconds":30,"runner":"custom","modes":{{"quick":{{"command":"touch heavy-started; {wait}touch heavy-ran"}}}}}}
        ]}}"#
    )
}

fn linked_worktree(first: &Path, config: &str) -> std::path::PathBuf {
    let linked = first.join("linked");
    git(
        first,
        &["worktree", "add", "--detach", linked.to_str().unwrap()],
    );
    std::fs::create_dir_all(linked.join(".codeflow")).unwrap();
    std::fs::write(linked.join(".codeflow/test-config.json"), config).unwrap();
    linked
}

fn wait_for_file(path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "{} never appeared",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// AC-1 (journey), AC-5: `codeflow reserve -- codeflow test --mode quick`
/// runs the quick run's heavy target under the reserve's lock, while a
/// second heavy run from another worktree is still refused.
#[test]
fn a_quick_run_under_reserve_runs_its_heavy_targets_under_the_reserved_lock() {
    let home = tempfile::tempdir().unwrap();
    let first = repo(&heavy_quick(true));
    let linked = linked_worktree(first.path(), &heavy_quick(false));
    let (mut reserve, rx, _stdout) = spawn(command(
        first.path(),
        home.path(),
        &["reserve", "--", BIN, "test", "--mode", "quick"],
    ));
    wait_for(&rx, "[codeflow reserve] started process");
    wait_for_file(&first.path().join("heavy-started"));

    let contender = run(&linked, home.path(), &["test", "--mode", "quick"]);
    assert_eq!(contender.status.code(), Some(1), "{}", stderr(&contender));
    assert!(
        stderr(&contender).contains("heavy target refused"),
        "{}",
        stderr(&contender)
    );
    assert!(!linked.join("heavy-ran").exists());
    let essential = run(&linked, home.path(), &["test", "--mode", "essential"]);
    assert_eq!(essential.status.code(), Some(1), "{}", stderr(&essential));
    let reserved = run(
        &linked,
        home.path(),
        &["reserve", "--", "touch", "reserved"],
    );
    assert_eq!(reserved.status.code(), Some(1), "{}", stderr(&reserved));
    assert!(!linked.join("reserved").exists());

    std::fs::write(first.path().join("release"), "done").unwrap();
    let status = reserve.wait().unwrap();
    let lines: Vec<String> = rx.try_iter().collect();
    assert!(status.success(), "{lines:#?}");
    assert!(first.path().join("heavy-ran").exists());
    assert!(
        lines
            .iter()
            .any(|line| line.contains("enclosing `codeflow reserve`")),
        "{lines:#?}"
    );
    // The lock is free again once the reserve ends.
    let after = run(&linked, home.path(), &["test", "--mode", "quick"]);
    assert!(after.status.success(), "{}", stderr(&after));
}

/// AC-5, AC-6: an essential gate under `codeflow reserve` takes the lock
/// itself, so it is still refused and runs no target.
#[test]
fn an_essential_gate_under_reserve_still_refuses() {
    let home = tempfile::tempdir().unwrap();
    let dir = repo(
        r#"{"schema_version":"1.0","targets":[{"name":"gate","runner":"custom","modes":{"essential":{"command":"touch essential-ran"}}}]}"#,
    );
    let output = run(
        dir.path(),
        home.path(),
        &["reserve", "--", BIN, "test", "--mode", "essential"],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("one heavy gate"),
        "{}",
        stderr(&output)
    );
    assert!(!dir.path().join("essential-ran").exists());
}

/// AC-2: a process that opens the lock files itself and names them in the
/// hand-over variable gains nothing while another gate holds the lock.
#[test]
fn a_forged_hand_over_is_refused_while_another_gate_holds_the_lock() {
    let home = tempfile::tempdir().unwrap();
    let first = held_until_released("full");
    let linked = linked_worktree(first.path(), &heavy_quick(false));
    let (mut holder, rx, _stdout) = spawn(command(first.path(), home.path(), &["test"]));
    wait_for(&rx, "starting target 'holder'");
    let home_lock = home.path().join("locks/full-gate.lock");
    let repo_lock = first.path().join(".git/codeflow/full-gate.lock");
    assert!(home_lock.exists() && repo_lock.exists());
    let output = std::process::Command::new("sh")
        .args([
            "-c",
            r#"exec 7<"$1" 8<"$2"; exec "$3" test --mode quick"#,
            "sh",
            home_lock.to_str().unwrap(),
            repo_lock.to_str().unwrap(),
            BIN,
        ])
        .current_dir(&linked)
        .env("CODEFLOW_HOME", home.path())
        .env("CODEFLOW_RESERVED_LOCK_FDS", "7,8")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CI")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("heavy target refused"),
        "{}",
        stderr(&output)
    );
    assert!(!stderr(&output).contains("enclosing `codeflow reserve`"));
    assert!(!linked.join("heavy-started").exists());
    std::fs::write(first.path().join("release"), "done").unwrap();
    assert!(holder.wait().unwrap().success());
}

/// AC-4: `--wait-seconds` bounds how long heavy quick work waits for the
/// lock: it refuses after the bound, and runs once the holder ends inside it.
#[test]
fn a_direct_quick_run_waits_for_the_lock_up_to_its_bound() {
    let home = tempfile::tempdir().unwrap();
    let first = held_until_released("full");
    let linked = linked_worktree(first.path(), &heavy_quick(false));
    let (mut holder, rx, _stdout) = spawn(command(first.path(), home.path(), &["test"]));
    wait_for(&rx, "starting target 'holder'");

    let start = Instant::now();
    let refused = run(
        &linked,
        home.path(),
        &["test", "--mode", "quick", "--wait-seconds", "1"],
    );
    assert_eq!(refused.status.code(), Some(1), "{}", stderr(&refused));
    assert!(start.elapsed() >= Duration::from_secs(1));
    assert!(
        stderr(&refused).contains("waiting up to 1s"),
        "{}",
        stderr(&refused)
    );
    assert!(!linked.join("heavy-started").exists());

    let release = first.path().join("release");
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        std::fs::write(release, "done").unwrap();
    });
    let start = Instant::now();
    let waited = run(
        &linked,
        home.path(),
        &["test", "--mode", "quick", "--wait-seconds", "30"],
    );
    releaser.join().unwrap();
    assert!(waited.status.success(), "{}", stderr(&waited));
    assert!(start.elapsed() >= Duration::from_secs(1));
    assert!(
        stderr(&waited).contains("waiting up to 30s"),
        "{}",
        stderr(&waited)
    );
    assert!(linked.join("heavy-ran").exists());
    assert!(holder.wait().unwrap().success());
}
