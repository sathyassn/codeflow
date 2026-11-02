//! Limit readings of each family, in one structure (TSK-431).
//!
//! A reading is a percentage used and a reset time per window, kept in a
//! small file per family under the project's untracked local folder, one entry
//! per config home that produced readings. Nothing here asks a vendor for
//! anything: the readings come from what each CLI already hands the host.
//!
//! | Family | Where the reading comes from | Shape |
//! |---|---|---|
//! | Claude Code | the statusline JSON, `rate_limits.five_hour` and `seven_day` | `used_percentage`, `resets_at` (epoch seconds) |
//! | Codex | the app-server's `account/rateLimits/read` result or `account/rateLimits/updated` event | `rateLimits.primary` and `secondary`: `usedPercent`, `windowDurationMins`, `resetsAt` |
//! | Grok Build | no usage signal; the limit-hit error | a time, and a reset time when the error gives one |
//!
//! Claude's statusline names are from the Claude Code documentation. Codex's
//! names are from the installed CLI's own generated schema
//! (`codex app-server generate-json-schema`), not from vendor documentation;
//! the qualification record says which release was read.
//!
//! A reading belongs to the config home of the process that recorded it, never
//! to whatever the profile names later: a profile change makes the old reading
//! show as belonging to another home. Every read and write goes through the
//! confined local folder (no link at any component), and the read-merge-write
//! of one family is serialized by a lock file.

use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{confined_read, Family, LOCAL_DIR};

const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024;
const FIVE_HOUR_MAX_MINS: i64 = 6 * 60;
const WEEKLY_MIN_MINS: i64 = 6 * 24 * 60;
const MAX_WINDOW_MINS: i64 = 366 * 24 * 60;
const MAX_HOMES: usize = 8;
const LOCK_WAIT: Duration = Duration::from_secs(10);

/// The latest time any input may carry: 9999-12-31T23:59:59Z, the last second
/// the RFC 3339 form shown to the operator can print. One range for payload
/// times, `--resets-at`, the clock and stored files.
pub const MAX_EPOCH: u64 = 253_402_300_799;

/// Folder, under the local folder, that holds one reading file per family.
pub const LIMITS_DIR: &str = "limits";

/// One usage window.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    /// Share of the window used, 0 to 100.
    pub used_percent: f64,
    /// Unix time the window resets, when the CLI says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<u64>,
}

impl Window {
    fn validate(&self, what: &str) -> Result<(), String> {
        if !(self.used_percent.is_finite() && (0.0..=100.0).contains(&self.used_percent)) {
            return Err(format!(
                "{what}.used_percent must be a number from 0 to 100"
            ));
        }
        if let Some(reset) = self.resets_at {
            valid_time(reset, &format!("{what}.resets_at"))?;
        }
        Ok(())
    }
}

/// What a payload carried.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    /// The 5-hour window.
    pub five_hour: Option<Window>,
    /// The weekly window.
    pub weekly: Option<Window>,
    /// A limit-hit error was reported (Grok).
    pub limit_hit: bool,
    /// Reset time given with a limit-hit error.
    pub limit_hit_resets_at: Option<u64>,
}

impl Reading {
    /// Whether the payload carried nothing to record.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.five_hour.is_none() && self.weekly.is_none() && !self.limit_hit
    }

    fn validate(&self) -> Result<(), String> {
        if let Some(window) = &self.five_hour {
            window.validate("five_hour")?;
        }
        if let Some(window) = &self.weekly {
            window.validate("weekly")?;
        }
        if let Some(reset) = self.limit_hit_resets_at {
            valid_time(reset, "limit_hit_resets_at")?;
        }
        Ok(())
    }
}

/// What one config home has reported.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HomeReading {
    /// The config home of the recording process (a folder path, never a
    /// credential), or `None` for the CLI's default home.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_home: Option<String>,
    /// Where the reading came from.
    pub source: String,
    /// Unix time the entry was last written.
    pub recorded_at: u64,
    /// The 5-hour window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<Window>,
    /// The weekly window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekly: Option<Window>,
    /// Unix time a limit-hit error was reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_hit_at: Option<u64>,
    /// Reset time that error gave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_hit_resets_at: Option<u64>,
}

/// A stored file: the readings of one family, per producing config home.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    /// Always 2.
    pub schema_version: u32,
    /// Family id.
    pub family: String,
    /// One entry per config home that reported, newest data last written.
    pub homes: Vec<HomeReading>,
}

fn valid_time(time: u64, what: &str) -> Result<(), String> {
    if time > MAX_EPOCH {
        return Err(format!(
            "{what} must be a Unix time from 0 to {MAX_EPOCH} (year 9999)"
        ));
    }
    Ok(())
}

fn valid_home(home: &str) -> Result<(), String> {
    if home.is_empty()
        || home.len() > 1024
        || home.chars().any(char::is_control)
        || !Path::new(home).is_absolute()
    {
        return Err(
            "a config home must be an absolute path of at most 1024 bytes with no control \
             characters"
                .into(),
        );
    }
    Ok(())
}

fn valid_source(source: &str) -> Result<(), String> {
    if source.is_empty() || source.len() > 128 || source.chars().any(char::is_control) {
        return Err("a source label must be 1 to 128 bytes with no control characters".into());
    }
    Ok(())
}

