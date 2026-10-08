//! The project's estimate adoption record, `.codeflow/estimate.json`, and
//! the frozen forecasts under its home. Read only: nothing here writes the
//! record, creates the home or reads a forecast's pinned sources.

use std::path::{Component, Path, PathBuf};

/// The adoption record's path, relative to the repository root.
pub const ADOPTION_PATH: &str = ".codeflow/estimate.json";

const MAX_ADOPTION_BYTES: u64 = 64 * 1024;
/// At most this many forecast files are joined; more is a finding.
pub(super) const MAX_FORECAST_FILES: usize = 256;

/// What `.codeflow/estimate.json` says, as the outcomes report and the
/// status line read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Adoption {
    /// No adoption record: the method is not adopted here.
    Absent,
    /// A recorded decline.
    Declined,
    /// Adopted, with its declared home and what is there.
    Adopted { root: String, home: Home },
    /// A record that cannot be read as version one, with the reason.
    Invalid(String),
}

/// The state of an adopted estimate home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Home {
    /// The home exists; its frozen forecasts, `forecasts/<name>/v<N>.json`,
    /// as repository-relative paths in name then version order.
    Present { forecasts: Vec<String> },
    /// `root` names no directory.
    Missing,
    /// `root` is not a safe project-relative path, or more forecasts than
    /// the bound are present.
    Unusable(String),
}

/// Read the adoption record of the repository at `repo_root`.
#[must_use]
pub fn read(repo_root: &Path) -> Adoption {
    if std::fs::symlink_metadata(repo_root.join(ADOPTION_PATH)).is_err() {
        return Adoption::Absent;
    }
    // Read through the confined root, so a symlinked `.codeflow` cannot
    // redirect the read outside the repository.
    let bytes = std::fs::canonicalize(repo_root)
        .ok()
        .and_then(|root| crate::bounded_file::ConfinedRoot::open(&root).ok())
        .and_then(|root| root.read(Path::new(ADOPTION_PATH), MAX_ADOPTION_BYTES).ok());
    let Some(bytes) = bytes else {
        return Adoption::Invalid(format!(
            "{ADOPTION_PATH} is not a regular file of at most 64 KiB inside the repository"
        ));
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Adoption::Invalid(format!("{ADOPTION_PATH} is not JSON"));
    };
    if value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        return Adoption::Invalid(format!("{ADOPTION_PATH} is not schema_version 1"));
    }
    match value.get("status").and_then(serde_json::Value::as_str) {
        Some("declined") => Adoption::Declined,
        Some("adopted") => match value.get("root").and_then(serde_json::Value::as_str) {
            Some(root) => Adoption::Adopted {
                root: root.to_string(),
                home: home(repo_root, root),
            },
            None => Adoption::Invalid(format!("{ADOPTION_PATH} is adopted but names no root")),
        },
        _ => Adoption::Invalid(format!(
            "{ADOPTION_PATH} status is neither adopted nor declined"
        )),
    }
}

fn home(repo_root: &Path, root: &str) -> Home {
    let Some(dir) = safe_dir(repo_root, root) else {
        return Home::Unusable(format!("root `{root}` is not a safe project-relative path"));
    };
    match std::fs::symlink_metadata(&dir) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Home::Unusable(format!("root `{root}` is not a directory"));
        }
        Err(_) => return Home::Missing,
    }
    let mut found: Vec<(String, u64, String)> = Vec::new();
    for name in real_dirs(&dir.join("forecasts")) {
        let Some(entries) = std::fs::read_dir(dir.join("forecasts").join(&name)).ok() else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let file = entry.file_name().to_string_lossy().into_owned();
            let Some(version) = file
                .strip_prefix('v')
                .and_then(|rest| rest.strip_suffix(".json"))
                .and_then(|number| number.parse::<u64>().ok())
            else {
                continue;
            };
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            if found.len() == MAX_FORECAST_FILES {
                return Home::Unusable(format!(
                    "more than {MAX_FORECAST_FILES} forecast files under `{root}/forecasts`"
                ));
            }
            let root = root.trim_end_matches('/');
            found.push((
                name.clone(),
                version,
                format!("{root}/forecasts/{name}/{file}"),
            ));
        }
    }
    found.sort();
    Home::Present {
        forecasts: found.into_iter().map(|(_, _, path)| path).collect(),
    }
}

/// Subdirectories of `dir` that are real directories, by name.
fn real_dirs(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    if !std::fs::symlink_metadata(dir).is_ok_and(|metadata| metadata.is_dir()) {
        return Vec::new();
    }
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .collect();
    names.sort();
    names
}

/// `root` joined to the repository, when it is relative, plain and crosses
/// no symlink.
fn safe_dir(repo_root: &Path, root: &str) -> Option<PathBuf> {
    let relative = Path::new(root.trim_end_matches('/'));
    if root.is_empty()
        || root.len() > 1024
        || root.contains('\\')
        || relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || super::sources::secret_path(relative)
    {
        return None;
    }
    let mut full = repo_root.to_path_buf();
    for component in relative.components() {
        full.push(component);
        if std::fs::symlink_metadata(&full).is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return None;
        }
    }
    Some(full)
}

/// The one `codeflow status` line for estimates, or `None` when the project
/// has no adoption record. `completed` is the count of complete task
/// records, each an outcome the report can derive.
#[must_use]
pub fn status_line(repo_root: &Path, completed: usize) -> Option<String> {
    match read(repo_root) {
        Adoption::Absent => None,
        Adoption::Declined => Some("estimates: declined".to_string()),
        Adoption::Invalid(reason) => Some(format!("estimates: unreadable adoption record ({reason})")),
        Adoption::Adopted { root, home } => Some(match home {
            Home::Present { forecasts } => format!(
                "estimates: adopted; {} frozen forecast(s); {completed} completed task(s) to compare; run `codeflow estimate outcomes`",
                forecasts.len()
            ),
            Home::Missing => format!(
                "estimates: adopted; home missing ({root}): no profile, no frozen forecast, no outcomes"
            ),
            Home::Unusable(reason) => format!("estimates: adopted; home unusable ({reason})"),
        }),
    }
}
