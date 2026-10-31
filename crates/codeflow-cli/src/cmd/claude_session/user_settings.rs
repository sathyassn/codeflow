//! Optional additions to the person's own Claude Code user settings,
//! `~/.claude/settings.json`, offered one item at a time by
//! `codeflow claude-session --install` (TSK-1070, operator decision
//! 2026-10-31).
//!
//! The user file applies to every Claude Code session on the machine, so
//! nothing here is written without a typed yes per item. Each item shows
//! the entries it would add first. Accepted items are merged into the
//! file, the file as it was is kept as a backup, and what was added is
//! recorded in `~/.codeflow/claude-user-settings.json` so that
//! `--uninstall` removes exactly those entries and nothing the person
//! added. Bypass mode and the host list are not offered here: they live in
//! Codeflow's session file, which applies only inside Codeflow projects.

use std::io::BufRead;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

/// The Herdr exclusion's denies, which keep its operator commands the
/// person's (`cf-customize` personal-settings reference).
pub const HERDR_DENIES: [&str; 7] = [
    "Bash(herdr update*)",
    "Bash(herdr machine*)",
    "Bash(herdr server*)",
    "Bash(herdr config*)",
    "Bash(herdr integration*)",
    "Bash(herdr channel*)",
    "Bash(herdr --remote*)",
];

/// What the host offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Host {
    /// Claude Code's sandbox runs here: macOS, Linux or WSL2.
    pub sandbox_supported: bool,
    /// `herdr` is on `PATH`.
    pub herdr: bool,
}

impl Host {
    /// This machine.
    #[must_use]
    pub fn detect() -> Self {
        Self {
            sandbox_supported: sandbox_supported(),
            herdr: on_path("herdr"),
        }
    }
}

fn sandbox_supported() -> bool {
    if cfg!(target_os = "macos") {
        return true;
    }
    if cfg!(target_os = "linux") {
        // WSL1 reports "Microsoft" in the kernel release; WSL2 does not.
        return !std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .unwrap_or_default()
            .contains("Microsoft");
    }
    false
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
}

/// One change to the user file.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Change {
    /// The key path, such as `["sandbox", "enabled"]`.
    pub path: Vec<String>,
    /// Array entries added, absent before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<Value>,
    /// The value set, for a scalar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<Value>,
    /// The value before a scalar was set; `None` when the key was absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<Value>,
}

/// What was added, by item.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Record {
    /// The user file the entries were added to.
    pub file: Option<PathBuf>,
    /// Each accepted item's changes.
    pub items: Vec<RecordedItem>,
}

/// One accepted item.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecordedItem {
    /// The item's id.
    pub id: String,
    /// What it changed.
    pub changes: Vec<Change>,
}

struct Item {
    id: &'static str,
    question: &'static str,
    entries: Vec<(Vec<&'static str>, Vec<Value>)>,
    set: Option<(Vec<&'static str>, Value)>,
}

fn strings(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

/// The items offered on this host, from the shipped preset.
fn items(preset: &Value, host: Host) -> Vec<Item> {
    let sandbox = &preset["sandbox"];
    let mut out = Vec::new();
    if host.sandbox_supported {
        out.push(Item {
            id: "sandbox",
            question: "Turn Claude Code's sandbox on for every session on this machine, so Bash commands run sandboxed in projects that do not set it? Commands that need the network or files outside the project may then prompt or fail.",
            entries: Vec::new(),
            set: Some((vec!["sandbox", "enabled"], json!(true))),
        });
    }
    let home_reads: Vec<Value> = strings(&preset["permissions"]["deny"])
        .into_iter()
        .filter(|v| v.as_str().is_some_and(|s| s.starts_with("Read(~/")))
        .collect();
    out.push(Item {
        id: "credential-files",
        question: "Refuse reads of the credential files in your home directory (SSH, cloud, registry and CLI token stores) in every session, by Claude's Read tool and by sandboxed commands?",
        entries: vec![
            (vec!["permissions", "deny"], home_reads),
            (
                vec!["sandbox", "filesystem", "denyRead"],
                strings(&sandbox["filesystem"]["denyRead"]),
            ),
        ],
        set: None,
    });
    out.push(Item {
        id: "token-env",
        question: "Hide the provider and registry token variables (such as GH_TOKEN and NPM_TOKEN) from sandboxed commands in every session?",
        entries: vec![(
            vec!["sandbox", "credentials", "envVars"],
            strings(&sandbox["credentials"]["envVars"]),
        )],
        set: None,
    });
    if host.herdr {
        out.push(Item {
            id: "herdr",
            question: "Let `herdr` run outside the sandbox so a session can drive Herdr seats, with its update, machine, server, config, integration, channel and remote commands denied?",
            entries: vec![
                (vec!["sandbox", "excludedCommands"], vec![json!("herdr *")]),
                (
                    vec!["permissions", "deny"],
                    HERDR_DENIES.iter().map(|s| json!(s)).collect(),
                ),
            ],
            set: None,
        });
    }
    out
}

fn get<'a>(root: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter().try_fold(root, |v, k| v.get(k))
}

/// The object at `path`, created as needed; `None` when a non-object is
/// in the way.
fn parent_mut<'a>(root: &'a mut Value, path: &[String]) -> Option<&'a mut Map<String, Value>> {
    let (_, parents) = path.split_last()?;
    let mut cur = root;
    for key in parents {
        let map = cur.as_object_mut()?;
        cur = map.entry(key.clone()).or_insert_with(|| json!({}));
    }
    cur.as_object_mut()
}