fn same_home(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Path::new(a) == Path::new(b),
        _ => false,
    }
}

impl HomeReading {
    fn validate(&self) -> Result<(), String> {
        if let Some(home) = &self.config_home {
            valid_home(home)?;
        }
        valid_source(&self.source)?;
        valid_time(self.recorded_at, "recorded_at")?;
        if let Some(window) = &self.five_hour {
            window.validate("five_hour")?;
        }
        if let Some(window) = &self.weekly {
            window.validate("weekly")?;
        }
        for (time, what) in [
            (self.limit_hit_at, "limit_hit_at"),
            (self.limit_hit_resets_at, "limit_hit_resets_at"),
        ] {
            if let Some(time) = time {
                valid_time(time, what)?;
            }
        }
        Ok(())
    }
}

impl Snapshot {
    fn validate(&self, family: Family) -> Result<(), String> {
        if self.schema_version != 2 || self.family != family.id() {
            return Err(format!("not a version 2 snapshot of {}", family.id()));
        }
        if self.homes.len() > MAX_HOMES {
            return Err(format!("more than {MAX_HOMES} homes"));
        }
        for (position, home) in self.homes.iter().enumerate() {
            home.validate()?;
            let repeated = self.homes[..position]
                .iter()
                .any(|other| same_home(other.config_home.as_deref(), home.config_home.as_deref()));
            if repeated {
                return Err("a config home is listed twice".into());
            }
        }
        Ok(())
    }
}

/// Seconds since the Unix epoch.
#[must_use]
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn parse_json(text: &str) -> Result<Value, String> {
    if text.len() > MAX_PAYLOAD_BYTES {
        return Err("payload is larger than 256 KiB".into());
    }
    let json: Value =
        serde_json::from_str(text).map_err(|error| format!("payload is not JSON: {error}"))?;
    if !json.is_object() {
        return Err("payload must be a JSON object".into());
    }
    Ok(json)
}

/// A field that may be absent or null, but when present must be an object.
fn optional_object<'a>(
    parent: &'a Value,
    key: &str,
    path: &str,
) -> Result<Option<&'a Value>, String> {
    match parent.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) if value.is_object() => Ok(Some(value)),
        Some(_) => Err(format!("{path} must be an object")),
    }
}

/// A required percentage, 0 to 100.
fn percent(entry: &Value, key: &str, path: &str) -> Result<f64, String> {
    let value = entry
        .get(key)
        .ok_or_else(|| format!("{path}.{key} is missing"))?;
    value
        .as_f64()
        .filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
        .ok_or_else(|| format!("{path}.{key} must be a number from 0 to 100"))
}

/// Whole seconds as an integer, or a float with no fraction (`1738425600.0`).
// MAX_EPOCH is far below 2^53, so the float comparison and the casts are exact.
#[allow(
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn whole_seconds(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        let n = value.as_f64()?;
        (n.is_finite() && n >= 0.0 && n <= MAX_EPOCH as f64 && n.trunc() == n).then_some(n as u64)
    })
}

/// An optional Unix time in the one supported range.
fn epoch(entry: &Value, key: &str, path: &str) -> Result<Option<u64>, String> {
    match entry.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let time = whole_seconds(value)
                .ok_or_else(|| format!("{path}.{key} must be a Unix time in whole seconds"))?;
            valid_time(time, &format!("{path}.{key}"))?;
            Ok(Some(time))
        }
    }
}

/// An optional window length in minutes: a whole number from 1 to a year.
fn duration_mins(entry: &Value, path: &str) -> Result<Option<i64>, String> {
    match entry.get("windowDurationMins") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let mins = value
                .as_i64()
                .filter(|mins| (1..=MAX_WINDOW_MINS).contains(mins))
                .ok_or_else(|| {
                    format!(
                        "{path}.windowDurationMins must be a whole number of minutes from 1 to \
                         {MAX_WINDOW_MINS}"
                    )
                })?;
            Ok(Some(mins))
        }
    }
}

/// Read Claude Code's statusline JSON. A payload without `rate_limits` (an
/// API-key session, or before the first response) is an empty reading, not an
/// error, and so is a window that is absent or null; a field of the wrong type
/// is an error.
///
/// # Errors
/// Invalid JSON, or a field with a wrong type or a value out of range.
pub fn parse_claude_statusline(text: &str) -> Result<Reading, String> {
    let json = parse_json(text)?;
    let mut reading = Reading::default();
    let Some(limits) = optional_object(&json, "rate_limits", "rate_limits")? else {
        return Ok(reading);
    };
    let window = |name: &str| -> Result<Option<Window>, String> {
        let path = format!("rate_limits.{name}");
        let Some(entry) = optional_object(limits, name, &path)? else {
            return Ok(None);
        };
        Ok(Some(Window {
            used_percent: percent(entry, "used_percentage", &path)?,
            resets_at: epoch(entry, "resets_at", &path)?,
        }))
    };
    reading.five_hour = window("five_hour")?;
    reading.weekly = window("seven_day")?;
    Ok(reading)
}

