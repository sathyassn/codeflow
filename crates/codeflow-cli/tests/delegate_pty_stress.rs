#![cfg(unix)]

//! PTY-level stress tests for the host side of the schema-v2 delegate lifecycle.
//!
//! The fake TUI is this integration-test executable running one hidden helper
//! test inside a native pseudo-terminal. It emits the same Claude hook events
//! as the real adapter, but the assertions observe only durable lifecycle
//! records and stable CLI exits. Terminal output is used solely for the dialog
//! case and to distinguish a spinner from a silent process.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use serde_json::{json, Value};
use tempfile::TempDir;

const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";
const DIALOG_MARKER: &str = "FAKE_TUI_DIALOG: continue?";

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_codeflow")
}

fn run_codeflow(args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("run codeflow {args:?}: {error}"))
}

fn assert_exit(output: &Output, expected: i32, operation: &str) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "{operation}: stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct LifecycleFixture {
    root: TempDir,
    state: PathBuf,
    run_id: String,
}

impl LifecycleFixture {
    fn new(label: &str) -> Self {
        let root = tempfile::tempdir().expect("fixture tempdir");
        let state = root.path().join("state");
        let run_id = format!("pty-{label}");
        let output = run_codeflow(&[
            "delegate",
            "init",
            "--run-id",
            &run_id,
            "--state-dir",
            state.to_str().expect("utf8 state"),
        ]);
        assert_exit(&output, 0, "delegate init");
        Self {
            root,
            state,
            run_id,
        }
    }

    fn arm(&self, turn_id: &str, prompt: &[u8]) {
        let prompt_path = self.root.path().join(format!("{turn_id}.prompt"));
        std::fs::write(&prompt_path, prompt).expect("write prompt fixture");
        let output = run_codeflow(&[
            "delegate",
            "arm",
            "--run-id",
            &self.run_id,
            "--state-dir",
            self.state.to_str().expect("utf8 state"),
            "--turn-id",
            turn_id,
            "--prompt-file",
            prompt_path.to_str().expect("utf8 prompt path"),
        ]);
        assert_exit(&output, 0, "delegate arm");
    }

    fn wait(&self, until: &str, turn_id: Option<&str>, timeout_seconds: u64) -> Output {
        let mut command = Command::new(binary());
        command.args([
            "delegate",
            "wait",
            "--run-id",
            &self.run_id,
            "--state-dir",
            self.state.to_str().expect("utf8 state"),
            "--until",
            until,
            "--timeout-seconds",
            &timeout_seconds.to_string(),
        ]);
        if let Some(turn_id) = turn_id {
            command.args(["--turn-id", turn_id]);
        }
        command.output().expect("delegate wait")
    }
}

struct FakeTui {
    child: Option<Box<dyn Child + Send + Sync>>,
    writer: Option<Box<dyn Write + Send>>,
    output: Arc<Mutex<Vec<u8>>>,
    reader: Option<thread::JoinHandle<()>>,
}

impl FakeTui {
    fn spawn(fixture: &LifecycleFixture, scenario: &str, turns: usize) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 40,
                cols: 160,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("open native PTY");

        let mut command = CommandBuilder::new(std::env::current_exe().expect("test executable"));
        command.args([
            "--exact",
            "fake_tui_process",
            "--nocapture",
            "--test-threads=1",
        ]);
        command.env("CF_FAKE_TUI", "1");
        command.env("CF_FAKE_SCENARIO", scenario);
        command.env("CF_FAKE_TURNS", turns.to_string());
        command.env("CF_FAKE_RUN_ID", &fixture.run_id);
        command.env("CF_FAKE_STATE", &fixture.state);

        let child = pair
            .slave
            .spawn_command(command)
            .expect("spawn fake TUI in PTY");
        let mut source = pair.master.try_clone_reader().expect("clone PTY reader");
        let writer = pair.master.take_writer().expect("take PTY writer");
        drop(pair);

