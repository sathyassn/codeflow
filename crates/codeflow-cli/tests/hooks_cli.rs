//! End-to-end tests for the hook plane CLI surface:
//! `codeflow hook <git-guard|session-orient|session-summary>`,
//! `codeflow git-hook <pre-commit|commit-msg|pre-push>`, `codeflow orient`.
//!
//! Each test runs the real binary (`CARGO_BIN_EXE_codeflow`) in a tempdir
//! git repo, exercising the exit-code contracts the hooks rely on.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

#[cfg(unix)]
struct RestorePermissions {
    path: std::path::PathBuf,
    original: std::fs::Permissions,
}

#[cfg(unix)]
impl RestorePermissions {
    fn deny(path: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;

        let original = std::fs::metadata(path).unwrap().permissions();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
        Self {
            path: path.to_path_buf(),
            original,
        }
    }
}

#[cfg(unix)]
impl Drop for RestorePermissions {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.path, self.original.clone());
    }
}

/// Shared isolated `CODEFLOW_HOME`: these tests initialize tempdir repos
/// (policy.json / project.toml), so without the override every invocation's
/// registry touch would write them into the developer's real
/// `~/.codeflow/registry.json` (per the `recall_remote_cli.rs` pattern).
fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn codeflow() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_codeflow"));
    // Isolate from the developer's environment.
    cmd.env("CODEFLOW_HOME", isolated_home())
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    cmd
}

fn run_with_stdin(cmd: &mut Command, stdin: &str) -> Output {
    run_with_stdin_bytes(cmd, stdin.as_bytes())
}

fn run_with_stdin_bytes(cmd: &mut Command, stdin: &[u8]) -> Output {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary spawns");
    child
        .stdin
        .as_mut()
        .expect("stdin piped")
        .write_all(stdin)
        .expect("stdin written");
    child.wait_with_output().expect("binary exits")
}

#[cfg(unix)]
fn task_branch_ci(dir: &Path, branch: &str, valid_body: bool) -> Output {
    let mut command = codeflow();
    command
        .args(["ci", "--base", "main", "--head", "HEAD", "--branch", branch])
        .current_dir(dir);
    if valid_body {
        command.args([
            "--pr-body",
            "## Summary\nBounded task.\n\n## Changes\n- implementation\n\n## Testing\n- focused test\nNot tested: Windows.\n\n## Reviews\nNone: pending review.\n\n## Release impact\n- Impact: patch\n- Breaking: no\n- Rationale: Preserve the public contract.\n- Migration: none",
        ]);
    }
    command.output().unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn init_repo(dir: &Path, branch: &str) {
    git(dir, &["init", "-b", branch]);
    git(dir, &["config", "user.email", "t@example.com"]);
    git(dir, &["config", "user.name", "t"]);
    std::fs::write(dir.join("base.txt"), "base\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "chore: init"]);
}

fn write_policy(dir: &Path, json: &str) {
    let cf = dir.join(".codeflow");
    std::fs::create_dir_all(&cf).unwrap();
    std::fs::write(cf.join("policy.json"), json).unwrap();
}

fn wire_reference_transaction_hook(dir: &Path) {
    let hook = dir.join(".git/hooks/reference-transaction");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nexec '{}' git-hook reference-transaction \"$@\"\n",
            env!("CARGO_BIN_EXE_codeflow")
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn guard_payload(command: &str, cwd: &Path) -> String {
    format!(
        r#"{{"tool_name":"Bash","tool_input":{{"command":{}}},"cwd":{}}}"#,
        json_string(command),
        json_string(&cwd.to_string_lossy()),
    )
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// codeflow hook delegate-turn
// ---------------------------------------------------------------------------

#[test]
#[cfg(unix)]
fn delegate_turn_persists_once_and_signals_scoped_tmux_channel() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let tmux = bin.join("tmux");
    std::fs::write(
        &tmux,
        "#!/bin/sh\n[ \"$1\" = wait-for ] && [ \"$2\" = -S ] && [ \"$3\" = codeflow-delegate-review-42 ]\n",
    )
    .unwrap();
    std::fs::set_permissions(&tmux, std::fs::Permissions::from_mode(0o700)).unwrap();

    let result = dir.path().join("result.json");
    let payload = r#"{"hook_event_name":"Stop","session_id":"session-1","last_assistant_message":"VERDICT: approved"}"#;
    let out = run_with_stdin(
        codeflow()
            .args(["hook", "delegate-turn", "--run-id", "review-42", "--result"])
            .arg(&result)
            .env("PATH", &bin)
            .current_dir(dir.path()),
        payload,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&result).unwrap()).unwrap();
    assert_eq!(saved["run_id"], "review-42");
    assert_eq!(saved["status"], "completed");
    assert_eq!(saved["last_assistant_message"], "VERDICT: approved");
    assert_eq!(
        std::fs::metadata(&result).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let retry = run_with_stdin(
        codeflow()
            .args(["hook", "delegate-turn", "--run-id", "review-42", "--result"])
            .arg(&result)
            .env("PATH", &bin)
            .current_dir(dir.path()),
        payload,
    );
    assert_eq!(retry.status.code(), Some(0));

    let conflicting = run_with_stdin(
        codeflow()
            .args(["hook", "delegate-turn", "--run-id", "review-42", "--result"])
            .arg(&result)
            .env("PATH", &bin)
            .current_dir(dir.path()),
        r#"{"hook_event_name":"Stop","session_id":"session-1","last_assistant_message":"different"}"#,
    );
    assert_eq!(conflicting.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&conflicting.stderr).contains("different terminal evidence"));
}

#[test]
fn delegate_turn_rejects_missing_and_unsafe_arguments() {
    let missing = run_with_stdin(
        codeflow().args(["hook", "delegate-turn"]),
        r#"{"hook_event_name":"Stop","last_assistant_message":"done"}"#,
    );
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("--run-id is required"));

    let missing_result = run_with_stdin(
        codeflow().args(["hook", "delegate-turn", "--run-id", "review-42"]),
        r#"{"hook_event_name":"Stop","last_assistant_message":"done"}"#,
    );
    assert_eq!(missing_result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing_result.stderr).contains("--result is required"));

    let unsafe_id = run_with_stdin(
        codeflow().args([
            "hook",
            "delegate-turn",
            "--run-id",
            "bad/channel",
            "--result",
            "/private/result.json",
        ]),
        r#"{"hook_event_name":"Stop","last_assistant_message":"done"}"#,
    );
    assert_eq!(unsafe_id.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&unsafe_id.stderr).contains("run id must be"));
}

// ---------------------------------------------------------------------------
// codeflow hook git-guard
// ---------------------------------------------------------------------------

