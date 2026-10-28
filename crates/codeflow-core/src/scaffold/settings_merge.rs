//! Structured merge for `.claude/settings.json` (charter §4.3.2).
//!
//! Codeflow owns these kinds of keys inside the user's settings file:
//!
//! - **hook entries** whose command starts with the `codeflow` binary name
//!   (`"codeflow hook git-guard"` etc.) — added on init, regenerated on
//!   update (stale codeflow hooks not present in the shipped preset are
//!   removed); user hooks are never touched;
//! - **permission entries** (`permissions.deny` / `ask` / `allow` arrays) —
//!   merged three ways on update against the prior shipped baseline
//!   (ADR-0075, TSK-171): an entry the preset retired is removed, an entry
//!   the project removed stays removed and is reported, a project-only entry
//!   stays, and a new entry is added; shipped entries keep the preset's
//!   order, since a `!` exception lifts only the rules before it. No entry
//!   the project has moves when that would put one of its own exceptions
//!   past a rule it did not lift before. Without a recorded baseline for an
//!   array, nothing is removed or moved: new exceptions go first and new
//!   rules last, so none can lift or be lifted by a project rule. An earlier
//!   release's preset then only retires an array this preset dropped
//!   ([`merge_settings_from_prior_release`]). Scalar permission keys
//!   (`defaultMode`) are set only when absent.
//! - **sandbox entries** — recursively add missing keys and union shipped array
//!   entries, while preserving every explicit user scalar. On update, the prior
//!   shipped baseline also identifies retired managed array entries; those are
//!   removed while user-only entries survive. This lets security additions and
//!   boundary corrections reach existing projects without taking ownership of
//!   project-specific sandbox configuration.
//! - **env entries** — the shipped `env` keys are added to a project that
//!   lacks them, unless the recorded baseline shows codeflow wrote a key
//!   before and the project removed it since; every entry the project has
//!   keeps its value, and an `env` that is not an object of strings is
//!   reported and left untouched.
//!
//! - **personal preference keys** ([`PREFERENCE_KEYS`]: `statusLine`,
//!   `effortLevel`) — merged against the recorded baseline (TSK-1028): a
//!   value still equal to the one codeflow recorded moves to the new shipped
//!   value, a value the project changed stays, a shipped key the project
//!   removed stays removed, and a key this preset no longer ships is removed
//!   while it still holds the recorded value. With no recorded baseline, a
//!   missing key is added and every existing value stays.
//! - **every other shipped top-level key** (`$schema`, and `attribution`
//!   and `includeCoAuthoredBy`, which keep the host from adding the AI
//!   attribution project policy forbids) — added whenever the project lacks
//!   it, a removal included, and never changed once present.
//!
//! A key this preset does not ship stays verbatim, except a preference key
//! the recorded baseline holds as shipped with the same value, which is
//! retired. Every mutation is reported.

use serde_json::{Map, Value};

use super::ScaffoldError;

/// Commands with this prefix (or exactly equal to the binary name) are
/// codeflow-managed hook entries.
const COMMAND_PREFIX: &str = "codeflow ";

/// The report wording for a permission entry the project removed and update
/// keeps removed.
pub const KEPT_REMOVAL: &str = "kept the project's removal of";

/// The start of every report line about a top-level value the project
/// changed that update keeps while the shipped default moved or retired.
const KEPT_OWN: &str = "kept the project's \"";

/// The start of every report line about a malformed `env`.
const ENV_MALFORMED: &str = "settings: \"env\" ";

/// The wording of every report line about a shipped key, in `env` or at the
/// top level, that update did not add.
const ENV_UNSET: &str = "stays unset";

/// Whether a report line describes a standing condition that update reports
/// on every run, even when the settings file does not change: a permission
/// entry the project removed, a malformed `env`, a shipped `env` or
/// top-level key update did not add, or a project value update kept while
/// the shipped default moved or retired. Such lines name keys, never values.
#[must_use]
pub fn is_standing_note(line: &str) -> bool {
    line.contains(KEPT_REMOVAL)
        || line.contains(KEPT_OWN)
        || line.starts_with(ENV_MALFORMED)
        || line.contains(ENV_UNSET)
}

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
    merge_settings_from_baseline(current, None, incoming, report)
}

/// Merges settings during update, using the prior shipped baseline to retire
/// the CodeFlow-managed permission and sandbox array entries and the plain
/// top-level keys the new preset removed, to move a top-level value still at
/// its recorded value to the new one, and to keep a permission entry or key
/// the project removed from coming back. User-only entries are preserved.
///
/// # Errors
///
/// Returns an error when current, previous, or incoming settings are invalid
/// JSON, or when current or incoming settings are not top-level objects.
pub fn merge_settings_from_baseline(
    current: &str,
    previous: Option<&str>,
    incoming: &str,
    report: &mut Vec<String>,
) -> Result<String, ScaffoldError> {
    merge(current, previous, incoming, Previous::Recorded, report)
}

