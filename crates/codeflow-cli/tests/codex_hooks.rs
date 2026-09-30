//! Structural tests for the shipped Codex hooks config
//! (`assets/base/codex/hooks.json`). Codex parity (ADR-0008): the SAME
//! `codeflow hook` binaries that bind a Claude session bind an interactive
//! Codex session — only the wiring file differs, so no handler is duplicated.
//! These tests pin that wiring the way `settings_presets.rs` pins the Claude
//! presets: every command is a real hook subcommand, and the parity-relevant
//! events (guards on Bash, orient on `SessionStart` incl. post-compaction) stay
//! wired.

use std::path::PathBuf;

/// Every `codeflow hook` subcommand codeflow ships. A command naming anything
/// else is a typo caught here. (Codex wires a subset — it has no `SessionEnd`
/// event, so `session-summary` is not expected, but it stays a *known* name.)
const KNOWN_HOOKS: [&str; 5] = [
    "git-guard",
    "exec-guard",
    "edit-guard",
    "session-orient",
    "session-summary",
];

fn hooks_json() -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/codex/hooks.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("codex hooks.json is not valid JSON: {e}"))
}

/// Collect every `command` string of a `{"type": "command", ...}` hook object
/// anywhere under the given value.
fn collect_hook_commands(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            if map.get("type").and_then(serde_json::Value::as_str) == Some("command") {
                let command = map
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .expect("command hook carries a command string");
                out.push(command.to_string());
            }
            for v in map.values() {
                collect_hook_commands(v, out);
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                collect_hook_commands(v, out);
            }
        }
        _ => {}
    }
}

#[test]
fn parses_as_json_object_with_hooks() {
    let v = hooks_json();
    assert!(v.is_object(), "top level must be an object");
    assert!(v.get("hooks").is_some(), "hooks key missing");
}

#[test]
fn every_hook_command_is_a_known_codeflow_hook() {
    let v = hooks_json();
    let mut commands = Vec::new();
    collect_hook_commands(&v["hooks"], &mut commands);
    assert!(!commands.is_empty(), "no hook commands found");
    for command in &commands {
        assert!(
            command.starts_with("codeflow hook "),
            "hook command {command:?} must start with \"codeflow hook \""
        );
        let sub = command
            .trim_start_matches("codeflow hook ")
            .split_whitespace()
            .next()
            .unwrap();
        assert!(
            KNOWN_HOOKS.contains(&sub),
            "hook command {command:?} names unknown subcommand {sub:?}"
        );
    }
}

#[test]
fn pretooluse_binds_git_and_exec_guard_on_supported_shells() {
    let v = hooks_json();
    let pre = v["hooks"]["PreToolUse"].clone();
    let mut commands = Vec::new();
    collect_hook_commands(&pre, &mut commands);
    for hook in ["git-guard", "exec-guard"] {
        assert!(
            commands
                .iter()
                .any(|c| c.starts_with(&format!("codeflow hook {hook} --contract 3"))),
            "PreToolUse: {hook} not wired"
        );
    }
    // Keep the shared hook payload compatible with Unix/WSL Bash and native
    // Windows PowerShell command events.
    let matcher = pre[0]["matcher"].as_str().unwrap_or_default();
    assert!(
        matcher.contains("Bash"),
        "PreToolUse matcher must target Bash, got {matcher:?}"
    );
    assert!(
        matcher.contains("PowerShell"),
        "PreToolUse matcher must target PowerShell, got {matcher:?}"
    );
}

#[test]
fn sessionstart_wires_orient_across_all_sources() {
    // Codex parity: orient runs at session start AND after a compaction
    // (source=compact), so the matcher must cover the compact source — that is
    // the post-compaction re-orientation half of the recovery loop.
    let v = hooks_json();
    let start = v["hooks"]["SessionStart"].clone();
    assert!(!start.is_null(), "SessionStart not wired for Codex");
    let mut commands = Vec::new();
    collect_hook_commands(&start, &mut commands);
    assert!(
        commands
            .iter()
            .any(|c| c.starts_with("codeflow hook session-orient --contract 3")),
        "SessionStart: session-orient not wired"
    );
    let matcher = start[0]["matcher"].as_str().unwrap_or_default();
    // Both headline behaviors must stay covered: `startup` = a session opens with
    // the digest; `compact` = it re-orients after a compaction. (resume/clear are
    // wired too, but these two are the behaviors the capability claims.)
    for source in ["startup", "compact"] {
        assert!(
            matcher.contains(source),
            "SessionStart matcher must include the {source:?} source, got {matcher:?}"
        );
    }
}

