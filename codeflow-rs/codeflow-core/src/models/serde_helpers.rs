/// Custom serde helpers for `SurrealDB` compatibility.
///
/// `SurrealDB` returns the `id` field as a `RecordId` (table + key compound),
/// but our domain models use `id: String` (just the key part).
/// This module provides a custom deserializer that bridges the two formats.
///
/// **Important:** The `deserialize_record_id` function uses `RecordId::deserialize`
/// which only works with `SurrealDB`'s internal deserializer. Models using this
/// cannot be deserialized from plain JSON via `serde_json::from_str` because
/// `RecordId` does not parse from plain JSON strings. If plain JSON deserialization
/// is needed in the future, use a separate DTO struct without the custom
/// deserializer and convert to the domain model.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use surrealdb::RecordId;

/// Wrapper that deserializes as `RecordId` and converts to `String`.
#[derive(Deserialize)]
struct RecordIdWrapper(RecordId);

/// Deserialize a `SurrealDB` record id as a plain `String`.
///
/// Works with `SurrealDB`'s internal deserializer (select, query, create).
/// Extracts the key part from the `RecordId`, stripping the table prefix
/// and any backtick quoting.
///
/// # Errors
///
/// Returns `D::Error` if the deserializer cannot produce a valid `RecordId`.
pub fn deserialize_record_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let wrapper = RecordIdWrapper::deserialize(deserializer)?;
    let key = wrapper.0.key().to_string();
    // SurrealDB wraps non-numeric string keys in backticks (e.g., `abc123`).
    // Strip them to get the raw key.
    Ok(key
        .strip_prefix('`')
        .and_then(|s| s.strip_suffix('`'))
        .unwrap_or(&key)
        .to_string())
}

/// Serialize an id as a plain `String` (passthrough).
///
/// # Errors
///
/// Returns `S::Error` if the serializer fails to serialize the string.
pub fn serialize_record_id<S>(id: &String, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    id.serialize(serializer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, serde::Serialize, PartialEq)]
    struct TestRecord {
        #[serde(serialize_with = "serialize_record_id")]
        pub id: String,
        pub name: String,
    }

    #[test]
    fn serialize_id_as_plain_string() {
        let record = TestRecord {
            id: "abc123".to_string(),
            name: "test".to_string(),
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains(r#""id":"abc123""#));
    }

    #[test]
    fn strip_backticks_from_key() {
        // Verify the backtick-stripping logic.
        let key = "`mykey`".to_string();
        let stripped = key
            .strip_prefix('`')
            .and_then(|s| s.strip_suffix('`'))
            .unwrap_or(&key)
            .to_string();
        assert_eq!(stripped, "mykey");
    }

    #[test]
    fn no_strip_without_backticks() {
        let key = "mykey".to_string();
        let stripped = key
            .strip_prefix('`')
            .and_then(|s| s.strip_suffix('`'))
            .unwrap_or(&key)
            .to_string();
        assert_eq!(stripped, "mykey");
    }
}
