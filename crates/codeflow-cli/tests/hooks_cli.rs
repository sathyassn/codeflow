//! End-to-end tests for the hook plane CLI surface:
//! `codeflow hook <git-guard|session-orient|prompt-reminder|session-summary>`,
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
            "## Summary\nBounded task.\n\n- one change\n\nTask: TSK-001\n\n## Changes\n- implementation\n\n## Testing\n- focused test\nNot tested: Windows.\n\n## Reviews\nNone: pending review.\n\n## Release impact\n- Impact: patch\n- Breaking: no\n- Rationale: Preserve the public contract.\n- Migration: none",
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

fn write_agent_policy(dir: &Path, json: &str) {
    write_policy(dir, json);
    git(dir, &["add", ".codeflow/policy.json"]);
    git(
        dir,
        &["commit", "--allow-empty", "-m", "chore: fixture policy"],
    );
}

/// The protected-branch policy of the retargeting fixtures. Their scratch
/// repositories commit at a root checkout on a feature branch on purpose,
/// to probe protected-branch targeting, so the root-checkout rule
/// (TSK-165, judged in its own tests) is off.
const TARGETING_POLICY: &str =
    r#"{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off"}}"#;

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

/// `path` as a bare word in a Bash command. Bash removes an unquoted
/// backslash, so on Windows the word uses `/`, which git and Git Bash both
/// read as the separator.
fn shell_path(path: &Path) -> String {
    codeflow_core::portable_path::slashed(path)
}

/// Run `codeflow hook git-guard` on a Claude Code `PreToolUse` payload whose
/// session cwd is `session`.
fn guard_run(command: &str, session: &Path) -> Output {
    run_with_stdin(
        codeflow().args(["hook", "git-guard"]).current_dir(session),
        &guard_payload(command, session),
    )
}

// TSK-112 AC-4: the harness payload, the real binary and real repositories.
// The session repository is on `main`; a scratch repository with its own
// policy is on a feature branch; another has no policy and sits on `main`.
#[test]
fn git_guard_judges_the_repository_a_command_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let session = tmp.path().join("session");
    let scratch = tmp.path().join("scratch");
    let bare = tmp.path().join("nopolicy");
    for (dir, branch) in [(&session, "main"), (&scratch, "feat/x"), (&bare, "main")] {
        std::fs::create_dir_all(dir).unwrap();
        init_repo(dir, branch);
    }
    write_agent_policy(&session, TARGETING_POLICY);
    write_agent_policy(&scratch, TARGETING_POLICY);
    let s = shell_path(&scratch);
    let b = shell_path(&bare);
    let sg = shell_path(&session.join(".git"));

    for allowed in [
        format!("git -C {s} commit -m 'feat: x'"),
        format!("R={s}; git -C \"$R\" commit -m 'feat: x'"),
        format!(
            "cd {} && git -C scratch commit -m 'feat: x'",
            shell_path(tmp.path())
        ),
        // A double-quoted native path keeps its separators (TSK-197).
        format!("git -C \"{}\" commit -m 'feat: x'", scratch.display()),
    ] {
        let out = guard_run(&allowed, &session);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{allowed}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    for blocked in [
        "git commit -m 'feat: x'".to_string(),
        format!("git -C {b} commit -m 'feat: x'"),
        format!("git -C {s} --git-dir={sg} commit -m 'feat: x'"),
    ] {
        let out = guard_run(&blocked, &session);
        assert_eq!(out.status.code(), Some(2), "{blocked}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("git.commit_to_protected"),
            "{blocked}: {stderr}"
        );
    }

    let out = guard_run("git -C \"$UNSET_DIR\" commit -m 'feat: x'", &session);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("target unresolved: `$UNSET_DIR`")
            && stderr.contains("cannot prove it is not a protected branch"),
        "{stderr}"
    );
}

