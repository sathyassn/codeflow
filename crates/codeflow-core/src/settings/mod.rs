//! Structured `settings.json` merge helpers.
//!
//! Implements the class-2 (managed-region) ownership contract from charter
//! section 4.3: in a consumer's `.claude/settings.json`, only the keys and
//! hook entries owned by codeflow — identified by the `codeflow` command
//! prefix — are touched; everything else belongs to the user.
//!
//! Imported from v1 and simplified: the v1 settings-template system
//! (template discovery, SHA256 checks, copy mappings, enforcement-policy
//! model) is gone — the v2 scaffold engine's manifest + 3-way merge owns
//! file-level updates. What survives is the structured JSON merge.

use serde_json::Value;

/// Deep merge two JSON values. `managed` is the new baseline, `user`
/// provides overrides-by-omission: keys present only in `user` are
/// preserved; keys present in `managed` take the managed value (recursing
/// into objects so user-only subkeys survive).
#[must_use]
pub fn merge_json(managed: &Value, user: &Value) -> Value {
    match (managed, user) {
        (Value::Object(m), Value::Object(u)) => {
            let mut result = serde_json::Map::new();

            // Managed keys win; recurse where both sides are objects.
            for (k, v) in m {
                if let Some(user_v) = u.get(k) {
                    result.insert(k.clone(), merge_json(v, user_v));
                } else {
                    result.insert(k.clone(), v.clone());
                }
            }

            // Preserve user keys not in the managed set.
            for (k, v) in u {
                if !m.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }

            Value::Object(result)
        }
        // For non-object values, managed wins (it's the new baseline).
        _ => managed.clone(),
    }
}

/// Returns true if a hook command string invokes the codeflow binary.
///
/// Matches `codeflow ...` as well as path-qualified invocations like
/// `/usr/local/bin/codeflow hook git-guard`.
#[must_use]
pub fn is_codeflow_command(command: &str) -> bool {
    let Some(first) = command
        .split([' ', '\t', '\n'])
        .find(|word| !word.is_empty())
    else {
        return false;
    };
    let basename = first.rsplit('/').next().unwrap_or(first);
    basename == "codeflow"
}

/// Returns true if a hook entry (any JSON value) contains a codeflow
/// command anywhere in its tree — the marker that codeflow owns it.
#[must_use]
pub fn is_codeflow_hook_entry(entry: &Value) -> bool {
    match entry {
        Value::Object(map) => {
            if let Some(Value::String(cmd)) = map.get("command") {
                if is_codeflow_command(cmd) {
                    return true;
                }
            }
            map.values().any(is_codeflow_hook_entry)
        }
        Value::Array(arr) => arr.iter().any(is_codeflow_hook_entry),
        _ => false,
    }
}

/// Merge codeflow-managed settings into a user's `settings.json` value.
///
/// Per the class-2 contract:
/// - In the `hooks` object, entries owned by codeflow (any entry containing
///   a `codeflow` command) are removed from every event and replaced by the
///   entries in `managed.hooks`. User hook entries are never touched.
/// - Every other managed key is deep-merged via [`merge_json`]: managed
///   values win, user-only keys survive.
///
/// Returns the merged settings value; neither input is mutated.
#[must_use]
pub fn merge_settings(user: &Value, managed: &Value) -> Value {
    let mut result = if let Value::Object(u) = user {
        u.clone()
    } else {
        serde_json::Map::new()
    };

    let Value::Object(m) = managed else {
        return Value::Object(result);
    };

    for (key, managed_val) in m {
        if key == "hooks" {
            let merged_hooks = merge_hooks(result.get("hooks"), managed_val);
            result.insert("hooks".to_string(), merged_hooks);
        } else {
            let merged = match result.get(key) {
                Some(user_val) => merge_json(managed_val, user_val),
                None => managed_val.clone(),
            };
            result.insert(key.clone(), merged);
        }
    }

    Value::Object(result)
}