fn owned(path: &[&str]) -> Vec<String> {
    path.iter().map(|s| (*s).to_string()).collect()
}

/// The changes an item would make to `root`: entries not already present,
/// and the scalar when it differs. Empty when the item is already in place.
fn pending(item: &Item, root: &Value) -> Vec<Change> {
    let mut out = Vec::new();
    for (path, entries) in &item.entries {
        let path = owned(path);
        let have = get(root, &path).map(strings).unwrap_or_default();
        let added: Vec<Value> = entries
            .iter()
            .filter(|e| !have.contains(e))
            .cloned()
            .collect();
        if !added.is_empty() {
            out.push(Change {
                path,
                added,
                set: None,
                previous: None,
            });
        }
    }
    if let Some((path, value)) = &item.set {
        let path = owned(path);
        let previous = get(root, &path).cloned();
        if previous.as_ref() != Some(value) {
            out.push(Change {
                path,
                added: Vec::new(),
                set: Some(value.clone()),
                previous,
            });
        }
    }
    out
}

fn apply(root: &mut Value, change: &Change) -> bool {
    let Some(key) = change.path.last().cloned() else {
        return false;
    };
    let Some(map) = parent_mut(root, &change.path) else {
        return false;
    };
    if let Some(value) = &change.set {
        map.insert(key, value.clone());
        return true;
    }
    let slot = map.entry(key).or_insert_with(|| json!([]));
    let Some(array) = slot.as_array_mut() else {
        return false;
    };
    for entry in &change.added {
        if !array.contains(entry) {
            array.push(entry.clone());
        }
    }
    true
}

fn revert(root: &mut Value, change: &Change) {
    let Some(key) = change.path.last().cloned() else {
        return;
    };
    let Some(map) = parent_mut(root, &change.path) else {
        return;
    };
    if let Some(value) = &change.set {
        // Only a value still as set is restored; a later edit is kept.
        if map.get(&key) == Some(value) {
            match &change.previous {
                Some(previous) => {
                    map.insert(key, previous.clone());
                }
                None => {
                    map.remove(&key);
                }
            }
        }
        prune(root, &change.path);
        return;
    }
    if let Some(array) = map.get_mut(&key).and_then(Value::as_array_mut) {
        array.retain(|entry| !change.added.contains(entry));
    }
    prune(root, &change.path);
}

/// Remove the empty arrays and objects left along `path`, deepest first,
/// so a removal leaves no empty key behind.
fn prune(root: &mut Value, path: &[String]) {
    for depth in (1..=path.len()).rev() {
        let (parents, key) = (&path[..depth - 1], &path[depth - 1]);
        let Some(map) = parents
            .iter()
            .try_fold(&mut *root, |v, k| v.get_mut(k))
            .and_then(Value::as_object_mut)
        else {
            return;
        };
        let empty = match map.get(key) {
            Some(Value::Array(items)) => items.is_empty(),
            Some(Value::Object(fields)) => fields.is_empty(),
            _ => false,
        };
        if !empty {
            return;
        }
        map.remove(key);
    }
}