#[test]
fn git_guard_blocks_push_to_protected_with_exit_2() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");

    let payload = guard_payload("git push origin main", dir.path());
    let out = run_with_stdin(
        codeflow()
            .args(["hook", "git-guard"])
            .current_dir(dir.path()),
        &payload,
    );

    assert_eq!(out.status.code(), Some(2), "expected exit 2 (block)");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("git.push_to_protected"), "{stderr}");
    assert!(stderr.contains("codeflow integrate"), "{stderr}");
}

#[test]
fn git_guard_allows_force_push_to_feature_branch() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");

    let payload = guard_payload("git push --force origin feat/x", dir.path());
    let out = run_with_stdin(
        codeflow()
            .args(["hook", "git-guard"])
            .current_dir(dir.path()),
        &payload,
    );
    assert_eq!(out.status.code(), Some(0), "force-push to feature allowed");
}

#[test]
fn git_guard_honors_policy_glob_extension() {
    // AC #3: adding release/* to policy.json changes guard behavior with no
    // code change.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "release/9.9");
    write_policy(
        dir.path(),
        r#"{"schema_version":1,"git":{"protected_branches":["main","release/*"]}}"#,
    );

    let payload = guard_payload("git commit -m 'fix: x'", dir.path());
    let out = run_with_stdin(
        codeflow()
            .args(["hook", "git-guard"])
            .current_dir(dir.path()),
        &payload,
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("git.commit_to_protected"),);
}

#[test]
fn git_guard_integrate_token_allows_protected_commit() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");

    let payload = guard_payload("git commit -m 'chore: integrate'", dir.path());
    let out = run_with_stdin(
        codeflow()
            .args(["hook", "git-guard"])
            .env("CODEFLOW_INTEGRATE_TOKEN", "gate")
            .current_dir(dir.path()),
        &payload,
    );
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn git_guard_blocks_hook_plane_self_disarm_with_exit_2() {
    // Hook-plane self-disarm on a *feature* branch (where a plain commit is
    // fine): the block proves the hook-integrity rule, not commit-to-protected.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    for cmd in [
        "rm -rf .git/hooks",
        "git config core.hooksPath /tmp/evil",
        "git -c core.hooksPath=/dev/null commit -m x",
        "GIT_SKIP_HOOKS=1 git commit -m x",
        "echo bad > .codeflow/policy.json",
    ] {
        let payload = guard_payload(cmd, dir.path());
        let out = run_with_stdin(
            codeflow()
                .args(["hook", "git-guard"])
                .current_dir(dir.path()),
            &payload,
        );
        assert_eq!(out.status.code(), Some(2), "{cmd}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("git.hook_integrity"),
            "{cmd}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn git_guard_blocks_remote_tracking_ref_poisoning_with_exit_2() {
    // Task 2a: the agent-writable sync oracle cannot be hand-set.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    let payload = guard_payload(
        "git update-ref refs/remotes/origin/main deadbeef",
        dir.path(),
    );
    let out = run_with_stdin(
        codeflow()
            .args(["hook", "git-guard"])
            .current_dir(dir.path()),
        &payload,
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("git.local_ref_protection"));
}

#[test]
fn exec_guard_blocks_dangerous_command_with_exit_2() {
    // Task 6: a PreToolUse block must map to exit 2, not 0.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    let payload = guard_payload("rm -rf /", dir.path());
    let out = run_with_stdin(
        codeflow()
            .args(["hook", "exec-guard"])
            .current_dir(dir.path()),
        &payload,
    );
    assert_eq!(
        out.status.code(),
        Some(2),
        "dangerous command must block with exit 2"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("security.dangerous_commands"));
}

#[test]
fn exec_guard_blocks_powershell_catastrophe_with_exit_2() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    let payload = serde_json::json!({
        "tool_name": "PowerShell",
        "tool_input": {"command": r"Remove-Item -Recurse C:\Windows"},
        "cwd": dir.path(),
    })
    .to_string();
    let out = run_with_stdin(
        codeflow()
            .args(["hook", "exec-guard"])
            .current_dir(dir.path()),
        &payload,
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("security.dangerous_commands"));
}

#[test]
fn exec_guard_unwraps_bundled_shell_flags_without_blocking_project_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");

    for command in ["bash -lc 'rm -rf /System'", "sh -ic 'rm -rf /Library'"] {
        let out = run_with_stdin(
            codeflow()
                .args(["hook", "exec-guard"])
                .current_dir(dir.path()),
            &guard_payload(command, dir.path()),
        );
        assert_eq!(out.status.code(), Some(2), "should block: {command}");
    }

    for command in ["rm -rf ~/code/app/target", "rm -rf $HOME/code/app/build"] {
        let out = run_with_stdin(
            codeflow()
                .args(["hook", "exec-guard"])
                .current_dir(dir.path()),
            &guard_payload(command, dir.path()),
        );
        assert!(out.status.success(), "should allow: {command}");
    }
}

#[test]
fn git_guard_blocks_pr_body_attribution() {
    // AC #13: attribution in a PR body blocked at gh pr create.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");

    let cmd = "gh pr create --title 'feat: x' --body 'Generated with Claude Code'";
    let out = run_with_stdin(
        codeflow()
            .args(["hook", "git-guard"])
            .current_dir(dir.path()),
        &guard_payload(cmd, dir.path()),
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("git.ai_attribution"));
}

#[test]
fn git_guard_ignores_non_bash_tools_and_garbage() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");

    let out = run_with_stdin(
        codeflow()
            .args(["hook", "git-guard"])
            .current_dir(dir.path()),
        r#"{"tool_name":"Write","tool_input":{"file_path":"x"}}"#,
    );
    assert_eq!(out.status.code(), Some(0));

    let out = run_with_stdin(
        codeflow()
            .args(["hook", "git-guard"])
            .current_dir(dir.path()),
        "not json",
    );
    assert_eq!(out.status.code(), Some(0), "fail open on garbage payload");
}

// ---------------------------------------------------------------------------
// codeflow git-hook pre-commit / commit-msg / pre-push
// ---------------------------------------------------------------------------

#[test]
fn pre_commit_blocks_secret_and_protected_branch() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    std::fs::write(dir.path().join("leak.txt"), "AKIAIOSFODNN7EXAMPLF\n").unwrap();
    git(dir.path(), &["add", "leak.txt"]);

    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("git.commit_to_protected"), "{stderr}");
    assert!(stderr.contains("git.secret_scan"), "{stderr}");
}

