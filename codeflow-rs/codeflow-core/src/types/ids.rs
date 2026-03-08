use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Error returned when parsing an ID string fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseIdError {
    pub kind: &'static str,
    pub value: String,
}

impl fmt::Display for ParseIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid {}: '{}'", self.kind, self.value)
    }
}

impl std::error::Error for ParseIdError {}

/// Macro to generate newtype ID wrappers with standard trait implementations.
macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Create a new ID from a string value.
            ///
            /// # Errors
            ///
            /// Returns `ParseIdError` if the value is empty.
            pub fn new(value: impl Into<String>) -> Result<Self, ParseIdError> {
                let s = value.into();
                if s.is_empty() {
                    return Err(ParseIdError {
                        kind: $kind,
                        value: s,
                    });
                }
                Ok(Self(s))
            }

            /// Create a new ID without validation. Use when the value is known-good.
            #[must_use]
            pub fn new_unchecked(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Return the inner string value.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume self and return the inner string.
            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = ParseIdError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::new(s)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

define_id!(
    /// A unique session identifier (e.g., `ses-177137202131769e89b2d5688`).
    SessionId,
    "session id"
);

define_id!(
    /// A unique task identifier (e.g., `task-01KHSQPQSF26W0SQE9GZPM8XDE`).
    TaskId,
    "task id"
);

define_id!(
    /// A unique epic identifier (e.g., `epic-01KHSQPQRNQP0XTXCRHXX9YW1T`).
    EpicId,
    "epic id"
);

define_id!(
    /// A unique work identifier (e.g., `work-019c69be9ae779adedfbfb8ca9980534`).
    WorkId,
    "work id"
);

define_id!(
    /// A git branch name (e.g., `feat/rust-core-types`).
    BranchName,
    "branch name"
);

define_id!(
    /// A human-readable format identifier (e.g., `INF-TSK-022-006`).
    FormatId,
    "format id"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_id_display() {
        let id = SessionId::new("ses-123abc").unwrap();
        assert_eq!(id.to_string(), "ses-123abc");
    }

    #[test]
    fn test_session_id_from_str() {
        let id: SessionId = "ses-456".parse().unwrap();
        assert_eq!(id.as_str(), "ses-456");
    }

    #[test]
    fn test_empty_id_rejected() {
        assert!(SessionId::new("").is_err());
        assert!(TaskId::new("").is_err());
        assert!(EpicId::new("").is_err());
        assert!(WorkId::new("").is_err());
        assert!(BranchName::new("").is_err());
        assert!(FormatId::new("").is_err());
    }

    #[test]
    fn test_id_serde_roundtrip() {
        let id = TaskId::new("task-01ABC").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"task-01ABC\"");
        let parsed: TaskId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn test_id_as_ref() {
        let id = EpicId::new("epic-xyz").unwrap();
        let s: &str = id.as_ref();
        assert_eq!(s, "epic-xyz");
    }

    #[test]
    fn test_id_into_inner() {
        let id = WorkId::new("work-123").unwrap();
        let inner = id.into_inner();
        assert_eq!(inner, "work-123");
    }

    #[test]
    fn test_branch_name_display() {
        let b = BranchName::new("feat/rust-core-types").unwrap();
        assert_eq!(b.to_string(), "feat/rust-core-types");
    }

    #[test]
    fn test_format_id_display() {
        let f = FormatId::new("INF-TSK-022-006").unwrap();
        assert_eq!(f.to_string(), "INF-TSK-022-006");
    }

    #[test]
    fn test_id_hash_equality() {
        use std::collections::HashSet;
        let id1 = SessionId::new("ses-1").unwrap();
        let id2 = SessionId::new("ses-1").unwrap();
        let id3 = SessionId::new("ses-2").unwrap();
        let mut set = HashSet::new();
        set.insert(id1.clone());
        assert!(set.contains(&id2));
        assert!(!set.contains(&id3));
    }

    #[test]
    fn test_parse_id_error_display() {
        let err = ParseIdError {
            kind: "session id",
            value: String::new(),
        };
        assert_eq!(err.to_string(), "invalid session id: ''");
    }

    #[test]
    fn test_new_unchecked() {
        let id = SessionId::new_unchecked("anything");
        assert_eq!(id.as_str(), "anything");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn session_id_serde_roundtrip(s in "[a-z0-9-]{1,50}") {
            let id = SessionId::new_unchecked(&s);
            let json = serde_json::to_string(&id).unwrap();
            let parsed: SessionId = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, id);
        }

        #[test]
        fn task_id_serde_roundtrip(s in "[a-z0-9-]{1,50}") {
            let id = TaskId::new_unchecked(&s);
            let json = serde_json::to_string(&id).unwrap();
            let parsed: TaskId = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, id);
        }

        #[test]
        fn epic_id_serde_roundtrip(s in "[a-z0-9-]{1,50}") {
            let id = EpicId::new_unchecked(&s);
            let json = serde_json::to_string(&id).unwrap();
            let parsed: EpicId = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, id);
        }

        #[test]
        fn work_id_serde_roundtrip(s in "[a-z0-9-]{1,50}") {
            let id = WorkId::new_unchecked(&s);
            let json = serde_json::to_string(&id).unwrap();
            let parsed: WorkId = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, id);
        }

        #[test]
        fn branch_name_serde_roundtrip(s in "[a-z0-9/.-]{1,50}") {
            let id = BranchName::new_unchecked(&s);
            let json = serde_json::to_string(&id).unwrap();
            let parsed: BranchName = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, id);
        }

        #[test]
        fn format_id_serde_roundtrip(s in "[A-Z0-9-]{1,30}") {
            let id = FormatId::new_unchecked(&s);
            let json = serde_json::to_string(&id).unwrap();
            let parsed: FormatId = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(parsed, id);
        }
    }
}
