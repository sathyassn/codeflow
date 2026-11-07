//! TSK-1344: the built guards judge a command run through the Herdr app
//! (`hc`) or the older `herdr` CLI as the same command typed bare. Nothing
//! here runs `hc` or `herdr`; each string is only judged by the guards.
use super::{
    codeflow, enforced_repo, guard_payload, init_repo, run_with_stdin, shell_path,
    write_agent_policy, TARGETING_POLICY,
};
use std::path::Path;
use std::process::Output;

fn judge(hook: &str, command: &str, cwd: &Path, config: Option<&Path>) -> Output {
    let mut cmd = codeflow();
    cmd.args(["hook", hook, "--contract", "3"]).current_dir(cwd);
    if let Some(config) = config {
        cmd.env("XDG_CONFIG_HOME", config);
    }
    run_with_stdin(&mut cmd, &guard_payload(command, cwd))
}

fn refused(out: &Output, rule: &str) -> bool {
    out.status.code() == Some(2) && String::from_utf8_lossy(&out.stderr).contains(rule)
}

fn passed(out: &Output) -> bool {
    out.status.code() == Some(0)
}

/// A session repository on a task branch, beside a second repository on
/// `main`, both under the protected-branch policy.
struct Fixture {
    _temp: tempfile::TempDir,
    session: std::path::PathBuf,
    other: std::path::PathBuf,
    config: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let parent = codeflow_core::portable_path::canonicalize(&std::env::temp_dir()).unwrap();
        let temp = tempfile::tempdir_in(parent).unwrap();
        let base = codeflow_core::portable_path::canonicalize(temp.path()).unwrap();
        let session = base.join("session");
        std::fs::create_dir(&session).unwrap();
        init_repo(&session, "task/x");
        write_agent_policy(&session, TARGETING_POLICY);
        let other = base.join("other");
        std::fs::create_dir(&other).unwrap();
        enforced_repo(&other);
        let config = base.join("config");
        let templates = config.join("herdr-core/templates");
        std::fs::create_dir_all(&templates).unwrap();
        std::fs::write(
            templates.join("peer.md"),
            "---\nname: peer\ncommand: claude -p\n---\nReview.\n",
        )
        .unwrap();
        std::fs::write(
            templates.join("seat.md"),
            "---\nname: seat\ncommand: codex --no-daemon\n---\nReview.\n",
        )
        .unwrap();
        std::fs::write(templates.join("odd.md"), "---\ncommand: |\n  claude\n---\n").unwrap();
        Self {
            _temp: temp,
            session,
            other,
            config,
        }
    }

    fn judge(&self, hook: &str, command: &str) -> Output {
        judge(hook, command, &self.session, Some(&self.config))
    }
}

/// The carrier forms, each with `{}` where the carried command goes, as
/// words or as one quoted line.
const WORD_CARRIERS: &[&str] = &[
    "hc run -- {}",
    "hc run {}",
    "hc run --bg --name x -- {}",
    "hc run --bg --no-parent --no-notify --name x --cwd . -- {}",
    "hc run --outlive-parent --rm --keep --timeout 5 --label a=b -- {}",
    "hc run --name=x --timeout=5 -- {}",
    "hc run --parent t1 --die-with-parent --json -- {}",
    "hc exec {}",
    "hc exec --cwd . --env A=B --timeout 5 -i {}",
    "env A=B hc run -- {}",
    "/opt/herdr/bin/hc run -- {}",
    "herdr-core run -- {}",
];
const LINE_CARRIERS: &[&str] = &[
    "hc run '{}'",
    "herdr pane run w1:p1 '{}'",
    "sh -c 'hc run -- {}'",
];

