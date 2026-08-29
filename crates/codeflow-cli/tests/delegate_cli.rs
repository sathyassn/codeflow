//! End-to-end coverage for the schema-v2 delegate lifecycle.
#![cfg(unix)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const PROMPT_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

fn codeflow() -> Command {
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
}

fn run(command: &mut Command) -> Output {
    command.output().expect("codeflow runs")
}

fn hook(state: &Path, payload: &str) -> Output {
    hook_with_environment(state, payload, None)
}

fn hook_with_environment(state: &Path, payload: &str, environment: Option<(&str, &str)>) -> Output {
    let mut command = codeflow();
    command
        .args(["hook", "delegate-turn", "--run-id", "run-1", "--state-dir"])
        .arg(state)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some((marker, path)) = environment {
        command.env("TMUX_MARKER", marker).env("PATH", path);
    }
    let mut child = command.spawn().expect("hook starts");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn initialized() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    let output = run(codeflow()
        .args(["delegate", "init", "--run-id", "run-1", "--state-dir"])
        .arg(&state));
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        state.join("settings.json").to_string_lossy()
    );
    (temp, state)
}

fn make_ready(state: &Path) {
    let output = hook(
        state,
        r#"{"hook_event_name":"SessionStart","source":"startup","session_id":"s1","cwd":"/tmp"}"#,
    );
    assert_eq!(output.status.code(), Some(0));
}

fn arm(state: &Path, turn: &str, prompt: &str) -> Output {
    arm_bytes(state, turn, prompt.as_bytes())
}

fn arm_bytes(state: &Path, turn: &str, prompt: &[u8]) -> Output {
    let prompt_file = state.parent().unwrap().join(format!("{turn}.txt"));
    std::fs::write(&prompt_file, prompt).unwrap();
    run(codeflow()
        .args(["delegate", "arm", "--run-id", "run-1", "--state-dir"])
        .arg(state)
        .args(["--turn-id", turn, "--prompt-file"])
        .arg(prompt_file))
}

#[test]
fn arm_rejects_noncanonical_prompt_files_before_creating_turn_state() {
    for (turn, prompt) in [
        ("empty", b"".as_slice()),
        ("crlf", b"first\r\nsecond".as_slice()),
        ("terminal-lf", b"terminal line break\n".as_slice()),
        ("tab", b"tab\tbecomes spaces".as_slice()),
        ("escape", b"escape\x1bsequence".as_slice()),
        ("delete", b"delete\x7fcharacter".as_slice()),
        ("nul", b"embedded\0nul".as_slice()),
        ("invalid-utf8", b"\xffinvalid".as_slice()),
    ] {
        let (_temp, state) = initialized();
        let output = arm_bytes(&state, turn, prompt);
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("canonical UTF-8 text"),
            "stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!state.join("turns").join(turn).exists());
    }
}

fn wait(state: &Path, turn: Option<&str>, until: &str, seconds: &str) -> Output {
    let mut command = codeflow();
    command
        .args(["delegate", "wait", "--run-id", "run-1", "--state-dir"])
        .arg(state)
        .args(["--until", until, "--timeout-seconds", seconds]);
    if let Some(turn) = turn {
        command.args(["--turn-id", turn]);
    }
    run(&mut command)
}

#[test]
fn lifecycle_reports_ready_accepted_and_completed_without_tmux() {
    let (_temp, state) = initialized();
    make_ready(&state);
    assert_eq!(wait(&state, None, "ready", "0").status.code(), Some(0));
    assert_eq!(arm(&state, "turn-1", "hello").status.code(), Some(0));
    let accepted = hook(
        &state,
        &format!(
            r#"{{"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt_id":"{PROMPT_ID}","prompt":"hello"}}"#
        ),
    );
    assert_eq!(accepted.status.code(), Some(0));
    assert_eq!(
        wait(&state, Some("turn-1"), "accepted", "0").status.code(),
        Some(0)
    );
    let stopped = hook(
        &state,
        &format!(
            r#"{{"hook_event_name":"Stop","session_id":"s1","prompt_id":"{PROMPT_ID}","last_assistant_message":"done"}}"#
        ),
    );
    assert_eq!(stopped.status.code(), Some(0));
    let terminal = wait(&state, Some("turn-1"), "terminal", "0");
    assert_eq!(terminal.status.code(), Some(0));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&terminal.stdout).unwrap()["status"],
        "completed"
    );
}

