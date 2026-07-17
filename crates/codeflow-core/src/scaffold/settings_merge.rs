//! Structured merge for `.claude/settings.json` (charter §4.3.2).
//!
//! Codeflow owns exactly two kinds of keys inside the user's settings file:
//!
//! - **hook entries** whose command starts with the `codeflow` binary name
//!   (`"codeflow hook git-guard"` etc.) — added on init, regenerated on
//!   update (stale codeflow hooks not present in the shipped preset are
//!   removed); user hooks are never touched;
//! - **permission entries** (`permissions.deny` / `ask` / `allow` arrays) —
//!   union-added, never removed; scalar permission keys (`defaultMode`) are
//!   set only when absent.
//! - **sandbox entries** — recursively add missing keys and union shipped array
//!   entries, while preserving every explicit user scalar. This lets security
//!   additions such as fail-closed startup reach an existing partial sandbox
//!   without silently changing a user's chosen value.
//!
//! Every other key — user or shipped — is preserved verbatim: shipped
//! top-level keys (`statusLine`, `$schema`) are added only when
//! the user file lacks them. Every mutation is reported.

use serde_json::{Map, Value};

use super::ScaffoldError;

/// Commands with this prefix (or exactly equal to the binary name) are
/// codeflow-managed hook entries.
const COMMAND_PREFIX: &str = "codeflow ";

fn is_codeflow_command(cmd: &str) -> bool {
    cmd == "codeflow" || cmd.starts_with(COMMAND_PREFIX)
}

fn hook_command(hook: &Value) -> Option<&str> {
    hook.get("command").and_then(Value::as_str)
}

fn matcher_of(group: &Value) -> Option<&str> {
    group.get("matcher").and_then(Value::as_str)
}

/// Merges the shipped settings preset into the user's settings file.
/// Returns the merged JSON text (pretty, 2-space) and appends one line per
/// mutation/preservation decision to `report`.
///
/// # Errors
///
/// Fails when either side is not valid JSON or not a top-level object.
pub fn merge_settings(
    current: &str,
    incoming: &str,
    report: &mut Vec<String>,
) -> Result<String, ScaffoldError> {
    let mut current: Value = serde_json::from_str(current)?;
    let incoming: Value = serde_json::from_str(incoming)?;

    let Value::Object(cur) = &mut current else {
        return Err(ScaffoldError::InvalidState {
            what: "settings.json".to_string(),
            detail: "top level is not a JSON object".to_string(),
        });
    };
    let Value::Object(inc) = &incoming else {
        return Err(ScaffoldError::InvalidState {
            what: "shipped settings preset".to_string(),
            detail: "top level is not a JSON object".to_string(),
        });
    };

    for (key, inc_val) in inc {
        match key.as_str() {
            "hooks" => merge_hooks(cur, inc_val, report),
            "permissions" => merge_permissions(cur, inc_val, report),
            "sandbox" => merge_sandbox(cur, inc_val, report),
            _ => {
                if cur.contains_key(key) {
                    if cur[key] != *inc_val {
                        report.push(format!("settings: preserved user value for \"{key}\""));
                    }
                } else {
                    cur.insert(key.clone(), inc_val.clone());
                    report.push(format!("settings: added \"{key}\""));
                }
            }
        }
    }

    serde_json::to_string_pretty(&current)
        .map(|mut s| {
            s.push('\n');
            s
        })
        .map_err(ScaffoldError::from)
}

fn merge_sandbox(cur: &mut Map<String, Value>, inc_sandbox: &Value, report: &mut Vec<String>) {
    let Some(inc_sandbox) = inc_sandbox.as_object() else {
        return;
    };
    let cur_sandbox = cur
        .entry("sandbox")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(cur_sandbox) = cur_sandbox.as_object_mut() else {
        report.push("settings: \"sandbox\" is not an object; left untouched".to_string());
        return;
    };
    merge_additive_object(cur_sandbox, inc_sandbox, "sandbox", report);
}

fn merge_additive_object(
    current: &mut Map<String, Value>,
    incoming: &Map<String, Value>,
    path: &str,
    report: &mut Vec<String>,
) {
    for (key, inc_val) in incoming {
        let key_path = format!("{path}.{key}");
        match (current.get_mut(key), inc_val) {
            (Some(Value::Object(cur_obj)), Value::Object(inc_obj)) => {
                merge_additive_object(cur_obj, inc_obj, &key_path, report);
            }
            (Some(Value::Array(cur_arr)), Value::Array(inc_arr)) => {
                for item in inc_arr {
                    if !cur_arr.contains(item) {
                        cur_arr.push(item.clone());
                        report.push(format!("settings: added {key_path} entry {item}"));
                    }
                }
            }
            (Some(existing), _) => {
                if existing != inc_val {
                    report.push(format!("settings: preserved user value for {key_path}"));
                }
            }
            (None, _) => {
                current.insert(key.clone(), inc_val.clone());
                report.push(format!("settings: added {key_path}"));
            }
        }
    }
}

