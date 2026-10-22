//! Version-skew detection (charter §10): scaffold version ≡ binary version
//! at install time; every CLI invocation cheaply compares the recorded
//! scaffold version against the running binary and warns when behind.

use std::path::Path;

/// Parse the numeric core and prerelease; build metadata has no precedence.
fn parse(version: &str) -> Option<(Vec<u64>, Option<&str>)> {
    let version = match version.split_once('+') {
        Some((version, build)) if valid_identifiers(build, false) => version,
        Some(_) => return None,
        None => version,
    };
    let (core, pre) = match version.split_once('-') {
        Some((c, p)) if valid_identifiers(p, true) => (c, Some(p)),
        Some(_) => return None,
        None => (version, None),
    };
    let nums: Vec<u64> = core
        .split('.')
        .map(|part| {
            (!part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part == "0" || !part.starts_with('0')))
            .then(|| part.parse().ok())
            .flatten()
        })
        .collect::<Option<_>>()?;
    (nums.len() == 3).then_some((nums, pre))
}

fn valid_identifiers(value: &str, prerelease: bool) -> bool {
    value.split('.').all(|part| {
        !part.is_empty()
            && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !(prerelease
                && part.len() > 1
                && part.starts_with('0')
                && part.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// Does `version` read as `MAJOR.MINOR.PATCH` with an optional prerelease
/// and build? Every pin check goes through this one parser.
#[must_use]
pub fn is_parseable(version: &str) -> bool {
    parse(version).is_some()
}

/// A version the strict parser rejects. Two versions with one of these never
/// compare, so no caller can fall back to a string order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unparseable(pub String);

impl std::fmt::Display for Unparseable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} is not a parseable version", self.0)
    }
}

/// Check an existing pin before init or update writes any project file.
pub(super) fn check_write_version(
    pinned: &str,
    binary: &str,
    command: &str,
) -> Result<(), super::ScaffoldError> {
    let project_file = super::state::PROJECT_TOML;
    for (name, value, location, remedy) in [
        (
            "scaffold_version",
            pinned,
            format!(" in {project_file}"),
            "correct it",
        ),
        (
            "binary version",
            binary,
            String::new(),
            "install a build with a valid version",
        ),
    ] {
        if !is_parseable(value) {
            return Err(super::ScaffoldError::InvalidState {
                what: name.to_string(),
                detail: format!(
                    "{value:?}{location} is not a parseable version; {remedy} and retry; {command} wrote no project files"
                ),
            });
        }
    }
    // Both values parsed above, so the comparison is always `Ok`.
    if is_older(binary, pinned) == Ok(true) {
        return Err(super::ScaffoldError::InvalidState {
            what: format!("{command} version"),
            detail: format!(
                "binary {binary} is older than scaffold_version {pinned} in {project_file}; install the pinned version {pinned} and retry; {command} wrote no project files"
            ),
        });
    }
    Ok(())
}

fn prerelease_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    // Numeric identifiers compare by magnitude, even beyond u64's range.
    a.split('.')
        .map(|part| {
            let numeric = part.bytes().all(|b| b.is_ascii_digit());
            (!numeric, if numeric { part.len() } else { 0 }, part)
        })
        .cmp(b.split('.').map(|part| {
            let numeric = part.bytes().all(|b| b.is_ascii_digit());
            (!numeric, if numeric { part.len() } else { 0 }, part)
        }))
}

/// Is `a` strictly older than `b`?
///
/// # Errors
///
/// The first of `a` and `b` the strict parser rejects; an unparseable
/// version is never ordered, as text or otherwise.
pub fn is_older(a: &str, b: &str) -> Result<bool, Unparseable> {
    let (na, pa) = parse(a).ok_or_else(|| Unparseable(a.to_string()))?;
    let (nb, pb) = parse(b).ok_or_else(|| Unparseable(b.to_string()))?;
    if na != nb {
        return Ok(na < nb);
    }
    Ok(match (pa, pb) {
        (Some(_), None) => true, // pre-release < release
        (None, _) => false,
        (Some(x), Some(y)) => prerelease_cmp(x, y).is_lt(),
    })
}

/// One-line warning when the project's scaffold is behind the binary.
/// Quiet (None) when the project is uninitialized or up to date.
#[must_use]
pub fn version_skew_warning(root: &Path, binary_version: &str) -> Option<String> {
    let state = super::state::ProjectState::load(root).ok()?;
    // An unparseable pin stays quiet here: this runs on every invocation,
    // and `init`, `update` and `doctor` already name it where it matters.
    (is_older(&state.scaffold_version, binary_version) == Ok(true)).then(|| {
        format!(
            "codeflow: scaffold {} is behind binary {} — run `codeflow update`",
            state.scaffold_version, binary_version
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_ordering() {
        assert!(is_older("1.9.0", "2.0.0").unwrap());
        assert!(is_older("2.0.0-dev", "2.0.0").unwrap());
        assert!(!is_older("2.0.0", "2.0.0").unwrap());
        assert!(!is_older("2.1.0", "2.0.9").unwrap());
        assert!(is_older("2.0.0-alpha", "2.0.0-beta").unwrap());
        assert!(is_older("2.0.0-rc.2", "2.0.0-rc.10").unwrap());
        assert!(is_older("2.0.0-1", "2.0.0-alpha").unwrap());
        assert!(is_older("2.0.0-alpha", "2.0.0-alpha.1").unwrap());
        assert!(!is_older("2.0.0+one", "2.0.0+two").unwrap());
        assert!(is_older("2.0.0+two", "2.0.1+one").unwrap());
    }

    #[test]
    fn unparseable_versions_are_never_ordered() {
        for bad in [
            "3.10", "10.0", "3.2.0x", "3.2", "v3.2.0", "", "3.2.0-", "01.0.0",
        ] {
            assert_eq!(is_older(bad, "9.0.0"), Err(Unparseable(bad.to_string())));
            assert_eq!(is_older("9.0.0", bad), Err(Unparseable(bad.to_string())));
        }
        // As strings "10.0" < "9.0"; neither order is answered.
        assert!(is_older("10.0", "9.0").is_err());
        assert!(is_older("9.0", "10.0").is_err());
    }

    #[test]
    fn skew_warning_is_quiet_for_an_unparseable_pin() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        for pin in ["10.0", "3.2.0x"] {
            std::fs::write(
                dir.path().join(".codeflow/project.toml"),
                format!("schema_version = 1\ntier = \"minimal\"\nscaffold_version = \"{pin}\"\n"),
            )
            .unwrap();
            assert_eq!(version_skew_warning(dir.path(), "9.0.0"), None, "{pin}");
        }
    }
}