/// Merges settings during an update that found no recorded baseline, using
/// the preset an earlier release shipped (`prior`) only for a permission
/// array this preset no longer ships (the 2.x `ask` array): its entries that
/// release shipped are retired. Every other array keeps the project's
/// entries exactly in their order, with new exceptions placed before them
/// and new rules after them. Membership in an older release does not show where the
/// project meant an entry to sit among its own `!` exceptions, so no
/// existing entry is moved, and a shipped entry the project lacks is added
/// back, since a removal cannot be told apart from an entry the older
/// version never had.
///
/// # Errors
///
/// As [`merge_settings_from_baseline`].
pub fn merge_settings_from_prior_release(
    current: &str,
    prior: &str,
    incoming: &str,
    report: &mut Vec<String>,
) -> Result<String, ScaffoldError> {
    merge(
        current,
        Some(prior),
        incoming,
        Previous::PriorRelease,
        report,
    )
}

/// What the previous side of a merge is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Previous {
    /// The shipped copy recorded at the last install: it says which entries
    /// were shipped and which the project removed.
    Recorded,
    /// An earlier release's copy standing in for a missing baseline: it
    /// only retires the entries of an array this preset dropped.
    PriorRelease,
}

fn merge(
    current: &str,
    previous: Option<&str>,
    incoming: &str,
    kind: Previous,
    report: &mut Vec<String>,
) -> Result<String, ScaffoldError> {
    let mut current: Value = serde_json::from_str(current)?;
    let incoming: Value = serde_json::from_str(incoming)?;
    let previous: Option<Value> = previous.map(serde_json::from_str).transpose()?;

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
            "permissions" => merge_permissions(
                cur,
                previous.as_ref().and_then(|value| value.get("permissions")),
                inc_val,
                kind,
                report,
            ),
            "sandbox" => merge_sandbox(
                cur,
                previous
                    .as_ref()
                    .filter(|_| kind == Previous::Recorded)
                    .and_then(|value| value.get("sandbox")),
                inc_val,
                report,
            ),
            "env" => merge_env(
                cur,
                previous.as_ref().filter(|_| kind == Previous::Recorded),
                inc_val,
                report,
            ),
            _ => merge_top_level_key(
                cur,
                key,
                previous
                    .as_ref()
                    .filter(|_| kind == Previous::Recorded)
                    .filter(|_| PREFERENCE_KEYS.contains(&key.as_str()))
                    .and_then(|value| value.get(key)),
                inc_val,
                report,
            ),
        }
    }
    if kind == Previous::Recorded {
        if let Some(Value::Object(baseline)) = &previous {
            retire_top_level_keys(cur, baseline, inc, report);
        }
    }

    serde_json::to_string_pretty(&current)
        .map(|mut s| {
            s.push('\n');
            s
        })
        .map_err(ScaffoldError::from)
}

/// The top-level keys that are each person's preference, not policy
/// (TSK-1028, ADR-0087): update moves, keeps or retires them against the
/// recorded baseline. Every other plain key, such as the attribution
/// controls, is restored whenever a project lacks it, so a deletion never
/// weakens what it guarantees.
pub const PREFERENCE_KEYS: [&str; 2] = ["statusLine", "effortLevel"];

/// Merge one shipped top-level value such as `statusLine` (TSK-1028).
/// `shipped_before` is the value the recorded baseline holds for this key,
/// `None` when there is no recorded baseline, it lacks the key, or the key
/// is not one of [`PREFERENCE_KEYS`].
///
/// - A value equal to the one the baseline recorded is still codeflow's, so
///   it moves to the new shipped value.
/// - Any other value is the project's and stays. When the shipped default
///   changed, the report says so even if nothing else in the file changes.
/// - A key the baseline recorded and the project no longer has was removed
///   by the project and stays removed, reported on every update.
/// - Otherwise a missing key is added.
fn merge_top_level_key(
    cur: &mut Map<String, Value>,
    key: &str,
    shipped_before: Option<&Value>,
    inc_val: &Value,
    report: &mut Vec<String>,
) {
    match cur.get_mut(key) {
        Some(existing) if existing == inc_val => {}
        Some(existing) if Some(&*existing) == shipped_before => {
            *existing = inc_val.clone();
            report.push(format!(
                "settings: updated \"{key}\" to the new shipped default"
            ));
        }
        Some(_) if shipped_before.is_some_and(|before| before != inc_val) => {
            report.push(format!(
                "settings: {KEPT_OWN}{key}\"; the shipped default changed"
            ));
        }
        Some(_) => report.push(format!("settings: preserved user value for \"{key}\"")),
        None if shipped_before.is_some() => report.push(format!(
            "settings: \"{key}\" {ENV_UNSET}; update does not restore a shipped key \
             the project removed"
        )),
        None => {
            cur.insert(key.to_string(), inc_val.clone());
            report.push(format!("settings: added \"{key}\""));
        }
    }
}

/// Retire the [`PREFERENCE_KEYS`] the recorded baseline carried and this
/// preset no longer ships, such as `effortLevel` (TSK-1028): a value still
/// equal to the recorded one is removed, a value the project changed stays.
/// Both are reported. No other key is ever retired here.
fn retire_top_level_keys(
    cur: &mut Map<String, Value>,
    baseline: &Map<String, Value>,
    incoming: &Map<String, Value>,
    report: &mut Vec<String>,
) {
    for (key, shipped_before) in baseline {
        if !PREFERENCE_KEYS.contains(&key.as_str()) || incoming.contains_key(key) {
            continue;
        }
        match cur.get(key) {
            Some(existing) if existing == shipped_before => {
                cur.shift_remove(key);
                report.push(format!(
                    "settings: removed \"{key}\"; this release no longer ships it"
                ));
            }
            Some(_) => report.push(format!(
                "settings: {KEPT_OWN}{key}\"; this release no longer ships it"
            )),
            None => {}
        }
    }
}

