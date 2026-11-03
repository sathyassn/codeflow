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
/// gate in another worktree, with no wait and no "not run".
#[cfg(unix)]
#[test]
fn a_quick_run_during_a_full_gate_runs_its_heavy_targets() {
    let home = tempfile::tempdir().unwrap();
    let first = repo(&holding("full", ""));
    let linked = linked_worktree(first.path(), "linked", &marking("quick", "", ""));
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

/// Two heavy targets, each declaring `workers` and writing the jobs it was
/// given to `jobs-<name>`.
fn two_wide(mode: &str, execution: &str, workers: usize) -> String {
    let target = |name: &str| {
        format!(
            r#"{{"name":"{name}","heavy":true,"workers":{workers},"runner":"custom","timeout_seconds":60,"modes":{{"{mode}":{{"command":"printf %s \"$CARGO_BUILD_JOBS\" > jobs-{name}"}}}}}}"#
        )
    };
    format!(
        r#"{{"schema_version":"1.0","execution":{{{execution}}},"targets":[{},{}]}}"#,
        target("one"),
        target("two")
    )
}

fn jobs(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(format!("jobs-{name}"))).unwrap_or_default()
}

/// R483-04: under a budget lowered to two jobs, targets that declare four
/// workers still run, each with two jobs, in full and in quick runs. Before
/// the fix a full run left them pending and failed with "unsatisfied
/// prerequisites", and a quick run gave its first target four jobs.
#[cfg(unix)]
#[test]
fn declared_widths_above_a_lowered_budget_run_at_the_budget() {
    let home = tempfile::tempdir().unwrap();
    let budget = r#""host_jobs":4,"worker_budget":4"#;
    let first = repo(
        &holding("full", budget)
            .replace(r#""runner":"custom""#, r#""heavy":true,"runner":"custom""#),
    );
    let (mut holder, rx, _stdout) = spawn(command(
        first.path(),
        home.path(),
        &["test", "--mode", "full"],
    ));
    wait_for(&rx, "starting target 'holder'");

    for mode in ["full", "quick"] {
        let linked = linked_worktree(first.path(), mode, &two_wide(mode, budget, 4));
        let output = run(&linked, home.path(), &["test", "--mode", mode]);
        let err = stderr(&output);
        assert_eq!(output.status.code(), Some(0), "{mode}: {err}");
        assert!(!err.contains("unsatisfied prerequisites"), "{mode}: {err}");
        assert_eq!(jobs(&linked, "one"), "2", "{mode}: {err}");
        assert_eq!(jobs(&linked, "two"), "2", "{mode}: {err}");
    }

    release(first.path());
    assert!(holder.wait().unwrap().success());
}

/// R483-05: the first heavy run on an idle host is held to the host budget,
/// with no per-run budget and with a per-run budget above the host's.
/// Before the fix both runs gave their targets no job limit or nine jobs.
#[cfg(unix)]
#[test]
fn the_first_run_is_held_to_the_host_budget() {
    let home = tempfile::tempdir().unwrap();
    for execution in [r#""host_jobs":2"#, r#""host_jobs":2,"worker_budget":9"#] {
        let dir = repo(&two_wide("full", execution, 1).replace(r#""workers":1,"#, ""));
        let output = run(dir.path(), home.path(), &["test", "--mode", "full"]);
        let err = stderr(&output);
        assert_eq!(output.status.code(), Some(0), "{execution}: {err}");
        assert_eq!(jobs(dir.path(), "one"), "2", "{execution}: {err}");
        assert_eq!(jobs(dir.path(), "two"), "2", "{execution}: {err}");
    }
}

/// The saved `JUnit` report of every run of the `reporter` target under
/// `dir`, by the suite name it carries.
fn saved_suites(dir: &Path) -> Vec<String> {
    let mut suites = Vec::new();
    let runs = dir.join("target/codeflow-gate");
    for entry in std::fs::read_dir(&runs).unwrap().flatten() {
        let report = entry.path().join("reporter/junit.xml");
        if let Ok(text) = std::fs::read_to_string(report) {
            let name = text
                .split("testsuite name=\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .unwrap_or_default()
                .to_string();
            suites.push(name);
        }
    }
    suites.sort();
    suites
}

/// R483-03 (journey): two full gates in one worktree write the same
/// configured report path. Each run keeps its own report: the second run's
/// target waits until the first has written and collected its report.
/// Before the fix the first run saved the second run's report as its own.
#[cfg(unix)]
#[test]
fn two_gates_in_one_worktree_keep_their_own_reports() {
    let home = tempfile::tempdir().unwrap();
    let command_text = "mkdir -p out; if mkdir claim-a 2>/dev/null; then n=A; else n=B; fi; printf '<testsuites><testsuite name=\\\"%s\\\" tests=\\\"1\\\" failures=\\\"0\\\"><testcase name=\\\"%s\\\"/></testsuite></testsuites>' $n $n > out/junit.xml; if [ $n = A ]; then while ! test -f release; do sleep 0.01; done; fi";
    let config = format!(
        r#"{{"schema_version":"1.0","targets":[
          {{"name":"reporter","runner":"custom","timeout_seconds":60,"report":{{"format":"junit","path":"out/junit.xml"}},"modes":{{"full":{{"command":"{command_text}"}}}}}}
        ]}}"#
    );
    let dir = repo(&config);
    let (mut first, rx_first, _out_first) = spawn(command(
        dir.path(),
        home.path(),
        &["test", "--mode", "full"],
    ));
    wait_for(&rx_first, "starting target 'reporter'");
    wait_for_file(&dir.path().join("out/junit.xml"));

    let (mut second, rx_second, _out_second) = spawn(command(
        dir.path(),
        home.path(),
        &["test", "--mode", "full"],
    ));
    // The second run either waits for the first run's report, or (before
    // the fix) overwrites it at once; give it time to do the latter.
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) && second.try_wait().unwrap().is_none() {
        std::thread::sleep(Duration::from_millis(20));
    }
    release(dir.path());
    assert!(first.wait().unwrap().success());
    assert!(second.wait().unwrap().success());
    drop((rx_first, rx_second));
    assert_eq!(saved_suites(dir.path()), ["A", "B"]);
}