#[test]
fn pre_commit_passes_clean_feature_branch() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    std::fs::write(dir.path().join("ok.txt"), "nothing sensitive\n").unwrap();
    git(dir.path(), &["add", "ok.txt"]);

    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn commit_msg_blocks_attribution_and_malformed_subject() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");

    let msg = dir.path().join("MSG");
    std::fs::write(
        &msg,
        "feat: x\n\nCo-Authored-By: Claude <noreply@anthropic.com>\n",
    )
    .unwrap();
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "commit-msg", msg.to_str().unwrap()])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("git.ai_attribution"));

    std::fs::write(&msg, "added some stuff\n").unwrap();
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "commit-msg", msg.to_str().unwrap()])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("git.commit_format"));

    std::fs::write(&msg, "feat(cli): wire the hook plane\n").unwrap();
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "commit-msg", msg.to_str().unwrap()])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn commit_msg_blocks_on_invalid_policy_value() {
    // Strict validation: a typo'd policy value must fail loudly at the hook,
    // naming the key and its valid options — never silently revert the whole
    // file to defaults (which could disable a hardened gate). A conforming
    // message is otherwise irrelevant.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    write_policy(
        dir.path(),
        r#"{"schema_version":1,"git":{"commit_ticket_required":"worn"}}"#,
    );
    let msg = dir.path().join("MSG");
    std::fs::write(&msg, "feat: x\n").unwrap();
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "commit-msg", msg.to_str().unwrap()])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(1), "an invalid policy must block");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("git.commit_ticket_required"), "{stderr}");
    assert!(stderr.contains("off, warn, allow, block"), "{stderr}");

    // Fixing the value lets the same commit through.
    write_policy(
        dir.path(),
        r#"{"schema_version":1,"git":{"commit_ticket_required":"block","commit_ticket_keys":["Refs"]}}"#,
    );
    std::fs::write(&msg, "feat: x\n\n- do it\n\nRefs: PROJ-1\n").unwrap();
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "commit-msg", msg.to_str().unwrap()])
            .current_dir(dir.path()),
        "",
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "a valid policy + conforming commit passes: {stderr}"
    );
}

#[test]
fn commit_msg_warn_level_proceeds() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    write_policy(
        dir.path(),
        r#"{"schema_version":1,"git":{"commit_format":"warn"}}"#,
    );

    let msg = dir.path().join("MSG");
    std::fs::write(&msg, "freestyle subject\n").unwrap();
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "commit-msg", msg.to_str().unwrap()])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(0), "warn level must not block");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("warning"), "{stderr}");
}

#[test]
fn pre_push_blocks_protected_and_honors_glob_extension() {
    const ZERO: &str = "0000000000000000000000000000000000000000";

    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    write_policy(
        dir.path(),
        r#"{"schema_version":1,"git":{"protected_branches":["main","release/*"]}}"#,
    );

    let stdin = format!("refs/heads/release/1.2 abc123 refs/heads/release/1.2 {ZERO}\n");
    let out = run_with_stdin(
        codeflow()
            .args([
                "git-hook",
                "pre-push",
                "origin",
                "https://example.com/r.git",
            ])
            .current_dir(dir.path()),
        &stdin,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("git.push_to_protected"));

    // Feature branch with sanctioned prefix passes (test gate note allowed).
    // The push set runs `codeflow ci` on the pushed range, so the local sha
    // must be a real commit, as it always is under git.
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git rev-parse");
    let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
    let stdin = format!("refs/heads/feat/x {head} refs/heads/feat/x {ZERO}\n");
    let out = run_with_stdin(
        codeflow()
            .args([
                "git-hook",
                "pre-push",
                "origin",
                "https://example.com/r.git",
            ])
            .current_dir(dir.path()),
        &stdin,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn pre_merge_commit_blocks_on_protected_and_honors_overrides() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");

    // Default: a merge commit on main is blocked.
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-merge-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("git.merge_to_protected"));

    // A human's CODEFLOW_HUMAN_OVERRIDE=1 passes the git layer (ADR-0007).
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-merge-commit"])
            .env("CODEFLOW_HUMAN_OVERRIDE", "1")
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "human override passes the git layer"
    );

    // The integrate gate token also passes (sanctioned local merge).
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-merge-commit"])
            .env("CODEFLOW_INTEGRATE_TOKEN", "gate")
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn pre_merge_commit_passes_on_feature_branch() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-merge-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn real_wired_reference_transaction_closes_ff_merge_gap() {
    // The full plane through git (ADR-0007), boundary now CLOSED. A
    // fast-forward merge fires no pre-merge-commit (no commit is created) but
    // it moves refs/heads/main, which fires reference-transaction — so the
    // ff-merge onto a protected branch is refused harness-agnostically, and a
    // human's override still passes.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");

    // A fast-forwardable branch, built before the ref hook is wired.
    git(dir.path(), &["checkout", "-b", "feat/ff"]);
    std::fs::write(dir.path().join("ff.txt"), "ff\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "feat: ff"]);
    git(dir.path(), &["checkout", "main"]);

    wire_reference_transaction_hook(dir.path());

    // ff-merge onto protected main is now refused at the git layer.
    let ff = Command::new("git")
        .args(["merge", "--ff-only", "feat/ff"])
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .output()
        .unwrap();
    assert!(
        !ff.status.success(),
        "ff-merge into protected is now refused"
    );
    assert!(String::from_utf8_lossy(&ff.stderr).contains("git.local_ref_protection"));

    // A human's CODEFLOW_HUMAN_OVERRIDE=1 lets the same ff-merge through.
    let ff_human = Command::new("git")
        .args(["merge", "--ff-only", "feat/ff"])
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("CODEFLOW_HUMAN_OVERRIDE", "1")
        .output()
        .unwrap();
    assert!(
        ff_human.status.success(),
        "human override passes the git layer: {}",
        String::from_utf8_lossy(&ff_human.stderr)
    );
}

#[test]
fn reference_transaction_invalid_utf8_blocks_but_pre_push_remains_nonblocking() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");

    let prepared = run_with_stdin_bytes(
        codeflow()
            .args(["git-hook", "reference-transaction", "prepared"])
            .current_dir(dir.path()),
        &[0xff],
    );
    let prepared_stderr = String::from_utf8_lossy(&prepared.stderr);
    assert_eq!(prepared.status.code(), Some(1), "{prepared_stderr}");
    assert!(
        prepared_stderr.contains("could not read reference-transaction input"),
        "{prepared_stderr}"
    );
    for misleading in ["warning", "check skipped", "ref checks degraded"] {
        assert!(
            !prepared_stderr.contains(misleading),
            "fatal diagnostic contains {misleading:?}: {prepared_stderr}"
        );
    }

    let pre_push = run_with_stdin_bytes(
        codeflow()
            .args(["git-hook", "pre-push", "origin", "example.invalid"])
            .current_dir(dir.path()),
        &[0xff],
    );
    let pre_push_stderr = String::from_utf8_lossy(&pre_push.stderr);
    assert_eq!(pre_push.status.code(), Some(0), "{pre_push_stderr}");
    assert!(
        pre_push_stderr.contains("ref checks degraded"),
        "{pre_push_stderr}"
    );
}