/// Merge the shipped `env` keys into the project's `env` without changing any
/// entry the project has. `baseline` is the shipped settings recorded at the
/// last install or update, or `None` when no such record exists.
///
/// - With no `env`, the shipped keys are added, except a key the baseline
///   shows codeflow wrote before: the project removed it, so it stays out.
/// - With an `env` object, a shipped key the project lacks is added only when
///   the baseline exists and did not carry it. A key the baseline carried was
///   removed by the project; with no baseline, update cannot tell a removal
///   from a key the project never had, so it adds nothing.
/// - An `env` that is not an object of strings is reported and left as is.
///
/// Report lines name keys only, never values, since `env` may hold secrets.
fn merge_env(
    cur: &mut Map<String, Value>,
    baseline: Option<&Value>,
    inc_env: &Value,
    report: &mut Vec<String>,
) {
    let Some(inc_env) = inc_env.as_object() else {
        return;
    };
    let shipped_before = baseline
        .and_then(|value| value.get("env"))
        .and_then(Value::as_object);
    let removed = |key: &str| shipped_before.is_some_and(|env| env.contains_key(key));
    let kept = |key: &str| {
        format!(
            "settings: env.{key} {ENV_UNSET}; update does not restore a shipped key \
             it wrote before"
        )
    };
    match cur.get_mut("env") {
        None => {
            let mut added = Map::new();
            for (key, value) in inc_env {
                if removed(key) {
                    report.push(kept(key));
                } else {
                    added.insert(key.clone(), value.clone());
                    report.push(format!("settings: added env.{key}"));
                }
            }
            if !added.is_empty() {
                cur.insert("env".to_string(), Value::Object(added));
            }
        }
        Some(Value::Object(env)) => {
            if env.values().any(|value| !value.is_string()) {
                report.push(format!(
                    "{ENV_MALFORMED}has a value that is not a string; left untouched"
                ));
                return;
            }
            for (key, value) in inc_env {
                match env.get(key) {
                    Some(existing) => {
                        if existing != value {
                            report.push(format!("settings: preserved user value for env.{key}"));
                        }
                    }
                    None if baseline.is_none() => report.push(format!(
                        "settings: env.{key} {ENV_UNSET}; no recorded baseline shows \
                         whether the project removed it from its own \"env\""
                    )),
                    None if removed(key) => report.push(kept(key)),
                    None => {
                        env.insert(key.clone(), value.clone());
                        report.push(format!("settings: added env.{key}"));
                    }
                }
            }
        }
        Some(_) => {
            report.push(format!("{ENV_MALFORMED}is not an object; left untouched"));
        }
    }
}

fn merge_sandbox(
    cur: &mut Map<String, Value>,
    previous_sandbox: Option<&Value>,
    inc_sandbox: &Value,
    report: &mut Vec<String>,
) {
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
    if let Some(previous_sandbox) = previous_sandbox.and_then(Value::as_object) {
        remove_retired_array_entries(
            cur_sandbox,
            previous_sandbox,
            inc_sandbox,
            "sandbox",
            report,
        );
    }
    merge_additive_object(cur_sandbox, inc_sandbox, "sandbox", report);
}

fn remove_retired_array_entries(
    current: &mut Map<String, Value>,
    previous: &Map<String, Value>,
    incoming: &Map<String, Value>,
    path: &str,
    report: &mut Vec<String>,
) {
    for (key, previous_value) in previous {
        let Some(incoming_value) = incoming.get(key) else {
            continue;
        };
        let Some(current_value) = current.get_mut(key) else {
            continue;
        };
        let key_path = format!("{path}.{key}");
        match (current_value, previous_value, incoming_value) {
            (Value::Object(current), Value::Object(previous), Value::Object(incoming)) => {
                remove_retired_array_entries(current, previous, incoming, &key_path, report);
            }
            (Value::Array(current), Value::Array(previous), Value::Array(incoming)) => {
                current.retain(|entry| {
                    let retired = previous.contains(entry) && !incoming.contains(entry);
                    if retired {
                        report.push(format!(
                            "settings: removed retired {key_path} entry {entry}"
                        ));
                    }
                    !retired
                });
            }
            _ => {}
        }
    }
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

        // The codeflow commands the shipped preset wants for this event, each
        // with the matcher of the group that carries it.
        let wanted: Vec<(Option<&str>, &str)> = inc_groups
            .iter()
            .flat_map(|g| {
                let matcher = matcher_of(g);
                g.get("hooks")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(hook_command)
                    .filter(|c| is_codeflow_command(c))
                    .map(move |c| (matcher, c))
            })
            .collect();

        retire_codeflow_hooks(event, cur_groups, &wanted, report);
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

/// Drop stale codeflow hooks (regenerate-the-keys semantics), and take a
/// wanted one out of a group whose matcher is not the shipped one, so the
/// add pass puts it back under the shipped matcher instead of running it
/// twice; never touch user hooks.
fn retire_codeflow_hooks(
    event: &str,
    cur_groups: &mut [Value],
    wanted: &[(Option<&str>, &str)],
    report: &mut Vec<String>,
) {
    for group in cur_groups.iter_mut() {
        let group_matcher = matcher_of(group).map(str::to_string);
        let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            continue;
        };
        hooks.retain(|h| {
            let Some(cmd) = hook_command(h) else {
                return true;
            };
            if !is_codeflow_command(cmd) {
                return true;
            }
            if !wanted.iter().any(|(_, c)| *c == cmd) {
                report.push(format!("settings: removed stale hook {event}: \"{cmd}\""));
                return false;
            }
            if wanted
                .iter()
                .any(|(m, c)| *c == cmd && *m == group_matcher.as_deref())
            {
                return true;
            }
            report.push(format!(
                "settings: moved hook {event}: \"{cmd}\" to the shipped matcher"
            ));
            false
        });
    }
}

