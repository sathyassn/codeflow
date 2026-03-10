//! ULID-based ID generation.
//!
//! Provides `new_ulid()` and `new_prefixed_ulid()` for generating unique
//! identifiers. Format ID generation (e.g., `INF-TSK-022-014`) already
//! exists in `workgraph/format_id.rs` and is NOT duplicated here.

use crate::error::IdgenError;

/// Generate a new ULID string.
///
/// Returns a lowercase 26-character Crockford base32 encoded ULID.
///
/// # Errors
///
/// Returns `IdgenError::Generation` if entropy source fails (should not
/// happen on supported platforms).
pub fn new_ulid() -> Result<String, IdgenError> {
    let id = ulid::Ulid::new();
    Ok(id.to_string().to_lowercase())
}

/// Generate a new ULID prefixed with the given string, separated by a hyphen.
///
/// For example, `new_prefixed_ulid("task")` returns `"task-01jm..."`.
/// If prefix is empty, the bare ULID is returned.
///
/// # Errors
///
/// Returns `IdgenError::Generation` if ULID generation fails.
pub fn new_prefixed_ulid(prefix: &str) -> Result<String, IdgenError> {
    let id = new_ulid()?;
    if prefix.is_empty() {
        return Ok(id);
    }
    Ok(format!("{prefix}-{id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_ulid_length() {
        let id = new_ulid().unwrap();
        assert_eq!(id.len(), 26, "ULID should be 26 characters, got: {id}");
    }

    #[test]
    fn test_new_ulid_lowercase() {
        let id = new_ulid().unwrap();
        assert_eq!(id, id.to_lowercase(), "ULID should be lowercase: {id}");
    }

    #[test]
    fn test_new_ulid_uniqueness() {
        let id1 = new_ulid().unwrap();
        let id2 = new_ulid().unwrap();
        assert_ne!(id1, id2, "Two ULIDs should be unique");
    }

    #[test]
    fn test_new_ulid_valid_crockford_base32() {
        let id = new_ulid().unwrap();
        // Crockford base32 lowercase: 0-9 a-z excluding i, l, o, u
        for ch in id.chars() {
            assert!(
                ch.is_ascii_digit()
                    || (ch.is_ascii_lowercase()
                        && ch != 'i'
                        && ch != 'l'
                        && ch != 'o'
                        && ch != 'u'),
                "Invalid Crockford base32 character: {ch} in {id}"
            );
        }
    }

    #[test]
    fn test_prefixed_ulid_with_prefix() {
        let id = new_prefixed_ulid("task").unwrap();
        assert!(id.starts_with("task-"), "Should start with prefix: {id}");
        assert_eq!(
            id.len(),
            26 + 5, // "task-" = 5 chars + 26 char ULID
            "Prefixed ULID wrong length: {id}"
        );
    }

    #[test]
    fn test_prefixed_ulid_empty_prefix() {
        let id = new_prefixed_ulid("").unwrap();
        assert_eq!(id.len(), 26, "Empty prefix should return bare ULID: {id}");
        assert!(!id.contains('-'), "Should not contain hyphen: {id}");
    }

    #[test]
    fn test_prefixed_ulid_various_prefixes() {
        for prefix in &["epic", "ses", "work"] {
            let id = new_prefixed_ulid(prefix).unwrap();
            assert!(
                id.starts_with(&format!("{prefix}-")),
                "Should start with {prefix}-: {id}"
            );
        }
    }
}