#[test]
fn guards_and_orient_stay_on_their_own_events() {
    // The events are not interchangeable: a mis-wire that adds a guard to
    // SessionStart, or orient to PreToolUse, must fail here.
    let v = hooks_json();
    let mut pre = Vec::new();
    collect_hook_commands(&v["hooks"]["PreToolUse"], &mut pre);
    let mut start = Vec::new();
    collect_hook_commands(&v["hooks"]["SessionStart"], &mut start);
    assert!(
        !pre.iter().any(|c| c.contains("session-orient")),
        "orient must not ride PreToolUse — it is a per-session digest, not a per-tool gate"
    );
    for guard in ["git-guard", "exec-guard"] {
        assert!(
            !start.iter().any(|c| c.contains(guard)),
            "{guard} must not ride SessionStart — guards gate tool calls, not session open"
        );
    }
}

#[test]
fn dogfood_codex_hooks_matches_shipped_scaffold() {
    // This repo is its own first consumer: the dogfood `.codex/hooks.json` must
    // stay byte-identical to the scaffold it ships, or the two silently diverge.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let shipped = std::fs::read_to_string(root.join("assets/base/codex/hooks.json"))
        .expect("read shipped codex hooks.json");
    let dogfood = std::fs::read_to_string(root.join(".codex/hooks.json"))
        .expect("read dogfood .codex/hooks.json");
    assert_eq!(
        shipped, dogfood,
        "dogfood .codex/hooks.json drifted from assets/base/codex/hooks.json"
    );
}

/// TSK-128: Grok Build's session, compaction and prompt events exist, but it
/// ignores their stdout and discards an allowing prompt hook's output
/// (Grok Build 1.0.41 hook reference): events present, context injection
/// unavailable. So the Grok file wires only the guards, the same payload as
/// Codex, and no advisory `session-orient` whose output no model receives.
#[test]
fn dogfood_grok_hooks_share_pretooluse_and_wire_no_advisory_hook() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let shipped = std::fs::read_to_string(root.join("assets/base/grok/hooks.json"))
        .expect("read shipped grok hooks.json");
    let dogfood = std::fs::read_to_string(root.join(".grok/hooks/codeflow.json"))
        .expect("read dogfood .grok/hooks/codeflow.json");
    assert_eq!(
        shipped, dogfood,
        "dogfood Grok hooks drifted from assets/base/grok/hooks.json"
    );
    let grok: serde_json::Value = serde_json::from_str(&shipped).unwrap();
    let codex: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("assets/base/codex/hooks.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        grok["hooks"]["PreToolUse"][0], codex["hooks"]["PreToolUse"][0],
        "Grok PreToolUse must stay the same git-guard/exec-guard payload as Codex"
    );
    let events: Vec<&String> = grok["hooks"].as_object().unwrap().keys().collect();
    assert_eq!(events, vec!["PreToolUse"], "Grok wires only the guards");
    let mut commands = Vec::new();
    collect_hook_commands(&grok["hooks"], &mut commands);
    assert!(
        !commands
            .iter()
            .any(|c| c.contains("session-orient") || c.contains("prompt-reminder")),
        "an advisory hook on Grok would claim context that never reaches the model"
    );
}

/// TSK-128 AC-5: Codex wires `UserPromptSubmit` to the stable advisory
/// entry, where plain stdout becomes developer context, with no matcher
/// (Codex ignores one on this event) and never the manual command.
#[test]
fn user_prompt_submit_wires_the_stable_advisory_entry() {
    let v = hooks_json();
    let prompt = v["hooks"]["UserPromptSubmit"].clone();
    let mut commands = Vec::new();
    collect_hook_commands(&prompt, &mut commands);
    assert_eq!(commands.len(), 1);
    assert!(commands[0].starts_with("codeflow hook session-orient --contract 3"));
    assert!(prompt[0].get("matcher").is_none());
    let mut all = Vec::new();
    collect_hook_commands(&v["hooks"], &mut all);
    assert!(!all.iter().any(|c| c.contains("prompt-reminder")));
}