fn describe(change: &Change) -> Vec<String> {
    let path = change.path.join(".");
    if let Some(value) = &change.set {
        return vec![format!("      {path}: {value}")];
    }
    change
        .added
        .iter()
        .map(|entry| format!("      + {path}: {entry}"))
        .collect()
}

fn read_user(path: &Path) -> Result<(Value, bool), String> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok((json!({}), true)),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(value) if value.is_object() => Ok((value, true)),
            Ok(_) => Err(format!("{} is not a JSON object", path.display())),
            Err(err) => Err(format!("{} is not valid JSON ({err})", path.display())),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok((json!({}), false)),
        Err(err) => Err(format!("cannot read {} ({err})", path.display())),
    }
}

/// The record of what was added: empty when there is none yet, an error
/// when it exists but cannot be read or parsed, so nothing it tracks is
/// lost or overwritten.
fn read_record(path: &Path) -> Result<Record, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Record::default()),
        Err(err) => return Err(format!("cannot read {} ({err})", path.display())),
    };
    serde_json::from_str(&text)
        .map_err(|err| format!("{} is not a valid record ({err})", path.display()))
}

/// Copy the user file to a new backup next to it; returns the backup path.
fn backup(user: &Path) -> std::io::Result<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let mut name = user.as_os_str().to_os_string();
    name.push(format!(".codeflow-backup-{stamp}"));
    let target = PathBuf::from(name);
    std::fs::copy(user, &target)?;
    Ok(target)
}

fn write_json(path: &Path, value: &Value) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(value).unwrap_or_default() + "\n";
    super::write_atomic(path, &text)
}

/// Offer each item not already in place, and merge the accepted ones.
/// Returns the exit code.
pub fn offer(
    input: &mut impl BufRead,
    user: &Path,
    record_path: &Path,
    preset: &Value,
    host: Host,
) -> i32 {
    let (mut root, existed) = match read_user(user) {
        Ok(found) => found,
        Err(why) => {
            println!(
                "  Your Claude Code user settings are not offered: {why}. Nothing written there."
            );
            return 0;
        }
    };
    let mut record = match read_record(record_path) {
        Ok(record) => record,
        Err(why) => {
            println!(
                "  Your Claude Code user settings are not offered: {why}. Fix or remove it first."
            );
            return 1;
        }
    };
    let mut accepted: Vec<RecordedItem> = Vec::new();
    let offered: Vec<(Item, Vec<Change>)> = items(preset, host)
        .into_iter()
        .map(|item| {
            let changes = pending(&item, &root);
            (item, changes)
        })
        .filter(|(_, changes)| !changes.is_empty())
        .collect();
    if offered.is_empty() {
        return 0;
    }
    println!(
        "Optional additions to your Claude Code user settings, {}, which apply to every session on this machine. Each is shown first and added only on your yes:",
        user.display()
    );
    for (item, changes) in offered {
        println!("  {}:", item.id);
        for change in &changes {
            for line in describe(change) {
                println!("{line}");
            }
        }
        if !super::ask(input, &format!("    {}", item.question)) {
            continue;
        }
        let applied: Vec<Change> = changes
            .into_iter()
            .filter(|change| apply(&mut root, change))
            .collect();
        accepted.push(RecordedItem {
            id: item.id.to_string(),
            changes: applied,
        });
    }
    if accepted.is_empty() {
        return 0;
    }
    if existed {
        match backup(user) {
            Ok(path) => println!("  Kept the file as it was in {}.", path.display()),
            Err(err) => {
                println!(
                    "  Cannot back up {}: {err}. Nothing written.",
                    user.display()
                );
                return 1;
            }
        }
    }
    if let Err(err) = write_json(user, &root) {
        println!("  Cannot write {}: {err}", user.display());
        return 1;
    }
    record.file = Some(user.to_path_buf());
    record.items.extend(accepted);
    let text = serde_json::to_string_pretty(&record).unwrap_or_default() + "\n";
    if let Err(err) = super::write_atomic(record_path, &text) {
        println!("  Cannot write {}: {err}", record_path.display());
        return 1;
    }
    println!(
        "  Updated {}; {} records what was added.",
        user.display(),
        record_path.display()
    );
    0
}