fn merge_permissions(
    cur: &mut Map<String, Value>,
    previous_perms: Option<&Value>,
    inc_perms: &Value,
    kind: Previous,
    report: &mut Vec<String>,
) {
    let Some(inc_perms) = inc_perms.as_object() else {
        return;
    };
    let previous = previous_perms.and_then(Value::as_object);
    let cur_perms = cur
        .entry("permissions")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(cur_perms) = cur_perms.as_object_mut() else {
        report.push("settings: \"permissions\" is not an object; left untouched".to_string());
        return;
    };

    // An array the preset dropped entirely (the 2.x `ask` array) retires
    // every entry it shipped; a project-only entry keeps the array alive.
    if let Some(previous) = previous {
        for (key, prev_val) in previous {
            if inc_perms.contains_key(key) {
                continue;
            }
            let (Some(prev_arr), Some(Value::Array(cur_arr))) =
                (prev_val.as_array(), cur_perms.get_mut(key))
            else {
                continue;
            };
            retire_entries(key, cur_arr, prev_arr, &[], report);
            if cur_arr.is_empty() {
                cur_perms.remove(key);
                report.push(format!(
                    "settings: removed permissions.{key}: the preset no longer ships it"
                ));
            }
        }
    }

    for (key, inc_val) in inc_perms {
        let prev_arr = previous
            .and_then(|previous| previous.get(key))
            .and_then(Value::as_array);
        match (cur_perms.get_mut(key), inc_val) {
            (Some(Value::Array(cur_arr)), Value::Array(inc_arr)) => {
                let previous = prev_arr
                    .filter(|_| kind == Previous::Recorded)
                    .map(Vec::as_slice);
                merge_array(key, cur_arr, previous, inc_arr, report);
            }
            (Some(existing), _) => {
                if existing != inc_val {
                    report.push(format!(
                        "settings: preserved user value for permissions.{key}"
                    ));
                }
            }
            (None, _) => {
                if kind == Previous::Recorded && prev_arr.is_some() && inc_val.is_array() {
                    report.push(format!(
                        "settings: {KEPT_REMOVAL} permissions.{key}; update does not restore it"
                    ));
                    continue;
                }
                cur_perms.insert(key.clone(), inc_val.clone());
                report.push(format!("settings: added permissions.{key}"));
            }
        }
    }
}

/// Whether a permission entry is a `!` exception, such as `Read(!**/x)`,
/// which lifts the rules listed before it.
fn is_exception(entry: &Value) -> bool {
    entry
        .as_str()
        .and_then(|rule| rule.split_once('('))
        .is_some_and(|(_, pattern)| pattern.starts_with('!'))
}

/// Add the shipped entries a project's array lacks without moving any entry
/// it already has, so every one of its rules keeps its meaning. A new
/// exception goes before every existing entry, where it lifts nothing, and a
/// new rule after every existing entry, where no exception can lift it. A
/// new exception then may not reach an older rule the project carries,
/// which errs toward denying. An entry `removed` lists was shipped before
/// and the project removed it: it is reported, not added.
fn add_around_project_entries(
    key: &str,
    current: &mut Vec<Value>,
    incoming: &[Value],
    removed: &[Value],
    report: &mut Vec<String>,
) {
    let mut front: Vec<Value> = Vec::new();
    let mut back: Vec<Value> = Vec::new();
    for item in incoming {
        if current.contains(item) || front.contains(item) || back.contains(item) {
            continue;
        }
        if removed.contains(item) {
            report.push(format!(
                "settings: {KEPT_REMOVAL} permissions.{key} entry {item}; \
                 update does not restore it"
            ));
            continue;
        }
        report.push(format!("settings: added permissions.{key} entry {item}"));
        if is_exception(item) {
            front.push(item.clone());
        } else {
            back.push(item.clone());
        }
    }
    if !front.is_empty() && !current.is_empty() {
        report.push(format!(
            "settings: kept the order of the project's permissions.{key} entries; \
             the new exceptions go first and may not lift an older rule"
        ));
    }
    front.append(current);
    front.append(&mut back);
    *current = front;
}

