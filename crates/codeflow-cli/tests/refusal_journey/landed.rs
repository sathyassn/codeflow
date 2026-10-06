fn binary() -> String {
    std::env::var("TSK189_TEST_BINARY").unwrap_or_else(|_| env!("CARGO_BIN_EXE_codeflow").into())
}

fn commands(value: &serde_json::Value, out: &mut Vec<String>) {
    if let Some(command) = value.get("command").and_then(|v| v.as_str()) {
        if command.contains("codeflow hook ") {
            out.push(command.into());
        }
    }
    if let Some(map) = value.as_object() {
        for v in map.values() {
            commands(v, out);
        }
    }
    if let Some(list) = value.as_array() {
        for v in list {
            commands(v, out);
        }
    }
}

use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

struct Repo {
    temp: tempfile::TempDir,
}
impl Repo {
    fn new() -> Self {
        let repo = Self {
            temp: tempfile::tempdir().unwrap(),
        };
        repo.git(&["init", "-q", "-b", "task/x"]);
        repo.git(&["config", "user.name", "Test"]);
        repo.git(&["config", "user.email", "test@example.invalid"]);
        repo.policy("off");
        repo.git(&["add", "."]);
        repo.git(&["commit", "-qm", "chore: fixture"]);
        repo
    }
    fn root(&self) -> &Path {
        self.temp.path()
    }
    fn command(&self, program: &str) -> Command {
        let mut c = Command::new(program);
        c.current_dir(self.root())
            .env("HOME", self.root())
            .env("XDG_CONFIG_HOME", self.root().join(".config"))
            .env(
                "PATH",
                std::env::join_paths(
                    [std::path::PathBuf::from(binary())
                        .parent()
                        .unwrap()
                        .to_path_buf()]
                    .into_iter()
                    .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
                )
                .unwrap(),
            )
            .env("CODEFLOW_HOME", self.root().join("state"))
            .env("GIT_CONFIG_GLOBAL", self.root().join("global.config"))
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE");
        c
    }
    fn git(&self, args: &[&str]) -> String {
        let o = self.command("git").args(args).output().unwrap();
        assert!(
            o.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).trim().into()
    }
    fn policy(&self, level: &str) {
        std::fs::create_dir_all(self.root().join(".codeflow")).unwrap();
        std::fs::write(
            self.root().join(".codeflow/policy.json"),
            json!({"security":{"headless_peer_runs":level}}).to_string(),
        )
        .unwrap();
    }
    fn remote(&self) {
        self.git(&["init", "-q", "--bare", "remote.git"]);
        self.git(&[
            "remote",
            "add",
            "origin",
            self.root().join("remote.git").to_str().unwrap(),
        ]);
    }
    fn landed(&self) {
        self.remote();
        self.policy("block");
        self.git(&["add", ".codeflow/policy.json"]);
        self.git(&["commit", "-qm", "chore: landed policy"]);
        self.git(&["push", "-q", "origin", "HEAD:main"]);
        self.git(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ]);
        self.policy("off");
    }
    fn hook(&self, hook: &str, command: &str) -> Output {
        run_hook(self.command(&binary()), self.root(), hook, command)
    }
    fn check(&self, hook: &str, cmd: &str, code: i32) -> Output {
        let out = self.hook(hook, cmd);
        assert_eq!(
            out.status.code(),
            Some(code),
            "{cmd}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }
}

#[test]
fn ac1_landed_policy_ignores_worktree_index_head_and_local_branch() {
    let repo = Repo::new();
    repo.landed();
    for state in 0..4 {
        if state == 1 {
            repo.git(&["add", ".codeflow/policy.json"]);
        }
        if state == 2 {
            repo.git(&["commit", "-qm", "chore: local policy"]);
        }
        if state == 3 {
            repo.git(&["switch", "-qc", "feat/weak"]);
        }
        let out = repo.check("exec-guard", "claude -p hello", 2);
        assert!(String::from_utf8_lossy(&out.stderr).contains("refs/remotes/origin/main"));
    }
}

#[test]
fn ac1_namespace_bootstrap_and_ac2_missing_authority() {
    let repo = Repo::new();
    repo.check("exec-guard", "claude -p hello", 0);
    repo.remote();
    repo.check("exec-guard", "claude -p hello", 0);
    repo.check("git-guard", "git push origin task/x", 0);
    repo.policy("block");
    repo.git(&["add", ".codeflow/policy.json"]);
    repo.git(&["commit", "-qm", "chore: policy"]);
    let out = repo.check("exec-guard", "claude -p hello", 2);
    assert!(String::from_utf8_lossy(&out.stderr).contains("before the first fetch of origin"));
    repo.git(&["push", "-q", "origin", "HEAD:trunk"]);
    let out = repo.check("exec-guard", "echo ok", 2);
    assert!(String::from_utf8_lossy(&out.stderr).contains("git remote set-head origin --auto"));
    repo.check("git-guard", "git fetch origin", 0);
    repo.check("exec-guard", "git fetch origin", 0);
}

#[test]
fn ac2_last_tracking_ref_and_ref_plumbing_are_protected() {
    let repo = Repo::new();
    repo.landed();
    let unique = repo.git(&["commit-tree", "HEAD^{tree}", "-m", "unique"]);
    repo.git(&["update-ref", "refs/heads/task/unique", &unique]);
    for cmd in [
        "git update-ref -d refs/remotes/origin/main",
        "git symbolic-ref refs/remotes/origin/HEAD refs/heads/task/x",
        "git branch -d -r origin/main",
        "git branch -Dr origin/main",
        "git remote remove origin",
        "git remote rename origin other",
        "git remote set-url origin elsewhere",
        "git remote set-head origin main",
        "git remote add origin elsewhere",
        "git config --global remote.origin.url elsewhere",
        "git config url.elsewhere.insteadOf here",
        "git config --local include.path x",
        "git fetch other main:refs/remotes/origin/main",
        "git fetch origin --refmap=",
        "git push . :refs/heads/x",
        "git update-ref -d refs/heads/task/unique",
        "rm -rf .git/refs/remotes",
        "printf x > .git/packed-refs",
        "printf x > .git/config",
    ] {
        repo.check("git-guard", cmd, 2);
    }
    for cmd in [
        "git fetch",
        "git fetch origin",
        "git push origin task/x",
        "git status",
        "git log",
        "git config user.name Test",
    ] {
        repo.check("git-guard", cmd, 0);
    }
}

#[test]
fn ac3_shims_fail_closed_and_harness_contract_is_wired() {
    use std::os::unix::fs::PermissionsExt;
    let repo = Repo::new();
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base");
    let bin = repo.root().join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::os::unix::fs::symlink("/usr/bin/grep", bin.join("grep")).unwrap();
    for stage in [
        "pre-commit",
        "commit-msg",
        "pre-push",
        "pre-merge-commit",
        "reference-transaction",
    ] {
        let shim = assets.join("git-hooks").join(stage);
        let out = repo
            .command("/bin/sh")
            .arg(&shim)
            .env("PATH", &bin)
            .env_remove("CODEFLOW_HOOK_BINARY")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1), "missing {stage}: {out:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("codeflow update"));
        for (cap, code, expected) in [
            ("hooks 3", 0, 0),
            ("hooks 3", 2, 1),
            ("hooks 3", 1, 1),
            ("hooks 3", 127, 1),
            ("hooks 2", 0, 1),
        ] {
            let fake = bin.join("codeflow");
            std::fs::write(&fake, format!("#!/bin/sh\nif [ \"$2\" = capabilities ]; then echo '{cap}'; exit 0; fi\nexit {code}\n")).unwrap();
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
            let out = repo
                .command("/bin/sh")
                .arg(&shim)
                .env("PATH", &bin)
                .env_remove("CODEFLOW_HOOK_BINARY")
                .output()
                .unwrap();
            assert_eq!(
                out.status.code(),
                Some(expected),
                "{stage} {cap} {code}: {out:?}"
            );
            std::fs::remove_file(fake).unwrap();
        }
    }
    wrappers_advise_reinstall_only_for_a_missing_binary(&repo, &assets, &bin);
}

