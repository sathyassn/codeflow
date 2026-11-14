//! TSK-1077 AC-6 and AC-8: on a scratch adopter whose integration line
//! holds a path exception and a stricter level, pre-commit, pre-push, the
//! git-guard and `codeflow ci --policy-from <default>` all judge with the
//! default branch at one commit, and the bootstrap states refuse or are
//! labelled instead of guessing. `TSK1077_TEST_BINARY` runs the journey
//! with another binary, such as an installed release, for the
//! fail-before record.
use serde_json::json;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn binary() -> PathBuf {
    std::env::var_os("TSK1077_TEST_BINARY").map_or_else(
        || PathBuf::from(env!("CARGO_BIN_EXE_codeflow")),
        PathBuf::from,
    )
}

const ZERO: &str = "0000000000000000000000000000000000000000";
const MAIN_POLICY: &str = r#"{"git":{"force_push_unprotected":"allow"}}"#;
const LINE_POLICY: &str = r#"{"git":{"force_push_unprotected":"block","blocked_names":[{"pattern":"*.pem","except_under":["fixtures"],"reason":"line exception"}]}}"#;

struct Adopter {
    temp: tempfile::TempDir,
}

impl Adopter {
    fn new() -> Self {
        let adopter = Self {
            temp: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir(adopter.work()).unwrap();
        adopter.git(&["init", "-q", "-b", "main"]);
        adopter.git(&["config", "user.name", "Test"]);
        adopter.git(&["config", "user.email", "test@example.invalid"]);
        adopter
    }

    fn work(&self) -> PathBuf {
        self.temp.path().join("work")
    }

    fn remote_path(&self) -> PathBuf {
        self.temp.path().join("remote.git")
    }

    fn command(&self, program: &Path) -> Command {
        let mut c = Command::new(program);
        let home = self.temp.path();
        c.current_dir(self.work())
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("CODEFLOW_HOME", home.join("state"))
            .env("GIT_CONFIG_GLOBAL", home.join("global.config"))
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE");
        c
    }

    fn git(&self, args: &[&str]) -> String {
        let out = self.command(Path::new("git")).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().into()
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.work().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn commit(&self, files: &[(&str, &str)], message: &str) -> String {
        for (path, text) in files {
            self.write(path, text);
            self.git(&["add", path]);
        }
        self.git(&["commit", "-q", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn codeflow(&self, args: &[&str], stdin: &str) -> Output {
        let mut child = self
            .command(&binary())
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn guard(&self, command: &str) -> Output {
        let payload =
            json!({"tool_name":"Bash","cwd":self.work(),"tool_input":{"command":command}});
        self.codeflow(&["hook", "git-guard"], &payload.to_string())
    }

    fn doctor(&self) -> String {
        said(&self.codeflow(&["doctor", "--check", "policy-source"], ""))
    }

    /// `main` with a plain policy and `integration/line` with the
    /// exception, the stricter level and the task record that declares
    /// it, both on the remote with remote `HEAD` set to `main`; the task
    /// branch is checked out from the line.
    fn with_line() -> (Self, String) {
        let a = Self::new();
        a.git(&[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            a.remote_path().to_str().unwrap(),
        ]);
        a.git(&["remote", "add", "origin", a.remote_path().to_str().unwrap()]);
        let main = a.commit(
            &[
                (".codeflow/policy.json", MAIN_POLICY),
                ("README.md", "adopter\n"),
            ],
            "chore: adopt codeflow",
        );
        a.git(&["push", "-q", "origin", "main"]);
        a.git(&["checkout", "-q", "-b", "integration/line"]);
        a.commit(
            &[
                (".codeflow/policy.json", LINE_POLICY),
                (
                    "project-management/tasks/TSK-007.md",
                    "---\nid: TSK-007\nintegration_target: integration/line\n---\n",
                ),
            ],
            "chore: line exception",
        );
        a.git(&["push", "-q", "origin", "integration/line"]);
        a.git(&["fetch", "-q", "origin"]);
        a.git(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ]);
        a.git(&["checkout", "-q", "-b", "task/TSK-007-cert"]);
        (a, main)
    }
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// AC-6: `doctor --check policy-source` names the authority ref and the
/// commit it resolved to.
#[test]
fn doctor_reports_the_authority_ref_and_its_commit() {
    let (a, main) = Adopter::with_line();
    let text = a.doctor();
    assert!(
        text.contains(&format!(
            "policy source: refs/remotes/origin/main at {main}"
        )),
        "{text}"
    );
    println!("{text}");
}

/// AC-8: pre-commit, pre-push and `codeflow ci --policy-from <default>`
/// agree and refuse the line-exempted file, and the guard ignores the
/// line's stricter level.
#[test]
fn every_local_surface_judges_with_the_default_branch() {
    let (a, main) = Adopter::with_line();
    a.write("fixtures/cert.pem", "public fixture\n");
    a.git(&["add", "fixtures/cert.pem"]);

    let pre_commit = a.codeflow(&["git-hook", "pre-commit"], "");
    let text = said(&pre_commit);
    println!("pre-commit:\n{text}");
    assert_ne!(pre_commit.status.code(), Some(0), "{text}");
    assert!(text.contains("fixtures/cert.pem"), "{text}");

    a.git(&["commit", "-q", "-m", "test: add a fixture"]);
    let head = a.git(&["rev-parse", "HEAD"]);
    let remote = a.remote_path();
    let pre_push = a.codeflow(
        &["git-hook", "pre-push", "origin", remote.to_str().unwrap()],
        &format!("refs/heads/task/TSK-007-cert {head} refs/heads/task/TSK-007-cert {ZERO}\n"),
    );
    let text = said(&pre_push);
    println!("pre-push:\n{text}");
    assert_ne!(pre_push.status.code(), Some(0), "{text}");
    assert!(text.contains("fixtures/cert.pem"), "{text}");

    let ci = a.codeflow(
        &[
            "ci",
            "--policy-from",
            &main,
            "--base",
            "origin/integration/line",
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-007-cert",
        ],
        "",
    );
    let text = said(&ci);
    println!("ci:\n{text}");
    assert_ne!(ci.status.code(), Some(0), "{text}");
    assert!(text.contains("fixtures/cert.pem"), "{text}");

    // The line's stricter force-push level is not the authority's.
    let guard = a.guard("git push --force-with-lease origin task/TSK-007-cert");
    let text = said(&guard);
    println!("git-guard:\n{text}");
    assert_eq!(guard.status.code(), Some(0), "{text}");
}

/// AC-8, the bootstrap states: unset remote `HEAD`, an empty namespace for
/// a remote with refs, a remote with no refs and no remote.
#[test]
fn the_bootstrap_states_refuse_or_are_labelled() {
    let (a, _) = Adopter::with_line();

    a.git(&["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"]);
    let text = a.doctor();
    println!("remote HEAD unset:\n{text}");
    assert!(text.contains("git remote set-head origin -a"), "{text}");
    let guard = a.guard("git commit -m 'chore: x'");
    assert_eq!(guard.status.code(), Some(2), "{}", said(&guard));

    for branch in ["main", "integration/line"] {
        a.git(&["update-ref", "-d", &format!("refs/remotes/origin/{branch}")]);
    }
    let text = a.doctor();
    println!("empty namespace, remote with refs:\n{text}");
    assert!(text.contains("run git fetch origin"), "{text}");

    let empty = a.temp.path().join("empty.git");
    a.git(&["init", "-q", "--bare", empty.to_str().unwrap()]);
    a.git(&["remote", "set-url", "origin", empty.to_str().unwrap()]);
    let text = a.doctor();
    println!("remote advertising no refs:\n{text}");
    assert!(
        text.contains("initialization: origin advertises no refs"),
        "{text}"
    );

    a.git(&["remote", "remove", "origin"]);
    let text = a.doctor();
    println!("no remote:\n{text}");
    assert!(text.contains("local only: no remote"), "{text}");
}

/// Review round 02 F4: an inherited `GIT_NAMESPACE` would make a local
/// remote that holds branches advertise none, and the empty tracking
/// namespace would read as initialization, judged by `HEAD`. The question
/// to the remote ignores the variable, so doctor and the guard still name
/// the fetch route.
#[test]
fn an_inherited_git_namespace_cannot_fabricate_initialization() {
    let (a, _) = Adopter::with_line();
    for branch in ["HEAD", "main", "integration/line"] {
        a.git(&[
            "update-ref",
            "--no-deref",
            "-d",
            &format!("refs/remotes/origin/{branch}"),
        ]);
    }
    let doctor = a
        .command(&binary())
        .args(["doctor", "--check", "policy-source"])
        .env("GIT_NAMESPACE", "tsk1077-review-empty")
        .output()
        .unwrap();
    let text = said(&doctor);
    println!("doctor with GIT_NAMESPACE:\n{text}");
    assert!(text.contains("run git fetch origin"), "{text}");
    assert!(!text.contains("initialization"), "{text}");
    let payload = json!({"tool_name":"Bash","cwd":a.work(),"tool_input":{"command":"git commit -m 'chore: x'"}});
    let mut child = a
        .command(&binary())
        .args(["hook", "git-guard"])
        .env("GIT_NAMESPACE", "tsk1077-review-empty")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let guard = child.wait_with_output().unwrap();
    let text = said(&guard);
    assert_eq!(guard.status.code(), Some(2), "{text}");
    assert!(text.contains("run git fetch origin"), "{text}");
}

/// A loopback HTTP server that answers every request with a Basic
/// authentication challenge and records each request's headers.
fn challenging_server() -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{BufRead as _, BufReader};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = std::sync::Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let log = std::sync::Arc::clone(&log);
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut writer = stream;
                loop {
                    let mut request = String::new();
                    loop {
                        let mut line = String::new();
                        match reader.read_line(&mut line) {
                            Ok(0) | Err(_) => return,
                            Ok(_) => {}
                        }
                        if line == "\r\n" {
                            break;
                        }
                        request.push_str(&line);
                    }
                    log.lock().unwrap().push(request);
                    let answer = "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"x\"\r\nContent-Length: 0\r\n\r\n";
                    if writer.write_all(answer.as_bytes()).is_err() {
                        return;
                    }
                }
            });
        }
    });
    (port, seen)
}