        let output = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&output);
        let reader = thread::spawn(move || {
            let mut chunk = [0_u8; 8192];
            loop {
                match source.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => sink
                        .lock()
                        .expect("output lock")
                        .extend_from_slice(&chunk[..count]),
                }
            }
        });

        Self {
            child: Some(child),
            writer: Some(writer),
            output,
            reader: Some(reader),
        }
    }

    fn send_frame(&mut self, bytes: &[u8], enter_count: usize) {
        let writer = self.writer.as_mut().expect("PTY writer");
        writer.write_all(PASTE_START).expect("write paste start");
        writer.write_all(bytes).expect("write prompt");
        writer.write_all(PASTE_END).expect("write paste end");
        writer.flush().expect("flush literal paste");
        thread::sleep(Duration::from_millis(25));
        for _ in 0..enter_count {
            writer.write_all(b"\r").expect("write Enter");
        }
        writer.flush().expect("flush PTY");
    }

    fn wait_for_output(&self, marker: &str, timeout: Duration) {
        let started = Instant::now();
        loop {
            let found = {
                let output = self.output.lock().expect("output lock");
                String::from_utf8_lossy(&output).contains(marker)
            };
            if found {
                return;
            }
            assert!(
                started.elapsed() < timeout,
                "PTY output did not contain {marker:?}: {}",
                self.output_text()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn output_text(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().expect("output lock")).into_owned()
    }

    fn kill(&mut self) {
        if let Some(child) = self.child.as_mut() {
            child.kill().expect("kill fake TUI");
        }
        self.finish(false);
    }

    fn finish(&mut self, expect_success: bool) {
        let started = Instant::now();
        let status = loop {
            let child = self.child.as_mut().expect("fake TUI child");
            if let Some(status) = child.try_wait().expect("poll fake TUI") {
                break status;
            }
            if started.elapsed() >= Duration::from_secs(5) {
                child.kill().expect("kill stalled fake TUI");
                break child.wait().expect("wait killed fake TUI");
            }
            thread::sleep(Duration::from_millis(10));
        };
        self.child.take();
        self.writer.take();
        if let Some(reader) = self.reader.take() {
            reader.join().expect("join PTY reader");
        }
        if expect_success {
            assert!(
                status.success(),
                "fake TUI failed: {status}; output={}",
                self.output_text()
            );
        }
    }
}

impl Drop for FakeTui {
    fn drop(&mut self) {
        if self.child.is_some() {
            self.finish(false);
        }
    }
}

fn parse_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse wait JSON: {error}; stdout={}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[test]
fn delayed_lifecycle_preserves_unicode_lf_multiline_and_large_prompts() {
    let fixture = LifecycleFixture::new("special-inputs");
    let started = Instant::now();
    let mut tui = FakeTui::spawn(&fixture, "delay-ready-and-accept", 3);

    let ready = fixture.wait("ready", None, 3);
    assert_exit(&ready, 0, "delayed ready");
    assert!(started.elapsed() >= Duration::from_millis(75));

    let prompts = [
        "naïve café — नमस्ते — e\u{301}\nsecond line"
            .as_bytes()
            .to_vec(),
        b"first\nsecond\nthird".to_vec(),
        vec![b'x'; 1024 * 1024],
    ];
    for (index, prompt) in prompts.iter().enumerate() {
        let turn_id = format!("turn-{}", index + 1);
        fixture.arm(&turn_id, prompt);
        tui.send_frame(prompt, 1);
        let accepted = fixture.wait("accepted", Some(&turn_id), 3);
        assert_exit(&accepted, 0, "delayed acceptance");
        assert_eq!(parse_json(&accepted)["turn_id"], turn_id);
        let terminal = fixture.wait("terminal", Some(&turn_id), 3);
        assert_exit(&terminal, 0, "terminal");
        assert_eq!(parse_json(&terminal)["status"], "completed");
    }

    tui.finish(true);
}

#[test]
fn arm_rejects_noncanonical_line_endings_before_delivery() {
    for (label, prompt) in [
        ("reject-crlf", b"first\r\nsecond".as_slice()),
        ("reject-terminal-lf", b"first\nsecond\n".as_slice()),
    ] {
        let fixture = LifecycleFixture::new(label);
        let prompt_path = fixture.root.path().join("turn-1.prompt");
        std::fs::write(&prompt_path, prompt).expect("write line-ending fixture");
        let output = run_codeflow(&[
            "delegate",
            "arm",
            "--run-id",
            &fixture.run_id,
            "--state-dir",
            fixture.state.to_str().expect("utf8 state path"),
            "--turn-id",
            "turn-1",
            "--prompt-file",
            prompt_path.to_str().expect("utf8 prompt path"),
        ]);
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("canonical UTF-8 text"),
            "stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            std::fs::read_dir(fixture.state.join("turns"))
                .expect("read turns")
                .next()
                .is_none(),
            "rejected prompt created durable turn state"
        );
    }
}

