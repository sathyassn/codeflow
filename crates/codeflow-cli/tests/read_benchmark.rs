//! Benchmark (TSK-110 AC-3, SPC-013 R-103, R-104): the read commands on
//! 10,000 task records across ten integration lines, with thousands of stale
//! task branches (merged, squash-landed and never landed). The first run on
//! each operating system recorded its `read_benchmark_baseline-<os>.json`;
//! later runs are guarded against the baseline of their own system:
//!
//! - git processes started: at most the baseline count (deterministic);
//! - peak memory: at most 1.5 times the baseline, plus 16 MiB;
//! - latency: the command's time over a calibration of plain git reads on
//!   the same fixture, at most twice the baseline ratio, so the budget holds
//!   on a slower or faster machine.
//!
//! The ratio travels across machines, not across operating systems: the
//! calibration is a few short git processes, and starting a process costs
//! several times as much on macOS as on Linux, while the commands spend
//! most of their time in their own work. The same commands take about as
//! long on both (status 48.8 s on macOS, 48.4 s on the hosted Linux runner)
//! against calibrations of 296 ms and 101 ms.
//!
//! It is ignored by `cargo test`; `scripts/journey-gate.py` runs it with
//! `--include-ignored`. `CODEFLOW_BENCH_RECORD=1` rewrites the baseline from
//! this run.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const LINES: usize = 10;
const TASKS_PER_LINE: usize = 1_000;
const MERGED_PER_LINE: usize = 200;
const SQUASHED_PER_LINE: usize = 50;
const OPEN_PER_LINE: usize = 50;
/// Calibration runs. Each command runs once: a run of tens of seconds
/// varies far less than its two-times latency budget.
const RUNS: usize = 3;
const COMMANDS: [&[&str]; 4] = [
    &["work", "next"],
    &["status"],
    &["orient"],
    &["validate", "--docs"],
];

#[path = "support/process_folders.rs"]
mod process_folders;
use process_folders::isolated_home;
#[path = "support/outcome.rs"]
mod outcome;

fn with_env(command: &mut Command) -> &mut Command {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    command
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Bench")
        .env("GIT_AUTHOR_EMAIL", "bench@example.test")
        .env("GIT_COMMITTER_NAME", "Bench")
        .env("GIT_COMMITTER_EMAIL", "bench@example.test")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_TRACE2_EVENT")
}

