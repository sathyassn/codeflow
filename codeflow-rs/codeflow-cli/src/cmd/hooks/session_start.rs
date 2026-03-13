//! Session start hook handlers: 3 handlers matching Go CLI.

use anyhow::Result;
use clap::Subcommand;

use crate::helpers;

/// Session start handler subcommands.
#[derive(Debug, Clone, Copy, Subcommand)]
pub enum SessionStartHandler {
    /// Session initialization
    Init,
    /// Load instructions
    Instructions,
    /// Session start logging
    Logging,
}

fn now() -> String {
    utc_now_iso8601()
}

/// ISO 8601 UTC timestamp without external dependencies.
///
/// Produces format: `2026-03-10T12:34:56Z` matching the core library's
/// expected timestamp format (see `fixed_now()` in core test fixtures).
fn utc_now_iso8601() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = dur.as_secs();

    let days = total_secs / 86_400;
    let day_secs = total_secs % 86_400;
    let hours = day_secs / 3_600;
    let minutes = (day_secs % 3_600) / 60;
    let seconds = day_secs % 60;

    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
///
/// Uses Howard Hinnant's `civil_from_days` algorithm.
pub(super) fn days_to_ymd(days_since_epoch: u64) -> (u64, u64, u64) {
    let z = days_since_epoch + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

pub fn run(handler: SessionStartHandler) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    let h = build_handler(handler, project_dir);
    helpers::run_hook_handler(h.as_ref());
}

