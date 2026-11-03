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
    // A Monitor call with no `command` field has no shell text to judge. This
    // is only the missing-command case: the field a socket watch uses is
    // unconfirmed, so no real socket form is claimed here.
    let no_command = project.replay("claude", "Monitor", json!({"description": "events"}));
    for output in no_command {
        success(&output);
    }
}
