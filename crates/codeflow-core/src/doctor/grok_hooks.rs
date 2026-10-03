//! Whether Grok can run the `CodeFlow` hooks it loads (TSK-215, issue 29).
//!
//! Grok expands `$name` and `${...}` in a hook command itself and skips the
//! hook, failing open, when a name is unset. A `CodeFlow` command that carries
//! a shell variable therefore never runs in a Grok session. Grok discovers
//! project hooks in `.grok/hooks/*.json` and, with its Claude compatibility
//! on (the default), in `.claude/settings.json` and
//! `.claude/settings.local.json`.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::settings::is_codeflow_command;

/// The tool call the canary sends: a `PreToolUse` payload in the shape Grok
/// Build 1.0.46 sends (every field under both spellings, captured from a
/// live session) for a shell command that the non-relaxable
/// dangerous-command floor refuses under any policy. The guard only reads
/// it; nothing runs the command.
pub(super) const CANARY_PAYLOAD: &str = concat!(
    r#"{"hookEventName":"pre_tool_use","toolName":"run_terminal_command","#,
    r#""toolInput":{"command":"rm -rf /","description":"codeflow doctor canary"},"#,
    r#""hook_event_name":"PreToolUse","tool_name":"run_terminal_command","#,
    r#""tool_input":{"command":"rm -rf /","description":"codeflow doctor canary"}}"#
);

/// The local Claude settings Grok also reads, which `codeflow update` does
/// not manage.
pub(super) const LOCAL_SETTINGS: &str = ".claude/settings.local.json";

/// The files Grok reads project hooks from, in a stable order.
fn hook_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join(".grok").join("hooks"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    for name in ["settings.json", "settings.local.json"] {
        let path = root.join(".claude").join(name);
        if path.is_file() {
            files.push(path);
        }
    }
    files
}

/// Every `CodeFlow` command in a hooks value, with the event it is bound to.
fn codeflow_commands(hooks: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(events) = hooks.as_object() else {
        return out;
    };
    for (event, groups) in events {
        collect(groups, event, &mut out);
    }
    out
}

fn collect(value: &Value, event: &str, out: &mut Vec<(String, String)>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(command)) = map.get("command") {
                if is_codeflow_command(command) {
                    out.push((event.to_string(), command.clone()));
                }
            }
            for child in map.values() {
                collect(child, event, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect(child, event, out);
            }
        }
        _ => {}
    }
}

/// The parsed `hooks` object of each hook file Grok reads, with its path
/// relative to `root`. A file that does not parse is left to the checks
/// that own its format.
fn parsed(root: &Path) -> Vec<(String, Value)> {
    hook_files(root)
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let value: Value = serde_json::from_str(&text).ok()?;
            let hooks = value.get("hooks")?.clone();
            let shown = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            Some((shown, hooks))
        })
        .collect()
}

/// The hook files holding a `CodeFlow` command Grok would skip because it
/// carries a `$`.
pub(super) fn templated(root: &Path) -> Vec<String> {
    parsed(root)
        .into_iter()
        .filter(|(_, hooks)| {
            codeflow_commands(hooks)
                .iter()
                .any(|(_, command)| command.contains('$'))
        })
        .map(|(path, _)| path)
        .collect()
}

/// The names Grok matches a `PreToolUse` matcher against for its shell
/// tool: the tool itself and its Claude alias.
const SHELL_TOOL_NAMES: [&str; 2] = ["run_terminal_command", "Bash"];

/// The hook files `CodeFlow` ships, whose exec-guard commands are the forms
/// doctor recognises.
const SHIPPED_HOOK_FILES: [&str; 5] = [
    include_str!("../../../../assets/base/grok/hooks.json"),
    include_str!("../../../../assets/base/codex/hooks.json"),
    include_str!("../../../../assets/base/settings/default.json"),
    include_str!("../../../../assets/base/settings/acceptEdits.json"),
    include_str!("../../../../assets/base/settings/bypass-sandboxed.json"),
];

/// The exact exec-guard commands `CodeFlow` ships.
fn shipped_exec_guard_commands() -> Vec<String> {
    SHIPPED_HOOK_FILES
        .iter()
        .filter_map(|text| serde_json::from_str::<Value>(text).ok())
        .filter_map(|value| value.get("hooks").cloned())
        .flat_map(|hooks| codeflow_commands(&hooks))
        .filter(|(_, command)| is_exec_guard(command))
        .map(|(_, command)| command)
        .collect()
}