/// Merge one permission array. The result lists the preset's entries in
/// its order, since a `!` exception lifts only the rules before it, then
/// the project's own entries in their order, when that keeps the meaning of
/// every rule the project has ([`keeps_meaning`]); otherwise no entry
/// moves ([`add_around_project_entries`]). With the prior shipped baseline
/// (`previous`), an entry the preset retired is removed and a shipped entry
/// the project removed stays removed and is reported.
fn merge_array(
    key: &str,
    current: &mut Vec<Value>,
    previous: Option<&[Value]>,
    incoming: &[Value],
    report: &mut Vec<String>,
) {
    if let Some(previous) = previous {
        retire_entries(key, current, previous, incoming, report);
    }
    let removed = previous.unwrap_or_default();
    let mut next: Vec<Value> = Vec::with_capacity(incoming.len() + current.len());
    let mut lines = Vec::new();
    for item in incoming {
        if current.contains(item) {
            next.push(item.clone());
        } else if removed.contains(item) {
            lines.push(format!(
                "settings: {KEPT_REMOVAL} permissions.{key} entry {item}; \
                 update does not restore it"
            ));
        } else if !next.contains(item) {
            next.push(item.clone());
            lines.push(format!("settings: added permissions.{key} entry {item}"));
        }
    }
    for item in current.iter() {
        if !incoming.contains(item) && !next.contains(item) {
            next.push(item.clone());
        }
    }
    if keeps_meaning(current, &next, previous.is_some()) {
        report.extend(lines);
        *current = next;
    } else {
        add_around_project_entries(key, current, incoming, removed, report);
    }
}

/// Whether `next` keeps the meaning of every rule in `current`: no existing
/// exception moves past a rule that followed it, which it would then lift;
/// and, unless the prior shipped baseline says which entries were shipped
/// (`known_shipped`), no new exception follows an existing rule, since an
/// entry equal to a shipped one may be the project's own.
fn keeps_meaning(current: &[Value], next: &[Value], known_shipped: bool) -> bool {
    let at = |entry: &Value| next.iter().position(|e| e == entry);
    let moved_past = current.iter().enumerate().any(|(i, exception)| {
        is_exception(exception)
            && current[i + 1..].iter().any(|rule| {
                !is_exception(rule)
                    && matches!((at(exception), at(rule)), (Some(e), Some(r)) if e > r)
            })
    });
    let new_lifts_old = !known_shipped
        && next.iter().enumerate().any(|(j, exception)| {
            is_exception(exception)
                && !current.contains(exception)
                && next[..j]
                    .iter()
                    .any(|rule| !is_exception(rule) && current.contains(rule))
        });
    !moved_past && !new_lifts_old
}

