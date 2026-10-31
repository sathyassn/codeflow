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
        let text = a.doctor();
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