fn is_exec_guard(command: &str) -> bool {
    is_codeflow_command(command) && command.contains("hook exec-guard")
}

/// Whether a `PreToolUse` matcher selects Grok's shell tool: no matcher,
/// an empty one or `*` selects every tool; otherwise the matcher is a
/// regular expression that must match a shell tool name whole.
fn selects_shell(matcher: Option<&Value>) -> bool {
    match matcher {
        None | Some(Value::Null) => true,
        Some(Value::String(matcher)) if matcher.is_empty() || matcher == "*" => true,
        Some(Value::String(matcher)) => regex::Regex::new(&format!("^(?:{matcher})$"))
            .is_ok_and(|re| SHELL_TOOL_NAMES.iter().any(|name| re.is_match(name))),
        Some(_) => false,
    }
}

/// The `CodeFlow` exec-guard commands Grok runs before its shell tool in
/// one hooks value: command handlers in `PreToolUse` groups whose matcher
/// selects the shell tool.
fn shell_guard_commands(hooks: &Value) -> Vec<String> {
    let Some(groups) = hooks.get("PreToolUse").and_then(Value::as_array) else {
        return Vec::new();
    };
    groups
        .iter()
        .filter(|group| selects_shell(group.get("matcher")))
        .filter_map(|group| group.get("hooks").and_then(Value::as_array))
        .flatten()
        .filter(|handler| handler.get("type").and_then(Value::as_str) == Some("command"))
        .filter_map(|handler| handler.get("command").and_then(Value::as_str))
        .filter(|command| is_exec_guard(command))
        .map(str::to_string)
        .collect()
}

/// The exec-guard Grok runs before a shell command, as doctor can know it
/// without running anything from the repository.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ShellGuard {
    /// A shipped exec-guard command is bound where the shell tool hits it.
    Shipped,
    /// Only customised exec-guard commands are bound there; the files that
    /// hold them. Doctor never runs repository hook text, so these stay
    /// unverified.
    Customised(Vec<String>),
    /// No `CodeFlow` exec-guard is bound where the shell tool hits it.
    Missing,
}

/// Which exec-guard Grok would run before its shell tool, across every
/// hook file it reads.
pub(super) fn shell_guard(root: &Path) -> ShellGuard {
    let shipped = shipped_exec_guard_commands();
    let mut customised = Vec::new();
    for (path, hooks) in parsed(root) {
        let commands = shell_guard_commands(&hooks);
        if commands.iter().any(|command| shipped.contains(command)) {
            return ShellGuard::Shipped;
        }
        if !commands.is_empty() {
            customised.push(path);
        }
    }
    if customised.is_empty() {
        ShellGuard::Missing
    } else {
        ShellGuard::Customised(customised)
    }
}