#[test]
fn duplicate_enter_does_not_duplicate_delivery() {
    let fixture = LifecycleFixture::new("duplicate-enter");
    let mut tui = FakeTui::spawn(&fixture, "normal", 1);
    assert_exit(&fixture.wait("ready", None, 2), 0, "ready");

    fixture.arm("turn-1", b"exactly once");
    tui.send_frame(b"exactly once", 2);
    let accepted = fixture.wait("accepted", Some("turn-1"), 2);
    assert!(
        accepted.status.code() == Some(0),
        "accepted: stdout={}\nstderr={}\nPTY={}",
        String::from_utf8_lossy(&accepted.stdout),
        String::from_utf8_lossy(&accepted.stderr),
        tui.output_text()
    );
    assert_exit(&fixture.wait("terminal", Some("turn-1"), 2), 0, "terminal");
    tui.finish(true);

    let turn_dirs = std::fs::read_dir(fixture.state.join("turns"))
        .expect("turns directory")
        .count();
    assert_eq!(turn_dirs, 1, "duplicate Enter created another turn");
}

#[test]
fn concurrent_pty_sessions_cannot_cross_deliver() {
    let left = LifecycleFixture::new("isolation-left");
    let right = LifecycleFixture::new("isolation-right");
    let mut left_tui = FakeTui::spawn(&left, "normal", 1);
    let mut right_tui = FakeTui::spawn(&right, "normal", 1);
    assert_exit(&left.wait("ready", None, 2), 0, "left ready");
    assert_exit(&right.wait("ready", None, 2), 0, "right ready");

    left.arm("left-turn", b"left-only");
    right.arm("right-turn", b"right-only");
    right_tui.send_frame(b"right-only", 1);
    left_tui.send_frame(b"left-only", 1);

    let left_result = left.wait("terminal", Some("left-turn"), 2);
    let right_result = right.wait("terminal", Some("right-turn"), 2);
    assert_exit(&left_result, 0, "left terminal");
    assert_exit(&right_result, 0, "right terminal");
    assert_ne!(
        parse_json(&left_result)["session_id"],
        parse_json(&right_result)["session_id"]
    );
    assert!(!left.state.join("turns/right-turn").exists());
    assert!(!right.state.join("turns/left-turn").exists());
    left_tui.finish(true);
    right_tui.finish(true);
}

#[test]
fn wrong_pane_prompt_is_rejected_by_the_armed_digest() {
    let fixture = LifecycleFixture::new("misdirected-prompt");
    let mut tui = FakeTui::spawn(&fixture, "reject-accept", 1);
    assert_exit(&fixture.wait("ready", None, 2), 0, "ready");
    fixture.arm("turn-1", b"right-pane-only");

    tui.send_frame(b"left-pane-prompt", 1);
    assert_exit(
        &fixture.wait("accepted", Some("turn-1"), 1),
        124,
        "misdirected prompt acceptance",
    );
    tui.finish(true);
    assert!(!fixture.state.join("turns/turn-1/accepted.json").exists());
}

#[test]
fn spinner_and_silent_panes_never_count_as_ready() {
    for (label, scenario, output_marker) in [
        ("spinner", "spinner-no-ready", Some("FAKE_TUI_SPINNER")),
        ("silent", "silent-no-ready", None),
    ] {
        let fixture = LifecycleFixture::new(label);
        let mut tui = FakeTui::spawn(&fixture, scenario, 0);
        let ready = fixture.wait("ready", None, 1);
        assert_exit(&ready, 124, "ready timeout");
        if let Some(marker) = output_marker {
            tui.wait_for_output(marker, Duration::from_secs(1));
        } else {
            assert!(
                !tui.output_text().contains("FAKE_TUI_SPINNER"),
                "silent fixture emitted spinner output"
            );
        }
        tui.kill();
    }
}

