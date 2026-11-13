//! TSK-1344: the built guards judge a command run through the Herdr app
//! (`hc`) or the older `herdr` CLI as the same command typed bare. Nothing
//! here runs `hc` or `herdr`; each string is only judged by the guards.
use super::{
    codeflow, enforced_repo, init_repo, run_with_stdin, shell_path, write_agent_policy,
    TARGETING_POLICY,
};
use std::path::Path;
use std::process::Output;

fn judge(hook: &str, command: &str, cwd: &Path, config: Option<&Path>) -> Output {
    let mut cmd = codeflow();
    cmd.args(["hook", hook, "--contract", "3"]).current_dir(cwd);
    if let Some(config) = config {
        cmd.env("XDG_CONFIG_HOME", config);
    }
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": { "command": command },
        "cwd": cwd,
    });
    run_with_stdin(&mut cmd, &payload.to_string())
}

/// The reason every `hc run` refuses, as the guards print it.
const HC_RUN_REFUSAL: &str = "can start a template";

fn refused(out: &Output, rule: &str) -> bool {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let deadline = "the guard reached no decision within 20 seconds";
    out.status.code() == Some(2)
        && stderr.contains(rule)
        && !stderr.contains(deadline)
        && !String::from_utf8_lossy(&out.stdout).contains(deadline)
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
        "hc ls",
        "hc exec --cwd . ls",
        "hc file read README.md",
        "hc send t1 hello",
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
/// host finds under another spelling still has its command read.
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
    // Inline text also refuses under the carrier rule.
    for command in [
        "gsudo.exe grep 'hc run' notes.txt".to_string(),
        "GSUDO echo 'hc run peer'".to_string(),
        "/usr/bin/SUDO grep 'hc run' notes.txt".to_string(),
        format!("'{native}' echo 'hc run peer'"),
    ] {
        for hook in ["exec-guard", "git-guard"] {
            let out = fixture.judge(hook, &command);
            if !refused(&out, UNPROVEN_REFUSAL) {
                wrong.push(format!(
                    "{hook} should refuse inline text: {command}: {}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// What the guards print for a carrier name with no proof, and for a
/// wrapper or launcher whose command cannot be read.
const UNPROVEN_REFUSAL: &str = "outside a position where the guard runs and judges it";
const UNREAD_COMMAND_REFUSAL: &str = "cannot be read";

/// A carrier name needs its own parsed program position, under
/// any program, and a wrapper or launcher whose command cannot be read
/// refuses, in both guards. Inline text has no exemption.
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
        // Inline text is kept in a file instead.
        "ls hc",
        "grep 'hc run peer' commands.txt | sh",
        "hc exec git commit -m x; uv run --directory /other hc exec git commit -m x",
        "gh extension exec runner --body 'hc run peer'",
        "grep 'hc run' notes.txt",
        "grep -rn hc docs",
        "echo 'hc run peer'",
        "echo hc run | grep hc",
        "which hc",
        "busybox grep 'hc run' notes.txt",
        "nice grep 'hc run' notes.txt",
        "timeout 5 echo 'hc run peer'",
        "sudo grep 'hc run' notes.txt",
        "gsudo echo 'hc run peer'",
        "gsudo gh pr create --body 'hc run peer'",
        "gh pr create --body 'hc run peer'",
        "gh pr create --title 'hc run peer' --body x",
        "git commit -m 'hc run peer'",
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
        "env /opt/bin/[h]c run peer",
        "sudo /opt/bin/[h]c run peer",
        "find . -maxdepth 0 -exec /opt/bin/[h]c run peer {} \\;",
        "echo x | xargs /opt/bin/[h]c run peer",
        "flock lock /opt/bin/[h]c run peer",
        "script out /opt/bin/[h]c run peer",
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
    // These files remain subject to ordinary read and execution checks.
    for command in ["grep -f patterns.txt notes.md"] {
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
    // The Git and hosting rules judge these file forms independently.
    for command in ["gh pr create --body-file body.md", "git commit -F msg.txt"] {
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

#[test]
fn original_carried_program_readings_reach_the_git_rule() {
    let fixture = Fixture::new();
    for command in [
        "setsid /usr/bin/gi[t] push --force origin main",
        "env /usr/bin/gi[t] push --force origin main",
        "flock lock /usr/bin/gi[t] push --force origin main",
    ] {
        let out = fixture.judge("git-guard", command);
        let said = String::from_utf8_lossy(&out.stderr);
        assert!(
            refused(&out, "git.") && said.contains("command unresolved"),
            "{command}: {said}"
        );
    }
}

#[test]
fn incomplete_carried_walks_refuse_in_both_guards() {
    let fixture = Fixture::new();
    let command = format!("{}hc exec git status", "env ".repeat(40));
    for hook in ["exec-guard", "git-guard"] {
        let out = fixture.judge(hook, &command);
        assert!(
            refused(&out, UNREAD_COMMAND_REFUSAL),
            "{hook}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn plain_program_handling_and_carried_readability_keep_their_boundaries() {
    let fixture = Fixture::new();
    for command in ["f() { echo hi; }; f", "P=ls; $P"] {
        let out = judge("git-guard", command, &fixture.other, Some(&fixture.config));
        assert!(passed(&out), "{command}: {out:?}");
    }
    let out = judge(
        "git-guard",
        "setsid /usr/bin/gi[t] push --force origin main",
        &fixture.other,
        Some(&fixture.config),
    );
    assert!(refused(&out, "git."), "{out:?}");
}

#[test]
fn validated_env_splits_preserve_unread_carried_words() {
    let fixture = Fixture::new();
    for hook in ["git-guard", "exec-guard"] {
        let out = fixture.judge(hook, "env -S 'python3 tool.py' data.json");
        assert!(passed(&out), "{hook}: {out:?}");
        for command in [
            "env -S 'setsid' /usr/bin/gi[t] push --force origin main",
            "env -S 'command ${PROGRAM}'",
        ] {
            let out = fixture.judge(hook, command);
            assert!(
                refused(&out, UNREAD_COMMAND_REFUSAL),
                "{hook}: {command}: {out:?}"
            );
        }
    }
}

#[test]
fn applescript_shell_bodies_and_terminal_input_keep_their_boundaries() {
    let fixture = Fixture::new();
    for (hook, rule) in [
        ("git-guard", "git.hook_integrity"),
        ("exec-guard", "security.dangerous_commands"),
    ] {
        let out = fixture.judge(hook, "osascript -e 'do shell script \"cat README.md\"'");
        assert!(passed(&out), "{hook}: {out:?}");
        let out = fixture.judge(
            hook,
            "osascript -e 'tell app \"Terminal\" to do script \"git push --force origin main\"'",
        );
        assert!(refused(&out, rule), "{hook}: {out:?}");
    }
}

#[test]
fn carrier_delivery_uses_only_modeled_statements_and_text() {
    let fixture = Fixture::new();
    for command in [
        "tmux send-keys git Space push Enter",
        "tmux send-keys 'C- ' Enter",
        "tmux send-keys 'M- x' Enter",
        "tmux send-keys 'S- ' Enter",
        "tmux send-keys -t seat \"Read /literal/brief.txt in full and follow it.\" Space git Space status Enter",
        "osascript -e 'tell application \"Terminal\" to do  script \"git status\"'",
        "osascript -e 'tell application \"Terminal\" to do' -e 'script \"git status\"'",
        "osascript <<'APPLESCRIPT'\ntell application \"Terminal\" to do script \"git status\"\nAPPLESCRIPT",
        "osascript -e 'tell application \"System Events\" to key code 36'",
        "hc exec /usr/bin/gi[t] status",
        "/opt/bin/[h]c ls",
        "screen -S seat -X stuff 'g^Mit status'",
        "screen -S seat -X stuff 'g\\nit status'",
        "screen -S seat -X stuff '\\147it status'",
        "screen -S seat -X stuff hello world",
        "expect -c 'send \"g\\rit status\"'",
        "expect -c 'send \"\\x67it status\"'",
        "expect -c 'send \"[format %c 103]it status\"'",
        "expect -c 'send \"$cmd\"'",
        "expect -c 'send -re \"hello\"'",
        "expect script.exp",
        "hc send seat 'git status'",
        "hc keys seat g i t Space status Enter",
        "herdr-core send seat 'git status'",
        "herdr-core keys seat g i t Space status Enter",
        "herdr pane send-text seat 'git status'",
        "herdr pane send-keys seat g i t Space status Enter",
        "herdr agent send-keys seat g i t Space status Enter",
        "herdr agent prompt seat 'git status'",
        "osascript script.scpt",
        "osascript -l JavaScript -e '1'",
        "osascript -e 'do shell script \"cat README.md\"' -e 'tell application \"Terminal\" to do script \"git status\"'",
    ] {
        for (hook, rule) in [
            ("git-guard", "git.hook_integrity"),
            ("exec-guard", "security.dangerous_commands"),
        ] {
            let out = fixture.judge(hook, command);
            assert!(refused(&out, rule), "{hook}: {command}: {out:?}");
        }
    }
}

#[test]
fn carried_reentry_checks_program_readability() {
    let fixture = Fixture::new();
    for command in [
        "hc exec /usr/bin/gi[t] status",
        "hc machine add --via '/usr/bin/gi[t] status'",
        "herdr-core exec /usr/bin/gi[t] status",
        "herdr pane run seat '/usr/bin/gi[t] status'",
        "orca terminal create --command '/usr/bin/gi[t] status'",
        "tmux new-session '/usr/bin/gi[t] status'",
        "screen -dm /usr/bin/gi[t] status",
        "osascript -e 'do shell script \"/usr/bin/gi[t] status\"'",
    ] {
        for (hook, rule) in [
            ("git-guard", "git.hook_integrity"),
            ("exec-guard", "security.dangerous_commands"),
        ] {
            let out = fixture.judge(hook, command);
            assert!(refused(&out, rule), "{hook}: {command}: {out:?}");
        }
    }
}

#[test]
fn shell_tests_do_not_select_a_terminal_carrier() {
    let fixture = Fixture::new();
    for command in [
        "if [ -f notes.txt ]; then true; fi",
        "if [[ -f notes.txt ]]; then true; fi",
    ] {
        for hook in ["git-guard", "exec-guard"] {
            let out = fixture.judge(hook, command);
            assert!(passed(&out), "{hook}: {command}: {out:?}");
        }
    }
}

#[test]
fn operator_seat_forms_remain_available() {
    let fixture = Fixture::new();
    let dir = shell_path(&fixture.session);
    let brief = "/Volumes/DATA/Local/software-workspace/projects/agentic-systems/coordination/coordinators/parallel/briefs/pr706-r13-build-codex.md";
    let mut commands = vec![
        format!("orca terminal create --worktree active --title \"cf-codex-builder\" --command \"cd '{dir}' && nice -n 10 codex --ask-for-approval never -c default_permissions=\\\"cf-builder\\\"\" --json"),
        format!("orca terminal create --worktree active --title \"cf-grok-review\" --command \"cd '{dir}' && grok --always-approve -m grok-4.7 --effort high\""),
        format!("orca terminal send --terminal term_abc --text \"Read {brief} in full and do exactly what it says.\" --enter --wait-submit 20"),
        "orca terminal read --terminal term_abc --screen --json".to_string(),
        "orca terminal wait --terminal term_abc --for tui-idle --timeout-ms 300000 --json".to_string(),
        "orca terminal list --json".to_string(),
        "orca terminal close --terminal term_abc".to_string(),
        "hc ls".to_string(),
        "hc read seat".to_string(),
        format!("hc send seat \"Read {brief} in full and do exactly what it says.\""),
        "hc keys seat Enter".to_string(),
        "osascript -e 'do shell script \"cat README.md\"'".to_string(),
    ];
    for enter in ["Enter", "C-m"] {
        commands.push(format!(
            "tmux send-keys -t seat \"Read {brief} in full and do exactly what it says.\" {enter}"
        ));
    }
    let mut wrong = Vec::new();
    for command in commands {
        for hook in ["git-guard", "exec-guard"] {
            let out = fixture.judge(hook, &command);
            if !passed(&out) {
                wrong.push(format!(
                    "{hook}: {command}: {}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn modeled_admissions_keep_their_bounds() {
    let fixture = Fixture::new();
    for command in [
        "orca terminal read --terminal term_abc --screen --json",
        "expect -c 'spawn python3 tool.py {notes.txt}'",
        "expect -c 'spawn python3 tool.py notes.txt'",
        "osascript -e 'return \"a\"'",
        "osascript -l AppleScript -e 'return \"a\"'",
        "tmux send-keys -t seat hello Enter",
        "tmux send-keys -t seat -l Space",
    ] {
        for hook in ["git-guard", "exec-guard"] {
            let out = fixture.judge(hook, command);
            assert!(passed(&out), "{hook}: {command}: {out:?}");
        }
    }
    for command in [
        "orca terminal wait --terminal term_abc --screen --json",
        "orca terminal read --terminal term_abc --screen=yes",
        "expect -c 'spawn $x tool.py'",
        "expect -c 'spawn [exec which python3] tool.py'",
        "expect -c 'spawn python3 tool\\.py'",
        "expect -c 'spawn python3 tool.py; send hi'",
        "expect -c 'spawn python3 tool.py\nexit'",
        "osascript -e 'return \"a\" & \"b\"'",
        "osascript -l JavaScript -e 'return \"a\"'",
        "osascript -e 'return \"a\"' -e 'return \"b\"'",
        "osascript -e 'return \"a\"\nreturn \"b\"'",
        "tmux send-keys Space",
        "tmux send-keys space",
        "tmux send-keys C-c",
        "tmux send-keys m-x",
        "tmux send-keys S-Tab",
        "tmux send-keys '^C'",
        "tmux send-keys 0x67",
        "tmux send-keys F12",
        "tmux send-keys KPEnter",
        "tmux send-keys unknown-key",
    ] {
        for (hook, rule) in [
            ("git-guard", "git.hook_integrity"),
            ("exec-guard", "security.dangerous_commands"),
        ] {
            let out = fixture.judge(hook, command);
            assert!(refused(&out, rule), "{hook}: {command}: {out:?}");
        }
    }
    let out = fixture.judge(
        "git-guard",
        "expect -c 'spawn git push --force origin main'",
    );
    assert!(refused(&out, "git.force_push_protected"), "{out:?}");
    for hook in ["git-guard", "exec-guard"] {
        let bare = fixture.judge(hook, "git push --force origin main");
        let carried = fixture.judge(hook, "expect -c 'spawn git push --force origin main'");
        assert_eq!(bare.status.code(), carried.status.code(), "{hook}");
    }
    let out = fixture.judge("exec-guard", "expect -c 'spawn rm -rf /'");
    assert!(refused(&out, "security.dangerous_commands"), "{out:?}");
}

/// Catalog rows are only passed to guard code; none of the payloads run.
#[test]
fn orca_catalog_rows_reach_the_named_rules() {
    let fixture = Fixture::new();
    let typed = [
        Some("git.hook_integrity"),
        Some("security.dangerous_commands"),
    ];
    let git = [
        Some("git.force_push_protected"),
        Some("security.dangerous_commands"),
    ];
    let cases = [
        ("ctl-git", "git push --force origin main", [Some("git.force_push_protected"), None]),
        ("ctl-headless", "claude -p hi", [None, Some("security.headless_peer_runs")]),
        ("ctl-orca-plain", "orca terminal create --worktree active --command \"pnpm dev\"", [None, None]),
        ("ctl-orca-list", "orca terminal list", [None, None]),
        ("tc-git", "orca terminal create --worktree active --command \"git push --force origin main\"", git),
        ("tc-headless", "orca terminal create --worktree active --command 'claude -p hi'", [Some("git.hook_integrity"), Some("security.headless_peer_runs")]),
        ("ts-git", "orca terminal send --terminal term_abc --text \"git push --force origin main\" --enter", typed),
        ("ts-headless", "orca terminal send --terminal term_abc --text 'codex exec hi' --enter", typed),
        ("ts-ansi", "orca terminal send --terminal term_abc --text $'git push --force origin main\\r'", typed),
        ("split-git", "orca terminal split --terminal term_abc --command \"git push --force origin main\"", git),
        ("agent-prompt", "orca agent launch --agent codex --prompt \"run git push --force origin main\"", typed),
        ("wt-prompt", "orca worktree create --name n --agent codex --prompt \"git push --force origin main\"", typed),
        ("auto-precheck", "orca automations create --name n --trigger daily --prompt p --provider claude --precheck \"git push --force origin main\"", git),
        ("worker-spec", "orca orchestration worker-start --spec \"git push --force origin main\"", typed),
        ("orch-send", "orca orchestration send --to dispatch:d1 --subject s --body \"git push --force origin main\"", typed),
        ("type-text", "orca computer type-text --app Orca --text \"git push --force origin main\"", typed),
        ("paste-text", "orca computer paste-text --app Orca --text \"git push --force origin main\"", typed),
        ("type-stdin", "printf '%s\\n' 'git push --force origin main' | orca computer type-text --app Orca --text-stdin", typed),
        ("ts-file", "orca terminal send --terminal term_abc --text \"$(cat cmd.txt)\" --enter", typed),
    ];
    let mut wrong = Vec::new();
    for (id, command, expected) in cases {
        for (hook, expected) in ["git-guard", "exec-guard"].into_iter().zip(expected) {
            let out = fixture.judge(hook, command);
            let correct = expected.map_or_else(|| passed(&out), |rule| refused(&out, rule));
            if !correct {
                wrong.push(format!(
                    "{id} {hook} expected {expected:?}: {}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
            if !id.starts_with("ctl-") {
                let class_rule = if hook == "git-guard" {
                    "git.hook_integrity"
                } else {
                    "security.dangerous_commands"
                };
                if !refused(&out, class_rule) {
                    wrong.push(format!(
                        "{id} {hook} lost its carrier rule: {}",
                        String::from_utf8_lossy(&out.stderr)
                    ));
                }
            }
            if id == "ts-file" && hook == "exec-guard" && !refused(&out, "security.text_guard") {
                wrong.push(format!(
                    "{id} lost its independent text rule: {}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn orca_seat_and_brief_file_routes_remain_available() {
    let fixture = Fixture::new();
    let dir = codeflow_core::portable_path::slashed(&fixture.session);
    let brief = codeflow_core::portable_path::slashed(&fixture.session.join("brief.txt"));
    let cases = [
        format!("orca terminal create --worktree active --title t --command \"cd '{dir}' && codex --ask-for-approval never\""),
        format!("orca terminal send --terminal term_abc --text \"Read {brief} in full and follow it.\" --enter"),
        "orca terminal read --terminal term_abc".to_string(),
        "orca terminal list".to_string(),
        "orca terminal wait --terminal term_abc --for idle".to_string(),
        "orca terminal close --terminal term_abc".to_string(),
    ];
    let mut wrong = Vec::new();
    for command in cases {
        for hook in ["git-guard", "exec-guard"] {
            let out = fixture.judge(hook, &command);
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

#[test]
fn terminal_carrier_seats_and_file_pointer_prompts_remain_available() {
    let fixture = Fixture::new();
    let dir = codeflow_core::portable_path::slashed(&fixture.session);
    let brief = codeflow_core::portable_path::slashed(&fixture.session.join("brief.txt"));
    let cases = [
        format!("tmux new-session -d -s seat -c '{dir}' 'codex --ask-for-approval never'"),
        format!("tmux new-window -t seat -c '{dir}' 'codex --ask-for-approval never'"),
        format!("tmux send-keys -t seat \"Read {brief} in full and follow it.\" Enter"),
        format!("tmux send -t seat \"Read {brief} in full and follow it.\" Enter"),
        format!("screen -S seat -X stuff 'Read {brief} in full and follow it.'"),
        format!("osascript -e 'tell application \"System Events\" to keystroke \"Read {brief} in full and follow it.\"'"),
        format!("expect -c 'send \"Read {brief} in full and follow it.\"'"),
    ];
    let mut wrong = Vec::new();
    for command in cases {
        for hook in ["git-guard", "exec-guard"] {
            let out = fixture.judge(hook, &command);
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

#[test]
fn terminal_carrier_unknown_routes_and_unread_input_refuse_with_a_remedy() {
    let fixture = Fixture::new();
    for command in [
        "tmux future-text-route --text 'git status'",
        "orca terminal future-text-route --text 'git status'",
        "screen -S seat -X future-text-route 'git status'",
        "tmux send-keys -t seat g 'it status' Enter",
        "tmux send-keys -t seat Enter",
        "tmux paste-buffer -t seat",
        "tmux send-keys -H 67 69 74 Enter",
    ] {
        for (hook, rule) in [
            ("git-guard", "git.hook_integrity"),
            ("exec-guard", "security.dangerous_commands"),
        ] {
            let out = fixture.judge(hook, command);
            let said = String::from_utf8_lossy(&out.stderr);
            assert!(
                refused(&out, rule)
                    && said.contains("keep instructions in a brief file and send its path"),
                "{hook}: {command}: {said}"
            );
        }
    }
}

#[test]
fn orca_remaining_text_and_file_routes_refuse_in_both_guards() {
    let fixture = Fixture::new();
    for command in [
        "orca terminal send --terminal term_abc --text 'git status'",
        "orca terminal send --terminal term_abc --enter",
        "orca terminal send --terminal term_abc --text $'\\x1b'",
        "cat commands.txt | xargs orca terminal send --terminal term_abc --text '{}' --enter",
        "orca computer paste-text --app Terminal --text-stdin",
        "orca emulator type --text 'git status'",
        "orca automations create --name n --trigger daily --provider claude --prompt 'codex exec hi'",
        "orca orchestration worker-start --spec-file spec.txt --agent codex",
        "orca orchestration ask --to dispatch:d1 --subject s --body 'git status'",
        "orca orchestration reply --id m1 --body 'git status'",
        "orca orchestration dispatch --task t1 --to term_abc --inject",
        "orca plugin dev hook --project .",
        "orca plugin dev request --project .",
        "orca plugin dev emit --project .",
        "orca plugin test --project .",
        "orca plugin check --project .",
        "orca terminal create --title hc --command 'pnpm dev'",
        "echo orca terminal list",
        "orca terminal create --command \"$COMMAND\"",
    ] {
        for (hook, rule) in [("git-guard", "git.hook_integrity"), ("exec-guard", "security.dangerous_commands")] {
            let out = fixture.judge(hook, command);
            assert!(refused(&out, rule), "{hook}: {command}: {}", String::from_utf8_lossy(&out.stderr));
        }
    }
}

#[test]
fn sibling_terminal_input_rows_refuse_under_the_carrier_rule() {
    let fixture = Fixture::new();
    for command in [
        "tmux send-keys -t peer 'git push --force origin main' Enter",
        "tmux send -t peer 'git push --force origin main' Enter",
        "tmux paste-buffer -t peer",
        "tmux run-shell 'git push --force origin main'",
        "tmux new-session 'git push --force origin main'",
        "tmux new-window 'git push --force origin main'",
        "tmux split-window 'git push --force origin main'",
        "screen -S peer -X stuff 'git push --force origin main\r'",
        "screen -dm git push --force origin main",
        "osascript -e 'tell application \"System Events\" to keystroke \"git push --force origin main\"'",
        "osascript -e 'tell application \"Terminal\" to do script \"git push --force origin main\"'",
        "expect -c 'send \"git push --force origin main\\r\"'",
    ] {
        for (hook, rule) in [("git-guard", "git.hook_integrity"), ("exec-guard", "security.dangerous_commands")] {
            let out = fixture.judge(hook, command);
            assert!(refused(&out, rule), "{hook}: {command}: {}", String::from_utf8_lossy(&out.stderr));
        }
    }
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
