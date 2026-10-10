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
    // Only a proven missing record is no adoption (issue 79); one that
    // cannot be inspected is reported, never read as absent.
    match crate::absence::proven_absent(&repo_root.join(ADOPTION_PATH)) {
        Ok(true) => return Adoption::Absent,
        Ok(false) => {}
        Err(error) => {
            return Adoption::Invalid(format!("{ADOPTION_PATH} cannot be inspected: {error}"))
        }
    }
    // Read through the confined root, so a symlinked `.codeflow` cannot
    // redirect the read outside the repository.
    let bytes = std::fs::canonicalize(repo_root)
        .and_then(|root| crate::bounded_file::ConfinedRoot::open(&root))
        .and_then(|root| root.read(Path::new(ADOPTION_PATH), MAX_ADOPTION_BYTES));
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => {
            return Adoption::Invalid(format!(
                "{ADOPTION_PATH} is not a regular file of at most 64 KiB inside the repository ({error})"
            ))
        }
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
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Home::Missing,
        Err(error) => return Home::Unusable(format!("root `{root}` cannot be read: {error}")),
    }
    // A forecast directory or entry that cannot be read leaves the home
    // unusable (issue 79), never a home with fewer forecasts.
    let unreadable = |what: &str, error: std::io::Error| {
        Home::Unusable(format!(
            "{what} under `{root}/forecasts` cannot be read: {error}"
        ))
    };
    let names = match real_dirs(&dir.join("forecasts")) {
        Ok(names) => names,
        Err(error) => return unreadable("the forecasts directory", error),
    };
    let mut found: Vec<(String, u64, String)> = Vec::new();
    for name in names {
        let entries = match std::fs::read_dir(dir.join("forecasts").join(&name)) {
            Ok(entries) => entries,
            Err(error) => return unreadable(&format!("`{name}`"), error),
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => return unreadable(&format!("an entry of `{name}`"), error),
            };
            // A frozen forecast is named `v<N>.json`, which is ASCII, so a
            // name that is not UTF-8 is no forecast.
            let Ok(file) = entry.file_name().into_string() else {
                continue;
            };
            let Some(version) = file
                .strip_prefix('v')
                .and_then(|rest| rest.strip_suffix(".json"))
                .and_then(|number| number.parse::<u64>().ok())
            else {
                continue;
            };
            match entry.file_type() {
                Ok(kind) if kind.is_file() => {}
                Ok(_) => continue,
                Err(error) => return unreadable(&format!("`{name}/{file}`"), error),
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

/// Subdirectories of `dir` that are real directories, by name; none when
/// `dir` is missing or is not a real directory.
///
/// # Errors
///
/// A directory or entry that cannot be read.
fn real_dirs(dir: &Path) -> std::io::Result<Vec<String>> {
    match std::fs::symlink_metadata(dir) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Ok(Vec::new()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        // A forecast name is a path segment the report writes back as
        // text; a directory whose name is not UTF-8 may hold forecasts, so
        // it refuses rather than hiding them (issue 79).
        let name = entry.file_name().into_string().map_err(|name| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "the forecast directory `{}` has a name that is not UTF-8",
                    crate::git::GitName::from_bytes(name.as_encoded_bytes()).display()
                ),
            )
        })?;
        names.push(name);
    }
    names.sort();
    Ok(names)
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
