//! Limit readings of each family, in one structure (TSK-431).
//!
//! A reading is a percentage used and a reset time per window, kept in a
//! small file per family under the project's untracked local folder. Nothing
//! here asks a vendor for anything: the readings come from what each CLI
//! already hands the host.
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

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Family;

const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
const FIVE_HOUR_MAX_MINS: i64 = 6 * 60;
const WEEKLY_MIN_MINS: i64 = 6 * 24 * 60;

/// Folder, under the local folder, that holds one reading file per family.
pub const LIMITS_DIR: &str = "limits";

/// One usage window.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Window {
    /// Share of the window used, 0 to 100.
    pub used_percent: f64,
    /// Unix time the window resets, when the CLI says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<u64>,
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
    fn is_empty(&self) -> bool {
        self.five_hour.is_none() && self.weekly.is_none() && !self.limit_hit
    }
}

/// A stored reading.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    /// Always 1.
    pub schema_version: u32,
    /// Family id.
    pub family: String,
    /// Where the reading came from.
    pub source: String,
    /// Unix time the file was last written.
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

/// Seconds since the Unix epoch.
#[must_use]
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn percent(value: &Value, what: &str) -> Result<f64, String> {
    let number = value
        .as_f64()
        .filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
        .ok_or_else(|| format!("{what} must be a number from 0 to 100"))?;
    Ok(number)
}

/// A JSON writer may print whole seconds as `1738425600.0`; accept that, not
/// a fraction, a negative number or one too large to be a time.
#[allow(
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn whole_seconds(n: f64) -> Option<u64> {
    (n.is_finite() && (0.0..1e15).contains(&n) && n.trunc() == n).then_some(n as u64)
}

fn epoch(value: Option<&Value>, what: &str) -> Result<Option<u64>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .or_else(|| v.as_f64().and_then(whole_seconds))
            .map(Some)
            .ok_or_else(|| format!("{what} must be a Unix time in whole seconds")),
    }
}

fn parse_json(text: &str) -> Result<Value, String> {
    if text.len() > MAX_PAYLOAD_BYTES {
        return Err("payload is larger than 256 KiB".into());
    }
    serde_json::from_str(text).map_err(|error| format!("payload is not JSON: {error}"))
}

/// Read Claude Code's statusline JSON. A payload without `rate_limits` (an
/// API-key session, or before the first response) is an empty reading, not an
/// error.
///
/// # Errors
/// Invalid JSON, or a window with a malformed percentage or reset time.
pub fn parse_claude_statusline(text: &str) -> Result<Reading, String> {
    let json = parse_json(text)?;
    let mut reading = Reading::default();
    let Some(limits) = json.get("rate_limits").filter(|v| v.is_object()) else {
        return Ok(reading);
    };
    let window = |name: &str| -> Result<Option<Window>, String> {
        let Some(entry) = limits.get(name).filter(|v| v.is_object()) else {
            return Ok(None);
        };
        let used = entry
            .get("used_percentage")
            .ok_or_else(|| format!("rate_limits.{name}.used_percentage is missing"))?;
        Ok(Some(Window {
            used_percent: percent(used, &format!("rate_limits.{name}.used_percentage"))?,
            resets_at: epoch(
                entry.get("resets_at"),
                &format!("rate_limits.{name}.resets_at"),
            )?,
        }))
    };
    reading.five_hour = window("five_hour")?;
    reading.weekly = window("seven_day")?;
    Ok(reading)
}