fn run(program: &str, root: &Path, args: &[&str]) -> String {
    let out = with_env(&mut Command::new(program))
        .args(args)
        .current_dir(root)
        .output()
        .expect("command runs");
    assert!(
        out.status.success(),
        "{program} {args:?}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn codeflow(root: &Path, args: &[&str]) -> String {
    run(env!("CARGO_BIN_EXE_codeflow"), root, args)
}

fn git(root: &Path, args: &[&str]) -> String {
    run("git", root, args)
}

fn line_name(line: usize) -> String {
    format!("integration/EPC-{line:03}-line-{line}")
}

fn task_id(line: usize, k: usize) -> String {
    format!("TSK-{:03}", (line - 1) * TASKS_PER_LINE + k)
}

/// A record written by the CLI, with its identity replaced.
fn record(template: &str, from_id: &str, to_id: &str, uid: &str, title: &str) -> String {
    template
        .lines()
        .map(|line| {
            if line.starts_with("uid:") {
                format!("uid: {uid}")
            } else if line.starts_with("title:") {
                format!("title: \"{title}\"")
            } else {
                line.replace(from_id, to_id)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// A synthetic record identity: the `n`th uid of the inventory.
fn synthetic_uid(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}

/// The epic and task records the CLI writes for line 1, filled in as their
/// author does before committing them (criteria and an `## Outcome` line),
/// then removed: every inventory record is one of these with its identity
/// replaced. Returns `(epic, task)`.
fn templates(root: &Path) -> (String, String) {
    codeflow(root, &["epic", "new", "Template"]);
    codeflow(
        root,
        &[
            "task",
            "new",
            "--epic",
            "EPC-001",
            "--into",
            &line_name(1),
            "Template",
        ],
    );
    let epic_path = root.join("project-management/epics/EPC-001.md");
    let task_path = root.join("project-management/tasks/TSK-001.md");
    let epic = outcome::with_outcome(
        &std::fs::read_to_string(&epic_path)
            .unwrap()
            .replace("- AC-1\n", "- AC-1 When run, the system shall work.\n"),
    );
    let task = outcome::with_outcome(
        &std::fs::read_to_string(&task_path).unwrap().replace(
            "- AC-1\n",
            "- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the flow shall pass.\n",
        ),
    );
    std::fs::remove_file(epic_path).unwrap();
    std::fs::remove_file(task_path).unwrap();
    (epic, task)
}

/// The epic record of `line`, from the epic template.
fn epic_record(template: &str, line: usize, uid: &str) -> String {
    record(
        template,
        "EPC-001",
        &format!("EPC-{line:03}"),
        uid,
        &format!("Line {line}"),
    )
}

/// The `k`th task record of `line`, from the task template: every tenth
/// task starts a chain, and the others depend on the task before them.
fn task_record(template: &str, line: usize, k: usize, uid: &str) -> String {
    let id = task_id(line, k);
    let text = record(template, "TSK-001", &id, uid, &format!("Task {id}"))
        .replace(&line_name(1), &line_name(line))
        .replace("EPC-001", &format!("EPC-{line:03}"));
    if k % 10 == 1 {
        text
    } else {
        let previous = task_id(line, k - 1);
        text.replacen("depends_on: []", &format!("depends_on: [{previous}]"), 1)
    }
}

fn data(stream: &mut Vec<u8>, bytes: &[u8]) {
    writeln!(stream, "data {}", bytes.len()).unwrap();
    stream.extend_from_slice(bytes);
    stream.push(b'\n');
}

struct Stream {
    bytes: Vec<u8>,
    mark: usize,
    time: u64,
}

impl Stream {
    fn blob(&mut self, content: &str) -> usize {
        self.mark += 1;
        writeln!(self.bytes, "blob\nmark :{}", self.mark).unwrap();
        data(&mut self.bytes, content.as_bytes());
        self.mark
    }

    /// A commit on `reference` from `from`, merging `merge`, adding `files`.
    fn commit(
        &mut self,
        reference: &str,
        from: &str,
        merge: Option<usize>,
        files: &[(String, usize)],
        message: &str,
    ) -> usize {
        self.mark += 1;
        self.time += 1;
        writeln!(
            self.bytes,
            "commit {reference}\nmark :{}\ncommitter Bench <bench@example.test> {} +0000",
            self.mark, self.time
        )
        .unwrap();
        data(&mut self.bytes, message.as_bytes());
        writeln!(self.bytes, "from {from}").unwrap();
        if let Some(merge) = merge {
            writeln!(self.bytes, "merge :{merge}").unwrap();
        }
        for (path, blob) in files {
            writeln!(self.bytes, "M 100644 :{blob} {path}").unwrap();
        }
        self.mark
    }

    fn reset(&mut self, reference: &str, to: usize) {
        writeln!(self.bytes, "reset {reference}\nfrom :{to}\n").unwrap();
    }
}

/// A fresh full-tier project holding the benchmark inventory, checked out on
/// the first line. Returns the project and the counts it holds.
#[allow(clippy::too_many_lines)] // One fixture writes the whole inventory in order.
fn inventory() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    codeflow(&root, &["init", "--yes", "--full"]);
    let default = git(&root, &["branch", "--show-current"]);
    let base = git(&root, &["rev-parse", "HEAD"]);
    codeflow(&root, &["ids", "seed"]);
    git(&root, &["branch", &line_name(1), &base]);
    let (epic, task) = templates(&root);

    let mut stream = Stream {
        bytes: Vec::new(),
        mark: 0,
        time: 1_700_000_000,
    };
    let mut uid = 0_u64;
    let mut next_uid = || {
        uid += 1;
        synthetic_uid(uid)
    };
    let mut plan_files = Vec::new();
    for line in 1..=LINES {
        let blob = stream.blob(&epic_record(&epic, line, &next_uid()));
        plan_files.push((format!("project-management/epics/EPC-{line:03}.md"), blob));
        for k in 1..=TASKS_PER_LINE {
            let blob = stream.blob(&task_record(&task, line, k, &next_uid()));
            plan_files.push((
                format!("project-management/tasks/{}.md", task_id(line, k)),
                blob,
            ));
        }
    }
    let plan = stream.commit(
        "refs/heads/plan/inventory",
        &base,
        None,
        &plan_files,
        "chore: plan ten lines",
    );
    for line in 1..=LINES {
        let reference = format!("refs/heads/{}", line_name(line));
        let marker = stream.blob(&format!("# Line {line}\n"));
        let mut tip = stream.commit(
            &reference,
            &format!(":{plan}"),
            None,
            &[(format!("docs/line-{line}.md"), marker)],
            "chore: open the line",
        );
        let opened = tip;
        let mut k = 0;
        for n in 0..MERGED_PER_LINE {
            k += 1;
            let file = stream.blob(&format!("merged {line} {n}\n"));
            let branch = format!("refs/heads/task/{}-merged", task_id(line, k));
            let work = stream.commit(
                &branch,
                &format!(":{tip}"),
                None,
                &[(format!("work/{line}/merged-{n}.txt"), file)],
                "feat: merged work",
            );
            tip = stream.commit(
                &reference,
                &format!(":{tip}"),
                Some(work),
                &[(format!("work/{line}/merged-{n}.txt"), file)],
                "chore: land merged work",
            );
        }
        for n in 0..SQUASHED_PER_LINE {
            k += 1;
            let file = stream.blob(&format!("squashed {line} {n}\n"));
            let path = format!("work/{line}/squashed-{n}.txt");
            let branch = format!("refs/heads/task/{}-squashed", task_id(line, k));
            stream.commit(
                &branch,
                &format!(":{opened}"),
                None,
                &[(path.clone(), file)],
                "feat: squashed work",
            );
            tip = stream.commit(
                &reference,
                &format!(":{tip}"),
                None,
                &[(path, file)],
                "chore: land squashed work",
            );
        }
        for n in 0..OPEN_PER_LINE {
            k += 1;
            let file = stream.blob(&format!("open {line} {n}\n"));
            let branch = format!("refs/heads/task/{}-open", task_id(line, k));
            stream.commit(
                &branch,
                &format!(":{opened}"),
                None,
                &[(format!("work/{line}/open-{n}.txt"), file)],
                "feat: open work",
            );
        }
        stream.reset(&reference, tip);
    }
    let mut child = with_env(&mut Command::new("git"))
        .args(["fast-import", "--quiet"])
        .current_dir(&root)
        .stdin(Stdio::piped())
        .spawn()
        .expect("git fast-import");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&stream.bytes)
        .unwrap();
    assert!(child.wait().unwrap().success(), "fast-import failed");
    git(&root, &["switch", "-q", "--discard-changes", &line_name(1)]);
    git(&root, &["branch", "-q", "-D", "plan/inventory"]);
    let _ = default;
    (dir, root)
}

/// One measured run: wall time, peak resident memory in KiB, and the git
/// processes the command started.
struct Measure {
    wall: Duration,
    peak_kib: u64,
    git_processes: usize,
}

fn measure(root: &Path, program: &str, args: &[&str], trace: &Path) -> Measure {
    let _ = std::fs::remove_file(trace);
    let start = Instant::now();
    let child = with_env(&mut Command::new(program))
        .env("GIT_TRACE2_EVENT", trace)
        .args(args)
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    let pid = libc::pid_t::try_from(child.id()).expect("pid");
    let mut status = 0;
    // SAFETY: an all-zero rusage is a valid value for wait4 to fill.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: `pid` is our own child, not yet waited for.
    let waited = unsafe { libc::wait4(pid, &raw mut status, 0, &raw mut usage) };
    let wall = start.elapsed();
    assert_eq!(waited, pid, "wait4");
    if !(libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0) {
        // The measured run discards its output; run it again to say why.
        // The failure reported is the measured run's; the rerun only shows
        // its output, under its own status. On ten thousand records the
        // output is long and its verdict comes last, so the message keeps
        // the end.
        let out = with_env(&mut Command::new(program))
            .args(args)
            .current_dir(root)
            .output()
            .expect("command reruns");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let lines: Vec<&str> = text.lines().collect();
        let tail = lines[lines.len().saturating_sub(40)..].join("\n");
        panic!(
            "{program} {args:?} failed with {}; a rerun to show why ended with {}, and the last {} of its {} output lines are:\n{tail}",
            wait_status(status),
            out.status,
            lines.len().min(40),
            lines.len()
        );
    }
    std::mem::forget(child);
    let raw = u64::try_from(usage.ru_maxrss).unwrap_or(0);
    // macOS reports bytes, Linux KiB.
    let peak_kib = if cfg!(target_os = "macos") {
        raw / 1024
    } else {
        raw
    };
    let git_processes = std::fs::read_to_string(trace)
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("\"event\":\"start\""))
        .count();
    Measure {
        wall,
        peak_kib,
        git_processes,
    }
}

/// A `wait4` status in words: the exit code, or the signal that ended the
/// process.
fn wait_status(status: libc::c_int) -> String {
    if libc::WIFEXITED(status) {
        format!("exit code {}", libc::WEXITSTATUS(status))
    } else if libc::WIFSIGNALED(status) {
        format!("signal {}", libc::WTERMSIG(status))
    } else {
        format!("wait status {status}")
    }
}

fn median(mut values: Vec<Duration>) -> Duration {
    values.sort();
    values[values.len() / 2]
}

/// Plain git reads of the same inventory: every line tip's tree and every
/// ref. The latency budget is a ratio to this, so it travels across machines.
fn calibration(root: &Path, trace: &Path) -> Duration {
    let mut runs = Vec::new();
    for _ in 0..RUNS {
        let mut total = Duration::ZERO;
        for line in 1..=LINES {
            total += measure(root, "git", &["ls-tree", "-r", &line_name(line)], trace).wall;
        }
        total += measure(root, "git", &["for-each-ref"], trace).wall;
        runs.push(total);
    }
    median(runs)
}

/// This operating system's baseline: the latency ratio is comparable only
/// between runs on the same system (see the module notes).
fn baseline_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "tests/read_benchmark_baseline-{}.json",
        std::env::consts::OS
    ))
}