/// Remove each entry the prior shipped preset carried and the new one does
/// not; entries the project added itself are never in `previous`.
fn retire_entries(
    key: &str,
    current: &mut Vec<Value>,
    previous: &[Value],
    incoming: &[Value],
    report: &mut Vec<String>,
) {
    current.retain(|entry| {
        let retired = previous.contains(entry) && !incoming.contains(entry);
        if retired {
            report.push(format!(
                "settings: removed retired permissions.{key} entry {entry}"
            ));
        }
        !retired
    });
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
    fn attribution_keys_are_added_to_an_older_consumer_file() {
        let current = r#"{"effortLevel":"high","env":{"KEEP":"1"},"hooks":{}}"#;
        let incoming = r#"{"effortLevel":"high","includeCoAuthoredBy":false,"attribution":{"commit":"","pr":""},"hooks":{}}"#;
        let mut report = vec![];
        let merged = merge_settings(current, incoming, &mut report).unwrap();
        let value: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["includeCoAuthoredBy"], false);
        assert_eq!(value["attribution"]["commit"], "");
        assert_eq!(value["attribution"]["pr"], "");
        assert_eq!(value["env"]["KEEP"], "1");
        assert!(report
            .iter()
            .any(|l| l.contains("added \"includeCoAuthoredBy\"")));
        assert!(report.iter().any(|l| l.contains("added \"attribution\"")));
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

    /// An earlier release's shipped top-level keys (TSK-1028): the
    /// branch-only status line and the effort pin.
    const OLD_TOP_LEVEL: &str = r#"{
        "statusLine": {"type": "command", "command": "printf old"},
        "effortLevel": "high"
    }"#;

    /// This release's: a new status line and no effort pin.
    const NEW_TOP_LEVEL: &str = r#"{
        "statusLine": {"type": "command", "command": "printf new"}
    }"#;

    fn merged_value(current: &str, baseline: Option<&str>) -> (Value, Vec<String>) {
        let mut report = vec![];
        let merged =
            merge_settings_from_baseline(current, baseline, NEW_TOP_LEVEL, &mut report).unwrap();
        (serde_json::from_str(&merged).unwrap(), report)
    }

    #[test]
    fn a_shipped_key_the_project_deleted_is_not_added_back() {
        // The adopter's case in issue 351: both keys deleted from the shared
        // file, then `codeflow update`.
        let (merged, report) = merged_value("{}", Some(OLD_TOP_LEVEL));
        assert!(merged.get("statusLine").is_none(), "{merged}");
        assert!(merged.get("effortLevel").is_none(), "{merged}");
        let line = "settings: \"statusLine\" stays unset; update does not restore \
                    a shipped key the project removed";
        assert!(report.iter().any(|l| l == line), "{report:?}");
        assert!(is_standing_note(line));

        // The next update, against the baseline this one records, keeps it so.
        let mut again = vec![];
        let twice =
            merge_settings_from_baseline("{}", Some(NEW_TOP_LEVEL), NEW_TOP_LEVEL, &mut again)
                .unwrap();
        assert!(serde_json::from_str::<Value>(&twice).unwrap()["statusLine"].is_null());
    }

    #[test]
    fn a_status_line_still_at_the_shipped_value_moves_to_the_new_default() {
        let (merged, report) = merged_value(OLD_TOP_LEVEL, Some(OLD_TOP_LEVEL));
        assert_eq!(merged["statusLine"]["command"], "printf new");
        assert!(report
            .iter()
            .any(|l| l == "settings: updated \"statusLine\" to the new shipped default"));
    }

    #[test]
    fn a_status_line_the_project_changed_stays_and_is_reported() {
        let current = r#"{"statusLine": {"type": "command", "command": "my-line"}}"#;
        let (merged, report) = merged_value(current, Some(OLD_TOP_LEVEL));
        assert_eq!(merged["statusLine"]["command"], "my-line");
        let kept = "settings: kept the project's \"statusLine\"; the shipped default changed";
        assert!(report.iter().any(|l| l == kept), "{report:?}");
        assert!(is_standing_note(kept));

        // Once the baseline records the new default, the kept line is an
        // ordinary preserved value.
        let mut again = vec![];
        merge_settings_from_baseline(current, Some(NEW_TOP_LEVEL), NEW_TOP_LEVEL, &mut again)
            .unwrap();
        assert_eq!(again, ["settings: preserved user value for \"statusLine\""]);
        assert!(!is_standing_note(&again[0]));
    }

    #[test]
    fn an_effort_pin_at_the_shipped_value_is_removed_and_a_changed_one_kept() {
        let (merged, report) = merged_value(OLD_TOP_LEVEL, Some(OLD_TOP_LEVEL));
        assert!(merged.get("effortLevel").is_none(), "{merged}");
        assert!(report
            .iter()
            .any(|l| l == "settings: removed \"effortLevel\"; this release no longer ships it"));

        // The removal keeps the order of the keys around it.
        let ordered = r#"{"a": 1, "effortLevel": "high", "b": 2, "c": 3}"#;
        let (merged, _) = merged_value(ordered, Some(OLD_TOP_LEVEL));
        let keys: Vec<&String> = merged.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["a", "b", "c"]);

        let chosen = r#"{"effortLevel": "xhigh"}"#;
        let (merged, report) = merged_value(chosen, Some(OLD_TOP_LEVEL));
        assert_eq!(merged["effortLevel"], "xhigh");
        let kept = "settings: kept the project's \"effortLevel\"; this release no longer ships it";
        assert!(report.iter().any(|l| l == kept), "{report:?}");
        assert!(is_standing_note(kept));

        // Once removed, later updates never bring it back.
        let mut again = vec![];
        let twice =
            merge_settings_from_baseline("{}", Some(NEW_TOP_LEVEL), NEW_TOP_LEVEL, &mut again)
                .unwrap();
        assert!(serde_json::from_str::<Value>(&twice).unwrap()["effortLevel"].is_null());
    }

    /// The shipped default preset, the recorded baseline of a fresh install.
    const SHIPPED: &str = include_str!("../../../../assets/base/settings/default.json");

    /// Review of PR 368: a deleted attribution control, or any other
    /// security key, comes back on update even with a recorded baseline;
    /// only the preference keys keep a project's deletion.
    #[test]
    fn deleted_security_keys_are_restored_with_a_recorded_baseline() {
        let shipped: Value = serde_json::from_str(SHIPPED).unwrap();
        for key in [
            "attribution",
            "includeCoAuthoredBy",
            "hooks",
            "sandbox",
            "$schema",
        ] {
            let mut current = shipped.clone();
            current.as_object_mut().unwrap().shift_remove(key);
            let mut report = vec![];
            let merged = merge_settings_from_baseline(
                &current.to_string(),
                Some(SHIPPED),
                SHIPPED,
                &mut report,
            )
            .unwrap();
            let merged: Value = serde_json::from_str(&merged).unwrap();
            assert_eq!(merged[key], shipped[key], "{key} was not restored");
            assert!(
                !report.iter().any(|line| line.contains("stays unset")),
                "{key}: {report:?}"
            );
        }
        assert_eq!(PREFERENCE_KEYS, ["statusLine", "effortLevel"]);
    }

    /// A security key a later preset stopped shipping is never retired by
    /// the preference rule, even while it holds the recorded value.
    #[test]
    fn only_preference_keys_are_retired() {
        let mut report = vec![];
        let merged =
            merge_settings_from_baseline(SHIPPED, Some(SHIPPED), NEW_TOP_LEVEL, &mut report)
                .unwrap();
        let merged: Value = serde_json::from_str(&merged).unwrap();
        let shipped: Value = serde_json::from_str(SHIPPED).unwrap();
        for key in [
            "$schema",
            "attribution",
            "includeCoAuthoredBy",
            "hooks",
            "permissions",
            "sandbox",
            "env",
        ] {
            assert_eq!(merged[key], shipped[key], "{key} was retired");
        }
        assert!(
            !report.iter().any(|line| line.contains("no longer ships")),
            "{report:?}"
        );
    }

    #[test]
    fn without_a_recorded_baseline_top_level_values_stay() {
        // No record proves the project's value is one CodeFlow wrote, so the
        // old line and the effort pin stay, and a missing line is added.
        let mut report = vec![];
        let merged = merge_settings(OLD_TOP_LEVEL, NEW_TOP_LEVEL, &mut report).unwrap();
        let merged: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(merged["statusLine"]["command"], "printf old");
        assert_eq!(merged["effortLevel"], "high");

        let mut prior_report = vec![];
        let prior = merge_settings_from_prior_release(
            OLD_TOP_LEVEL,
            OLD_TOP_LEVEL,
            NEW_TOP_LEVEL,
            &mut prior_report,
        )
        .unwrap();
        let prior: Value = serde_json::from_str(&prior).unwrap();
        assert_eq!(prior["statusLine"]["command"], "printf old");
        assert_eq!(prior["effortLevel"], "high");

        let (added, _) = merged_value("{}", None);
        assert_eq!(added["statusLine"]["command"], "printf new");
    }

    /// The shipped compaction keys, as the presets carry them.
    const ENV_PRESET: &str = r#"{
        "env": {
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "1000000",
            "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "50"
        }
    }"#;

    fn env_of(merged: &str) -> Value {
        serde_json::from_str::<Value>(merged).unwrap()["env"].clone()
    }

    /// No report line carries an `env` value, which may be a secret.
    fn assert_no_values(report: &[String], values: &[&str]) {
        for line in report {
            for value in values {
                assert!(!line.contains(value), "report printed a value: {line}");
            }
        }
    }

    #[test]
    fn a_project_without_env_gains_the_shipped_keys() {
        for baseline in [None, Some("{}"), Some(PRESET)] {
            let mut report = vec![];
            let merged =
                merge_settings_from_baseline(PRESET, baseline, ENV_PRESET, &mut report).unwrap();
            assert_eq!(
                env_of(&merged),
                serde_json::json!({
                    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "1000000",
                    "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "50"
                }),
                "baseline {baseline:?}"
            );
            assert!(report
                .iter()
                .any(|line| line == "settings: added env.CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"));
            assert_no_values(&report, &["1000000", "\"50\""]);

            let mut again = vec![];
            let twice =
                merge_settings_from_baseline(&merged, Some(ENV_PRESET), ENV_PRESET, &mut again)
                    .unwrap();
            assert_eq!(merged, twice);
            assert!(again.is_empty(), "{again:?}");
        }
    }

    #[test]
    fn a_project_env_keeps_its_values_and_gains_a_key_codeflow_never_wrote() {
        let user = r#"{
            "env": {
                "KEEP_PROJECT_VALUE": "opaque-secret",
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "750000"
            }
        }"#;
        let mut report = vec![];
        let merged =
            merge_settings_from_baseline(user, Some("{}"), ENV_PRESET, &mut report).unwrap();
        assert_eq!(
            env_of(&merged),
            serde_json::json!({
                "KEEP_PROJECT_VALUE": "opaque-secret",
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "750000",
                "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "50"
            })
        );
        assert!(report
            .iter()
            .any(|line| line
                == "settings: preserved user value for env.CLAUDE_CODE_AUTO_COMPACT_WINDOW"));
        assert!(report
            .iter()
            .any(|line| line == "settings: added env.CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"));
        assert_no_values(&report, &["opaque-secret", "750000", "1000000"]);
    }

    #[test]
    fn a_key_the_project_removed_or_changed_stays_its_choice() {
        let user = r#"{"env": {"CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "30"}}"#;
        let mut report = vec![];
        let merged =
            merge_settings_from_baseline(user, Some(ENV_PRESET), ENV_PRESET, &mut report).unwrap();
        assert_eq!(
            env_of(&merged),
            serde_json::json!({"CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "30"})
        );
        assert!(report.iter().any(|line| line
            == "settings: env.CLAUDE_CODE_AUTO_COMPACT_WINDOW stays unset; \
                update does not restore a shipped key it wrote before"));
        // The kept key is a standing note, reported on every update; the
        // preserved value is not.
        for line in &report {
            assert_eq!(
                is_standing_note(line),
                line.contains("stays unset"),
                "{line}"
            );
        }
        assert!(report
            .iter()
            .any(|line| line
                == "settings: preserved user value for env.CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"));
        assert_no_values(&report, &["\"30\"", "1000000"]);

        let mut again = vec![];
        let twice = merge_settings_from_baseline(&merged, Some(ENV_PRESET), ENV_PRESET, &mut again)
            .unwrap();
        assert_eq!(merged, twice);
    }

    #[test]
    fn a_removed_env_object_stays_removed() {
        let mut report = vec![];
        let merged =
            merge_settings_from_baseline("{}", Some(ENV_PRESET), ENV_PRESET, &mut report).unwrap();
        let merged: Value = serde_json::from_str(&merged).unwrap();
        assert!(merged.get("env").is_none(), "{merged}");
        assert_eq!(report.len(), 2, "{report:?}");
    }

    #[test]
    fn without_a_baseline_an_existing_env_gains_nothing() {
        let user = r#"{"env": {"KEEP_PROJECT_VALUE": "yes"}}"#;
        let mut report = vec![];
        let merged = merge_settings(user, ENV_PRESET, &mut report).unwrap();
        assert_eq!(
            env_of(&merged),
            serde_json::json!({"KEEP_PROJECT_VALUE": "yes"})
        );
        assert!(report
            .iter()
            .all(|line| line.contains("no recorded baseline shows whether")
                && is_standing_note(line)));
        assert_eq!(report.len(), 2);

        // An earlier release's preset is no record of what this project had.
        let mut prior_report = vec![];
        let prior =
            merge_settings_from_prior_release(user, PRESET, ENV_PRESET, &mut prior_report).unwrap();
        assert_eq!(
            env_of(&prior),
            serde_json::json!({"KEEP_PROJECT_VALUE": "yes"})
        );
        let mut absent_report = vec![];
        let absent =
            merge_settings_from_prior_release("{}", PRESET, ENV_PRESET, &mut absent_report)
                .unwrap();
        assert_eq!(env_of(&absent)["CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"], "50");
    }

    #[test]
    fn a_malformed_env_is_reported_and_left_as_is() {
        for (user, line) in [
            (
                r#"{"env": ["not", "a", "string map"]}"#,
                "settings: \"env\" is not an object; left untouched",
            ),
            (
                r#"{"env": {"PORT": 8080}}"#,
                "settings: \"env\" has a value that is not a string; left untouched",
            ),
        ] {
            for baseline in [None, Some("{}"), Some(ENV_PRESET)] {
                let mut report = vec![];
                let merged =
                    merge_settings_from_baseline(user, baseline, ENV_PRESET, &mut report).unwrap();
                let expected: Value = serde_json::from_str(user).unwrap();
                assert_eq!(env_of(&merged), expected["env"]);
                assert_eq!(report, vec![line.to_string()]);
                assert!(is_standing_note(line), "{line}");
            }
        }
    }

    /// A shipped matcher change moves the codeflow hook to the new group
    /// once; the user's hooks stay where they were (TSK-128).
    #[test]
    fn a_matcher_change_moves_the_codeflow_hook_without_a_duplicate() {
        let user = r#"{
            "hooks": {
                "SessionStart": [
                    {"hooks": [
                        {"type": "command", "command": "codeflow hook session-orient"},
                        {"type": "command", "command": "./my-start.sh"}
                    ]}
                ]
            }
        }"#;
        let incoming = r#"{
            "hooks": {
                "SessionStart": [
                    {"matcher": "startup|resume|clear|compact", "hooks": [
                        {"type": "command", "command": "codeflow hook session-orient"}
                    ]}
                ]
            }
        }"#;
        let mut report = vec![];
        let merged = merge_settings(user, incoming, &mut report).unwrap();
        let v: Value = serde_json::from_str(&merged).unwrap();
        let groups = v["hooks"]["SessionStart"].as_array().unwrap();
        let orient: Vec<&Value> = groups
            .iter()
            .filter(|g| {
                g["hooks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|h| h["command"] == "codeflow hook session-orient")
            })
            .collect();
        assert_eq!(orient.len(), 1, "{merged}");
        assert_eq!(orient[0]["matcher"], "startup|resume|clear|compact");
        assert!(groups
            .iter()
            .any(|g| g.get("matcher").is_none() && g["hooks"][0]["command"] == "./my-start.sh"));
        assert!(report.iter().any(|l| l.contains("moved hook SessionStart")));

        let mut again = vec![];
        let twice = merge_settings(&merged, incoming, &mut again).unwrap();
        assert_eq!(merged, twice);
        assert!(again.is_empty(), "{again:?}");
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
    fn sandbox_update_retires_managed_array_entries_and_preserves_user_entries() {
        let previous = r#"{
            "sandbox": {"filesystem": {"allowRead": ["~/.claude/plugins"]}}
        }"#;
        let user = r#"{
            "sandbox": {"filesystem": {"allowRead": [
                "~/.claude/plugins",
                "~/project-owned-reference"
            ]}}
        }"#;
        let incoming = r#"{
            "sandbox": {"filesystem": {"allowRead": ["~/.claude/plugins/cache"]}}
        }"#;

        let mut report = vec![];
        let merged =
            merge_settings_from_baseline(user, Some(previous), incoming, &mut report).unwrap();
        let value: Value = serde_json::from_str(&merged).unwrap();
        let allow_read = value["sandbox"]["filesystem"]["allowRead"]
            .as_array()
            .unwrap();

        assert_eq!(
            allow_read,
            &[
                Value::String("~/project-owned-reference".to_string()),
                Value::String("~/.claude/plugins/cache".to_string()),
            ]
        );
        assert!(report
            .iter()
            .any(|line| { line.contains("removed retired sandbox.filesystem.allowRead entry") }));
        assert!(report
            .iter()
            .any(|line| { line.contains("added sandbox.filesystem.allowRead entry") }));

        let mut second_report = vec![];
        let second =
            merge_settings_from_baseline(&merged, Some(incoming), incoming, &mut second_report)
                .unwrap();
        assert_eq!(second, merged);
        assert!(second_report.is_empty());
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