#[test]
fn reference_transaction_non_utf8_remote_input_respects_active_policy() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    let input = b"0000000000000000000000000000000000000000 1111111111111111111111111111111111111111 refs/remotes/origin/topic-\xff\n";
    let run = || {
        run_with_stdin_bytes(
            codeflow()
                .args(["git-hook", "reference-transaction", "prepared"])
                .current_dir(dir.path()),
            input,
        )
    };
    let active = run();
    assert_eq!(active.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&active.stderr)
        .contains("operation blocked while ref protection is active"));

    std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
    std::fs::write(
        dir.path().join(".codeflow/policy.json"),
        r#"{"git":{"local_ref_protection":"off","delete_protected":"off"}}"#,
    )
    .unwrap();
    let inactive = run();
    assert_eq!(inactive.status.code(), Some(0));
    assert!(inactive.stderr.is_empty());
}

#[test]
fn reference_transaction_valid_remote_only_input_remains_nonblocking() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "reference-transaction", "prepared"])
            .current_dir(dir.path()),
        "0000000000000000000000000000000000000000 1111111111111111111111111111111111111111 refs/remotes/origin/main\n",
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn real_wired_reference_transaction_allows_git_pull_sync() {
    // ADR-0009: narrowing the sync oracle to an exact remote-head match must
    // not break the one legitimate protected-branch sync — `git pull` that
    // fast-forwards main onto the freshly fetched origin/main.
    let origin = tempfile::tempdir().unwrap();
    init_repo(origin.path(), "main");

    let workroot = tempfile::tempdir().unwrap();
    let work = workroot.path().join("repo");
    let clone = Command::new("git")
        .args([
            "clone",
            origin.path().to_str().unwrap(),
            work.to_str().unwrap(),
        ])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        clone.status.success(),
        "clone: {}",
        String::from_utf8_lossy(&clone.stderr)
    );
    git(&work, &["config", "user.email", "t@example.com"]);
    git(&work, &["config", "user.name", "t"]);

    let hook = work.join(".git/hooks/reference-transaction");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nexec '{}' git-hook reference-transaction \"$@\"\n",
            env!("CARGO_BIN_EXE_codeflow")
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // Advance origin so there is something to sync.
    std::fs::write(origin.path().join("o.txt"), "o\n").unwrap();
    git(origin.path(), &["add", "."]);
    git(origin.path(), &["commit", "-m", "feat: origin advance"]);

    // `git pull --ff-only`: the fetch updates refs/remotes/origin/main, then the
    // fast-forward moves refs/heads/main onto exactly that head — allowed.
    let pull = Command::new("git")
        .args(["pull", "--ff-only", "origin", "main"])
        .current_dir(&work)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .output()
        .unwrap();
    assert!(
        pull.status.success(),
        "git pull ff-only on protected main must stay allowed: {}",
        String::from_utf8_lossy(&pull.stderr)
    );
}

#[test]
fn real_wired_reference_transaction_blocks_reset_hard_and_branch_delete_on_protected() {
    // The two ref updates classic client hooks never saw: `reset --hard` and
    // `branch -D` on a protected branch (ADR-0007), now caught at the git layer.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "chore: two"]);
    // A protected sibling (master is protected by default) to attempt deleting.
    git(dir.path(), &["branch", "master"]);

    let hook = dir.path().join(".git/hooks/reference-transaction");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nexec '{}' git-hook reference-transaction \"$@\"\n",
            env!("CARGO_BIN_EXE_codeflow")
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let git_run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("CODEFLOW_INTEGRATE_TOKEN")
            .env_remove("CODEFLOW_HUMAN_OVERRIDE")
            .output()
            .unwrap()
    };

    // reset --hard on protected main is refused.
    let reset = git_run(&["reset", "--hard", "HEAD~1"]);
    assert!(
        !reset.status.success(),
        "reset --hard on protected is refused"
    );
    assert!(String::from_utf8_lossy(&reset.stderr).contains("git.local_ref_protection"));

    // branch -D of a protected branch is refused (via delete_protected).
    let del = git_run(&["branch", "-D", "master"]);
    assert!(
        !del.status.success(),
        "branch -D of a protected branch is refused"
    );
    assert!(String::from_utf8_lossy(&del.stderr).contains("git.delete_protected"));
}

#[test]
fn real_wired_reference_transaction_fails_closed_when_reftable_cannot_be_evaluated() {
    let dir = tempfile::tempdir().unwrap();
    let init = Command::new("git")
        .args(["init", "--ref-format=reftable", "-b", "main"])
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    if !init.status.success() {
        let stderr = String::from_utf8_lossy(&init.stderr);
        let unsupported = stderr.contains("ref-format")
            && (stderr.contains("unknown option")
                || stderr.contains("unknown ref storage format")
                || stderr.contains("unsupported ref storage format"));
        if unsupported {
            eprintln!(
                "skipping reftable hook regression: installed git lacks --ref-format=reftable: {stderr}"
            );
            return;
        }
        panic!("reftable repository setup failed unexpectedly: {stderr}");
    }
    git(dir.path(), &["config", "user.email", "t@example.com"]);
    git(dir.path(), &["config", "user.name", "t"]);
    std::fs::write(dir.path().join("base.txt"), "base\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "chore: init"]);
    let original = Command::new("git")
        .args(["rev-parse", "refs/heads/main"])
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(original.status.success());
    wire_reference_transaction_hook(dir.path());

    let delete = Command::new("git")
        .args(["update-ref", "-d", "refs/heads/main"])
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&delete.stderr);
    assert!(
        !delete.status.success(),
        "an unevaluable protected-ref transaction must fail closed: {stderr}"
    );
    if stderr.contains("could not evaluate protected-ref transaction") {
        for misleading in ["warning", "check skipped", "ref checks degraded"] {
            assert!(
                !stderr.contains(misleading),
                "fatal diagnostic contains {misleading:?}: {stderr}"
            );
        }
    } else {
        assert!(
            stderr.contains("git.delete_protected"),
            "a backend with reftable support must enforce the ordinary protected-delete rule: {stderr}"
        );
    }

    let retained = Command::new("git")
        .args(["rev-parse", "refs/heads/main"])
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        retained.status.success(),
        "protected ref was deleted despite hook failure"
    );
    assert_eq!(retained.stdout, original.stdout);
}

#[test]
fn real_wired_hook_blocks_commit_via_git() {
    // The full plane: shim in .git/hooks execs the real binary, git commit
    // on a protected branch is refused end-to-end.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");

    let hook = dir.path().join(".git/hooks/pre-commit");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nexec '{}' git-hook pre-commit \"$@\"\n",
            env!("CARGO_BIN_EXE_codeflow")
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    std::fs::write(dir.path().join("f.txt"), "x\n").unwrap();
    git(dir.path(), &["add", "f.txt"]);
    let out = Command::new("git")
        .args(["commit", "-m", "feat: should be blocked"])
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "commit on main must be refused by the wired hook"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("git.commit_to_protected"),);
}