#[test]
#[ignore = "benchmark: run by scripts/journey-gate.py in release mode"]
fn read_commands_on_ten_thousand_records_stay_within_their_budgets() {
    let started = Instant::now();
    let (dir, root) = inventory();
    eprintln!("inventory written in {:?}", started.elapsed());
    let trace = dir.path().join("trace.json");

    // The inventory is what it claims: ten lines of a thousand tasks, and the
    // stale branches visible.
    let tasks = std::fs::read_dir(root.join("project-management/tasks"))
        .unwrap()
        .count();
    assert_eq!(tasks, LINES * TASKS_PER_LINE);
    let branches = git(
        &root,
        &["for-each-ref", "--format=%(refname)", "refs/heads/task/"],
    )
    .lines()
    .count();
    assert_eq!(
        branches,
        LINES * (MERGED_PER_LINE + SQUASHED_PER_LINE + OPEN_PER_LINE)
    );
    let next = codeflow(&root, &["work", "next"]);
    for line in 1..=LINES {
        assert!(
            next.contains(&line_name(line)),
            "line {line} unread:\n{}",
            &next[..next.len().min(2000)]
        );
    }

    let calibration = calibration(&root, &trace);
    let mut measured = BTreeMap::new();
    for args in COMMANDS {
        let exe = env!("CARGO_BIN_EXE_codeflow");
        let run = measure(&root, exe, args, &trace);
        let ratio = run.wall.as_secs_f64() / calibration.as_secs_f64();
        measured.insert(
            args.join(" "),
            serde_json::json!({
                "wall_ms": run.wall.as_millis(),
                "latency_ratio": (ratio * 100.0).round() / 100.0,
                "peak_kib": run.peak_kib,
                "git_processes": run.git_processes,
            }),
        );
    }
    let report = serde_json::json!({
        "inventory": {
            "lines": LINES,
            "task_records": LINES * TASKS_PER_LINE,
            "stale_task_branches": branches,
        },
        "calibration_ms": calibration.as_millis(),
        "commands": measured,
    });
    eprintln!("{}", serde_json::to_string_pretty(&report).unwrap());

    if std::env::var_os("CODEFLOW_BENCH_RECORD").is_some() {
        let mut recorded = report.clone();
        recorded["recorded_on"] = serde_json::json!(format!(
            "{}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
        std::fs::write(
            baseline_path(),
            serde_json::to_string_pretty(&recorded).unwrap() + "\n",
        )
        .unwrap();
        return;
    }
    let baseline: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(baseline_path()).expect("the benchmark baseline is recorded"),
    )
    .expect("baseline JSON");
    let over = over_budget(&baseline, &measured);
    assert!(over.is_empty(), "over budget:\n{}", over.join("\n"));
}

/// The inventory's records pass `validate --docs`, which the benchmark times
/// (TSK-1068). The benchmark is ignored and runs only in the release gate,
/// so this small inventory, built from the same templates and record
/// builders, catches a record rule the generator misses in the ordinary Unix
/// workspace test runs. It checks that validation passes, not that the
/// records raise no warnings.
#[test]
fn the_inventory_records_pass_validate_docs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    codeflow(&root, &["init", "--yes", "--full"]);
    let base = git(&root, &["rev-parse", "HEAD"]);
    codeflow(&root, &["ids", "seed"]);
    git(&root, &["branch", &line_name(1), &base]);
    let (epic, task) = templates(&root);
    let pm = root.join("project-management");
    std::fs::write(
        pm.join("epics/EPC-001.md"),
        epic_record(&epic, 1, &synthetic_uid(1)),
    )
    .unwrap();
    for k in 1..=2 {
        std::fs::write(
            pm.join(format!("tasks/{}.md", task_id(1, k))),
            task_record(&task, 1, k, &synthetic_uid(1 + u64::try_from(k).unwrap())),
        )
        .unwrap();
    }
    codeflow(&root, &["validate", "--docs"]);
}