#[cfg(unix)]
#[test]
fn doctor_roundtrip_drives_the_real_binary() {
    let binary = std::path::PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let mut paths = vec![binary.parent().unwrap().to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    let path = std::env::join_paths(paths).unwrap();
    let output = run(codeflow()
        .args(["doctor", "--check", "delegate-roundtrip"])
        .env("PATH", path));
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("delegate-roundtrip"));
}

#[cfg(unix)]
#[test]
fn schema_v2_hooks_make_zero_tmux_calls_including_retries() {
    use std::os::unix::fs::PermissionsExt;

    let (temp, state) = initialized();
    let fake_bin = temp.path().join("bin");
    std::fs::create_dir(&fake_bin).unwrap();
    let fake_tmux = fake_bin.join("tmux");
    std::fs::write(&fake_tmux, "#!/bin/sh\n: > \"$TMUX_MARKER\"\nexit 99\n").unwrap();
    std::fs::set_permissions(&fake_tmux, std::fs::Permissions::from_mode(0o755)).unwrap();
    let marker = temp.path().join("tmux-called");
    let environment = (marker.to_str().unwrap(), fake_bin.to_str().unwrap());

    let ready_payload =
        r#"{"hook_event_name":"SessionStart","source":"startup","session_id":"s1","cwd":"/tmp"}"#;
    for _ in 0..2 {
        assert_eq!(
            hook_with_environment(&state, ready_payload, Some(environment))
                .status
                .code(),
            Some(0)
        );
    }
    assert_eq!(arm(&state, "turn-1", "hello").status.code(), Some(0));
    let accepted = format!(
        r#"{{"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt_id":"{PROMPT_ID}","prompt":"hello"}}"#
    );
    assert_eq!(
        hook_with_environment(&state, &accepted, Some(environment))
            .status
            .code(),
        Some(0)
    );
    let terminal = format!(
        r#"{{"hook_event_name":"Stop","session_id":"s1","prompt_id":"{PROMPT_ID}","last_assistant_message":"done"}}"#
    );
    for _ in 0..2 {
        assert_eq!(
            hook_with_environment(&state, &terminal, Some(environment))
                .status
                .code(),
            Some(0)
        );
    }
    assert!(!marker.exists(), "schema-v2 must never invoke tmux");
}

#[test]
fn prompt_commit_failures_exit_two_and_do_not_accept() {
    let (_temp, state) = initialized();
    make_ready(&state);
    assert_eq!(arm(&state, "turn-1", "expected").status.code(), Some(0));
    let mismatch = hook(
        &state,
        r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt":"different"}"#,
    );
    assert_eq!(mismatch.status.code(), Some(2));
    assert!(!state.join("turns/turn-1/accepted.json").exists());

    let missing = hook(
        &state,
        r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1"}"#,
    );
    assert_eq!(missing.status.code(), Some(2));
    assert!(!state.join("turns/turn-1/accepted.json").exists());
}

#[test]
fn a_new_prompt_id_after_acceptance_exits_two_and_preserves_the_first_record() {
    let (_temp, state) = initialized();
    make_ready(&state);
    assert_eq!(arm(&state, "turn-1", "hello").status.code(), Some(0));
    let first = format!(
        r#"{{"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt_id":"{PROMPT_ID}","prompt":"hello"}}"#
    );
    assert_eq!(hook(&state, &first).status.code(), Some(0));
    let accepted = state.join("turns/turn-1/accepted.json");
    let original = std::fs::read(&accepted).unwrap();

    let duplicate = hook(
        &state,
        r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt_id":"223e4567-e89b-12d3-a456-426614174000","prompt":"hello"}"#,
    );
    assert_eq!(duplicate.status.code(), Some(2));
    assert_eq!(std::fs::read(accepted).unwrap(), original);
}