/// Read a Codex app-server rate-limit payload: the `account/rateLimits/read`
/// result, the `account/rateLimits/updated` params, or either wrapped in its
/// JSON-RPC envelope. Windows are told apart by `windowDurationMins` (up to 6
/// hours is the 5-hour window, 6 days or more is the weekly one, any length
/// between is not one this reads and is left out); a window that gives no
/// duration is placed by position, `primary` first.
///
/// # Errors
/// Invalid JSON, a missing `rateLimits`, or a field with a wrong type or a
/// value out of range.
pub fn parse_codex_rate_limits(text: &str) -> Result<Reading, String> {
    let mut json = parse_json(text)?;
    for envelope in ["params", "result"] {
        if let Some(inner) = json.get(envelope).filter(|v| v.is_object()).cloned() {
            json = inner;
        }
    }
    let limits = json
        .get("rateLimits")
        .filter(|v| v.is_object())
        .ok_or("no rateLimits object; expected the account/rateLimits read result or update")?;
    let mut reading = Reading::default();
    for (position, name) in ["primary", "secondary"].into_iter().enumerate() {
        let path = format!("rateLimits.{name}");
        let Some(entry) = optional_object(limits, name, &path)? else {
            continue;
        };
        let window = Window {
            used_percent: percent(entry, "usedPercent", &path)?,
            resets_at: epoch(entry, "resetsAt", &path)?,
        };
        let weekly = match duration_mins(entry, &path)? {
            Some(mins) if mins <= FIVE_HOUR_MAX_MINS => false,
            Some(mins) if mins >= WEEKLY_MIN_MINS => true,
            Some(_) => continue,
            None => position == 1,
        };
        if weekly {
            reading.weekly = Some(window);
        } else {
            reading.five_hour = Some(window);
        }
    }
    Ok(reading)
}

/// A Grok Build limit-hit report: Grok exposes no usage reading, so the error
/// itself is the signal.
///
/// # Errors
/// The reset time is outside the supported range.
pub fn grok_limit_hit(resets_at: Option<u64>) -> Result<Reading, String> {
    if let Some(reset) = resets_at {
        valid_time(reset, "the reset time")?;
    }
    Ok(Reading {
        limit_hit: true,
        limit_hit_resets_at: resets_at,
        ..Reading::default()
    })
}

fn limits_folder() -> String {
    format!("{LOCAL_DIR}/{LIMITS_DIR}")
}

fn file_for(family: Family) -> String {
    format!("{}/{}.json", limits_folder(), family.id())
}

/// Whether opening the lock failed with the transient "no such file" a
/// concurrent replace in the same folder can cause on macOS. A link in the path
/// is reported by [`crate::contained::Tree`] without an OS error code, so it
/// never matches.
#[cfg(unix)]
fn lock_creation_raced(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(libc::ENOENT)
}

#[cfg(not(unix))]
fn lock_creation_raced(_: &std::io::Error) -> bool {
    false
}

/// An exclusive hold on one family's reading file while it is merged.
struct FamilyLock {
    _file: std::fs::File,
}