// Codex EPC-017 review round 2: Git runs commit-msg before its own message
// cleanup, so the installed hook must scan what that cleanup keeps. These
// drive real `git commit` through the wired hook in each cleanup situation.

/// Wire the commit-msg hook under a policy that blocks the policy
/// characters (the level this repository uses; the shipped default only
/// warns), so a scanned dash refuses the commit.
#[cfg(unix)]
fn wire_commit_msg_hook(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    write_policy(
        dir,
        r#"{"schema_version":1,"git":{"policy_characters":"block"}}"#,
    );
    let hook = dir.join(".git/hooks/commit-msg");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nexec '{}' git-hook commit-msg \"$@\"\n",
            env!("CARGO_BIN_EXE_codeflow")
        ),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// An editor that puts `text` above Git's template, keeping every hint line.
#[cfg(unix)]
fn prepending_editor(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let body = dir.join(format!("{name}.txt"));
    std::fs::write(&body, text).unwrap();
    let editor = dir.join(format!("{name}.sh"));
    std::fs::write(
        &editor,
        format!(
            "#!/bin/sh\ncat '{}' \"$1\" > \"$1.new\" && mv \"$1.new\" \"$1\"\n",
            body.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
    editor
}

/// Stage a fresh file holding an em dash, then run `git <args>` with the
/// hook wired. `editor: None` is a non-editor commit.
#[cfg(unix)]
fn wired_commit(dir: &Path, args: &[&str], editor: Option<&Path>) -> Output {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let file = format!("staged-{n}.txt");
    std::fs::write(dir.join(&file), "range 1\u{2014}3\n").unwrap();
    git(dir, &["add", &file]);
    let out = run_wired_git(dir, args, editor);
    if !out.status.success() {
        git(dir, &["reset", "-q", "--", &file]);
    }
    out
}

/// Run `git <args>` with the hook wired, in the C locale so Git writes its
/// untranslated cut signature.
#[cfg(unix)]
fn run_wired_git(dir: &Path, args: &[&str], editor: Option<&Path>) -> Output {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("CODEFLOW_HOME", isolated_home())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE");
    match editor {
        Some(e) => cmd.env("GIT_EDITOR", e),
        None => cmd.env_remove("GIT_EDITOR"),
    };
    cmd.output().unwrap()
}

#[cfg(unix)]
fn assert_policy_character_block(out: &Output, case: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{case}: commit must be refused");
    assert!(stderr.contains("git.policy_characters"), "{case}: {stderr}");
}

/// Assert the commit landed and return its stored message.
#[cfg(unix)]
fn assert_committed(dir: &Path, out: &Output, case: &str) -> String {
    assert!(
        out.status.success(),
        "{case}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let log = Command::new("git")
        .args(["log", "-1", "--format=%B"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&log.stdout).into_owned()
}

#[cfg(unix)]
#[test]
fn wired_commit_msg_scans_hash_lines_git_keeps_without_editor() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    wire_commit_msg_hook(dir.path());
    let msg = "feat: add ranges\n\n# pages 1\u{2014}3\n";

    // `-m` cleans whitespace only: Git would record the `#` line.
    let out = wired_commit(dir.path(), &["commit", "-m", msg], None);
    assert_policy_character_block(&out, "-m default");
    // The same for `-F` and for each non-stripping cleanup config.
    let file = dir.path().join("MSG");
    std::fs::write(&file, msg).unwrap();
    let out = wired_commit(dir.path(), &["commit", "-F", file.to_str().unwrap()], None);
    assert_policy_character_block(&out, "-F default");
    for mode in ["whitespace", "verbatim", "scissors"] {
        let config = format!("commit.cleanup={mode}");
        let out = wired_commit(dir.path(), &["-c", &config, "commit", "-m", msg], None);
        assert_policy_character_block(&out, &config);
    }

    // commit.cleanup=strip really drops the line, so the hook allows it.
    let out = wired_commit(
        dir.path(),
        &["-c", "commit.cleanup=strip", "commit", "-m", msg],
        None,
    );
    let stored = assert_committed(dir.path(), &out, "commit.cleanup=strip");
    assert_eq!("feat: add ranges\n\n", stored);
}

// Operator direction 2026-09-25 (ADR-0067 note): without a policy file the
// built-in level warns, so the dash is reported and the commit lands.
#[cfg(unix)]
#[test]
fn wired_commit_msg_default_level_warns_and_commits() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    wire_commit_msg_hook(dir.path());
    std::fs::remove_file(dir.path().join(".codeflow/policy.json")).unwrap();
    let out = wired_commit(
        dir.path(),
        &["commit", "-m", "feat: add ranges\n\n- pages 1\u{2013}3\n"],
        None,
    );
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let stored = assert_committed(dir.path(), &out, "default warn");
    assert!(stored.contains("pages 1\u{2013}3"), "{stored}");
    assert!(stderr.contains("git.policy_characters"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn wired_commit_msg_scans_scissors_text_git_keeps() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    wire_commit_msg_hook(dir.path());
    let cut = "# ------------------------ >8 ------------------------";
    let msg = format!("feat: add ranges\n\n{cut}\n- pages 1\u{2013}3\n");

    // Without an editor or verbose mode, Git keeps the cut line and below.
    let out = wired_commit(dir.path(), &["commit", "-m", &msg], None);
    assert_policy_character_block(&out, "-m exact cut line");
    let loose = "feat: add ranges\n\n# ---- >8 ----\n- pages 1\u{2013}3\n";
    let out = wired_commit(dir.path(), &["commit", "-m", loose], None);
    assert_policy_character_block(&out, "-m loose scissors");

    // Codex round 3, N2: an edited commit with no verbose flag strips the
    // bare cut line as a comment and keeps the bullet below it.
    let editor = prepending_editor(dir.path(), "bare-cut", &msg);
    let out = wired_commit(dir.path(), &["commit"], Some(&editor));
    assert_policy_character_block(&out, "editor, bare exact cut line");

    // Git truncates here, but commit.verbose is not proof for every hook
    // run (a direct merge ignores it), so the hook scans conservatively.
    let out = wired_commit(
        dir.path(),
        &["-c", "commit.verbose=true", "commit", "-m", &msg],
        None,
    );
    assert_policy_character_block(&out, "-m, commit.verbose");
}

// Codex round 3, N2: a direct `git merge` cleans up with verbose off even
// when commit.verbose is set, so a cut line and what follows are kept.
#[cfg(unix)]
#[test]
fn wired_commit_msg_scans_direct_merge_text_despite_commit_verbose() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    git(dir.path(), &["checkout", "-q", "-b", "feat/side"]);
    std::fs::write(dir.path().join("side.txt"), "side\n").unwrap();
    git(dir.path(), &["add", "side.txt"]);
    git(dir.path(), &["commit", "-q", "-m", "feat: side"]);
    git(dir.path(), &["checkout", "-q", "feat/x"]);
    std::fs::write(dir.path().join("main.txt"), "main\n").unwrap();
    git(dir.path(), &["add", "main.txt"]);
    git(dir.path(), &["commit", "-q", "-m", "feat: main"]);
    wire_commit_msg_hook(dir.path());

    let cut = "# ------------------------ >8 ------------------------";
    let msg = format!("Merge feat/side\n\n{cut}\n- pages 1\u{2013}3\n");
    let out = run_wired_git(
        dir.path(),
        &[
            "-c",
            "commit.verbose=true",
            "merge",
            "--no-ff",
            "--no-edit",
            "feat/side",
            "-m",
            &msg,
        ],
        None,
    );
    assert_policy_character_block(&out, "direct merge, commit.verbose");
    git(dir.path(), &["merge", "--abort"]);

    // Control: the same merge without the dash lands with the text kept.
    let clean = format!("Merge feat/side\n\n{cut}\n- pages 1 to 3\n");
    let out = run_wired_git(
        dir.path(),
        &[
            "-c",
            "commit.verbose=true",
            "merge",
            "--no-ff",
            "--no-edit",
            "feat/side",
            "-m",
            &clean,
        ],
        None,
    );
    let stored = assert_committed(dir.path(), &out, "direct merge control");
    assert!(stored.contains("- pages 1 to 3"), "{stored}");
}

#[cfg(unix)]
#[test]
fn wired_commit_msg_scans_hash_lines_an_editor_session_keeps() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    wire_commit_msg_hook(dir.path());
    let editor = prepending_editor(
        dir.path(),
        "kept",
        "feat: edited\n\n# local note \u{2014} kept\n",
    );

    // A non-stripping cleanup keeps the edited `#` line.
    let out = wired_commit(
        dir.path(),
        &["-c", "commit.cleanup=whitespace", "commit"],
        Some(&editor),
    );
    assert_policy_character_block(&out, "editor, commit.cleanup=whitespace");
    // With `;` as the comment prefix, Git keeps `#` lines.
    let out = wired_commit(
        dir.path(),
        &["-c", "core.commentChar=;", "commit"],
        Some(&editor),
    );
    assert_policy_character_block(&out, "editor, core.commentChar=;");
}

// Codex round 3, N3: `core.commentString` is used byte for byte. With `# `,
// Git keeps `#kept` and drops only lines starting `# `.
#[cfg(unix)]
#[test]
fn wired_commit_msg_uses_the_exact_comment_string() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    git(dir.path(), &["config", "core.commentString", "# "]);
    wire_commit_msg_hook(dir.path());

    let kept = prepending_editor(dir.path(), "hash-kept", "feat: x\n\n#kept \u{2014} line\n");
    let out = wired_commit(dir.path(), &["commit"], Some(&kept));
    assert_policy_character_block(&out, "commentString `# `, #kept line");

    let dropped = prepending_editor(
        dir.path(),
        "hash-space",
        "feat: x\n\n# real comment \u{2014} dropped\n",
    );
    let out = wired_commit(dir.path(), &["commit"], Some(&dropped));
    let stored = assert_committed(dir.path(), &out, "commentString `# `, comment");
    assert_eq!("feat: x\n\n", stored);
}

#[cfg(unix)]
#[test]
fn wired_commit_msg_allows_edited_template_comments_and_diff_preview() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    wire_commit_msg_hook(dir.path());

    // Default strip cleanup: the `#` note, Git's hints and the verbose diff
    // preview (the staged file holds an em dash) are all dropped by Git.
    let editor = prepending_editor(
        dir.path(),
        "dropped",
        "feat: edited\n\n# local note \u{2014} dropped\n",
    );
    let out = wired_commit(dir.path(), &["commit", "-v"], Some(&editor));
    let stored = assert_committed(dir.path(), &out, "editor -v");
    assert_eq!("feat: edited\n\n", stored);

    // Scissors cleanup keeps `#` lines but cuts the verbose preview.
    let editor = prepending_editor(dir.path(), "scissors", "feat: scissors\n");
    let out = wired_commit(
        dir.path(),
        &["-c", "commit.cleanup=scissors", "commit", "-v"],
        Some(&editor),
    );
    let stored = assert_committed(dir.path(), &out, "editor scissors -v");
    assert!(stored.starts_with("feat: scissors\n"), "{stored}");
    assert!(!stored.contains(">8"), "{stored}");
}