/// `doctor` once the remote has answered its question. The question runs
/// inside the guard's fixed lookup wait (`GUARD_REMOTE_QUERY_SECS`, a
/// product limit no policy changes), and on a loaded runner starting `git`
/// and its HTTP helper can take longer than that wait, although the
/// loopback server answers at once (TSK-1253, hosted Windows). Such a run
/// says nothing about which headers were sent, so it is asked again; the
/// requests of every attempt stay in the server's log and are all checked.
/// Each attempt is one child run to its exit; the attempt count only stops
/// a remote that never answers.
fn doctor_until_answered(a: &Adopter) -> String {
    use codeflow_core::hooks::guard_deadline::GUARD_REMOTE_QUERY_SECS;
    const ATTEMPTS: usize = 6;
    let expired = format!("no answer within {GUARD_REMOTE_QUERY_SECS}s per attempt");
    let mut text = String::new();
    for attempt in 1..=ATTEMPTS {
        text = a.doctor();
        if !text.contains(&expired) {
            return text;
        }
        println!("doctor attempt {attempt}: the guard's lookup wait expired:\n{text}");
    }
    panic!("the remote answered none of {ATTEMPTS} questions: {text}");
}

/// Review round 03 F5: git's HTTP transport reads a `.netrc` in the
/// inherited home whatever its configuration says, so a remote URL that
/// names a host the attacker controls would receive those credentials
/// when the empty tracking namespace makes the guards ask it. The question
/// runs with an empty home, so neither a matching `machine` entry nor a
/// `default` entry is sent, and the result is the fetch route.
#[test]
fn asking_the_remote_sends_no_stored_http_credentials() {
    for entry in [
        "machine 127.0.0.1 login synthetic password not-a-secret\n",
        "default login synthetic password not-a-secret\n",
    ] {
        let (a, _) = Adopter::with_line();
        let (port, seen) = challenging_server();
        a.git(&[
            "remote",
            "set-url",
            "origin",
            &format!("http://127.0.0.1:{port}/remote.git"),
        ]);
        for branch in ["HEAD", "main", "integration/line"] {
            a.git(&[
                "update-ref",
                "--no-deref",
                "-d",
                &format!("refs/remotes/origin/{branch}"),
            ]);
        }
        std::fs::write(a.temp.path().join(".netrc"), entry).unwrap();
        std::fs::write(a.temp.path().join("_netrc"), entry).unwrap();
        let text = doctor_until_answered(&a);
        println!("doctor with a .netrc {entry:?}:\n{text}");
        let requests = seen.lock().unwrap().clone();
        assert!(!requests.is_empty(), "the remote was not asked: {text}");
        for request in &requests {
            assert!(
                !request.to_ascii_lowercase().contains("authorization:"),
                "credentials were sent: {request}"
            );
        }
        assert!(text.contains("run git fetch origin"), "{text}");
        assert!(!text.contains("initialization"), "{text}");
    }
}

