//! Which text rule judges a shell call (TSK-397 design D, A2).
//!
//! The exec guard has two outcomes. **Baseline** is the line's rule. It
//! applies where the readable harness configuration shows the sandbox
//! enabled, failing closed and carrying the shipped floor. **Tightened**
//! adds the strict branch, which judges every word after a program the
//! guard cannot read through, and applies when any trigger below holds.
//! The tightened outcome keeps every baseline finding, so neither outcome
//! allows what the baseline refuses.
//!
//! Every trigger reads state a session's own writes cannot reach on the
//! common posture (the settings files are kernel write-denied there), and
//! every unreadable or malformed file tightens. An absent file contributes
//! nothing. What this module cannot read (a `--settings` file,
//! server-managed settings, MDM policy, a Codex launch flag) is a residual
//! `codeflow doctor --check sandbox` names, never a reason to relax.
//!
//! Harnesses are identified by a union rule: the guard reads the layers of
//! every harness whose shape the payload does not contradict. Only a key
//! unique to one harness excludes the others; shared keys never identify.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::security::actions;

/// The harnesses whose settings a call is judged against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Harnesses {
    pub claude: bool,
    pub codex: bool,
    pub grok: bool,
}

impl Harnesses {
    /// Every harness: the reading for a payload that names none.
    pub const ALL: Self = Self {
        claude: true,
        codex: true,
        grok: true,
    };

    /// The harnesses a payload's top-level keys do not contradict. Grok
    /// alone sends the camelCase `toolName`; Codex sends `turn_id` and
    /// `model` together; Claude Code sends `prompt_id` and `scratchpad_dir`
    /// together (TSK-397 canary, Claude Code 2.1.312). A payload carrying
    /// the unique keys of more than one harness contradicts itself and is
    /// read against all of them.
    #[must_use]
    pub fn from_keys(keys: &[String]) -> Self {
        let has = |key: &str| keys.iter().any(|k| k == key);
        let grok = has("toolName");
        let codex = has("turn_id") && has("model");
        let claude = has("prompt_id") && has("scratchpad_dir");
        if u8::from(grok) + u8::from(codex) + u8::from(claude) != 1 {
            return Self::ALL;
        }
        Self {
            claude: !(grok || codex),
            codex: !(grok || claude),
            grok: !(codex || claude),
        }
    }
}

/// Where the guard reads harness configuration on this host.
#[derive(Debug, Clone)]
pub struct Host {
    /// The project root.
    pub root: PathBuf,
    /// For a linked worktree, the main checkout's root, or why it could not
    /// be found; `Ok(None)` for a main checkout.
    pub main_root: Result<Option<PathBuf>, String>,
    /// The user's home directory.
    pub home: Option<PathBuf>,
    /// `CLAUDE_CONFIG_DIR` when set: Claude Code's user settings directory.
    pub claude_config_dir: Option<PathBuf>,
    /// `CODEX_HOME` when set.
    pub codex_home: Option<PathBuf>,
    /// Claude Code's managed settings file and its drop-in directory.
    pub managed_file: Option<PathBuf>,
    pub managed_dir: Option<PathBuf>,
    /// Native Windows, where Claude Code and Grok Build have no sandbox.
    pub native_windows: bool,
}

/// The debug-build variable that moves the managed settings location, so a
/// test is not judged by the host's own managed policy. A release build
/// never reads it.
pub const TEST_MANAGED_DIR: &str = "CODEFLOW_TEST_CLAUDE_MANAGED_DIR";

impl Host {
    /// The host the running process sees for the project at `root`.
    #[must_use]
    pub fn from_process(root: &Path) -> Self {
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        let managed_base = managed_base();
        #[cfg(debug_assertions)]
        let managed_base = var(TEST_MANAGED_DIR).or(managed_base);
        Self {
            root: root.to_path_buf(),
            main_root: main_root(root),
            home: crate::portable_path::user_home(),
            claude_config_dir: var("CLAUDE_CONFIG_DIR"),
            codex_home: var("CODEX_HOME"),
            managed_file: managed_base
                .as_ref()
                .map(|base| base.join("managed-settings.json")),
            managed_dir: managed_base.map(|base| base.join("managed-settings.d")),
            native_windows: cfg!(windows),
        }
    }
}

