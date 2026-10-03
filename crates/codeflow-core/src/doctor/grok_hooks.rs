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

/// What the exec-guard prints when it refuses a command.
pub(super) const CANARY_REFUSAL: &str = "exec-guard: BLOCKED";

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

/// The first `CodeFlow` exec-guard command bound to `PreToolUse` in a file
/// Grok reads: the shell guard the canary runs.
pub(super) fn canary_command(root: &Path) -> Option<String> {
    parsed(root).into_iter().find_map(|(_, hooks)| {
        codeflow_commands(&hooks)
            .into_iter()
            .find(|(event, command)| event == "PreToolUse" && command.contains("hook exec-guard"))
            .map(|(_, command)| command)
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
        let command = canary_command(dir.path()).unwrap();
        assert!(command.starts_with("codeflow hook exec-guard --contract 3"));
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
    fn no_exec_guard_on_pre_tool_use_means_no_canary() {
        let elsewhere = r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"codeflow hook exec-guard --contract 3"}]}]}}"#;
        let dir = project(&[(".grok/hooks/codeflow.json", elsewhere)]);
        assert_eq!(canary_command(dir.path()), None);
        let dir = project(&[(".grok/hooks/codeflow.json", "{}")]);
        assert_eq!(canary_command(dir.path()), None);
    }
}