/// TSK-1301: a folder holding a `git` that answers `ls-remote` only after
/// `delay` (or never, for `None`) and passes every other command to the
/// real git, and the `PATH` that puts it first. Each `ls-remote` it is
/// asked adds a line to [`ls_remote_calls`] first, so a test can tell "asked
/// nothing" from "answered fast" without a clock (TSK-1253).
#[cfg(unix)]
fn slow_ls_remote(a: &Adopter, delay: Option<&str>) -> std::ffi::OsString {
    use std::os::unix::fs::PermissionsExt as _;
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|folder| folder.join("git"))
        .find(|candidate| candidate.is_file())
        .expect("git on PATH");
    let bin = a.temp.path().join("slow-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let wait = delay.map_or_else(
        || "sleep 60; exit 128".to_string(),
        |delay| format!("sleep {delay}"),
    );
    let script = format!(
        "#!/bin/sh\nfor word in \"$@\"; do\n  if [ \"$word\" = ls-remote ]; then\n    echo ls-remote >> '{}'\n    {wait}\n    break\n  fi\ndone\nexec '{}' \"$@\"\n",
        calls_log(a).display(),
        real.display()
    );
    let git = bin.join("git");
    std::fs::write(&git, script).unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::join_paths(
        std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap()
}

#[cfg(unix)]
fn calls_log(a: &Adopter) -> std::path::PathBuf {
    a.temp.path().join("ls-remote-calls")
}

