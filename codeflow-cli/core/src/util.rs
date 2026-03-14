//! Shared utility functions.

use chrono::{DateTime, Utc};

/// Parse an RFC 3339 timestamp string into a `DateTime<Utc>`.
///
/// This is the single, canonical timestamp parser for the codebase,
/// addressing DRY violation 3.6 from the Go audit.
///
/// # Errors
///
/// Returns an error if the string is not valid RFC 3339.
pub fn parse_timestamp(s: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    s.parse::<DateTime<Utc>>()
}

/// Return the current UTC time as an RFC 3339 string.
#[must_use]
pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_rfc3339() {
        let ts = "2026-03-07T00:00:00Z";
        let dt = parse_timestamp(ts).unwrap();
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 3);
        assert_eq!(dt.day(), 7);
    }

    use chrono::Datelike;

    #[test]
    fn test_parse_with_offset() {
        let ts = "2026-03-07T12:30:00+05:30";
        let dt = parse_timestamp(ts).unwrap();
        // Converts to UTC: 12:30 - 5:30 = 07:00 UTC
        assert_eq!(dt.hour(), 7);
        assert_eq!(dt.minute(), 0);
    }

    use chrono::Timelike;

    #[test]
    fn test_parse_invalid_timestamp() {
        let result = parse_timestamp("not-a-timestamp");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_string() {
        let result = parse_timestamp("");
        assert!(result.is_err());
    }

    #[test]
    fn test_now_rfc3339_format() {
        let ts = now_rfc3339();
        // Should be parseable back
        let dt = parse_timestamp(&ts).unwrap();
        assert_eq!(dt.year(), Utc::now().year());
    }

    #[test]
    fn test_parse_preserves_subseconds() {
        let ts = "2026-03-07T12:00:00.123456Z";
        let dt = parse_timestamp(ts).unwrap();
        assert_eq!(dt.nanosecond() / 1_000_000, 123);
    }
}