/// A measured command that fails says how, with its own output, rather
/// than a raw wait status (TSK-1068): the measured run's status, then the
/// rerun's status and output, each named.
#[test]
fn a_failed_measured_command_shows_its_output() {
    let dir = tempfile::tempdir().unwrap();
    let panic = std::panic::catch_unwind(|| {
        measure(
            dir.path(),
            "git",
            &["no-such-command"],
            &dir.path().join("trace.json"),
        );
    })
    .expect_err("a failed command panics");
    let message = panic.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(
        message.contains("failed with exit code 1; a rerun to show why ended with exit status: 1")
            && message.contains("is not a git command"),
        "{message}"
    );
}

/// When the measured run and the rerun end differently, the failure reported
/// first is the measured run's, and the output shown is the rerun's under
/// the rerun's own status: here the measured run exits 3 and the rerun
/// succeeds.
#[test]
fn a_failure_reports_the_measured_status_not_the_rerun_s() {
    let dir = tempfile::tempdir().unwrap();
    let script = "if [ -e ran ]; then echo second run; exit 0; fi; : >ran; echo first run; exit 3";
    let panic = std::panic::catch_unwind(|| {
        measure(
            dir.path(),
            "sh",
            &["-c", script],
            &dir.path().join("trace.json"),
        );
    })
    .expect_err("a failed command panics");
    let message = panic.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(
        message.starts_with(&format!(
            "sh [\"-c\", {script:?}] failed with exit code 3; a rerun to show why ended with exit status: 0"
        )),
        "{message}"
    );
    // The arguments printed above hold the script text, so only the output
    // section after them is compared.
    assert_eq!(
        message
            .split_once(" output lines are:\n")
            .map(|(_, output)| output),
        Some("second run"),
        "{message}"
    );
}

