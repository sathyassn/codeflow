//! The coordination ledger (ADR-0086): the clone-local working state of the
//! sessions that coordinate work, kept under the common git directory at
//! `codeflow/coord/`.
//!
//! The ledger is an aid, never a control. Nothing in records, pull request
//! bodies, CI, git hooks or landing reads it, and it never writes a record:
//! it reads task, epic and ADR state through the workgraph readers and keeps
//! its own events in one append-only log.
//!
//! - [`actor`]: who acts and how an actor is bound to its role.
//! - [`reference`](mod@reference): typed links to tasks, records, pull requests, commits,
//!   branches, follow-ups, lanes and times.
//! - [`schema`]: the event envelope and the per-event validation.
//! - [`transition`]: the fold of the log into state, the transition rules
//!   and the permission matrix, applied on input and on replay alike.
//! - [`store`]: the files, the lock, sequence, sync, idempotency and
//!   tail repair.
//! - [`due`]: due times and target-aware fulfilment.
//! - [`observe`]: the one bounded interface for every outside observation
//!   and heartbeat refresh, with per-observer limits and freshness.
//! - [`text`]: free-text bounds, the secret scan and safe rendering.
//! - [`fixture`]: role binding for tests until the role commands ship.
//!
//! The field-level reference is `docs/coordination-ledger.md`.

pub mod actor;
pub mod due;
pub mod fixture;
pub mod observe;
pub mod reference;
pub mod schema;
pub mod store;
pub mod text;
pub mod transition;

use std::fmt;

/// A refused or failed ledger operation. Each kind maps to its own exit
/// status in `codeflow follow`, so a caller can tell busy from empty and a
/// denied write from a damaged ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordError {
    /// The operation breaks a rule: a permission, a transition, a reference
    /// or a field. Nothing was written.
    Refused(String),
    /// Another process held the lock for longer than the bounded wait.
    Busy,
    /// The log is damaged or written by a newer schema; the ledger is
    /// read-only until it is repaired.
    ReadOnly(String),
    /// The ledger could not be written (a denied write, a full disk).
    Unwritten(String),
    /// The ledger could not be read (a denied read).
    Unavailable(String),
    /// The repository could not be found or read.
    Repository(String),
    /// An idempotency key was reused for a different operation or actor.
    KeyConflict(String),
}

impl fmt::Display for CoordError {
    /// Every diagnostic passes through [`text::scrub`]: none echoes a
    /// secret-shaped value, whatever parser or log line it came from.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&text::scrub(&self.unscrubbed()))
    }
}

impl CoordError {
    fn unscrubbed(&self) -> String {
        Raw(self).to_string()
    }
}

struct Raw<'a>(&'a CoordError);

impl fmt::Display for Raw<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            CoordError::Refused(reason) => write!(f, "refused: {reason}"),
            CoordError::Busy => f.write_str(
                "busy: another codeflow process holds the coordination ledger lock; try again",
            ),
            CoordError::ReadOnly(reason) => write!(f, "read-only: {reason}"),
            CoordError::Unwritten(reason) => write!(f, "unwritten: {reason}"),
            CoordError::Unavailable(reason) => write!(f, "unavailable: {reason}"),
            CoordError::Repository(reason) => write!(f, "repository: {reason}"),
            CoordError::KeyConflict(reason) => write!(f, "key conflict: {reason}"),
        }
    }
}

impl std::error::Error for CoordError {}

/// The default interval between checks of a follow-up, in seconds.
pub const CHECK_INTERVAL_SECS: u64 = 10 * 60;

/// The default time after which a holder's contact is overdue, in seconds.
pub const HEARTBEAT_TIMEOUT_SECS: u64 = 30 * 60;

/// The longest a follow-up waiting on a prerequisite goes between checks.
pub const MAX_RECHECK_SECS: u64 = 24 * 60 * 60;

/// The current time in seconds since the Unix epoch.
#[must_use]
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Format seconds since the Unix epoch as an RFC 3339 UTC time.
#[must_use]
pub fn format_time(secs: u64) -> String {
    crate::registry::rfc3339_from_epoch_secs(secs)
}

/// Parse an RFC 3339 UTC time of the form `YYYY-MM-DDTHH:MM:SSZ`.
///
/// # Errors
///
/// The reason the text is not such a time.
pub fn parse_time(text: &str) -> Result<u64, String> {
    let bad = || {
        format!(
            "`{}` is not a UTC time of the form YYYY-MM-DDTHH:MM:SSZ",
            text::shown(text)
        )
    };
    let bytes = text.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return Err(bad());
    }
    let number = |range: std::ops::Range<usize>| -> Result<u64, String> {
        let part = &text[range];
        if part.bytes().all(|byte| byte.is_ascii_digit()) {
            part.parse::<u64>().map_err(|_| bad())
        } else {
            Err(bad())
        }
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(bad());
    }
    let days = days_from_civil(year, month, day);
    Ok(days * 86_400 + hour * 3600 + minute * 60 + second)
}

fn days_in_month(year: u64, month: u64) -> u64 {
    match month {
        2 if (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    // Howard Hinnant's days_from_civil, for dates from 1970.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year / 400;
    let year_of_era = year - era * 400;
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Parse a duration such as `90s`, `10m`, `2h` or `1d` into seconds.
///
/// # Errors
///
/// The reason the text is not such a duration.
pub fn parse_duration(text: &str) -> Result<u64, String> {
    let bad = || {
        format!(
            "`{}` is not a duration such as 30m, 2h or 1d",
            text::shown(text)
        )
    };
    let unit = text.chars().last().ok_or_else(bad)?;
    let scale = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        _ => return Err(bad()),
    };
    let number = &text[..text.len() - 1];
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(bad());
    }
    number
        .parse::<u64>()
        .ok()
        .and_then(|value| value.checked_mul(scale))
        .filter(|value| *value > 0)
        .ok_or_else(bad)
}

/// A schedule given as a duration from `now` or as an absolute UTC time.
///
/// # Errors
///
/// The reason the text is neither.
pub fn parse_schedule(text: &str, now: u64) -> Result<u64, String> {
    if text.contains('T') {
        parse_time(text)
    } else {
        Ok(now.saturating_add(parse_duration(text)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_round_trip() {
        for secs in [0, 951_782_400, 1_709_164_800, 1_781_181_045, 4_102_444_799] {
            assert_eq!(parse_time(&format_time(secs)).unwrap(), secs);
        }
        assert!(parse_time("2026-02-30T00:00:00Z").is_err());
        assert!(parse_time("2026-10-23 00:00:00Z").is_err());
        assert!(parse_time("2026-10-23T00:00:00+01:00").is_err());
    }

    #[test]
    fn durations_parse() {
        assert_eq!(parse_duration("90s").unwrap(), 90);
        assert_eq!(parse_duration("10m").unwrap(), 600);
        assert_eq!(parse_duration("2h").unwrap(), 7200);
        assert_eq!(parse_duration("1d").unwrap(), 86_400);
        assert!(parse_duration("0m").is_err());
        assert!(parse_duration("m").is_err());
        assert!(parse_duration("1w").is_err());
        assert_eq!(parse_schedule("1h", 100).unwrap(), 3700);
    }
}