/// Read a Codex app-server rate-limit payload: the `account/rateLimits/read`
/// result, the `account/rateLimits/updated` params, or either wrapped in its
/// JSON-RPC envelope. Windows are told apart by `windowDurationMins` (up to 6
/// hours is the 5-hour window, 6 days or more is the weekly one); a window
/// that gives no duration is placed by position, `primary` first.
///
/// # Errors
/// Invalid JSON, a missing `rateLimits`, or a window with a malformed value.
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
        let Some(entry) = limits.get(name).filter(|v| v.is_object()) else {
            continue;
        };
        let used = entry
            .get("usedPercent")
            .ok_or_else(|| format!("rateLimits.{name}.usedPercent is missing"))?;
        let window = Window {
            used_percent: percent(used, &format!("rateLimits.{name}.usedPercent"))?,
            resets_at: epoch(
                entry.get("resetsAt"),
                &format!("rateLimits.{name}.resetsAt"),
            )?,
        };
        let weekly = match entry.get("windowDurationMins").and_then(Value::as_i64) {
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
#[must_use]
pub fn grok_limit_hit(resets_at: Option<u64>) -> Reading {
    Reading {
        limit_hit: true,
        limit_hit_resets_at: resets_at,
        ..Reading::default()
    }
}

fn file_for(local: &Path, family: Family) -> PathBuf {
    local.join(LIMITS_DIR).join(format!("{}.json", family.id()))
}

/// Merge `reading` into the family's stored snapshot and write it. A window
/// the reading carries replaces the stored one; a window it lacks is kept
/// (Codex updates are sparse); a window already past its reset time is
/// dropped first.
///
/// Returns `None`, writing nothing, when the reading is empty.
///
/// # Errors
/// The stored file is unreadable or invalid, or the write fails.
pub fn record(
    local: &Path,
    family: Family,
    source: &str,
    reading: &Reading,
    now: u64,
) -> Result<Option<Snapshot>, String> {
    if reading.is_empty() {
        return Ok(None);
    }
    let mut snapshot = load(local, family)?.unwrap_or(Snapshot {
        schema_version: 1,
        family: family.id().into(),
        source: source.into(),
        recorded_at: now,
        five_hour: None,
        weekly: None,
        limit_hit_at: None,
        limit_hit_resets_at: None,
    });
    let live = |w: Option<Window>| w.filter(|w| w.resets_at.is_none_or(|t| t > now));
    snapshot.five_hour = reading.five_hour.or(live(snapshot.five_hour));
    snapshot.weekly = reading.weekly.or(live(snapshot.weekly));
    if reading.limit_hit {
        snapshot.limit_hit_at = Some(now);
        snapshot.limit_hit_resets_at = reading.limit_hit_resets_at;
    }
    snapshot.source = source.into();
    snapshot.recorded_at = now;
    let path = file_for(local, family);
    let dir = path.parent().ok_or("no limits folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let text = serde_json::to_string_pretty(&snapshot).map_err(|e| e.to_string())? + "\n";
    let mut file = tempfile::NamedTempFile::new_in(dir)
        .map_err(|e| format!("cannot write in {}: {e}", dir.display()))?;
    std::io::Write::write_all(&mut file, text.as_bytes())
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    file.persist(&path)
        .map_err(|e| format!("cannot replace {}: {}", path.display(), e.error))?;
    Ok(Some(snapshot))
}

/// The stored snapshot for a family, or `None` when nothing was recorded.
///
/// # Errors
/// The file exists but is a link, too large or not a valid snapshot.
pub fn load(local: &Path, family: Family) -> Result<Option<Snapshot>, String> {
    let path = file_for(local, family);
    let bytes = match crate::bounded_file::read_bounded_regular(&path, 64 * 1024) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: cannot read ({error})", path.display())),
    };
    let snapshot: Snapshot = serde_json::from_slice(&bytes)
        .map_err(|error| format!("{}: not a limit snapshot ({error})", path.display()))?;
    if snapshot.schema_version != 1 || snapshot.family != family.id() {
        return Err(format!(
            "{}: not a version 1 snapshot of {}",
            path.display(),
            family.id()
        ));
    }
    Ok(Some(snapshot))
}

/// What the view says about one window.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WindowView {
    /// The CLI gave no reading for this window.
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

/// One family's limit state at a moment.
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
    /// Whether the family exposes any reading in this project.
    pub exposed: bool,
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

/// The limit state of `family` at `now`.
///
/// # Errors
/// The stored snapshot is invalid.
pub fn state(local: &Path, family: Family, now: u64) -> Result<FamilyLimits, String> {
    let snapshot = load(local, family)?;
    let exposed = snapshot.is_some();
    Ok(FamilyLimits {
        family: family.id(),
        five_hour: view(snapshot.as_ref().and_then(|s| s.five_hour), now),
        weekly: view(snapshot.as_ref().and_then(|s| s.weekly), now),
        limit_hit_at: snapshot.as_ref().and_then(|s| s.limit_hit_at),
        limit_hit_resets_at: snapshot.as_ref().and_then(|s| s.limit_hit_resets_at),
        source: snapshot.as_ref().map(|s| s.source.clone()),
        recorded_at: snapshot.as_ref().map(|s| s.recorded_at),
        exposed,
    })
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // fixture percentages are exact
mod tests {
    use super::*;

    const FAR: u64 = 4_102_444_800; // 2100-01-01

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
    fn claude_statusline_rejects_malformed_windows() {
        for payload in [
            "not json",
            r#"{"rate_limits":{"five_hour":{"used_percentage":101}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":"9"}}}"#,
            r#"{"rate_limits":{"five_hour":{"resets_at":5}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":5,"resets_at":-1}}}"#,
            r#"{"rate_limits":{"five_hour":{"used_percentage":5,"resets_at":1.5}}}"#,
        ] {
            assert!(parse_claude_statusline(payload).is_err(), "{payload}");
        }
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
        // Only a weekly window, sent as `primary` with its duration.
        let weekly_only =
            r#"{"rateLimits":{"primary":{"usedPercent":7,"windowDurationMins":10080}}}"#;
        let reading = parse_codex_rate_limits(weekly_only).unwrap();
        assert!(reading.five_hour.is_none() && reading.weekly.is_some());
        // No durations: position decides.
        let bare = r#"{"rateLimits":{"primary":{"usedPercent":1},"secondary":{"usedPercent":2}}}"#;
        let reading = parse_codex_rate_limits(bare).unwrap();
        assert_eq!(reading.five_hour.unwrap().used_percent, 1.0);
        assert_eq!(reading.weekly.unwrap().used_percent, 2.0);
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

    #[test]
    fn a_sparse_update_keeps_the_window_it_does_not_carry() {
        let dir = tempfile::tempdir().unwrap();
        let full = parse_codex_rate_limits(
            r#"{"rateLimits":{"primary":{"usedPercent":10,"windowDurationMins":300,"resetsAt":4102444800},
                "secondary":{"usedPercent":50,"windowDurationMins":10080,"resetsAt":4102531200}}}"#,
        )
        .unwrap();
        record(dir.path(), Family::Codex, "codex", &full, 1000).unwrap();
        let sparse = parse_codex_rate_limits(
            r#"{"rateLimits":{"primary":{"usedPercent":30,"windowDurationMins":300,"resetsAt":4102444800}}}"#,
        )
        .unwrap();
        let merged = record(dir.path(), Family::Codex, "codex", &sparse, 2000)
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
        record(dir.path(), Family::Claude, "claude", &old, 100).unwrap();
        let shown = state(dir.path(), Family::Claude, 600).unwrap();
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
        let merged = record(dir.path(), Family::Claude, "claude", &only_weekly, 600)
            .unwrap()
            .unwrap();
        assert!(merged.five_hour.is_none());
    }

    #[test]
    fn nothing_recorded_reads_as_not_exposed_and_an_empty_reading_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let shown = state(dir.path(), Family::Grok, 1).unwrap();
        assert!(!shown.exposed);
        assert_eq!(shown.five_hour, WindowView::NotExposed);
        assert!(
            record(dir.path(), Family::Grok, "grok", &Reading::default(), 1)
                .unwrap()
                .is_none()
        );
        assert!(!dir.path().join(LIMITS_DIR).exists());
    }

    #[test]
    fn a_grok_limit_hit_is_kept_with_its_time() {
        let dir = tempfile::tempdir().unwrap();
        record(
            dir.path(),
            Family::Grok,
            "grok limit-hit",
            &grok_limit_hit(Some(FAR)),
            700,
        )
        .unwrap();
        let shown = state(dir.path(), Family::Grok, 800).unwrap();
        assert_eq!(shown.limit_hit_at, Some(700));
        assert_eq!(shown.limit_hit_resets_at, Some(FAR));
        assert_eq!(shown.five_hour, WindowView::NotExposed);
    }

    #[test]
    fn a_damaged_or_foreign_snapshot_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join(LIMITS_DIR);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("claude.json"), "{").unwrap();
        assert!(load(dir.path(), Family::Claude).is_err());
        std::fs::write(
            folder.join("claude.json"),
            r#"{"schema_version":1,"family":"codex","source":"x","recorded_at":1}"#,
        )
        .unwrap();
        assert!(load(dir.path(), Family::Claude).is_err());
    }
}