// ---------------------------------------------------------------------------
// orient + session hooks
// ---------------------------------------------------------------------------

#[test]
fn orient_prints_digest_within_budget() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(
        dir.path().join("docs/product.md"),
        "# demo\n\n## Purpose\n\nDemo project for orient.\n",
    )
    .unwrap();

    let out = codeflow()
        .args(["orient"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("# orient —"), "{stdout}");
    assert!(stdout.contains("branch: feat/x"), "{stdout}");
    assert!(stdout.lines().count() <= 30, "{stdout}");
}

#[test]
fn hook_session_orient_emits_same_digest_to_stdout() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");

    let out = run_with_stdin(
        codeflow()
            .args(["hook", "session-orient"])
            .current_dir(dir.path()),
        r#"{"session_id":"s1","hook_event_name":"SessionStart","source":"startup"}"#,
    );
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("# orient —"), "{stdout}");
    assert!(stdout.contains("gates:"), "{stdout}");
}

#[test]
fn hook_session_summary_appends_ledger_record() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");

    let out = run_with_stdin(
        codeflow()
            .args(["hook", "session-summary"])
            .current_dir(dir.path()),
        r#"{"session_id":"cli-test","reason":"exit","hook_event_name":"SessionEnd"}"#,
    );
    assert_eq!(out.status.code(), Some(0));

    let ledger = dir
        .path()
        .join(".git/codeflow/ledger/sessions/sessions-ses-cli-test.jsonl");
    let line = std::fs::read_to_string(&ledger).expect("session record written");
    assert!(line.contains("\"event\":\"session_end\""), "{line}");
    assert!(line.contains("feat/x"), "{line}");
}

#[test]
fn hook_session_summary_never_fails_outside_repo() {
    let dir = tempfile::tempdir().unwrap();
    let out = run_with_stdin(
        codeflow()
            .args(["hook", "session-summary"])
            .current_dir(dir.path()),
        "{}",
    );
    assert_eq!(out.status.code(), Some(0), "must never fail the session");
    assert!(String::from_utf8_lossy(&out.stderr).contains("warning"));
}

// ---------------------------------------------------------------------------
// bootstrap grace
// ---------------------------------------------------------------------------