#[path = "../../codeflow-core/src/security/guard_forms.rs"]
#[allow(dead_code)]
mod guard_forms;

/// Run the shipped Codex exec-guard wiring through the shell, as Codex
/// does, with a Bash tool call for `command` on stdin.
#[cfg(unix)]
fn run_codex_exec_guard(root: &std::path::Path, command: &str) -> std::process::Output {
    use std::io::Write as _;
    let mut commands = Vec::new();
    collect_hook_commands(&hooks_json()["hooks"]["PreToolUse"], &mut commands);
    let hook = commands
        .into_iter()
        .find(|c| c.starts_with("codeflow hook exec-guard --contract 3"))
        .expect("exec-guard wired");
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(
        exe.parent()
            .map(std::path::Path::to_path_buf)
            .into_iter()
            .chain(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            )),
    )
    .unwrap();
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "cwd": root,
    });
    let mut child = std::process::Command::new("sh")
        .args(["-c", &hook])
        .current_dir(root)
        .env("PATH", path)
        .env("CODEFLOW_HOME", root.join(".home"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// TSK-141 AC-5: the Codex wiring refuses each composed deletion, passes a
/// project deletion, and lets a help invocation through while its data twin
/// is reported.
#[cfg(unix)]
#[test]
fn the_codex_exec_guard_wiring_judges_composed_deletions_and_help() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (form, _) in guard_forms::COMPOSED_PAIRS {
        let out = run_codex_exec_guard(root, form);
        assert_eq!(out.status.code(), Some(2), "{form}");
    }
    for command in guard_forms::PROJECT_DELETIONS {
        let out = run_codex_exec_guard(root, command);
        assert_eq!(out.status.code(), Some(0), "{command}");
        assert!(out.stderr.is_empty(), "{command}");
    }
    for (help, twin) in guard_forms::HELP_PAIRS {
        let out = run_codex_exec_guard(root, help);
        assert!(out.stderr.is_empty(), "{help}");
        let out = run_codex_exec_guard(root, twin);
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("headless peer run"),
            "{twin}"
        );
    }
}

/// TSK-180: the built hook runs a cleanup after a trap reset or a removed
/// hook, and refuses one whose reset or removal may not take effect.
#[cfg(unix)]
#[test]
fn the_codex_exec_guard_wiring_holds_the_round_four_probes() {
    use guard_forms::Expect;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::os::unix::fs::symlink("/", root.join("root-link")).unwrap();
    for sub in ["build", "empty"] {
        std::fs::create_dir(root.join(sub)).unwrap();
    }
    let mut wrong = Vec::new();
    for (case, command, expect) in guard_forms::REVIEW_ROUND_FOUR_PROBES {
        let out = run_codex_exec_guard(root, command);
        let want = if *expect == Expect::Allowed { 0 } else { 2 };
        if out.status.code() != Some(want) {
            wrong.push(format!(
                "{case} ({expect:?}): {command} -> {:?} {}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

#[test]
fn native_edit_tools_are_wired_to_the_edit_guard() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (harness, tools) in [
        ("codex", vec!["apply_patch", "Edit", "Write"]),
        ("grok", vec!["write", "search_replace", "Edit", "Write"]),
    ] {
        let value: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(format!("assets/base/{harness}/hooks.json")))
                .unwrap(),
        )
        .unwrap();
        let entry = &value["hooks"]["PreToolUse"][1];
        let matcher = regex::Regex::new(entry["matcher"].as_str().unwrap()).unwrap();
        for tool in tools {
            assert!(matcher.is_match(tool), "{harness}: {tool}");
        }
        assert!(!matcher.is_match("Bash"));
        assert!(entry["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .starts_with("codeflow hook edit-guard --contract 3"));
    }
}