/// Claude Code's managed settings directory for this platform.
fn managed_base() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some(PathBuf::from("/Library/Application Support/ClaudeCode"))
    } else if cfg!(windows) {
        Some(PathBuf::from(r"C:\Program Files\ClaudeCode"))
    } else if cfg!(unix) {
        Some(PathBuf::from("/etc/claude-code"))
    } else {
        None
    }
}

/// The main checkout of a linked worktree at `root`.
fn main_root(root: &Path) -> Result<Option<PathBuf>, String> {
    let repo = git2::Repository::open(root).map_err(|e| e.message().to_string())?;
    if !repo.is_worktree() {
        return Ok(None);
    }
    repo.commondir()
        .parent()
        .map(|main| Some(main.to_path_buf()))
        .ok_or_else(|| "the common git directory has no parent".to_string())
}

/// Which rule judges a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The line's rule.
    Baseline,
    /// The baseline and the strict branch, with the first trigger that
    /// selected it.
    Tightened(String),
}

impl Outcome {
    /// The trigger text the strict branch's findings name, if tightened.
    #[must_use]
    pub fn trigger(&self) -> Option<&str> {
        match self {
            Self::Baseline => None,
            Self::Tightened(trigger) => Some(trigger),
        }
    }
}

/// The outcome for one call: tightened when the policy asks for the strict
/// rule, when the call asks to run outside the sandbox, or on any
/// configuration trigger.
#[must_use]
pub fn outcome(host: &Host, harnesses: Harnesses, retry: bool, policy_strict: bool) -> Outcome {
    if policy_strict {
        return Outcome::Tightened(
            "the project policy sets `security.text_guard` to `strict`".to_string(),
        );
    }
    if retry {
        return Outcome::Tightened(
            "the call asks to run outside the sandbox (`dangerouslyDisableSandbox`)".to_string(),
        );
    }
    triggers(host, harnesses)
        .into_iter()
        .next()
        .map_or(Outcome::Baseline, Outcome::Tightened)
}

/// Every configuration trigger that holds on this host, in a fixed order.
#[must_use]
pub fn triggers(host: &Host, harnesses: Harnesses) -> Vec<String> {
    let mut found = Vec::new();
    if host.native_windows && (harnesses.claude || harnesses.grok) {
        found.push("native Windows has no sandbox under Claude Code or Grok Build".to_string());
    }
    if harnesses.claude {
        claude_triggers(host, &mut found);
    }
    if harnesses.codex {
        codex_triggers(host, &mut found);
    }
    if harnesses.grok {
        grok_triggers(host, &mut found);
    }
    // One entry per trigger, though several layers may carry it.
    let mut seen = std::collections::HashSet::new();
    found.retain(|trigger| seen.insert(trigger.clone()));
    found
}

/// A settings file as read.
enum Read<T> {
    Absent,
    Unreadable(String),
    Parsed(T),
}

/// The most bytes read from one settings file.
const READ_LIMIT: u64 = 16 * 1024 * 1024;

fn read_text(path: &Path) -> Read<String> {
    match crate::absence::metadata_optional(path) {
        Ok(None) => return Read::Absent,
        Err(error) => return Read::Unreadable(error.to_string()),
        Ok(Some(meta)) if meta.len() > READ_LIMIT => {
            return Read::Unreadable(format!("larger than {READ_LIMIT} bytes"));
        }
        Ok(Some(_)) => {}
    }
    match std::fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Read::Parsed(text),
            Err(_) => Read::Unreadable("not UTF-8".to_string()),
        },
        Err(error) => Read::Unreadable(error.to_string()),
    }
}

/// Where a Claude settings layer sits in the documented precedence, lowest
/// first. Two layers at the same rank that disagree make a value
/// uncomputable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    User,
    Project,
    Local,
    Managed,
}

