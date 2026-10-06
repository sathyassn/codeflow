//! Journeys (TSK-110 AC-2, SPC-013 R-103): the read commands `status`,
//! `work next`, `validate --docs` and `orient` make no network call, read
//! each target tip in process without one git process per record, and stay
//! bounded on a hostile inventory. `work claim`, which fetches by design, is
//! the fault control that proves the network detectors see a real call.
//! The binary under test is the one Cargo built.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const LINE: &str = "integration/EPC-001-offline";

fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

/// The environment every command in these journeys runs under. `trap` is a
/// directory put first on `PATH` that holds network tools which only record
/// that they ran.
fn with_env<'c>(command: &'c mut Command, trap: Option<&Path>) -> &'c mut Command {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(
        trap.map(Path::to_path_buf)
            .into_iter()
            .chain(exe.parent().map(Path::to_path_buf))
            .chain(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            )),
    )
    .expect("joinable PATH");
    command
        .env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        // Apply before init, including git children of codeflow: the scaffold
        // commit must not leave detached maintenance writing during the clone.
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "maintenance.auto")
        .env("GIT_CONFIG_VALUE_0", "false")
        .env("GIT_CONFIG_KEY_1", "gc.auto")
        .env("GIT_CONFIG_VALUE_1", "0")
        .env("GIT_AUTHOR_NAME", "Journey")
        .env("GIT_AUTHOR_EMAIL", "journey@example.test")
        .env("GIT_COMMITTER_NAME", "Journey")
        .env("GIT_COMMITTER_EMAIL", "journey@example.test")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_TRACE2_EVENT")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GITHUB_EVENT_NAME")
}

fn codeflow(root: &Path, args: &[&str]) -> Output {
    with_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")), None)
        .args(args)
        .current_dir(root)
        .output()
        .expect("codeflow runs")
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = with_env(&mut Command::new("git"), None)
        .args(args)
        .current_dir(root)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn ok(out: &Output, what: &str) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "{what} failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

fn edit(root: &Path, relative: &str, from: &str, to: &str) {
    let path = root.join(relative);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{relative} lacks {from:?}");
    std::fs::write(path, text.replacen(from, to, 1)).unwrap();
}

/// A fresh full-tier project with a bare origin, an epic line holding
/// `tasks` planned tasks, and that line fetched.
fn planned_project(tasks: usize) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    let bare = dir.path().join("origin.git");
    std::fs::create_dir(&root).unwrap();
    ok(
        &codeflow(&root, &["init", "--yes", "--full"]),
        "init --full",
    );
    let default = git(&root, &["branch", "--show-current"]);
    git(&root, &["branch", LINE, &default]);
    git(
        dir.path(),
        &[
            "clone",
            "--no-local",
            "-q",
            "--bare",
            root.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    git(&root, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&root, &["fetch", "-q", "origin"]);
    ok(&codeflow(&root, &["ids", "seed"]), "ids seed");
    git(&root, &["switch", "-q", "-c", "plan/offline", LINE]);
    ok(&codeflow(&root, &["epic", "new", "Offline"]), "epic new");
    let criterion = "- AC-1 When run, the system shall work.\n";
    edit(
        &root,
        "project-management/epics/EPC-001.md",
        "- AC-1\n",
        criterion,
    );
    for n in 1..=tasks {
        let title = format!("Task {n}");
        ok(
            &codeflow(
                &root,
                &["task", "new", "--epic", "EPC-001", "--into", LINE, &title],
            ),
            "task new",
        );
        edit(
            &root,
            &format!("project-management/tasks/TSK-{n:03}.md"),
            "- AC-1\n",
            &format!("{criterion}- AC-2 (journey) On a fresh project, the flow shall pass.\n"),
        );
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "chore: plan the line"]);
    git(&root, &["switch", "-q", LINE]);
    git(
        &root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "plan/offline",
            "-m",
            "chore: land the plan",
        ],
    );
    git(&root, &["push", "-q", "origin", LINE]);
    git(&root, &["fetch", "-q", "origin"]);
    (dir, root)
}

/// A stand-in `origin` on loopback: it counts every connection and answers
/// each with an HTTP error, so a network call fails fast and is seen.
struct Origin {
    url: String,
    connections: Arc<AtomicUsize>,
}

impl Origin {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener");
        let url = format!(
            "http://{}/origin.git",
            listener.local_addr().expect("listener address")
        );
        let connections = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&connections);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                seen.fetch_add(1, Ordering::SeqCst);
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(
                    b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        Self { url, connections }
    }

    fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