#[test]
fn bootstrap_grace_is_inert_after_first_commit() {
    // ADR-0009: policy_armed=false is not a persistent, agent-flippable
    // off-switch. `init_repo` already made a commit, so a disarmed flag written
    // afterwards is ignored — protected-branch rules stay armed.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    let cf = dir.path().join(".codeflow");
    std::fs::create_dir_all(&cf).unwrap();
    std::fs::write(cf.join("project.toml"), "policy_armed = false\n").unwrap();

    // Protected-branch commit: still BLOCKED despite policy_armed=false.
    std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
    git(dir.path(), &["add", "a.txt"]);
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "disarm flag is inert once committed"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("git.commit_to_protected"));
}

#[test]
fn bootstrap_grace_applies_before_first_commit() {
    // Grace still serves its real purpose: in the genuine pre-scaffold-commit
    // window (an unborn HEAD, no commit yet), policy_armed=false suspends the
    // branch rules so the first commit is not walled — but secrets are never
    // graced.
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "t@example.com"]);
    git(dir.path(), &["config", "user.name", "t"]);
    let cf = dir.path().join(".codeflow");
    std::fs::create_dir_all(&cf).unwrap();
    std::fs::write(cf.join("project.toml"), "policy_armed = false\n").unwrap();

    // Protected-branch commit (no prior commit): allowed under grace.
    std::fs::write(dir.path().join("f.txt"), "x\n").unwrap();
    git(dir.path(), &["add", "f.txt"]);
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "grace suspends branch rules pre-commit"
    );

    // Staged secret: still blocked under grace. The fixture is assembled at
    // runtime so this source file itself carries no contiguous key literal.
    let fake_key = format!("{}IOSFODNN7EXAMPLF\n", "AKIA");
    std::fs::write(dir.path().join("leak.txt"), fake_key).unwrap();
    git(dir.path(), &["add", "leak.txt"]);
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(1), "secrets are never graced");
    assert!(String::from_utf8_lossy(&out.stderr).contains("git.secret_scan"));
}

#[test]
fn pre_commit_blocks_implementation_when_task_exists_only_on_task_branch() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    git(dir.path(), &["switch", "-c", "task/TSK-001-unanchored"]);
    let task_dir = dir.path().join("project-management/tasks");
    std::fs::create_dir_all(&task_dir).unwrap();
    std::fs::write(
        task_dir.join("TSK-001.md"),
        "---\nid: TSK-001\nepic_id: null\nstandalone_reason: branch-only task\nintegration_target: main\ntitle: unanchored\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n\n## Description\nBranch-only planning must not authorize itself.\n\n## Acceptance Criteria\n- AC-1 planning is anchored\n",
    )
    .unwrap();
    git(dir.path(), &["add", "project-management"]);
    git(dir.path(), &["commit", "-m", "plan: add branch-only task"]);
    std::fs::write(dir.path().join("implementation.rs"), "fn work() {}\n").unwrap();
    git(dir.path(), &["add", "implementation.rs"]);

    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("task implementation is not ready"), "{err}");
    assert!(err.contains("not present at the merge-base"), "{err}");
}

#[test]
fn pre_commit_blocks_planning_records_created_on_a_task_branch() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    git(dir.path(), &["switch", "-c", "task/TSK-001-self-plan"]);
    let task_dir = dir.path().join("project-management/tasks");
    std::fs::create_dir_all(&task_dir).unwrap();
    std::fs::write(
        task_dir.join("TSK-001.md"),
        "---\nid: TSK-001\nepic_id: null\nstandalone_reason: branch-only task\nintegration_target: main\ntitle: self planning\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n\n## Description\nPlanning belongs on plan branches.\n\n## Acceptance Criteria\n- AC-1 planning is anchored\n",
    )
    .unwrap();
    git(dir.path(), &["add", "project-management"]);

    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not present at the merge-base"), "{err}");
}

#[test]
fn pre_commit_blocks_an_invalid_visible_workgraph_before_task_work() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    let task_dir = dir.path().join("project-management/tasks");
    std::fs::create_dir_all(&task_dir).unwrap();
    std::fs::write(
        task_dir.join("TSK-001.md"),
        "---\nid: TSK-001\nepic_id: null\nstandalone_reason: bounded repair\nintegration_target: main\ntitle: repair\nstatus: todo\nwork_type: fix\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n\n## Description\nRepair the implementation.\n\n## Acceptance Criteria\n- AC-1 repair verified\n",
    )
    .unwrap();
    git(dir.path(), &["add", "project-management"]);
    git(dir.path(), &["commit", "-m", "plan: anchor repair task"]);
    git(dir.path(), &["switch", "-c", "task/TSK-001-repair"]);

    let epic_dir = dir.path().join("project-management/epics");
    std::fs::create_dir_all(&epic_dir).unwrap();
    std::fs::write(
        epic_dir.join("EPC-999.md"),
        "---\nid: EPC-998\ntitle: mismatch\nstatus: planning\nwork_type: feat\ncreated: 2026-07-29\n---\n\n## Summary\nMismatch.\n\n## Acceptance Criteria\n- AC-1 fixed\n",
    )
    .unwrap();
    git(dir.path(), &["add", "project-management"]);

    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("workgraph error"), "{err}");
    assert!(err.contains("EPC-999.md"), "{err}");
}

#[test]
fn pre_commit_blocks_task_branch_without_a_visible_task_record() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    let state_dir = dir.path().join(".codeflow");
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::write(
        state_dir.join("project.toml"),
        "schema_version = 1\ntier = 'full'\nscaffold_version = '3.0.0'\nstack = 'rust'\nareas = []\npolicy_armed = true\ngit_hooks = 'wired'\npermission_preset = 'strict'\n",
    )
    .unwrap();
    git(dir.path(), &["switch", "-c", "task/TSK-001-missing-record"]);
    std::fs::write(dir.path().join("implementation.rs"), "fn work() {}\n").unwrap();
    git(dir.path(), &["add", "implementation.rs"]);

    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("does not identify a visible durable task record"),
        "{err}"
    );
    assert!(err.contains("before implementation"), "{err}");
}

#[test]
fn pre_commit_keeps_task_prefix_available_without_durable_work_tracking() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    let foreign_tasks = dir.path().join("project-management/tasks");
    std::fs::create_dir_all(&foreign_tasks).unwrap();
    std::fs::write(foreign_tasks.join("notes.md"), "External tracker notes").unwrap();
    git(dir.path(), &["switch", "-c", "task/tidy-the-logger"]);
    std::fs::write(dir.path().join("implementation.rs"), "fn work() {}\n").unwrap();
    git(dir.path(), &["add", "implementation.rs"]);

    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "minimal and standard tiers must not require full-tier task records: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn pre_commit_blocks_indeterminate_state_without_a_task_directory() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    let state_dir = dir.path().join(".codeflow");
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::write(state_dir.join("project.toml"), "tier = [invalid").unwrap();
    git(dir.path(), &["switch", "-c", "task/TSK-001-repair"]);
    std::fs::write(dir.path().join("implementation.rs"), "fn work() {}\n").unwrap();
    git(dir.path(), &["add", "implementation.rs"]);

    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("cannot determine durable-work tracking"),
        "{err}"
    );
    assert!(err.contains("cannot read existing CodeFlow state"), "{err}");
    assert!(
        !err.contains("[invalid"),
        "state contents must not be echoed: {err}"
    );
}