fn merge_hooks(cur: &mut Map<String, Value>, inc_hooks: &Value, report: &mut Vec<String>) {
    let Some(inc_events) = inc_hooks.as_object() else {
        return;
    };
    let cur_hooks = cur
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(cur_events) = cur_hooks.as_object_mut() else {
        report.push("settings: \"hooks\" is not an object; left untouched".to_string());
        return;
    };

    for (event, inc_groups) in inc_events {
        let Some(inc_groups) = inc_groups.as_array() else {
            continue;
        };
        let cur_groups = cur_events
            .entry(event.clone())
            .or_insert_with(|| Value::Array(vec![]));
        let Some(cur_groups) = cur_groups.as_array_mut() else {
            report.push(format!(
                "settings: hooks.{event} is not an array; left untouched"
            ));
            continue;
        };

        // The set of codeflow commands the shipped preset wants for this event.
        let wanted: Vec<&str> = inc_groups
            .iter()
            .filter_map(|g| g.get("hooks").and_then(Value::as_array))
            .flatten()
            .filter_map(hook_command)
            .filter(|c| is_codeflow_command(c))
            .collect();

        // Drop stale codeflow hooks (regenerate-the-keys semantics); never
        // touch user hooks.
        for group in cur_groups.iter_mut() {
            let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                continue;
            };
            hooks.retain(|h| {
                let Some(cmd) = hook_command(h) else {
                    return true;
                };
                if is_codeflow_command(cmd) && !wanted.contains(&cmd) {
                    report.push(format!("settings: removed stale hook {event}: \"{cmd}\""));
                    false
                } else {
                    true
                }
            });
        }
        cur_groups.retain(|g| {
            g.get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|h| !h.is_empty())
        });

        // Add missing codeflow hooks into the matching matcher group.
        for inc_group in inc_groups {
            let inc_matcher = matcher_of(inc_group);
            let Some(inc_hooks) = inc_group.get("hooks").and_then(Value::as_array) else {
                continue;
            };
            let codeflow_hooks: Vec<&Value> = inc_hooks
                .iter()
                .filter(|h| hook_command(h).is_some_and(is_codeflow_command))
                .collect();
            if codeflow_hooks.is_empty() {
                continue;
            }

            let existing = cur_groups.iter_mut().find(|g| matcher_of(g) == inc_matcher);
            if let Some(group) = existing {
                let hooks = group
                    .as_object_mut()
                    .expect("hook group is an object")
                    .entry("hooks")
                    .or_insert_with(|| Value::Array(vec![]));
                if let Some(hooks) = hooks.as_array_mut() {
                    for h in codeflow_hooks {
                        let cmd = hook_command(h).unwrap_or_default();
                        if !hooks.iter().any(|e| hook_command(e) == Some(cmd)) {
                            hooks.push(h.clone());
                            report.push(format!("settings: added hook {event}: \"{cmd}\""));
                        }
                    }
                }
            } else {
                cur_groups.push(inc_group.clone());
                for h in &codeflow_hooks {
                    let cmd = hook_command(h).unwrap_or_default();
                    report.push(format!("settings: added hook {event}: \"{cmd}\""));
                }
            }
        }
    }
}