// TSK-112 review round 1 (Codex probe table), on real repositories. The
// session is on an unprotected branch, so every block below comes from the
// repository git would actually write to, which is on `main`.
#[test]
#[allow(clippy::too_many_lines)] // one fixture replaying the review probe tables
fn git_guard_blocks_targets_it_cannot_prove() {
    let tmp = tempfile::tempdir().unwrap();
    let session = tmp.path().join("session");
    let feature = tmp.path().join("feature");
    let protected = tmp.path().join("protected");
    for (dir, branch) in [
        (&session, "feat/s"),
        (&feature, "feat/x"),
        (&protected, "main"),
    ] {
        std::fs::create_dir_all(dir).unwrap();
        init_repo(dir, branch);
    }
    write_agent_policy(&session, TARGETING_POLICY);
    write_agent_policy(&feature, TARGETING_POLICY);
    // A directory literally named `$R` inside the session, a repository on main.
    let literal = session.join("$R");
    std::fs::create_dir_all(&literal).unwrap();
    init_repo(&literal, "main");
    let f = shell_path(&feature);
    let pg = shell_path(&protected.join(".git"));
    let absent = shell_path(&tmp.path().join("absent/out"));

    for (case, command) in [
        (
            "escaped dollar, quoted",
            format!("R={f}; git -C \"\\$R\" commit -m 'fix: p'"),
        ),
        (
            "escaped dollar, bare",
            format!("R={f}; git -C \\$R commit -m 'fix: p'"),
        ),
        (
            "env GIT_DIR",
            format!("R={f}; env GIT_DIR={pg} git -C \"$R\" commit -m 'fix: p'"),
        ),
        (
            "command env GIT_DIR",
            format!("R={f}; command env GIT_DIR={pg} git -C \"$R\" commit -m 'fix: p'"),
        ),
        (
            "unresolved variable",
            "git -C \"$DEST\" commit -m 'fix: p'".to_string(),
        ),
        (
            "subshell assignment",
            format!("(R={f}); git -C \"$R\" commit -m 'fix: p'"),
        ),
    ] {
        let out = guard_run(&command, &session);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{case}: {command}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // The failed `cd` probe needs a protected session: after `;` the commit
    // may run where the shell started.
    let main_session = tmp.path().join("main-session");
    std::fs::create_dir_all(&main_session).unwrap();
    init_repo(&main_session, "main");
    let failed_cd = format!("R={f}; cd \"$R\" > {absent}; git commit -m 'fix: p'");
    assert_eq!(guard_run(&failed_cd, &main_session).status.code(), Some(2));

    // Round 3 (R3-1): a substitution before the subcommand hides it. The
    // first three run from a protected session, the last from a feature one.
    let p = shell_path(&protected);
    for (command, from) in [
        (
            "git $(printf '') commit --allow-empty -m 'fix: p'".to_string(),
            &main_session,
        ),
        (
            "git `printf ''` commit --allow-empty -m 'fix: p'".to_string(),
            &main_session,
        ),
        (
            "git $(printf -- '--no-pager') commit --allow-empty -m 'fix: p'".to_string(),
            &main_session,
        ),
        (
            format!("git -C {p} $(printf '') commit --allow-empty -m 'fix: p'"),
            &session,
        ),
    ] {
        let out = guard_run(&command, from);
        assert_eq!(out.status.code(), Some(2), "{command}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("command unresolved"), "{command}: {stderr}");
    }

    // Round 2 (R2-1): a protected repository nested in the feature one,
    // reached through a substitution in the path.
    let nested = feature.join("protected");
    std::fs::create_dir_all(&nested).unwrap();
    init_repo(&nested, "main");
    for command in [
        format!("R={f}$(printf /protected); git -C \"$R\" commit -m 'fix: p'"),
        format!("R={f}`printf /protected`; git -C \"$R\" commit -m 'fix: p'"),
    ] {
        let out = guard_run(&command, &session);
        assert_eq!(out.status.code(), Some(2), "{command}");
    }

    // Controls: proven targets on a feature branch pass, including an
    // and-list continued on the next line (R2-2).
    for command in [
        format!("R={f}; git -C \"$R\" commit -m 'fix: p'"),
        format!("R={f}; cd \"$R\" && git commit -m 'fix: p'"),
        format!("cd {f} && git commit -m \"$(printf 'fix: p')\""),
        format!("cd {f} &&\ngit commit -m 'fix: p'"),
        format!("cd {f} && # continue\ngit commit -m 'fix: p'"),
    ] {
        let out = guard_run(&command, &main_session);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{command}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

// TSK-112 round 4 (R4-1): an unclassifiable argument never skips the
// protected-commit check. With `commit_to_protected: block`, a commit on main
// blocks at every `local_ref_protection` level, with or without a substitution.
#[test]
fn git_guard_uncertainty_keeps_the_protected_commit_check() {
    for local in ["off", "warn", "block"] {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path(), "main");
        write_agent_policy(
            tmp.path(),
            &format!(
                r#"{{"git":{{"protected_branches":["main","master"],"commit_to_protected":"block","local_ref_protection":"{local}"}}}}"#
            ),
        );
        for command in [
            "git commit --allow-empty -m x --author=\"$(printf 'X <x@example.com>')\"",
            "git commit $(printf '') --allow-empty -m x",
            "git commit --allow-empty -m x",
        ] {
            let out = guard_run(command, tmp.path());
            assert_eq!(
                out.status.code(),
                Some(2),
                "{local}: {command}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

// TSK-112 (primary ruling after round 4): an alias is read the way git reads
// it, in the repository the command targets, and its expansion is judged; a
// rebase's `<branch>` is the branch judged. Codex's round 4 reproductions.
#[test]
fn git_guard_resolves_aliases_and_rebase_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    let feat = tmp.path().join("feat");
    for (dir, branch) in [(&main, "main"), (&feat, "feat/x")] {
        std::fs::create_dir_all(dir).unwrap();
        init_repo(dir, branch);
        write_agent_policy(dir, TARGETING_POLICY);
    }
    let inc = tmp.path().join("aliases.ini");
    std::fs::write(&inc, "[alias]\n    x = commit\n").unwrap();
    let inc = inc.to_string_lossy();
    let m = main.to_string_lossy();

    let blocked = [
        "git -c alias.x=commit x --allow-empty -m \"$(printf x)\"".to_string(),
        "git -c alias.x=commit x $(printf '') --allow-empty -m x".to_string(),
        format!("git -c include.path={inc} x --allow-empty -m \"$(printf x)\""),
        format!("git -c include.path={inc} x $(printf '') --allow-empty -m x"),
    ];
    for command in &blocked {
        let out = guard_run(command, &main);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{command}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    // An alias in the repository's own config.
    git(&main, &["config", "alias.x", "commit"]);
    git(&main, &["config", "alias.st", "status"]);
    git(&feat, &["config", "alias.x", "commit"]);
    git(&feat, &["config", "alias.sh", "!git commit"]);
    for (command, from) in [
        ("git x --allow-empty -m \"$(printf x)\"".to_string(), &main),
        (format!("git -C {m} x --allow-empty -m x"), &feat),
        ("git sh -m x".to_string(), &feat),
        ("git rebase feat/y main".to_string(), &feat),
        ("git rebase --onto feat/z feat/y main".to_string(), &feat),
    ] {
        let out = guard_run(&command, from);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{command}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    // Controls: the same alias on a feature branch, a read-only alias, a
    // name that is no alias, and a rebase of the current branch.
    for (command, from) in [
        ("git x --allow-empty -m x", &feat),
        ("git st", &main),
        ("git frobnicate", &main),
        ("git rebase main", &feat),
    ] {
        let out = guard_run(command, from);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{command}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

// TSK-112 round 5: every row of Codex's boundary script
// (`tsk112-r5-boundaries.py`), on real repositories, plus failed moves
// after `;` and a newline and config reads. Each row gets a fresh repository
// on `main` with a `feat/x` branch, checked out on the branch given.
#[test]
#[allow(clippy::too_many_lines)]
fn git_guard_round_5_boundaries() {
    let tmp = tempfile::tempdir().unwrap();
    let mut n = 0;
    let mut fixture = |on: &str| {
        n += 1;
        let dir = tmp.path().join(format!("r{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        init_repo(&dir, "main");
        git(&dir, &["branch", "feat/x"]);
        if on != "main" {
            git(&dir, &["checkout", "-q", on]);
        }
        write_agent_policy(&dir, TARGETING_POLICY);
        dir
    };
    let commit = "commit --allow-empty -m \"fix: probe\"";
    let mut rows: Vec<(String, &str, i32)> = vec![
        // Codex rows, in script order.
        (
            "git -c alias.x=commit x --allow-empty -m \"fix: probe\"".to_string(),
            "main",
            2,
        ),
        // Git rejects an attached `-c`, so nothing runs.
        (
            "git -calias.x=commit x --allow-empty -m \"fix: probe\"".to_string(),
            "main",
            0,
        ),
        (
            "git checkout main && git x --allow-empty -m \"fix: probe\"".to_string(),
            "feat/x",
            2,
        ),
        (
            "git config alias.x commit; git x --allow-empty -m \"fix: probe\"".to_string(),
            "main",
            2,
        ),
        (
            "git config include.path ROOT/alias.ini; git x --allow-empty -m x".to_string(),
            "main",
            2,
        ),
        (
            format!("git rebase does-not-exist feat/x ; git {commit}"),
            "main",
            2,
        ),
        (
            format!("git rebase does-not-exist feat/x \n git {commit}"),
            "main",
            2,
        ),
        (
            format!("git rebase does-not-exist feat/x && git {commit}"),
            "main",
            0,
        ),
        ("git rebase main main".to_string(), "feat/x", 2),
        (
            "git rebase --onto feat/x main main".to_string(),
            "feat/x",
            2,
        ),
        ("git rebase --root main".to_string(), "feat/x", 2),
        (
            "git rebase --strategy recursive main main".to_string(),
            "feat/x",
            2,
        ),
        ("git rebase --exec true main main".to_string(), "feat/x", 2),
        (format!("git rebase main feat/x && git {commit}"), "main", 0),
        (
            "git rebase main \"$(printf main)\"".to_string(),
            "feat/x",
            2,
        ),
        ("git -c alias.x=status x --short".to_string(), "main", 0),
        (
            "git -c alias.x=y -c alias.y=commit x --allow-empty -m x".to_string(),
            "main",
            2,
        ),
        ("git -c alias.x='!git commit' x -m x".to_string(), "main", 2),
    ];
    rows.extend([
        // A failed checkout, and a rebase from an alias, after `;`.
        (
            format!("git checkout does-not-exist; git {commit}"),
            "main",
            2,
        ),
        (
            format!("git checkout does-not-exist\ngit {commit}"),
            "main",
            2,
        ),
        (
            format!("git -c alias.rb=rebase rb does-not-exist feat/x; git {commit}"),
            "main",
            2,
        ),
        // The same in a repository reached with `-C`.
        (
            format!("git -C ROOT rebase does-not-exist feat/x; git -C ROOT {commit}"),
            "main",
            2,
        ),
        (
            format!("git -C ROOT rebase main feat/x && git -C ROOT {commit}"),
            "main",
            0,
        ),
        // Config reads change nothing.
        (
            "git config --get user.name; git -c alias.x=status x".to_string(),
            "main",
            0,
        ),
        (
            "git config user.name && git -c alias.x=status x".to_string(),
            "main",
            0,
        ),
    ]);
    for (command, on, expected) in rows {
        let dir = fixture(on);
        std::fs::write(dir.join("alias.ini"), "[alias]\n x = commit\n").unwrap();
        // Codex's conditional row: an alias included only on `main`.
        if command.starts_with("git checkout main && git x") {
            let inc = dir.join("conditional.ini");
            std::fs::write(&inc, "[alias]\n x = commit\n").unwrap();
            git(
                &dir,
                &[
                    "config",
                    "includeIf.onbranch:main.path",
                    &inc.to_string_lossy(),
                ],
            );
        }
        let command = command.replace("ROOT", &shell_path(&dir));
        let out = guard_run(&command, &dir);
        assert_eq!(
            out.status.code(),
            Some(expected),
            "{command} (on {on}): {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
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
    write_agent_policy(
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

/// Run the exec-guard on `command` with `TMPDIR` set, from `repo`.
fn exec_guard_with_tmpdir(repo: &Path, tmpdir: &Path, command: &str) -> Output {
    run_with_stdin(
        codeflow()
            .args(["hook", "exec-guard"])
            .env("TMPDIR", tmpdir)
            .current_dir(repo),
        &guard_payload(command, repo),
    )
}

#[test]
fn exec_guard_allows_removal_below_temp_roots_and_blocks_system_paths() {
    // TSK-137 AC-5: an agent's scratch space sits below `/private` and
    // `/var` on macOS; removal there is allowed, the same command on a
    // system directory, a temp root itself or a path leading out of temp
    // space is still blocked.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    let scratch = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(scratch.path()).unwrap();
    let base = base.to_str().unwrap();
    let tmpdir = scratch.path().join("agent-tmp");
    std::fs::create_dir(&tmpdir).unwrap();
    let own = format!("{base}/agent-tmp");
    let link = format!("{base}/outside-link");
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc", &link).unwrap();
    let guard = |command: &str| exec_guard_with_tmpdir(dir.path(), &tmpdir, command);

    // Below the Unix temp roots. On native Windows these spellings name no
    // fixed place (Git Bash maps them below its install root, and a junction
    // can redirect them), so the guard refuses them as unresolved temp paths.
    let unix_temp = [
        "rm -rf /private/var/folders/ab/cd123/T/scratch".to_string(),
        "rm -rf /private/tmp/claude-501/work".to_string(),
        "rm -rf /var/tmp/cache && ls".to_string(),
    ];
    let mut allowed = Vec::new();
    // `$TMPDIR` below a Unix temp root; on native Windows the temp folder
    // lies below none, so these spellings name no exempt place there.
    if cfg!(unix) {
        allowed.extend(unix_temp.iter().cloned());
        allowed.extend([
            format!("rm -rf {own}/work"),
            format!("rm -rf \"{own}/quoted dir\""),
            format!("rm -rf {own}/sub/*"),
            format!("rm -rf {base}/sibling"),
        ]);
    }
    if cfg!(target_os = "macos") {
        allowed.push("rm -rf /var/folders/xy/zz9/T/build".to_string());
    }
    for command in &allowed {
        let out = guard(command);
        assert!(
            out.status.success(),
            "should allow: {command}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let mut blocked = vec![
        "rm -rf /etc/hosts".to_string(),
        "rm -rf /private/var/db/receipts".to_string(),
        "rm -rf /var/lib/dpkg".to_string(),
        // Temp roots themselves, the per-user one and globs over them.
        "rm -rf /tmp".to_string(),
        "rm -rf /tmp/".to_string(),
        "rm -rf /tmp/*".to_string(),
        "rm -rf /private/tmp".to_string(),
        "rm -rf /var/tmp".to_string(),
        "rm -rf /private/var/folders/ab/cd123/T".to_string(),
        // Lexical escapes.
        "rm -rf /private/tmp/claude-501/../../etc".to_string(),
        "rm -rf //private//tmp/../etc/hosts".to_string(),
        // Lookalikes of the per-user root.
        "rm -rf /private/var/folders/not-xx/id/T/cache".to_string(),
        "rm -rf /private/var/foldersXX/ab/id/T/cache".to_string(),
        "rm -rf /private/var/folders/ab/id/T-not/cache".to_string(),
    ];
    if cfg!(unix) {
        // The configured `$TMPDIR` itself, nested below `/private/tmp` or
        // the per-user root.
        blocked.extend([
            format!("rm -rf {own}"),
            format!("rm -rf {own}/"),
            format!("rm -rf {own}/*"),
        ]);
        // A link below temp space leading to `/etc`, however spelled.
        blocked.extend([
            format!("rm -rf {link}/hosts"),
            format!("rm -rf \"{link}/hosts\""),
            format!("rm -rf {link}/*"),
            format!("rm -rf {link}/../var/db"),
        ]);
    } else {
        blocked.extend(unix_temp.iter().cloned());
    }
    for command in &blocked {
        let out = guard(command);
        assert_eq!(out.status.code(), Some(2), "should block: {command}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("security.dangerous_commands"));
    }
}

/// A scratch directory below the shared temp root, and its spellings: the
/// canonical one and, where `/tmp` is a link to `/private/tmp`, the other.
#[cfg(unix)]
fn shared_temp_scratch() -> (tempfile::TempDir, Vec<String>) {
    let scratch = tempfile::Builder::new()
        .tempdir_in("/tmp")
        .or_else(|_| tempfile::tempdir())
        .unwrap();
    let canonical = std::fs::canonicalize(scratch.path()).unwrap();
    let canonical = canonical.to_str().unwrap().to_string();
    let mut spellings = vec![canonical.clone()];
    let alias = if let Some(rest) = canonical.strip_prefix("/private/tmp/") {
        Some(format!("/tmp/{rest}"))
    } else {
        canonical
            .strip_prefix("/tmp/")
            .map(|rest| format!("/private/tmp/{rest}"))
    };
    // Only a spelling that reaches the same directory is an alias: Linux has
    // no `/private/tmp`, so there the canonical spelling is the only one.
    if let Some(alias) = alias.filter(|alias| {
        std::fs::canonicalize(alias).is_ok_and(|path| path.to_str() == Some(canonical.as_str()))
    }) {
        spellings.push(alias);
    }
    (scratch, spellings)
}

#[cfg(unix)]
#[test]
fn exec_guard_refuses_unresolved_temp_paths_through_both_aliases() {
    // TSK-137 round 2 (T137-1a, T137-1b): an operand whose identity the
    // classifier cannot establish (a literal backslash in a quoted name) or
    // whose containment it cannot resolve (a glob before the last component)
    // is refused under temp space, through `/tmp` and `/private/tmp` alike.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    let (scratch, spellings) = shared_temp_scratch();
    std::os::unix::fs::symlink("/etc", scratch.path().join("odd\\link")).unwrap();
    std::os::unix::fs::symlink("/etc", scratch.path().join("glob-link")).unwrap();
    let guard = |command: &str| exec_guard_with_tmpdir(dir.path(), scratch.path(), command);

    for base in &spellings {
        for command in [
            format!("rm -rf '{base}/odd\\link/hosts'"),
            format!("rm -rf {base}/odd\\\\link/hosts"),
            format!("rm -rf {base}/glob-*/hosts"),
            format!("rm -rf {base}/glob-*/../var/db"),
            format!("rm -rf {base}/glob-link/hosts"),
            format!("rm -rf \"{base}/$SUB\"/x"),
        ] {
            let out = guard(&command);
            assert_eq!(out.status.code(), Some(2), "should block: {command}");
        }
        // Ordinary temp removal through the same spelling stays allowed.
        for command in [
            format!("rm -rf {base}/work"),
            format!("rm -rf '{base}/quoted dir'"),
            format!("rm -rf {base}/glob-*"),
        ] {
            let out = guard(&command);
            assert!(
                out.status.success(),
                "should allow: {command}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
    let out = guard("rm -rf /private/etc/hosts");
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn exec_guard_honours_no_system_directory_as_tmpdir() {
    // TSK-137 T137-2: `$TMPDIR` confers no exemption; a system directory
    // given as one leaves its descendants blocked.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    for (tmpdir, command) in [
        ("/private/etc", "rm -rf /private/etc/hosts"),
        ("/private/var", "rm -rf /private/var/db"),
        ("/usr/local", "rm -rf /usr/local/bin"),
        ("/usr", "rm -rf /usr/lib"),
        ("/", "rm -rf /etc/ssh"),
    ] {
        let out = exec_guard_with_tmpdir(dir.path(), Path::new(tmpdir), command);
        assert_eq!(out.status.code(), Some(2), "TMPDIR={tmpdir}: {command}");
    }
    // An alias spelling of a valid `$TMPDIR` still protects the directory.
    #[cfg(unix)]
    {
        let scratch = tempfile::tempdir().unwrap();
        let real = scratch.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let alias = scratch.path().join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let real = std::fs::canonicalize(&real).unwrap();
        let out = exec_guard_with_tmpdir(dir.path(), &alias, &format!("rm -rf {}", real.display()));
        assert_eq!(out.status.code(), Some(2), "alias TMPDIR root");
    }
}

/// Run git in `dir` with the hook environment, returning its output.
fn git_out(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .output()
        .unwrap()
}

#[test]
fn reference_transaction_lets_ref_packing_through_and_blocks_a_move() {
    // `git pack-refs` and `git gc` move protected refs between loose files
    // and packed-refs without changing what they point to; that is not an
    // update. A real move or deletion of `main` still blocks.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    git(
        dir.path(),
        &["commit", "--allow-empty", "-q", "-m", "chore: second"],
    );
    git(dir.path(), &["checkout", "-q", "-b", "other"]);
    let main = rev(dir.path(), "main");
    let loose = dir.path().join(".git/refs/heads/main");
    wire_reference_transaction_hook(dir.path());

    // A loose-only `main` cannot be deleted by naming its value.
    let out = git_out(dir.path(), &["update-ref", "-d", "refs/heads/main", &main]);
    assert!(!out.status.success(), "loose-only delete must block");

    let out = git_out(dir.path(), &["pack-refs", "--all"]);
    assert!(
        out.status.success(),
        "pack-refs: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(rev(dir.path(), "main"), main);
    assert!(!loose.exists(), "the loose copy was pruned");
    let packed = std::fs::read_to_string(dir.path().join(".git/packed-refs")).unwrap();
    assert!(
        packed.contains(&format!("{main} refs/heads/main")),
        "{packed}"
    );

    // A loose copy equal to the packed value, as before a prune, can be
    // pruned by `git gc`, but not deleted outright.
    std::fs::write(&loose, format!("{main}\n")).unwrap();
    let out = git_out(dir.path(), &["update-ref", "-d", "refs/heads/main", &main]);
    assert!(
        !out.status.success(),
        "delete with a packed copy must block"
    );
    let out = git_out(dir.path(), &["gc", "--quiet"]);
    assert!(
        out.status.success(),
        "gc: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(rev(dir.path(), "main"), main);
    assert!(!loose.exists(), "gc packed the loose copy");

    // Real changes to the packed `main` still block.
    for args in [
        &["update-ref", "refs/heads/main", "HEAD~1"][..],
        &["update-ref", "-d", "refs/heads/main", &main],
        &["branch", "-D", "main"],
    ] {
        let out = git_out(dir.path(), args);
        assert!(!out.status.success(), "{args:?} must block");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("git.local_ref_protection") || stderr.contains("git.delete_protected"),
            "{args:?}: {stderr}"
        );
    }
    assert_eq!(rev(dir.path(), "main"), main);
}

/// The TSK-136 review round 1 probes: commands that run a peer headless.
const HEADLESS_PROBES: &[&str] = &[
    "claude -p x",
    "codex exec x",
    "grok -p x",
    "'claude' -p x",
    "cl\"au\"de -p x",
    r"clau\de -p x",
    "$(printf claude) -p x",
    "/usr/local/bin/claude -p x",
    "alias peer='claude -p'\npeer x",
    "shopt -s expand_aliases\nalias peer='claude -p'\npeer x",
    "peer() { claude -p x; }; peer",
    "printf x | xargs claude -p",
    r"find . -maxdepth 0 -exec claude -p x \;",
    "timeout 5 claude -p x",
    "nice -n 1 claude -p x",
    "time claude -p x",
    "command claude -p x",
    "exec claude -p x",
    "bash -lc 'claude -p x'",
    "env bash -lc 'echo harmless; claude -p x'",
    "env bash -c 'echo harmless; claude -p x'",
    "bash <<< 'claude -p x'",
    "for x in one; do claude -p x; done",
    "while false; do claude -p x; done",
    "while true; do claude -p x; break; done",
    "if true; then claude -p x; fi",
    "(claude -p x)",
    "{ claude -p x; }",
    "claude -p <<< x",
    "claude -dp x",
    "grok -px",
    "grok --single x",
    "grok --single=x",
    "grok --prompt-file prompt.txt",
    "grok --prompt-json \"[]\"",
    "codex --model demo exec x",
    "codex --model=demo exec x",
    "codex -c model=\"demo\" exec x",
];

/// The same review's controls: commands that are not headless runs.
const HEADLESS_CONTROLS: &[&str] = &[
    // Prints help and exits; a headless run until TSK-141 AC-3.
    "claude --help -p",
    "codex exec --help",
    "claude --help",
    "codex login",
    "grok --version",
    "claude -- -p",
    "grok -- -p",
    "codex -- exec",
    "claude --system-prompt \"-p\"",
    "rg -p pattern",
    "cat -- -p",
    "git commit -m 'claude -p x'",
    "printf '%s' 'claude -p x'",
];

#[test]
fn exec_guard_classifies_every_review_probe() {
    // T136-1 to T136-3: shell syntax, wrappers, Grok's native forms and
    // argument roles, through the real hook at the block level.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    write_agent_policy(
        dir.path(),
        r#"{"security": {"headless_peer_runs": "block"}}"#,
    );
    let guard = |command: &str| {
        run_with_stdin(
            codeflow()
                .args(["hook", "exec-guard"])
                .current_dir(dir.path()),
            &guard_payload(command, dir.path()),
        )
    };
    for command in HEADLESS_PROBES {
        let out = guard(command);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "should block: {command}: {err}");
        assert!(
            err.contains("security.headless_peer_runs"),
            "{command}: {err}"
        );
    }
    for command in HEADLESS_CONTROLS {
        let out = guard(command);
        assert_eq!(out.status.code(), Some(0), "should allow: {command}");
        assert!(
            out.stderr.is_empty(),
            "{command}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn exec_guard_points_text_that_names_a_peer_to_a_file() {
    // TSK-223 AC-3 (sathyassn/codeflow#52): text that only mentions a peer
    // passes when it is written to a file and passed by path; the same text
    // inline on a line exec-guard cannot fully parse is still refused, and
    // the refusal names the file route.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    write_agent_policy(
        dir.path(),
        r#"{"security": {"headless_peer_runs": "block"}}"#,
    );
    std::fs::write(
        dir.path().join("msg.txt"),
        "docs: record the review\n\nNo codex exec run was used.\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("body.md"),
        "The Codex review approved; codex exec was not used.\n",
    )
    .unwrap();
    let guard = |command: &str| {
        run_with_stdin(
            codeflow()
                .args(["hook", "exec-guard"])
                .current_dir(dir.path()),
            &guard_payload(command, dir.path()),
        )
    };
    for command in [
        "git commit -F msg.txt",
        "gh pr create --body-file body.md",
        "gh api -X PATCH repos/o/r/pulls/1 -F body=@body.md",
    ] {
        let out = guard(command);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(0), "should allow: {command}: {err}");
        assert!(!err.contains("headless"), "{command}: {err}");
    }
    for command in [
        "grep -c review <<< 'Codex review: approve'",
        "$EDITOR notes.md; git commit -m 'docs: record the Codex review'",
    ] {
        let out = guard(command);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "should block: {command}: {err}");
        for part in [
            "security.headless_peer_runs",
            "could not be fully parsed",
            "`git commit -F <file>`",
            "`gh pr create --body-file <file>`",
            "-F body=@<file>",
            "flagged by design",
        ] {
            assert!(err.contains(part), "{command}: missing {part}: {err}");
        }
    }
}

#[test]
fn exec_guard_flags_headless_peer_runs_per_level() {
    // TSK-136 AC-1 as amended by ADR-0075 D4: each headless form is refused
    // by default and at block, warns with the rule and the interactive path
    // at warn, and nothing else is touched.
    let runs = [
        "claude -p 'review this'",
        "codex exec 'fix it'",
        "grok -p 'x'",
    ];
    let others = [
        "codex --version",
        "claude",
        "claude --model opus --effort high",
        "grok --version",
        "git commit -m 'no claude -p here'",
    ];
    for level in ["default", "warn", "block", "off"] {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        if level != "default" {
            write_agent_policy(
                dir.path(),
                &format!(r#"{{"security": {{"headless_peer_runs": "{level}"}}}}"#),
            );
        }
        let guard = |command: &str| {
            run_with_stdin(
                codeflow()
                    .args(["hook", "exec-guard"])
                    .current_dir(dir.path()),
                &guard_payload(command, dir.path()),
            )
        };
        for command in runs {
            let out = guard(command);
            let err = String::from_utf8_lossy(&out.stderr).to_string();
            match level {
                "block" | "default" => {
                    assert_eq!(out.status.code(), Some(2), "{level}: {command}: {err}");
                    assert!(err.contains("security.headless_peer_runs"), "{err}");
                }
                "off" => {
                    assert_eq!(out.status.code(), Some(0), "{level}: {command}");
                    assert!(!err.contains("headless"), "{err}");
                }
                _ => {
                    assert_eq!(out.status.code(), Some(0), "{level}: {command}: {err}");
                    assert!(err.contains("security.headless_peer_runs"), "{err}");
                    assert!(err.contains("codeflow delegate"), "{err}");
                }
            }
        }
        for command in others {
            let out = guard(command);
            let err = String::from_utf8_lossy(&out.stderr);
            assert_eq!(out.status.code(), Some(0), "{level}: {command}: {err}");
            assert!(!err.contains("headless"), "{level}: {command}: {err}");
        }
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
fn commit_msg_requires_a_blank_line_after_the_subject() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    let msg = dir.path().join("MSG");
    let run = |text: &str| {
        std::fs::write(&msg, text).unwrap();
        run_with_stdin(
            codeflow()
                .args(["git-hook", "commit-msg", msg.to_str().unwrap()])
                .current_dir(dir.path()),
            "",
        )
    };

    let out = run("feat(cli): wire the hook plane\n- keep the shim\n");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("git.commit_format"), "{stderr}");
    assert!(stderr.contains("must be blank"), "{stderr}");

    let out = run("feat(cli): wire the hook plane\n\n- keep the shim\n");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run("feat(cli): wire the hook plane\n");
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
    answering_destination(dir.path(), "origin");
    let url = push_url(dir.path(), "origin");
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-push", "origin", &url])
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

// ---------------------------------------------------------------------------
// rules re-injected after compaction and on prompt triggers (TSK-128)
// ---------------------------------------------------------------------------

const GUIDANCE_HEADING: &str = "## Rules after compaction or resume";

fn write_tier(dir: &Path, tier: &str) {
    let cf = dir.join(".codeflow");
    std::fs::create_dir_all(&cf).unwrap();
    std::fs::write(cf.join("project.toml"), format!("tier = \"{tier}\"\n")).unwrap();
}

/// The one advisory command both harnesses wire for both events.
fn session_orient(dir: &Path, payload: &str) -> Output {
    run_with_stdin(
        codeflow().args(["hook", "session-orient"]).current_dir(dir),
        payload,
    )
}

fn start_payload(source: &str) -> String {
    format!(r#"{{"session_id":"s1","hook_event_name":"SessionStart","source":"{source}"}}"#)
}

fn prompt_payload(prompt: &str) -> String {
    serde_json::json!({
        "session_id": "s1",
        "hook_event_name": "UserPromptSubmit",
        "prompt": prompt,
    })
    .to_string()
}

/// AC-1: compact, resume and fork add the guidance block after the
/// unchanged digest; startup and clear print today's digest only. The block
/// size is printed against its guideline, not capped.
#[test]
fn session_orient_adds_the_guidance_block_after_compact_resume_and_fork() {
    for tier in ["minimal", "standard", "full"] {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        write_tier(dir.path(), tier);
        let digest = codeflow()
            .args(["orient"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let digest = String::from_utf8(digest.stdout).unwrap();
        assert!(digest.contains("# orient"), "{digest}");

        for source in ["startup", "clear"] {
            let out = session_orient(dir.path(), &start_payload(source));
            assert_eq!(out.status.code(), Some(0));
            let stdout = String::from_utf8(out.stdout).unwrap();
            assert_eq!(stdout, digest, "{tier} {source}: the digest changed");
        }

        for source in ["compact", "resume", "fork"] {
            let out = session_orient(dir.path(), &start_payload(source));
            assert_eq!(out.status.code(), Some(0));
            let stdout = String::from_utf8(out.stdout).unwrap();
            let (head, block) = stdout
                .split_once(&format!("\n{GUIDANCE_HEADING}"))
                .unwrap_or_else(|| panic!("{tier} {source}: no guidance block\n{stdout}"));
            assert_eq!(head, digest, "{tier} {source}: the digest changed");
            let block = format!("{GUIDANCE_HEADING}{block}");
            assert!(block.contains(&format!("({tier} tier)")), "{block}");
            // The route rule leads the map at standard and full only.
            let always = if tier == "minimal" {
                "Always: Work to the outcome."
            } else {
                "Always: Route by touched paths. Work to the outcome."
            };
            assert!(block.contains(always), "{block}");
            assert!(
                block.contains("- give a duration, date or effort:"),
                "{block}"
            );
            if tier == "minimal" {
                assert!(block.contains("Skills: none at this tier."), "{block}");
            } else {
                assert!(block.contains("cf-estimate"), "{block}");
                assert!(block.contains("Agents: cf-reviewer"), "{block}");
            }
            println!(
                "guidance block {tier} {source}: {} bytes (guideline 1536)",
                block.len()
            );
        }
    }
}

/// AC-1: no tier (not a scaffolded project) and a garbled payload keep the
/// plain digest; an event-less payload is a session start, as before.
#[test]
fn session_orient_without_a_tier_or_payload_keeps_the_digest() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    let out = session_orient(dir.path(), &start_payload("compact"));
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("# orient"), "{stdout}");
    assert!(!stdout.contains(GUIDANCE_HEADING), "{stdout}");

    write_tier(dir.path(), "standard");
    for payload in ["", "not json", r#"{"source":7}"#, "{}"] {
        let out = session_orient(dir.path(), payload);
        assert_eq!(out.status.code(), Some(0), "{payload}");
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert!(stdout.contains("# orient"), "{payload}: {stdout}");
        assert!(!stdout.contains(GUIDANCE_HEADING), "{payload}: {stdout}");
    }
}

/// F1: the same wired command on `UserPromptSubmit` prints a reminder or
/// nothing, never the digest; any other named event is an advisory no-op.
#[test]
fn session_orient_dispatches_on_the_event() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    write_tier(dir.path(), "standard");
    let out = session_orient(dir.path(), &prompt_payload("How long will it take?"));
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("codeflow reminder: "), "{stdout}");
    assert!(!stdout.contains("# orient"), "{stdout}");

    let out = session_orient(dir.path(), &prompt_payload("Rename foo to bar"));
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());

    for event in ["PreCompact", "PostCompact", "Stop", "SomeFutureEvent"] {
        let out = session_orient(
            dir.path(),
            &format!(r#"{{"hook_event_name":"{event}","source":"compact"}}"#),
        );
        assert_eq!(out.status.code(), Some(0), "{event}");
        assert!(out.stdout.is_empty(), "{event}: advisory no-op");
        assert!(out.stderr.is_empty(), "{event}");
    }

    // The manual convenience command gives the same line.
    let manual = run_with_stdin(
        codeflow()
            .args(["hook", "prompt-reminder"])
            .current_dir(dir.path()),
        &prompt_payload("How long will it take?"),
    );
    assert_eq!(manual.status.code(), Some(0));
    assert_eq!(String::from_utf8(manual.stdout).unwrap(), stdout);
}

/// F1: a closed stdout (the harness stopped reading) never fails either
/// event.
#[test]
fn session_orient_survives_a_closed_stdout() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    write_tier(dir.path(), "full");
    for payload in [
        start_payload("startup"),
        start_payload("compact"),
        prompt_payload("What's the status of the release?"),
    ] {
        let mut child = codeflow()
            .args(["hook", "session-orient"])
            .current_dir(dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{payload}: {out:?}");
    }
}

/// AC-3: the fixture corpus through the wired command; a matching prompt
/// gets exactly one rule line, any other prompt gets nothing. Line sizes are
/// printed against the 300-byte guideline, not capped.
#[test]
fn prompt_reminder_follows_the_fixture_corpus() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/prompt-reminders.json")).unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let matching = cases.iter().filter(|c| !c["expect"].is_null()).count();
    assert!(matching >= 10, "{matching} matching prompts");
    assert!(cases.len() - matching >= 10, "too few non-matching prompts");

    for tier in ["minimal", "standard", "full"] {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        write_tier(dir.path(), tier);
        let mut longest = 0;
        for case in cases {
            let prompt = case["prompt"].as_str().unwrap();
            let out = session_orient(dir.path(), &prompt_payload(prompt));
            assert_eq!(out.status.code(), Some(0), "{prompt}");
            assert!(out.stderr.is_empty(), "{prompt}");
            let stdout = String::from_utf8(out.stdout).unwrap();
            let Some(expect) = case["expect"].as_str() else {
                assert_eq!(stdout, "", "{tier}: {prompt:?} should add nothing");
                continue;
            };
            assert_eq!(stdout.lines().count(), 1, "{tier} {prompt:?}: {stdout}");
            assert!(stdout.starts_with("codeflow reminder: "), "{stdout}");
            longest = longest.max(stdout.trim_end().len());
            let title = match expect {
                "duration" => "When you give a duration, date or effort:",
                "status" if stdout.contains("When you report status or hand off:") => {
                    "When you report status or hand off:"
                }
                "status" => "When you report status or summarize work:",
                "explanation" if stdout.contains("When you show something complex:") => {
                    "When you show something complex:"
                }
                "explanation" => "When you explain a flow, comparison, plan or decision:",
                other => panic!("unknown expectation {other}"),
            };
            assert!(
                stdout.contains(title),
                "{tier} {prompt:?}: expected {title}, got {stdout}"
            );
        }
        println!("reminder lines {tier}: longest {longest} bytes (guideline 300)");
    }
}

/// AC-4: advisory by default, `off` or `allow` by policy, never blocking; a
/// missing or malformed policy file means the default (warn), and a
/// missing project state or payload means no line, always exit 0.
#[test]
fn prompt_reminder_is_advisory_switchable_and_never_fails() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    let ask = prompt_payload("How long will the migration take?");

    // No project state at all: nothing, exit 0.
    let out = session_orient(dir.path(), &ask);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());

    // Tier but no policy file: the default level (warn) adds the line.
    write_tier(dir.path(), "standard");
    assert!(!dir.path().join(".codeflow/policy.json").exists());
    let out = session_orient(dir.path(), &ask);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("codeflow reminder: "));

    for (policy, expect_line) in [
        (r#"{"guidance": {"prompt_reminders": "off"}}"#, false),
        (r#"{"guidance": {"prompt_reminders": "allow"}}"#, false),
        (r#"{"guidance": {"prompt_reminders": "warn"}}"#, true),
        // Refused by validation, and still only advisory if written.
        (r#"{"guidance": {"prompt_reminders": "block"}}"#, true),
        // Malformed: the loader falls back to the defaults.
        ("{ not json", true),
    ] {
        write_policy(dir.path(), policy);
        let out = session_orient(dir.path(), &ask);
        assert_eq!(out.status.code(), Some(0), "{policy}");
        assert!(out.stderr.is_empty(), "{policy}");
        assert_eq!(!out.stdout.is_empty(), expect_line, "{policy}");
    }

    write_policy(dir.path(), r#"{"guidance": {"prompt_reminders": "block"}}"#);
    let out = codeflow()
        .args(["validate"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("guidance.prompt_reminders"), "{text}");

    write_policy(dir.path(), r#"{"guidance": {"prompt_reminders": "warn"}}"#);
    for payload in [
        r#"{"hook_event_name":"UserPromptSubmit"}"#,
        r#"{"hook_event_name":"UserPromptSubmit","prompt":7}"#,
    ] {
        let out = session_orient(dir.path(), payload);
        assert_eq!(out.status.code(), Some(0), "{payload}");
        assert!(out.stdout.is_empty(), "{payload}");
    }
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
    // No repository, no session to record: nothing to clear, so no warning
    // (TSK-147 review F5).
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("nothing to record"), "{stderr}");
    assert!(!stderr.contains("warning"), "{stderr}");
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
/// The planning anchor is checked once, at `work start` and in CI, never
/// per commit (the 2026-09-26 audit decision): pre-commit lets the commit
/// through, and `work start` names the missing anchor.
fn pre_commit_leaves_the_planning_anchor_to_work_start_and_ci() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    git(dir.path(), &["switch", "-c", "task/TSK-001-unanchored"]);
    let task_dir = dir.path().join("project-management/tasks");
    std::fs::create_dir_all(&task_dir).unwrap();
    std::fs::write(
        task_dir.join("TSK-001.md"),
        "---\nid: TSK-001\nepic_id: EPC-001\nstandalone_reason: null\nintegration_target: main\ntitle: unanchored\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n\n## Description\nBranch-only epic planning must not authorize itself.\n\n## Acceptance Criteria\n- AC-1 planning is anchored\n",
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
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let start = codeflow()
        .args(["work", "start", "TSK-001"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(start.status.code(), Some(1));
    let err = String::from_utf8_lossy(&start.stderr);
    assert!(err.contains("not present at the merge-base"), "{err}");
}

#[test]
/// TSK-133 AC-1: pre-commit does not re-validate the workgraph on each
/// commit. A task branch whose tree holds an unrelated graph defect (an
/// epic file whose id does not match its name) commits; CI reports the
/// defect once, as `work.valid_graph`.
fn pre_commit_ignores_an_unrelated_graph_defect_on_a_task_branch() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    let tasks = dir.path().join("project-management/tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    std::fs::write(
        tasks.join("TSK-001.md"),
        "---\nid: TSK-001\nepic_id: null\nstandalone_reason: bounded repair\nintegration_target: main\ntitle: repair\nstatus: todo\nwork_type: fix\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n\n## Description\nRepair.\n\n## Acceptance Criteria\n- AC-1 repair verified\n",
    )
    .unwrap();
    git(dir.path(), &["add", "project-management"]);
    git(dir.path(), &["commit", "-m", "chore: plan the repair"]);
    git(dir.path(), &["switch", "-c", "task/TSK-001-repair"]);
    let epics = dir.path().join("project-management/epics");
    std::fs::create_dir_all(&epics).unwrap();
    std::fs::write(
        epics.join("EPC-999.md"),
        "---\nid: EPC-998\ntitle: mismatch\nstatus: planning\nwork_type: feat\ncreated: 2026-07-29\n---\n\n## Summary\nMismatch.\n\n## Acceptance Criteria\n- AC-1 fixed\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("repair.rs"), "fn repair() {}\n").unwrap();
    git(dir.path(), &["add", "."]);

    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(dir.path()),
        "",
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{err}");
    assert!(!err.contains("work."), "{err}");
    git(dir.path(), &["commit", "-m", "fix: repair the work"]);

    let ci = codeflow()
        .args([
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            "task/TSK-001-repair",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&ci.stderr);
    assert_eq!(ci.status.code(), Some(1), "{err}");
    assert!(err.contains("work.valid_graph (block)"), "{err}");
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

#[cfg(unix)]
#[test]
fn unreadable_state_blocks_ci_not_the_commit() {
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
    assert_eq!(
        hook.status.code(),
        Some(0),
        "pre-commit leaves the tracking state to work start and CI: {}",
        String::from_utf8_lossy(&hook.stderr)
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
fn unreadable_task_home_blocks_ci_not_the_commit() {
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
    assert_eq!(
        hook.status.code(),
        Some(0),
        "pre-commit leaves the tracking state to work start and CI: {}",
        String::from_utf8_lossy(&hook.stderr)
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
fn task_home_inventory_limit_blocks_ci_not_the_commit() {
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
    assert_eq!(
        hook.status.code(),
        Some(0),
        "pre-commit leaves the tracking state to work start and CI: {}",
        String::from_utf8_lossy(&hook.stderr)
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

// ---------------------------------------------------------------------------
// pre-push push set bound to the pushed commits (TSK-132 review round 1)
// ---------------------------------------------------------------------------

const ZERO_SHA: &str = "0000000000000000000000000000000000000000";

fn rev(dir: &Path, rev: &str) -> String {
    let out = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(dir)
        .output()
        .expect("git rev-parse");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The location git passes a pre-push hook for `remote`: its push URL (a
/// `pushurl` when set), or the argument itself for a URL or path.
/// Configure `name` as an empty bare destination inside `dir`'s git
/// directory, so the pre-push hook can ask it what it holds (nothing).
fn answering_destination(dir: &Path, name: &str) {
    let bare = dir.join(".git").join(format!("{name}-destination.git"));
    git(
        dir,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(dir, &["remote", "add", name, bare.to_str().unwrap()]);
}

fn push_url(dir: &Path, remote: &str) -> String {
    Command::new("git")
        .args(["remote", "get-url", "--push", remote])
        .current_dir(dir)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map_or_else(
            || remote.to_string(),
            |out| String::from_utf8_lossy(&out.stdout).trim().to_string(),
        )
}

fn push_hook(dir: &Path, remote: &str, refs: &[(&str, &str)]) -> (Option<i32>, String) {
    let stdin = refs
        .iter()
        .map(|(branch, sha)| format!("refs/heads/{branch} {sha} refs/heads/{branch} {ZERO_SHA}"))
        .collect::<Vec<_>>()
        .join("\n");
    let url = push_url(dir, remote);
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-push", remote, &url])
            .current_dir(dir),
        &stdin,
    );
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// A repo on `main` whose quick target fails when `bad.txt` exists, with a
/// sibling branch `feat/bad` that commits `bad.txt` and `feat/good` that
/// does not.
#[cfg(unix)]
fn sibling_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
    std::fs::write(
        dir.path().join(".codeflow/test-config.json"),
        r#"{"schema_version": "1.0", "targets": [
  {"name": "lint", "runner": "custom", "modes": {"quick": {"command": "test ! -f bad.txt"}}}]}"#,
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "chore: add a lint target"]);
    git(dir.path(), &["checkout", "-b", "feat/bad"]);
    std::fs::write(dir.path().join("bad.txt"), "bad\n").unwrap();
    git(dir.path(), &["add", "bad.txt"]);
    git(dir.path(), &["commit", "-m", "feat: add a bad file"]);
    git(dir.path(), &["checkout", "main"]);
    git(dir.path(), &["checkout", "-b", "feat/good"]);
    std::fs::write(dir.path().join("good.txt"), "good\n").unwrap();
    git(dir.path(), &["add", "good.txt"]);
    git(dir.path(), &["commit", "-m", "feat: add a good file"]);
    answering_destination(dir.path(), "upstream");
    dir
}

#[cfg(unix)]
#[test]
fn push_set_tree_checks_never_pass_for_a_sibling_ref() {
    let dir = sibling_repo();
    let good = rev(dir.path(), "feat/good");
    let bad = rev(dir.path(), "feat/bad");

    // Multi-ref push from the clean `feat/good` checkout: the tree checks run
    // for `feat/good` only, and say they did not run for `feat/bad`.
    let (code, err) = push_hook(
        dir.path(),
        "upstream",
        &[("feat/good", &good), ("feat/bad", &bad)],
    );
    assert_eq!(code, Some(0), "{err}");
    assert!(
        err.contains("quick targets passed on the working checkout (1 target(s)"),
        "{err}"
    );
    assert!(
        err.contains(
            "tree checks (`codeflow validate --docs`, quick targets) did not run for 'feat/bad'"
        ),
        "{err}"
    );
    assert!(err.contains("is not the checked-out commit"), "{err}");
    assert!(
        !err.contains("quick targets) did not run for 'feat/good'"),
        "{err}"
    );

    // The same bad commit checked out is blocked by its quick target.
    git(dir.path(), &["checkout", "feat/bad"]);
    let (code, err) = push_hook(dir.path(), "upstream", &[("feat/bad", &bad)]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("push set failed for: lint"), "{err}");

    // Uncommitted tracked changes cannot stand in for the pushed tree: a
    // dirty checkout runs no tree checks and says so.
    std::fs::write(dir.path().join("base.txt"), "edited\n").unwrap();
    let (code, err) = push_hook(dir.path(), "upstream", &[("feat/bad", &bad)]);
    assert_eq!(code, Some(0), "{err}");
    assert!(
        err.contains("did not run for 'feat/bad': tracked files differ"),
        "{err}"
    );
    assert!(!err.contains("quick targets passed"), "{err}");
}

/// Push one ref whose destination currently holds `remote_sha`.
fn push_hook_onto(
    dir: &Path,
    remote: &str,
    branch: &str,
    sha: &str,
    remote_sha: &str,
) -> (Option<i32>, String) {
    let url = push_url(dir, remote);
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-push", remote, &url])
            .current_dir(dir),
        &format!("refs/heads/{branch} {sha} refs/heads/{branch} {remote_sha}\n"),
    );
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn commit_file(dir: &Path, file: &str, body: &str, message: &str) -> String {
    std::fs::write(dir.join(file), body).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", message]);
    rev(dir, "HEAD")
}

#[test]
fn push_set_by_path_reports_an_unresolved_range_without_borrowing_history() {
    // T132-2 and R2-2: a first push to an empty bare repository named by its
    // path, with an unrelated tracking ref and a local `main` at hand. Neither
    // stands in for the destination's history: the range is unresolved.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    git(dir.path(), &["checkout", "-b", "feat/x"]);
    let head = commit_file(dir.path(), "x.txt", "x\n", "Not conventional.");
    git(
        dir.path(),
        &["update-ref", "refs/remotes/origin/seen", &head],
    );
    let bare = tempfile::tempdir().unwrap();
    git(bare.path(), &["init", "--bare", "-q"]);

    let path = bare.path().to_str().unwrap();
    let (code, err) = push_hook(dir.path(), path, &[("feat/x", &head)]);
    assert_eq!(code, Some(0), "{err}");
    assert!(
        err.contains("`codeflow ci` did not run for 'feat/x': range unresolved"),
        "{err}"
    );
    assert!(
        !err.contains("codeflow ci --base"),
        "no local base substituted: {err}"
    );
}

/// The destination `bare` fetches `refspec` from `from`, as if it had been
/// pushed there, without running any client hook.
fn receive(bare: &Path, from: &Path, refspec: &str) {
    git(bare, &["fetch", "-q", from.to_str().unwrap(), refspec]);
}

/// A bare destination whose only branch is `stable`, holding `commits` made
/// in a scratch clone, and a local repository with it configured as `dest`.
fn stable_destination(legacy_subject: &str) -> (tempfile::TempDir, tempfile::TempDir) {
    let bare = tempfile::tempdir().unwrap();
    git(bare.path(), &["init", "--bare", "-q", "-b", "stable"]);
    let local = tempfile::tempdir().unwrap();
    init_repo(local.path(), "main");
    commit_file(local.path(), "legacy.txt", "legacy\n", legacy_subject);
    let url = bare.path().to_str().unwrap();
    // The destination takes the commits by fetching them: no client hook.
    receive(bare.path(), local.path(), "main:stable");
    git(local.path(), &["remote", "add", "dest", url]);
    git(local.path(), &["fetch", "-q", "dest"]);
    // `stable` is the destination's protected branch.
    write_policy(
        local.path(),
        r#"{"git": {"protected_branches": ["stable"], "test_gate_on_push": "block"}}"#,
    );
    (bare, local)
}

#[test]
fn push_set_uses_the_destination_base_not_local_main() {
    // R2-2 control 1: the destination's default branch is `stable`; local
    // `main` moves to the bad commit. The destination's own tracking ref is
    // the base, so the bad commit is checked and blocked.
    let (_bare, local) = stable_destination("chore: legacy base");
    git(
        local.path(),
        &["checkout", "-q", "-b", "feat/x", "dest/stable"],
    );
    let bad = commit_file(local.path(), "x.txt", "x\n", "Not conventional.");
    git(local.path(), &["branch", "-f", "main", &bad]);
    let (code, err) = push_hook(local.path(), "dest", &[("feat/x", &bad)]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("push set check failed: `codeflow ci --base"),
        "{err}"
    );
}

#[test]
fn push_set_ignores_a_stale_local_main() {
    // R2-2 control 2: the destination already holds a legacy non-conventional
    // commit; local `main` is behind it. Only the new conventional commit is
    // in range, so the push passes and ci did run.
    let (_bare, local) = stable_destination("Legacy subject.");
    git(
        local.path(),
        &["checkout", "-q", "-b", "feat/y", "dest/stable"],
    );
    git(local.path(), &["branch", "-f", "main", "HEAD~1"]);
    let good = commit_file(local.path(), "y.txt", "y\n", "feat: add y");
    let (code, err) = push_hook(local.path(), "dest", &[("feat/y", &good)]);
    assert_eq!(code, Some(0), "{err}");
    assert!(!err.contains("did not run"), "{err}");
}

#[test]
fn push_set_checks_a_rewrite_for_its_own_commits_and_says_so() {
    // A branch the destination holds is rebased onto a destination commit it
    // did not have. The destination advertises that commit now, so only the
    // rebased commit is checked, and the hook says how many.
    let (bare, local) = stable_destination("chore: legacy base");
    git(local.path(), &["checkout", "-q", "-b", "feat/r", "main~1"]);
    let old = commit_file(local.path(), "r.txt", "r\n", "feat: add r");
    receive(bare.path(), local.path(), "feat/r:feat/r");
    git(local.path(), &["fetch", "-q", "dest"]);
    git(local.path(), &["rebase", "-q", "dest/stable"]);
    let rebased = rev(local.path(), "HEAD");
    let (code, err) = push_hook_onto(local.path(), "dest", "feat/r", &rebased, &old);
    assert_eq!(code, Some(0), "{err}");
    assert!(
        err.contains("'feat/r' rewrites the destination's")
            && err.contains("checks 1 commit(s), leaving out history"),
        "{err}"
    );

    // A bad commit in the rewrite is blocked.
    let bad = commit_file(local.path(), "s.txt", "s\n", "Not conventional.");
    let (code, err) = push_hook_onto(local.path(), "dest", "feat/r", &bad, &old);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("push set check failed: `codeflow ci"), "{err}");
}

/// A task branch `task/t` on the destination, cut from `integration/line`
/// with `own` commits; then the line moves on the destination by a legacy
/// non-conventional commit, fetched here. Returns the task branch's old tip.
fn task_on_moved_line(bare: &Path, local: &Path, own: &[(&str, &str)]) -> String {
    integration_line(bare, local);
    git(local, &["checkout", "-q", "-b", "task/t", "line"]);
    for (file, message) in own {
        commit_file(local, file, "t\n", message);
    }
    let old = rev(local, "HEAD");
    receive(bare, local, "task/t:task/t");
    git(local, &["checkout", "-q", "line"]);
    commit_file(local, "line2.txt", "line2\n", "Legacy line subject two.");
    receive(bare, local, "line:integration/line");
    git(local, &["fetch", "-q", "dest"]);
    git(local, &["checkout", "-q", "task/t"]);
    old
}

#[test]
fn push_set_checks_only_the_own_commits_of_a_branch_rebased_onto_a_moved_line() {
    // TSK-115: a task branch rebased onto its moved integration line and
    // force-pushed. The line's new commit is on the destination through the
    // line's own ref, so only the task's commit is checked.
    let (bare, local) = stable_destination("chore: legacy base");
    let old = task_on_moved_line(bare.path(), local.path(), &[("t.txt", "feat: add t")]);
    git(local.path(), &["rebase", "-q", "dest/integration/line"]);
    let rebased = rev(local.path(), "HEAD");
    let (code, err) = push_hook_onto(local.path(), "dest", "task/t", &rebased, &old);
    assert_eq!(code, Some(0), "{err}");
    assert!(
        err.contains("'task/t' rewrites the destination's")
            && err.contains("checks 1 commit(s), leaving out history"),
        "{err}"
    );
    assert!(!err.contains("Legacy line subject two."), "{err}");

    // A bad own commit in the rebased branch still blocks.
    let bad = commit_file(local.path(), "s.txt", "s\n", "Not conventional.");
    let (code, err) = push_hook_onto(local.path(), "dest", "task/t", &bad, &old);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("Not conventional."), "{err}");
    assert!(!err.contains("Legacy line subject two."), "{err}");
}

#[test]
fn push_set_checks_only_the_own_commits_of_a_branch_that_merged_the_line_twice() {
    // TSK-165: a pushed task branch merges its moved line, the line moves
    // again, and the branch merges it once more before the next push. Both
    // line tips border the range; the older one's successors are history the
    // destination holds through the line, so they are never checked again.
    let (bare, local) = stable_destination("chore: legacy base");
    let old = task_on_moved_line(bare.path(), local.path(), &[("t.txt", "feat: add t")]);
    let merge = |message: &str| {
        git(
            local.path(),
            &[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                message,
                "dest/integration/line",
            ],
        );
    };
    merge("chore: take the line");
    git(local.path(), &["checkout", "-q", "line"]);
    commit_file(
        local.path(),
        "line3.txt",
        "line3\n",
        "Legacy line subject three.",
    );
    receive(bare.path(), local.path(), "line:integration/line");
    git(local.path(), &["fetch", "-q", "dest"]);
    git(local.path(), &["checkout", "-q", "task/t"]);
    merge("chore: take the line again");
    let head = commit_file(local.path(), "u.txt", "u\n", "feat: add u");
    let (code, err) = push_hook_onto(local.path(), "dest", "task/t", &head, &old);
    assert_eq!(code, Some(0), "{err}");
    assert!(!err.contains("Legacy line subject"), "{err}");

    // A bad own commit after the second merge still blocks.
    let bad = commit_file(local.path(), "s.txt", "s\n", "Not conventional.");
    let (code, err) = push_hook_onto(local.path(), "dest", "task/t", &bad, &old);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("Not conventional."), "{err}");
    assert!(!err.contains("Legacy line subject"), "{err}");
}

#[test]
fn push_set_blocks_a_rewrite_that_drops_a_commit_and_adds_a_bad_one() {
    // The rewrite drops the branch's second commit, adds a bad one and moves
    // onto the line's new tip. The dropped commit's history does not hide
    // the bad one, and the line's commit is not checked again.
    let (bare, local) = stable_destination("chore: legacy base");
    let old = task_on_moved_line(
        bare.path(),
        local.path(),
        &[("a.txt", "feat: add a"), ("b.txt", "feat: add b")],
    );
    git(
        local.path(),
        &[
            "rebase",
            "-q",
            "--onto",
            "dest/integration/line",
            "line~1",
            "HEAD~1",
        ],
    );
    let bad = commit_file(local.path(), "c.txt", "c\n", "Not conventional.");
    let (code, err) = push_hook_onto(local.path(), "dest", "task/t", &bad, &old);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("Not conventional."), "{err}");
    assert!(!err.contains("Legacy line subject two."), "{err}");
    assert!(
        err.contains("checks 2 commit(s), leaving out history"),
        "{err}"
    );
}

#[test]
fn push_set_refuses_a_rewrite_when_ls_remote_fails() {
    // The destination cannot be asked, so whether the release rules apply
    // cannot be read there (SPC-013 R-120, Codex R145-R4-1): the rewrite is
    // refused, never judged from its advertised sha alone.
    let (bare, local) = stable_destination("chore: legacy base");
    let old = task_on_moved_line(bare.path(), local.path(), &[("t.txt", "feat: add t")]);
    git(local.path(), &["rebase", "-q", "dest/integration/line"]);
    let rebased = rev(local.path(), "HEAD");
    let gone = local.path().join("no-such-destination");
    git(
        local.path(),
        &["remote", "set-url", "dest", gone.to_str().unwrap()],
    );
    let (code, err) = push_hook_onto(local.path(), "dest", "task/t", &rebased, &old);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("the destination did not answer, so whether 'task/t' is a release branch cannot be read"),
        "{err}"
    );
    assert!(
        !err.contains("`codeflow ci --base"),
        "no range is judged: {err}"
    );
}

/// The R4-1 history: the destination held B ("Not conventional.") on the
/// `stale` branches, then rewrote them to O without a fetch here, so their
/// cached tracking refs still hold B. Returns (local head C on B, O).
fn stale_destination_history(bare: &Path, local: &Path, stale: &[&str]) -> (String, String) {
    git(local, &["checkout", "-q", "-b", "work", "main~1"]);
    let bad = commit_file(local, "b.txt", "b\n", "Not conventional.");
    for name in stale {
        receive(bare, local, &format!("work:{name}"));
    }
    git(local, &["fetch", "-q", "dest"]);
    git(local, &["checkout", "-q", "-b", "other", "main~1"]);
    let replaced = commit_file(local, "o.txt", "o\n", "feat: replace previous work");
    for name in stale {
        receive(bare, local, &format!("+other:{name}"));
    }
    for name in stale {
        assert_eq!(rev(local, &format!("dest/{name}")), bad, "stale {name}");
    }
    git(local, &["checkout", "-q", "work"]);
    let head = commit_file(local, "c.txt", "c\n", "feat: add new work");
    (head, replaced)
}

#[test]
fn push_set_checks_a_commit_only_a_stale_tracking_ref_holds() {
    // R4-1: a push of C restores B, which the destination dropped. Whatever
    // cached ref still holds B (the pushed branch's own, its own integration
    // ref, or a sibling integration ref), B is checked.
    for (branch, stale) in [
        ("feat/x", &["feat/x"][..]),
        ("integration/probe", &["integration/probe"][..]),
        ("feat/x", &["feat/x", "integration/sibling"][..]),
    ] {
        let (bare, local) = stable_destination("chore: legacy base");
        let (head, replaced) = stale_destination_history(bare.path(), local.path(), stale);
        let (code, err) = push_hook_onto(local.path(), "dest", branch, &head, &replaced);
        assert_eq!(code, Some(1), "{branch} with stale {stale:?}: {err}");
        assert!(err.contains("Not conventional."), "{err}");
    }
}

/// An integration line `integration/line` on the destination, cut from
/// `stable` with a legacy non-conventional commit, fetched here; returns the
/// line's tip.
fn integration_line(bare: &Path, local: &Path) -> String {
    git(local, &["checkout", "-q", "-b", "line", "dest/stable"]);
    let tip = commit_file(local, "line.txt", "line\n", "Legacy line subject.");
    receive(bare, local, "line:integration/line");
    git(local, &["fetch", "-q", "dest"]);
    tip
}

#[test]
fn push_set_checks_only_the_own_commits_of_a_new_branch_off_a_line() {
    // A new branch cut from an integration line is bounded by what the
    // destination advertises now, not by its protected branches alone: the
    // line's legacy commit is not checked again, a bad own commit is.
    let (bare, local) = stable_destination("chore: legacy base");
    integration_line(bare.path(), local.path());
    git(local.path(), &["checkout", "-q", "-b", "feat/new", "line"]);
    let good = commit_file(local.path(), "n.txt", "n\n", "feat: add new work");
    let (code, err) = push_hook(local.path(), "dest", &[("feat/new", &good)]);
    assert_eq!(code, Some(0), "{err}");
    assert!(!err.contains("did not run for 'feat/new'"), "{err}");
    assert!(!err.contains("ls-remote"), "{err}");

    let bad = commit_file(local.path(), "m.txt", "m\n", "Not conventional.");
    let (code, err) = push_hook(local.path(), "dest", &[("feat/new", &bad)]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("Not conventional."), "{err}");
    assert!(!err.contains("Legacy line subject."), "{err}");
}

/// A valid task record on the advertised line, before the task's own work.
fn declared_line(bare: &Path, local: &Path) -> String {
    // The task record makes the project tracked, and a tracked default
    // target carries its policy: the release scope is read there, and an
    // unreadable one refuses (SPC-013 R-120).
    git(local, &["add", ".codeflow/policy.json"]);
    git(local, &["commit", "-q", "-m", "chore: commit the policy"]);
    receive(bare, local, "main:stable");
    git(local, &["fetch", "-q", "dest"]);
    integration_line(bare, local);
    std::fs::create_dir_all(local.join("project-management/tasks")).unwrap();
    let tip = commit_file(
        local,
        "project-management/tasks/TSK-001.md",
        "---\nid: TSK-001\nepic_id: null\nstandalone_reason: bounded repair\nintegration_target: integration/line\ntitle: repair\nstatus: todo\nwork_type: fix\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n\n## Description\nRepair.\n\n## Acceptance Criteria\n- AC-1 repair verified\n",
        "docs: declare the task target",
    );
    receive(bare, local, "line:integration/line");
    git(local, &["fetch", "-q", "dest"]);
    git(local, &["branch", "-f", "integration/line", "line"]);
    tip
}

fn declared_task_on_moved_line(bare: &Path, local: &Path, own: &[(&str, &str)]) -> String {
    declared_line(bare, local);
    git(
        local,
        &["checkout", "-q", "-b", "task/TSK-001-change", "line"],
    );
    for (file, message) in own {
        commit_file(local, file, "t\n", message);
    }
    let old = rev(local, "HEAD");
    receive(bare, local, "task/TSK-001-change:task/TSK-001-change");
    git(local, &["checkout", "-q", "line"]);
    commit_file(local, "line2.txt", "line2\n", "Legacy line subject two.");
    receive(bare, local, "line:integration/line");
    git(local, &["fetch", "-q", "dest"]);
    git(local, &["branch", "-f", "integration/line", "line"]);
    git(local, &["checkout", "-q", "task/TSK-001-change"]);
    old
}

#[test]
fn push_set_declared_target_rebase_checks_the_line_s_commits_from_the_default() {
    // TSK-115: a task branch rebased onto its moved integration line and
    // force-pushed. The line's new commit is on the destination through the
    // line's own ref, so the range of the other checks holds only the
    // task's commit.
    // TSK-212 review round five: the declared line still bounds the other
    // checks, but the commit checks run from the default branch's tip, the
    // candidate authority, for every branch: nothing proves the pull request
    // targets the line, and the line's commits must pass that policy when
    // the line reaches it anyway. So the line's inherited legacy commits are
    // judged too, and refuse the push.
    let (bare, local) = stable_destination("chore: legacy base");
    let old = declared_task_on_moved_line(bare.path(), local.path(), &[("t.txt", "feat: add t")]);
    git(local.path(), &["rebase", "-q", "dest/integration/line"]);
    let rebased = rev(local.path(), "HEAD");
    let (code, err) = push_hook_onto(local.path(), "dest", "task/TSK-001-change", &rebased, &old);
    assert!(
        err.contains("uses advertised target 'integration/line'"),
        "{err}"
    );
    assert!(
        err.contains("'task/TSK-001-change' rewrites the destination's")
            && err.contains("checks 1 commit(s), leaving out history"),
        "{err}"
    );
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("Legacy line subject two."), "{err}");

    // A bad own commit in the rebased branch blocks as well.
    let bad = commit_file(local.path(), "s.txt", "s\n", "Not conventional.");
    let (code, err) = push_hook_onto(local.path(), "dest", "task/TSK-001-change", &bad, &old);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("Not conventional."), "{err}");
}

#[test]
fn push_set_declared_target_rewrite_checks_the_added_bad_commit() {
    // The rewrite drops the branch's second commit, adds a bad one and moves
    // onto the line's new tip. The dropped commit's history does not hide
    // the bad one.
    // TSK-212 review round five: the declared line still bounds the other
    // checks, but the commit checks run from the default branch's tip, the
    // candidate authority, for every branch: nothing proves the pull request
    // targets the line, and the line's commits must pass that policy when
    // the line reaches it anyway. So the line's inherited legacy commits are
    // judged too, and refuse the push.
    let (bare, local) = stable_destination("chore: legacy base");
    let old = declared_task_on_moved_line(
        bare.path(),
        local.path(),
        &[("a.txt", "feat: add a"), ("b.txt", "feat: add b")],
    );
    git(
        local.path(),
        &[
            "rebase",
            "-q",
            "--onto",
            "dest/integration/line",
            "line~1",
            "HEAD~1",
        ],
    );
    let bad = commit_file(local.path(), "c.txt", "c\n", "Not conventional.");
    let (code, err) = push_hook_onto(local.path(), "dest", "task/TSK-001-change", &bad, &old);
    assert!(
        err.contains("uses advertised target 'integration/line'"),
        "{err}"
    );
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("Not conventional."), "{err}");
    assert!(err.contains("Legacy line subject two."), "{err}");
    assert!(
        err.contains("checks 2 commit(s), leaving out history"),
        "{err}"
    );
}

#[test]
fn push_set_declared_target_new_branch_checks_the_line_s_commits_from_the_default() {
    // The task record anchors the target on the line before the branch
    // starts, and the target selection reports its base.
    // TSK-212 review round five: the declared line still bounds the other
    // checks, but the commit checks run from the default branch's tip, the
    // candidate authority, for every branch: nothing proves the pull request
    // targets the line, and the line's commits must pass that policy when
    // the line reaches it anyway. So the line's inherited legacy commits are
    // judged too, and refuse the push.
    let (bare, local) = stable_destination("chore: legacy base");
    declared_line(bare.path(), local.path());
    git(
        local.path(),
        &["checkout", "-q", "-b", "task/TSK-001-change", "line"],
    );
    let good = commit_file(local.path(), "n.txt", "n\n", "feat: add new work");
    let (code, err) = push_hook(local.path(), "dest", &[("task/TSK-001-change", &good)]);
    assert!(
        err.contains("uses advertised target 'integration/line'"),
        "{err}"
    );
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("Legacy line subject."), "{err}");
    assert!(
        !err.contains("did not run for 'task/TSK-001-change'"),
        "{err}"
    );
    assert!(!err.contains("ls-remote"), "{err}");

    let bad = commit_file(local.path(), "m.txt", "m\n", "Not conventional.");
    let (code, err) = push_hook(local.path(), "dest", &[("task/TSK-001-change", &bad)]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("Not conventional."), "{err}");
}

#[test]
fn push_set_ignores_a_tracking_ref_the_destination_deleted() {
    // A sibling branch held a bad commit, then was deleted on the
    // destination; its tracking ref here still holds it. It no longer
    // bounds a new branch built on it, so the commit is checked.
    let (bare, local) = stable_destination("chore: legacy base");
    git(
        local.path(),
        &["checkout", "-q", "-b", "sib", "dest/stable"],
    );
    let bad = commit_file(local.path(), "b.txt", "b\n", "Not conventional.");
    receive(bare.path(), local.path(), "sib:integration/sib");
    git(local.path(), &["fetch", "-q", "dest"]);
    git(
        bare.path(),
        &["update-ref", "-d", "refs/heads/integration/sib"],
    );
    assert_eq!(rev(local.path(), "dest/integration/sib"), bad, "stale ref");

    git(
        local.path(),
        &["checkout", "-q", "-b", "feat/on-sib", "sib"],
    );
    let head = commit_file(local.path(), "c.txt", "c\n", "feat: add new work");
    let (code, err) = push_hook(local.path(), "dest", &[("feat/on-sib", &head)]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("Not conventional."), "{err}");
}

#[test]
fn push_set_refuses_a_new_branch_when_ls_remote_fails() {
    // The destination cannot be asked: a new branch is refused rather than
    // bounded by the tracked protected branches, since whether the release
    // rules apply there cannot be read (Codex R145-R4-1).
    let (bare, local) = stable_destination("Legacy subject.");
    integration_line(bare.path(), local.path());
    let gone = local.path().join("no-such-destination");
    git(
        local.path(),
        &["remote", "set-url", "dest", gone.to_str().unwrap()],
    );

    git(
        local.path(),
        &["checkout", "-q", "-b", "feat/f", "dest/stable"],
    );
    let good = commit_file(local.path(), "f.txt", "f\n", "feat: add f");
    let (code, err) = push_hook(local.path(), "dest", &[("feat/f", &good)]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("the destination did not answer, so whether 'feat/f' is a release branch cannot be read"),
        "{err}"
    );
    assert!(!err.contains("is bounded by its tracked"), "{err}");
}

#[test]
fn push_set_asks_the_push_location_not_the_fetch_location() {
    // NB-1: `dest` fetches from one repository and pushes to another. Only
    // the fetch repository holds B; the push repository must still check it.
    let (fetch_repo, local) = stable_destination("chore: legacy base");
    let push_repo = tempfile::tempdir().unwrap();
    git(push_repo.path(), &["init", "--bare", "-q", "-b", "stable"]);
    receive(push_repo.path(), local.path(), "main:stable");
    git(
        local.path(),
        &["checkout", "-q", "-b", "line", "dest/stable"],
    );
    commit_file(local.path(), "b.txt", "b\n", "Not conventional.");
    receive(fetch_repo.path(), local.path(), "line:integration/line");
    git(local.path(), &["fetch", "-q", "dest"]);
    git(
        local.path(),
        &[
            "remote",
            "set-url",
            "--push",
            "dest",
            push_repo.path().to_str().unwrap(),
        ],
    );
    git(
        local.path(),
        &["checkout", "-q", "-b", "feat/new", "dest/integration/line"],
    );
    let head = commit_file(local.path(), "c.txt", "c\n", "feat: add new work");
    let (code, err) = push_hook(local.path(), "dest", &[("feat/new", &head)]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("Not conventional."), "{err}");

    // The push location cannot be asked, and the tracking refs describe the
    // fetch location: the range is unresolved, never bounded by them, and
    // with no answer the push is refused (SPC-013 R-120).
    let gone = local.path().join("no-such-destination");
    git(
        local.path(),
        &[
            "remote",
            "set-url",
            "--push",
            "dest",
            gone.to_str().unwrap(),
        ],
    );
    let (code, err) = push_hook(local.path(), "dest", &[("feat/new", &head)]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("the destination did not answer, so whether 'feat/new' is a release branch cannot be read")
            && err.contains("`git ls-remote` failed"),
        "{err}"
    );
}

/// A loopback HTTP server: `respond` answers every request with a 401 that
/// asks for credentials; otherwise each connection is held open, unanswered.
#[cfg(unix)]
fn http_destination(respond: bool) -> String {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            if respond {
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"t\"\r\n\
                      Content-Length: 0\r\nConnection: close\r\n\r\n",
                );
            } else {
                held.push(stream);
            }
        }
    });
    format!("http://127.0.0.1:{port}/repo.git")
}

/// The pre-push hook for a new branch pushed to `url` under `dest`, with
/// `askpass` as the inherited `GIT_ASKPASS`; returns its exit, stderr and
/// how long it took.
#[cfg(unix)]
fn push_hook_over_http(
    dir: &Path,
    url: &str,
    askpass: &Path,
) -> (Option<i32>, String, std::time::Duration) {
    git(dir, &["remote", "add", "dest", url]);
    let head = rev(dir, "HEAD");
    let started = std::time::Instant::now();
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-push", "dest", url])
            .env("GIT_ASKPASS", askpass)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .current_dir(dir),
        &format!("refs/heads/feat/h {head} refs/heads/feat/h {ZERO_SHA}\n"),
    );
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        started.elapsed(),
    )
}

#[cfg(unix)]
#[test]
fn push_set_never_prompts_and_bounds_the_destination_query() {
    // NB-2: a destination asking for credentials gets no askpass prompt, and
    // one that never answers is abandoned at the deadline; either way the
    // hook says why, and refuses the unjudged push (SPC-013 R-120).
    use std::os::unix::fs::PermissionsExt;
    let scratch = tempfile::tempdir().unwrap();
    let marker = scratch.path().join("askpass-called");
    let askpass = scratch.path().join("askpass");
    std::fs::write(
        &askpass,
        format!(
            "#!/bin/sh\nprintf invoked > '{}'\nsleep 60\n",
            marker.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&askpass, std::fs::Permissions::from_mode(0o755)).unwrap();

    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/h");
    let (code, err, took) = push_hook_over_http(dir.path(), &http_destination(true), &askpass);
    assert_eq!(code, Some(1), "{err}");
    assert!(!marker.exists(), "askpass was invoked: {err}");
    assert!(took < std::time::Duration::from_secs(10), "{took:?}: {err}");
    assert!(
        err.contains("the destination did not answer") && err.contains("`git ls-remote` failed"),
        "{err}"
    );

    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/h");
    let (code, err, took) = push_hook_over_http(dir.path(), &http_destination(false), &askpass);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("no answer within 10s"), "{err}");
    assert!(took < std::time::Duration::from_secs(30), "{took:?}");
}

#[test]
fn push_set_fetches_nothing_in_a_partial_clone() {
    // NB-3: an advertised tip missing from a partial clone is left out of
    // the bound, and never downloaded.
    let source = tempfile::tempdir().unwrap();
    init_repo(source.path(), "main");
    let bare = tempfile::tempdir().unwrap();
    git(bare.path(), &["init", "--bare", "-q", "-b", "main"]);
    git(bare.path(), &["config", "uploadpack.allowFilter", "true"]);
    receive(bare.path(), source.path(), "main:main");
    let url = format!("file://{}", bare.path().display());
    let clone = tempfile::tempdir().unwrap();
    git(
        clone.path(),
        &["clone", "-q", "--filter=blob:none", &url, "."],
    );
    git(clone.path(), &["config", "user.email", "t@example.com"]);
    git(clone.path(), &["config", "user.name", "t"]);
    git(source.path(), &["checkout", "-q", "-b", "feature"]);
    let remote_only = commit_file(source.path(), "next.txt", "next\n", "feat: add next");
    receive(bare.path(), source.path(), "feature:feature");

    git(clone.path(), &["checkout", "-q", "-b", "feat/own"]);
    let head = commit_file(clone.path(), "own.txt", "own\n", "feat: add own");
    let objects = || {
        let out = Command::new("git")
            .args([
                "cat-file",
                "--batch-all-objects",
                "--batch-check=%(objectname)",
            ])
            .current_dir(clone.path())
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let before = objects();
    assert!(!before.contains(&remote_only));
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-push", "origin", &url])
            .current_dir(clone.path()),
        &format!("refs/heads/feat/own {head} refs/heads/feat/own {ZERO_SHA}\n"),
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{err}");
    assert!(!err.contains("range unresolved"), "bounded by main: {err}");
    assert_eq!(objects(), before, "the hook fetched objects");
}

#[test]
fn push_set_blocks_an_unrelated_base_and_notes_an_unresolved_one() {
    // T132-3: a resolved but unrelated base (an orphan pushed over a branch
    // the destination holds) is a failed check with its diagnostic.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    let main = rev(dir.path(), "main");
    answering_destination(dir.path(), "origin");
    git(dir.path(), &["push", "-q", "origin", "main"]);
    git(dir.path(), &["checkout", "-q", "--orphan", "feat/orphan"]);
    let orphan = commit_file(dir.path(), "o.txt", "o\n", "chore: start an orphan");
    let (code, err) = push_hook_onto(dir.path(), "origin", "feat/orphan", &orphan, &main);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("push set check failed: `codeflow ci"), "{err}");
    assert!(!err.contains("range unresolved"), "{err}");

    // No destination sha and no tracking history: a note, never a pass.
    let lone = tempfile::tempdir().unwrap();
    init_repo(lone.path(), "feat/lone");
    answering_destination(lone.path(), "origin");
    let head = rev(lone.path(), "HEAD");
    let (code, err) = push_hook(lone.path(), "origin", &[("feat/lone", &head)]);
    assert_eq!(code, Some(0), "{err}");
    assert!(
        err.contains("`codeflow ci` did not run for 'feat/lone': range unresolved"),
        "{err}"
    );

    // A destination that does not answer cannot say whether the name is a
    // release branch under its policy: refused, whatever the name.
    let (code, err) = push_hook(lone.path(), "nowhere", &[("feat/lone", &head)]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("the destination did not answer"), "{err}");
}

/// A repo on `feat/t` whose quick target is `command`, committed.
fn quick_repo(command: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/t");
    std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
    std::fs::write(
        dir.path().join(".codeflow/test-config.json"),
        format!(
            r#"{{"schema_version": "1.0", "targets": [
  {{"name": "lint", "runner": "custom", "modes": {{"quick": {{"command": "{command}"}}}}}}]}}"#
        ),
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &["commit", "-q", "-m", "chore: add a lint target"],
    );
    answering_destination(dir.path(), "upstream");
    dir
}

#[test]
fn push_set_untracked_input_is_named_in_the_pass_line() {
    // R2-1 control 1: an untracked file flips the quick target. The hook
    // cannot tell, so its pass line says it checked the working checkout.
    let dir = quick_repo("test -f lint-ignore || test ! -f bad.txt");
    let head = commit_file(dir.path(), "bad.txt", "bad\n", "feat: add bad");
    let (code, err) = push_hook(dir.path(), "upstream", &[("feat/t", &head)]);
    assert_eq!(code, Some(1), "complete control blocks: {err}");
    std::fs::write(dir.path().join("lint-ignore"), "").unwrap();
    let (code, err) = push_hook(dir.path(), "upstream", &[("feat/t", &head)]);
    assert_eq!(code, Some(0), "{err}");
    assert!(
        err.contains("quick targets passed on the working checkout")
            && err.contains("untracked files there can influence them"),
        "{err}"
    );
}

#[test]
fn push_set_does_not_run_tree_checks_on_a_sparse_checkout() {
    // R2-1 control 2.
    let dir = quick_repo("test ! -f omitted/bad.txt");
    std::fs::create_dir_all(dir.path().join("omitted")).unwrap();
    let head = commit_file(dir.path(), "omitted/bad.txt", "bad\n", "feat: add omitted");
    let (code, err) = push_hook(dir.path(), "upstream", &[("feat/t", &head)]);
    assert_eq!(code, Some(1), "complete control blocks: {err}");
    git(dir.path(), &["sparse-checkout", "init", "--cone"]);
    git(dir.path(), &["sparse-checkout", "set", ".codeflow"]);
    assert!(!dir.path().join("omitted/bad.txt").exists());
    let (code, err) = push_hook(dir.path(), "upstream", &[("feat/t", &head)]);
    assert_eq!(code, Some(0), "{err}");
    assert!(
        err.contains("did not run for 'feat/t': the checkout is sparse"),
        "{err}"
    );
    assert!(!err.contains("quick targets passed"), "{err}");
}

#[test]
fn push_set_does_not_run_tree_checks_with_an_uninitialized_submodule() {
    // R2-1 control 3.
    let vendor = tempfile::tempdir().unwrap();
    init_repo(vendor.path(), "main");
    commit_file(vendor.path(), "bad.txt", "bad\n", "feat: add bad");
    let dir = quick_repo("test ! -f vendor/bad.txt");
    let url = vendor.path().to_str().unwrap();
    git(
        dir.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            url,
            "vendor",
        ],
    );
    git(dir.path(), &["commit", "-q", "-m", "chore: add vendor"]);
    let head = rev(dir.path(), "HEAD");
    let (code, err) = push_hook(dir.path(), "upstream", &[("feat/t", &head)]);
    assert_eq!(code, Some(1), "complete control blocks: {err}");
    git(dir.path(), &["submodule", "deinit", "-q", "-f", "vendor"]);
    let (code, err) = push_hook(dir.path(), "upstream", &[("feat/t", &head)]);
    assert_eq!(code, Some(0), "{err}");
    assert!(
        err.contains("did not run for 'feat/t': submodule vendor is not initialized"),
        "{err}"
    );
    assert!(!err.contains("quick targets passed"), "{err}");
}

/// SPC-013 R-85: the shims probe the binary's hook capability first. An
/// older binary refuses with the install command; the current binary dispatches.
#[cfg(unix)]
#[test]
fn hook_shims_refuse_when_the_binary_is_older() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let old_bin = dir.path().join("old-bin");
    std::fs::create_dir_all(&old_bin).unwrap();
    let fake = old_bin.join("codeflow");
    std::fs::write(
        &fake,
        "#!/bin/sh\nif [ \"$1 $2\" = \"git-hook capabilities\" ]; then echo \"error: invalid value 'capabilities'\" >&2; exit 2; fi\necho \"dispatched $*\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let new_bin = std::path::PathBuf::from(env!("CARGO_BIN_EXE_codeflow"))
        .parent()
        .unwrap()
        .to_path_buf();
    let shims =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/git-hooks");
    for stage in ["pre-commit", "commit-msg", "pre-push", "pre-merge-commit"] {
        let run = |bin: &Path| {
            Command::new("sh")
                .arg(shims.join(stage))
                .arg("ARG")
                .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
                .env("CODEFLOW_HOME", isolated_home())
                .current_dir(dir.path())
                .stdin(std::process::Stdio::null())
                .output()
                .unwrap()
        };
        let old = run(&old_bin);
        let stderr = String::from_utf8_lossy(&old.stderr);
        assert!(
            stderr.contains("missing, older or failed"),
            "{stage}: {stderr}"
        );
        assert!(stderr.contains("codeflow update"), "{stage}: {stderr}");
        assert_eq!(old.status.code(), Some(1));
        assert!(old.stdout.is_empty(), "older binary must not dispatch");
        let new = run(&new_bin);
        let stderr = String::from_utf8_lossy(&new.stderr);
        assert!(
            !stderr.contains("older than these hooks"),
            "{stage}: {stderr}"
        );
    }
}

// ---------------------------------------------------------------------------
// pre-push: the work-record baseline authority of an existing branch
// ---------------------------------------------------------------------------

const RECORDS_STATE: &str = "schema_version = 1\ntier = \"full\"\n\
scaffold_version = \"3.0.0\"\nstack = \"rust\"\nareas = []\npolicy_armed = true\n\
git_hooks = \"unwired\"\npermission_preset = \"default\"\n";

/// A task written under the old rules: complete, checkbox criteria, no
/// acceptance block. Legacy only while a governing baseline holds it.
fn legacy_task(id: &str) -> String {
    format!(
        "---\nid: {id}\nepic_id: null\nstandalone_reason: \"one change\"\n\
integration_target: main\ntitle: \"work\"\nstatus: complete\nwork_type: feat\n\
specs: []\ndepends_on: []\ncreated: 2026-09-26\n---\n\n# {id}: work\n\n\
## Description\n\nWork.\n\n## Acceptance Criteria\n\n- [x] AC-1 Done.\n\n\
## Closeout\n\nDone long ago.\n"
    )
}

fn set_records_baseline(dir: &Path, commits: &[&str]) -> String {
    let list = commits
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    std::fs::write(
        dir.join(".codeflow/project.toml"),
        format!("{RECORDS_STATE}work_records_baseline = [{list}]\n"),
    )
    .unwrap();
    git(dir, &["add", ".codeflow/project.toml"]);
    git(
        dir,
        &["commit", "-q", "-m", "chore: record the records baseline"],
    );
    rev(dir, "HEAD")
}

/// The release history: a destination whose `stable` holds a full-tier seed
/// with no baseline list, and two lines, each adding a legacy task and then
/// a list naming that commit. Returns (bare, local, line a's record commit,
/// line b's record commit); the local checkout is on `stable`'s seed.
const TWO_LINES_POLICY: &str = r#"{"git": {"protected_branches": ["stable"], "test_gate_on_push": "block", "work_records": "block"}}"#;

fn two_lines_with_lists() -> (tempfile::TempDir, tempfile::TempDir, String, String) {
    let bare = tempfile::tempdir().unwrap();
    git(bare.path(), &["init", "--bare", "-q", "-b", "stable"]);
    let local = tempfile::tempdir().unwrap();
    init_repo(local.path(), "main");
    std::fs::create_dir_all(local.path().join(".codeflow")).unwrap();
    std::fs::write(local.path().join(".codeflow/project.toml"), RECORDS_STATE).unwrap();
    // The default target carries the policy, so its release pattern can be
    // read (SPC-013 R-120: a tracked project whose default target has no
    // policy fails closed).
    write_policy(local.path(), TWO_LINES_POLICY);
    git(
        local.path(),
        &["add", ".codeflow/project.toml", ".codeflow/policy.json"],
    );
    git(local.path(), &["commit", "-q", "-m", "chore: full tier"]);
    receive(bare.path(), local.path(), "main:stable");
    git(
        local.path(),
        &["remote", "add", "dest", bare.path().to_str().unwrap()],
    );
    let mut records = Vec::new();
    for (line, id) in [("a", "TSK-001"), ("b", "TSK-002")] {
        git(local.path(), &["checkout", "-q", "-b", line, "main"]);
        std::fs::create_dir_all(local.path().join("project-management/tasks")).unwrap();
        let record = commit_file(
            local.path(),
            &format!("project-management/tasks/{id}.md"),
            &legacy_task(id),
            "docs: add a task under the old rules",
        );
        set_records_baseline(local.path(), &[&record]);
        receive(
            bare.path(),
            local.path(),
            &format!("{line}:integration/line-{line}"),
        );
        records.push(record);
    }
    git(local.path(), &["checkout", "-q", "main"]);
    let b = records.pop().unwrap();
    let a = records.pop().unwrap();
    (bare, local, a, b)
}

/// Merge a line; the lines' differing baseline lists resolve to ours, and
/// the caller records the list it means.
fn merge_line(local: &Path, line: &str) {
    git(
        local,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-X",
            "ours",
            "-m",
            "chore: merge a line",
            line,
        ],
    );
}

/// Merge both lines into a release checkout and record both baselines.
fn release_of_both_lines(local: &Path, a: &str, b: &str) -> String {
    for line in ["a", "b"] {
        merge_line(local, line);
    }
    set_records_baseline(local, &[a, b])
}

#[test]
fn push_to_an_existing_branch_without_a_list_is_judged_by_its_own_list() {
    // The release branch exists on the destination at the list-less seed.
    // The range starts at the release branch's advertised old tip. That
    // tip has no list, so the push introduces the migration and the head's
    // list governs, every entry printed.
    let (bare, local, a, b) = two_lines_with_lists();
    let seed = rev(local.path(), "main");
    receive(bare.path(), local.path(), "main:integration/release");
    git(local.path(), &["checkout", "-q", "-b", "release", "main"]);
    let head = release_of_both_lines(local.path(), &a, &b);
    let (_, err) = push_hook_onto(local.path(), "dest", "integration/release", &head, &seed);
    assert!(!err.contains("work.records (block)"), "{err}");
    assert!(
        err.contains("introduces work_records_baseline") && err.contains(&a) && err.contains(&b),
        "{err}"
    );
    assert!(err.contains(&format!("--baseline-from {seed}")), "{err}");
}

#[test]
fn push_to_an_existing_branch_with_a_list_is_judged_by_that_list() {
    // The release branch already holds line a with list [a]. The push merges
    // line b, adds a record of its own and lists everything. The old tip's
    // list governs: line a's record stays legacy. Line b's record and the
    // push's own record are both new to this target and must satisfy its
    // rules, since a list edit takes effect only after it lands.
    let (bare, local, a, b) = two_lines_with_lists();
    git(local.path(), &["checkout", "-q", "-b", "release", "main"]);
    merge_line(local.path(), "a");
    let old = rev(local.path(), "HEAD");
    receive(bare.path(), local.path(), "release:integration/release");
    merge_line(local.path(), "b");
    let own = commit_file(
        local.path(),
        "project-management/tasks/TSK-003.md",
        &legacy_task("TSK-003"),
        "docs: add a task the push wants exempt",
    );
    let head = set_records_baseline(local.path(), &[&a, &b, &own]);
    let (code, err) = push_hook_onto(local.path(), "dest", "integration/release", &head, &old);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("TSK-003.md: a complete record needs an acceptance block"),
        "{err}"
    );
    assert!(!err.contains("TSK-001.md"), "{err}");
    assert!(
        err.contains("TSK-002.md: a complete record needs an acceptance block"),
        "{err}"
    );
    assert!(
        err.contains("edits work_records_baseline") && err.contains(&own),
        "{err}"
    );
    assert!(!err.contains("introduces work_records_baseline"), "{err}");
}

#[test]
fn a_new_branch_push_keeps_the_range_base_as_the_baseline_authority() {
    // A first push of the release branch: no old tip, so the range's base
    // (a line tip with its own list) governs as before, and one line's
    // record is judged new.
    let (_bare, local, a, b) = two_lines_with_lists();
    git(local.path(), &["checkout", "-q", "-b", "release", "main"]);
    let head = release_of_both_lines(local.path(), &a, &b);
    let (code, err) = push_hook(local.path(), "dest", &[("integration/release", &head)]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("work.records (block)")
            && err.contains(".md: a complete record needs an acceptance block"),
        "{err}"
    );
    assert!(!err.contains("--baseline-from"), "{err}");
    assert!(!err.contains("introduces work_records_baseline"), "{err}");
}

// ---------------------------------------------------------------------------
// TSK-147 AC-2: a plane running another plane's check prints each finding at
// its effective level (SPC-013 R-80).
// ---------------------------------------------------------------------------

/// A destination whose `stable` holds the seed, with `policy` in the local
/// checkout and, when `tracking`, durable work tracking on. The local
/// checkout is on a new `feat/x` from the destination's `stable`.
fn gate_destination(policy: &str, tracking: bool) -> (tempfile::TempDir, tempfile::TempDir) {
    let bare = tempfile::tempdir().unwrap();
    git(bare.path(), &["init", "--bare", "-q", "-b", "stable"]);
    let local = tempfile::tempdir().unwrap();
    init_repo(local.path(), "main");
    if tracking {
        std::fs::create_dir_all(local.path().join(".codeflow")).unwrap();
        std::fs::write(local.path().join(".codeflow/project.toml"), RECORDS_STATE).unwrap();
    }
    // The default target carries the policy: the push gate and the rules
    // are read there (sathyassn/codeflow#22), and for a tracked target the
    // release scope too, where an unreadable one refuses (SPC-013 R-120).
    write_policy(local.path(), policy);
    git(local.path(), &["add", ".codeflow"]);
    git(local.path(), &["commit", "-q", "-m", "chore: the policy"]);
    receive(bare.path(), local.path(), "main:stable");
    git(
        local.path(),
        &["remote", "add", "dest", bare.path().to_str().unwrap()],
    );
    git(local.path(), &["fetch", "-q", "dest"]);
    write_policy(local.path(), policy);
    git(
        local.path(),
        &["checkout", "-q", "-b", "feat/x", "dest/stable"],
    );
    (bare, local)
}

const GATE_AT_WARN: &str =
    r#"{"git": {"protected_branches": ["stable"], "test_gate_on_push": "warn"}}"#;

#[test]
fn a_downgradable_ci_finding_prints_at_the_push_gate_level_warn() {
    let (_bare, local) = gate_destination(GATE_AT_WARN, false);
    let bad = commit_file(local.path(), "x.txt", "x\n", "Not conventional.");
    let (code, err) = push_hook(local.path(), "dest", &[("feat/x", &bad)]);
    assert_eq!(code, Some(0), "{err}");
    assert!(!err.contains("BLOCKED"), "{err}");
    assert!(
        err.contains("warning — policy rule git.commit_format (warn)"),
        "{err}"
    );
    assert!(err.contains("codeflow pre-push: push not stopped"), "{err}");
}

#[test]
fn a_downgradable_ci_finding_blocks_at_the_push_gate_level_block() {
    let (_bare, local) = gate_destination(
        r#"{"git": {"protected_branches": ["stable"], "test_gate_on_push": "block"}}"#,
        false,
    );
    let bad = commit_file(local.path(), "x.txt", "x\n", "Not conventional.");
    let (code, err) = push_hook(local.path(), "dest", &[("feat/x", &bad)]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("BLOCKED — policy rule git.commit_format (block)"),
        "{err}"
    );
    assert!(err.contains("codeflow pre-push: push stopped"), "{err}");
}

#[test]
fn an_always_blocking_ci_finding_stops_the_push_at_warn() {
    // Control: a registry rule has no level, so the push gate's warn cannot
    // lower it.
    let (_bare, local) = gate_destination(GATE_AT_WARN, true);
    std::fs::create_dir_all(local.path().join("project-management/tasks")).unwrap();
    let head = commit_file(
        local.path(),
        "project-management/tasks/TSK-001.md",
        &legacy_task("TSK-001"),
        "docs: add a task without a reservation",
    );
    let (code, err) = push_hook(local.path(), "dest", &[("feat/x", &head)]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("BLOCKED — lifecycle invariant work.id_registry (block)"),
        "{err}"
    );
    assert!(err.contains("codeflow pre-push: push stopped"), "{err}");
}

#[test]
fn a_configured_block_on_a_never_exempt_rule_stops_the_push_at_warn() {
    // Control: the project set `commit_emoji` to block itself, so the push
    // gate's warn cannot lower it.
    let (_bare, local) = gate_destination(
        r#"{"git": {"protected_branches": ["stable"], "test_gate_on_push": "warn", "commit_emoji": "block"}}"#,
        false,
    );
    let bad = commit_file(local.path(), "x.txt", "x\n", "feat: add x \u{1F680}");
    let (code, err) = push_hook(local.path(), "dest", &[("feat/x", &bad)]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("BLOCKED — policy rule git.commit_emoji (block)"),
        "{err}"
    );
    assert!(err.contains("codeflow pre-push: push stopped"), "{err}");
}

// ---------------------------------------------------------------------------
// TSK-147 AC-4: a commit on a watched contract path gets a local note.
// ---------------------------------------------------------------------------

#[test]
fn commit_msg_on_a_watched_path_notes_the_release_impact_fields() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    write_policy(
        dir.path(),
        r#"{"git": {"breaking_watch_paths": ["api/**"]}}"#,
    );
    std::fs::create_dir_all(dir.path().join("api")).unwrap();
    std::fs::write(dir.path().join("api/v1.rs"), "pub fn f() {}\n").unwrap();
    git(dir.path(), &["add", "api/v1.rs"]);
    let msg = dir.path().join("MSG");
    std::fs::write(&msg, "feat: add the v1 api\n").unwrap();
    let out = codeflow()
        .args(["git-hook", "commit-msg", msg.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{err}");
    assert!(
        err.contains(
            "codeflow commit-msg: note: commit touches a declared contract surface (api/v1.rs)"
        ),
        "{err}"
    );
    assert!(
        err.contains("`Breaking: no` with a `Rationale` under Release impact"),
        "{err}"
    );
    assert!(
        !err.contains("warning"),
        "a local run cannot settle it: {err}"
    );
    assert!(
        err.contains("codeflow commit-msg: commit not stopped"),
        "{err}"
    );
}

#[test]
fn a_configured_block_under_another_key_name_stops_the_push_at_warn() {
    // The ticket rule prints as `git.commit_ticket` but its level is the
    // `commit_ticket_required` key: a block set there keeps its level.
    let (_bare, local) = gate_destination(
        r#"{"git": {"protected_branches": ["stable"], "test_gate_on_push": "warn", "commit_ticket_required": "block", "commit_ticket_keys": ["Refs"], "commit_footer_tokens": ["Refs"]}}"#,
        false,
    );
    let bad = commit_file(local.path(), "x.txt", "x\n", "feat: add x");
    let (code, err) = push_hook(local.path(), "dest", &[("feat/x", &bad)]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("BLOCKED — policy rule git.commit_ticket (block)"),
        "{err}"
    );
    assert!(err.contains("codeflow pre-push: push stopped"), "{err}");
}

#[test]
fn an_unconfigured_ticket_rule_prints_at_the_push_gate_level() {
    // Control: the ticket rule at warn keeps the push going.
    let (_bare, local) = gate_destination(
        r#"{"git": {"protected_branches": ["stable"], "test_gate_on_push": "warn", "commit_ticket_required": "warn", "commit_ticket_keys": ["Refs"], "commit_footer_tokens": ["Refs"]}}"#,
        false,
    );
    let bad = commit_file(local.path(), "x.txt", "x\n", "feat: add x");
    let (code, err) = push_hook(local.path(), "dest", &[("feat/x", &bad)]);
    assert_eq!(code, Some(0), "{err}");
    assert!(
        err.contains("warning — policy rule git.commit_ticket (warn)"),
        "{err}"
    );
    assert!(err.contains("codeflow pre-push: push not stopped"), "{err}");
}

/// The `refusal` events in the repository's refusals ledger (TSK-149).
fn refusal_events(dir: &Path) -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(dir.join(".git/codeflow/ledger/refusals/refusals.jsonl"))
        .unwrap_or_default();
    text.lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|event| event["event"] == "refusal")
        .collect()
}

/// The raw refusals ledger, to check what it must never hold.
fn refusal_ledger_text(dir: &Path) -> String {
    std::fs::read_to_string(dir.join(".git/codeflow/ledger/refusals/refusals.jsonl"))
        .unwrap_or_default()
}

// TSK-149 AC-3: a push the pre-push hook stops appends one event naming the
// rule, the plane and the effective level, with no command text.
#[test]
fn a_blocked_push_appends_one_refusal_event() {
    let (_bare, local) = gate_destination(
        r#"{"git": {"protected_branches": ["stable"], "test_gate_on_push": "warn", "commit_ticket_required": "block", "commit_ticket_keys": ["Refs"], "commit_footer_tokens": ["Refs"]}}"#,
        false,
    );
    let bad = commit_file(local.path(), "x.txt", "x\n", "feat: add x");
    let (code, err) = push_hook(local.path(), "dest", &[("feat/x", &bad)]);
    assert_eq!(code, Some(1), "{err}");
    let events = refusal_events(local.path());
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["plane"], "pre-push");
    assert_eq!(events[0]["level"], "block");
    assert_eq!(
        events[0]["rules"],
        serde_json::json!(["git.test_gate_on_push", "git.commit_ticket"])
    );
    let text = refusal_ledger_text(local.path());
    for content in ["feat/x", "dest", &bad[..12], "add x"] {
        assert!(!text.contains(content), "{content:?} in {text}");
    }
}

// TSK-149 AC-3: the same finding at the push gate's warn level stops
// nothing, so it is not a refusal.
#[test]
fn a_warned_push_appends_no_refusal_event() {
    let (_bare, local) = gate_destination(
        r#"{"git": {"protected_branches": ["stable"], "test_gate_on_push": "warn", "commit_ticket_required": "warn", "commit_ticket_keys": ["Refs"], "commit_footer_tokens": ["Refs"]}}"#,
        false,
    );
    let bad = commit_file(local.path(), "x.txt", "x\n", "feat: add x");
    let (code, err) = push_hook(local.path(), "dest", &[("feat/x", &bad)]);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.contains("codeflow pre-push: push not stopped"), "{err}");
    assert!(
        refusal_events(local.path()).is_empty(),
        "{}",
        refusal_ledger_text(local.path())
    );
    assert!(
        refusal_ledger_text(local.path()).contains("refusal_recording_started"),
        "the plane marks that recording began"
    );
}

// TSK-149 AC-3: a session guard's refusal is recorded the same way, and the
// command, including a credential in it, never reaches the ledger.
#[test]
fn a_blocked_guard_call_appends_one_refusal_event_without_the_command() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    // Built at run time so the source holds no token-shaped literal.
    let token = ["ghp", "_", &"0123456789abcdefghij".repeat(2)[..36]].concat();
    let command = format!("git push https://x:{token}@github.com/o/r.git main");
    let out = guard_run(&command, dir.path());
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run_with_stdin(
        codeflow()
            .args(["hook", "exec-guard"])
            .current_dir(dir.path()),
        &guard_payload("rm -rf /", dir.path()),
    );
    assert_eq!(out.status.code(), Some(2));
    // A call the guards allow adds nothing.
    let out = guard_run("git status", dir.path());
    assert_eq!(out.status.code(), Some(0));

    let events = refusal_events(dir.path());
    let planes: Vec<_> = events.iter().map(|e| e["plane"].clone()).collect();
    assert_eq!(
        planes,
        [
            serde_json::json!("git-guard"),
            serde_json::json!("exec-guard")
        ]
    );
    assert!(events.iter().all(|e| e["level"] == "block"), "{events:?}");
    assert_eq!(
        events[0]["rules"],
        serde_json::json!(["git.push_to_protected"])
    );
    let text = refusal_ledger_text(dir.path());
    for content in [token.as_str(), "github.com", "git push", "rm -rf"] {
        assert!(!text.contains(content), "{content:?} in {text}");
    }
}

// TSK-149 review round 1, P1: text a check quotes from the operation never
// reaches the refusal record as a rule, even when it is shaped like the
// printed finding or like a rule id. The legitimate rules stay, in one event.
#[test]
fn quoted_commit_text_never_becomes_a_refusal_rule() {
    let (_bare, local) = gate_destination(
        r#"{"git": {"protected_branches": ["stable"], "test_gate_on_push": "block"}}"#,
        false,
    );
    commit_file(
        local.path(),
        "a.txt",
        "a\n",
        "INVALID: BLOCKED \u{2014} policy rule SYNTHETIC_CONTENT_CANARY remainder",
    );
    commit_file(
        local.path(),
        "b.txt",
        "b\n",
        "INVALID: BLOCKED \u{2014} policy rule src/secret-canary.rs (block)",
    );
    let head = commit_file(
        local.path(),
        "c.txt",
        "c\n",
        "INVALID: BLOCKED \u{2014} policy rule git.injected_canary (block)",
    );
    let (code, err) = push_hook(local.path(), "dest", &[("feat/x", &head)]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("SYNTHETIC_CONTENT_CANARY"),
        "the finding quotes it: {err}"
    );
    let events = refusal_events(local.path());
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        events[0]["rules"],
        serde_json::json!(["git.test_gate_on_push", "git.commit_format"])
    );
    let text = refusal_ledger_text(local.path());
    for content in ["CANARY", "canary", "src/", "INVALID"] {
        assert!(!text.contains(content), "{content:?} in {text}");
    }
}

/// Hold an exclusive `flock` on `path` until the returned file is dropped,
/// as another writer of the ledger would.
#[cfg(unix)]
fn hold_lock(path: &Path) -> std::fs::File {
    use std::os::unix::io::AsRawFd;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .unwrap();
    // SAFETY: flock on a descriptor this function owns.
    assert_eq!(unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) }, 0);
    file
}

// TSK-149 review round 1, P2: a lock another process holds on the refusals
// ledger delays a hook or guard by at most the bounded wait, and never
// changes its verdict: first for the recording marker of an allowed call,
// then for the record of a refused one.
#[cfg(unix)]
#[test]
fn a_held_ledger_lock_neither_stalls_nor_changes_a_verdict() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    let ledger = dir.path().join(".git/codeflow/ledger/refusals");
    let held = hold_lock(&ledger.join("refusals.jsonl.lock"));

    let started = std::time::Instant::now();
    let allowed = guard_run("git status", dir.path());
    assert_eq!(allowed.status.code(), Some(0));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(
        !ledger.join("refusals.jsonl").exists(),
        "the marker waits for no lock"
    );

    std::fs::write(
        ledger.join("refusals.jsonl"),
        "{\"event\":\"refusal_recording_started\",\"timestamp\":\"2026-09-28T00:00:00Z\"}\n",
    )
    .unwrap();
    let started = std::time::Instant::now();
    let refused = guard_run("git push origin main", dir.path());
    let err = String::from_utf8_lossy(&refused.stderr).to_string();
    assert_eq!(refused.status.code(), Some(2), "{err}");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(
        err.contains("refusal not recorded")
            && err.contains("end the process that holds the lock on the ledger file"),
        "{err}"
    );
    assert!(refusal_events(dir.path()).is_empty());

    drop(held);
    let refused = guard_run("git push origin main", dir.path());
    assert_eq!(refused.status.code(), Some(2));
    assert_eq!(refusal_events(dir.path()).len(), 1);
}

// ---------------------------------------------------------------------------
// TSK-141 AC-4 (SPC-013 R-85 as amended): a hook that git fires while a
// codeflow command runs dispatches to that same codeflow binary, never to
// another `codeflow` found first on PATH.

/// A `codeflow` stub for PATH. `refuse` answers the capability probe as an
/// older binary and fails every hook; otherwise it records each call in
/// `called` and succeeds.
#[cfg(unix)]
fn path_stub(dir: &Path, refuse: bool) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let marker = dir.join("called");
    let body = if refuse {
        "if [ \"$1 $2\" = \"git-hook capabilities\" ]; then echo 'hooks 1'; exit 0; fi\n\
         echo \"stub codeflow on PATH refused: $*\" >&2\nexit 1\n"
            .to_string()
    } else {
        format!("echo \"$*\" >> '{}'\nexit 0\n", marker.display())
    };
    let stub = dir.join("codeflow");
    std::fs::write(&stub, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    marker
}

/// PATH with `first` ahead of the built binary's directory.
#[cfg(unix)]
fn path_with(first: &[&Path]) -> std::ffi::OsString {
    let exe = Path::new(env!("CARGO_BIN_EXE_codeflow"));
    std::env::join_paths(
        first
            .iter()
            .map(|dir| dir.to_path_buf())
            .chain(exe.parent().map(Path::to_path_buf))
            .chain(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            )),
    )
    .unwrap()
}

/// Run `binary` with PATH set to `path` and the test's isolation.
#[cfg(unix)]
fn codeflow_at(binary: &Path, dir: &Path, path: &std::ffi::OsStr, args: &[&str]) -> Output {
    let mut cmd = Command::new(binary);
    cmd.env("CODEFLOW_HOME", isolated_home())
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Hooks")
        .env("GIT_AUTHOR_EMAIL", "hooks@example.test")
        .env("GIT_COMMITTER_NAME", "Hooks")
        .env("GIT_COMMITTER_EMAIL", "hooks@example.test")
        .env_remove("CODEFLOW_HOOK_BINARY")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .args(args)
        .current_dir(dir);
    cmd.output().unwrap()
}

/// Git with PATH set to `path`, outside any codeflow command.
#[cfg(unix)]
fn git_at(dir: &Path, path: &std::ffi::OsStr, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("PATH", path)
        .env("CODEFLOW_HOME", isolated_home())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Hooks")
        .env("GIT_AUTHOR_EMAIL", "hooks@example.test")
        .env("GIT_COMMITTER_NAME", "Hooks")
        .env("GIT_COMMITTER_EMAIL", "hooks@example.test")
        .env_remove("CODEFLOW_HOOK_BINARY")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap()
}

#[cfg(unix)]
fn both(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[cfg(unix)]
const REGISTRY_LINE: &str = "integration/EPC-001-dispatch";

/// A full-tier project on a line pushed to a bare remote, its registry
/// seeded. Returns (tempdir, project).
#[cfg(unix)]
fn registry_project() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let plain = path_with(&[]);
    let exe = Path::new(env!("CARGO_BIN_EXE_codeflow"));
    let bare = dir.path().join("remote.git");
    let ok = |out: Output, what: &str| assert!(out.status.success(), "{what}: {}", both(&out));
    ok(
        git_at(
            dir.path(),
            &plain,
            &["init", "-q", "--bare", "-b", "main", "remote.git"],
        ),
        "bare",
    );
    let root = dir.path().join("proj");
    std::fs::create_dir(&root).unwrap();
    ok(
        codeflow_at(exe, &root, &plain, &["init", "--yes", "--full"]),
        "init",
    );
    ok(
        git_at(&root, &plain, &["switch", "-q", "-c", REGISTRY_LINE]),
        "line",
    );
    ok(
        git_at(
            &root,
            &plain,
            &["remote", "add", "origin", bare.to_str().unwrap()],
        ),
        "remote",
    );
    ok(
        git_at(&root, &plain, &["push", "-q", "origin", REGISTRY_LINE]),
        "push",
    );
    ok(
        codeflow_at(exe, &root, &plain, &["ids", "seed"]),
        "ids seed",
    );
    ok(
        codeflow_at(exe, &root, &plain, &["epic", "new", "dispatch outcome"]),
        "epic new",
    );
    (dir, root)
}

/// `task new` pushes its reservation to the registry, and the pre-push hook
/// git fires runs the calling binary, although an older `codeflow` that
/// refuses the registry push is first on PATH. The control shows that stub
/// does refuse when a hook is dispatched by PATH.
#[cfg(unix)]
#[test]
fn task_new_issues_through_the_registry_push_with_an_older_codeflow_first_on_path() {
    let (dir, root) = registry_project();
    let older = dir.path().join("older");
    path_stub(&older, true);
    let path = path_with(&[&older]);
    let exe = Path::new(env!("CARGO_BIN_EXE_codeflow"));

    // Control: the same hook dispatched by PATH, outside codeflow, runs the
    // stub and fails.
    let control = git_at(&root, &path, &["push", "-q", "origin", REGISTRY_LINE]);
    assert!(!control.status.success(), "{}", both(&control));
    assert!(
        both(&control).contains("missing, older or failed"),
        "{}",
        both(&control)
    );

    let out = codeflow_at(
        exe,
        &root,
        &path,
        &[
            "task",
            "new",
            "--epic",
            "EPC-001",
            "--into",
            REGISTRY_LINE,
            "dispatched",
        ],
    );
    assert!(out.status.success(), "{}", both(&out));
    assert!(both(&out).contains("TSK-001"), "{}", both(&out));
    assert!(!both(&out).contains("stub codeflow"), "{}", both(&out));
}

/// The designated binary is the caller's, whatever the environment offers:
/// a poisoned inherited value, another build first on PATH, a launcher
/// shim on PATH, or a caller whose path holds a space.
#[cfg(unix)]
#[test]
fn a_hook_runs_the_calling_binary_and_never_another_codeflow() {
    let (dir, root) = registry_project();
    let exe = Path::new(env!("CARGO_BIN_EXE_codeflow"));
    let other_build = dir.path().join("other-worktree/target/debug");
    let other_called = path_stub(&other_build, false);
    let launcher = dir.path().join("shims");
    let launcher_called = path_stub(&launcher, false);
    let refusing = dir.path().join("older");
    path_stub(&refusing, true);

    // A poisoned inherited value names the refusing stub.
    let path = path_with(&[&other_build]);
    let mut cmd = Command::new(exe);
    cmd.env("CODEFLOW_HOME", isolated_home())
        .env("PATH", &path)
        .env("CODEFLOW_HOOK_BINARY", refusing.join("codeflow"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Hooks")
        .env("GIT_AUTHOR_EMAIL", "hooks@example.test")
        .env("GIT_COMMITTER_NAME", "Hooks")
        .env("GIT_COMMITTER_EMAIL", "hooks@example.test")
        .args([
            "task",
            "new",
            "--epic",
            "EPC-001",
            "--into",
            REGISTRY_LINE,
            "poisoned",
        ])
        .current_dir(&root);
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "poisoned value: {}", both(&out));
    assert!(!both(&out).contains("stub codeflow"), "{}", both(&out));

    // A launcher shim first on PATH.
    let out = codeflow_at(
        exe,
        &root,
        &path_with(&[&launcher]),
        &[
            "task",
            "new",
            "--epic",
            "EPC-001",
            "--into",
            REGISTRY_LINE,
            "launcher",
        ],
    );
    assert!(out.status.success(), "launcher: {}", both(&out));

    // A caller whose path holds a space, with the refusing stub on PATH.
    let spaced = dir.path().join("bin dir");
    std::fs::create_dir_all(&spaced).unwrap();
    let copy = spaced.join("codeflow");
    std::fs::copy(exe, &copy).unwrap();
    let out = codeflow_at(
        &copy,
        &root,
        &path_with(&[&refusing]),
        &[
            "task",
            "new",
            "--epic",
            "EPC-001",
            "--into",
            REGISTRY_LINE,
            "spaced",
        ],
    );
    assert!(out.status.success(), "spaced path: {}", both(&out));

    assert!(!other_called.exists(), "another build ran a hook");
    assert!(!launcher_called.exists(), "a launcher shim ran a hook");
}

/// A designated binary that is missing or not executable fails the hook;
/// it never falls back to PATH or to a no-op.
#[cfg(unix)]
#[test]
fn a_missing_or_non_executable_designated_binary_fails_the_hook() {
    let (dir, root) = registry_project();
    let plain = path_with(&[]);
    let not_executable = dir.path().join("codeflow-not-executable");
    std::fs::write(&not_executable, "#!/bin/sh\nexit 0\n").unwrap();
    for designated in [dir.path().join("missing/codeflow"), not_executable] {
        let out = Command::new("git")
            .args(["push", "-q", "origin", REGISTRY_LINE])
            .current_dir(&root)
            .env("PATH", &plain)
            .env("CODEFLOW_HOME", isolated_home())
            .env("CODEFLOW_HOOK_BINARY", &designated)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert!(
            !out.status.success(),
            "{}: {}",
            designated.display(),
            both(&out)
        );
        assert!(
            both(&out).contains("missing, older or failed"),
            "{}: {}",
            designated.display(),
            both(&out)
        );
    }
}

/// The value is set only in the calling binary's git children: a child
/// that is not git, such as a test target, inherits neither the value the
/// caller set nor one it inherited.
#[cfg(unix)]
#[test]
fn a_child_that_is_not_git_never_inherits_the_designated_binary() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init_repo(root, "feat/dispatch");
    let seen = root.join("seen.txt");
    std::fs::create_dir_all(root.join(".codeflow")).unwrap();
    std::fs::write(
        root.join(".codeflow/test-config.json"),
        format!(
            r#"{{"schema_version":"1.0","targets":[{{"name":"probe","enabled":true,"runner":"custom","modes":{{"full":{{"command":"if [ -n \"${{CODEFLOW_HOOK_BINARY+set}}\" ]; then echo set > '{}'; else echo unset > '{}'; fi"}}}}}}]}}"#,
            seen.display(),
            seen.display()
        ),
    )
    .unwrap();
    let out = codeflow()
        .args(["test"])
        .current_dir(root)
        .env("CODEFLOW_HOOK_BINARY", "/poisoned/codeflow")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", both(&out));
    assert_eq!(std::fs::read_to_string(&seen).unwrap().trim(), "unset");
}

// -- conflict markers through the installed hooks (TSK-170 AC-6) -------------

/// Run `program` in `dir` with the binary under test first on `PATH`, so
/// the installed hook shims run it, and no user git configuration.
#[cfg(unix)]
fn with_installed_hooks(program: &str, dir: &Path, args: &[&str]) -> Output {
    let bin = Path::new(env!("CARGO_BIN_EXE_codeflow")).parent().unwrap();
    let path = std::env::join_paths(std::iter::once(bin.to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    Command::new(program)
        .args(args)
        .current_dir(dir)
        .env("PATH", path)
        .env("CODEFLOW_HOME", isolated_home())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env("GIT_EDITOR", "true")
        .env_remove("CODEFLOW_HOOK_BINARY")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_EVENT_NAME")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("BITBUCKET_PR_ID")
        .output()
        .unwrap()
}

#[cfg(unix)]
fn both_streams(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[cfg(unix)]
fn ok(program: &str, dir: &Path, args: &[&str]) -> String {
    let out = with_installed_hooks(program, dir, args);
    let text = both_streams(&out);
    assert!(out.status.success(), "{program} {args:?}: {text}");
    text
}

/// A marker line built at run time, so this file holds none itself.
#[cfg(unix)]
fn marker_line(fill: char, size: usize, label: &str) -> String {
    format!("{}{label}", fill.to_string().repeat(size))
}

#[cfg(unix)]
fn ci_over(dir: &Path, branch: &str, base: &str) -> Output {
    with_installed_hooks(
        env!("CARGO_BIN_EXE_codeflow"),
        dir,
        &["ci", "--base", base, "--head", "HEAD", "--branch", branch],
    )
}

/// Two branches that change the same line of `shared.txt` differently.
#[cfg(unix)]
fn diverged(root: &Path, base: &str, ours: &str, theirs: &str) {
    ok("git", root, &["switch", "-q", "-c", theirs, base]);
    std::fs::write(root.join("shared.txt"), "theirs\n").unwrap();
    ok(
        "git",
        root,
        &["commit", "-q", "-am", "feat: change the line there"],
    );
    ok("git", root, &["switch", "-q", "-c", ours, base]);
    std::fs::write(root.join("shared.txt"), "ours\n").unwrap();
    ok(
        "git",
        root,
        &["commit", "-q", "-am", "feat: change the line here"],
    );
}

#[cfg(unix)]
#[test]
fn installed_hooks_refuse_conflict_markers_and_ci_catches_what_they_cannot() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    std::fs::create_dir(&root).unwrap();
    ok(
        env!("CARGO_BIN_EXE_codeflow"),
        &root,
        &["init", "--yes", "--minimal"],
    );
    ok("git", &root, &["switch", "-q", "-c", "feat/seed"]);
    std::fs::write(root.join("shared.txt"), "line\n").unwrap();
    ok("git", &root, &["add", "shared.txt"]);
    ok(
        "git",
        &root,
        &["commit", "-q", "-m", "feat: add the shared line"],
    );

    // A conflicted merge concluded with `git commit` while markers remain is
    // refused by the pre-commit hook.
    diverged(&root, "feat/seed", "feat/merge", "feat/other");
    let merged = with_installed_hooks("git", &root, &["merge", "-q", "feat/other"]);
    assert!(!merged.status.success(), "the merge conflicts");
    ok("git", &root, &["add", "shared.txt"]);
    let refused = with_installed_hooks("git", &root, &["commit", "--no-edit"]);
    let text = both_streams(&refused);
    assert!(!refused.status.success(), "{text}");
    assert!(
        text.contains("BLOCKED — policy rule git.conflict_markers (block)"),
        "{text}"
    );
    assert!(
        text.contains("shared.txt:1 adds an unresolved opening"),
        "{text}"
    );

    // The same content committed past the hook, kept on its own branch:
    // `codeflow ci` over that range refuses it.
    ok("git", &root, &["commit", "-q", "--no-verify", "--no-edit"]);
    ok("git", &root, &["branch", "feat/bypassed"]);
    ok("git", &root, &["reset", "-q", "--hard", "HEAD^"]);
    ok("git", &root, &["switch", "-q", "feat/bypassed"]);
    let out = ci_over(&root, "feat/bypassed", "feat/seed");
    let text = both_streams(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("shared.txt:1 adds an unresolved opening"),
        "{text}"
    );

    // With the markers resolved, the merge commits and the range passes.
    ok("git", &root, &["switch", "-q", "feat/merge"]);
    let merged = with_installed_hooks("git", &root, &["merge", "-q", "feat/other"]);
    assert!(!merged.status.success(), "the merge conflicts again");
    std::fs::write(root.join("shared.txt"), "ours and theirs\n").unwrap();
    ok("git", &root, &["add", "shared.txt"]);
    ok("git", &root, &["commit", "--no-edit"]);
    let out = ci_over(&root, "feat/merge", "feat/seed");
    assert_eq!(out.status.code(), Some(0), "{}", both_streams(&out));

    // A Markdown underline and a fixture under conflict-marker-size=32
    // both commit through the hooks, and the range passes.
    ok(
        "git",
        &root,
        &["switch", "-q", "-c", "feat/fixtures", "feat/seed"],
    );
    std::fs::write(
        root.join("README.md"),
        format!("Title\n{}\n\nText.\n", marker_line('=', 7, "")),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("fixtures")).unwrap();
    std::fs::write(
        root.join("fixtures/conflict.txt"),
        format!(
            "{}\nours\n{}\ntheirs\n{}\n",
            marker_line('<', 7, " HEAD"),
            marker_line('=', 7, ""),
            marker_line('>', 7, " feat/y")
        ),
    )
    .unwrap();
    std::fs::write(
        root.join(".gitattributes"),
        "fixtures/** conflict-marker-size=32\n",
    )
    .unwrap();
    ok("git", &root, &["add", "-A"]);
    ok(
        "git",
        &root,
        &["commit", "-q", "-m", "test: add the conflict fixture"],
    );
    let out = ci_over(&root, "feat/fixtures", "feat/seed");
    assert_eq!(out.status.code(), Some(0), "{}", both_streams(&out));

    // A marker left while resolving `git rebase --continue` is committed
    // locally, since no pre-commit hook runs; `codeflow ci` refuses it.
    diverged(&root, "feat/seed", "feat/rebase", "feat/upstream");
    let rebased = with_installed_hooks("git", &root, &["rebase", "-q", "feat/upstream"]);
    assert!(!rebased.status.success(), "the rebase conflicts");
    ok("git", &root, &["add", "shared.txt"]);
    ok("git", &root, &["rebase", "--continue"]);
    let committed = std::fs::read_to_string(root.join("shared.txt")).unwrap();
    assert!(
        committed.starts_with(&marker_line('<', 7, " ")),
        "{committed}"
    );
    let out = ci_over(&root, "feat/rebase", "feat/seed");
    let text = both_streams(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("shared.txt:1 adds an unresolved opening"),
        "{text}"
    );
}

/// A fresh `--minimal` project on `feat/x` with a clean tracked `work.txt`
/// and `clean.txt`, its installed hooks running the binary under test.
#[cfg(unix)]
fn minimal_project(dir: &Path) -> std::path::PathBuf {
    let root = dir.join("p");
    std::fs::create_dir(&root).unwrap();
    ok(
        env!("CARGO_BIN_EXE_codeflow"),
        &root,
        &["init", "--yes", "--minimal"],
    );
    ok("git", &root, &["switch", "-q", "-c", "feat/x"]);
    std::fs::write(root.join("work.txt"), "plain\n").unwrap();
    std::fs::write(root.join("clean.txt"), "one\n").unwrap();
    ok("git", &root, &["add", "work.txt", "clean.txt"]);
    ok("git", &root, &["commit", "-q", "-m", "feat: add the files"]);
    root
}

#[cfg(unix)]
fn leftover_markers() -> String {
    format!(
        "{}\nours\n{}\ntheirs\n{}\n",
        marker_line('<', 7, " HEAD"),
        marker_line('=', 7, ""),
        marker_line('>', 7, " other")
    )
}

/// TSK-170 review P1: `git commit -a` and `git commit <path>` commit a
/// temporary index named by `GIT_INDEX_FILE`; the hook judges that index,
/// not the ordinary one.
#[cfg(unix)]
#[test]
fn installed_hook_judges_the_index_git_commits() {
    let dir = tempfile::tempdir().unwrap();
    let root = minimal_project(dir.path());
    let head = ok("git", &root, &["rev-parse", "HEAD"]);
    std::fs::write(root.join("work.txt"), leftover_markers()).unwrap();

    // Unstaged markers committed with `-a` are refused.
    let all = with_installed_hooks("git", &root, &["commit", "-am", "fix: work"]);
    let text = both_streams(&all);
    assert!(!all.status.success(), "commit -a: {text}");
    assert!(
        text.contains("work.txt:1 adds an unresolved opening"),
        "{text}"
    );

    // The same file named on the command line is refused too.
    let path = with_installed_hooks("git", &root, &["commit", "work.txt", "-m", "fix: work"]);
    let text = both_streams(&path);
    assert!(!path.status.success(), "commit <path>: {text}");
    assert!(
        text.contains("work.txt:1 adds an unresolved opening"),
        "{text}"
    );
    assert_eq!(ok("git", &root, &["rev-parse", "HEAD"]), head);

    // A clean selected path commits while unrelated staged markers, which
    // that commit leaves out, stay staged and are not judged.
    ok("git", &root, &["checkout", "--", "work.txt"]);
    std::fs::write(root.join("staged.txt"), leftover_markers()).unwrap();
    ok("git", &root, &["add", "staged.txt"]);
    std::fs::write(root.join("clean.txt"), "one\ntwo\n").unwrap();
    let clean = with_installed_hooks("git", &root, &["commit", "clean.txt", "-m", "fix: clean"]);
    assert!(clean.status.success(), "{}", both_streams(&clean));
    let committed = ok("git", &root, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(committed.trim(), "clean.txt");
    let staged = ok("git", &root, &["diff", "--cached", "--name-only"]);
    assert_eq!(staged.trim(), "staged.txt");
}

/// TSK-170 review P1: an index the hook cannot read is reported at the
/// configured level, never passed. The secret scan, which has no lower
/// level, refuses it too, so the commit stops either way.
#[cfg(unix)]
#[test]
fn an_unreadable_commit_index_is_reported_at_the_configured_level() {
    let dir = tempfile::tempdir().unwrap();
    let root = minimal_project(dir.path());
    let bad = dir.path().join("broken-index");
    std::fs::write(&bad, "not an index").unwrap();
    for (level, code, verdict) in [("block", 1, "BLOCKED"), ("warn", 1, "warning")] {
        let mut policy: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(".codeflow/policy.json")).unwrap(),
        )
        .unwrap();
        policy["git"]["conflict_markers"] = level.into();
        std::fs::write(
            root.join(".codeflow/policy.json"),
            serde_json::to_string_pretty(&policy).unwrap(),
        )
        .unwrap();
        let out = codeflow()
            .args(["git-hook", "pre-commit"])
            .current_dir(&root)
            .env("GIT_INDEX_FILE", &bad)
            .output()
            .unwrap();
        let text = both_streams(&out);
        assert_eq!(out.status.code(), Some(code), "{level}: {text}");
        assert!(
            text.contains(&format!(
                "{verdict} — policy rule git.conflict_markers ({level})"
            )),
            "{level}: {text}"
        );
        assert!(text.contains("cannot read the index"), "{level}: {text}");
        assert!(
            text.contains("BLOCKED — policy rule git.secret_scan (block)")
                && text.contains("staged secret scan incomplete"),
            "{level}: {text}"
        );
    }
}

/// TSK-170 review follow-up (security): the secret scan judges the index git
/// commits too, so a key committed with `git commit -a` or `git commit
/// <path>` without staging it first is refused and never reaches HEAD.
#[cfg(unix)]
#[test]
fn installed_hook_refuses_a_secret_in_the_index_git_commits() {
    let dir = tempfile::tempdir().unwrap();
    let root = minimal_project(dir.path());
    let head = ok("git", &root, &["rev-parse", "HEAD"]);
    // Built at run time, so this file carries no key shape itself.
    let key = format!("AKIA{}", "IOSFODNN7EXAMPLF");
    std::fs::write(root.join("work.txt"), format!("key = {key}\n")).unwrap();
    for args in [
        &["commit", "-am", "feat: add the key"][..],
        &["commit", "work.txt", "-m", "feat: add the key"][..],
    ] {
        let out = with_installed_hooks("git", &root, args);
        let text = both_streams(&out);
        assert!(!out.status.success(), "{args:?}: {text}");
        assert!(text.contains("possible secret"), "{args:?}: {text}");
        assert_eq!(ok("git", &root, &["rev-parse", "HEAD"]), head, "{args:?}");
        let committed = ok("git", &root, &["show", "HEAD:work.txt"]);
        assert!(!committed.contains(&key), "{args:?}: the key reached HEAD");
    }
}

// --- TSK-041 CLI help-string regressions --------------------------------------

/// Run `codeflow <args>` purely for its help output, with no repo context.
fn help_text(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("run codeflow {args:?}: {e}"));
    assert!(
        output.status.success(),
        "codeflow {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 help output")
}

/// The four hook events `delegate-turn` actually dispatches on
/// (`codeflow_core::delegate::handle_hook`).
const DELEGATE_TURN_EVENTS: [&str; 4] = ["SessionStart", "UserPromptSubmit", "Stop", "StopFailure"];

/// DEFECT 4: `codeflow hook --help` names every delegate event. The help used
/// to advertise only `Stop`/`StopFailure`, hiding the schema-v2 lifecycle
/// events a caller must wire.
#[test]
fn hook_help_names_every_delegate_turn_event() {
    let help = help_text(&["hook", "--help"]);
    let line = help
        .lines()
        .find(|line| line.trim_start().starts_with("- delegate-turn:"))
        .unwrap_or_else(|| panic!("hook --help has no delegate-turn entry:\n{help}"));
    for event in DELEGATE_TURN_EVENTS {
        assert!(
            line.contains(event),
            "hook --help delegate-turn entry omits {event}: {line}"
        );
    }
    // NEGATIVE: the assertion is not satisfied by a line that drops one event.
    let degraded = line.replace("UserPromptSubmit", "");
    assert!(
        !DELEGATE_TURN_EVENTS.iter().all(|e| degraded.contains(e)),
        "the check would pass with an event removed"
    );
}

/// DEFECT 5: `doctor --help` is generated from the check registry. Every
/// registered check name appears; nothing is hand-listed beside it.
#[test]
fn doctor_help_equals_the_check_registry() {
    let help = help_text(&["doctor", "--help"]);
    let registered = codeflow_core::doctor::check_names();
    assert!(!registered.is_empty(), "check registry is empty");

    let about = help
        .lines()
        .find(|line| line.starts_with("Health checks:"))
        .unwrap_or_else(|| panic!("doctor --help has no derived about line:\n{help}"));
    let listed: Vec<&str> = about
        .trim_start_matches("Health checks:")
        .split(". See")
        .next()
        .expect("about line has a check list")
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    assert_eq!(
        listed, registered,
        "doctor --help must list exactly the registry's check names, in order"
    );

    // `doctor --list` is the registry's own rendering; help and list agree.
    let listing = help_text(&["doctor", "--list"]);
    for name in &registered {
        assert!(
            listing.contains(name),
            "doctor --list omits registered check {name}"
        );
    }
}

/// DEFECT 9: the `git-hook` help names the path the install code writes the
/// shims to — compared against the constant that code uses, not a literal.
#[test]
fn git_hook_help_matches_the_install_path_constant() {
    let installed = codeflow_core::scaffold::detect::CODEFLOW_HOOKS_PATH;
    let help = help_text(&["--help"]);
    let line = help
        .lines()
        .find(|line| line.trim_start().starts_with("git-hook"))
        .unwrap_or_else(|| panic!("codeflow --help has no git-hook row:\n{help}"));
    assert!(
        line.contains(installed),
        "git-hook help must name {installed}, got: {line}"
    );
    // NEGATIVE: the old, wrong path must not reappear.
    assert!(
        !line.contains(".git/hooks"),
        "git-hook help names .git/hooks, which the install code does not use"
    );
}