#[test]
fn kill_at_each_nonterminal_stage_yields_only_explicit_timeouts() {
    // Before ready.
    let pre_ready = LifecycleFixture::new("kill-pre-ready");
    let mut tui = FakeTui::spawn(&pre_ready, "delay-ready-long", 1);
    tui.kill();
    assert_exit(&pre_ready.wait("ready", None, 1), 124, "pre-ready timeout");

    // After ready, before arm.
    let pre_arm = LifecycleFixture::new("kill-pre-arm");
    let mut tui = FakeTui::spawn(&pre_arm, "normal", 1);
    assert_exit(&pre_arm.wait("ready", None, 2), 0, "pre-arm ready");
    tui.kill();
    assert_exit(
        &pre_arm.wait("accepted", Some("turn-1"), 1),
        124,
        "pre-arm timeout",
    );

    // After arm, before acceptance.
    let pre_accept = LifecycleFixture::new("kill-pre-accept");
    let mut tui = FakeTui::spawn(&pre_accept, "normal", 1);
    assert_exit(&pre_accept.wait("ready", None, 2), 0, "pre-accept ready");
    pre_accept.arm("turn-1", b"never delivered");
    tui.kill();
    assert_exit(
        &pre_accept.wait("accepted", Some("turn-1"), 1),
        124,
        "pre-accept timeout",
    );

    // After acceptance, before terminal.
    let pre_terminal = LifecycleFixture::new("kill-pre-terminal");
    let mut tui = FakeTui::spawn(&pre_terminal, "delay-terminal-long", 1);
    assert_exit(
        &pre_terminal.wait("ready", None, 2),
        0,
        "pre-terminal ready",
    );
    pre_terminal.arm("turn-1", b"accepted then killed");
    tui.send_frame(b"accepted then killed", 1);
    assert_exit(
        &pre_terminal.wait("accepted", Some("turn-1"), 2),
        0,
        "accepted before kill",
    );
    tui.kill();
    assert_exit(
        &pre_terminal.wait("terminal", Some("turn-1"), 1),
        124,
        "pre-terminal timeout",
    );
}

#[test]
fn malformed_terminal_event_never_becomes_completion() {
    let fixture = LifecycleFixture::new("malformed-terminal");
    let mut tui = FakeTui::spawn(&fixture, "malformed-terminal", 1);
    assert_exit(&fixture.wait("ready", None, 2), 0, "ready");
    fixture.arm("turn-1", b"malformed terminal");
    tui.send_frame(b"malformed terminal", 1);
    assert_exit(&fixture.wait("accepted", Some("turn-1"), 2), 0, "accepted");
    assert_exit(
        &fixture.wait("terminal", Some("turn-1"), 1),
        124,
        "malformed terminal timeout",
    );
    tui.finish(true);
}

#[test]
fn dialog_is_interaction_not_completion() {
    let fixture = LifecycleFixture::new("dialog");
    let mut tui = FakeTui::spawn(&fixture, "dialog", 1);
    assert_exit(&fixture.wait("ready", None, 2), 0, "ready");
    fixture.arm("turn-1", b"ask before finishing");
    tui.send_frame(b"ask before finishing", 1);
    assert_exit(&fixture.wait("accepted", Some("turn-1"), 2), 0, "accepted");
    tui.wait_for_output(DIALOG_MARKER, Duration::from_secs(2));
    assert_exit(
        &fixture.wait("terminal", Some("turn-1"), 1),
        124,
        "dialog is not terminal",
    );
    tui.send_frame(b"continue", 1);
    assert_exit(
        &fixture.wait("terminal", Some("turn-1"), 2),
        0,
        "terminal after dialog answer",
    );
    tui.finish(true);
}