fn merge_permissions(cur: &mut Map<String, Value>, inc_perms: &Value, report: &mut Vec<String>) {
    let Some(inc_perms) = inc_perms.as_object() else {
        return;
    };
    let cur_perms = cur
        .entry("permissions")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(cur_perms) = cur_perms.as_object_mut() else {
        report.push("settings: \"permissions\" is not an object; left untouched".to_string());
        return;
    };

    for (key, inc_val) in inc_perms {
        match (cur_perms.get_mut(key), inc_val) {
            (Some(Value::Array(cur_arr)), Value::Array(inc_arr)) => {
                for item in inc_arr {
                    if !cur_arr.contains(item) {
                        cur_arr.push(item.clone());
                        report.push(format!("settings: added permissions.{key} entry {item}"));
                    }
                }
            }
            (Some(existing), _) => {
                if existing != inc_val {
                    report.push(format!(
                        "settings: preserved user value for permissions.{key}"
                    ));
                }
            }
            (None, _) => {
                cur_perms.insert(key.clone(), inc_val.clone());
                report.push(format!("settings: added permissions.{key}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRESET: &str = r#"{
        "permissions": {
            "defaultMode": "default",
            "deny": ["Read(**/.env)", "Read(**/*.pem)"]
        },
        "hooks": {
            "PreToolUse": [
                {"matcher": "Bash", "hooks": [{"type": "command", "command": "codeflow hook git-guard"}]}
            ],
            "SessionStart": [
                {"hooks": [{"type": "command", "command": "codeflow hook session-orient"}]}
            ]
        },
        "statusLine": {"type": "command", "command": "echo cf"}
        ,"sandbox": {
            "enabled": true,
            "failIfUnavailable": true,
            "allowUnsandboxedCommands": false,
            "network": {"allowLocalBinding": true}
        }
    }"#;

    #[test]
    fn preserves_foreign_keys_and_user_hooks() {
        let user = r#"{
            "model": "opus",
            "permissions": {"defaultMode": "plan", "deny": ["Read(secrets/**)"]},
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"type": "command", "command": "./my-guard.sh"}]}
                ]
            }
        }"#;
        let mut report = vec![];
        let merged = merge_settings(user, PRESET, &mut report).unwrap();
        let v: Value = serde_json::from_str(&merged).unwrap();

        assert_eq!(v["model"], "opus");
        assert_eq!(v["permissions"]["defaultMode"], "plan");
        let deny = v["permissions"]["deny"].as_array().unwrap();
        assert!(deny.iter().any(|d| d == "Read(secrets/**)"));
        assert!(deny.iter().any(|d| d == "Read(**/.env)"));
        let bash_hooks = v["hooks"]["PreToolUse"][0]["hooks"].as_array().unwrap();
        assert!(bash_hooks.iter().any(|h| h["command"] == "./my-guard.sh"));
        assert!(bash_hooks
            .iter()
            .any(|h| h["command"] == "codeflow hook git-guard"));
        assert_eq!(
            v["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            "codeflow hook session-orient"
        );
        assert_eq!(v["statusLine"]["command"], "echo cf");
        assert!(report.iter().any(|l| l.contains("preserved user value")));
        assert!(report.iter().any(|l| l.contains("git-guard")));
    }

    #[test]
    fn idempotent_on_remerge() {
        let mut report = vec![];
        let once = merge_settings("{}", PRESET, &mut report).unwrap();
        let mut report2 = vec![];
        let twice = merge_settings(&once, PRESET, &mut report2).unwrap();
        assert_eq!(once, twice);
        assert!(
            report2.is_empty(),
            "second merge reports nothing: {report2:?}"
        );
    }

    #[test]
    fn removes_stale_codeflow_hooks_only() {
        let user = r#"{
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [
                        {"type": "command", "command": "codeflow hook old-guard"},
                        {"type": "command", "command": "./mine.sh"}
                    ]}
                ]
            }
        }"#;
        let mut report = vec![];
        let merged = merge_settings(user, PRESET, &mut report).unwrap();
        let v: Value = serde_json::from_str(&merged).unwrap();
        let hooks = v["hooks"]["PreToolUse"][0]["hooks"].as_array().unwrap();
        assert!(!hooks
            .iter()
            .any(|h| h["command"] == "codeflow hook old-guard"));
        assert!(hooks.iter().any(|h| h["command"] == "./mine.sh"));
        assert!(report.iter().any(|l| l.contains("removed stale hook")));
    }

    #[test]
    fn sandbox_adds_missing_guards_but_preserves_explicit_values() {
        let user = r#"{
            "sandbox": {
                "enabled": false,
                "network": {"allowedDomains": ["internal.example"]}
            }
        }"#;
        let mut report = vec![];
        let merged = merge_settings(user, PRESET, &mut report).unwrap();
        let v: Value = serde_json::from_str(&merged).unwrap();

        assert_eq!(v["sandbox"]["enabled"], false);
        assert_eq!(v["sandbox"]["failIfUnavailable"], true);
        assert_eq!(v["sandbox"]["allowUnsandboxedCommands"], false);
        assert_eq!(v["sandbox"]["network"]["allowLocalBinding"], true);
        assert_eq!(
            v["sandbox"]["network"]["allowedDomains"][0],
            "internal.example"
        );
        assert!(report
            .iter()
            .any(|line| line.contains("preserved user value for sandbox.enabled")));
    }

    #[test]
    fn new_ask_rule_reaches_consumers_with_a_stale_allow_entry() {
        let user = r#"{
            "permissions": {
                "allow": ["Bash(git restore *)"]
            }
        }"#;
        let incoming = r#"{
            "permissions": {
                "allow": [],
                "ask": ["Bash(git restore *)"]
            }
        }"#;

        let mut report = vec![];
        let merged = merge_settings(user, incoming, &mut report).unwrap();
        let value: Value = serde_json::from_str(&merged).unwrap();

        assert!(value["permissions"]["allow"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry == "Bash(git restore *)"));
        assert!(value["permissions"]["ask"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry == "Bash(git restore *)"));
        assert!(report
            .iter()
            .any(|line| line.contains("added permissions.ask")));
    }
}
