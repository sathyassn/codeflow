//! Shared, closed adoption schemas. V1 is normalized only in memory until an
//! explicitly successful migration or transfer publishes V2.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{deserialize_portal_files, path_text, validate_portal_root, MAX_BUNDLE_FILES};
use crate::strict_json::parse_strict_json;

pub(crate) const MANAGED_GENERATOR: &str = "@codeflow/docs-portal";
pub(crate) const MAX_IDENTITY_BYTES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Generator {
    pub(crate) name: String,
    pub(crate) version: String,
}

impl Generator {
    pub(crate) fn managed(version: &str) -> Self {
        Self {
            name: MANAGED_GENERATOR.into(),
            version: version.into(),
        }
    }

    pub(crate) fn is_valid(&self) -> bool {
        [&self.name, &self.version]
            .iter()
            .all(|value| !value.trim().is_empty() && value.len() <= MAX_IDENTITY_BYTES)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RuntimeOwnership {
    Managed,
    Transferred,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PortalState {
    pub(crate) schema_version: u32,
    pub(crate) root: String,
    pub(crate) starter_version: String,
    pub(crate) runtime_ownership: RuntimeOwnership,
    pub(crate) generator: Generator,
    #[serde(deserialize_with = "deserialize_portal_files")]
    pub(crate) files: BTreeMap<String, PortalFileState>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PortalFileState {
    pub(crate) ownership: String,
    pub(crate) pristine_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyState {
    schema_version: u32,
    root: String,
    starter_version: String,
    #[serde(deserialize_with = "deserialize_portal_files")]
    files: BTreeMap<String, PortalFileState>,
}

/// Extract only the root needed to authenticate recovery, without interpreting
/// adoption schema fields. Duplicate keys at every depth still fail.
pub(crate) fn recovery_root(bytes: &[u8]) -> Result<String, String> {
    check_size(bytes)?;
    let value: serde_json::Value =
        parse_strict_json(bytes).map_err(|_| "invalid portal adoption JSON".to_string())?;
    let root = value
        .get("root")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "adoption state has no string root".to_string())?;
    let normalized = validate_portal_root(Path::new(root)).map_err(|e| e.to_string())?;
    if path_text(&normalized) != root {
        return Err("adoption root is not canonical".into());
    }
    Ok(root.into())
}

pub(crate) fn parse(bytes: &[u8]) -> Result<PortalState, String> {
    check_size(bytes)?;
    let value: serde_json::Value =
        parse_strict_json(bytes).map_err(|_| "invalid portal adoption JSON".to_string())?;
    let state = match value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
    {
        Some(1) => {
            let legacy: LegacyState = serde_json::from_value(value)
                .map_err(|_| "invalid portal adoption schema fields".to_string())?;
            PortalState {
                schema_version: legacy.schema_version,
                root: legacy.root,
                generator: Generator::managed(&legacy.starter_version),
                starter_version: legacy.starter_version,
                runtime_ownership: RuntimeOwnership::Managed,
                files: legacy.files,
            }
        }
        Some(2) => serde_json::from_value(value)
            .map_err(|_| "invalid portal adoption schema fields".to_string())?,
        _ => return Err("unsupported portal adoption schema_version".into()),
    };
    validate(&state)?;
    Ok(state)
}

pub(crate) fn validate(state: &PortalState) -> Result<(), String> {
    let root = validate_portal_root(Path::new(&state.root)).map_err(|e| e.to_string())?;
    if !matches!(state.schema_version, 1 | 2)
        || path_text(&root) != state.root
        || state.starter_version.trim().is_empty()
        || state.starter_version.len() > MAX_IDENTITY_BYTES
        || state.files.is_empty()
        || state.files.len() > MAX_BUNDLE_FILES
        || !state.generator.is_valid()
    {
        return Err("invalid portal adoption root, version, identity or file count".into());
    }
    if state.runtime_ownership == RuntimeOwnership::Managed
        && (state.generator.name != MANAGED_GENERATOR
            || state.generator.version != state.starter_version)
    {
        return Err("managed generator identity does not match the adopted release".into());
    }
    let mut paths = std::collections::BTreeSet::new();
    for (path, metadata) in &state.files {
        super::validate_asset_path(path).map_err(|_| "unsafe portal file path".to_string())?;
        if !paths.insert(super::portable_key(path))
            || !matches!(metadata.ownership.as_str(), "managed" | "user-owned")
            || !super::valid_lower_sha256(&metadata.pristine_sha256)
        {
            return Err("invalid or duplicate portal file metadata".into());
        }
    }
    Ok(())
}

fn check_size(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > super::MAX_STATE_BYTES {
        Err("portal adoption state exceeds its byte limit".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy() -> serde_json::Value {
        serde_json::json!({"schema_version":1,"root":"guide","starter_version":"1.0.0",
            "files":{"runtime":{"ownership":"managed","pristine_sha256":"a".repeat(64)}}})
    }

    fn parse_value(value: &serde_json::Value) -> Result<PortalState, String> {
        parse(&serde_json::to_vec(value).unwrap())
    }

    #[test]
    fn closed_versions_and_identities_are_shared_for_both_consumers() {
        let one = parse_value(&legacy()).unwrap();
        assert_eq!(one.schema_version, 1);
        let mut two = serde_json::to_value(&one).unwrap();
        two["schema_version"] = 2.into();
        assert!(parse_value(&two).is_ok());
        for field in ["runtime_ownership", "generator"] {
            let mut missing = two.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(parse_value(&missing).is_err());
        }
        two["generator"]["name"] = "project/fork".into();
        assert!(parse_value(&two).is_err());
        two["runtime_ownership"] = "transferred".into();
        two["generator"]["version"] = "2.0.0".into();
        assert!(parse_value(&two).is_ok());
        for field in ["name", "version"] {
            for bad in [
                String::new(),
                " ".to_string(),
                "x".repeat(MAX_IDENTITY_BYTES + 1),
            ] {
                let mut invalid = two.clone();
                invalid["generator"][field] = bad.into();
                assert!(parse_value(&invalid).is_err());
            }
        }
        for version in [0, 3, 999] {
            let mut invalid = legacy();
            invalid["schema_version"] = version.into();
            assert!(parse_value(&invalid).is_err());
        }
    }

    #[test]
    fn extraction_is_schema_agnostic_but_bounded_and_duplicate_rejecting() {
        let mut future = legacy();
        future["schema_version"] = 99.into();
        future["extra"] = true.into();
        let bytes = serde_json::to_vec(&future).unwrap();
        assert_eq!(recovery_root(&bytes).unwrap(), "guide");
        assert!(parse(&bytes).is_err());
        for bytes in [
            br#"{"root":"guide","root":"other"}"#.as_slice(),
            br#"{"root":"guide","extra":{"x":1,"x":2}}"#.as_slice(),
            br#"{"root":"../escape"}"#.as_slice(),
            br#"{"root":"guide/../other"}"#.as_slice(),
            br#"{"root":"./guide"}"#.as_slice(),
        ] {
            assert!(recovery_root(bytes).is_err());
        }
        let oversized = vec![b' '; usize::try_from(super::super::MAX_STATE_BYTES).unwrap() + 1];
        assert!(recovery_root(&oversized).is_err());
        assert!(parse(&oversized).is_err());
    }

    #[test]
    fn unknown_duplicate_and_unsafe_file_fields_fail_without_echoing_input() {
        let mut value = legacy();
        value["SENTINEL_SECRET"] = "do not echo".into();
        let error = parse_value(&value).unwrap_err();
        assert!(!error.contains("SENTINEL"));
        let mut value = legacy();
        value["files"]["runtime"]["extra"] = true.into();
        assert!(parse_value(&value).is_err());
        let mut value = legacy();
        value["files"]["../outside"] = value["files"]["runtime"].clone();
        assert!(parse_value(&value).is_err());
        let duplicate = br#"{"schema_version":1,"root":"guide","starter_version":"1","files":{"runtime":{"ownership":"managed","pristine_sha256":"a","pristine_sha256":"b"}}}"#;
        assert!(parse(duplicate).is_err());
    }
}