struct Layer {
    rank: Rank,
    name: String,
    value: Value,
}

/// The Claude settings layers, read; each unreadable or malformed one is a
/// trigger.
fn claude_layers(host: &Host, found: &mut Vec<String>) -> Vec<Layer> {
    let mut paths: Vec<(Rank, PathBuf)> = Vec::new();
    let user_dir = host
        .claude_config_dir
        .clone()
        .or_else(|| host.home.as_ref().map(|home| home.join(".claude")));
    if let Some(dir) = user_dir {
        paths.push((Rank::User, dir.join("settings.json")));
    }
    paths.push((Rank::Project, host.root.join(".claude/settings.json")));
    paths.push((Rank::Local, host.root.join(".claude/settings.local.json")));
    match &host.main_root {
        Ok(Some(main)) => paths.push((Rank::Local, main.join(".claude/settings.local.json"))),
        Ok(None) => {}
        Err(why) => found.push(format!(
            "the main checkout of this worktree cannot be found, so its Claude local settings cannot be read ({why})"
        )),
    }
    if let Some(file) = &host.managed_file {
        paths.push((Rank::Managed, file.clone()));
    }
    if let Some(dir) = &host.managed_dir {
        match std::fs::read_dir(dir) {
            Ok(entries) => {
                let mut files: Vec<PathBuf> = Vec::new();
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            let path = entry.path();
                            if path.extension().is_some_and(|ext| ext == "json") {
                                files.push(path);
                            }
                        }
                        Err(error) => found.push(format!(
                            "the managed settings directory `{}` cannot be listed ({error})",
                            dir.display()
                        )),
                    }
                }
                files.sort();
                paths.extend(files.into_iter().map(|file| (Rank::Managed, file)));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => found.push(format!(
                "the managed settings directory `{}` cannot be listed ({error})",
                dir.display()
            )),
        }
    }
    let mut layers = Vec::new();
    for (rank, path) in paths {
        let name = format!("`{}`", path.display());
        match read_text(&path) {
            Read::Absent => {}
            Read::Unreadable(why) => {
                found.push(format!("the Claude settings {name} cannot be read ({why})"));
            }
            Read::Parsed(text) => match serde_json::from_str::<Value>(&text) {
                Ok(value) if value.is_object() => layers.push(Layer { rank, name, value }),
                Ok(_) | Err(_) => {
                    found.push(format!("the Claude settings {name} are not a JSON object"));
                }
            },
        }
    }
    layers
}

/// The effective value of `pointer` by precedence: the highest-ranked
/// layer that sets it. `Err` when two layers of that rank disagree.
fn effective<'a>(layers: &'a [Layer], pointer: &str) -> Result<Option<&'a Value>, ()> {
    let mut best: Option<(Rank, &Value)> = None;
    for layer in layers {
        let Some(value) = layer.value.pointer(pointer) else {
            continue;
        };
        match best {
            Some((rank, _)) if rank > layer.rank => {}
            Some((rank, current)) if rank == layer.rank && current != value => return Err(()),
            _ => best = Some((layer.rank, value)),
        }
    }
    Ok(best.map(|(_, value)| value))
}

/// Every string of the array at `pointer` across the layers; a value that
/// is not an array of strings is named in `found`.
fn merged(layers: &[Layer], pointer: &str, found: &mut Vec<String>) -> Vec<String> {
    let mut items = Vec::new();
    for layer in layers {
        match layer.value.pointer(pointer) {
            None => {}
            Some(Value::Array(values)) => {
                for value in values {
                    match value.as_str() {
                        Some(text) => items.push(text.to_string()),
                        None => found.push(format!(
                            "`{pointer}` in {} holds a value that is not a string",
                            layer.name
                        )),
                    }
                }
            }
            Some(_) => found.push(format!("`{pointer}` in {} is not an array", layer.name)),
        }
    }
    items
}