/// Whether a guard's stdout is the deny answer Grok honours: a JSON object
/// whose `decision` is `deny` with a nonempty `reason`.
pub(super) fn is_deny_answer(stdout: &str) -> bool {
    serde_json::from_str::<Value>(stdout.trim()).is_ok_and(|value| {
        value.get("decision").and_then(Value::as_str) == Some("deny")
            && value
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| !reason.trim().is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    const SHIPPED: &str = include_str!("../../../../assets/base/grok/hooks.json");
    const TEMPLATED: &str = r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"codeflow hook exec-guard --contract 3; codeflow_status=$?; exit 2"}]}]}}"#;

    #[test]
    fn the_shipped_grok_hooks_carry_no_template_and_bind_the_shell_guard() {
        let dir = project(&[(".grok/hooks/codeflow.json", SHIPPED)]);
        assert!(templated(dir.path()).is_empty());
        assert_eq!(shell_guard(dir.path()), ShellGuard::Shipped);
    }

    #[test]
    fn every_shipped_hook_file_binds_a_recognised_shell_guard() {
        for text in SHIPPED_HOOK_FILES {
            let hooks: Value = serde_json::from_str(text).unwrap();
            let commands = shell_guard_commands(&hooks["hooks"]);
            assert!(!commands.is_empty(), "{text}");
            assert!(commands
                .iter()
                .all(|command| command.starts_with("codeflow hook exec-guard --contract 3 || ")));
        }
    }

    #[test]
    fn a_matcher_selects_the_shell_only_when_it_matches_a_shell_tool_name_whole() {
        for matcher in [
            Value::Null,
            Value::from(""),
            Value::from("*"),
            Value::from("Bash"),
            Value::from("^(Bash|PowerShell)$"),
            Value::from("run_terminal_command|write"),
        ] {
            assert!(selects_shell(Some(&matcher)), "{matcher}");
        }
        assert!(selects_shell(None));
        for matcher in [
            Value::from("^Read$"),
            Value::from("Bas"),
            Value::from("Edit|Write"),
            Value::from("("),
            Value::from(1),
        ] {
            assert!(!selects_shell(Some(&matcher)), "{matcher}");
        }
    }

    #[test]
    fn a_customised_or_misbound_guard_is_not_the_shipped_one() {
        let customised = SHIPPED.replace(
            "codeflow hook exec-guard --contract 3 || ",
            "codeflow hook exec-guard --contract 3; touch /tmp/x || ",
        );
        let dir = project(&[(".grok/hooks/codeflow.json", &customised)]);
        assert_eq!(
            shell_guard(dir.path()),
            ShellGuard::Customised(vec![".grok/hooks/codeflow.json".into()])
        );
        let read_only = SHIPPED.replace("^(Bash|PowerShell)$", "^Read$");
        let dir = project(&[(".grok/hooks/codeflow.json", &read_only)]);
        assert_eq!(shell_guard(dir.path()), ShellGuard::Missing);
        let prompt = SHIPPED.replace(r#""type": "command""#, r#""type": "prompt""#);
        let dir = project(&[(".grok/hooks/codeflow.json", &prompt)]);
        assert_eq!(shell_guard(dir.path()), ShellGuard::Missing);
    }

    #[test]
    fn only_a_json_deny_with_a_reason_is_a_deny_answer() {
        assert!(is_deny_answer(r#"{"decision":"deny","reason":"blocked"}"#));
        assert!(is_deny_answer(
            "{\"decision\":\"deny\",\"reason\":\"blocked\"}\n"
        ));
        for stdout in [
            "",
            "codeflow exec-guard: BLOCKED",
            r#"{"decision":"allow","reason":"x"}"#,
            r#"{"decision":"deny","reason":" "}"#,
            r#"{"decision":"deny"}"#,
        ] {
            assert!(!is_deny_answer(stdout), "{stdout}");
        }
    }

    #[test]
    fn a_dollar_in_a_codeflow_command_is_named_in_every_file_grok_reads() {
        let dir = project(&[
            (".grok/hooks/codeflow.json", TEMPLATED),
            (".claude/settings.json", TEMPLATED),
            (".claude/settings.local.json", TEMPLATED),
        ]);
        assert_eq!(
            templated(dir.path()),
            [
                ".grok/hooks/codeflow.json",
                ".claude/settings.json",
                ".claude/settings.local.json"
            ]
        );
    }

    #[test]
    fn only_codeflow_hook_commands_count() {
        let other = r#"{"$schema":"x","statusLine":{"type":"command","command":"echo $(pwd)"},"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"my-tool $HOME"}]}]}}"#;
        let dir = project(&[
            (".grok/hooks/codeflow.json", SHIPPED),
            (".claude/settings.json", other),
        ]);
        assert!(templated(dir.path()).is_empty());
    }

    #[test]
    fn the_canary_payload_is_the_shape_grok_sends_and_the_guard_reads() {
        let value: Value = serde_json::from_str(CANARY_PAYLOAD).unwrap();
        assert_eq!(value["toolName"], value["tool_name"]);
        assert_eq!(value["toolInput"], value["tool_input"]);
        let payload = crate::hooks::git_guard::HookPayload::parse(CANARY_PAYLOAD).unwrap();
        assert_eq!(payload.shell_command(), Some("rm -rf /"));
    }

    #[test]
    fn no_exec_guard_on_pre_tool_use_means_no_shell_guard() {
        let elsewhere = r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"codeflow hook exec-guard --contract 3"}]}]}}"#;
        let dir = project(&[(".grok/hooks/codeflow.json", elsewhere)]);
        assert_eq!(shell_guard(dir.path()), ShellGuard::Missing);
        let dir = project(&[(".grok/hooks/codeflow.json", "{}")]);
        assert_eq!(shell_guard(dir.path()), ShellGuard::Missing);
    }
}