/// The fake `codeflow` for a wrapper case: the hook prints `stderr`
/// (nothing when it is empty) and exits `code`; a negative `code` makes it
/// kill itself with that signal. `git-hook capabilities` leaves a mark and
/// then stalls, so a wrapper that calls the binary a second time is seen
/// and would hang.
fn fake_codeflow(bin: &Path, stderr: &str, code: i32) {
    use std::os::unix::fs::PermissionsExt;
    let fake = bin.join("codeflow");
    let say = if stderr.is_empty() {
        String::new()
    } else {
        format!("echo \"{stderr}\" >&2\n")
    };
    let end = if code < 0 {
        format!("kill -{} $$\n", -code)
    } else {
        format!("exit {code}\n")
    };
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\nif [ \"$1 $2\" = \"git-hook capabilities\" ]; then : > '{}'; exec /bin/sleep 30; fi\n{say}{end}",
            bin.join("probed").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// AC-3, and TSK-215 AC-2 (issue 29): every harness wrapper is
/// `codeflow hook <name> --contract 3` followed by one shared fallback with
/// no `$`, which Grok would read as its own template and skip. It exits 0
/// when the binary allows and 2 whenever the hook fails: a policy refusal
/// prints the guard's message first, a missing binary names the installer
/// and `codeflow update`, a binary older than contract 3 keeps its own
/// usage error, and any other failure still blocks. Every failure ends with
/// one closing line, so a binary that fails silently, or is killed, still
/// leaves the nonempty reason Codex needs before it blocks on exit 2
/// (TSK-215 round 1). The wrapper never calls the binary a second time, so
/// the fallback adds no wait of its own (the hook call itself can still
/// stall until the harness timeout), and it writes no temporary file.
fn wrappers_advise_reinstall_only_for_a_missing_binary(repo: &Repo, assets: &Path, bin: &Path) {
    let tmp = repo.root().join("wrapper-tmp");
    std::fs::create_dir(&tmp).unwrap();
    let mut shared_tail: Option<String> = None;
    for path in [
        "codex/hooks.json",
        "grok/hooks.json",
        "settings/default.json",
        "settings/acceptEdits.json",
        "settings/bypass-sandboxed.json",
    ] {
        let text = std::fs::read_to_string(assets.join(path)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut hooks = Vec::new();
        commands(&value, &mut hooks);
        assert!(!hooks.is_empty(), "{path}");
        for command in &hooks {
            assert!(!command.contains('$'), "{path}: {command}");
            let (head, tail) = command.split_once(" || ").unwrap();
            assert!(
                head.starts_with("codeflow hook ") && head.ends_with(" --contract 3"),
                "{path}: {command}"
            );
            assert_eq!(
                shared_tail.get_or_insert_with(|| tail.to_string()),
                tail,
                "{path}: {command}"
            );
            for (binary, expected, advised) in [
                (None, 2, true),
                (Some(("", 0)), 0, false),
                (Some(("codeflow git-guard: BLOCKED", 2)), 2, false),
                (
                    Some(("error: unexpected argument '--contract' found", 2)),
                    2,
                    false,
                ),
                (Some(("codeflow hook: internal error", 1)), 2, false),
                (Some(("codeflow: cannot execute", 126)), 2, false),
                (Some(("", 1)), 2, false),
                (Some(("", -9)), 2, false),
            ] {
                wrapper_case(repo, bin, &tmp, command, binary, expected, advised);
            }
        }
    }
}

/// One wrapper run against a fake binary (`None` for a missing one): the
/// expected exit code within the bound, advice exactly when `advised`, the
/// binary's own text kept, no temporary file and no second call.
fn wrapper_case(
    repo: &Repo,
    bin: &Path,
    tmp: &Path,
    command: &str,
    binary: Option<(&str, i32)>,
    expected: i32,
    advised: bool,
) {
    if let Some((stderr, code)) = binary {
        fake_codeflow(bin, stderr, code);
    }
    let started = std::time::Instant::now();
    let out = repo
        .command("/bin/sh")
        .args(["-c", command])
        .env("PATH", bin)
        .env("TMPDIR", tmp)
        .output()
        .unwrap();
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "{binary:?}: {elapsed:?}"
    );
    assert_eq!(out.status.code(), Some(expected), "{command}: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    // The harness decision, not just the exit code: Codex blocks on exit 2
    // only when stderr carries a nonempty reason; Claude and Grok block on
    // exit 2.
    if expected == 2 {
        assert!(
            !stderr.trim().is_empty(),
            "{binary:?} {command}: exit 2 with no reason"
        );
    }
    assert_eq!(
        stderr.contains("codeflow-cli-installer.sh"),
        advised,
        "{binary:?} {command}: {stderr}"
    );
    assert_eq!(stderr.contains("codeflow update"), advised, "{stderr}");
    assert_eq!(
        std::fs::read_dir(tmp).unwrap().count(),
        0,
        "wrapper wrote a temporary file"
    );
    if let Some((text, _)) = binary {
        assert!(stderr.contains(text), "{stderr}");
        assert!(
            !bin.join("probed").exists(),
            "{command}: the wrapper called the binary a second time"
        );
        std::fs::remove_file(bin.join("codeflow")).unwrap();
    }
}

#[test]
fn ac4_doctor_and_orient_report_policy_authority_and_drift() {
    let repo = Repo::new();
    repo.landed();
    for command in ["doctor", "orient"] {
        let mut c = repo.command(&binary());
        c.arg(command);
        if command == "doctor" {
            c.args(["--check", "policy-source"]);
        }
        let out = c.output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            text.contains("refs/remotes/origin/main"),
            "{command}: {text}"
        );
        assert!(text.contains("local policy differs"), "{command}: {text}");
    }
}

#[test]
fn ac2_other_checkout_and_common_dir_paths() {
    let repo = Repo::new();
    repo.landed();
    let other = repo.root().join("other");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feat/other",
        other.to_str().unwrap(),
    ]);
    for path in [
        other.join(".codeflow/policy.json"),
        other.join(".claude/settings.json"),
        other.join(".codex/hooks.json"),
        repo.root().join(".git/config"),
        repo.root().join(".git/packed-refs"),
    ] {
        repo.check("git-guard", &format!("printf x > '{}'", path.display()), 2);
        repo.check(
            "exec-guard",
            &format!("python -c \"open('{}', 'w').write('x')\"", path.display()),
            2,
        );
        let mut child = repo
            .command(&binary())
            .args(["hook", "edit-guard"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        write!(
            child.stdin.take().unwrap(),
            "{}",
            json!({"tool_name":"Write", "cwd":other, "tool_input":{"file_path":path,"content":"x"}})
        )
        .unwrap();
        let out = child.wait_with_output().unwrap();
        assert_eq!(out.status.code(), Some(2), "native edit: {out:?}");
    }
}

#[test]
fn ac1_target_levels_and_retargeted_repository_use_landed_refs() {
    let repo = Repo::new();
    repo.remote();
    repo.git(&["push", "-q", "origin", "HEAD:main"]);
    repo.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    repo.git(&["switch", "-qc", "integration/test"]);
    std::fs::create_dir_all(repo.root().join("project-management/tasks")).unwrap();
    std::fs::write(
        repo.root().join("project-management/tasks/TSK-001.md"),
        "---\nid: TSK-001\nintegration_target: integration/test\n---\n",
    )
    .unwrap();
    repo.policy("block");
    repo.git(&["add", ".codeflow/policy.json", "project-management"]);
    repo.git(&["commit", "-qm", "chore: strict line"]);
    repo.git(&["push", "-q", "origin", "HEAD:integration/test"]);
    repo.git(&["switch", "-qc", "task/TSK-001-impl"]);
    repo.policy("off");
    repo.check("exec-guard", "claude -p hello", 2);
    let target = Repo::new();
    target.landed();
    std::fs::write(
        target.root().join(".codeflow/policy.json"),
        r#"{"git":{"push_to_protected":"off"},"security":{"headless_peer_runs":"off"}}"#,
    )
    .unwrap();
    for cmd in [
        format!("git -C '{}' push origin main", target.root().display()),
        format!(
            "git --git-dir='{}' push origin main",
            target.root().join(".git").display()
        ),
        format!(
            "GIT_DIR='{}' git push origin main",
            target.root().join(".git").display()
        ),
    ] {
        repo.check("git-guard", &cmd, 2);
    }
    target.git(&["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"]);
    repo.check(
        "git-guard",
        &format!("git -C '{}' status", target.root().display()),
        0,
    );
}

#[test]
fn ac2_fetch_rewrites_and_last_ref_deletion_routes() {
    let repo = Repo::new();
    repo.landed();
    repo.git(&["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"]);
    // Check the last tracking ref without changing it through any agent path.
    for cmd in [
        "git update-ref -d refs/remotes/origin/main",
        "git branch -Dr origin/main",
        "git remote remove origin",
        "git remote rename origin other",
        "rm -rf .git/refs/remotes",
        "printf x > .git/packed-refs",
        "git fetch nowhere main:refs/remotes/origin/main",
    ] {
        repo.check("git-guard", cmd, 2);
    }
    repo.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    let raw = repo
        .root()
        .join("remote.git")
        .to_string_lossy()
        .into_owned();
    repo.git(&["config", "--global", "url./elsewhere/.insteadOf", &raw]);
    let out = repo.check("git-guard", "git fetch origin", 2);
    assert!(String::from_utf8_lossy(&out.stderr).contains("effective URL"));
    repo.check(
        "git-guard",
        "git config --global --get remote.origin.url",
        0,
    );
}

#[test]
fn ac2_tag_created_by_update_ref_cannot_be_published_in_same_call() {
    let repo = Repo::new();
    repo.landed();
    repo.check(
        "exec-guard",
        "git update-ref refs/tags/new HEAD && git push origin new",
        2,
    );
    repo.check("exec-guard", "git update-ref refs/tags/new HEAD", 0);
}

#[test]
#[allow(clippy::too_many_lines)] // One installed journey across fetch, policy and hook failure.
fn ac5_installed_project_observes_landed_policy_and_missing_binary() {
    let repo = Repo::new();
    repo.remote();
    std::fs::write(
        repo.root().join(".codeflow/policy.json"),
        r#"{"security":{"headless_peer_runs":"block","privilege_escalation":"off"}}"#,
    )
    .unwrap();
    repo.git(&["add", ".codeflow/policy.json"]);
    repo.git(&["commit", "-qm", "chore: landed policy"]);
    repo.git(&["push", "-q", "origin", "HEAD:main"]);
    repo.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    let bin = repo.root().join("tools");
    std::fs::create_dir(&bin).unwrap();
    std::fs::copy(binary(), bin.join("codeflow")).unwrap();
    let path = std::env::join_paths(
        [bin.clone()]
            .into_iter()
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let out = repo
        .command(&binary())
        .env("PATH", &path)
        .args(["init", "--yes", "--minimal"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    // Commit through the installed shim on the normal root branch, then work on a branch.
    repo.git(&["switch", "-c", "feat/weak"]);
    let mut weak: serde_json::Value =
        serde_json::from_slice(&std::fs::read(repo.root().join(".codeflow/policy.json")).unwrap())
            .unwrap();
    weak["git"]["commit_to_protected"] = json!("off");
    weak["git"]["root_checkout_commits"] = json!("off");
    weak["security"]["headless_peer_runs"] = json!("off");
    std::fs::write(repo.root().join(".codeflow/policy.json"), weak.to_string()).unwrap();
    repo.git(&["add", ".codeflow/policy.json"]);
    let out = repo
        .command("git")
        .env("PATH", &path)
        .args(["commit", "-qm", "chore: local policy"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    repo.check("exec-guard", "claude -p hello", 2);
    repo.check(
        "git-guard",
        &format!(
            "git fetch '{}' main:refs/remotes/origin/main",
            repo.root().join("weaker.git").display()
        ),
        2,
    );
    repo.check("exec-guard", "sudo true", 0);
    let operator = repo.root().join("operator");
    codeflow_fixture::clone(repo.root(), repo.root().join("remote.git"), &operator)
        .branch("main")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .run();
    let operator_git = |args: &[&str]| {
        let out = repo
            .command("git")
            .current_dir(&operator)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "operator {args:?}: {out:?}");
    };
    operator_git(&["config", "user.name", "Test"]);
    operator_git(&["config", "user.email", "test@example.invalid"]);
    std::fs::write(
        operator.join(".codeflow/policy.json"),
        r#"{"security":{"headless_peer_runs":"block","privilege_escalation":"block"}}"#,
    )
    .unwrap();
    operator_git(&["add", ".codeflow/policy.json"]);
    operator_git(&["commit", "-qm", "chore: stricter policy"]);
    operator_git(&["push", "-q", "origin", "main"]);
    repo.git(&["fetch", "origin"]);
    repo.check("exec-guard", "sudo true", 2);
    let empty = repo.root().join("no-binary");
    std::fs::create_dir(&empty).unwrap();
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(repo.root().join(".claude/settings.json")).unwrap())
            .unwrap();
    let command = config["hooks"]["PreToolUse"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|e| e["hooks"].as_array().unwrap())
        .find_map(|h| {
            h["command"]
                .as_str()
                .filter(|s| s.starts_with("codeflow hook git-guard"))
        })
        .unwrap();
    let out = repo
        .command("/bin/sh")
        .env("PATH", &empty)
        .args(["-c", command])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("codeflow update"));
    let out = repo
        .command("/usr/bin/git")
        .env("PATH", &empty)
        .env_remove("CODEFLOW_HOOK_BINARY")
        .args(["commit", "--allow-empty", "-m", "chore: refused"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("codeflow update"));
}

#[test]
fn ac1_exec_guard_retargets_git_policy_without_borrowing_session_levels() {
    let session = Repo::new();
    session.landed();
    let other = Repo::new();
    std::fs::write(
        other.root().join(".codeflow/policy.json"),
        r#"{"security":{"outward_actions":"off"}}"#,
    )
    .unwrap();
    other.git(&["add", ".codeflow/policy.json"]);
    other.git(&["commit", "-qm", "chore: permitted"]);
    for command in [
        format!("git -C '{}' push origin --tags", other.root().display()),
        format!(
            "git --git-dir='{}' push origin --tags",
            other.root().join(".git").display()
        ),
        format!(
            "GIT_DIR='{}' git push origin --tags",
            other.root().join(".git").display()
        ),
    ] {
        session.check("exec-guard", &command, 0);
    }
}

#[test]
fn ac2_the_last_tracking_ref_cannot_be_removed() {
    let repo = Repo::new();
    repo.landed();
    let tip = repo.git(&["rev-parse", "refs/remotes/origin/main"]);
    repo.git(&["update-ref", "--no-deref", "refs/remotes/origin/HEAD", &tip]);
    repo.git(&["update-ref", "-d", "refs/remotes/origin/main"]);
    assert_eq!(
        repo.git(&[
            "for-each-ref",
            "--format=%(refname)",
            "refs/remotes/origin/"
        ])
        .lines()
        .count(),
        1
    );
    for command in [
        "git update-ref -d refs/remotes/origin/HEAD",
        "git symbolic-ref --delete refs/remotes/origin/HEAD",
        "git branch -Dr origin/HEAD",
        "git remote remove origin",
        "git remote rename origin elsewhere",
        "git fetch elsewhere main:refs/remotes/origin/HEAD",
        "rm -rf .git/refs/remotes/origin",
        "printf x > .git/packed-refs",
        "git config --unset remote.origin.url",
        "git config --global --unset-all url.x.insteadOf",
    ] {
        repo.check("git-guard", command, 2);
    }
    for command in [
        "git status",
        "git fetch origin",
        "git fetch --prune origin",
        "git config --get remote.origin.url",
    ] {
        repo.check("git-guard", command, 0);
    }
}

#[test]
fn ac4_missing_and_older_binaries_name_recovery_in_doctor_and_orient() {
    use std::os::unix::fs::PermissionsExt;
    let repo = Repo::new();
    let bin = repo.root().join("empty-path");
    std::fs::create_dir(&bin).unwrap();
    for old in [false, true] {
        if old {
            let fake = bin.join("codeflow");
            std::fs::write(&fake, "#!/bin/sh\necho 'hooks 2'\n").unwrap();
            std::fs::set_permissions(fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        for args in [vec!["doctor", "--check", "hooks"], vec!["orient"]] {
            let out = repo
                .command(&binary())
                .env("PATH", &bin)
                .args(args)
                .output()
                .unwrap();
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(text.contains("codeflow update"), "{text}");
            assert!(text.contains("codeflow-cli-installer.sh"), "{text}");
        }
    }
}

#[test]
fn ac2_ref_plumbing_observes_option_values_and_aliases() {
    let repo = Repo::new();
    repo.landed();
    repo.git(&["config", "alias.forget", "remote remove"]);
    for command in [
        "git push --repo . :refs/heads/task/x",
        "git push --repo=. :refs/heads/task/x",
        "git remote -v remove origin",
        "git forget origin",
    ] {
        repo.check("git-guard", command, 2);
    }
    repo.check(
        "git-guard",
        "git update-ref -m refs/remotes/origin/main refs/heads/feat/x HEAD",
        0,
    );
    repo.check("git-guard", "git symbolic-ref refs/remotes/origin/HEAD", 0);
}

#[test]
fn ac2_authority_metadata_is_not_disabled_by_local_edit_relief() {
    let repo = Repo::new();
    repo.remote();
    std::fs::write(
        repo.root().join(".codeflow/policy.json"),
        r#"{"git":{"hook_integrity":"off"}}"#,
    )
    .unwrap();
    repo.git(&["add", ".codeflow/policy.json"]);
    repo.git(&["commit", "-qm", "chore: local edit relief"]);
    repo.git(&["push", "-q", "origin", "HEAD:main"]);
    repo.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    for command in [
        "printf x > .git/config",
        "rm -rf .git/refs/remotes",
        "printf x > .git/packed-refs",
    ] {
        repo.check("git-guard", command, 2);
    }
    repo.check("git-guard", "printf x > .codeflow/policy.json", 0);
    repo.check(
        "exec-guard",
        "python -c \"open('.git/config', 'w').write('x')\"",
        2,
    );
}

#[test]
fn ac2_fetch_cannot_override_the_remote_in_command_configuration() {
    let repo = Repo::new();
    repo.landed();
    for command in ["git -cremote.origin.url=elsewhere fetch origin", "git --config-env=remote.origin.url=OTHER fetch origin", "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=remote.origin.url GIT_CONFIG_VALUE_0=elsewhere git fetch origin", "GIT_CONFIG_PARAMETERS=remote.origin.url=elsewhere git fetch origin", "GIT_CONFIG_GLOBAL=other.conf git fetch origin"] {
        repo.check("git-guard", command, 2);
    }
    repo.check("git-guard", "GIT_CONFIG_GLOBAL=/dev/null git status", 0);
    repo.check("git-guard", "git log --grep remote.origin.url", 0);
    repo.check("git-guard", "git -c user.name=Test fetch origin", 0);
}

#[test]
fn ac2_worktree_configuration_cannot_redirect_authority_fetch() {
    let repo = Repo::new();
    repo.landed();
    repo.git(&["config", "extensions.worktreeConfig", "true"]);
    repo.check("git-guard", "printf x > .git/config.worktree", 2);
    repo.git(&["worktree", "add", "-q", "-b", "task/other", "other"]);
    let private_git = repo.git(&["-C", "other", "rev-parse", "--absolute-git-dir"]);
    repo.check(
        "git-guard",
        &format!("printf x > {private_git}/config.worktree"),
        2,
    );
    repo.check("git-guard", "cat .git/config.worktree", 0);
}

fn run_hook(mut process: Command, cwd: &Path, hook: &str, command: &str) -> Output {
    let mut child = process
        .args(["hook", hook])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    write!(
        child.stdin.take().unwrap(),
        "{}",
        json!({"tool_name":"Bash","cwd":cwd,"tool_input":{"command":command}})
    )
    .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn r2_f1_prefix_rewrites_from_global_config_cannot_replace_authority() {
    for scope in ["home", "xdg", "explicit"] {
        let repo = Repo::new();
        repo.landed();
        let weak = Repo::new();
        weak.remote();
        weak.git(&["push", "-q", "origin", "HEAD:main"]);
        let config = match scope {
            "home" => repo.root().join(".gitconfig"),
            "xdg" => repo.root().join(".config/git/config"),
            _ => repo.root().join("selected.gitconfig"),
        };
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        let process = |program: &str| {
            let mut process = repo.command(program);
            process.env("XDG_CONFIG_HOME", repo.root().join(".config"));
            if scope == "explicit" {
                process.env("GIT_CONFIG_GLOBAL", &config);
            } else {
                process.env_remove("GIT_CONFIG_GLOBAL");
            }
            process
        };
        let prefix = format!("{}/", repo.root().display());
        let replacement = format!("{}/", weak.root().display());
        let raw = repo.git(&["config", "--get", "remote.origin.url"]);
        let original = repo.git(&["rev-parse", "refs/remotes/origin/main"]);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&config)
            .unwrap();
        writeln!(
            file,
            "[url \"{replacement}\"]\ninsteadOf = {prefix}\npushInsteadOf = {prefix}"
        )
        .unwrap();
        drop(file);
        let effective = process("git")
            .args(["remote", "get-url", "origin"])
            .output()
            .unwrap();
        assert!(effective.status.success());
        assert_ne!(String::from_utf8_lossy(&effective.stdout).trim(), raw);
        let out = run_hook(
            process(&binary()),
            repo.root(),
            "git-guard",
            "git fetch origin",
        );
        let text = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{scope}: {text}");
        assert!(
            text.contains("git.policy_authority") && text.contains("effective URL"),
            "{text}"
        );
        assert!(
            text.contains("operator") && text.contains("--show-origin"),
            "{text}"
        );
        assert_eq!(
            repo.git(&["rev-parse", "refs/remotes/origin/main"]),
            original
        );
        // A push-only rewrite does not redirect a fetch, nor do user preferences.
        std::fs::write(
            &config,
            format!("[user]\nname = Test\n[url \"{replacement}\"]\npushInsteadOf = {prefix}\n"),
        )
        .unwrap();
        let out = run_hook(
            process(&binary()),
            repo.root(),
            "git-guard",
            "git fetch origin",
        );
        assert_eq!(
            out.status.code(),
            Some(0),
            "{scope}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn r2_f2_linked_worktree_pull_uses_fetch_authority_checks() {
    let repo = Repo::new();
    repo.landed();
    repo.git(&["worktree", "add", "-q", "-b", "task/linked", "linked"]);
    let linked = repo.root().join("linked");
    let weak = Repo::new();
    weak.remote();
    let url = weak.root().join("remote.git");
    for command in [
        format!("git pull '{}' main:refs/remotes/origin/main", url.display()),
        format!(
            "git pull --no-rebase '{}' main:refs/remotes/origin/main",
            url.display()
        ),
        format!("git pull '{}' main", url.display()),
        "git pull origin main:refs/remotes/origin/main".into(),
        "git pull --refmap= origin main".into(),
        "GIT_CONFIG_GLOBAL=other.conf git pull origin main".into(),
    ] {
        let mut process = repo.command(&binary());
        process.current_dir(&linked);
        let out = run_hook(process, &linked, "git-guard", &command);
        let text = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{command}: {text}");
        assert!(
            text.contains("git.policy_authority") && text.contains("sanctioned:"),
            "{text}"
        );
    }
    for command in [
        "git pull",
        "git pull origin main",
        "git pull --no-rebase origin main",
        "git pull --strategy recursive origin main",
    ] {
        let mut process = repo.command(&binary());
        process.current_dir(&linked);
        let out = run_hook(process, &linked, "git-guard", command);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{command}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn r2_f1_direct_git_config_writes_are_authority_edits() {
    let repo = Repo::new();
    repo.landed();
    for path in [
        "~/.gitconfig".to_string(),
        "$HOME/.gitconfig".to_string(),
        "${HOME}/.config/git/config".to_string(),
        repo.root().join("global.config").display().to_string(),
    ] {
        repo.check("git-guard", &format!("printf x >> {path}"), 2);
        repo.check("git-guard", &format!("cat {path}"), 0);
    }
    repo.check(
        "git-guard",
        "printf x >> ~/.gitconfig && git fetch origin",
        2,
    );
    repo.check("git-guard", "git config --global user.name Test", 0);
    let mut process = repo.command(&binary());
    process.env("GIT_CONFIG_GLOBAL", "/dev/null");
    let out = run_hook(process, repo.root(), "git-guard", "printf x >/dev/null");
    assert_eq!(out.status.code(), Some(0), "null redirect: {out:?}");
    let path = repo.root().join(".gitconfig");
    let mut child = repo
        .command(&binary())
        .args(["hook", "edit-guard"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    write!(child.stdin.take().unwrap(), "{}", json!({"tool_name":"Write", "cwd":repo.root(), "tool_input":{"file_path":path,"content":"x"}})).unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(2), "native edit: {out:?}");
}

#[test]
fn r3_f3_linked_remote_update_rejects_configuration_overrides() {
    let repo = Repo::new();
    repo.landed();
    repo.git(&["worktree", "add", "-q", "-b", "task/linked", "linked"]);
    let linked = repo.root().join("linked");
    let weak = Repo::new();
    weak.remote();
    weak.git(&["push", "-q", "origin", "HEAD:main"]);
    let config = linked.join("g.conf");
    let prefix = format!("{}/", repo.root().display());
    let replacement = format!("{}/", weak.root().display());
    std::fs::write(
        &config,
        format!("[url \"{replacement}\"]\ninsteadOf = {prefix}\n"),
    )
    .unwrap();
    for relative in ["other-home/.gitconfig", "other-xdg/git/config"] {
        let path = linked.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(&config, path).unwrap();
    }
    let mut failures = Vec::new();
    for environment in [
        "GIT_CONFIG_GLOBAL=g.conf".to_string(),
        "HOME=other-home".into(),
        "XDG_CONFIG_HOME=other-xdg".into(),
        format!("GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=url.{replacement}.insteadOf GIT_CONFIG_VALUE_0={prefix}"),
        "env GIT_CONFIG_GLOBAL=g.conf".into(),
    ] {
        for arguments in ["update", "update origin", "update --prune origin", "-v update -p origin"] {
            let command = format!("{environment} git remote {arguments}");
            let mut process = repo.command(&binary());
            process.current_dir(&linked);
            let out = run_hook(process, &linked, "git-guard", &command);
            let text = String::from_utf8_lossy(&out.stderr);
            if out.status.code() != Some(2) || !text.contains("git.policy_authority") || !text.contains("configuration environment overrides") {
                failures.push(format!("{command}: exit {:?}: {text}", out.status.code()));
            }
        }
    }
    for command in [
        "git remote update",
        "git remote update origin",
        "git remote update -p origin",
        "git remote -v",
        "git remote show origin",
        "git ls-remote origin",
        "GIT_CONFIG_GLOBAL=g.conf git remote -v",
        "git config --get remotes.team",
    ] {
        let mut process = repo.command(&binary());
        process.current_dir(&linked);
        let out = run_hook(process, &linked, "git-guard", command);
        assert_eq!(out.status.code(), Some(0), "{command}: {out:?}");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn r3_f3_remote_update_checks_every_selected_remote() {
    let repo = Repo::new();
    repo.landed();
    repo.git(&["worktree", "add", "-q", "-b", "task/linked", "linked"]);
    let linked = repo.root().join("linked");
    let weak = Repo::new();
    weak.remote();
    weak.git(&["push", "-q", "origin", "HEAD:main"]);
    // Only the second remote is rewritten; selecting origin alone remains safe.
    repo.git(&["remote", "add", "secondary", "trusted:remote.git"]);
    repo.git(&["config", "--add", "remotes.team", "origin"]);
    repo.git(&["config", "--add", "remotes.team", "secondary origin"]);
    std::fs::write(
        repo.root().join("global.config"),
        format!(
            "[url \"{}/\"]\ninsteadOf = trusted:\n",
            weak.root().display()
        ),
    )
    .unwrap();
    let mut failures = Vec::new();
    let mut check = |command: &str, expected: i32| {
        let mut process = repo.command(&binary());
        process.current_dir(&linked);
        let out = run_hook(process, &linked, "git-guard", command);
        let text = String::from_utf8_lossy(&out.stderr);
        if out.status.code() != Some(expected)
            || (expected == 2
                && !(text.contains("git.policy_authority")
                    && text.contains("effective URL for secondary")
                    && text.contains("--show-origin")))
        {
            failures.push(format!(
                "{command}: expected {expected}, exit {:?}: {text}",
                out.status.code()
            ));
        }
    };
    for command in [
        "git remote update",
        "git remote update origin secondary",
        "git remote update --prune secondary",
        "git remote -v update -p team",
        "git remote update -- team",
    ] {
        check(command, 2);
    }
    check("git remote update origin", 0);
    repo.git(&["config", "remote.secondary.skipDefaultUpdate", "true"]);
    check("git remote update", 0);
    check("git remote update secondary", 2);
    repo.git(&["config", "remotes.default", "origin secondary"]);
    check("git remote update", 2);
    repo.git(&["config", "remotes.default", "origin"]);
    check("git remote update", 0);
    check("git remote show secondary", 0);
    // Group changes must not hide a selected remote from pre-command inspection.
    for command in [
        "git -c remotes.default=secondary remote update",
        "git config remotes.default secondary && git remote update",
    ] {
        let mut process = repo.command(&binary());
        process.current_dir(&linked);
        let out = run_hook(process, &linked, "git-guard", command);
        if out.status.code() != Some(2)
            || !String::from_utf8_lossy(&out.stderr).contains("git.policy_authority")
        {
            failures.push(format!("{command}: exit {:?}", out.status.code()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn r4_init_and_push_without_remote_head_uses_landed_main() {
    let repo = Repo::new();
    repo.remote();
    repo.policy("block");
    repo.git(&["add", ".codeflow/policy.json"]);
    repo.git(&["commit", "-qm", "chore: strict landed policy"]);
    repo.git(&["push", "-qu", "origin", "HEAD:main"]);
    assert!(!repo
        .command("git")
        .args(["rev-parse", "--verify", "refs/remotes/origin/HEAD"])
        .output()
        .unwrap()
        .status
        .success());
    repo.check("git-guard", "git status", 0);
    repo.policy("off");
    let out = repo.check("exec-guard", "claude -p hello", 2);
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(
        text.contains("refs/remotes/origin/main (remote HEAD not set)"),
        "{text}"
    );
    repo.git(&["add", ".codeflow/policy.json"]);
    repo.git(&["commit", "-qm", "chore: weaker local policy"]);
    repo.check("exec-guard", "claude -p hello", 2);
    let doctor = repo
        .command(&binary())
        .args(["doctor", "--check", "policy-source"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&doctor.stdout);
    assert!(
        text.contains("refs/remotes/origin/main (remote HEAD not set)")
            && text.contains("git remote set-head origin --auto"),
        "{text}"
    );
}

#[test]
fn r4_policy_remedy_clears_only_after_landing_and_fetch() {
    let repo = Repo::new();
    repo.landed();
    let out = repo.check("git-guard", "git branch -D main", 2);
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(
        text.contains("land")
            && text.contains("fetch")
            && text.contains("refs/remotes/origin/main"),
        "{text}"
    );
    std::fs::write(
        repo.root().join(".codeflow/policy.json"),
        r#"{"git":{"protected_branches":["master"]}}"#,
    )
    .unwrap();
    repo.check("git-guard", "git branch -D main", 2);
    repo.git(&["add", ".codeflow/policy.json"]);
    repo.git(&["commit", "-qm", "chore: operator changes protection"]);
    repo.check("git-guard", "git branch -D main", 2);
    repo.git(&["push", "-q", "origin", "HEAD:main"]);
    repo.git(&["fetch", "-q", "origin"]);
    repo.check("git-guard", "git branch -D main", 0);
}

#[test]
fn r4_custom_default_without_remote_head_names_tried_refs_and_recovery() {
    let repo = Repo::new();
    repo.remote();
    repo.git(&["push", "-qu", "origin", "HEAD:trunk"]);
    repo.git(&[
        "--git-dir",
        "remote.git",
        "symbolic-ref",
        "HEAD",
        "refs/heads/trunk",
    ]);
    let out = repo.check("git-guard", "git status", 2);
    let text = String::from_utf8_lossy(&out.stderr);
    for part in [
        "refs/remotes/origin/main",
        "refs/remotes/origin/master",
        "git fetch origin",
        "git remote set-head origin --auto",
    ] {
        assert!(text.contains(part), "missing {part}: {text}");
    }
    let doctor = repo
        .command(&binary())
        .args(["doctor", "--check", "policy-source"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&doctor.stdout);
    assert!(
        text.contains("refs/remotes/origin/main")
            && text.contains("refs/remotes/origin/master")
            && text.contains("git remote set-head origin --auto"),
        "{text}"
    );
    // The named operator recovery works on the fixture's advertised default.
    repo.git(&["remote", "set-head", "origin", "--auto"]);
    repo.check("git-guard", "git status", 0);
}

#[test]
fn r4_remote_head_fallback_ignores_local_branch_selection() {
    let repo = Repo::new();
    repo.remote();
    repo.git(&["push", "-q", "origin", "HEAD:master"]);
    repo.policy("block");
    repo.git(&["add", ".codeflow/policy.json"]);
    repo.git(&["commit", "-qm", "chore: stricter main"]);
    repo.git(&["push", "-q", "origin", "HEAD:main"]);
    std::fs::write(
        repo.root().join(".codeflow/policy.json"),
        r#"{"git":{"protected_branches":["master"]},"security":{"headless_peer_runs":"off"}}"#,
    )
    .unwrap();
    repo.git(&["add", ".codeflow/policy.json"]);
    repo.git(&[
        "commit",
        "-qm",
        "chore: local selection cannot choose master",
    ]);
    let out = repo.check("exec-guard", "claude -p hello", 2);
    assert!(String::from_utf8_lossy(&out.stderr)
        .contains("refs/remotes/origin/main (remote HEAD not set)"));
    repo.git(&["update-ref", "-d", "refs/remotes/origin/main"]);
    repo.check("exec-guard", "claude -p hello", 0);
    let doctor = repo
        .command(&binary())
        .args(["doctor", "--check", "policy-source"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&doctor.stdout)
        .contains("refs/remotes/origin/master (remote HEAD not set)"));
}

#[test]
fn r4_an_existing_remote_head_with_a_missing_target_does_not_fall_back() {
    let repo = Repo::new();
    repo.remote();
    repo.git(&["push", "-q", "origin", "HEAD:main"]);
    repo.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/trunk",
    ]);
    let out = repo.check("git-guard", "git status", 2);
    assert!(String::from_utf8_lossy(&out.stderr).contains("git fetch origin"));
}

#[test]
fn r4_fallback_default_record_still_requires_the_declared_target() {
    let repo = Repo::new();
    repo.remote();
    repo.git(&["push", "-q", "origin", "HEAD:main"]);
    std::fs::create_dir_all(repo.root().join("project-management/tasks")).unwrap();
    std::fs::write(
        repo.root().join("project-management/tasks/TSK-001.md"),
        "---\nid: TSK-001\nintegration_target: integration/test\n---\n",
    )
    .unwrap();
    repo.git(&["add", "project-management"]);
    repo.git(&["commit", "-qm", "chore: declare policy target"]);
    repo.git(&["push", "-q", "origin", "HEAD:master"]);
    repo.git(&["switch", "-qc", "task/TSK-001-impl"]);
    let out = repo.check("git-guard", "git status", 2);
    assert!(String::from_utf8_lossy(&out.stderr).contains("refs/remotes/origin/integration/test"));
    repo.policy("block");
    repo.git(&["add", ".codeflow/policy.json"]);
    repo.git(&["commit", "-qm", "chore: stricter target policy"]);
    repo.git(&["push", "-q", "origin", "HEAD:integration/test"]);
    repo.policy("off");
    let out = repo.check("exec-guard", "claude -p hello", 2);
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(
        text.contains("refs/remotes/origin/main (remote HEAD not set)")
            && text.contains("refs/remotes/origin/master (remote HEAD not set)")
            && text.contains("refs/remotes/origin/integration/test (stricter policy levels)"),
        "{text}"
    );
}

fn fetch_cannot_weaken_missing_head_authority(selective: bool) {
    let repo = Repo::new();
    repo.remote();
    // Keep the fallback case on Git versions that set remote HEAD during fetch.
    repo.git(&["config", "remote.origin.followRemoteHEAD", "never"]);
    repo.policy("block");
    repo.git(&["add", ".codeflow/policy.json"]);
    repo.git(&["commit", "-qm", "chore: strict master"]);
    repo.git(&["push", "-q", "origin", "HEAD:master"]);
    repo.policy("off");
    repo.git(&["add", ".codeflow/policy.json"]);
    repo.git(&["commit", "-qm", "chore: weaker main"]);
    // Model another checkout publishing main, without creating its tracking ref.
    repo.git(&["push", "-q", "./remote.git", "HEAD:main"]);
    repo.check("exec-guard", "claude -p hello", 2);
    let fetch = if selective {
        "git fetch origin main"
    } else {
        "git fetch origin"
    };
    repo.check("git-guard", fetch, 0);
    repo.git(&fetch.split_whitespace().skip(1).collect::<Vec<_>>());
    let out = repo.check("exec-guard", "claude -p hello", 2);
    let text = String::from_utf8_lossy(&out.stderr);
    for source in ["refs/remotes/origin/main", "refs/remotes/origin/master"] {
        assert!(text.contains(source), "{text}");
    }
}

#[test]
fn r4_fetch_adding_weaker_main_keeps_strict_master() {
    fetch_cannot_weaken_missing_head_authority(false);
}

#[test]
fn r4_selective_fetch_adding_weaker_main_keeps_strict_master() {
    fetch_cannot_weaken_missing_head_authority(true);
}

#[test]
fn r4_fallback_candidates_merge_each_policy_key() {
    let repo = Repo::new();
    repo.remote();
    for (branch, headless, force) in [("main", "off", "block"), ("master", "block", "allow")] {
        std::fs::write(
            repo.root().join(".codeflow/policy.json"),
            json!({"security":{"headless_peer_runs":headless},"git":{"force_push_unprotected":force}}).to_string(),
        ).unwrap();
        repo.git(&["add", ".codeflow/policy.json"]);
        repo.git(&["commit", "-qm", "chore: candidate policy"]);
        repo.git(&["push", "-q", "origin", &format!("HEAD:{branch}")]);
    }
    repo.policy("off");
    let authority = codeflow_core::hooks::landed_policy::load(repo.root()).unwrap();
    let policy = serde_json::to_value(authority.policy).unwrap();
    assert_eq!(policy["security"]["headless_peer_runs"], "block");
    assert_eq!(policy["git"]["force_push_unprotected"], "block");
    repo.check("exec-guard", "claude -p hello", 2);
    repo.check("git-guard", "git push --force origin task/x", 2);
    let doctor = repo
        .command(&binary())
        .args(["doctor", "--check", "policy-source"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&doctor.stdout);
    for part in [
        "refs/remotes/origin/main",
        "refs/remotes/origin/master",
        "remote HEAD not set",
        "custom default",
        "git remote set-head origin --auto",
    ] {
        assert!(text.contains(part), "missing {part}: {text}");
    }
}

fn publish_policy(repo: &Repo, branch: &str, policy: &serde_json::Value) {
    std::fs::write(
        repo.root().join(".codeflow/policy.json"),
        policy.to_string(),
    )
    .unwrap();
    repo.git(&["add", ".codeflow"]);
    repo.git(&["commit", "--allow-empty", "-qm", "chore: candidate control"]);
    repo.git(&["push", "-q", "origin", &format!("HEAD:{branch}")]);
}

#[test]
fn r4_review_candidate_order_equal_levels_and_protected_sets() {
    for reverse in [false, true] {
        let repo = Repo::new();
        repo.remote();
        let policies = [
            json!({"security":{"headless_peer_runs":"block"},"git":{"protected_branches":["one"],"force_push_unprotected":"allow"}}),
            json!({"security":{"headless_peer_runs":"warn"},"git":{"protected_branches":["two"],"force_push_unprotected":"block"}}),
        ];
        for (i, branch) in ["main", "master"].iter().enumerate() {
            publish_policy(&repo, branch, &policies[if reverse { 1 - i } else { i }]);
        }
        let p = codeflow_core::hooks::landed_policy::load(repo.root())
            .unwrap()
            .policy;
        let p = serde_json::to_value(p).unwrap();
        assert_eq!(p["security"]["headless_peer_runs"], "block");
        assert_eq!(p["git"]["force_push_unprotected"], "block");
        for protected in ["one", "two"] {
            assert!(p["git"]["protected_branches"]
                .as_array()
                .unwrap()
                .contains(&json!(protected)));
        }
        publish_policy(&repo, "master", &policies[usize::from(reverse)]);
        repo.check("exec-guard", "claude -p hello", if reverse { 0 } else { 2 });
    }
}

#[test]
fn r4_review_reverse_and_stricter_candidate_additions() {
    for (existing, added, level, before, after) in [
        ("main", "master", "off", 2, 2),
        ("main", "master", "block", 0, 2),
        ("master", "main", "block", 0, 2),
    ] {
        let repo = Repo::new();
        repo.remote();
        repo.git(&["config", "remote.origin.followRemoteHEAD", "never"]);
        publish_policy(
            &repo,
            existing,
            &json!({"security":{"headless_peer_runs":if before == 2 { "block" } else { "off" }}}),
        );
        let original = repo.git(&["rev-parse", &format!("refs/remotes/origin/{existing}")]);
        repo.policy(level);
        repo.git(&["add", ".codeflow/policy.json"]);
        repo.git(&["commit", "-qm", "chore: additional candidate"]);
        repo.git(&["push", "-q", "./remote.git", &format!("HEAD:{added}")]);
        repo.check("exec-guard", "claude -p hello", before);
        repo.check("git-guard", "git fetch origin", 0);
        repo.git(&["fetch", "-q", "origin"]);
        assert_eq!(
            original,
            repo.git(&["rev-parse", &format!("refs/remotes/origin/{existing}")])
        );
        repo.check("exec-guard", "claude -p hello", after);
    }
}

#[test]
fn r4_review_all_candidates_and_target_keys_survive_local_hints() {
    let repo = Repo::new();
    repo.remote();
    publish_policy(
        &repo,
        "integration/strict",
        &json!({"git":{"force_push_unprotected":"block","pr_merge_to_protected":"allow"},"security":{"headless_peer_runs":"off"}}),
    );
    publish_policy(
        &repo,
        "integration/weak",
        &json!({"git":{"force_push_unprotected":"allow","pr_merge_to_protected":"allow"},"security":{"headless_peer_runs":"off"}}),
    );
    publish_policy(
        &repo,
        "master",
        &json!({"git":{"root_branch":"integration/strict","protected_branches":["special"],"force_push_unprotected":"allow","pr_merge_to_protected":"allow"},"security":{"headless_peer_runs":"block"}}),
    );
    publish_policy(
        &repo,
        "main",
        &json!({"git":{"root_branch":"integration/weak","pr_merge_to_protected":"block","force_push_unprotected":"allow"},"security":{"headless_peer_runs":"off"}}),
    );
    let weak = json!({"git":{"protected_branches":[],"root_branch":"task/x","branch_prefixes":[]},"security":{"headless_peer_runs":"off"}});
    std::fs::write(repo.root().join(".codeflow/policy.json"), weak.to_string()).unwrap();
    std::fs::write(
        repo.root().join(".codeflow/project.toml"),
        "[git]\nroot_branch = 'task/x'\n",
    )
    .unwrap();
    for state in 0..4 {
        match state {
            1 => {
                repo.git(&["add", ".codeflow"]);
            }
            2 => {
                repo.git(&["commit", "-qm", "chore: weak local hints"]);
            }
            3 => {
                repo.git(&["switch", "-qc", "feat/local-hints"]);
            }
            _ => {}
        }
        let authority = codeflow_core::hooks::landed_policy::load(repo.root()).unwrap();
        let p = serde_json::to_value(authority.policy).unwrap();
        for key in ["force_push_unprotected", "pr_merge_to_protected"] {
            assert_eq!(p["git"][key], "block");
        }
        assert_eq!(p["security"]["headless_peer_runs"], "block");
        assert!(p["git"]["protected_branches"]
            .as_array()
            .unwrap()
            .contains(&json!("special")));
        assert!(authority.source.contains("integration/strict"));
        repo.check("exec-guard", "claude -p hello", 2);
    }
}

#[test]
fn r4_review_conflicting_candidate_project_settings_refuse() {
    let repo = Repo::new();
    repo.remote();
    for branch in ["main", "master"] {
        std::fs::write(
            repo.root().join(".codeflow/project.toml"),
            format!("[git]\nroot_branch = '{branch}'\n"),
        )
        .unwrap();
        publish_policy(&repo, branch, &json!({}));
    }
    repo.check("git-guard", "git status", 2);
}

#[test]
fn r4_review_broken_candidates_and_remote_head_refuse() {
    for broken in ["policy", "project", "object", "cycle", "head"] {
        let repo = Repo::new();
        repo.remote();
        publish_policy(&repo, "main", &json!({}));
        match broken {
            "policy" | "project" => {
                let path = if broken == "policy" {
                    ".codeflow/policy.json"
                } else {
                    ".codeflow/project.toml"
                };
                std::fs::write(repo.root().join(path), "{broken").unwrap();
                repo.git(&["add", ".codeflow"]);
                repo.git(&["commit", "-qm", "chore: malformed candidate"]);
                repo.git(&["push", "-q", "origin", "HEAD:master"]);
            }
            "object" => {
                std::fs::write(
                    repo.root().join(".git/refs/remotes/origin/master"),
                    "1111111111111111111111111111111111111111\n",
                )
                .unwrap();
            }
            "cycle" => {
                repo.git(&[
                    "symbolic-ref",
                    "refs/remotes/origin/HEAD",
                    "refs/remotes/origin/loop",
                ]);
                repo.git(&[
                    "symbolic-ref",
                    "refs/remotes/origin/loop",
                    "refs/remotes/origin/HEAD",
                ]);
            }
            _ => {
                std::fs::write(
                    repo.root().join(".git/refs/remotes/origin/HEAD"),
                    "broken\n",
                )
                .unwrap();
            }
        }
        repo.check("git-guard", "git status", 2);
    }
}

fn head_follow_modes(version: &str) -> &'static [&'static str] {
    let supported = version
        .strip_prefix("git version ")
        .and_then(|version| {
            let mut parts = version.split('.');
            Some((
                parts.next()?.parse::<u32>().ok()?,
                parts.next()?.parse::<u32>().ok()?,
            ))
        })
        .is_some_and(|version| version >= (2, 48));
    if supported {
        &["never", "create", "warn", "always", "warn-if-not-main"]
    } else {
        eprintln!("skipping HEAD-follow create, warn, always and warn-if-not-main legs: Git 2.48 or newer required; found {version}; running the never leg");
        &["never"]
    }
}

#[test]
fn r5_head_follow_modes_respect_git_version() {
    for version in [
        "git version 2.47.3",
        "git version 2.39.5 (Apple Git-154)",
        "unknown",
    ] {
        assert_eq!(head_follow_modes(version), &["never"], "{version}");
    }
    for version in [
        "git version 2.48.0",
        "git version 2.53.0.windows.1",
        "git version 3.0.0",
    ] {
        assert_eq!(head_follow_modes(version).len(), 5, "{version}");
    }
}

#[test]
fn r4_review_custom_default_beside_main_and_head_transition() {
    let version = Repo::new().git(&["--version"]);
    for &mode in head_follow_modes(&version) {
        let repo = Repo::new();
        repo.remote();
        publish_policy(
            &repo,
            "integration/strict",
            &json!({"git":{"force_push_unprotected":"block"},"security":{"headless_peer_runs":"off"}}),
        );
        publish_policy(
            &repo,
            "master",
            &json!({"git":{"root_branch":"integration/strict"},"security":{"headless_peer_runs":"off"}}),
        );
        publish_policy(
            &repo,
            "main",
            &json!({"git":{"root_branch":"integration/strict"},"security":{"headless_peer_runs":"off"}}),
        );
        publish_policy(
            &repo,
            "trunk",
            &json!({"git":{"root_branch":"integration/strict"},"security":{"headless_peer_runs":"block"}}),
        );
        repo.git(&[
            "--git-dir",
            "remote.git",
            "symbolic-ref",
            "HEAD",
            "refs/heads/trunk",
        ]);
        repo.git(&["config", "remote.origin.followRemoteHEAD", mode]);
        let diagnostic = codeflow_core::hooks::landed_policy::diagnostic(repo.root()).unwrap();
        assert!(
            diagnostic.contains("custom defaults")
                && diagnostic.contains("git remote set-head origin --auto")
        );
        let authority = codeflow_core::hooks::landed_policy::load(repo.root()).unwrap();
        assert!(!authority.source.contains("trunk"));
        for source in ["origin/main", "origin/master", "origin/integration/strict"] {
            assert!(authority.source.contains(source), "{}", authority.source);
        }
        repo.check("exec-guard", "claude -p hello", 0);
        repo.check("git-guard", "git fetch origin", 0);
        repo.git(&["fetch", "-q", "origin"]);
        if mode == "never" {
            repo.check("exec-guard", "claude -p hello", 0);
            repo.git(&["remote", "set-head", "origin", "--auto"]);
        }
        let diagnostic = codeflow_core::hooks::landed_policy::diagnostic(repo.root()).unwrap();
        assert!(
            diagnostic.contains("refs/remotes/origin/trunk")
                && !diagnostic.contains("remote HEAD not set"),
            "{mode}: {diagnostic}"
        );
        assert!(diagnostic.contains("origin/integration/strict"));
        repo.check("exec-guard", "claude -p hello", 2);
        // A trusted server default change follows the operator's configured mode.
        repo.git(&[
            "--git-dir",
            "remote.git",
            "symbolic-ref",
            "HEAD",
            "refs/heads/main",
        ]);
        repo.git(&["fetch", "-q", "origin"]);
        let canonical = repo.git(&["symbolic-ref", "refs/remotes/origin/HEAD"]);
        assert_eq!(
            canonical,
            if mode == "always" {
                "refs/remotes/origin/main"
            } else {
                "refs/remotes/origin/trunk"
            }
        );
        repo.check(
            "exec-guard",
            "claude -p hello",
            if mode == "always" { 0 } else { 2 },
        );
        repo.check("git-guard", "git push --force origin task/x", 2);
    }
}

#[test]
fn r4_review_head_follow_config_and_pruning_authority_routes() {
    let repo = Repo::new();
    repo.remote();
    publish_policy(
        &repo,
        "master",
        &json!({"security":{"headless_peer_runs":"block"}}),
    );
    for command in [
        "git config fetch.followRemoteHEAD always",
        "git config --global fetch.followRemoteHEAD always",
        "git -c fetch.followRemoteHEAD=always fetch origin",
        "git config remote.origin.followRemoteHEAD always",
        "git update-ref refs/remotes/origin/main HEAD",
        "git update-ref -d refs/remotes/origin/master",
        "git symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/main",
        "git branch -Dr origin/master",
        "git remote set-head origin main",
        "git fetch --prune origin :refs/remotes/origin/master",
        "git fetch --prune --refmap= origin",
        "rm -rf .git/refs/remotes/origin/master",
    ] {
        repo.check("git-guard", command, 2);
    }
    for command in [
        "git fetch --prune origin",
        "git remote prune origin",
        "git status",
        "git log",
        "git config --get fetch.followRemoteHEAD",
    ] {
        repo.check("git-guard", command, 0);
    }
    repo.git(&["fetch", "-q", "--prune", "origin"]);
    repo.check("exec-guard", "claude -p hello", 2);
}

#[test]
fn r4_review_remote_prune_checks_transport_authority() {
    let repo = Repo::new();
    repo.landed();
    repo.check("git-guard", "git remote prune origin", 0);
    repo.check(
        "git-guard",
        "GIT_CONFIG_GLOBAL=weak.conf git remote prune origin",
        2,
    );
    repo.git(&[
        "config",
        "--global",
        "url./elsewhere.insteadOf",
        repo.root().join("remote.git").to_str().unwrap(),
    ]);
    repo.check("git-guard", "git remote prune origin", 2);
}

#[test]
fn r5_policy_relaxation_names_every_source_and_requires_every_landing() {
    let repo = Repo::new();
    repo.remote();
    let strict = json!({"git":{"root_branch":"integration/line","protected_branches":["main"]}});
    let relaxed = json!({"git":{"root_branch":"integration/line","protected_branches":[]}});
    for branch in ["integration/line", "main", "master"] {
        publish_policy(&repo, branch, &strict);
    }
    let output = repo.check("git-guard", "git branch -D main", 2);
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(
        text.contains("on every branch named as a policy source"),
        "{text}"
    );
    for branch in ["main", "master", "integration/line"] {
        assert!(
            text.contains(&format!("refs/remotes/origin/{branch}")),
            "{text}"
        );
    }
    for (branch, expected) in [("integration/line", 2), ("main", 2), ("master", 0)] {
        publish_policy(&repo, branch, &relaxed);
        repo.git(&["fetch", "-q", "origin", branch]);
        repo.check("git-guard", "git branch -D main", expected);
    }
}
