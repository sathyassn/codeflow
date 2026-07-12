//! Epic record model.
//!
//! v2 trim: dual-purpose v1 fields tied to dead subsystems (area types,
//! pathflow domains, file-scope claims, external task-tracker mirroring)
//! are removed. Markdown + JSONL are the source of truth (charter D17).

use serde::{Deserialize, Deserializer, Serialize};

use super::status::EpicStatus;

#[derive(Debug, Clone, Serialize)]
pub struct Epic {
    /// ULID-based internal id (e.g., `epic-01abc...`).
    pub id: String,
    /// Human-readable format id (e.g., `EPC-001`).
    pub format_id: String,
    pub title: String,
    pub summary: Option<String>,
    pub status: EpicStatus,
    /// Work intent label from the v2 branch-prefix set (e.g., `feat`).
    pub work_type: String,
    pub priority: String,
    pub pr_number: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// Deserialization tolerates the canonical hand-authored planning
/// frontmatter (the pm-template/validator shape): `created` is accepted as
/// an alias of `created_at`, and a missing `updated_at` falls back to the
/// creation timestamp. Serialization stays canonical (`created_at` +
/// `updated_at`), so store rewrites normalize the record. Fields the
/// docs-lint owns (capabilities/adrs/specs) are ignored here by design.
impl<'de> Deserialize<'de> for Epic {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            id: String,
            format_id: String,
            title: String,
            #[serde(default)]
            summary: Option<String>,
            status: EpicStatus,
            work_type: String,
            // Trimmed from the scaffold template; default keeps epics that omit
            // it parseable (else the store skips them and `status` undercounts).
            #[serde(default)]
            priority: String,
            #[serde(default)]
            pr_number: Option<i64>,
            #[serde(alias = "created")]
            created_at: String,
            #[serde(default)]
            updated_at: Option<String>,
        }

        let w = Wire::deserialize(deserializer)?;
        let updated_at = w.updated_at.unwrap_or_else(|| w.created_at.clone());
        Ok(Epic {
            id: w.id,
            format_id: w.format_id,
            title: w.title,
            summary: w.summary,
            status: w.status,
            work_type: w.work_type,
            priority: w.priority,
            pr_number: w.pr_number,
            created_at: w.created_at,
            updated_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_epic() -> Epic {
        Epic {
            id: "epic-01abc123".to_string(),
            format_id: "EPC-001".to_string(),
            title: "Test Epic".to_string(),
            summary: Some("A test epic".to_string()),
            status: EpicStatus::Draft,
            work_type: "feat".to_string(),
            priority: "high".to_string(),
            pr_number: None,
            created_at: "2026-03-07T00:00:00Z".to_string(),
            updated_at: "2026-03-07T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_epic_serialize_id_as_plain_string() {
        let epic = make_epic();
        let json = serde_json::to_string(&epic).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["id"], "epic-01abc123");
    }

    #[test]
    fn test_epic_serialize_status() {
        let epic = make_epic();
        let json = serde_json::to_string(&epic).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "draft");
    }

    #[test]
    fn test_epic_serialize_optional_summary() {
        let mut epic = make_epic();
        epic.summary = None;
        let json = serde_json::to_string(&epic).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["summary"].is_null());
    }

    #[test]
    fn test_epic_serialize_pr_number_none() {
        let epic = make_epic();
        let json = serde_json::to_string(&epic).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["pr_number"].is_null());
    }

    #[test]
    fn test_epic_serialize_pr_number_some() {
        let mut epic = make_epic();
        epic.pr_number = Some(42);
        let json = serde_json::to_string(&epic).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["pr_number"], 42);
    }

    #[test]
    fn test_epic_json_roundtrip() {
        let epic = make_epic();
        let json = serde_json::to_string(&epic).unwrap();
        let back: Epic = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, epic.id);
        assert_eq!(back.format_id, epic.format_id);
        assert_eq!(back.status, epic.status);
        assert_eq!(back.work_type, epic.work_type);
    }

    #[test]
    fn test_epic_clone() {
        let epic = make_epic();
        let clone = epic.clone();
        assert_eq!(epic.id, clone.id);
        assert_eq!(epic.title, clone.title);
        assert_eq!(epic.status, clone.status);
    }

    #[test]
    fn test_epic_debug_contains_title() {
        let epic = make_epic();
        let debug = format!("{epic:?}");
        assert!(debug.contains("Test Epic"));
    }
}

#[cfg(test)]
mod planning_frontmatter_tests {
    use super::*;

    /// The literal EPC-001 frontmatter shape (hand-authored planning doc):
    /// `created` instead of `created_at`, no `updated_at`, plus doc-graph
    /// fields (capabilities/adrs/specs) the model deliberately ignores.
    #[test]
    fn test_epic_parses_canonical_planning_frontmatter() {
        let yaml = r"
id: EPC-001
format_id: EPC-001
title: v2 bootstrap
status: complete
work_type: feat
priority: high
capabilities: [CAP-001, CAP-002]
adrs: [ADR-0001, ADR-0002]
specs: []
created: 2026-06-11
";
        let epic: Epic = serde_yaml::from_str(yaml).expect("planning frontmatter must parse");
        assert_eq!(epic.format_id, "EPC-001");
        assert_eq!(epic.created_at, "2026-06-11");
        assert_eq!(
            epic.updated_at, "2026-06-11",
            "updated_at falls back to created"
        );
        assert_eq!(epic.status, EpicStatus::Complete);
    }

    /// The canonical store shape still round-trips unchanged.
    #[test]
    fn test_epic_canonical_shape_round_trips() {
        let yaml = r"
id: epic-01abc123
format_id: EPC-002
title: canonical
status: draft
work_type: feat
priority: normal
created_at: 2026-07-01T00:00:00Z
updated_at: 2026-07-02T00:00:00Z
";
        let epic: Epic = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(epic.updated_at, "2026-07-02T00:00:00Z");
        let out = serde_yaml::to_string(&epic).unwrap();
        assert!(out.contains("created_at:"), "serialization stays canonical");
        assert!(out.contains("updated_at:"));
    }

    /// An epic authored from the trimmed template omits `priority`; it must
    /// still parse (via serde default) so the store does not silently skip it
    /// and `status` undercount.
    #[test]
    fn test_epic_parses_without_priority() {
        let yaml = r"
id: EPC-003
format_id: EPC-003
title: trimmed
status: draft
work_type: feat
created: 2026-07-05
";
        let epic: Epic = serde_yaml::from_str(yaml).expect("trimmed epic must parse");
        assert_eq!(epic.format_id, "EPC-003");
        assert_eq!(epic.priority, "");
    }
}