impl FamilyLock {
    fn acquire(tree: &crate::contained::Tree, family: Family) -> Result<Self, String> {
        let relative = format!("{}/{}.lock", limits_folder(), family.id());
        let shown = crate::git::display_path(&tree.path().join(&relative));
        let started = Instant::now();
        // Creating the lock while another recorder replaces the snapshot in
        // the same folder can report "not found" on macOS (APFS) for an
        // instant, although the folder exists. Open again, bounded by the
        // same wait as the lock itself.
        let file = loop {
            match tree.lock_file(&relative) {
                Ok(file) => break file,
                Err(error) if lock_creation_raced(&error) && started.elapsed() < LOCK_WAIT => {
                    tree.create_dir_all(&limits_folder())
                        .map_err(|e| format!("cannot use {}: {e}", limits_folder()))?;
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => return Err(format!("{shown}: cannot open the lock ({error})")),
            }
        };
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(error) if error.kind() == fs2::lock_contended_error().kind() => {
                    if started.elapsed() >= LOCK_WAIT {
                        return Err(format!(
                            "{shown}: another process holds the lock; try again"
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(format!("{shown}: cannot take the lock ({error})")),
            }
        }
    }
}

/// Merge `reading` into the stored entry of the config home that produced it
/// (`home`, `None` for the CLI's default home) and write the file. A window the
/// reading carries replaces the stored one; a window it lacks is kept (Codex
/// updates are sparse); a window already past its reset time is dropped first.
/// The read, merge and replace happen under a per-family lock, so two
/// recorders never lose each other's windows.
///
/// Returns `None`, writing nothing, when the reading is empty.
///
/// # Errors
/// An input outside its range, a stored file that is unreadable or invalid, a
/// link or non-folder in the local folder's path, a lock that stays held, or a
/// failed write.
pub fn record(
    project: &Path,
    family: Family,
    home: Option<&str>,
    source: &str,
    reading: &Reading,
    now: u64,
) -> Result<Option<HomeReading>, String> {
    if let Some(home) = home {
        valid_home(home)?;
    }
    valid_source(source)?;
    valid_time(now, "the clock")?;
    reading.validate()?;
    if reading.is_empty() {
        return Ok(None);
    }
    let tree = crate::contained::Tree::open(project).map_err(|e| {
        format!(
            "{}: cannot be opened ({e})",
            crate::git::display_path(project)
        )
    })?;
    tree.create_dir_all(&limits_folder())
        .map_err(|e| format!("cannot use {}: {e}", limits_folder()))?;
    let _lock = FamilyLock::acquire(&tree, family)?;
    let mut snapshot = load(project, family)?.unwrap_or(Snapshot {
        schema_version: 2,
        family: family.id().into(),
        homes: Vec::new(),
    });
    let position = snapshot
        .homes
        .iter()
        .position(|entry| same_home(entry.config_home.as_deref(), home));
    let mut entry = position.map_or_else(
        || HomeReading {
            config_home: home.map(str::to_string),
            source: source.into(),
            recorded_at: now,
            five_hour: None,
            weekly: None,
            limit_hit_at: None,
            limit_hit_resets_at: None,
        },
        |at| snapshot.homes.remove(at),
    );
    let live = |w: Option<Window>| w.filter(|w| w.resets_at.is_none_or(|t| t > now));
    entry.five_hour = reading.five_hour.or(live(entry.five_hour));
    entry.weekly = reading.weekly.or(live(entry.weekly));
    if reading.limit_hit {
        entry.limit_hit_at = Some(now);
        entry.limit_hit_resets_at = reading.limit_hit_resets_at;
    }
    entry.source = source.into();
    entry.recorded_at = now;
    snapshot.homes.push(entry.clone());
    if snapshot.homes.len() > MAX_HOMES {
        snapshot.homes.sort_by_key(|h| h.recorded_at);
        snapshot.homes.remove(0);
    }
    snapshot.validate(family)?;
    let text = serde_json::to_string_pretty(&snapshot).map_err(|e| e.to_string())? + "\n";
    tree.write(&file_for(family), text.as_bytes())
        .map_err(|e| format!("cannot write {}: {e}", file_for(family)))?;
    Ok(Some(entry))
}

/// What is stored for a family.
enum Stored {
    /// No file.
    Absent,
    /// A version 1 file from an earlier build. It has no producing config
    /// home, so its readings belong to no account: they are never shown, and
    /// never assigned to the current home. The next recording replaces it.
    Legacy,
    /// A valid version 2 snapshot.
    Current(Snapshot),
}

fn read_stored(project: &Path, family: Family) -> Result<Stored, String> {
    let relative = file_for(family);
    let Some(bytes) = confined_read(project, &relative, MAX_SNAPSHOT_BYTES)? else {
        return Ok(Stored::Absent);
    };
    let shown = crate::git::display_path(&project.join(&relative));
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("{shown}: not a limit snapshot ({error})"))?;
    if value.get("schema_version") == Some(&serde_json::json!(1))
        && value.get("family") == Some(&serde_json::json!(family.id()))
    {
        return Ok(Stored::Legacy);
    }
    let snapshot: Snapshot = serde_json::from_value(value)
        .map_err(|error| format!("{shown}: not a limit snapshot ({error})"))?;
    snapshot
        .validate(family)
        .map_err(|reason| format!("{shown}: {reason}"))?;
    Ok(Stored::Current(snapshot))
}

/// The stored snapshot for a family, or `None` when nothing was recorded or
/// the file is a version 1 file from an earlier build (its readings name no
/// account, so none is kept; see [`record`], which replaces it).
///
/// # Errors
/// The file exists but is under a link, is not a regular file, is too large or
/// is not a valid snapshot (a percentage or time out of range included).
pub fn load(project: &Path, family: Family) -> Result<Option<Snapshot>, String> {
    Ok(match read_stored(project, family)? {
        Stored::Current(snapshot) => Some(snapshot),
        Stored::Absent | Stored::Legacy => None,
    })
}

/// What the view says about one window.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WindowView {
    /// No reading for this window and this config home.
    NotExposed,
    /// A reading still inside its window.
    Used {
        /// Share used, 0 to 100.
        used_percent: f64,
        /// Unix reset time, when known.
        resets_at: Option<u64>,
    },
    /// The window reset after the reading, so the number no longer applies.
    ResetSinceReading {
        /// The percentage that was read.
        was_percent: f64,
        /// When it reset.
        reset_at: u64,
    },
}

/// One family's limit state at a moment, for one config home.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FamilyLimits {
    /// Family id.
    pub family: &'static str,
    /// The 5-hour window.
    pub five_hour: WindowView,
    /// The weekly window.
    pub weekly: WindowView,
    /// A limit-hit error was reported, and when.
    pub limit_hit_at: Option<u64>,
    /// Reset time that error gave.
    pub limit_hit_resets_at: Option<u64>,
    /// Where the reading came from, or `None` when nothing was recorded.
    pub source: Option<String>,
    /// Unix time of the reading.
    pub recorded_at: Option<u64>,
    /// Whether this config home has a reading.
    pub exposed: bool,
    /// A version 1 file from an earlier build is stored. It names no
    /// account, so it is ignored here and replaced by the next recording.
    pub legacy_unbound: bool,
    /// Config homes that have readings other than the one asked about (the
    /// default home is `default home`). Those readings are not this
    /// account's and are not shown as its windows.
    pub other_homes: Vec<String>,
}

fn view(window: Option<Window>, now: u64) -> WindowView {
    match window {
        None => WindowView::NotExposed,
        Some(w) => match w.resets_at {
            Some(reset) if reset <= now => WindowView::ResetSinceReading {
                was_percent: w.used_percent,
                reset_at: reset,
            },
            resets_at => WindowView::Used {
                used_percent: w.used_percent,
                resets_at,
            },
        },
    }
}

