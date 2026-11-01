//! TSK-1145: heavy runs share the host by capacity. Two full gates in two
//! worktrees run at once, a quick run's heavy targets run during a full
//! gate, a run past the host's capacity starts with fewer jobs instead of
//! waiting, and only a `host_exclusive` target runs alone.

use super::{command, git, repo, run, spawn, stderr, wait_for};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A config whose one target, in `mode`, waits for a `release` file.
fn holding(mode: &str, execution: &str) -> String {
    format!(
        r#"{{"schema_version":"1.0","execution":{{{execution}}},"targets":[
          {{"name":"holder","runner":"custom","timeout_seconds":60,"modes":{{"{mode}":{{"command":"while ! test -f release; do sleep 0.01; done"}}}}}}
        ]}}"#
    )
}

/// A config whose one heavy target, in `mode`, marks that it ran.
fn marking(mode: &str, execution: &str, extra: &str) -> String {
    format!(
        r#"{{"schema_version":"1.0","execution":{{{execution}}},"targets":[
          {{"name":"marker","heavy":true{extra},"runner":"custom","timeout_seconds":60,"modes":{{"{mode}":{{"command":"touch ran"}}}}}}
        ]}}"#
    )
}

fn linked_worktree(first: &Path, name: &str, config: &str) -> PathBuf {
    let linked = first.join(name);
    git(
        first,
        &["worktree", "add", "--detach", linked.to_str().unwrap()],
    );
    std::fs::create_dir_all(linked.join(".codeflow")).unwrap();
    std::fs::write(linked.join(".codeflow/test-config.json"), config).unwrap();
    linked
}

fn release(dir: &Path) {
    std::fs::write(dir.join("release"), "").unwrap();
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

/// AC-1 (journey): a full gate and an essential gate in two worktrees of
/// one repository, and a full gate in another repository, all run while the
/// first full gate is still running.
#[cfg(unix)]
#[test]
fn two_full_gates_in_two_worktrees_run_at_once() {
    let home = tempfile::tempdir().unwrap();
    let first = repo(&holding("full", ""));
    let linked = linked_worktree(first.path(), "linked", &marking("full", "", ""));
    let (mut holder, rx, _stdout) = spawn(command(
        first.path(),
        home.path(),
        &["test", "--mode", "full"],
    ));
    wait_for(&rx, "starting target 'holder'");

    let second = run(&linked, home.path(), &["test", "--mode", "full"]);
    assert_eq!(second.status.code(), Some(0), "{}", stderr(&second));
    assert!(linked.join("ran").exists(), "{}", stderr(&second));

    let other = repo(&marking("essential", "", ""));
    let third = run(other.path(), home.path(), &["test", "--mode", "essential"]);
    assert_eq!(third.status.code(), Some(0), "{}", stderr(&third));
    assert!(other.path().join("ran").exists());

    assert!(
        holder.try_wait().unwrap().is_none(),
        "the first gate still runs"
    );
    release(first.path());
    assert!(holder.wait().unwrap().success());
}

/// AC-2 (journey): a quick run's heavy target runs at once during a full
/// gate in another worktree, with no wait configured and no "not run".
#[cfg(unix)]
#[test]
fn a_quick_run_during_a_full_gate_runs_its_heavy_targets() {
    let home = tempfile::tempdir().unwrap();
    let first = repo(&holding("full", ""));
    let linked = linked_worktree(
        first.path(),
        "linked",
        &marking("quick", r#""heavy_wait_seconds":0"#, ""),
    );
    let (mut holder, rx, _stdout) = spawn(command(
        first.path(),
        home.path(),
        &["test", "--mode", "full"],
    ));
    wait_for(&rx, "starting target 'holder'");

    let quick = run(&linked, home.path(), &["test", "--mode", "quick"]);
    let err = stderr(&quick);
    assert_eq!(quick.status.code(), Some(0), "{err}");
    assert!(!err.contains("not run"), "{err}");
    assert!(linked.join("ran").exists(), "{err}");

    release(first.path());
    assert!(holder.wait().unwrap().success());
}

/// AC-3: with a host budget of four jobs and four jobs per run, the host
/// has room for one run at full width; a second run starts at once with
/// half the jobs instead of waiting.
#[cfg(unix)]
#[test]
fn a_run_past_capacity_starts_with_fewer_jobs() {
    let home = tempfile::tempdir().unwrap();
    let budget = r#""host_jobs":4,"worker_budget":4"#;
    let first = repo(
        &holding("full", budget)
            .replace(r#""runner":"custom""#, r#""heavy":true,"runner":"custom""#),
    );
    let linked = linked_worktree(first.path(), "linked", &marking("quick", budget, ""));
    let (mut holder, rx, _stdout) = spawn(command(
        first.path(),
        home.path(),
        &["test", "--mode", "full"],
    ));
    let first_jobs = wait_for(&rx, "workers for 'holder'");
    assert!(first_jobs.contains("jobs=4"), "{first_jobs}");
    wait_for(&rx, "starting target 'holder'");

    let started = Instant::now();
    let quick = run(&linked, home.path(), &["test", "--mode", "quick"]);
    let err = stderr(&quick);
    assert_eq!(quick.status.code(), Some(0), "{err}");
    assert!(err.contains("workers for 'marker': jobs=2"), "{err}");
    assert!(linked.join("ran").exists(), "{err}");
    assert!(started.elapsed() < Duration::from_secs(20), "no queueing");

    release(first.path());
    assert!(holder.wait().unwrap().success());
}

/// AC-4: a `host_exclusive` target does not start while another heavy run
/// holds the host, and no heavy run starts while it runs; a target without
/// the declaration runs beside other runs.
#[cfg(unix)]
#[test]
fn a_host_exclusive_target_runs_alone() {
    let home = tempfile::tempdir().unwrap();
    let first = repo(&holding("full", ""));
    let exclusive = linked_worktree(
        first.path(),
        "exclusive",
        &marking("full", "", r#","host_exclusive":true"#),
    );
    let (mut holder, rx, _stdout) = spawn(command(
        first.path(),
        home.path(),
        &["test", "--mode", "full"],
    ));
    wait_for(&rx, "starting target 'holder'");

    // Another heavy run holds the host: the exclusive target does not run.
    let refused = run(&exclusive, home.path(), &["test", "--mode", "full"]);
    let err = stderr(&refused);
    assert_eq!(refused.status.code(), Some(1), "{err}");
    assert!(err.contains("host-exclusive"), "{err}");
    assert!(!exclusive.join("ran").exists(), "{err}");
    release(first.path());
    assert!(holder.wait().unwrap().success());

    // The exclusive target holds the host: no other heavy run starts.
    let held = linked_worktree(
        first.path(),
        "held",
        &holding("full", "").replace(
            r#""runner":"custom""#,
            r#""host_exclusive":true,"runner":"custom""#,
        ),
    );
    let (mut alone, rx, _stdout) = spawn(command(&held, home.path(), &["test", "--mode", "full"]));
    wait_for(&rx, "starting target 'holder'");
    let other = repo(&marking("full", "", ""));
    let blocked = run(other.path(), home.path(), &["test", "--mode", "full"]);
    let err = stderr(&blocked);
    assert_eq!(blocked.status.code(), Some(1), "{err}");
    assert!(err.contains("host-exclusive"), "{err}");
    assert!(!other.path().join("ran").exists(), "{err}");
    release(&held);
    assert!(alone.wait().unwrap().success());

    let after = run(other.path(), home.path(), &["test", "--mode", "full"]);
    assert_eq!(after.status.code(), Some(0), "{}", stderr(&after));
    wait_for_file(&other.path().join("ran"));
}
