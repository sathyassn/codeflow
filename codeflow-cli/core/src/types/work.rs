use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::enums::ParseEnumError;

/// Area type classification for project organization.
///
/// Maps to the `area_types.code` column in the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AreaType {
    Inf,
    Pln,
    Doc,
    Tst,
    Sec,
    Frm,
    Prj,
    Aut,
}

impl fmt::Display for AreaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inf => f.write_str("INF"),
            Self::Pln => f.write_str("PLN"),
            Self::Doc => f.write_str("DOC"),
            Self::Tst => f.write_str("TST"),
            Self::Sec => f.write_str("SEC"),
            Self::Frm => f.write_str("FRM"),
            Self::Prj => f.write_str("PRJ"),
            Self::Aut => f.write_str("AUT"),
        }
    }
}

impl FromStr for AreaType {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "INF" | "inf" => Ok(Self::Inf),
            "PLN" | "pln" => Ok(Self::Pln),
            "DOC" | "doc" => Ok(Self::Doc),
            "TST" | "tst" => Ok(Self::Tst),
            "SEC" | "sec" => Ok(Self::Sec),
            "FRM" | "frm" => Ok(Self::Frm),
            "PRJ" | "prj" => Ok(Self::Prj),
            "AUT" | "aut" => Ok(Self::Aut),
            _ => Err(ParseEnumError {
                enum_name: "AreaType",
                value: s.to_string(),
            }),
        }
    }
}

/// Work type classification for task routing.
///
/// Maps to the `work_types.code` column in the schema.
/// 10 variants matching the schema table entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkType {
    Feat,
    Fix,
    Rfct,
    Cicd,
    Docs,
    Test,
    Plan,
    Spke,
    Htfx,
    Chor,
}

impl fmt::Display for WorkType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Feat => f.write_str("FEAT"),
            Self::Fix => f.write_str("FIX"),
            Self::Rfct => f.write_str("RFCT"),
            Self::Cicd => f.write_str("CICD"),
            Self::Docs => f.write_str("DOCS"),
            Self::Test => f.write_str("TEST"),
            Self::Plan => f.write_str("PLAN"),
            Self::Spke => f.write_str("SPKE"),
            Self::Htfx => f.write_str("HTFX"),
            Self::Chor => f.write_str("CHOR"),
        }
    }
}

impl FromStr for WorkType {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "FEAT" | "feat" => Ok(Self::Feat),
            "FIX" | "fix" => Ok(Self::Fix),
            "RFCT" | "rfct" => Ok(Self::Rfct),
            "CICD" | "cicd" => Ok(Self::Cicd),
            "DOCS" | "docs" => Ok(Self::Docs),
            "TEST" | "test" => Ok(Self::Test),
            "PLAN" | "plan" => Ok(Self::Plan),
            "SPKE" | "spke" => Ok(Self::Spke),
            "HTFX" | "htfx" => Ok(Self::Htfx),
            "CHOR" | "chor" => Ok(Self::Chor),
            _ => Err(ParseEnumError {
                enum_name: "WorkType",
                value: s.to_string(),
            }),
        }
    }
}

/// Task status values.
///
/// Aligned with schema CHECK constraint:
/// `CHECK(status IN ('todo', 'blocked', 'in_progress', 'complete', 'cancelled'))`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    Blocked,
    InProgress,
    Complete,
    Cancelled,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Todo => f.write_str("todo"),
            Self::Blocked => f.write_str("blocked"),
            Self::InProgress => f.write_str("in_progress"),
            Self::Complete => f.write_str("complete"),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl FromStr for TaskStatus {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "todo" => Ok(Self::Todo),
            "blocked" => Ok(Self::Blocked),
            "in_progress" => Ok(Self::InProgress),
            "complete" => Ok(Self::Complete),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(ParseEnumError {
                enum_name: "TaskStatus",
                value: s.to_string(),
            }),
        }
    }
}

