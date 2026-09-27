//! Workgraph record access: reading epic/task state and validating format ids.
//!
//! Persistence flows through the [`RecordStore`] trait (markdown files with
//! YAML frontmatter — charter D17); [`MarkdownStore`] is the production
//! implementation. `codeflow status` and the orient digest read epic/task
//! state through it. Format-id shape validation ([`is_valid_epic_format_id`] /
//! [`is_valid_task_format_id`]) is the single source of truth reused by
//! `validate` for frontmatter checks.

pub mod allocate;
pub mod classify;
pub mod deps;
mod format_id;
pub(crate) mod layout;
pub mod lifecycle;
pub mod light_paths;
pub mod readiness;
pub mod record_text;
pub mod status_verb;
pub mod store;
pub mod work_start;

pub use allocate::{
    create_epic, create_spec, create_task, next_epic_id, next_spec_id, next_task_id, NewRecord,
};
pub use format_id::{
    is_canonical_task_format_id, is_legacy_task_format_id, is_valid_epic_format_id,
    is_valid_spec_format_id, is_valid_task_format_id,
};
pub use store::{MarkdownStore, RecordStore, StoreError};
pub use work_start::{
    branch_claims_task_id, check_epic_line, check_work_start, check_work_start_anchored,
    check_work_start_for_branch, declared_work_target, default_work_target,
    durable_work_tracking_enabled, durable_work_tracking_enabled_at, is_stable_work_target,
    resolve_work_target, resolve_work_target_checked, task_id_from_branch, work_target_resolves,
    DurableTrackingError, RecordKind, ResolvedWorkTarget, WorkStartError, WorkStartReport,
};

/// Generate an RFC 3339 UTC timestamp string.
pub(crate) fn now_rfc3339() -> String {
    rfc3339_at(std::time::SystemTime::now())
}

/// An RFC 3339 UTC timestamp for `time`.
pub(crate) fn rfc3339_at(time: std::time::SystemTime) -> String {
    let duration = time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
/// Algorithm from Howard Hinnant's `civil_from_days`.
fn days_to_ymd(days_since_epoch: u64) -> (u64, u64, u64) {
    let z = days_since_epoch + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_now_rfc3339_format() {
        let ts = super::now_rfc3339();
        // Format: YYYY-MM-DDTHH:MM:SSZ
        assert_eq!(ts.len(), 20);
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
    }

    #[test]
    fn test_days_to_ymd_epoch() {
        let (y, m, d) = super::days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_ymd_known_date() {
        // 2024-01-01 = 19723 days since epoch
        let (y, m, d) = super::days_to_ymd(19723);
        assert_eq!((y, m, d), (2024, 1, 1));
    }
}
