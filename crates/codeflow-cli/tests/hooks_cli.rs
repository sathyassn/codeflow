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
    write_policy(
        &session,
        r#"{"git":{"protected_branches":["main","master"]}}"#,
    );
    write_policy(
        &scratch,
        r#"{"git":{"protected_branches":["main","master"]}}"#,
    );
    let s = scratch.to_string_lossy();
    let b = bare.to_string_lossy();
    let sg = session.join(".git");
    let sg = sg.to_string_lossy();

    for allowed in [
        format!("git -C {s} commit -m 'feat: x'"),
        format!("R={s}; git -C \"$R\" commit -m 'feat: x'"),
        format!(
            "cd {} && git -C scratch commit -m 'feat: x'",
            tmp.path().display()
        ),
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
    // A directory literally named `$R` inside the session, a repository on main.
    let literal = session.join("$R");
    std::fs::create_dir_all(&literal).unwrap();
    init_repo(&literal, "main");
    let f = feature.to_string_lossy();
    let pg = protected.join(".git");
    let pg = pg.to_string_lossy();
    let absent = tmp.path().join("absent/out");
    let absent = absent.to_string_lossy();

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
    let p = protected.to_string_lossy();
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
        write_policy(
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
        write_policy(dir, r#"{"git":{"protected_branches":["main","master"]}}"#);
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
        write_policy(&dir, r#"{"git":{"protected_branches":["main","master"]}}"#);
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
        let command = command.replace("ROOT", &dir.to_string_lossy());
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
fn exec_guard_allows_removal_below_temp_roots_and_blocks_system_paths() {
    // TSK-137 AC-5: an agent's scratch space sits below `/private` and
    // `/var` on macOS; removal there is allowed, the same command on a
    // system directory is still blocked.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");
    let tmpdir = "/private/var/folders/ab/cd123/T/";
    let guard = |command: &str| {
        run_with_stdin(
            codeflow()
                .args(["hook", "exec-guard"])
                .env("TMPDIR", tmpdir)
                .current_dir(dir.path()),
            &guard_payload(command, dir.path()),
        )
    };
    for command in [
        "rm -rf /private/var/folders/ab/cd123/T/scratch",
        "rm -rf /var/folders/xy/zz9/T/build",
        "rm -rf /private/tmp/claude-501/work",
        "rm -rf /var/tmp/cache && ls",
    ] {
        let out = guard(command);
        assert!(
            out.status.success(),
            "should allow: {command}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    for command in [
        "rm -rf /private/var/db/receipts",
        "rm -rf /var/lib/dpkg",
        "rm -rf /private/tmp",
        "rm -rf /private/var/folders/ab/cd123/T",
        "rm -rf /private/tmp/claude-501/../../etc",
    ] {
        let out = guard(command);
        assert_eq!(out.status.code(), Some(2), "should block: {command}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("security.dangerous_commands"));
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

fn push_hook(dir: &Path, remote: &str, refs: &[(&str, &str)]) -> (Option<i32>, String) {
    let stdin = refs
        .iter()
        .map(|(branch, sha)| format!("refs/heads/{branch} {sha} refs/heads/{branch} {ZERO_SHA}"))
        .collect::<Vec<_>>()
        .join("\n");
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-push", remote, remote])
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
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-push", remote, remote])
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
fn push_set_checks_a_rewrite_from_the_destination_sha_and_says_so() {
    // A branch the destination holds is rebased onto a destination commit it
    // did not have. The range starts at the destination's advertised sha, so
    // the commit the rebase brought in is checked again, and the hook says
    // why: no cached tracking ref can prove it is still on the destination.
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
            && err.contains("checks all 2 commit(s) not on it"),
        "{err}"
    );

    // A bad commit in the rewrite is blocked.
    let bad = commit_file(local.path(), "s.txt", "s\n", "Not conventional.");
    let (code, err) = push_hook_onto(local.path(), "dest", "feat/r", &bad, &old);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("push set check failed: `codeflow ci"), "{err}");
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

#[test]
fn push_set_blocks_an_unrelated_base_and_notes_an_unresolved_one() {
    // T132-3: a resolved but unrelated base (an orphan pushed over a branch
    // the destination holds) is a failed check with its diagnostic.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    let main = rev(dir.path(), "main");
    git(dir.path(), &["checkout", "-q", "--orphan", "feat/orphan"]);
    let orphan = commit_file(dir.path(), "o.txt", "o\n", "chore: start an orphan");
    let (code, err) = push_hook_onto(dir.path(), "origin", "feat/orphan", &orphan, &main);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("push set check failed: `codeflow ci"), "{err}");
    assert!(!err.contains("range unresolved"), "{err}");

    // No destination sha and no tracking history: a note, never a pass.
    let lone = tempfile::tempdir().unwrap();
    init_repo(lone.path(), "feat/lone");
    let head = rev(lone.path(), "HEAD");
    let (code, err) = push_hook(lone.path(), "origin", &[("feat/lone", &head)]);
    assert_eq!(code, Some(0), "{err}");
    assert!(
        err.contains("`codeflow ci` did not run for 'feat/lone': range unresolved"),
        "{err}"
    );
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