fn claude_triggers(host: &Host, found: &mut Vec<String>) {
    let layers = claude_layers(host, found);
    for (pointer, key) in [
        ("/sandbox/enabled", "sandbox.enabled"),
        ("/sandbox/failIfUnavailable", "sandbox.failIfUnavailable"),
    ] {
        match effective(&layers, pointer) {
            Ok(Some(Value::Bool(true))) => {}
            Ok(_) => found.push(format!("the Claude settings do not set `{key}` to true")),
            Err(()) => found.push(format!(
                "the Claude settings layers disagree on `{key}`, so its value cannot be computed"
            )),
        }
    }
    for layer in &layers {
        if let Some(value) = layer.value.pointer("/sandbox/filesystem/disabled") {
            if value != &Value::Bool(false) {
                found.push(format!("{} sets `sandbox.filesystem.disabled`", layer.name));
            }
        }
        escape_settings(layer, found);
    }
    let excluded = merged(&layers, "/sandbox/excludedCommands", found);
    if !excluded.is_empty() {
        found.push(format!(
            "the Claude settings list `sandbox.excludedCommands` ({}), which run outside the sandbox",
            excluded.join(", ")
        ));
    }
    let deny = merged(&layers, "/permissions/deny", found);
    let deny_write = merged(&layers, "/sandbox/filesystem/denyWrite", found);
    let deny_read = merged(&layers, "/sandbox/filesystem/denyRead", found);
    for missing in missing_floor(&deny, &deny_write, &deny_read) {
        found.push(format!(
            "the Claude settings are missing the floor entry {missing}; `codeflow update` restores it"
        ));
    }
    let floor = floor_paths(host);
    let allow_write = merged(&layers, "/sandbox/filesystem/allowWrite", found);
    let allow = merged(&layers, "/permissions/allow", found);
    let directories = merged(&layers, "/permissions/additionalDirectories", found);
    let edit_allows = allow.iter().filter_map(|rule| {
        let rule = rule.trim();
        if rule == "Edit" || rule == "Write" {
            return Some("**".to_string());
        }
        rule.strip_prefix("Edit(")
            .or_else(|| rule.strip_prefix("Write("))
            .and_then(|inner| inner.strip_suffix(')'))
            .map(str::to_string)
    });
    for (kind, entry) in allow_write
        .iter()
        .map(|e| ("`sandbox.filesystem.allowWrite`", e.clone()))
        .chain(edit_allows.map(|e| ("an `Edit` allow rule", e)))
        .chain(
            directories
                .iter()
                .map(|e| ("`permissions.additionalDirectories`", e.clone())),
        )
    {
        if reaches_floor(&entry, host, &floor) {
            found.push(format!(
                "{kind} entry `{entry}` reaches a protected path the sandbox floor denies"
            ));
        }
    }
}

/// The settings that weaken filesystem containment, anywhere under
/// `sandbox` in a layer.
fn escape_settings(layer: &Layer, found: &mut Vec<String>) {
    fn walk(value: &Value, path: &str, out: &mut Vec<String>) {
        let Value::Object(map) = value else {
            return;
        };
        for (key, child) in map {
            let here = format!("{path}.{key}");
            let set = match key.as_str() {
                "enableWeakerNestedSandbox"
                | "allowNestedUserNamespaces"
                | "allowAllUnixSockets" => child != &Value::Bool(false),
                "allowUnixSockets" => !matches!(child, Value::Array(items) if items.is_empty()),
                "ignoreViolations" => !child.is_null(),
                _ => false,
            };
            if set {
                out.push(here.clone());
            }
            walk(child, &here, out);
        }
    }
    let mut keys = Vec::new();
    if let Some(sandbox) = layer.value.get("sandbox") {
        walk(sandbox, "sandbox", &mut keys);
    }
    for key in keys {
        found.push(format!(
            "{} sets `{key}`, which weakens the sandbox",
            layer.name
        ));
    }
}