/// Merge the `hooks` section: strip codeflow-owned entries from every user
/// event array, then append the managed entries per event.
fn merge_hooks(user_hooks: Option<&Value>, managed_hooks: &Value) -> Value {
    let mut result = serde_json::Map::new();

    // Start from user hooks with codeflow-owned entries stripped.
    if let Some(Value::Object(u)) = user_hooks {
        for (event, entries) in u {
            let kept: Vec<Value> = match entries {
                Value::Array(arr) => arr
                    .iter()
                    .filter(|e| !is_codeflow_hook_entry(e))
                    .cloned()
                    .collect(),
                other => {
                    result.insert(event.clone(), other.clone());
                    continue;
                }
            };
            if !kept.is_empty() {
                result.insert(event.clone(), Value::Array(kept));
            }
        }
    }

    // Append managed entries per event.
    if let Value::Object(m) = managed_hooks {
        for (event, entries) in m {
            let Value::Array(managed_arr) = entries else {
                result.insert(event.clone(), entries.clone());
                continue;
            };
            match result.get_mut(event) {
                Some(Value::Array(existing)) => {
                    existing.extend(managed_arr.iter().cloned());
                }
                _ => {
                    result.insert(event.clone(), Value::Array(managed_arr.clone()));
                }
            }
        }
    }

    Value::Object(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- merge_json --

    #[test]
    fn test_merge_json_managed_wins_on_shared_keys() {
        let managed = json!({"a": 1, "b": {"c": 2}});
        let user = json!({"a": 99, "b": {"c": 100}});
        let merged = merge_json(&managed, &user);
        assert_eq!(merged, json!({"a": 1, "b": {"c": 2}}));
    }

    #[test]
    fn test_merge_json_preserves_user_only_keys() {
        let managed = json!({"a": 1});
        let user = json!({"a": 2, "custom": "mine"});
        let merged = merge_json(&managed, &user);
        assert_eq!(merged, json!({"a": 1, "custom": "mine"}));
    }

    #[test]
    fn test_merge_json_preserves_nested_user_keys() {
        let managed = json!({"outer": {"managed": true}});
        let user = json!({"outer": {"managed": false, "mine": 7}});
        let merged = merge_json(&managed, &user);
        assert_eq!(merged, json!({"outer": {"managed": true, "mine": 7}}));
    }

    #[test]
    fn test_merge_json_non_object_managed_wins() {
        let managed = json!("new");
        let user = json!({"old": true});
        assert_eq!(merge_json(&managed, &user), json!("new"));
    }

    // -- is_codeflow_command --

    #[test]
    fn test_is_codeflow_command_plain() {
        assert!(is_codeflow_command("codeflow hook git-guard"));
        assert!(is_codeflow_command("codeflow"));
    }

    #[test]
    fn test_is_codeflow_command_path_qualified() {
        assert!(is_codeflow_command(
            "/usr/local/bin/codeflow hook session-orient"
        ));
    }

    #[test]
    fn test_is_codeflow_command_negative() {
        assert!(!is_codeflow_command("npx eslint --fix"));
        assert!(!is_codeflow_command("codeflow-other tool"));
        assert!(!is_codeflow_command(""));
        assert!(!is_codeflow_command("echo codeflow"));
    }

    // -- is_codeflow_hook_entry --

    #[test]
    fn test_is_codeflow_hook_entry_nested() {
        let entry = json!({
            "matcher": "Bash",
            "hooks": [{"type": "command", "command": "codeflow hook git-guard"}]
        });
        assert!(is_codeflow_hook_entry(&entry));
    }

    #[test]
    fn test_is_codeflow_hook_entry_user_entry() {
        let entry = json!({
            "matcher": "Bash",
            "hooks": [{"type": "command", "command": "./scripts/my-hook.sh"}]
        });
        assert!(!is_codeflow_hook_entry(&entry));
    }

    // -- merge_settings --

    fn managed_settings() -> Value {
        json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{"type": "command", "command": "codeflow hook git-guard"}]
                }],
                "SessionStart": [{
                    "hooks": [{"type": "command", "command": "codeflow hook session-orient"}]
                }]
            },
            "permissions": {
                "deny": ["Read(**/.env*)"]
            }
        })
    }

    #[test]
    fn test_merge_settings_into_empty() {
        let merged = merge_settings(&json!({}), &managed_settings());
        assert_eq!(merged["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(merged["permissions"]["deny"][0], "Read(**/.env*)");
    }

    #[test]
    fn test_merge_settings_preserves_user_hooks() {
        let user = json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{"type": "command", "command": "./scripts/my-hook.sh"}]
                }]
            }
        });
        let merged = merge_settings(&user, &managed_settings());
        let pre = merged["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2, "user entry + managed entry");
        assert!(!is_codeflow_hook_entry(&pre[0]), "user entry kept first");
        assert!(is_codeflow_hook_entry(&pre[1]), "managed entry appended");
    }

    #[test]
    fn test_merge_settings_replaces_stale_codeflow_hooks() {
        // User has an old codeflow entry on an event the managed set no
        // longer wires — it must be stripped, not duplicated or orphaned.
        let user = json!({
            "hooks": {
                "PostToolUse": [{
                    "hooks": [{"type": "command", "command": "codeflow hook old-post-hook"}]
                }],
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{"type": "command", "command": "codeflow hook old-guard"}]
                }]
            }
        });
        let merged = merge_settings(&user, &managed_settings());
        let hooks = merged["hooks"].as_object().unwrap();
        assert!(
            !hooks.contains_key("PostToolUse"),
            "stale codeflow-only event removed"
        );
        let pre = hooks["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 1);
        assert!(
            pre[0]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .contains("git-guard"),
            "old codeflow entry replaced by the managed one"
        );
    }

    #[test]
    fn test_merge_settings_preserves_user_top_level_keys() {
        let user = json!({
            "model": "opus",
            "statusLine": {"type": "command", "command": "my-statusline"}
        });
        let merged = merge_settings(&user, &managed_settings());
        assert_eq!(merged["model"], "opus");
        assert_eq!(merged["statusLine"]["command"], "my-statusline");
    }

    #[test]
    fn test_merge_settings_deep_merges_permissions() {
        let user = json!({
            "permissions": {
                "allow": ["Bash(npm test)"],
                "deny": ["Read(secrets/**)"]
            }
        });
        let merged = merge_settings(&user, &managed_settings());
        // Managed deny wins; user-only allow key survives.
        assert_eq!(merged["permissions"]["deny"][0], "Read(**/.env*)");
        assert_eq!(merged["permissions"]["allow"][0], "Bash(npm test)");
    }

    #[test]
    fn test_merge_settings_idempotent() {
        let user = json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{"type": "command", "command": "./scripts/my-hook.sh"}]
                }]
            }
        });
        let once = merge_settings(&user, &managed_settings());
        let twice = merge_settings(&once, &managed_settings());
        assert_eq!(once, twice, "re-applying managed settings is a no-op");
    }

    #[test]
    fn test_merge_settings_non_object_user() {
        let merged = merge_settings(&json!(null), &managed_settings());
        assert!(merged.is_object());
        assert!(merged.get("hooks").is_some());
    }
}

#[cfg(test)]
mod r15_text_regressions {
    #[test]
    fn r15_command_word_keeps_unicode_space() {
        assert!(!super::is_codeflow_command("codeflow\u{a0}hook git-guard"));
        assert!(super::is_codeflow_command(" \tcodeflow hook git-guard"));
    }
}