/// How many `ls-remote` questions the [`slow_ls_remote`] git has been asked.
#[cfg(unix)]
fn ls_remote_calls(a: &Adopter) -> usize {
    std::fs::read_to_string(calls_log(a)).map_or(0, |log| log.lines().count())
}

#[cfg(unix)]
impl Adopter {
    fn codeflow_on(&self, path: &std::ffi::OsStr, args: &[&str], stdin: &str) -> Output {
        let mut child = self
            .command(&binary())
            .env("PATH", path)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    /// Land `policy` on the remote's `main` and fetch it, so it is the
    /// local policy authority, then return to the task branch.
    fn land_main_policy(&self, policy: &str) {
        self.git(&["checkout", "-q", "main"]);
        self.commit(&[(".codeflow/policy.json", policy)], "chore: tune waits");
        self.git(&["push", "-q", "origin", "main"]);
        self.git(&["fetch", "-q", "origin"]);
        self.git(&["checkout", "-q", "task/TSK-007-cert"]);
    }
}

/// TSK-1301 AC-2 and AC-5, the pre-push path: a destination whose
/// default-branch lookup answers after 1.5 s stands in for GitHub's
/// 11.3 s, scaled down. With the wait the default branch sets below that
/// answer (1 s, one attempt, as the old fixed 10 s was below 11.3 s) the
/// push refuses, naming the limits, the time measured and the key; with a
/// wait above it (3 s) the same push is judged and its lookup passes.
#[cfg(unix)]
#[test]
fn pre_push_waits_as_long_as_the_default_branch_policy_says() {
    let (a, _) = Adopter::with_line();
    let path = slow_ls_remote(&a, Some("1.5"));
    let head = a.git(&["rev-parse", "HEAD"]);
    let remote = a.remote_path();
    let push = |a: &Adopter| {
        a.codeflow_on(
            &path,
            &["git-hook", "pre-push", "origin", remote.to_str().unwrap()],
            &format!("refs/heads/task/TSK-007-cert {head} refs/heads/task/TSK-007-cert {ZERO}\n"),
        )
    };

    a.land_main_policy(
        r#"{"git":{"force_push_unprotected":"allow","remote_query_timeout_seconds":1,"remote_query_attempts":1}}"#,
    );
    let refused = push(&a);
    let text = said(&refused);
    println!("pre-push, 1 s wait:\n{text}");
    assert!(
        matches!(refused.status.code(), Some(code) if code != 0),
        "{:?}: {text}",
        refused.status
    );
    assert!(text.contains("ci.destination_unavailable"), "{text}");
    assert!(
        text.contains("no answer within 1s per attempt (1 attempt, "),
        "{text}"
    );
    assert!(
        text.contains(
            "s measured); git.remote_query_timeout_seconds and git.remote_query_attempts"
        ),
        "{text}"
    );
    assert!(
        text.contains("raise `git.remote_query_timeout_seconds`"),
        "{text}"
    );

    a.land_main_policy(
        r#"{"git":{"force_push_unprotected":"allow","remote_query_timeout_seconds":3,"remote_query_attempts":1}}"#,
    );
    let text = said(&push(&a));
    println!("pre-push, 3 s wait:\n{text}");
    assert!(!text.contains("ci.destination_unavailable"), "{text}");
    assert!(!text.contains("no answer within"), "{text}");
}

/// TSK-1301 AC-1 and AC-7, a direct `codeflow ci` and whose waits apply. The
/// lookup answers after 1.5 s. The default branch waits 3 s and the task
/// branch's head says 1 s. The hook reads the default branch, so the head
/// cannot change its wait. `codeflow ci` reads the authority it is given, as
/// it does every rule: naming the default branch passes the lookup, and
/// naming the head uses the head's wait and refuses.
#[cfg(unix)]
#[test]
fn a_head_sets_no_wait_for_the_hook_and_ci_reads_the_authority_it_names() {
    let (a, _) = Adopter::with_line();
    let path = slow_ls_remote(&a, Some("1.5"));
    let remote = a.remote_path();
    let url = remote.to_str().unwrap();
    a.land_main_policy(
        r#"{"git":{"force_push_unprotected":"allow","remote_query_timeout_seconds":3,"remote_query_attempts":1}}"#,
    );
    a.commit(
        &[(
            ".codeflow/policy.json",
            r#"{"git":{"force_push_unprotected":"allow","remote_query_timeout_seconds":1,"remote_query_attempts":1}}"#,
        )],
        "chore: shorten the head's wait",
    );
    let head = a.git(&["rev-parse", "HEAD"]);
    let main = a.git(&["rev-parse", "origin/main"]);

    let hook = a.codeflow_on(
        &path,
        &["git-hook", "pre-push", "origin", url],
        &format!("refs/heads/task/TSK-007-cert {head} refs/heads/task/TSK-007-cert {ZERO}\n"),
    );
    let text = said(&hook);
    println!("pre-push, the head says 1 s:\n{text}");
    assert!(!text.contains("no answer within"), "{text}");

    let ci = |authority: &str| {
        a.codeflow_on(
            &path,
            &[
                "ci",
                "--policy-from",
                authority,
                "--base",
                "origin/integration/line",
                "--head",
                "HEAD",
                "--branch",
                "task/TSK-007-cert",
                "--destination",
                url,
            ],
            "",
        )
    };
    let text = said(&ci(&main));
    println!("ci, the default branch's authority:\n{text}");
    assert!(!text.contains("ci.destination_unavailable"), "{text}");
    assert!(!text.contains("no answer within"), "{text}");

    let refused = ci(&head);
    let text = said(&refused);
    println!("ci, the head as its authority:\n{text}");
    assert!(
        matches!(refused.status.code(), Some(code) if code != 0),
        "{:?}: {text}",
        refused.status
    );
    assert!(text.contains("ci.destination_unavailable"), "{text}");
    assert!(
        text.contains("no answer within 1s per attempt (1 attempt, "),
        "{text}"
    );
}

/// TSK-1301 AC-8, the claim behind guard option (b): no configured wait
/// reaches a guard, whichever way the values point. A 1 s wait would refuse
/// early if a guard honoured it; a 60 s wait with 5 attempts would run into
/// the guard's deadline.
///
/// Where a guard asks (no tracking refs), the only copy of the keys is the
/// task branch's own, and the guard still spends exactly its wait, one
/// attempt, and refuses by name. Where the keys are the authority's (on the
/// default branch, fetched), the guard asks nothing: a remote that never
/// answers costs it no wait at all.
#[cfg(unix)]
#[test]
fn no_configured_wait_reaches_a_guard() {
    use codeflow_core::hooks::guard_deadline::{GUARD_DEADLINE_SECS, GUARD_REMOTE_QUERY_SECS};
    use std::time::{Duration, Instant};
    for (seconds, attempts) in [(1, 1), (60, 5)] {
        let policy = format!(
            r#"{{"git":{{"force_push_unprotected":"allow","remote_query_timeout_seconds":{seconds},"remote_query_attempts":{attempts}}}}}"#
        );

        let (a, _) = Adopter::with_line();
        let payload = json!({"tool_name":"Bash","cwd":a.work(),"tool_input":{"command":"git push --force-with-lease origin task/TSK-007-cert"}})
            .to_string();
        let silent = slow_ls_remote(&a, None);
        a.commit(&[(".codeflow/policy.json", &policy)], "chore: tune waits");
        for branch in ["HEAD", "main", "integration/line"] {
            a.git(&[
                "update-ref",
                "--no-deref",
                "-d",
                &format!("refs/remotes/origin/{branch}"),
            ]);
        }
        let started = Instant::now();
        let guard = a.codeflow_on(&silent, &["hook", "git-guard"], &payload);
        let took = started.elapsed();
        let text = said(&guard);
        println!("no authority, keys {seconds} s and {attempts} ({took:?}):\n{text}");
        assert_eq!(guard.status.code(), Some(2), "{text}");
        assert!(
            took >= Duration::from_secs(GUARD_REMOTE_QUERY_SECS),
            "{took:?}"
        );
        assert!(took < Duration::from_secs(GUARD_DEADLINE_SECS), "{took:?}");
        assert!(!text.contains("reached no decision"), "{text}");
        assert!(
            text.contains(&format!(
                "no answer within {GUARD_REMOTE_QUERY_SECS}s per attempt (1 attempt, "
            )),
            "{text}"
        );
        assert_eq!(ls_remote_calls(&a), 1, "one attempt: {text}");

        let (a, _) = Adopter::with_line();
        let payload = json!({"tool_name":"Bash","cwd":a.work(),"tool_input":{"command":"git push --force-with-lease origin task/TSK-007-cert"}})
            .to_string();
        let silent = slow_ls_remote(&a, None);
        a.land_main_policy(&policy);
        let asked = ls_remote_calls(&a);
        let started = Instant::now();
        let guard = a.codeflow_on(&silent, &["hook", "git-guard"], &payload);
        let took = started.elapsed();
        let text = said(&guard);
        println!("authority, keys {seconds} s and {attempts} ({took:?}):\n{text}");
        assert_eq!(guard.status.code(), Some(0), "{text}");
        // It asked the remote nothing, read from the fake git's log rather
        // than from a clock a loaded runner can stretch (TSK-1253).
        assert_eq!(ls_remote_calls(&a), asked, "{took:?}: {text}");
    }
}

/// TSK-1301 AC-8: while the policy authority is unresolved, both guards let
/// through one plain fetch of a configured remote and nothing else. A line
/// that does anything more goes on to the normal refusal, because the
/// allowance runs before the integrity checks. The guards judge these lines;
/// nothing here runs them.
#[cfg(unix)]
#[test]
fn only_a_plain_fetch_gets_the_recovery_allowance() {
    let (a, _) = Adopter::with_line();
    for branch in ["HEAD", "main", "integration/line"] {
        a.git(&[
            "update-ref",
            "--no-deref",
            "-d",
            &format!("refs/remotes/origin/{branch}"),
        ]);
    }
    let path = std::env::var_os("PATH").unwrap();
    let exit = |guard: &str, line: &str| {
        let payload =
            json!({"tool_name":"Bash","cwd":a.work(),"tool_input":{"command":line}}).to_string();
        a.codeflow_on(&path, &["hook", guard], &payload)
            .status
            .code()
    };
    for guard in ["git-guard", "exec-guard"] {
        for line in ["git fetch", "git fetch origin", "  git  fetch  origin  "] {
            assert_eq!(exit(guard, line), Some(0), "{guard}: {line}");
        }
    }
    let mut let_through = Vec::new();
    for guard in ["git-guard", "exec-guard"] {
        for line in [
            "git fetch origin > fetched.txt",
            "git fetch origin >> fetched.txt",
            "git fetch origin 2> fetched.txt",
            "git fetch origin &> fetched.txt",
            "git fetch origin < fetched.txt",
            "git fetch origin <<< fetched",
            "> fetched.txt git fetch origin",
            "git fetch origin | cat",
            "git fetch origin; true",
            "git fetch origin && true",
            "git fetch origin || true",
            "git fetch origin &",
            "git fetch origin\ntrue",
            "NAME=value git fetch origin",
            "env NAME=value git fetch origin",
            "command git fetch origin",
            "exec git fetch origin",
            "time git fetch origin",
            "! git fetch origin",
            "(git fetch origin)",
            "{ git fetch origin; }",
            "git fetch $(echo origin)",
            "git fetch `echo origin`",
            "git fetch $NAME",
            "git fetch \"origin\"",
            "git fetch 'origin'",
            "git fetch ori\\gin",
            "git fetch origin # note",
            "git\u{a0}fetch origin",
            "git fetch\u{2003}origin",
            "git fetch origin\u{85}",
            "git\u{3000}fetch\u{3000}origin",
            "git fetch --all",
            "git fetch origin main",
            "git -C . fetch origin",
            "git fetch elsewhere",
            "bash -c 'git fetch origin'",
            "eval git fetch origin",
        ] {
            if exit(guard, line) == Some(0) {
                let_through.push(format!("{guard}: {line}"));
            }
        }
    }
    // The read-only allowance beside it runs at the same point and is a
    // closed form too.
    for guard in ["git-guard", "exec-guard"] {
        assert_eq!(exit(guard, "git status"), Some(0), "{guard}");
        for line in [
            "git status > fetched.txt",
            "git log | cat",
            "git status &",
            "NAME=value git status",
            "command git status",
            "(git status)",
            "git status # note",
        ] {
            if exit(guard, line) == Some(0) {
                let_through.push(format!("{guard}: {line}"));
            }
        }
    }
    assert!(let_through.is_empty(), "{let_through:#?}");
}

/// TSK-1301 AC-8, the guard path: with the remote's tracking refs missing
/// the guard asks the remote whether it holds any. A remote that never
/// answers gets the guard's own refusal after its 10 s lookup wait,
/// naming the wait and the fetch that removes the need to ask, well
/// before the guard's 20 s deadline and the harness's 30 s hook timeout.
/// After that fetch the same guarded command passes while the remote still
/// never answers, because the guard reads local refs.
#[cfg(unix)]
#[test]
fn the_guard_refuses_a_silent_remote_after_its_wait_and_a_fetch_clears_it() {
    use codeflow_core::hooks::guard_deadline::{GUARD_DEADLINE_SECS, GUARD_REMOTE_QUERY_SECS};
    use std::time::{Duration, Instant};
    let (a, _) = Adopter::with_line();
    for branch in ["HEAD", "main", "integration/line"] {
        a.git(&[
            "update-ref",
            "--no-deref",
            "-d",
            &format!("refs/remotes/origin/{branch}"),
        ]);
    }
    let silent = slow_ls_remote(&a, None);
    let payload = json!({"tool_name":"Bash","cwd":a.work(),"tool_input":{"command":"git push --force-with-lease origin task/TSK-007-cert"}})
        .to_string();

    let started = Instant::now();
    let guard = a.codeflow_on(&silent, &["hook", "git-guard"], &payload);
    let took = started.elapsed();
    let text = said(&guard);
    println!("guard, silent remote, no tracking refs ({took:?}):\n{text}");
    assert_eq!(guard.status.code(), Some(2), "{text}");
    assert!(
        took >= Duration::from_secs(GUARD_REMOTE_QUERY_SECS),
        "{took:?}"
    );
    assert!(took < Duration::from_secs(GUARD_DEADLINE_SECS), "{took:?}");
    assert!(!text.contains("reached no decision"), "{text}");
    assert!(
        text.contains(&format!(
            "a session guard waits at most {GUARD_REMOTE_QUERY_SECS}s for each such lookup, whatever git.remote_query_timeout_seconds says, and its {GUARD_DEADLINE_SECS}s deadline bounds the whole command"
        )),
        "{text}"
    );
    assert!(text.contains("run git fetch origin"), "{text}");
    assert_eq!(ls_remote_calls(&a), 1, "one attempt: {text}");

    // The route the refusal names has to be open to the session it refused.
    let fetch_payload =
        json!({"tool_name":"Bash","cwd":a.work(),"tool_input":{"command":"git fetch origin"}})
            .to_string();
    let started = Instant::now();
    let fetch_guard = a.codeflow_on(&silent, &["hook", "git-guard"], &fetch_payload);
    let took = started.elapsed();
    let text = said(&fetch_guard);
    println!("guard on the fetch itself ({took:?}):\n{text}");
    assert_eq!(fetch_guard.status.code(), Some(0), "{text}");

    a.git(&["fetch", "-q", "origin"]);
    // Git 2.48 and later record the remote HEAD on fetch; an older git
    // needs it named once, without the network.
    if a.git(&["for-each-ref", "refs/remotes/origin/HEAD"])
        .is_empty()
    {
        a.git(&["remote", "set-head", "origin", "main"]);
    }
    let asked = ls_remote_calls(&a);
    let started = Instant::now();
    let guard = a.codeflow_on(&silent, &["hook", "git-guard"], &payload);
    let took = started.elapsed();
    let text = said(&guard);
    println!("guard after the fetch ({took:?}):\n{text}");
    assert_eq!(guard.status.code(), Some(0), "{text}");
    // It read local refs and asked the remote nothing (TSK-1253: an event,
    // where a 5 s bound stood before).
    assert_eq!(ls_remote_calls(&a), asked, "{took:?}: {text}");
}

/// `url` as a refusal shows it: each backslash escaped, as every message
/// shows text a configuration or a remote supplies (TSK-388).
fn shown(url: &str) -> String {
    codeflow_core::git::display_text(url)
}

/// TSK-1184 AC-1, AC-3 and AC-5: a fresh adopter whose remote is empty and
/// configured as git uses it is labelled initialization, and its first push
/// passes the guard and the pre-push hook. When an `insteadOf` rewrite makes
/// git use a populated repository instead, doctor, the git-guard and
/// pre-commit refuse with both identities named and never read `HEAD`.
#[test]
fn a_rewritten_empty_remote_refuses_and_a_matching_fresh_remote_pushes() {
    let a = Adopter::new();
    // A directory name holding a backslash gives every host what Windows
    // paths always carry: messages show each backslash escaped (TSK-1400).
    let remotes = a.temp.path().join(if cfg!(windows) {
        "remotes"
    } else {
        r"back\slash"
    });
    std::fs::create_dir(&remotes).unwrap();
    let empty = remotes.join("empty.git");
    let populated = remotes.join("populated.git");
    let empty = empty.to_str().unwrap();
    let populated = populated.to_str().unwrap();
    for bare in [empty, populated] {
        a.git(&["init", "-q", "--bare", "-b", "main", bare]);
    }
    a.git(&["remote", "add", "origin", empty]);
    a.commit(
        &[(".codeflow/policy.json", MAIN_POLICY)],
        "chore: adopt codeflow",
    );
    a.git(&["push", "-q", populated, "main"]);
    a.git(&["checkout", "-q", "-b", "feat/first"]);
    let head = a.commit(&[("README.md", "first\n")], "docs: first change");

    let text = a.doctor();
    println!("fresh remote:\n{text}");
    assert!(
        text.contains("initialization: origin advertises no refs"),
        "{text}"
    );
    let guard = a.guard("git push origin feat/first");
    assert_eq!(guard.status.code(), Some(0), "{}", said(&guard));
    let push = a.codeflow(
        &["git-hook", "pre-push", "origin", empty],
        &format!("refs/heads/feat/first {head} refs/heads/feat/first {ZERO}\n"),
    );
    let text = said(&push);
    println!("first push:\n{text}");
    assert_eq!(push.status.code(), Some(0), "{text}");

    a.git(&["config", &format!("url.{populated}.insteadOf"), empty]);
    let text = a.doctor();
    println!("rewritten remote:\n{text}");
    let (shown_empty, shown_populated) = (shown(empty), shown(populated));
    assert!(
        text.contains(&shown_empty) && text.contains(&shown_populated),
        "{text}"
    );
    assert!(text.contains("never read from HEAD"), "{text}");
    assert!(!text.contains("initialization"), "{text}");
    let guard = a.guard("git commit -m 'docs: x'");
    let text = said(&guard);
    println!("git-guard, rewritten remote:\n{text}");
    assert_eq!(guard.status.code(), Some(2), "{text}");
    assert!(text.contains(&shown_populated), "{text}");
    a.write("README.md", "second\n");
    a.git(&["add", "README.md"]);
    let commit = a.codeflow(&["git-hook", "pre-commit"], "");
    let text = said(&commit);
    println!("pre-commit, rewritten remote:\n{text}");
    assert_ne!(commit.status.code(), Some(0), "{text}");
    assert!(text.contains(&shown_populated), "{text}");
}