/// The floor entries a merged configuration lacks: each enforcement path
/// needs its `Edit(...)` deny or its `denyWrite` entry, and each secret
/// class its `Read(...)` deny or its `denyRead` entry.
#[must_use]
pub fn missing_floor(deny: &[String], deny_write: &[String], deny_read: &[String]) -> Vec<String> {
    let table = actions::table();
    let mut missing = Vec::new();
    let edits = table
        .claude_edit_denies
        .iter()
        .cloned()
        .chain(table.startup_paths.claude_edit_denies())
        .chain(table.git_config_paths.claude_edit_denies());
    for rule in edits {
        let written = actions::edit_deny_write_path(&rule)
            .is_some_and(|path| deny_write.iter().any(|entry| entry == &path));
        if !written && !deny.contains(&rule) {
            missing.push(format!("`{rule}`"));
        }
    }
    for rule in table.claude_read_denies() {
        let path = rule
            .strip_prefix("Read(")
            .and_then(|inner| inner.strip_suffix(')'));
        let read = path.is_some_and(|path| deny_read.iter().any(|entry| entry == path));
        if !read && !deny.contains(&rule) {
            missing.push(format!("`{rule}`"));
        }
    }
    for path in &table.sandbox_read_denies {
        let rule = format!("Read({path})");
        if !deny_read.contains(path) && !deny.contains(&rule) {
            missing.push(format!("`{path}` in `sandbox.filesystem.denyRead`"));
        }
    }
    missing
}

/// The protected paths as absolute paths on this host: the enforcement
/// paths of the action table.
fn floor_paths(host: &Host) -> Vec<PathBuf> {
    actions::table()
        .claude_edit_denies
        .iter()
        .filter_map(|rule| actions::edit_deny_write_path(rule))
        .filter_map(|path| absolute(&path, host, &host.root))
        .collect()
}

/// A settings path read as absolute: `~/` under the home, `//` and `/`
/// from the file system root, anything else under `base`.
fn absolute(entry: &str, host: &Host, base: &Path) -> Option<PathBuf> {
    if let Some(rest) = entry.strip_prefix("~/") {
        return host.home.as_ref().map(|home| home.join(rest));
    }
    if entry == "~" {
        return host.home.clone();
    }
    if let Some(rest) = entry.strip_prefix("//") {
        return Some(PathBuf::from("/").join(rest));
    }
    if entry.starts_with('/') {
        return Some(PathBuf::from(entry));
    }
    Some(base.join(entry.trim_start_matches("./")))
}

/// Whether an allow entry is at or under a protected path, or is a pattern
/// whose literal start could reach one. Every reading the settings
/// grammars give a path is tried: absolute, under the project root, and
/// under the home.
fn reaches_floor(entry: &str, host: &Host, floor: &[PathBuf]) -> bool {
    let glob_at = entry.find(['*', '?', '[', '{']);
    let literal = glob_at.map_or(entry, |at| &entry[..at]);
    let readings: Vec<PathBuf> = [
        absolute(literal, host, &host.root),
        host.home
            .as_ref()
            .map(|home| home.join(literal.trim_start_matches('/'))),
        Some(host.root.join(literal.trim_start_matches('/'))),
    ]
    .into_iter()
    .flatten()
    .collect();
    readings.iter().any(|reading| {
        let reading = crate::security::outward::lexical_join(Path::new("/"), reading);
        floor.iter().any(|protected| {
            reading.starts_with(protected)
                || (glob_at.is_some() && protected.starts_with(&reading))
                || (glob_at.is_some() && literal_prefix_overlaps(&reading, protected))
        })
    })
}

/// Whether a pattern's literal start, cut inside a path component, can
/// still begin a protected path (`.cl*` reaches `.claude`).
fn literal_prefix_overlaps(reading: &Path, protected: &Path) -> bool {
    let reading = reading.to_string_lossy();
    let protected = protected.to_string_lossy();
    protected.starts_with(reading.as_ref())
}

