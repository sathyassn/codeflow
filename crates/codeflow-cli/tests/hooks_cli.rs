//! End-to-end tests for the hook plane CLI surface:
//! `codeflow hook <git-guard|session-orient|session-summary>`,
//! `codeflow git-hook <pre-commit|commit-msg|pre-push>`, `codeflow orient`.
//!
//! Each test runs the real binary (`CARGO_BIN_EXE_codeflow`) in a tempdir
//! git repo, exercising the exit-code contracts the hooks rely on.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

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
        .write_all(stdin.as_bytes())
        .expect("stdin written");
    child.wait_with_output().expect("binary exits")
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
// codeflow hook git-guard
// ---------------------------------------------------------------------------

#[test]
fn git_guard_blocks_push_to_protected_with_exit_2() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");

    let payload = guard_payload("git push origin main", dir.path());
    let out = run_with_stdin(
        codeflow().args(["hook", "git-guard"]).current_dir(dir.path()),
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
        codeflow().args(["hook", "git-guard"]).current_dir(dir.path()),
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
        codeflow().args(["hook", "git-guard"]).current_dir(dir.path()),
        &payload,
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("git.commit_to_protected"),
    );
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
            codeflow().args(["hook", "git-guard"]).current_dir(dir.path()),
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
    let payload = guard_payload("git update-ref refs/remotes/origin/main deadbeef", dir.path());
    let out = run_with_stdin(
        codeflow().args(["hook", "git-guard"]).current_dir(dir.path()),
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
        codeflow().args(["hook", "exec-guard"]).current_dir(dir.path()),
        &payload,
    );
    assert_eq!(out.status.code(), Some(2), "dangerous command must block with exit 2");
    assert!(String::from_utf8_lossy(&out.stderr).contains("security.dangerous_commands"));
}

#[test]
fn git_guard_blocks_pr_body_attribution() {
    // AC #13: attribution in a PR body blocked at gh pr create.
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "feat/x");

    let cmd = "gh pr create --title 'feat: x' --body 'Generated with Claude Code'";
    let out = run_with_stdin(
        codeflow().args(["hook", "git-guard"]).current_dir(dir.path()),
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
        codeflow().args(["hook", "git-guard"]).current_dir(dir.path()),
        r#"{"tool_name":"Write","tool_input":{"file_path":"x"}}"#,
    );
    assert_eq!(out.status.code(), Some(0));

    let out = run_with_stdin(
        codeflow().args(["hook", "git-guard"]).current_dir(dir.path()),
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
    std::fs::write(&msg, "feat: x\n\nCo-Authored-By: Claude <noreply@anthropic.com>\n").unwrap();
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
            .args(["git-hook", "pre-push", "origin", "https://example.com/r.git"])
            .current_dir(dir.path()),
        &stdin,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("git.push_to_protected"));

    // Feature branch with sanctioned prefix passes (test gate note allowed).
    let stdin = format!("refs/heads/feat/x abc123 refs/heads/feat/x {ZERO}\n");
    let out = run_with_stdin(
        codeflow()
            .args(["git-hook", "pre-push", "origin", "https://example.com/r.git"])
            .current_dir(dir.path()),
        &stdin,
    );
    assert_eq!(out.status.code(), Some(0));
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
    assert_eq!(out.status.code(), Some(0), "human override passes the git layer");

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
    assert!(!ff.status.success(), "ff-merge into protected is now refused");
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
    assert!(clone.status.success(), "clone: {}", String::from_utf8_lossy(&clone.stderr));
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
    assert!(!reset.status.success(), "reset --hard on protected is refused");
    assert!(String::from_utf8_lossy(&reset.stderr).contains("git.local_ref_protection"));

    // branch -D of a protected branch is refused (via delete_protected).
    let del = git_run(&["branch", "-D", "master"]);
    assert!(!del.status.success(), "branch -D of a protected branch is refused");
    assert!(String::from_utf8_lossy(&del.stderr).contains("git.delete_protected"));
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
        format!("#!/bin/sh\nexec '{}' git-hook pre-commit \"$@\"\n", env!("CARGO_BIN_EXE_codeflow")),
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
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("git.commit_to_protected"),
    );
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
    assert_eq!(out.status.code(), Some(1), "disarm flag is inert once committed");
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
    assert_eq!(out.status.code(), Some(0), "grace suspends branch rules pre-commit");

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
