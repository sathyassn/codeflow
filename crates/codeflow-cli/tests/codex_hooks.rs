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
const KNOWN_HOOKS: [&str; 4] = [
    "git-guard",
    "exec-guard",
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
        let sub = command.trim_start_matches("codeflow hook ").trim();
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
                .any(|c| c == &format!("codeflow hook {hook}")),
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
        commands.iter().any(|c| c == "codeflow hook session-orient"),
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
