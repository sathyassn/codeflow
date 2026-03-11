use serde::{Deserialize, Serialize};

use super::serde_helpers::{deserialize_record_id, serialize_record_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvent {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    pub event_type: String,
    pub domain: String,
    pub work_id: Option<String>,
    pub data: String,
    pub memory_type: Option<String>,
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_memory_event() -> MemoryEvent {
        MemoryEvent {
            id: "mem-01abc".to_string(),
            event_type: "milestone".to_string(),
            domain: "INF".to_string(),
            work_id: Some("work-01xyz".to_string()),
            data: r#"{"content":"Test milestone reached"}"#.to_string(),
            memory_type: Some("milestone".to_string()),
            created_at: "2026-03-07T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_memory_event_serialize_id() {
        let evt = make_memory_event();
        let json = serde_json::to_string(&evt).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["id"], "mem-01abc");
    }

    #[test]
    fn test_memory_event_serialize_event_type() {
        let evt = make_memory_event();
        let json = serde_json::to_string(&evt).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["event_type"], "milestone");
    }

    #[test]
    fn test_memory_event_serialize_optional_null() {
        let mut evt = make_memory_event();
        evt.work_id = None;
        evt.memory_type = None;
        let json = serde_json::to_string(&evt).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["work_id"].is_null());
        assert!(parsed["memory_type"].is_null());
    }

    #[test]
    fn test_memory_event_clone() {
        let evt = make_memory_event();
        let clone = evt.clone();
        assert_eq!(evt.id, clone.id);
        assert_eq!(evt.event_type, clone.event_type);
    }

    #[test]
    fn test_memory_event_debug() {
        let evt = make_memory_event();
        let debug = format!("{evt:?}");
        assert!(debug.contains("milestone"));
    }
}