/// Epic status values.
///
/// Aligned with schema CHECK constraint:
/// `CHECK(status IN ('draft', 'planning', 'in_progress', 'blocked', 'complete', 'archived'))`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpicStatus {
    Draft,
    Planning,
    InProgress,
    Blocked,
    Complete,
    Archived,
}

impl fmt::Display for EpicStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Draft => f.write_str("draft"),
            Self::Planning => f.write_str("planning"),
            Self::InProgress => f.write_str("in_progress"),
            Self::Blocked => f.write_str("blocked"),
            Self::Complete => f.write_str("complete"),
            Self::Archived => f.write_str("archived"),
        }
    }
}

impl FromStr for EpicStatus {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "draft" => Ok(Self::Draft),
            "planning" => Ok(Self::Planning),
            "in_progress" => Ok(Self::InProgress),
            "blocked" => Ok(Self::Blocked),
            "complete" => Ok(Self::Complete),
            "archived" => Ok(Self::Archived),
            _ => Err(ParseEnumError {
                enum_name: "EpicStatus",
                value: s.to_string(),
            }),
        }
    }
}

/// Session status values (internal tracking, no schema CHECK constraint).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Ended,
    Crashed,
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => f.write_str("active"),
            Self::Ended => f.write_str("ended"),
            Self::Crashed => f.write_str("crashed"),
        }
    }
}

impl FromStr for SessionStatus {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "ended" => Ok(Self::Ended),
            "crashed" => Ok(Self::Crashed),
            _ => Err(ParseEnumError {
                enum_name: "SessionStatus",
                value: s.to_string(),
            }),
        }
    }
}

/// Active work status values.
///
/// Aligned with schema CHECK constraint:
/// `CHECK(status IN ('in_progress', 'complete', 'blocked'))`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveWorkStatus {
    InProgress,
    Complete,
    Blocked,
}

impl fmt::Display for ActiveWorkStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InProgress => f.write_str("in_progress"),
            Self::Complete => f.write_str("complete"),
            Self::Blocked => f.write_str("blocked"),
        }
    }
}

impl FromStr for ActiveWorkStatus {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "in_progress" | "active" => Ok(Self::InProgress),
            "complete" => Ok(Self::Complete),
            "blocked" => Ok(Self::Blocked),
            _ => Err(ParseEnumError {
                enum_name: "ActiveWorkStatus",
                value: s.to_string(),
            }),
        }
    }
}

/// Autorun session status values.
///
/// Aligned with schema CHECK constraint:
/// `CHECK(status IN ('pending', 'running', 'paused', 'completed', 'failed', 'cancelled', 'timeout', 'aborting'))`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutorunSessionStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
    Timeout,
    Aborting,
}

impl AutorunSessionStatus {
    /// Returns `true` if the status represents a terminal (final) state.
    ///
    /// Terminal statuses: `Completed`, `Failed`, `Cancelled`, `Timeout`.
    /// Non-terminal statuses: `Pending`, `Running`, `Paused`, `Aborting`.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Timeout
        )
    }
}

impl fmt::Display for AutorunSessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => f.write_str("pending"),
            Self::Running => f.write_str("running"),
            Self::Paused => f.write_str("paused"),
            Self::Completed => f.write_str("completed"),
            Self::Failed => f.write_str("failed"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Timeout => f.write_str("timeout"),
            Self::Aborting => f.write_str("aborting"),
        }
    }
}

impl FromStr for AutorunSessionStatus {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "paused" => Ok(Self::Paused),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "timeout" => Ok(Self::Timeout),
            "aborting" => Ok(Self::Aborting),
            _ => Err(ParseEnumError {
                enum_name: "AutorunSessionStatus",
                value: s.to_string(),
            }),
        }
    }
}

/// Autorun worker status values.
///
/// Aligned with schema CHECK constraint:
/// `CHECK(status IN ('queued', 'starting', 'running', 'completed', 'failed', 'skipped', 'timeout', 'cancelled'))`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutorunWorkerStatus {
    Queued,
    Starting,
    Running,
    Completed,
    Failed,
    Skipped,
    Timeout,
    Cancelled,
}

