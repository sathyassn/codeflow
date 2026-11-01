//! TSK-1128: a quick run under `codeflow reserve` uses the reserve's slot,
//! a forged hand-over grants nothing, a program only named `codeflow` gets
//! no lock, and a nested run's surviving target keeps the slot busy.
//! TSK-1145: other runs are admitted beside all of them in their own slots,
//! and heavy work waits a bounded time only for a host-exclusive target.

use super::{command, git, held_until_released, repo, run, spawn, stderr, wait_for};
use std::path::Path;
use std::time::{Duration, Instant};

/// The binary under test, the same one `command` runs, so a fail-before
/// run with `CODEFLOW_TEST_GATE_BIN` nests the same binary.
fn bin() -> String {
    std::env::var("CODEFLOW_TEST_GATE_BIN")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_codeflow").to_string())
}

/// A quick config whose one heavy target marks its start, waits for
/// `release` (when `hold`) and marks its end.
fn heavy_quick(hold: bool) -> String {
    let wait = if hold {
        "while ! test -f release; do sleep 0.01; done; "
    } else {
        ""
    };
    format!(
        r#"{{"schema_version":"1.0","execution":{{}},"targets":[
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
/// runs the quick run's heavy target under the reserve's slot, and heavy
/// runs from another worktree are admitted beside it (TSK-1145).
#[test]
fn a_quick_run_under_reserve_runs_its_heavy_targets_under_the_reserved_lock() {
    let home = tempfile::tempdir().unwrap();
    let first = repo(&heavy_quick(true));
    let linked = linked_worktree(first.path(), &heavy_quick(false));
    let (mut reserve, rx, _stdout) = spawn(command(
        first.path(),
        home.path(),
        &["reserve", "--", &bin(), "test", "--mode", "quick"],
    ));
    wait_for(&rx, "[codeflow reserve] started process");
    wait_for_file(&first.path().join("heavy-started"));

    // Another worktree's quick run is admitted beside the reserve, in the
    // next slot: the nested run took no slot of its own (TSK-1145).
    let contender = run(&linked, home.path(), &["test", "--mode", "quick"]);
    assert_eq!(contender.status.code(), Some(0), "{}", stderr(&contender));
    assert!(linked.join("heavy-ran").exists(), "{}", stderr(&contender));
    assert!(home.path().join("locks/full-gate-1.lock").exists());
    assert!(
        !home.path().join("locks/full-gate-2.lock").exists(),
        "the nested run shares the reserve's slot"
    );
    let reserved = run(
        &linked,
        home.path(),
        &["reserve", "--", "touch", "reserved"],
    );
    assert_eq!(reserved.status.code(), Some(0), "{}", stderr(&reserved));
    assert!(linked.join("reserved").exists());

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
}

/// AC-5, AC-6: an essential gate under `codeflow reserve` never inherits:
/// it takes a slot of its own beside the reserve's (TSK-1145).
#[test]
fn an_essential_gate_under_reserve_takes_its_own_slot() {
    let home = tempfile::tempdir().unwrap();
    let dir = repo(
        r#"{"schema_version":"1.0","targets":[{"name":"gate","runner":"custom","modes":{"essential":{"command":"touch essential-ran"}}}]}"#,
    );
    let output = run(
        dir.path(),
        home.path(),
        &["reserve", "--", &bin(), "test", "--mode", "essential"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(!stderr(&output).contains("enclosing `codeflow reserve`"));
    assert!(dir.path().join("essential-ran").exists());
    assert!(home.path().join("locks/full-gate-1.lock").exists());
}

/// AC-2: a process that opens the lock files itself and names them in the
/// hand-over variable gains nothing while another gate holds the slot: it
/// falls back to ordinary admission and takes a slot of its own.
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
            &bin(),
        ])
        .current_dir(&linked)
        .env("CODEFLOW_HOME", home.path())
        .env("CODEFLOW_RESERVED_LOCK_FDS", "7,8")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CI")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(!stderr(&output).contains("enclosing `codeflow reserve`"));
    assert!(linked.join("heavy-ran").exists());
    assert!(home.path().join("locks/full-gate-1.lock").exists());
    std::fs::write(first.path().join("release"), "done").unwrap();
    assert!(holder.wait().unwrap().success());
}

/// TSK-1128 AC-4, TSK-1145 AC-4: `--wait-seconds` bounds how long heavy
/// quick work waits while a host-exclusive target holds the host: it
/// refuses after the bound, and runs once the holder ends inside it.
#[test]
fn a_direct_quick_run_waits_for_a_host_exclusive_target_up_to_its_bound() {
    let home = tempfile::tempdir().unwrap();
    let first = repo(
        r#"{"schema_version":"1.0","targets":[{"name":"holder","host_exclusive":true,"runner":"custom","timeout_seconds":60,"modes":{"full":{"command":"while ! test -f release; do sleep 0.01; done"}}}]}"#,
    );
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
    let reserved = run(
        &linked,
        home.path(),
        &["reserve", "--", "touch", "reserved"],
    );
    assert_eq!(reserved.status.code(), Some(1), "{}", stderr(&reserved));
    assert!(
        stderr(&reserved).contains("host-exclusive"),
        "{}",
        stderr(&reserved)
    );

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

/// A config with an essential target, for the gate that must refuse.
const ESSENTIAL_PASS: &str = r#"{"schema_version":"1.0","targets":[
  {"name":"pass","runner":"custom","modes":{"essential":{"command":"exit 0"},"quick":{"command":"exit 0"}}}
]}"#;

