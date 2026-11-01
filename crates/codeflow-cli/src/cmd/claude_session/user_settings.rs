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

/// Where backups of the user file go: `~/.codeflow/claude/backups`, beside
/// the install record, whatever folder the user file is in. The shipped
/// presets deny that folder to the Read tool and the sandbox; a folder
/// beside the user file would move with `CLAUDE_CONFIG_DIR` out of their
/// reach (security review round 2, SEC-08).
pub(super) fn backup_dir(record_path: &Path) -> PathBuf {
    record_path.with_file_name("claude").join("backups")
}

/// Copy the user file to a new backup in [`backup_dir`], readable by its
/// owner only. Each backup gets a new name, so a second backup in the same
/// second never replaces the first. Returns the backup path.
fn backup(user: &Path, record_path: &Path) -> std::io::Result<PathBuf> {
    use std::io::Write;
    let bytes = std::fs::read(user)?;
    let dir = backup_dir(record_path);
    std::fs::create_dir_all(&dir)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    for n in 0..1000 {
        // The name is joined as bytes, never decoded.
        let mut file_name = user.file_name().unwrap_or_default().to_os_string();
        file_name.push(format!(".codeflow-backup-{stamp}-{n}"));
        let target = dir.join(file_name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&target) {
            Ok(mut file) => {
                file.write_all(&bytes)?;
                file.sync_all()?;
                return Ok(target);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::other("no free backup name"))
}

fn write_json(path: &Path, value: &Value) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(value).unwrap_or_default() + "\n";
    super::write_atomic(path, &text)
}

/// Refuse a change of the user file while the record still holds
/// additions made to another one (security review round 3,
/// SEC-07-R3-TARGET). The record names one file, and removal reverts every
/// recorded change in it; moving it to a new file would revert there what
/// the installer never set, and forget what it set in the old one. `Err` is the
/// message, which says how to finish the earlier installation first.
pub fn check_target(user: &Path, record_path: &Path) -> Result<(), String> {
    let record = read_record(record_path)?;
    let Some(recorded) = record.file.as_deref() else {
        return Ok(());
    };
    let same = recorded == user
        || matches!(
            (std::fs::canonicalize(recorded), std::fs::canonicalize(user)),
            (Ok(a), Ok(b)) if a == b
        );
    if record.items.is_empty() || same {
        return Ok(());
    }
    Err(format!(
        "CodeFlow's earlier additions are recorded for {}, and this installation would change {}. \
         Finish the earlier installation first: run `codeflow claude-session --uninstall`, which removes them from {}, then install again",
        recorded.display(),
        user.display(),
        recorded.display()
    ))
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
    if let Err(why) = check_target(user, record_path) {
        println!("  Your Claude Code user settings are not offered: {why}.");
        return 1;
    }
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
        match backup(user, record_path) {
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
    // The record goes first: if the user file then cannot be written, the
    // record names entries that are not there, which removal skips; the
    // other order could leave additions nothing records.
    record.file = Some(user.to_path_buf());
    record.items.extend(accepted);
    let text = serde_json::to_string_pretty(&record).unwrap_or_default() + "\n";
    if let Err(err) = super::write_atomic(record_path, &text) {
        println!(
            "  Cannot write {}: {err}. Nothing written to {}.",
            record_path.display(),
            user.display()
        );
        return 1;
    }
    if let Err(err) = write_json(user, &root) {
        println!("  Cannot write {}: {err}", user.display());
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
    match backup(&user, record_path) {
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
        let backups = std::fs::read_dir(backup_dir(&record))
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

    /// Security review SEC-08: backups go to the folder the presets deny to
    /// reads, owner-only, and two in the same second keep both earlier
    /// versions.
    #[cfg(unix)]
    #[test]
    fn backups_are_private_and_never_replace_each_other() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("settings.json");
        std::fs::write(&user, "first").unwrap();
        let record = dir.path().join("record.json");
        let one = backup(&user, &record).unwrap();
        std::fs::write(&user, "second").unwrap();
        let two = backup(&user, &record).unwrap();
        assert_ne!(one, two);
        assert_eq!(std::fs::read_to_string(&one).unwrap(), "first");
        assert_eq!(std::fs::read_to_string(&two).unwrap(), "second");
        for path in [&one, &two] {
            assert_eq!(path.parent().unwrap(), dir.path().join("claude/backups"));
            let mode = std::fs::metadata(path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "{}", path.display());
        }
    }

    /// Security review round 2 (SEC-08): with the user file in a folder
    /// `CLAUDE_CONFIG_DIR` names, the backup still goes beside the install
    /// record, where the presets deny reads, and never beside the user file.
    #[test]
    fn a_custom_config_folder_does_not_move_the_backups() {
        let dir = tempfile::tempdir().unwrap();
        let custom = dir.path().join("custom-claude");
        std::fs::create_dir_all(&custom).unwrap();
        let user = custom.join("settings.json");
        std::fs::write(&user, r#"{"env": {"PRIVATE": "synthetic"}}"#).unwrap();
        let record = dir.path().join("home/.codeflow/claude-user-settings.json");
        assert_eq!(
            offer(&mut "yes\n".as_bytes(), &user, &record, &preset(), ALL),
            0
        );
        assert!(
            !custom.join("backups").exists(),
            "no backup beside the user file"
        );
        let fixed = dir.path().join("home/.codeflow/claude/backups");
        assert_eq!(std::fs::read_dir(&fixed).unwrap().count(), 1);
    }

    /// The record is written before the user file, so a user file that
    /// cannot be written leaves no unrecorded addition.
    #[cfg(unix)]
    #[test]
    fn the_record_is_written_before_the_user_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("record.json");
        let preset = preset();
        // An absent user file under a read-only folder: the record is
        // written first and the user file write then fails.
        let locked = dir.path().join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        let user = locked.join("settings.json");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
        let code = offer(
            &mut "yes\nno\nno\nno\n".as_bytes(),
            &user,
            &record,
            &preset,
            ALL,
        );
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(code, 1);
        assert!(record.exists(), "the record precedes the user write");
        assert!(!user.exists());
        assert_eq!(remove(&record), 0);
    }

    /// Security review round 3 (SEC-07-R3-TARGET): install against A,
    /// select B (another `CLAUDE_CONFIG_DIR`), accept another item there,
    /// uninstall. The change of target is refused before any backup or
    /// write, so B keeps its own values and the record still names A.
    #[test]
    fn a_new_user_file_is_refused_while_the_record_names_another() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a/settings.json");
        let b = dir.path().join("b/settings.json");
        let record = dir.path().join("home/.codeflow/claude-user-settings.json");
        for path in [&a, &b] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        }
        std::fs::write(&a, r#"{"sandbox": {"enabled": false}}"#).unwrap();
        let b_text = r#"{"sandbox": {"enabled": true}, "model": "opus"}"#;
        std::fs::write(&b, b_text).unwrap();
        // Yes to the sandbox switch in A.
        assert_eq!(
            offer(
                &mut "yes\nno\nno\nno\n".as_bytes(),
                &a,
                &record,
                &preset(),
                ALL
            ),
            0
        );
        assert_eq!(read(&a)["sandbox"]["enabled"], true);
        let record_text = std::fs::read_to_string(&record).unwrap();
        let backups = std::fs::read_dir(backup_dir(&record)).unwrap().count();

        let why = check_target(&b, &record).unwrap_err();
        assert!(why.contains("--uninstall"), "{why}");
        assert!(why.contains(&a.display().to_string()), "{why}");
        // Yes to another item in B: refused, nothing written or backed up.
        assert_eq!(
            offer(
                &mut "yes\nyes\nyes\n".as_bytes(),
                &b,
                &record,
                &preset(),
                ALL
            ),
            1
        );
        assert_eq!(std::fs::read_to_string(&b).unwrap(), b_text);
        assert_eq!(std::fs::read_to_string(&record).unwrap(), record_text);
        assert_eq!(
            std::fs::read_dir(backup_dir(&record)).unwrap().count(),
            backups
        );

        // Uninstall reverts A only; B keeps its pre-existing values.
        assert_eq!(remove(&record), 0);
        assert_eq!(read(&a)["sandbox"]["enabled"], false);
        assert_eq!(std::fs::read_to_string(&b).unwrap(), b_text);
        // With the old installation finished, B can be installed.
        assert!(check_target(&b, &record).is_ok());
        assert!(check_target(&a, &record).is_ok());
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