#[cfg(unix)]
#[test]
fn unreadable_state_blocks_both_hook_and_ci() {
    use std::io::ErrorKind;

    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    let state_dir = dir.path().join(".codeflow");
    std::fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("project.toml");
    std::fs::write(
        &state_path,
        r#"schema_version = 1
tier = "minimal"
scaffold_version = "3.0.0"
stack = "rust"
areas = []
policy_armed = true
git_hooks = "wired"
permission_preset = "strict"
"#,
    )
    .unwrap();
    git(
        dir.path(),
        &["switch", "-c", "task/TSK-001-unreadable-state"],
    );
    std::fs::write(dir.path().join("implementation.rs"), "fn work() {}\n").unwrap();
    git(dir.path(), &["add", "implementation.rs"]);

    let denied_state = RestorePermissions::deny(&state_path);
    match std::fs::read(&state_path) {
        Ok(_) => {
            eprintln!("EACCES state probe unavailable under this test identity");
            return;
        }
        Err(error) => assert_eq!(error.kind(), ErrorKind::PermissionDenied),
    }
    let hook = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(hook.status.code(), Some(1));
    let hook_error = String::from_utf8_lossy(&hook.stderr);
    assert!(
        hook_error.contains("cannot read existing CodeFlow state"),
        "{hook_error}"
    );

    git(dir.path(), &["commit", "-m", "feat: add implementation"]);
    let ci = task_branch_ci(dir.path(), "task/TSK-001-unreadable-state", false);
    assert_eq!(ci.status.code(), Some(1));
    let ci_error = String::from_utf8_lossy(&ci.stderr);
    assert!(ci_error.contains("work.tracking_state"), "{ci_error}");
    assert!(
        ci_error.contains("cannot read existing CodeFlow state"),
        "{ci_error}"
    );
    drop(denied_state);

    std::fs::write(dir.path().join("control.rs"), "fn control() {}\n").unwrap();
    git(dir.path(), &["add", "control.rs"]);
    let restored_hook = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(
        restored_hook.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&restored_hook.stderr)
    );
    let restored_ci = task_branch_ci(dir.path(), "task/TSK-001-unreadable-state", true);
    assert_eq!(
        restored_ci.status.code(),
        Some(0),
        "{}{}",
        String::from_utf8_lossy(&restored_ci.stdout),
        String::from_utf8_lossy(&restored_ci.stderr)
    );
}

#[cfg(unix)]
#[test]
fn unreadable_task_home_blocks_both_hook_and_ci() {
    use std::io::ErrorKind;

    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    git(
        dir.path(),
        &["switch", "-c", "task/TSK-001-unreadable-tasks"],
    );
    let tasks = dir.path().join("project-management/tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    std::fs::write(dir.path().join("implementation.rs"), "fn work() {}\n").unwrap();
    git(dir.path(), &["add", "implementation.rs"]);

    let denied_tasks = RestorePermissions::deny(&tasks);
    match std::fs::read_dir(&tasks) {
        Ok(_) => {
            eprintln!("EACCES task-home probe unavailable under this test identity");
            return;
        }
        Err(error) => assert_eq!(error.kind(), ErrorKind::PermissionDenied),
    }
    let hook = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(hook.status.code(), Some(1));
    let hook_error = String::from_utf8_lossy(&hook.stderr);
    assert!(
        hook_error.contains("cannot inspect CodeFlow task-home inventory"),
        "{hook_error}"
    );

    git(dir.path(), &["commit", "-m", "feat: add implementation"]);
    let ci = task_branch_ci(dir.path(), "task/TSK-001-unreadable-tasks", false);
    assert_eq!(ci.status.code(), Some(1));
    let ci_error = String::from_utf8_lossy(&ci.stderr);
    assert!(ci_error.contains("work.tracking_state"), "{ci_error}");
    assert!(
        ci_error.contains("cannot inspect CodeFlow task-home inventory"),
        "{ci_error}"
    );
    drop(denied_tasks);

    std::fs::write(dir.path().join("control.rs"), "fn control() {}\n").unwrap();
    git(dir.path(), &["add", "control.rs"]);
    let restored_hook = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(
        restored_hook.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&restored_hook.stderr)
    );
    let restored_ci = task_branch_ci(dir.path(), "task/TSK-001-unreadable-tasks", true);
    assert_eq!(
        restored_ci.status.code(),
        Some(0),
        "{}{}",
        String::from_utf8_lossy(&restored_ci.stdout),
        String::from_utf8_lossy(&restored_ci.stderr)
    );
}

#[test]
fn pre_commit_recognizes_nested_only_historical_task() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    git(dir.path(), &["switch", "-c", "task/TSK-001-001-unanchored"]);
    let nested = dir.path().join("project-management/epics/EPC-001/tasks");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        nested.join("TSK-001-001.md"),
        "---\nid: TSK-001-001\nepic_id: null\nstandalone_reason: historical task\nintegration_target: main\ntitle: historical\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n\n## Description\nNested historical task.\n\n## Acceptance Criteria\n- AC-1 anchored first\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("implementation.rs"), "fn work() {}\n").unwrap();
    git(dir.path(), &["add", "."]);

    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not present at the merge-base"), "{err}");
}

#[test]
fn task_home_inventory_limit_blocks_both_hook_and_ci() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    git(dir.path(), &["switch", "-c", "task/TSK-001-overflow"]);
    let tasks = dir.path().join("project-management/tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    for index in 0..=16_384 {
        std::fs::write(tasks.join(format!("foreign-{index:05}.txt")), "").unwrap();
    }
    std::fs::write(dir.path().join("implementation.rs"), "fn work() {}\n").unwrap();
    git(dir.path(), &["add", "implementation.rs"]);

    let hook = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    assert_eq!(hook.status.code(), Some(1));
    let hook_error = String::from_utf8_lossy(&hook.stderr);
    assert!(
        hook_error.contains("inventory exceeds 16384"),
        "{hook_error}"
    );

    git(dir.path(), &["commit", "-m", "feat: add implementation"]);
    let ci = codeflow()
        .args([
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-001-overflow",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(ci.status.code(), Some(1));
    let ci_error = String::from_utf8_lossy(&ci.stderr);
    assert!(ci_error.contains("work.tracking_state"), "{ci_error}");
    assert!(ci_error.contains("inventory exceeds 16384"), "{ci_error}");
}