/// Records which inherited descriptors are the lock files named in its
/// arguments, unlocks every inherited descriptor, marks that it did, and
/// waits for `release`.
const UNLOCKER: &str = r#"#!/bin/sh
echo "${CODEFLOW_RESERVED_LOCK_FDS-unset}" > env.txt
exec perl -e '
use Fcntl qw(:flock);
my @locks = map { join ":", (stat $_)[0, 1] } @ARGV;
my @held;
for my $fd (3 .. 64) {
    open(my $fh, "<&=", $fd) or next;
    my $id = join ":", (stat $fh)[0, 1];
    push @held, $fd if grep { $_ eq $id } @locks;
    flock($fh, LOCK_UN);
}
open(my $out, ">", "fds.txt") or die; print $out join(",", @held), "\n"; close $out;
open(my $mark, ">", "unlocked") or die; close $mark;
select(undef, undef, undef, 0.02) until -e "release";
' "$@"
"#;

/// F1 (Grok round 01), AC-2: a program that is not this codeflow binary
/// gets no lock descriptor and no hand-over variable, even when it is named
/// `codeflow`, run by path or found on `PATH`; unlocking every descriptor it
/// has cannot free the reserve's lock.
#[test]
fn a_program_only_named_codeflow_receives_no_lock() {
    use std::os::unix::fs::PermissionsExt;
    for by_path in [true, false] {
        let home = tempfile::tempdir().unwrap();
        let first = repo(&heavy_quick(false));
        let linked = linked_worktree(first.path(), ESSENTIAL_PASS);
        let tools = tempfile::tempdir().unwrap();
        let fake = tools.path().join("codeflow");
        std::fs::write(&fake, UNLOCKER).unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let home_lock = home.path().join("locks/full-gate.lock");
        let repo_lock = first.path().join(".git/codeflow/full-gate.lock");
        let program = if by_path {
            fake.to_str().unwrap().to_string()
        } else {
            "codeflow".to_string()
        };
        let mut cmd = command(
            first.path(),
            home.path(),
            &[
                "reserve",
                "--",
                &program,
                home_lock.to_str().unwrap(),
                repo_lock.to_str().unwrap(),
            ],
        );
        let path = format!(
            "{}:{}",
            tools.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        cmd.env("PATH", path);
        let (mut reserve, rx, _stdout) = spawn(cmd);
        wait_for(&rx, "[codeflow reserve] started process");
        wait_for_file(&first.path().join("unlocked"));

        let env = std::fs::read_to_string(first.path().join("env.txt")).unwrap();
        assert_eq!(env.trim(), "unset", "by path {by_path}");
        let fds = std::fs::read_to_string(first.path().join("fds.txt")).unwrap();
        assert_eq!(fds.trim(), "", "by path {by_path}: lock descriptors {fds}");
        // The reserve's slot stays held, so the next run takes slot 1.
        let essential = run(&linked, home.path(), &["test", "--mode", "essential"]);
        assert_eq!(
            essential.status.code(),
            Some(0),
            "by path {by_path}: {}",
            stderr(&essential)
        );
        assert!(home.path().join("locks/full-gate-1.lock").exists());

        std::fs::write(first.path().join("release"), "done").unwrap();
        let status = reserve.wait().unwrap();
        let lines: Vec<String> = rx.try_iter().collect();
        assert!(status.success(), "by path {by_path}: {lines:#?}");
    }
}

/// F2 (Grok round 01), AC-5: a nested quick run records its heavy target's
/// process group on the reserve's record. When the nested run is killed and
/// the reserve ends while that target still runs, its slot stays busy
/// until the target exits, so the next run takes another slot.
#[test]
fn a_surviving_nested_target_keeps_the_lock_busy() {
    let home = tempfile::tempdir().unwrap();
    // Heavy targets are supervised and end with the run that started them.
    // The light `survivor` is not: it starts while the heavy `holder` keeps
    // the lock held, records its group, and outlives the killed run.
    let first = repo(
        r#"{"schema_version":"1.0","execution":{"parallel":true},"targets":[
          {"name":"first","heavy":true,"timeout_seconds":60,"runner":"custom","modes":{"quick":{"command":"touch first-ran"}}},
          {"name":"holder","heavy":true,"timeout_seconds":60,"runner":"custom","modes":{"quick":{"command":"while ! test -f release; do sleep 0.01; done"}}},
          {"name":"survivor","heavy":false,"timeout_seconds":60,"runner":"custom","requires":["first"],"modes":{"quick":{"command":"echo $$ > group.txt; sleep 30"}}}
        ]}"#,
    );
    let linked = linked_worktree(first.path(), ESSENTIAL_PASS);
    let (mut reserve, rx, _stdout) = spawn(command(
        first.path(),
        home.path(),
        &["reserve", "--", &bin(), "test", "--mode", "quick"],
    ));
    let started = wait_for(&rx, "[codeflow reserve] started process");
    let nested: u32 = started
        .rsplit(' ')
        .next()
        .and_then(|pid| pid.trim().parse().ok())
        .unwrap_or_else(|| panic!("no pid in {started}"));
    let group_file = first.path().join("group.txt");
    let group: u32 = (0..400)
        .find_map(|_| {
            let parsed = std::fs::read_to_string(&group_file)
                .unwrap_or_default()
                .trim()
                .parse()
                .ok();
            if parsed.is_none() {
                std::thread::sleep(Duration::from_millis(50));
            }
            parsed
        })
        .expect("the target wrote its group");
    let record = home.path().join("locks/full-gate.record");
    let recorded = (0..200).any(|_| {
        let text = std::fs::read_to_string(&record).unwrap_or_default();
        let found = text.lines().any(|line| line == format!("group={group}"));
        if !found {
            std::thread::sleep(Duration::from_millis(50));
        }
        found
    });
    assert!(
        recorded,
        "the nested run recorded its target's group: {}",
        std::fs::read_to_string(&record).unwrap_or_default()
    );

    let killed = std::process::Command::new("kill")
        .args(["-KILL", &nested.to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    reserve.wait().unwrap();
    let alive = |g: u32| {
        std::process::Command::new("kill")
            .args(["-0", "--", &format!("-{g}")])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    };
    assert!(
        alive(group),
        "the target outlived the nested run and reserve"
    );

    // The surviving target keeps slot 0 busy: the next run takes slot 1.
    let beside = run(&linked, home.path(), &["test", "--mode", "essential"]);
    let err = stderr(&beside);
    assert_eq!(beside.status.code(), Some(0), "{err}");
    assert!(home.path().join("locks/full-gate-1.lock").exists(), "{err}");

    std::process::Command::new("kill")
        .args(["-KILL", "--", &format!("-{group}")])
        .status()
        .unwrap();
    let gone = (0..100).any(|_| {
        let gone = !alive(group);
        if !gone {
            std::thread::sleep(Duration::from_millis(50));
        }
        gone
    });
    assert!(gone, "the target exited");
    let after = run(&linked, home.path(), &["test", "--mode", "essential"]);
    assert!(after.status.success(), "{}", stderr(&after));
}
