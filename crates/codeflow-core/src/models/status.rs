//! Status enums for epics and tasks.
//!
//! Small local enums replacing the v1 `types/` module (which is not
//! imported into v2).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Error for failed status parsing from string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseStatusError {
    pub kind: &'static str,
    pub value: String,
}

impl fmt::Display for ParseStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown {}: '{}'", self.kind, self.value)
    }
}

impl std::error::Error for ParseStatusError {}

/// Task status values.
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
    type Err = ParseStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "todo" => Ok(Self::Todo),
            "blocked" => Ok(Self::Blocked),
            "in_progress" => Ok(Self::InProgress),
            "complete" => Ok(Self::Complete),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(ParseStatusError {
                kind: "task status",
                value: s.to_string(),
            }),
        }
    }
}

/// Epic status values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpicStatus {
    Draft,
    Planning,
    InProgress,
    Blocked,
    Complete,
    Cancelled,
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
            Self::Cancelled => f.write_str("cancelled"),
            Self::Archived => f.write_str("archived"),
        }
    }
}

impl FromStr for EpicStatus {
    type Err = ParseStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "draft" => Ok(Self::Draft),
            "planning" => Ok(Self::Planning),
            "in_progress" => Ok(Self::InProgress),
            "blocked" => Ok(Self::Blocked),
            "complete" => Ok(Self::Complete),
            "cancelled" => Ok(Self::Cancelled),
            "archived" => Ok(Self::Archived),
            _ => Err(ParseStatusError {
                kind: "epic status",
                value: s.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_status_display() {
        assert_eq!(TaskStatus::Todo.to_string(), "todo");
        assert_eq!(TaskStatus::Blocked.to_string(), "blocked");
        assert_eq!(TaskStatus::InProgress.to_string(), "in_progress");
        assert_eq!(TaskStatus::Complete.to_string(), "complete");
        assert_eq!(TaskStatus::Cancelled.to_string(), "cancelled");
    }

    #[test]
    fn test_task_status_from_str_roundtrip() {
        for s in [
            TaskStatus::Todo,
            TaskStatus::Blocked,
            TaskStatus::InProgress,
            TaskStatus::Complete,
            TaskStatus::Cancelled,
        ] {
            let parsed: TaskStatus = s.to_string().parse().unwrap();
            assert_eq!(parsed, s);
        }
    }

    #[test]
    fn test_task_status_from_str_invalid() {
        let err = "bogus".parse::<TaskStatus>().unwrap_err();
        assert!(err.to_string().contains("bogus"));
    }

    #[test]
    fn test_task_status_serde() {
        let json = serde_json::to_string(&TaskStatus::InProgress).unwrap();
        assert_eq!(json, "\"in_progress\"");
        let parsed: TaskStatus = serde_json::from_str("\"todo\"").unwrap();
        assert_eq!(parsed, TaskStatus::Todo);
    }

    #[test]
    fn test_epic_status_display() {
        assert_eq!(EpicStatus::Draft.to_string(), "draft");
        assert_eq!(EpicStatus::Planning.to_string(), "planning");
        assert_eq!(EpicStatus::InProgress.to_string(), "in_progress");
        assert_eq!(EpicStatus::Blocked.to_string(), "blocked");
        assert_eq!(EpicStatus::Complete.to_string(), "complete");
        assert_eq!(EpicStatus::Archived.to_string(), "archived");
    }

    #[test]
    fn test_epic_status_from_str_roundtrip() {
        for s in [
            EpicStatus::Draft,
            EpicStatus::Planning,
            EpicStatus::InProgress,
            EpicStatus::Blocked,
            EpicStatus::Complete,
            EpicStatus::Archived,
        ] {
            let parsed: EpicStatus = s.to_string().parse().unwrap();
            assert_eq!(parsed, s);
        }
    }

    #[test]
    fn test_epic_status_from_str_invalid() {
        let err = "BAD".parse::<EpicStatus>().unwrap_err();
        assert!(err.to_string().contains("BAD"));
    }

    #[test]
    fn test_epic_status_serde() {
        let json = serde_json::to_string(&EpicStatus::Archived).unwrap();
        assert_eq!(json, "\"archived\"");
        let parsed: EpicStatus = serde_json::from_str("\"draft\"").unwrap();
        assert_eq!(parsed, EpicStatus::Draft);
    }
}