#[test]
fn path_semantic_turn_ids_fail_without_creating_records() {
    let (_temp, state) = initialized();
    for turn in [".", ".."] {
        assert_eq!(arm(&state, turn, "hello").status.code(), Some(1));
    }
    assert!(!state.join("request.json").exists());
    assert!(!state.join("turns/request.json").exists());
}

#[test]
fn failed_terminal_uses_exit_ten_and_timeout_uses_124() {
    let (_temp, state) = initialized();
    make_ready(&state);
    assert_eq!(
        wait(&state, Some("turn-1"), "terminal", "0").status.code(),
        Some(124)
    );
    assert_eq!(arm(&state, "turn-1", "hello").status.code(), Some(0));
    assert_eq!(
        hook(
            &state,
            &format!(
                r#"{{"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt_id":"{PROMPT_ID}","prompt":"hello"}}"#
            ),
        )
        .status
        .code(),
        Some(0)
    );
    assert_eq!(
        hook(
            &state,
            &format!(
                r#"{{"hook_event_name":"StopFailure","session_id":"s1","prompt_id":"{PROMPT_ID}","error":{{"type":"tool_error","message":"failed"}}}}"#
            ),
        )
        .status
        .code(),
        Some(0)
    );
    let terminal = wait(&state, Some("turn-1"), "terminal", "0");
    assert_eq!(terminal.status.code(), Some(10));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&terminal.stdout).unwrap()["status"],
        "failed"
    );
}

#[test]
fn poison_precedes_an_existing_ready_record() {
    let (_temp, state) = initialized();
    make_ready(&state);
    let resumed = hook(
        &state,
        r#"{"hook_event_name":"SessionStart","source":"resume","session_id":"s2","cwd":"/tmp"}"#,
    );
    assert_eq!(resumed.status.code(), Some(1));
    let ready = wait(&state, None, "ready", "0");
    assert_eq!(ready.status.code(), Some(11));
    assert!(String::from_utf8_lossy(&ready.stderr).contains("poisoned"));
    assert_eq!(arm(&state, "turn-1", "hello").status.code(), Some(1));
}

#[test]
fn schema_v2_rejects_legacy_result_and_never_falls_back() {
    let (_temp, state) = initialized();
    let result = state.join("legacy.json");
    let output = hook_with_both(&state, &result);
    assert_eq!(output.status.code(), Some(2));
    assert!(!result.exists());
}

fn hook_with_both(state: &Path, result: &Path) -> Output {
    let mut child = codeflow()
        .args(["hook", "delegate-turn", "--run-id", "run-1", "--state-dir"])
        .arg(state)
        .args(["--result"])
        .arg(result)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(
            br#"{"hook_event_name":"Stop","session_id":"s1","last_assistant_message":"done"}"#,
        )
        .unwrap();
    child.wait_with_output().unwrap()
}

#[cfg(unix)]
#[test]
fn sigint_after_acceptance_exits_130_and_poisons_the_run() {
    let (_temp, state) = initialized();
    make_ready(&state);
    assert_eq!(arm(&state, "turn-1", "hello").status.code(), Some(0));
    assert_eq!(
        hook(
            &state,
            &format!(
                r#"{{"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt_id":"{PROMPT_ID}","prompt":"hello"}}"#
            ),
        )
        .status
        .code(),
        Some(0)
    );

    let child = codeflow()
        .args(["delegate", "wait", "--run-id", "run-1", "--state-dir"])
        .arg(&state)
        .args([
            "--turn-id",
            "turn-1",
            "--until",
            "terminal",
            "--timeout-seconds",
            "30",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Coverage and parallel test startup can delay the child before it reaches
    // the installed SIGINT handler. Allow that bounded initialization window
    // so this test exercises interruption handling rather than pre-main exit.
    std::thread::sleep(std::time::Duration::from_secs(1));
    let signal = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .output()
        .unwrap();
    assert!(signal.status.success());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(130));
    assert!(state.join("poison.json").exists());
    assert_eq!(
        wait(&state, Some("turn-1"), "terminal", "0").status.code(),
        Some(11)
    );
}