impl fmt::Display for AutorunWorkerStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Queued => f.write_str("queued"),
            Self::Starting => f.write_str("starting"),
            Self::Running => f.write_str("running"),
            Self::Completed => f.write_str("completed"),
            Self::Failed => f.write_str("failed"),
            Self::Skipped => f.write_str("skipped"),
            Self::Timeout => f.write_str("timeout"),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl FromStr for AutorunWorkerStatus {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "queued" => Ok(Self::Queued),
            "starting" => Ok(Self::Starting),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "skipped" => Ok(Self::Skipped),
            "timeout" => Ok(Self::Timeout),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(ParseEnumError {
                enum_name: "AutorunWorkerStatus",
                value: s.to_string(),
            }),
        }
    }
}

/// Autorun task run status values.
///
/// Aligned with schema CHECK constraint:
/// `CHECK(status IN ('pending', 'running', 'completed', 'failed', 'skipped', 'timeout', 'cancelled'))`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutorunTaskRunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
    Timeout,
    Cancelled,
}

impl fmt::Display for AutorunTaskRunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => f.write_str("pending"),
            Self::Running => f.write_str("running"),
            Self::Completed => f.write_str("completed"),
            Self::Failed => f.write_str("failed"),
            Self::Skipped => f.write_str("skipped"),
            Self::Timeout => f.write_str("timeout"),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl FromStr for AutorunTaskRunStatus {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "skipped" => Ok(Self::Skipped),
            "timeout" => Ok(Self::Timeout),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(ParseEnumError {
                enum_name: "AutorunTaskRunStatus",
                value: s.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- AreaType tests --

    #[test]
    fn test_area_type_display() {
        assert_eq!(AreaType::Inf.to_string(), "INF");
        assert_eq!(AreaType::Aut.to_string(), "AUT");
    }

    #[test]
    fn test_area_type_from_str() {
        assert_eq!("INF".parse::<AreaType>().unwrap(), AreaType::Inf);
        assert_eq!("doc".parse::<AreaType>().unwrap(), AreaType::Doc);
    }

    #[test]
    fn test_area_type_from_str_invalid() {
        assert!("UNKNOWN".parse::<AreaType>().is_err());
    }

    #[test]
    fn test_area_type_serde_roundtrip() {
        let at = AreaType::Sec;
        let json = serde_json::to_string(&at).unwrap();
        assert_eq!(json, "\"SEC\"");
        let parsed: AreaType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, at);
    }

    #[test]
    fn test_area_type_variant_count() {
        let areas = [
            AreaType::Inf,
            AreaType::Pln,
            AreaType::Doc,
            AreaType::Tst,
            AreaType::Sec,
            AreaType::Frm,
            AreaType::Prj,
            AreaType::Aut,
        ];
        assert_eq!(areas.len(), 8);
    }

    // -- WorkType tests --

    #[test]
    fn test_work_type_display() {
        assert_eq!(WorkType::Feat.to_string(), "FEAT");
        assert_eq!(WorkType::Chor.to_string(), "CHOR");
    }

    #[test]
    fn test_work_type_from_str() {
        assert_eq!("FEAT".parse::<WorkType>().unwrap(), WorkType::Feat);
        assert_eq!("fix".parse::<WorkType>().unwrap(), WorkType::Fix);
    }

    #[test]
    fn test_work_type_from_str_invalid() {
        assert!("UNKNOWN".parse::<WorkType>().is_err());
    }

    #[test]
    fn test_work_type_serde_roundtrip() {
        let wt = WorkType::Rfct;
        let json = serde_json::to_string(&wt).unwrap();
        assert_eq!(json, "\"RFCT\"");
        let parsed: WorkType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, wt);
    }

    #[test]
    fn test_work_type_variant_count() {
        let types = [
            WorkType::Feat,
            WorkType::Fix,
            WorkType::Rfct,
            WorkType::Cicd,
            WorkType::Docs,
            WorkType::Test,
            WorkType::Plan,
            WorkType::Spke,
            WorkType::Htfx,
            WorkType::Chor,
        ];
        assert_eq!(types.len(), 10);
    }

    // -- TaskStatus tests --

    #[test]
    fn test_task_status_display() {
        assert_eq!(TaskStatus::Todo.to_string(), "todo");
        assert_eq!(TaskStatus::InProgress.to_string(), "in_progress");
        assert_eq!(TaskStatus::Complete.to_string(), "complete");
    }

    #[test]
    fn test_task_status_from_str() {
        assert_eq!("todo".parse::<TaskStatus>().unwrap(), TaskStatus::Todo);
        assert_eq!(
            "in_progress".parse::<TaskStatus>().unwrap(),
            TaskStatus::InProgress
        );
        assert_eq!(
            "complete".parse::<TaskStatus>().unwrap(),
            TaskStatus::Complete
        );
    }

    #[test]
    fn test_task_status_from_str_invalid() {
        assert!("done".parse::<TaskStatus>().is_err());
        assert!("skipped".parse::<TaskStatus>().is_err());
    }

    #[test]
    fn test_task_status_serde_roundtrip() {
        let ts = TaskStatus::Complete;
        let json = serde_json::to_string(&ts).unwrap();
        assert_eq!(json, "\"complete\"");
        let parsed: TaskStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ts);
    }

    #[test]
    fn test_task_status_schema_alignment() {
        // Verify all schema CHECK constraint values are representable
        let schema_values = ["todo", "blocked", "in_progress", "complete", "cancelled"];
        for val in schema_values {
            assert!(
                val.parse::<TaskStatus>().is_ok(),
                "schema value '{val}' not parseable"
            );
        }
    }

    // -- EpicStatus tests --

    #[test]
    fn test_epic_status_display() {
        assert_eq!(EpicStatus::Draft.to_string(), "draft");
        assert_eq!(EpicStatus::Complete.to_string(), "complete");
        assert_eq!(EpicStatus::Archived.to_string(), "archived");
    }

    #[test]
    fn test_epic_status_from_str() {
        assert_eq!("draft".parse::<EpicStatus>().unwrap(), EpicStatus::Draft);
        assert_eq!(
            "planning".parse::<EpicStatus>().unwrap(),
            EpicStatus::Planning
        );
        assert_eq!(
            "archived".parse::<EpicStatus>().unwrap(),
            EpicStatus::Archived
        );
    }

    #[test]
    fn test_epic_status_from_str_invalid() {
        assert!("todo".parse::<EpicStatus>().is_err());
        assert!("done".parse::<EpicStatus>().is_err());
    }

    #[test]
    fn test_epic_status_serde_roundtrip() {
        let es = EpicStatus::Planning;
        let json = serde_json::to_string(&es).unwrap();
        assert_eq!(json, "\"planning\"");
        let parsed: EpicStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, es);
    }

    #[test]
    fn test_epic_status_schema_alignment() {
        // Verify all schema CHECK constraint values are representable
        let schema_values = [
            "draft",
            "planning",
            "in_progress",
            "blocked",
            "complete",
            "archived",
        ];
        for val in schema_values {
            assert!(
                val.parse::<EpicStatus>().is_ok(),
                "schema value '{val}' not parseable"
            );
        }
    }

    #[test]
    fn test_epic_status_variant_count() {
        let statuses = [
            EpicStatus::Draft,
            EpicStatus::Planning,
            EpicStatus::InProgress,
            EpicStatus::Blocked,
            EpicStatus::Complete,
            EpicStatus::Archived,
        ];
        assert_eq!(statuses.len(), 6);
    }

    // -- SessionStatus tests --

    #[test]
    fn test_session_status_display() {
        assert_eq!(SessionStatus::Active.to_string(), "active");
        assert_eq!(SessionStatus::Ended.to_string(), "ended");
        assert_eq!(SessionStatus::Crashed.to_string(), "crashed");
    }

    #[test]
    fn test_session_status_from_str() {
        assert_eq!(
            "active".parse::<SessionStatus>().unwrap(),
            SessionStatus::Active
        );
        assert_eq!(
            "ended".parse::<SessionStatus>().unwrap(),
            SessionStatus::Ended
        );
    }

    #[test]
    fn test_session_status_from_str_invalid() {
        assert!("unknown".parse::<SessionStatus>().is_err());
    }

    #[test]
    fn test_session_status_serde_roundtrip() {
        let ss = SessionStatus::Active;
        let json = serde_json::to_string(&ss).unwrap();
        assert_eq!(json, "\"active\"");
        let parsed: SessionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ss);
    }

    // -- ActiveWorkStatus tests --

    #[test]
    fn test_active_work_status_display() {
        assert_eq!(ActiveWorkStatus::InProgress.to_string(), "in_progress");
        assert_eq!(ActiveWorkStatus::Complete.to_string(), "complete");
        assert_eq!(ActiveWorkStatus::Blocked.to_string(), "blocked");
    }

    #[test]
    fn test_active_work_status_from_str() {
        assert_eq!(
            "in_progress".parse::<ActiveWorkStatus>().unwrap(),
            ActiveWorkStatus::InProgress
        );
        assert_eq!(
            "complete".parse::<ActiveWorkStatus>().unwrap(),
            ActiveWorkStatus::Complete
        );
        assert_eq!(
            "blocked".parse::<ActiveWorkStatus>().unwrap(),
            ActiveWorkStatus::Blocked
        );
    }

    #[test]
    fn test_active_work_status_from_str_alias() {
        // "active" is accepted as alias for InProgress (backwards compat)
        assert_eq!(
            "active".parse::<ActiveWorkStatus>().unwrap(),
            ActiveWorkStatus::InProgress
        );
    }

    #[test]
    fn test_active_work_status_from_str_invalid() {
        assert!("unknown".parse::<ActiveWorkStatus>().is_err());
        assert!("done".parse::<ActiveWorkStatus>().is_err());
    }

    #[test]
    fn test_active_work_status_serde_roundtrip() {
        let s = ActiveWorkStatus::Blocked;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"blocked\"");
        let parsed: ActiveWorkStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn test_active_work_status_display_fromstr_roundtrip() {
        for status in [
            ActiveWorkStatus::InProgress,
            ActiveWorkStatus::Complete,
            ActiveWorkStatus::Blocked,
        ] {
            let s = status.to_string();
            let parsed: ActiveWorkStatus = s.parse().unwrap();
            assert_eq!(parsed, status);
        }
    }

    #[test]
    fn test_active_work_status_schema_alignment() {
        let schema_values = ["in_progress", "complete", "blocked"];
        for val in schema_values {
            assert!(
                val.parse::<ActiveWorkStatus>().is_ok(),
                "schema value '{val}' not parseable"
            );
        }
    }

    // -- AutorunSessionStatus tests --

    #[test]
    fn test_autorun_session_status_display() {
        assert_eq!(AutorunSessionStatus::Pending.to_string(), "pending");
        assert_eq!(AutorunSessionStatus::Running.to_string(), "running");
        assert_eq!(AutorunSessionStatus::Paused.to_string(), "paused");
        assert_eq!(AutorunSessionStatus::Completed.to_string(), "completed");
        assert_eq!(AutorunSessionStatus::Failed.to_string(), "failed");
        assert_eq!(AutorunSessionStatus::Cancelled.to_string(), "cancelled");
        assert_eq!(AutorunSessionStatus::Timeout.to_string(), "timeout");
    }

    #[test]
    fn test_autorun_session_status_from_str() {
        assert_eq!(
            "pending".parse::<AutorunSessionStatus>().unwrap(),
            AutorunSessionStatus::Pending
        );
        assert_eq!(
            "running".parse::<AutorunSessionStatus>().unwrap(),
            AutorunSessionStatus::Running
        );
        assert_eq!(
            "paused".parse::<AutorunSessionStatus>().unwrap(),
            AutorunSessionStatus::Paused
        );
        assert_eq!(
            "completed".parse::<AutorunSessionStatus>().unwrap(),
            AutorunSessionStatus::Completed
        );
        assert_eq!(
            "failed".parse::<AutorunSessionStatus>().unwrap(),
            AutorunSessionStatus::Failed
        );
        assert_eq!(
            "cancelled".parse::<AutorunSessionStatus>().unwrap(),
            AutorunSessionStatus::Cancelled
        );
        assert_eq!(
            "timeout".parse::<AutorunSessionStatus>().unwrap(),
            AutorunSessionStatus::Timeout
        );
    }

    #[test]
    fn test_autorun_session_status_from_str_invalid() {
        assert!("active".parse::<AutorunSessionStatus>().is_err());
    }

    #[test]
    fn test_autorun_session_status_serde_roundtrip() {
        let s = AutorunSessionStatus::Failed;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"failed\"");
        let parsed: AutorunSessionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn test_autorun_session_status_display_fromstr_roundtrip() {
        for status in [
            AutorunSessionStatus::Pending,
            AutorunSessionStatus::Running,
            AutorunSessionStatus::Paused,
            AutorunSessionStatus::Completed,
            AutorunSessionStatus::Failed,
            AutorunSessionStatus::Cancelled,
            AutorunSessionStatus::Timeout,
        ] {
            let s = status.to_string();
            let parsed: AutorunSessionStatus = s.parse().unwrap();
            assert_eq!(parsed, status);
        }
    }

    // -- AutorunWorkerStatus tests --

    #[test]
    fn test_autorun_worker_status_display() {
        assert_eq!(AutorunWorkerStatus::Queued.to_string(), "queued");
        assert_eq!(AutorunWorkerStatus::Starting.to_string(), "starting");
        assert_eq!(AutorunWorkerStatus::Running.to_string(), "running");
        assert_eq!(AutorunWorkerStatus::Completed.to_string(), "completed");
        assert_eq!(AutorunWorkerStatus::Failed.to_string(), "failed");
        assert_eq!(AutorunWorkerStatus::Skipped.to_string(), "skipped");
        assert_eq!(AutorunWorkerStatus::Timeout.to_string(), "timeout");
        assert_eq!(AutorunWorkerStatus::Cancelled.to_string(), "cancelled");
    }

    #[test]
    fn test_autorun_worker_status_from_str() {
        assert_eq!(
            "queued".parse::<AutorunWorkerStatus>().unwrap(),
            AutorunWorkerStatus::Queued
        );
        assert_eq!(
            "starting".parse::<AutorunWorkerStatus>().unwrap(),
            AutorunWorkerStatus::Starting
        );
        assert_eq!(
            "running".parse::<AutorunWorkerStatus>().unwrap(),
            AutorunWorkerStatus::Running
        );
        assert_eq!(
            "completed".parse::<AutorunWorkerStatus>().unwrap(),
            AutorunWorkerStatus::Completed
        );
        assert_eq!(
            "failed".parse::<AutorunWorkerStatus>().unwrap(),
            AutorunWorkerStatus::Failed
        );
        assert_eq!(
            "skipped".parse::<AutorunWorkerStatus>().unwrap(),
            AutorunWorkerStatus::Skipped
        );
        assert_eq!(
            "timeout".parse::<AutorunWorkerStatus>().unwrap(),
            AutorunWorkerStatus::Timeout
        );
        assert_eq!(
            "cancelled".parse::<AutorunWorkerStatus>().unwrap(),
            AutorunWorkerStatus::Cancelled
        );
    }

    #[test]
    fn test_autorun_worker_status_from_str_invalid() {
        assert!("active".parse::<AutorunWorkerStatus>().is_err());
    }

    #[test]
    fn test_autorun_worker_status_serde_roundtrip() {
        let s = AutorunWorkerStatus::Skipped;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"skipped\"");
        let parsed: AutorunWorkerStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn test_autorun_worker_status_display_fromstr_roundtrip() {
        for status in [
            AutorunWorkerStatus::Queued,
            AutorunWorkerStatus::Starting,
            AutorunWorkerStatus::Running,
            AutorunWorkerStatus::Completed,
            AutorunWorkerStatus::Failed,
            AutorunWorkerStatus::Skipped,
            AutorunWorkerStatus::Timeout,
            AutorunWorkerStatus::Cancelled,
        ] {
            let s = status.to_string();
            let parsed: AutorunWorkerStatus = s.parse().unwrap();
            assert_eq!(parsed, status);
        }
    }

    // -- AutorunTaskRunStatus tests --

    #[test]
    fn test_autorun_task_run_status_display() {
        assert_eq!(AutorunTaskRunStatus::Pending.to_string(), "pending");
        assert_eq!(AutorunTaskRunStatus::Running.to_string(), "running");
        assert_eq!(AutorunTaskRunStatus::Completed.to_string(), "completed");
        assert_eq!(AutorunTaskRunStatus::Failed.to_string(), "failed");
        assert_eq!(AutorunTaskRunStatus::Skipped.to_string(), "skipped");
        assert_eq!(AutorunTaskRunStatus::Timeout.to_string(), "timeout");
        assert_eq!(AutorunTaskRunStatus::Cancelled.to_string(), "cancelled");
    }

    #[test]
    fn test_autorun_task_run_status_from_str() {
        assert_eq!(
            "pending".parse::<AutorunTaskRunStatus>().unwrap(),
            AutorunTaskRunStatus::Pending
        );
        assert_eq!(
            "running".parse::<AutorunTaskRunStatus>().unwrap(),
            AutorunTaskRunStatus::Running
        );
        assert_eq!(
            "completed".parse::<AutorunTaskRunStatus>().unwrap(),
            AutorunTaskRunStatus::Completed
        );
        assert_eq!(
            "failed".parse::<AutorunTaskRunStatus>().unwrap(),
            AutorunTaskRunStatus::Failed
        );
        assert_eq!(
            "skipped".parse::<AutorunTaskRunStatus>().unwrap(),
            AutorunTaskRunStatus::Skipped
        );
        assert_eq!(
            "timeout".parse::<AutorunTaskRunStatus>().unwrap(),
            AutorunTaskRunStatus::Timeout
        );
        assert_eq!(
            "cancelled".parse::<AutorunTaskRunStatus>().unwrap(),
            AutorunTaskRunStatus::Cancelled
        );
    }

    #[test]
    fn test_autorun_task_run_status_from_str_invalid() {
        assert!("active".parse::<AutorunTaskRunStatus>().is_err());
    }

    #[test]
    fn test_autorun_task_run_status_serde_roundtrip() {
        let s = AutorunTaskRunStatus::Timeout;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"timeout\"");
        let parsed: AutorunTaskRunStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn test_autorun_task_run_status_display_fromstr_roundtrip() {
        for status in [
            AutorunTaskRunStatus::Pending,
            AutorunTaskRunStatus::Running,
            AutorunTaskRunStatus::Completed,
            AutorunTaskRunStatus::Failed,
            AutorunTaskRunStatus::Skipped,
            AutorunTaskRunStatus::Timeout,
            AutorunTaskRunStatus::Cancelled,
        ] {
            let s = status.to_string();
            let parsed: AutorunTaskRunStatus = s.parse().unwrap();
            assert_eq!(parsed, status);
        }
    }

    // -- AutorunSessionStatus::is_terminal tests --

    #[test]
    fn test_autorun_session_status_terminal() {
        assert!(AutorunSessionStatus::Completed.is_terminal());
        assert!(AutorunSessionStatus::Failed.is_terminal());
        assert!(AutorunSessionStatus::Cancelled.is_terminal());
        assert!(AutorunSessionStatus::Timeout.is_terminal());
    }

    #[test]
    fn test_autorun_session_status_non_terminal() {
        assert!(!AutorunSessionStatus::Pending.is_terminal());
        assert!(!AutorunSessionStatus::Running.is_terminal());
        assert!(!AutorunSessionStatus::Paused.is_terminal());
        assert!(!AutorunSessionStatus::Aborting.is_terminal());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_area_type() -> impl Strategy<Value = AreaType> {
        prop_oneof![
            Just(AreaType::Inf),
            Just(AreaType::Pln),
            Just(AreaType::Doc),
            Just(AreaType::Tst),
            Just(AreaType::Sec),
            Just(AreaType::Frm),
            Just(AreaType::Prj),
            Just(AreaType::Aut),
        ]
    }

    fn arb_work_type() -> impl Strategy<Value = WorkType> {
        prop_oneof![
            Just(WorkType::Feat),
            Just(WorkType::Fix),
            Just(WorkType::Rfct),
            Just(WorkType::Cicd),
            Just(WorkType::Docs),
            Just(WorkType::Test),
            Just(WorkType::Plan),
            Just(WorkType::Spke),
            Just(WorkType::Htfx),
            Just(WorkType::Chor),
        ]
    }

    fn arb_task_status() -> impl Strategy<Value = TaskStatus> {
        prop_oneof![
            Just(TaskStatus::Todo),
            Just(TaskStatus::Blocked),
            Just(TaskStatus::InProgress),
            Just(TaskStatus::Complete),
            Just(TaskStatus::Cancelled),
        ]
    }

    fn arb_epic_status() -> impl Strategy<Value = EpicStatus> {
        prop_oneof![
            Just(EpicStatus::Draft),
            Just(EpicStatus::Planning),
            Just(EpicStatus::InProgress),
            Just(EpicStatus::Blocked),
            Just(EpicStatus::Complete),
            Just(EpicStatus::Archived),
        ]
    }

    fn arb_session_status() -> impl Strategy<Value = SessionStatus> {
        prop_oneof![
            Just(SessionStatus::Active),
            Just(SessionStatus::Ended),
            Just(SessionStatus::Crashed),
        ]
    }

    proptest! {
        #[test]
        fn area_type_serde_roundtrip(at in arb_area_type()) {
            let json = serde_json::to_string(&at).unwrap();
            let parsed: AreaType = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, at);
        }

        #[test]
        fn work_type_serde_roundtrip(wt in arb_work_type()) {
            let json = serde_json::to_string(&wt).unwrap();
            let parsed: WorkType = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, wt);
        }

        #[test]
        fn task_status_serde_roundtrip(ts in arb_task_status()) {
            let json = serde_json::to_string(&ts).unwrap();
            let parsed: TaskStatus = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, ts);
        }

        #[test]
        fn epic_status_serde_roundtrip(es in arb_epic_status()) {
            let json = serde_json::to_string(&es).unwrap();
            let parsed: EpicStatus = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, es);
        }

        #[test]
        fn session_status_serde_roundtrip(ss in arb_session_status()) {
            let json = serde_json::to_string(&ss).unwrap();
            let parsed: SessionStatus = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, ss);
        }

        #[test]
        fn work_type_display_fromstr_roundtrip(wt in arb_work_type()) {
            let s = wt.to_string();
            let parsed: WorkType = s.parse().unwrap();
            prop_assert_eq!(parsed, wt);
        }

        #[test]
        fn task_status_display_fromstr_roundtrip(ts in arb_task_status()) {
            let s = ts.to_string();
            let parsed: TaskStatus = s.parse().unwrap();
            prop_assert_eq!(parsed, ts);
        }

        #[test]
        fn epic_status_display_fromstr_roundtrip(es in arb_epic_status()) {
            let s = es.to_string();
            let parsed: EpicStatus = s.parse().unwrap();
            prop_assert_eq!(parsed, es);
        }
    }
}