fn codex_triggers(host: &Host, found: &mut Vec<String>) {
    let user_dir = host
        .codex_home
        .clone()
        .or_else(|| host.home.as_ref().map(|home| home.join(".codex")));
    let mut tables = Vec::new();
    for path in user_dir
        .map(|dir| dir.join("config.toml"))
        .into_iter()
        .chain([host.root.join(".codex/config.toml")])
    {
        let name = format!("`{}`", path.display());
        match read_text(&path) {
            Read::Absent => {}
            Read::Unreadable(why) => {
                found.push(format!(
                    "the Codex configuration {name} cannot be read ({why})"
                ));
            }
            Read::Parsed(text) => match text.parse::<toml::Table>() {
                Ok(table) => tables.push((name, table)),
                Err(_) => found.push(format!("the Codex configuration {name} is not valid TOML")),
            },
        }
    }
    if tables.is_empty() {
        return;
    }
    for (name, table) in &tables {
        if has_key(table, "sandbox_mode") {
            found.push(format!(
                "the Codex configuration {name} sets `sandbox_mode`, which a permission profile does not compose with"
            ));
        }
    }
    // The project layer overrides the user layer.
    let value = |key: &str| {
        tables
            .iter()
            .rev()
            .find_map(|(_, table)| table.get(key))
            .and_then(toml::Value::as_str)
    };
    match value("default_permissions") {
        Some("cf-guard" | "cf-builder") => {}
        other => found.push(format!(
            "the Codex default permission profile is {} rather than `cf-guard` or `cf-builder`",
            other.map_or_else(|| "unset".to_string(), |v| format!("`{v}`"))
        )),
    }
    match value("approval_policy") {
        Some("never") => {}
        other => found.push(format!(
            "the Codex approval policy is {} rather than `never`, so an escalation can be approved",
            other.map_or_else(|| "unset".to_string(), |v| format!("`{v}`"))
        )),
    }
}

/// Whether `key` appears in the table at any depth.
fn has_key(table: &toml::Table, key: &str) -> bool {
    table.iter().any(|(name, value)| {
        name == key || value.as_table().is_some_and(|inner| has_key(inner, key))
    })
}

/// The `read_only` entries of each shipped Grok profile.
fn shipped_grok_profiles() -> Vec<(String, Vec<String>)> {
    let shipped: toml::Table = include_str!("../../../../assets/base/grok/sandbox.toml")
        .parse()
        .unwrap_or_default();
    grok_profiles(&shipped)
}

fn grok_profiles(table: &toml::Table) -> Vec<(String, Vec<String>)> {
    table
        .get("profiles")
        .and_then(toml::Value::as_table)
        .map(|profiles| {
            profiles
                .iter()
                .map(|(name, profile)| {
                    let entries = profile
                        .get("read_only")
                        .and_then(toml::Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(|item| item.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default();
                    (name.clone(), entries)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn grok_triggers(host: &Host, found: &mut Vec<String>) {
    let path = host.root.join(".grok/sandbox.toml");
    let name = format!("`{}`", path.display());
    let table = match read_text(&path) {
        Read::Absent => return,
        Read::Unreadable(why) => {
            found.push(format!(
                "the Grok sandbox profiles {name} cannot be read ({why})"
            ));
            return;
        }
        Read::Parsed(text) => {
            let Ok(table) = text.parse::<toml::Table>() else {
                found.push(format!(
                    "the Grok sandbox profiles {name} are not valid TOML"
                ));
                return;
            };
            table
        }
    };
    let present = grok_profiles(&table);
    for (profile, entries) in shipped_grok_profiles() {
        let Some((_, have)) = present.iter().find(|(name, _)| name == &profile) else {
            found.push(format!(
                "the Grok sandbox profiles {name} lack the shipped profile `{profile}`"
            ));
            continue;
        };
        for entry in entries {
            if !have.contains(&entry) {
                found.push(format!(
                    "the Grok profile `{profile}` in {name} lacks the shipped `read_only` entry `{entry}`"
                ));
            }
        }
    }
}

#[cfg(test)]
#[path = "sandbox_state_tests.rs"]
mod tests;