/// The measured run's own status is what a failure reports, whether the
/// process exited or was killed.
#[test]
fn a_wait_status_names_its_exit_code_or_signal() {
    // The encodings wait4 returns on Linux and macOS: the exit code in the
    // second byte; a terminating signal in the low seven bits.
    assert_eq!(wait_status(1 << 8), "exit code 1");
    assert_eq!(wait_status(0), "exit code 0");
    assert_eq!(wait_status(libc::SIGKILL), "signal 9");
}

/// The budgets a run is held to, from the recorded baseline: no more git
/// processes; peak memory at most 1.5 times plus 16 MiB; a latency ratio at
/// most twice. Returns each command over a budget, or with no baseline.
fn over_budget(
    baseline: &serde_json::Value,
    measured: &BTreeMap<String, serde_json::Value>,
) -> Vec<String> {
    let mut over = Vec::new();
    for (command, now) in measured {
        let was = &baseline["commands"][command];
        let (Some(processes_budget), Some(peak_was), Some(ratio_was)) = (
            was["git_processes"].as_u64(),
            was["peak_kib"].as_u64(),
            was["latency_ratio"].as_f64(),
        ) else {
            over.push(format!("{command}: no baseline"));
            continue;
        };
        let processes = now["git_processes"].as_u64().unwrap_or(u64::MAX);
        if processes > processes_budget {
            over.push(format!(
                "{command}: {processes} git processes, budget {processes_budget}"
            ));
        }
        let peak = now["peak_kib"].as_u64().unwrap_or(u64::MAX);
        let peak_budget = peak_was * 3 / 2 + 16 * 1024;
        if peak > peak_budget {
            over.push(format!(
                "{command}: peak {peak} KiB, budget {peak_budget} KiB"
            ));
        }
        let ratio = now["latency_ratio"].as_f64().unwrap_or(f64::MAX);
        let ratio_budget = ratio_was * 2.0;
        if ratio > ratio_budget {
            over.push(format!(
                "{command}: latency ratio {ratio}, budget {ratio_budget:.2}"
            ));
        }
    }
    over
}

