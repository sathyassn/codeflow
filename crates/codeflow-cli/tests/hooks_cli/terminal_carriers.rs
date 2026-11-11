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

/// The reason every `hc run` refuses, as the guards print it.
const HC_RUN_REFUSAL: &str = "can start a template";

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
        // An empty configuration folder: no template is read from it, and
        // every `hc run` refuses whether or not one exists.
        let config = base.join("config");
        std::fs::create_dir_all(&config).unwrap();
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

/// AC-2 (amended 2026-11-11, pending the operator): the interactive seat
/// launch lines refuse in both guards, because `hc`'s dispatch contract
/// cannot be verified from the command.
#[test]
fn the_seat_launch_lines_refuse() {
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
                if !refused(&out, HC_RUN_REFUSAL) {
                    wrong.push(format!(
                        "{hook}: {command}: {}",
                        String::from_utf8_lossy(&out.stderr)
                    ));
                }
            }
        }
    }
    let out = fixture.judge("exec-guard", "hc run --bg --name s seat 'review x.rs'");
    if !refused(&out, HC_RUN_REFUSAL) {
        wrong.push(format!(
            "seat template: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// AC-2: the Codex launch line refuses with `--cwd` at a large worktree,
/// inside the guard's deadline. The outer words are read once, by the
/// carrier reader; the secret-read walk does not read Codex's `-c` as
/// archive syntax over the whole `--cwd` tree.
#[test]
fn the_codex_launch_line_refuses_at_a_large_worktree() {
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
    assert!(
        refused(&out, HC_RUN_REFUSAL),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
}

/// Every `hc run` the guards judge refuses, whatever its options and
/// operands, and the refusal says what works instead. The lines cover the
/// plain forms, the seat lines and the payloads earlier readings of the
/// template folder were attacked with (PR 706 round 9).
#[test]
fn every_hc_run_refuses_in_both_guards() {
    let fixture = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        // Plain forms, with no template folder at all.
        "hc run",
        "hc run --bg",
        "hc run -- git status",
        "hc run 'git status'",
        "hc run peer",
        "hc run --bg peer 'review x.rs'",
        "herdr-core run -- ls",
        "/opt/herdr/bin/hc run -- ls",
        "env A=B hc run -- ls",
        "sh -c 'hc run -- ls'",
        "echo $(hc run -- ls)",
        // Lines that name or change a template folder, or a variable.
        "APPDATA=relative/dir hc run peer",
        "X=/opt/x hc run peer",
        "export APPDATA=/x\nhc run peer",
        "read APPDATA; hc run peer",
        "hc exec --env AppData=rel -- hc run peer",
        "i=0; (( i++ )); hc run peer",
        "FOO=$BAR hc run -- ls",
        // Help words refuse too: the option grammar is not known.
        "hc run --help",
        "hc run -h",
        "hc run -- peer --help",
        // Wrappers that run the command after their own words.
        "flock /tmp/review.lock hc run peer",
        "flock /tmp/review.lock -c 'hc run peer'",
        "script -c 'hc run peer' /dev/null",
        "script -q /dev/null hc run peer",
        "watch -n 5 hc run peer",
        "parallel hc run ::: peer",
        "arch -arm64 hc run peer",
        "sudo hc run peer",
        "doas hc run peer",
        "chrt 1 hc run peer",
        "taskset -c 0 hc run peer",
        "find . -maxdepth 0 -exec hc run peer \\;",
        "echo x | xargs hc run peer",
        "nice -n 5 hc run peer",
        // A carrier named by the words the shell reads.
        "h''c run peer",
        "\"h\"c run peer",
        "env h''c run peer",
        // Wrappers that run a command string, a shell or a launcher.
        "pkexec /bin/sh -c 'hc run peer'",
        "gsudo 'hc run peer'",
        "sudo sh -c 'hc run peer'",
        "doas sh -c 'hc run peer'",
        "busybox sh -c 'hc run peer'",
        "busybox env hc run peer",
        "busybox nohup hc run peer",
    ] {
        for hook in ["exec-guard", "git-guard"] {
            let out = fixture.judge(hook, command);
            if !refused(&out, HC_RUN_REFUSAL) {
                wrong.push(format!(
                    "{hook} should refuse: {command}: {}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
    }
    let exec = fixture.judge("exec-guard", "hc run peer");
    let said = String::from_utf8_lossy(&exec.stderr);
    if !said.contains("outside a guarded session") || !said.contains("host's own terminal") {
        wrong.push(format!("the refusal does not say what works: {said}"));
    }
    // Unaffected: a line without `hc run`, and the other `hc` forms.
    for command in [
        "git status",
        "echo hc run",
        "hc ls",
        "hc exec --cwd . ls",
        "hc file read README.md",
        "hc send t1 hello",
        "busybox grep 'hc run' notes.txt",
        "busybox echo 'hc run peer'",
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

/// Wrapper names are matched the way carrier names are, so a wrapper the
/// host finds under another spelling still has its command read; a wrapper
/// that only prints or searches stays data.
#[test]
fn wrapper_names_match_as_carrier_names_do() {
    let fixture = Fixture::new();
    let native = ["C:", "tools", "gsudo.exe"].join("\\");
    let mut wrong = Vec::new();
    for command in [
        "gsudo.exe 'hc run peer'".to_string(),
        "GSUDO 'hc run peer'".to_string(),
        "GSUDO.EXE 'hc run peer'".to_string(),
        "/usr/bin/SUDO hc run peer".to_string(),
        "SUDO.exe sh -c 'hc run peer'".to_string(),
        "Doas.Exe hc run peer".to_string(),
        "PKEXEC.EXE /bin/sh -c 'hc run peer'".to_string(),
        "Busybox.exe env hc run peer".to_string(),
        format!("'{native}' 'hc run peer'"),
        format!("\"{native}\" hc run peer"),
    ] {
        for hook in ["exec-guard", "git-guard"] {
            let out = fixture.judge(hook, &command);
            if !refused(&out, HC_RUN_REFUSAL) {
                wrong.push(format!(
                    "{hook} should refuse: {command}: {}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
    }
    // The privilege rule refuses these wrappers by itself, so the data-only
    // lines are checked against the carrier rule alone.
    for command in [
        "gsudo.exe grep 'hc run' notes.txt".to_string(),
        "GSUDO echo 'hc run peer'".to_string(),
        "/usr/bin/SUDO grep 'hc run' notes.txt".to_string(),
        format!("'{native}' echo 'hc run peer'"),
    ] {
        for hook in ["exec-guard", "git-guard"] {
            let out = fixture.judge(hook, &command);
            if String::from_utf8_lossy(&out.stderr).contains(HC_RUN_REFUSAL) {
                wrong.push(format!(
                    "{hook} should not apply the carrier rule: {command}: {}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// What the guards print for a carrier name with no proof, and for a
/// wrapper or launcher whose command cannot be read.
const UNPROVEN_REFUSAL: &str = "could not prove it is data";
const UNREAD_COMMAND_REFUSAL: &str = "cannot be read";

/// A carrier name needs an execution classification or a data proof, under
/// any program, and a wrapper or launcher whose command cannot be read
/// refuses, in both guards. Text a modeled grammar reads as data passes,
/// and data that reaches a command on the same line does not.
#[test]
fn a_carrier_name_needs_proof_and_unread_commands_refuse_in_both_guards() {
    let fixture = Fixture::new();
    let mut wrong = Vec::new();
    for command in [
        // A launcher on no list, a command string it is given, nested strings.
        "uv run hc run peer",
        "uv run 'hc run peer'",
        "frobnicate --flag hc",
        "uv run 'sh -c \"hc run peer\"'",
        "uv run 'frobnicate \"hc run peer\"'",
        // Data that reaches a command on the same line.
        "echo 'hc run peer' | sh",
        "printf '%s' 'hc run peer' | bash -s",
        "echo hc run peer | frobnicate",
        // The accepted cost: a bare carrier name given to a program with no
        // modeled data role.
        "ls hc",
    ] {
        for hook in ["exec-guard", "git-guard"] {
            let out = fixture.judge(hook, command);
            if !refused(&out, UNPROVEN_REFUSAL) && !refused(&out, HC_RUN_REFUSAL) {
                wrong.push(format!(
                    "{hook} should refuse: {command}: {}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
    }
    for command in [
        // A wrapper or launcher whose command is a word the guard cannot fill in.
        "gsudo \"$COMMAND\"",
        "sudo -E \"$COMMAND\"",
        "doas \"$COMMAND\" peer",
        "env \"$COMMAND\"",
        "env A=B \"$COMMAND\" peer",
        "nohup \"$COMMAND\" peer",
        "timeout 5 \"$COMMAND\"",
        "nice -n 5 \"$COMMAND\"",
        "find . -maxdepth 0 -exec \"$COMMAND\" {} \\;",
        "echo x | xargs \"$COMMAND\"",
    ] {
        for hook in ["exec-guard", "git-guard"] {
            let out = fixture.judge(hook, command);
            if !refused(&out, UNREAD_COMMAND_REFUSAL) {
                wrong.push(format!(
                    "{hook} should refuse as unread: {command}: {}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
    }
    // Data a modeled grammar reads passes.
    for command in [
        "grep 'hc run' notes.txt",
        "grep -rn hc docs",
        "echo 'hc run peer'",
        "echo hc run | grep hc",
        "which hc",
        "busybox grep 'hc run' notes.txt",
        "nice grep 'hc run' notes.txt",
        "timeout 5 echo 'hc run peer'",
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
    // The privilege rule refuses these wrappers by itself, and the `gh`
    // and `git` forms meet their own rules, so the data-only lines are
    // checked against the carrier rule alone.
    for command in [
        "sudo grep 'hc run' notes.txt",
        "gsudo echo 'hc run peer'",
        "gsudo gh pr create --body 'hc run peer'",
        "gh pr create --body 'hc run peer'",
        "gh pr create --title 'hc run peer' --body x",
        "git commit -m 'hc run peer'",
    ] {
        for hook in ["exec-guard", "git-guard"] {
            let out = fixture.judge(hook, command);
            let said = String::from_utf8_lossy(&out.stderr);
            if said.contains(UNPROVEN_REFUSAL)
                || said.contains(UNREAD_COMMAND_REFUSAL)
                || said.contains(HC_RUN_REFUSAL)
            {
                wrong.push(format!(
                    "{hook} should not apply the carrier rule: {command}: {said}"
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
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
