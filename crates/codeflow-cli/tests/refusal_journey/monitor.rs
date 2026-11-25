//! TSK-1058: the installed Claude hooks judge a `Monitor` command exactly as
//! they judge the same text in `Bash`. The `Monitor` tool takes its shell
//! text in `tool_input.command` (confirmed in the Claude Code 2.1.317
//! binary, see `docs/verification/tsk-1058-monitor-tool-input.md`). Payload
//! replay proves the installed path, not native harness dispatch.
use super::*;

fn refused_by(outputs: &[Output], rule: &str) -> bool {
    outputs.iter().any(|out| {
        out.status.code() == Some(2) && String::from_utf8_lossy(&out.stderr).contains(rule)
    })
}

#[test]
fn monitor_command_is_refused_where_the_same_bash_text_is_refused() {
    let project = Project::new();
    for (command, rule) in [
        ("timeout 1 sudo true", "security.privilege_escalation"),
        ("git push origin v1.2.3", "security.outward_actions"),
        ("gh -R o/r release create v1", "security.outward_actions"),
        // A git-policy text, so the criterion's "refused by git-guard" is
        // met by git-guard's own rule and not only by exec-guard's.
        ("git push --force origin main", "git.force_push_protected"),
    ] {
        let bash = project.replay("claude", "Bash", json!({"command": command}));
        let monitor = project.replay("claude", "Monitor", json!({"command": command}));
        assert!(refused_by(&bash, rule), "Bash {command}: {bash:?}");
        assert!(refused_by(&monitor, rule), "Monitor {command}: {monitor:?}");
    }
}

#[test]
fn monitor_runs_the_same_hooks_as_bash_and_allows_ordinary_watching() {
    let project = Project::new();
    let bash = project.replay("claude", "Bash", json!({"command": "git status"}));
    let monitor = project.replay(
        "claude",
        "Monitor",
        json!({
            "command": "git status",
            "description": "watch the tree",
            "timeout_ms": 60000,
            "persistent": false
        }),
    );
    assert_eq!(bash.len(), monitor.len(), "same hook chain");
    for output in monitor {
        success(&output);
    }
    // A socket watch carries no shell text; nothing to judge. `ws.url` is the
    // field the 2.1.317 tool schema reports.
    let socket = project.replay(
        "claude",
        "Monitor",
        json!({"ws": {"url": "wss://example.invalid/events"}, "description": "events"}),
    );
    for output in socket {
        success(&output);
    }
}

#[test]
fn monitor_input_the_judge_does_not_recognise_is_refused() {
    let project = Project::new();
    for input in [
        json!({"description": "events"}),
        json!({"script": "timeout 1 sudo true"}),
        json!({"command": "git status", "script": "timeout 1 sudo true"}),
        json!({"command": "git status", "ws": {"url": "wss://example.invalid/e"}}),
    ] {
        let outputs = project.replay("claude", "Monitor", input.clone());
        assert!(!outputs.is_empty(), "no hook ran for {input}");
        assert!(
            refused_by(&outputs, "unrecognised Monitor payload"),
            "{input}: {outputs:?}"
        );
    }
}