/// Build the concrete handler for the given variant.
fn build_handler(
    handler: SessionStartHandler,
    project_dir: std::path::PathBuf,
) -> Box<dyn codeflow_core::HookHandler> {
    match handler {
        SessionStartHandler::Init => {
            let ppid = codeflow_core::hooks::get_claude_pid();
            let home_dir = dirs_home();
            Box::new(codeflow_core::hooks::session_start::SessionStartInit {
                process_checker: codeflow_core::hooks::OsProcessChecker,
                tmux_checker: codeflow_core::hooks::OsTmuxChecker,
                ppid,
                home_dir,
                now,
            })
        }
        SessionStartHandler::Instructions => {
            Box::new(codeflow_core::hooks::session_start::SessionStartInstructions)
        }
        SessionStartHandler::Logging => Box::new(
            codeflow_core::hooks::logging::SessionStartLogging::new(project_dir),
        ),
    }
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var("HOME").map_or_else(
        |_| std::path::PathBuf::from("/tmp"),
        std::path::PathBuf::from,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dirs_home_returns_nonempty_path() {
        let home = dirs_home();
        // dirs_home reads $HOME; it should return a real directory path.
        assert!(
            !home.to_string_lossy().is_empty(),
            "dirs_home should return a non-empty path"
        );
    }

    #[test]
    fn test_utc_now_iso8601_format() {
        let ts = utc_now_iso8601();
        assert!(!ts.is_empty());
        assert!(ts.ends_with('Z'), "timestamp should end with Z: {ts}");
        assert_eq!(ts.len(), 20, "timestamp should be 20 chars: {ts}");
        assert_eq!(&ts[4..5], "-", "should have dash at pos 4: {ts}");
        assert_eq!(&ts[7..8], "-", "should have dash at pos 7: {ts}");
        assert_eq!(&ts[10..11], "T", "should have T at pos 10: {ts}");
        assert_eq!(&ts[13..14], ":", "should have colon at pos 13: {ts}");
        assert_eq!(&ts[16..17], ":", "should have colon at pos 16: {ts}");
    }

    #[test]
    fn test_days_to_ymd_epoch() {
        let (y, m, d) = days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_ymd_known_date() {
        // 2026-03-10 is 20_522 days from epoch
        let (y, m, d) = days_to_ymd(20_522);
        assert_eq!((y, m, d), (2026, 3, 10));
    }

    #[test]
    fn test_days_to_ymd_leap_year() {
        // 2024-02-29 is a leap year day (day 19_782)
        let (y, m, d) = days_to_ymd(19_782);
        assert_eq!((y, m, d), (2024, 2, 29));
    }

    #[test]
    fn test_days_to_ymd_year_boundary() {
        // 2025-01-01 is day 20_089
        let (y, m, d) = days_to_ymd(20_089);
        assert_eq!((y, m, d), (2025, 1, 1));
    }

    #[test]
    fn test_now_delegates_to_utc() {
        let ts = now();
        assert_eq!(ts.len(), 20);
        assert!(ts.ends_with('Z'));
    }

    #[test]
    fn test_build_handler_init_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(SessionStartHandler::Init, dir.path().to_path_buf());
        assert_eq!(h.name(), "session-start-init");
    }

    #[test]
    fn test_build_handler_instructions_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(SessionStartHandler::Instructions, dir.path().to_path_buf());
        assert_eq!(h.name(), "session-start-instructions");
    }

    #[test]
    fn test_build_handler_logging_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(SessionStartHandler::Logging, dir.path().to_path_buf());
        assert_eq!(h.name(), "session-start-logging");
    }

    #[test]
    fn test_build_handler_init_events() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(SessionStartHandler::Init, dir.path().to_path_buf());
        assert!(
            h.events().contains(&codeflow_core::HookEvent::SessionStart),
            "init handler should handle SessionStart events"
        );
    }

    #[test]
    fn test_build_handler_instructions_handle() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(SessionStartHandler::Instructions, dir.path().to_path_buf());
        let input = codeflow_core::HookInput {
            tool_name: None,
            tool_input: None,
            event: codeflow_core::HookEvent::SessionStart,
            session_id: Some("ses-testsessstartinstruct1".into()),
            project_dir: Some(dir.path().to_string_lossy().into()),
            source: None,
            transcript_path: None,
        };
        let result = h.handle(input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_handler_logging_handle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".state").join("logs")).unwrap();
        let h = build_handler(SessionStartHandler::Logging, dir.path().to_path_buf());
        let input = codeflow_core::HookInput {
            tool_name: None,
            tool_input: None,
            event: codeflow_core::HookEvent::SessionStart,
            session_id: Some("ses-testsessstartlogging1".into()),
            project_dir: Some(dir.path().to_string_lossy().into()),
            source: None,
            transcript_path: None,
        };
        let result = h.handle(input);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), codeflow_core::HookOutput::Allow));
    }

    // ─── proptest: days_to_ymd monotonicity and calendar bounds ─────────────

    use proptest::prelude::*;

    proptest! {
        /// `days_to_ymd` must return a valid Gregorian calendar triple
        /// for any plausible Unix-epoch day count (1970–2200 range).
        #[test]
        fn proptest_days_to_ymd_valid_calendar_triple(
            days in 0u64..84_000u64,  // 1970-01-01 to roughly 2199
        ) {
            let (year, month, day) = days_to_ymd(days);
            prop_assert!(year >= 1970, "year should be >= 1970, got {year}");
            prop_assert!((1..=12).contains(&month), "month should be 1–12, got {month}");
            prop_assert!((1..=31).contains(&day), "day should be 1–31, got {day}");
        }

        /// Two consecutive day values must produce equal or increasing (year, month, day) tuples.
        #[test]
        fn proptest_days_to_ymd_monotone(
            days in 0u64..83_999u64,
        ) {
            let (y1, m1, d1) = days_to_ymd(days);
            let (y2, m2, d2) = days_to_ymd(days + 1);
            // The next day must be >= the current day in lexicographic order.
            let current = (y1, m1, d1);
            let next = (y2, m2, d2);
            prop_assert!(next >= current,
                "days_to_ymd({}) = {:?} should be <= days_to_ymd({}) = {:?}",
                days, current, days + 1, next);
        }

        /// The ISO 8601 timestamp produced from any plausible day must have
        /// exactly the format `YYYY-MM-DDTHH:MM:SSZ` (length 20, correct separators).
        #[test]
        fn proptest_utc_now_iso8601_format_invariant(
            // We can't control the clock, but we can verify the format holds
            // by constructing an equivalent string from known days.
            days in 0u64..84_000u64,
        ) {
            let (year, month, day) = days_to_ymd(days);
            let ts = format!("{year:04}-{month:02}-{day:02}T00:00:00Z");
            prop_assert_eq!(ts.len(), 20, "timestamp length must be 20");
            prop_assert_eq!(&ts[4..5], "-");
            prop_assert_eq!(&ts[7..8], "-");
            prop_assert_eq!(&ts[10..11], "T");
            prop_assert!(ts.ends_with('Z'));
        }
    }
}