#[derive(Clone, Copy)]
enum StopHookFixture<'a> {
    TaskLifecycle,
    KnownCodexReviewGate { gate_off: bool },
    Unknown(&'a str),
}

// Test-harness representation of the exact decision table in SPC-002 Plan v5.
// It is intentionally not product code: the operator inventories the effective
// Claude hook set, while CodeFlow avoids coupling to plugin-private state.
fn preflight_stop_hooks(hooks: &[StopHookFixture<'_>]) -> Result<(), String> {
    let mut task_hooks = 0;
    for hook in hooks {
        match hook {
            StopHookFixture::TaskLifecycle => task_hooks += 1,
            StopHookFixture::KnownCodexReviewGate { gate_off: true } => {}
            StopHookFixture::KnownCodexReviewGate { gate_off: false } => {
                return Err("Codex stopReviewGate is enabled".to_string());
            }
            StopHookFixture::Unknown(command) => {
                return Err(format!("unknown sibling Stop hook: {command}"));
            }
        }
    }
    if task_hooks != 1 {
        return Err(format!(
            "expected exactly one task lifecycle Stop hook, found {task_hooks}"
        ));
    }
    Ok(())
}

#[test]
fn sibling_stop_preflight_rejects_synthetic_blocker_and_accepts_known_nonblocking() {
    let blocker = preflight_stop_hooks(&[
        StopHookFixture::TaskLifecycle,
        StopHookFixture::Unknown("synthetic-blocking-stop"),
    ])
    .expect_err("unknown blocking sibling must fail closed");
    assert!(blocker.contains("synthetic-blocking-stop"));

    let enabled = preflight_stop_hooks(&[
        StopHookFixture::TaskLifecycle,
        StopHookFixture::KnownCodexReviewGate { gate_off: false },
    ])
    .expect_err("enabled Codex review gate can block Stop");
    assert!(enabled.contains("enabled"));

    preflight_stop_hooks(&[
        StopHookFixture::TaskLifecycle,
        StopHookFixture::KnownCodexReviewGate { gate_off: true },
    ])
    .expect("the exact known sibling is nonblocking only with its gate off");
}

/// Hidden child-process entrypoint. Parent tests launch this exact test inside
/// a PTY; ordinary test-suite execution returns immediately.
#[test]
fn fake_tui_process() {
    if std::env::var_os("CF_FAKE_TUI").is_none() {
        return;
    }
    fake_tui_main();
}

fn fake_tui_main() {
    make_stdin_raw();
    let scenario = std::env::var("CF_FAKE_SCENARIO").expect("fake scenario");
    if scenario == "spinner-no-ready" {
        println!("FAKE_TUI_SPINNER");
        std::io::stdout().flush().expect("flush spinner");
        thread::sleep(Duration::from_secs(10));
        return;
    }
    if scenario == "silent-no-ready" {
        thread::sleep(Duration::from_secs(10));
        return;
    }
    if scenario == "delay-ready-long" {
        thread::sleep(Duration::from_secs(10));
        return;
    }
    if scenario == "delay-ready-and-accept" {
        thread::sleep(Duration::from_millis(100));
    }

    let run_id = std::env::var("CF_FAKE_RUN_ID").expect("fake run id");
    let state = PathBuf::from(std::env::var_os("CF_FAKE_STATE").expect("fake state"));
    let session_id = format!("{run_id}-session");
    send_hook(
        &run_id,
        &state,
        &json!({
            "hook_event_name": "SessionStart",
            "source": "startup",
            "session_id": session_id,
            "cwd": std::env::current_dir().expect("fake cwd"),
        }),
        true,
    );

    let turns = std::env::var("CF_FAKE_TURNS")
        .expect("fake turn count")
        .parse::<usize>()
        .expect("numeric fake turn count");
    let mut frames = FrameReader::new();
    for index in 0..turns {
        let prompt = frames.next().expect("prompt frame");
        if scenario == "delay-ready-and-accept" {
            thread::sleep(Duration::from_millis(75));
        }
        let prompt = String::from_utf8(prompt).expect("UTF-8 prompt");
        let prompt_id = format!("00000000-0000-4000-8000-{:012x}", index + 1);
        let accept_expected = scenario != "reject-accept";
        send_hook(
            &run_id,
            &state,
            &json!({
                "hook_event_name": "UserPromptSubmit",
                "session_id": session_id,
                "prompt_id": prompt_id,
                "prompt": prompt,
            }),
            accept_expected,
        );
        if !accept_expected {
            return;
        }

        if scenario == "dialog" {
            println!("{DIALOG_MARKER}");
            std::io::stdout().flush().expect("flush dialog");
            let answer = frames.next().expect("dialog answer frame");
            assert_eq!(answer, b"continue");
        }
        if scenario == "delay-terminal-long" {
            thread::sleep(Duration::from_secs(10));
        }
        if scenario == "malformed-terminal" {
            send_raw_hook(&run_id, &state, b"{", false);
            return;
        }
        send_hook(
            &run_id,
            &state,
            &json!({
                "hook_event_name": "Stop",
                "session_id": session_id,
                "prompt_id": prompt_id,
                "last_assistant_message": format!("fake turn {} complete", index + 1),
            }),
            true,
        );
    }
}

fn send_hook(run_id: &str, state: &Path, payload: &Value, expect_success: bool) {
    let bytes = serde_json::to_vec(payload).expect("serialize hook payload");
    send_raw_hook(run_id, state, &bytes, expect_success);
}

fn send_raw_hook(run_id: &str, state: &Path, bytes: &[u8], expect_success: bool) {
    let mut child = Command::new(binary())
        .args([
            "hook",
            "delegate-turn",
            "--run-id",
            run_id,
            "--state-dir",
            state.to_str().expect("utf8 state"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn delegate hook");
    {
        let mut stdin = child.stdin.take().expect("hook stdin");
        stdin.write_all(bytes).expect("write hook payload");
    }
    let output = child.wait_with_output().expect("wait delegate hook");
    assert_eq!(
        output.status.success(),
        expect_success,
        "hook success expectation for payload {}\nstdout={}\nstderr={}",
        String::from_utf8_lossy(bytes),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct FrameReader {
    input: std::io::BufReader<std::io::Stdin>,
    pending: Vec<u8>,
}

impl FrameReader {
    fn new() -> Self {
        Self {
            input: std::io::BufReader::new(std::io::stdin()),
            pending: Vec::new(),
        }
    }

    fn next(&mut self) -> std::io::Result<Vec<u8>> {
        let mut started = false;
        let mut pasted = None;
        loop {
            if !started && pasted.is_none() {
                if let Some(position) = find_bytes(&self.pending, PASTE_START) {
                    self.pending.drain(..position + PASTE_START.len());
                    started = true;
                }
            }
            if started && pasted.is_none() {
                if let Some(position) = find_bytes(&self.pending, PASTE_END) {
                    pasted = Some(self.pending[..position].to_vec());
                    self.pending.drain(..position + PASTE_END.len());
                }
            }
            if self
                .pending
                .first()
                .is_some_and(|byte| *byte == b'\r' || *byte == b'\n')
            {
                if let Some(frame) = pasted.take() {
                    while self
                        .pending
                        .first()
                        .is_some_and(|byte| *byte == b'\r' || *byte == b'\n')
                    {
                        self.pending.remove(0);
                    }
                    return Ok(frame);
                }
            }

            let mut chunk = [0_u8; 8192];
            let count = self.input.read(&mut chunk)?;
            if count == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "PTY closed before a pasted frame was submitted with Enter",
                ));
            }
            self.pending.extend_from_slice(&chunk[..count]);
        }
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn make_stdin_raw() {
    let mut attributes = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: fd 0 is the child process's PTY slave. `tcgetattr` initializes
    // `attributes` on success, and `tcsetattr` receives that initialized value.
    unsafe {
        assert_eq!(libc::tcgetattr(0, attributes.as_mut_ptr()), 0);
        let mut attributes = attributes.assume_init();
        libc::cfmakeraw(&raw mut attributes);
        assert_eq!(libc::tcsetattr(0, libc::TCSANOW, &raw const attributes), 0);
    }
}