/// Remove exactly the recorded entries from the user file. Returns the
/// exit code.
pub fn remove(record_path: &Path) -> i32 {
    let record = match read_record(record_path) {
        Ok(record) => record,
        Err(why) => {
            println!("  Kept the additions to your user settings: {why}.");
            return 1;
        }
    };
    let Some(user) = record.file.clone() else {
        return 0;
    };
    if record.items.is_empty() {
        let _ = std::fs::remove_file(record_path);
        return 0;
    }
    let (mut root, existed) = match read_user(&user) {
        Ok(found) => found,
        Err(why) => {
            println!("  Kept the additions to your user settings: {why}.");
            return 1;
        }
    };
    if !existed {
        let _ = std::fs::remove_file(record_path);
        return 0;
    }
    for item in &record.items {
        for change in &item.changes {
            revert(&mut root, change);
        }
    }
    match backup(&user) {
        Ok(path) => println!("  Kept the file as it was in {}.", path.display()),
        Err(err) => {
            println!(
                "  Cannot back up {}: {err}. Nothing removed.",
                user.display()
            );
            return 1;
        }
    }
    if let Err(err) = write_json(&user, &root) {
        println!("  Cannot write {}: {err}", user.display());
        return 1;
    }
    let _ = std::fs::remove_file(record_path);
    println!("  Removed CodeFlow's additions from {}.", user.display());
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Host = Host {
        sandbox_supported: true,
        herdr: true,
    };

    fn preset() -> Value {
        serde_json::from_str(super::super::PRESET).unwrap()
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn only_the_items_answered_yes_are_added_and_removal_keeps_the_persons_entries() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("claude/settings.json");
        let record = dir.path().join("codeflow/claude-user-settings.json");
        std::fs::create_dir_all(user.parent().unwrap()).unwrap();
        let mine = json!({
            "model": "opus",
            "permissions": {"deny": ["Bash(rm -rf *)", "Read(~/.ssh/**)"]},
            "sandbox": {"excludedCommands": ["docker *"]}
        });
        std::fs::write(&user, serde_json::to_string(&mine).unwrap()).unwrap();

        // Items in order: sandbox, credential-files, token-env, herdr.
        assert_eq!(
            offer(
                &mut "no\nyes\nno\nyes\n".as_bytes(),
                &user,
                &record,
                &preset(),
                ALL
            ),
            0
        );
        let after = read(&user);
        assert!(
            after["sandbox"].get("enabled").is_none(),
            "sandbox declined"
        );
        assert!(
            after["sandbox"].get("credentials").is_none(),
            "tokens declined"
        );
        let deny = strings(&after["permissions"]["deny"]);
        assert!(deny.contains(&json!("Read(~/.config/gh/**)")));
        assert!(deny.contains(&json!("Bash(herdr update*)")));
        assert!(deny.contains(&json!("Bash(rm -rf *)")));
        assert_eq!(
            deny.iter()
                .filter(|d| **d == json!("Read(~/.ssh/**)"))
                .count(),
            1
        );
        assert!(!deny
            .iter()
            .any(|d| d.as_str().is_some_and(|s| s.starts_with("Read(**"))));
        assert!(strings(&after["sandbox"]["excludedCommands"]).contains(&json!("herdr *")));
        assert!(strings(&after["sandbox"]["filesystem"]["denyRead"]).contains(&json!("~/.ssh")));
        assert_eq!(after["model"], "opus");
        let backups = std::fs::read_dir(user.parent().unwrap())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("codeflow-backup")
            })
            .count();
        assert_eq!(backups, 1);

        // A second run offers only what is still missing; yes to both.
        assert_eq!(
            offer(&mut "yes\nyes\n".as_bytes(), &user, &record, &preset(), ALL),
            0
        );
        let after = read(&user);
        assert_eq!(after["sandbox"]["enabled"], true);
        assert!(after["sandbox"]["credentials"]["envVars"]
            .as_array()
            .unwrap()
            .contains(&json!({"name": "GH_TOKEN", "mode": "deny"})));

        // The person adds an entry of their own; removal keeps it and theirs.
        let mut edited = after.clone();
        edited["permissions"]["deny"]
            .as_array_mut()
            .unwrap()
            .push(json!("Bash(curl *)"));
        std::fs::write(&user, serde_json::to_string(&edited).unwrap()).unwrap();
        assert_eq!(remove(&record), 0);
        let restored = read(&user);
        assert!(restored["sandbox"].get("enabled").is_none());
        assert_eq!(
            strings(&restored["permissions"]["deny"]),
            vec![
                json!("Bash(rm -rf *)"),
                json!("Read(~/.ssh/**)"),
                json!("Bash(curl *)")
            ]
        );
        assert_eq!(
            strings(&restored["sandbox"]["excludedCommands"]),
            vec![json!("docker *")]
        );
        assert_eq!(restored["model"], "opus");
        assert!(
            restored["sandbox"].get("filesystem").is_none(),
            "no empty keys left"
        );
        assert!(
            restored["sandbox"].get("credentials").is_none(),
            "no empty keys left"
        );
        assert!(!record.exists());
    }

    #[test]
    fn nothing_is_written_on_no_and_herdr_is_offered_only_when_installed() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("settings.json");
        let record = dir.path().join("record.json");
        let host = Host {
            sandbox_supported: false,
            herdr: false,
        };
        assert_eq!(items(&preset(), host).len(), 2);
        assert_eq!(
            offer(&mut "no\nno\n".as_bytes(), &user, &record, &preset(), host),
            0
        );
        assert!(!user.exists() && !record.exists());
        assert_eq!(offer(&mut "".as_bytes(), &user, &record, &preset(), ALL), 0);
        assert!(!user.exists());
    }

    #[test]
    fn an_unreadable_user_file_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("settings.json");
        let record = dir.path().join("record.json");
        std::fs::write(&user, "{ not json").unwrap();
        assert_eq!(
            offer(
                &mut "yes\nyes\nyes\nyes\n".as_bytes(),
                &user,
                &record,
                &preset(),
                ALL
            ),
            0
        );
        assert_eq!(std::fs::read_to_string(&user).unwrap(), "{ not json");
        assert!(!record.exists());
    }

    /// A damaged record refuses both the offer and the removal, so the
    /// entries it tracks are never forgotten.
    #[test]
    fn a_damaged_record_refuses_and_keeps_the_user_file() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("settings.json");
        let record = dir.path().join("record.json");
        std::fs::write(&user, r#"{"model": "opus"}"#).unwrap();
        std::fs::write(&record, "{ not json").unwrap();
        assert_eq!(
            offer(
                &mut "yes\nyes\nyes\nyes\n".as_bytes(),
                &user,
                &record,
                &preset(),
                ALL
            ),
            1
        );
        assert_eq!(remove(&record), 1);
        assert_eq!(
            std::fs::read_to_string(&user).unwrap(),
            r#"{"model": "opus"}"#
        );
        assert_eq!(std::fs::read_to_string(&record).unwrap(), "{ not json");
    }

    #[test]
    fn a_sandbox_switch_the_person_changed_later_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("settings.json");
        let record = dir.path().join("record.json");
        assert_eq!(
            offer(
                &mut "yes\nno\nno\nno\n".as_bytes(),
                &user,
                &record,
                &preset(),
                ALL
            ),
            0
        );
        assert_eq!(read(&user)["sandbox"]["enabled"], true);
        std::fs::write(&user, r#"{"sandbox": {"enabled": false}}"#).unwrap();
        assert_eq!(remove(&record), 0);
        assert_eq!(read(&user)["sandbox"]["enabled"], false);
        // From an absent file, a full install and removal leave `{}`.
        let fresh = dir.path().join("fresh.json");
        let fresh_record = dir.path().join("fresh-record.json");
        assert_eq!(
            offer(
                &mut "yes\nyes\nyes\nyes\n".as_bytes(),
                &fresh,
                &fresh_record,
                &preset(),
                ALL
            ),
            0
        );
        assert_eq!(remove(&fresh_record), 0);
        assert_eq!(read(&fresh), json!({}));
    }
}