/// AC-2: each rule class refuses the carried command as it refuses the
/// bare one, in the guard that owns the rule.
#[test]
fn carried_commands_are_judged_as_typed_bare() {
    let fixture = Fixture::new();
    let cases: &[(&str, &str, &str)] = &[
        (
            "claude -p hello",
            "exec-guard",
            "security.headless_peer_runs",
        ),
        (
            "codex exec hello",
            "exec-guard",
            "security.headless_peer_runs",
        ),
        ("grok -p hello", "exec-guard", "security.headless_peer_runs"),
        (
            "git push --force origin main",
            "git-guard",
            "git.force_push_protected",
        ),
        ("git branch -D main", "git-guard", "git.delete_protected"),
        (
            "git push --no-verify origin main",
            "git-guard",
            "git.no_verify_bypass",
        ),
        ("rm -rf /", "exec-guard", "security.dangerous_commands"),
        (
            "sudo rm -rf /var",
            "exec-guard",
            "security.privilege_escalation",
        ),
        ("cargo publish", "exec-guard", "security.outward_actions"),
    ];
    let mut wrong = Vec::new();
    for (payload, hook, rule) in cases {
        assert!(
            refused(&fixture.judge(hook, payload), rule),
            "bare control: {payload}"
        );
        let git = payload.starts_with("git ");
        let carriers = WORD_CARRIERS.iter().chain(LINE_CARRIERS);
        for carrier in carriers {
            // A pane's directory is unknown: its Git command refuses as
            // unreadable rather than under the branch rule.
            let rule = if git && carrier.starts_with("herdr pane") {
                "git.hook_integrity"
            } else {
                rule
            };
            let command = carrier.replace("{}", payload);
            if !refused(&fixture.judge(hook, &command), rule) {
                wrong.push(format!("{hook} should refuse under {rule}: {command}"));
            }
        }
    }
    let agent = fixture.judge(
        "exec-guard",
        "herdr agent start x --kind claude --pane p1 -- -p hello",
    );
    if !refused(&agent, "security.headless_peer_runs") {
        wrong.push("herdr agent start headless".to_string());
    }
    let template = fixture.judge("exec-guard", "hc run --bg peer 'review x.rs'");
    if !refused(&template, "security.headless_peer_runs") {
        wrong.push("hc run template headless".to_string());
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// AC-2: `--cwd` names the repository a carried Git command targets.
#[test]
fn a_carried_git_command_is_judged_in_its_directory() {
    let fixture = Fixture::new();
    let commit = "git commit --allow-empty -m 'chore: x'";
    assert!(passed(&fixture.judge("git-guard", commit)));
    let other = shell_path(&fixture.other);
    let control = fixture.judge(
        "git-guard",
        &format!("git -C '{other}' commit --allow-empty -m 'chore: x'"),
    );
    assert!(refused(&control, "git.commit_to_protected"), "{control:?}");
    for command in [
        format!("hc run --cwd '{other}' -- {commit}"),
        format!("hc exec --cwd '{other}' {commit}"),
        format!("hc run --cwd ../other -- {commit}"),
    ] {
        let out = fixture.judge("git-guard", &command);
        assert!(
            refused(&out, "git.commit_to_protected"),
            "{command}: {out:?}"
        );
    }
    let out = fixture.judge("git-guard", &format!("hc run --on vps -- {commit}"));
    assert!(refused(&out, "git.hook_integrity"), "{out:?}");
}

/// AC-2: the interactive seat launch lines still pass both guards.
#[test]
fn the_seat_launch_lines_pass() {
    let fixture = Fixture::new();
    let dir = shell_path(&fixture.session);
    let codex = "codex --no-daemon --model gpt-6.1-sol -c 'model_reasoning_effort=\"medium\"' --ask-for-approval never";
    let grok = "grok --model grok-4.7 --reasoning-effort medium --permission-mode auto";
    let mut wrong = Vec::new();
    for outlive in ["", "--outlive-parent "] {
        for (name, seat) in [("cf-x-cx01", codex), ("cf-x-gk01", grok)] {
            let command = format!(
                "hc run --bg --no-parent --no-notify {outlive}--name {name} --cwd '{dir}' -- {seat}"
            );
            for hook in ["exec-guard", "git-guard"] {
                let out = fixture.judge(hook, &command);
                if !passed(&out) {
                    wrong.push(format!(
                        "{hook}: {command}: {}",
                        String::from_utf8_lossy(&out.stderr)
                    ));
                }
            }
        }
    }
    let out = fixture.judge("exec-guard", "hc run --bg --name s seat 'review x.rs'");
    if !passed(&out) {
        wrong.push(format!(
            "seat template: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// AC-2: the Codex launch line passes with `--cwd` at a large worktree, inside
/// the guard's deadline. The outer words are read once, by the carrier
/// reader; the secret-read walk no longer reads Codex's `-c` as archive
/// syntax over the whole `--cwd` tree.
#[test]
fn the_codex_launch_line_passes_at_a_large_worktree() {
    let fixture = Fixture::new();
    let large = fixture.session.join("target/debug/deps");
    std::fs::create_dir_all(&large).unwrap();
    for at in 0..6000 {
        std::fs::write(large.join(format!("f{at}.rlib")), "").unwrap();
    }
    let dir = shell_path(&fixture.session);
    let command = format!(
        "hc run --bg --no-parent --no-notify --name cf-x-cx01 --cwd '{dir}' -- codex --no-daemon --model gpt-6.1-sol -c 'model_reasoning_effort=\"medium\"' --ask-for-approval never"
    );
    let started = std::time::Instant::now();
    let out = fixture.judge("exec-guard", &command);
    assert!(passed(&out), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
}

/// The secret-read rules still judge what a carrier reads: a carried
/// recursive read of a secret store, and the files `hc file read`, `hc put`
/// and `hc update --from` read.
#[test]
fn carried_and_carrier_reads_meet_the_secret_read_rules() {
    let fixture = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "hc run -- grep -r token ~/.ssh",
        "hc run --cwd . -- tar -czf out.tgz ~/.ssh",
        "hc exec grep -r token ~/.aws",
        "hc file read ~/.ssh/id_rsa",
        "hc file read --cwd ~/.ssh id_rsa",
        "hc put t1 ~/.ssh/id_rsa",
        "hc update --from ~/.aws/credentials",
    ] {
        let out = fixture.judge("exec-guard", command);
        if !refused(&out, "security.secret_reads") {
            wrong.push(format!(
                "should refuse: {command}: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    for command in ["hc file read README.md", "hc put t1 base.txt"] {
        let out = fixture.judge("exec-guard", command);
        if !passed(&out) {
            wrong.push(format!(
                "should allow: {command}: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// AC-3: a form the guards cannot read refuses in both; the read-only and
/// typing forms pass when their options parse.
#[test]
fn unreadable_forms_refuse_and_read_only_forms_pass() {
    let fixture = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        "hc frobnicate t1",
        "hc run --frob -- ls",
        "hc exec --frob ls",
        "hc send --frob t1 hello",
        "hc keys --frob t1 enter",
        "hc run --bg odd task",
        "hc run --cwd \"$(cat dir.txt)\" -- ls",
        "echo t1 | xargs hc run -- ls",
        // A directory the guard cannot establish (PR 706 review, F1):
        // another machine, with or without `--cwd`, and a pane's shell.
        "hc run --on vps -- ls",
        "hc exec --on vps --cwd / -- rm -rf usr",
        "hc exec --on vps --cwd ~/.ssh -- tar -cf - .",
        "hc file read --on vps --cwd ~/.ssh config",
        "herdr pane split --current --direction right --cwd /",
        "herdr pane split --current --env A=1",
        "herdr pane run w1:p1 'rm -rf usr'",
        "herdr agent start seat --kind codex --pane w1:p1",
    ] {
        let exec = fixture.judge("exec-guard", command);
        let git = fixture.judge("git-guard", command);
        if !refused(&exec, "security.dangerous_commands") || !refused(&git, "git.hook_integrity") {
            wrong.push(format!("should refuse: {command}"));
        }
    }
    for command in [
        "hc ls",
        "hc get t1",
        "hc read --lines 5 t1",
        "hc --skill",
        "hc wait t1",
        "hc kill t1",
        "hc rm t1",
        "hc attach t1",
        "hc send t1 'hello'",
        "hc send --if idle t1 'hello'",
        "hc keys t1 down enter",
        "hc file read README.md",
        "herdr pane list",
    ] {
        for hook in ["exec-guard", "git-guard"] {
            let out = fixture.judge(hook, command);
            if !passed(&out) {
                wrong.push(format!(
                    "{hook} should allow: {command}: {}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// AC-4: `hc file write` and `hc file edit` meet the Edit tool's path rules.
#[test]
fn hc_file_writes_meet_the_edit_path_rules() {
    let fixture = Fixture::new();
    for command in [
        "hc file write .codeflow/policy.json",
        "hc file edit --old a --new b .codeflow/policy.json",
        "hc file write --cwd .codeflow policy.json",
    ] {
        let out = fixture.judge("exec-guard", command);
        assert!(refused(&out, "git.hook_integrity"), "{command}: {out:?}");
    }
    let out = fixture.judge("exec-guard", "hc file write --on vps notes.txt");
    assert!(refused(&out, "security.dangerous_commands"), "{out:?}");
    for command in [
        "hc file write notes/x.txt",
        "hc file edit --old a --new b base.txt",
    ] {
        let out = fixture.judge("exec-guard", command);
        assert!(passed(&out), "{command}: {out:?}");
    }
}