/// The limit state of `family` at `now` for the config home `home` (`None` for
/// the CLI's default home), as the current profile says it.
///
/// # Errors
/// The stored snapshot is invalid.
pub fn state(
    project: &Path,
    family: Family,
    home: Option<&str>,
    now: u64,
) -> Result<FamilyLimits, String> {
    let (snapshot, legacy_unbound) = match read_stored(project, family)? {
        Stored::Current(snapshot) => (Some(snapshot), false),
        Stored::Legacy => (None, true),
        Stored::Absent => (None, false),
    };
    let homes = snapshot.as_ref().map_or(&[][..], |s| s.homes.as_slice());
    let mine = homes
        .iter()
        .find(|entry| same_home(entry.config_home.as_deref(), home));
    let other_homes = homes
        .iter()
        .filter(|entry| !same_home(entry.config_home.as_deref(), home))
        .map(|entry| {
            entry
                .config_home
                .as_deref()
                .map_or_else(|| "default home".to_string(), crate::git::display_text)
        })
        .collect();
    Ok(FamilyLimits {
        family: family.id(),
        five_hour: view(mine.and_then(|h| h.five_hour), now),
        weekly: view(mine.and_then(|h| h.weekly), now),
        limit_hit_at: mine.and_then(|h| h.limit_hit_at),
        limit_hit_resets_at: mine.and_then(|h| h.limit_hit_resets_at),
        source: mine.map(|h| h.source.clone()),
        recorded_at: mine.map(|h| h.recorded_at),
        exposed: mine.is_some(),
        legacy_unbound,
        other_homes,
    })
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // fixture percentages are exact
mod tests {
    use super::*;

    const FAR: u64 = 4_102_444_800; // 2100-01-01
    /// An absolute path on every platform: Windows needs a drive prefix, so a
    /// bare `/srv/x` is not absolute there.
    const ROOT: &str = if cfg!(windows) { "C:/" } else { "/" };
    const HOME_A: &str = if cfg!(windows) {
        "C:/srv/codex-a"
    } else {
        "/srv/codex-a"
    };
    const HOME_B: &str = if cfg!(windows) {
        "C:/srv/codex-b"
    } else {
        "/srv/codex-b"
    };

    fn stored(dir: &Path, family: Family) -> std::path::PathBuf {
        dir.join(file_for(family))
    }

    #[test]
    fn claude_statusline_windows_are_read_and_absence_is_not_an_error() {
        let payload = r#"{"model":{"id":"x"},"rate_limits":{
            "five_hour":{"used_percentage":23.5,"resets_at":4102444800},
            "seven_day":{"used_percentage":41.2,"resets_at":4102531200}}}"#;
        let reading = parse_claude_statusline(payload).unwrap();
        assert_eq!(reading.five_hour.unwrap().used_percent, 23.5);
        assert_eq!(reading.weekly.unwrap().resets_at, Some(4_102_531_200));
        assert!(parse_claude_statusline(r#"{"model":{}}"#)
            .unwrap()
            .is_empty());
        for absent in [
            r#"{"rate_limits":null}"#,
            r#"{"rate_limits":{"five_hour":null}}"#,
            r#"{"rate_limits":{}}"#,
        ] {
            assert!(
                parse_claude_statusline(absent).unwrap().is_empty(),
                "{absent}"
            );
        }
        let whole =
            r#"{"rate_limits":{"five_hour":{"used_percentage":5,"resets_at":4102444800.0}}}"#;
        assert_eq!(
            parse_claude_statusline(whole)
                .unwrap()
                .five_hour
                .unwrap()
                .resets_at,
            Some(4_102_444_800)
        );
        let one = r#"{"rate_limits":{"seven_day":{"used_percentage":5}}}"#;
        let one = parse_claude_statusline(one).unwrap();
        assert!(one.five_hour.is_none() && one.weekly.unwrap().resets_at.is_none());
    }

    #[test]
    fn wrong_types_and_out_of_range_values_are_errors_not_absence() {
        for payload in [
            "not json",
            "[]",
            r#"{"rate_limits":"oops"}"#,
            r#"{"rate_limits":[]}"#,
            r#"{"rate_limits":{"five_hour":"oops"}}"#,
            r#"{"rate_limits":{"seven_day":7}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":101}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":-123}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":"9"}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":null}}}"#,
            r#"{"rate_limits":{"five_hour":{"resets_at":5}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":5,"resets_at":-1}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":5,"resets_at":1.5}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":5,"resets_at":"soon"}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":5,"resets_at":18446744073709551615}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":5,"resets_at":253402300800}}}"#,
        ] {
            assert!(parse_claude_statusline(payload).is_err(), "{payload}");
        }
        // The latest supported second is accepted.
        let edge =
            r#"{"rate_limits":{"five_hour":{"used_percentage":5,"resets_at":253402300799}}}"#;
        assert!(parse_claude_statusline(edge).is_ok());
    }

    #[test]
    fn codex_windows_are_told_apart_by_duration_then_position() {
        let read = r#"{"rateLimits":{"limitId":"codex","planType":"pro",
            "primary":{"usedPercent":12,"windowDurationMins":300,"resetsAt":4102444800},
            "secondary":{"usedPercent":60,"windowDurationMins":10080,"resetsAt":4102531200}},
            "rateLimitsByLimitId":null}"#;
        let reading = parse_codex_rate_limits(read).unwrap();
        assert_eq!(reading.five_hour.unwrap().used_percent, 12.0);
        assert_eq!(reading.weekly.unwrap().used_percent, 60.0);
        let weekly_only =
            r#"{"rateLimits":{"primary":{"usedPercent":7,"windowDurationMins":10080}}}"#;
        let reading = parse_codex_rate_limits(weekly_only).unwrap();
        assert!(reading.five_hour.is_none() && reading.weekly.is_some());
        let bare = r#"{"rateLimits":{"primary":{"usedPercent":1},"secondary":{"usedPercent":2}}}"#;
        let reading = parse_codex_rate_limits(bare).unwrap();
        assert_eq!(reading.five_hour.unwrap().used_percent, 1.0);
        assert_eq!(reading.weekly.unwrap().used_percent, 2.0);
        // A window length that is neither 5-hour nor weekly is left out.
        let odd = r#"{"rateLimits":{"primary":{"usedPercent":3,"windowDurationMins":1440}}}"#;
        assert!(parse_codex_rate_limits(odd).unwrap().is_empty());
    }

    #[test]
    fn codex_durations_and_wrong_types_are_errors() {
        for payload in [
            r#"{"rateLimits":{"primary":{"usedPercent":5,"windowDurationMins":"weekly"}}}"#,
            r#"{"rateLimits":{"primary":{"usedPercent":5,"windowDurationMins":-5}}}"#,
            r#"{"rateLimits":{"primary":{"usedPercent":5,"windowDurationMins":0}}}"#,
            r#"{"rateLimits":{"primary":{"usedPercent":5,"windowDurationMins":300.5}}}"#,
            r#"{"rateLimits":{"primary":{"usedPercent":5,"windowDurationMins":99999999}}}"#,
            r#"{"rateLimits":{"primary":"oops"}}"#,
            r#"{"rateLimits":{"primary":{"usedPercent":150}}}"#,
            r#"{"rateLimits":{"primary":{"usedPercent":5,"resetsAt":18446744073709551615}}}"#,
            r#"{"rateLimits":"x"}"#,
        ] {
            assert!(parse_codex_rate_limits(payload).is_err(), "{payload}");
        }
    }

    #[test]
    fn codex_json_rpc_envelopes_are_unwrapped() {
        let event = r#"{"jsonrpc":"2.0","method":"account/rateLimits/updated",
            "params":{"rateLimits":{"primary":{"usedPercent":88,"windowDurationMins":300}}}}"#;
        assert_eq!(
            parse_codex_rate_limits(event)
                .unwrap()
                .five_hour
                .unwrap()
                .used_percent,
            88.0
        );
        let response = r#"{"id":3,"result":{"rateLimits":
            {"secondary":{"usedPercent":9,"windowDurationMins":10080}}}}"#;
        assert!(parse_codex_rate_limits(response).unwrap().weekly.is_some());
        assert!(parse_codex_rate_limits(r#"{"result":{}}"#).is_err());
        assert!(parse_codex_rate_limits(
            r#"{"rateLimits":{"primary":{"windowDurationMins":300}}}"#
        )
        .is_err());
    }

    fn full_codex() -> Reading {
        parse_codex_rate_limits(
            r#"{"rateLimits":{"primary":{"usedPercent":10,"windowDurationMins":300,"resetsAt":4102444800},
                "secondary":{"usedPercent":50,"windowDurationMins":10080,"resetsAt":4102531200}}}"#,
        )
        .unwrap()
    }

    #[test]
    fn a_sparse_update_keeps_the_window_it_does_not_carry() {
        let dir = tempfile::tempdir().unwrap();
        record(
            dir.path(),
            Family::Codex,
            Some(HOME_A),
            "codex",
            &full_codex(),
            1000,
        )
        .unwrap();
        let sparse = parse_codex_rate_limits(
            r#"{"rateLimits":{"primary":{"usedPercent":30,"windowDurationMins":300,"resetsAt":4102444800}}}"#,
        )
        .unwrap();
        let merged = record(
            dir.path(),
            Family::Codex,
            Some(HOME_A),
            "codex",
            &sparse,
            2000,
        )
        .unwrap()
        .unwrap();
        assert_eq!(merged.five_hour.unwrap().used_percent, 30.0);
        assert_eq!(merged.weekly.unwrap().used_percent, 50.0);
        assert_eq!(merged.recorded_at, 2000);
    }

    #[test]
    fn an_expired_window_is_shown_as_reset_and_dropped_on_the_next_record() {
        let dir = tempfile::tempdir().unwrap();
        let old = Reading {
            five_hour: Some(Window {
                used_percent: 95.0,
                resets_at: Some(500),
            }),
            weekly: Some(Window {
                used_percent: 40.0,
                resets_at: Some(FAR),
            }),
            ..Reading::default()
        };
        record(dir.path(), Family::Claude, None, "claude", &old, 100).unwrap();
        let shown = state(dir.path(), Family::Claude, None, 600).unwrap();
        assert_eq!(
            shown.five_hour,
            WindowView::ResetSinceReading {
                was_percent: 95.0,
                reset_at: 500
            }
        );
        assert!(matches!(shown.weekly, WindowView::Used { .. }));
        let only_weekly = Reading {
            weekly: Some(Window {
                used_percent: 41.0,
                resets_at: Some(FAR),
            }),
            ..Reading::default()
        };
        let merged = record(
            dir.path(),
            Family::Claude,
            None,
            "claude",
            &only_weekly,
            600,
        )
        .unwrap()
        .unwrap();
        assert!(merged.five_hour.is_none());
    }

    #[test]
    fn nothing_recorded_reads_as_not_exposed_and_an_empty_reading_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let shown = state(dir.path(), Family::Grok, None, 1).unwrap();
        assert!(!shown.exposed);
        assert_eq!(shown.five_hour, WindowView::NotExposed);
        assert!(record(
            dir.path(),
            Family::Grok,
            None,
            "grok",
            &Reading::default(),
            1
        )
        .unwrap()
        .is_none());
        assert!(!dir.path().join(LOCAL_DIR).exists());
    }

    #[test]
    fn a_grok_limit_hit_is_kept_with_its_time() {
        let dir = tempfile::tempdir().unwrap();
        record(
            dir.path(),
            Family::Grok,
            None,
            "grok limit-hit",
            &grok_limit_hit(Some(FAR)).unwrap(),
            700,
        )
        .unwrap();
        let shown = state(dir.path(), Family::Grok, None, 800).unwrap();
        assert_eq!(shown.limit_hit_at, Some(700));
        assert_eq!(shown.limit_hit_resets_at, Some(FAR));
        assert_eq!(shown.five_hour, WindowView::NotExposed);
        assert!(grok_limit_hit(Some(u64::MAX)).is_err());
    }

    #[test]
    fn a_reading_belongs_to_the_home_that_produced_it() {
        let dir = tempfile::tempdir().unwrap();
        record(
            dir.path(),
            Family::Codex,
            Some(HOME_A),
            "codex",
            &full_codex(),
            1000,
        )
        .unwrap();
        // The profile now names home B: A's windows are not B's.
        let b = state(dir.path(), Family::Codex, Some(HOME_B), 1500).unwrap();
        assert!(!b.exposed);
        assert_eq!(b.five_hour, WindowView::NotExposed);
        assert_eq!(b.weekly, WindowView::NotExposed);
        assert_eq!(b.other_homes, vec![HOME_A.to_string()]);
        // The default home is a different identity too.
        let default = state(dir.path(), Family::Codex, None, 1500).unwrap();
        assert!(!default.exposed);
        // A is still readable as itself.
        let a = state(dir.path(), Family::Codex, Some(HOME_A), 1500).unwrap();
        assert!(a.exposed && matches!(a.five_hour, WindowView::Used { .. }));
        assert!(a.other_homes.is_empty());
        // Recording for B keeps both.
        record(
            dir.path(),
            Family::Codex,
            Some(HOME_B),
            "codex",
            &full_codex(),
            1600,
        )
        .unwrap();
        assert_eq!(
            load(dir.path(), Family::Codex)
                .unwrap()
                .unwrap()
                .homes
                .len(),
            2
        );
    }

    #[test]
    fn only_the_newest_eight_homes_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..10u64 {
            let home = format!("{ROOT}srv/h{i}");
            record(
                dir.path(),
                Family::Codex,
                Some(&home),
                "codex",
                &full_codex(),
                100 + i,
            )
            .unwrap();
        }
        let snapshot = load(dir.path(), Family::Codex).unwrap().unwrap();
        assert_eq!(snapshot.homes.len(), MAX_HOMES);
        assert!(snapshot
            .homes
            .iter()
            .all(|h| h.config_home.as_deref() != Some(format!("{ROOT}srv/h0").as_str())));
    }

    #[test]
    fn bad_identities_and_times_are_refused_on_ingest() {
        let dir = tempfile::tempdir().unwrap();
        let reading = full_codex();
        for home in ["relative/path", "", "/srv/a\nb"] {
            assert!(record(dir.path(), Family::Codex, Some(home), "codex", &reading, 1).is_err());
        }
        assert!(record(dir.path(), Family::Codex, None, "", &reading, 1).is_err());
        assert!(record(
            dir.path(),
            Family::Codex,
            None,
            "codex",
            &reading,
            MAX_EPOCH + 1
        )
        .is_err());
        assert!(!dir.path().join(LOCAL_DIR).exists());
    }

    #[test]
    fn a_damaged_foreign_or_out_of_range_snapshot_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = stored(dir.path(), Family::Claude);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for text in [
            "{".to_string(),
            r#"{"schema_version":3,"family":"claude","homes":[]}"#.to_string(),
            r#"{"schema_version":2,"family":"codex","homes":[]}"#.to_string(),
            r#"{"schema_version":2,"family":"claude","homes":[{"source":"x","recorded_at":1,
                "five_hour":{"used_percent":-123.0}}]}"#
                .to_string(),
            r#"{"schema_version":2,"family":"claude","homes":[{"source":"x","recorded_at":1,
                "five_hour":{"used_percent":5.0,"resets_at":18446744073709551615}}]}"#
                .to_string(),
            r#"{"schema_version":2,"family":"claude","homes":[{"source":"x","recorded_at":1,
                "config_home":"relative"}]}"#
                .to_string(),
            r#"{"schema_version":2,"family":"claude","homes":[{"source":"x","recorded_at":1},
                {"source":"y","recorded_at":2}]}"#
                .to_string(),
        ] {
            std::fs::write(&path, &text).unwrap();
            assert!(load(dir.path(), Family::Claude).is_err(), "{text}");
            assert!(
                state(dir.path(), Family::Claude, None, 5).is_err(),
                "{text}"
            );
        }
    }

    #[test]
    fn a_version_1_file_is_ignored_then_replaced_and_never_given_to_a_home() {
        let dir = tempfile::tempdir().unwrap();
        let path = stored(dir.path(), Family::Claude);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // The shape the previous build wrote: one reading, no producing home.
        std::fs::write(
            &path,
            r#"{"schema_version":1,"family":"claude","source":"claude-statusline",
                "recorded_at":100,"five_hour":{"used_percent":77.0,"resets_at":4102444800}}"#,
        )
        .unwrap();
        // Read: no error, nothing shown for any home, and the file is named.
        assert!(load(dir.path(), Family::Claude).unwrap().is_none());
        for home in [None, Some(HOME_A)] {
            let view = state(dir.path(), Family::Claude, home, 200).unwrap();
            assert!(view.legacy_unbound && !view.exposed);
            assert_eq!(view.five_hour, WindowView::NotExposed);
        }
        // Another family's file is untouched by this one.
        assert!(
            !state(dir.path(), Family::Codex, None, 200)
                .unwrap()
                .legacy_unbound
        );
        // A fresh reading is recorded under its own home and replaces the file.
        record(
            dir.path(),
            Family::Claude,
            Some(HOME_A),
            "claude-statusline",
            &full_codex(),
            300,
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("\"schema_version\": 2") && !text.contains("77"),
            "{text}"
        );
        let view = state(dir.path(), Family::Claude, Some(HOME_A), 400).unwrap();
        assert!(view.exposed && !view.legacy_unbound);
        // The old 77 percent was not carried to the current home or any other.
        let other = state(dir.path(), Family::Claude, None, 400).unwrap();
        assert_eq!(other.five_hour, WindowView::NotExposed);
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_limits_folder_or_file_is_refused_for_reads_and_writes() {
        use std::os::unix::fs::symlink;
        let outside = tempfile::tempdir().unwrap();
        // The limits folder is a link to a folder outside.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(LOCAL_DIR)).unwrap();
        symlink(outside.path(), dir.path().join(LOCAL_DIR).join(LIMITS_DIR)).unwrap();
        let err = record(dir.path(), Family::Claude, None, "claude", &full_codex(), 1);
        assert!(err.is_err());
        assert!(state(dir.path(), Family::Claude, None, 1).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        // The local folder itself is a link.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        symlink(outside.path(), dir.path().join(LOCAL_DIR)).unwrap();
        assert!(record(dir.path(), Family::Claude, None, "claude", &full_codex(), 1).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        // The snapshot file is a link to a file outside.
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join(LOCAL_DIR).join(LIMITS_DIR);
        std::fs::create_dir_all(&folder).unwrap();
        let target = outside.path().join("target.json");
        std::fs::write(&target, "{}").unwrap();
        symlink(&target, folder.join("claude.json")).unwrap();
        assert!(record(dir.path(), Family::Claude, None, "claude", &full_codex(), 1).is_err());
        assert!(load(dir.path(), Family::Claude).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "{}");
        // The lock file is a link.
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join(LOCAL_DIR).join(LIMITS_DIR);
        std::fs::create_dir_all(&folder).unwrap();
        symlink(&target, folder.join("claude.lock")).unwrap();
        assert!(record(dir.path(), Family::Claude, None, "claude", &full_codex(), 1).is_err());
    }

    #[test]
    fn concurrent_sparse_recorders_never_lose_each_others_windows() {
        use std::sync::{Arc, Barrier};
        for round in 0..30u64 {
            let dir = tempfile::tempdir().unwrap();
            let barrier = Arc::new(Barrier::new(2));
            let primary = parse_codex_rate_limits(
                r#"{"rateLimits":{"primary":{"usedPercent":1,"windowDurationMins":300,"resetsAt":4102444800}}}"#,
            )
            .unwrap();
            let secondary = parse_codex_rate_limits(
                r#"{"rateLimits":{"secondary":{"usedPercent":2,"windowDurationMins":10080,"resetsAt":4102444800}}}"#,
            )
            .unwrap();
            let handles: Vec<_> = [primary, secondary]
                .into_iter()
                .map(|reading| {
                    let (root, barrier) = (dir.path().to_path_buf(), Arc::clone(&barrier));
                    std::thread::spawn(move || {
                        barrier.wait();
                        record(
                            &root,
                            Family::Codex,
                            Some(HOME_A),
                            "codex",
                            &reading,
                            50 + round,
                        )
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap().unwrap();
            }
            let shown = state(dir.path(), Family::Codex, Some(HOME_A), 100).unwrap();
            assert!(
                matches!(shown.five_hour, WindowView::Used { .. })
                    && matches!(shown.weekly, WindowView::Used { .. }),
                "round {round} lost a window: {shown:?}"
            );
        }
    }
}