/// Network tools that only record that they ran.
fn trap_dir(dir: &Path) -> PathBuf {
    let trap = dir.join("trap");
    std::fs::create_dir_all(&trap).unwrap();
    #[cfg(unix)]
    for tool in ["gh", "ssh", "curl", "wget"] {
        use std::os::unix::fs::PermissionsExt;
        let path = trap.join(tool);
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\necho {tool} >> \"{}\"\nexit 1\n",
                trap.join("ran").display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    trap
}

/// What one command did, read from its trace and the stand-in origin.
struct Observed {
    output: Output,
    /// The argv of every git process the command started.
    git_processes: Vec<Vec<String>>,
}

/// Run `codeflow args` with every git process it starts traced to a fresh
/// event log, and the trap tools first on `PATH`.
fn observed(root: &Path, trap: &Path, args: &[&str]) -> Observed {
    let log = trap.join(format!("trace-{}.json", args.join("-")));
    let _ = std::fs::remove_file(&log);
    let output = with_env(
        &mut Command::new(env!("CARGO_BIN_EXE_codeflow")),
        Some(trap),
    )
    .env("GIT_TRACE2_EVENT", &log)
    .args(args)
    .current_dir(root)
    .output()
    .expect("codeflow runs");
    let git_processes = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| event["event"] == "start")
        .map(|event| {
            event["argv"]
                .as_array()
                .map(|argv| {
                    argv.iter()
                        .filter_map(|arg| arg.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    Observed {
        output,
        git_processes,
    }
}

/// Git subcommands that talk to a remote.
const NETWORK_SUBCOMMANDS: [&str; 8] = [
    "fetch",
    "ls-remote",
    "push",
    "pull",
    "fetch-pack",
    "send-pack",
    "remote-http",
    "remote-https",
];

/// The subcommand of a git argv, past global options and their values.
fn git_subcommand(argv: &[String]) -> Option<&str> {
    let mut rest = argv.iter().skip(1);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" => {
                rest.next();
            }
            option if option.starts_with('-') => {}
            sub => return Some(sub),
        }
    }
    None
}

fn network_processes(observed: &Observed) -> Vec<String> {
    observed
        .git_processes
        .iter()
        .filter(|argv| git_subcommand(argv).is_some_and(|sub| NETWORK_SUBCOMMANDS.contains(&sub)))
        .map(|argv| argv.join(" "))
        .collect()
}

#[test]
fn read_commands_make_no_network_call_and_a_claim_is_seen_fetching() {
    let (dir, root) = planned_project(2);
    let origin = Origin::start();
    git(&root, &["remote", "set-url", "origin", &origin.url]);
    let trap = trap_dir(dir.path());

    // Control: every read command answers from the refs as last fetched.
    for args in [
        &["status"][..],
        &["work", "next"][..],
        &["validate", "--docs"][..],
        &["orient"][..],
    ] {
        let seen = observed(&root, &trap, args);
        ok(&seen.output, &args.join(" "));
        assert_eq!(
            network_processes(&seen),
            Vec::<String>::new(),
            "{args:?} started a network git process"
        );
        assert_eq!(origin.connections(), 0, "{args:?} connected to origin");
        assert!(
            !trap.join("ran").exists(),
            "{args:?} ran a network tool: {}",
            std::fs::read_to_string(trap.join("ran")).unwrap_or_default()
        );
    }
    let next = observed(&root, &trap, &["work", "next"]);
    let next = String::from_utf8_lossy(&next.output.stdout);
    assert!(next.contains("ready    TSK-001"), "{next}");

    // Fault: `work claim` fetches before it claims. The same detectors see
    // it, so the control above is not blind.
    let claim = observed(&root, &trap, &["work", "claim", "TSK-001"]);
    assert!(
        !claim.output.status.success(),
        "a claim against a failing origin succeeded"
    );
    assert!(
        !network_processes(&claim).is_empty(),
        "the trace missed the claim's fetch: {:?}",
        claim.git_processes
    );
    assert!(
        origin.connections() > 0,
        "the stand-in origin saw no connection from the claim"
    );
}

/// Git processes a read command starts, by subcommand, ignoring the
/// command's own git executable path.
fn subcommands(observed: &Observed) -> Vec<String> {
    let mut subs: Vec<String> = observed
        .git_processes
        .iter()
        .filter_map(|argv| git_subcommand(argv).map(str::to_owned))
        .collect();
    subs.sort();
    subs
}

#[test]
fn read_commands_start_the_same_git_processes_for_three_records_or_thirty() {
    // Reads are in process and batched: the git processes a read command
    // starts do not grow with the number of records on the tip.
    let (small_dir, small) = planned_project(3);
    let (large_dir, large) = planned_project(30);
    let small_trap = trap_dir(small_dir.path());
    let large_trap = trap_dir(large_dir.path());
    for args in [&["status"][..], &["work", "next"][..], &["orient"][..]] {
        let few = observed(&small, &small_trap, args);
        let many = observed(&large, &large_trap, args);
        ok(&few.output, &args.join(" "));
        ok(&many.output, &args.join(" "));
        assert_eq!(
            subcommands(&few),
            subcommands(&many),
            "{args:?} started git processes that grow with the records"
        );
    }
}

/// `git mktree` over `listing`, allowing entries whose objects are absent.
fn mktree(root: &Path, listing: &str) -> String {
    let mut child = with_env(&mut Command::new("git"), None)
        .args(["mktree", "--missing"])
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("git mktree");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{listing}\n").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "mktree failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Move the line tip to a new commit with `tree` on top of it.
fn replace_tip(root: &Path, tree: &str) {
    let commit = git(
        root,
        &["commit-tree", tree, "-p", LINE, "-m", "chore: hostile tip"],
    );
    git(
        root,
        &["update-ref", &format!("refs/heads/{LINE}"), &commit],
    );
}

/// The line tip's root tree with its `name` entry replaced by `entry`
/// (`mode type oid`), or with `entry` added when it has none.
fn root_with(root: &Path, name: &str, entry: &str) -> String {
    let listing: Vec<String> = git(root, &["ls-tree", LINE])
        .lines()
        .filter(|line| !line.ends_with(&format!("\t{name}")))
        .map(str::to_owned)
        .chain(std::iter::once(format!("{entry}\t{name}")))
        .collect();
    mktree(root, &listing.join("\n"))
}

fn failure_text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_missing_tree_outside_the_records_does_not_stop_the_reads() {
    // A treeless partial clone lacks trees the reads never need. The reader
    // walks only the record paths, so an absent `vendor/` tree is never
    // touched.
    let (_dir, root) = planned_project(1);
    let absent = "1111111111111111111111111111111111111111";
    replace_tip(
        &root,
        &root_with(&root, "vendor", &format!("040000 tree {absent}")),
    );
    let next = ok(&codeflow(&root, &["work", "next"]), "work next");
    assert!(next.contains("ready    TSK-001"), "{next}");
    let status = ok(&codeflow(&root, &["status"]), "status");
    assert!(
        !status.contains("derived task states unavailable"),
        "{status}"
    );

    // Fault: a missing tree on the record path itself is an error that names
    // the object, never an empty backlog.
    let records = "3333333333333333333333333333333333333333";
    replace_tip(
        &root,
        &root_with(
            &root,
            "project-management",
            &format!("040000 tree {records}"),
        ),
    );
    let next = codeflow(&root, &["work", "next"]);
    let text = failure_text(&next);
    assert!(!next.status.success(), "{text}");
    assert!(text.contains(records), "{text}");
}

/// The line tip with `project-management/tasks/<file>` holding `text`.
fn tip_with_task_file(root: &Path, file: &str, text: &str) {
    let path = root.join(".git").join("probe-record.md");
    std::fs::write(&path, text).unwrap();
    let blob = git(root, &["hash-object", "-w", path.to_str().unwrap()]);
    let tasks = git(
        root,
        &["rev-parse", &format!("{LINE}:project-management/tasks")],
    );
    let listing: Vec<String> = git(root, &["ls-tree", &tasks])
        .lines()
        .filter(|line| !line.ends_with(&format!("\t{file}")))
        .map(str::to_owned)
        .chain(std::iter::once(format!("100644 blob {blob}\t{file}")))
        .collect();
    let tasks = mktree(root, &listing.join("\n"));
    let pm = git(root, &["rev-parse", &format!("{LINE}:project-management")]);
    let listing: Vec<String> = git(root, &["ls-tree", &pm])
        .lines()
        .filter(|line| !line.ends_with("\ttasks"))
        .map(str::to_owned)
        .chain(std::iter::once(format!("040000 tree {tasks}\ttasks")))
        .collect();
    let pm = mktree(root, &listing.join("\n"));
    replace_tip(
        root,
        &root_with(root, "project-management", &format!("040000 tree {pm}")),
    );
}

#[test]
fn an_oversized_record_on_a_tip_is_refused_by_name() {
    let (_dir, root) = planned_project(1);
    let record = git(
        &root,
        &[
            "show",
            &format!("{LINE}:project-management/tasks/TSK-001.md"),
        ],
    );
    let padded = |id: &str, bytes: usize| {
        record.replace("TSK-001", id).replacen(
            "## Description\n",
            &format!("## Description\n\n{}\n", "a".repeat(bytes)),
            1,
        )
    };
    // Control: a large record under the bound is read.
    tip_with_task_file(&root, "TSK-002.md", &padded("TSK-002", 1024 * 1024));
    let next = ok(&codeflow(&root, &["work", "next"]), "work next");
    assert!(next.contains("TSK-002"), "{next}");

    // Fault: a record past the 4 MiB bound is refused by path and bound.
    tip_with_task_file(&root, "TSK-003.md", &padded("TSK-003", 5 * 1024 * 1024));
    let next = codeflow(&root, &["work", "next"]);
    let text = failure_text(&next);
    assert!(!next.status.success(), "{text}");
    assert!(
        text.contains("project-management/tasks/TSK-003.md"),
        "{text}"
    );
    assert!(text.contains("4 MiB"), "{text}");
}
