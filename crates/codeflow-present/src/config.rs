use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{error::PresentError, limits, Result};

const MAX_CONFIG_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub retention: RetentionPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RetentionPolicy {
    pub closed_days: u64,
    pub max_closed_sessions: usize,
    pub max_project_bytes: u64,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            schema_version: limits::SCHEMA_VERSION,
            retention: RetentionPolicy::default(),
        }
    }
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            closed_days: limits::CLOSED_RETENTION_DAYS,
            max_closed_sessions: limits::MAX_CLOSED_SESSIONS,
            max_project_bytes: limits::MAX_PROJECT_STATE_BYTES,
        }
    }
}

impl ProjectConfig {
    pub fn load(project_root: &Path) -> Result<Self> {
        let relative = Path::new(".codeflow/present/config.toml");
        let path = project_root.join(relative);
        if !path.exists() {
            return Ok(Self::default());
        }
        let path = resolve_existing_project_file(project_root, relative, MAX_CONFIG_BYTES)?;
        let raw = fs::read_to_string(&path).map_err(|error| PresentError::io(&path, error))?;
        let config: Self = toml::from_str(&raw).map_err(|error| {
            PresentError::InvalidDocument(format!("invalid cf-present config: {error}"))
        })?;
        config.validate(project_root)?;
        Ok(config)
    }

    fn validate(&self, _project_root: &Path) -> Result<()> {
        if self.schema_version != limits::SCHEMA_VERSION {
            return Err(PresentError::UnsupportedSchema {
                found: self.schema_version,
                supported: limits::SCHEMA_VERSION,
            });
        }
        if !(1..=3_650).contains(&self.retention.closed_days)
            || !(1..=10_000).contains(&self.retention.max_closed_sessions)
            || !(1024 * 1024..=10 * 1024 * 1024 * 1024).contains(&self.retention.max_project_bytes)
        {
            return Err(PresentError::InvalidDocument(
                "cf-present retention values are outside supported bounds".to_string(),
            ));
        }
        Ok(())
    }
}

fn resolve_existing_project_file(
    project_root: &Path,
    relative: &Path,
    max_bytes: u64,
) -> Result<PathBuf> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PresentError::UnsafePath(relative.to_path_buf()));
    }
    let path = project_root.join(relative);
    let metadata = fs::symlink_metadata(&path).map_err(|error| PresentError::io(&path, error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > max_bytes {
        return Err(PresentError::UnsafePath(path));
    }
    let canonical_root = project_root
        .canonicalize()
        .map_err(|error| PresentError::io(project_root, error))?;
    let canonical_path = path
        .canonicalize()
        .map_err(|error| PresentError::io(&path, error))?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(PresentError::UnsafePath(path));
    }
    Ok(canonical_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_config_uses_concrete_retention_defaults() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            ProjectConfig::load(temp.path()).unwrap(),
            ProjectConfig::default()
        );
    }

    #[test]
    fn config_is_closed() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join(".codeflow/present");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.toml"), "schema_version=1\nunknown=true\n").unwrap();
        assert!(ProjectConfig::load(temp.path()).is_err());
    }
}