/// The regression check itself: the recorded baseline passes, and a run
/// over any one budget, or a command with no baseline, is named.
#[test]
fn the_budget_check_flags_a_run_over_its_baseline() {
    let baseline: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(baseline_path()).expect("the benchmark baseline is recorded"),
    )
    .expect("baseline JSON");
    let recorded: BTreeMap<String, serde_json::Value> = baseline["commands"]
        .as_object()
        .expect("recorded commands")
        .iter()
        .map(|(command, value)| (command.clone(), value.clone()))
        .collect();
    let mut names: Vec<String> = COMMANDS.iter().map(|args| args.join(" ")).collect();
    names.sort();
    assert_eq!(recorded.keys().cloned().collect::<Vec<_>>(), names);
    assert!(over_budget(&baseline, &recorded).is_empty());
    let command = "work next".to_string();
    let was = recorded[&command].clone();
    for (field, value, named) in [
        (
            "git_processes",
            serde_json::json!(was["git_processes"].as_u64().unwrap() + 1),
            "git processes",
        ),
        (
            "peak_kib",
            serde_json::json!(was["peak_kib"].as_u64().unwrap() * 2 + 32 * 1024),
            "peak",
        ),
        (
            "latency_ratio",
            serde_json::json!(was["latency_ratio"].as_f64().unwrap() * 2.5),
            "latency ratio",
        ),
    ] {
        let mut run = recorded.clone();
        run.get_mut(&command).unwrap()[field] = value;
        let over = over_budget(&baseline, &run);
        assert!(
            over.len() == 1 && over[0].starts_with("work next: ") && over[0].contains(named),
            "{field}: {over:?}"
        );
    }
    let mut unknown = recorded;
    unknown.insert("work claim".to_string(), serde_json::json!({}));
    assert_eq!(
        over_budget(&baseline, &unknown),
        ["work claim: no baseline"]
    );
}
