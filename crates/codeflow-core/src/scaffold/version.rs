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

fn is_parseable(version: &str) -> bool {
    parse(version).is_some()
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
    if is_older(binary, pinned) {
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

/// Is `a` strictly older than `b`? Unparseable versions compare as strings;
/// callers that write state must validate their inputs first.
#[must_use]
pub fn is_older(a: &str, b: &str) -> bool {
    let (Some((na, pa)), Some((nb, pb))) = (parse(a), parse(b)) else {
        return a < b;
    };
    if na != nb {
        return na < nb;
    }
    match (pa, pb) {
        (Some(_), None) => true, // pre-release < release
        (None, _) => false,
        (Some(x), Some(y)) => prerelease_cmp(x, y).is_lt(),
    }
}

/// One-line warning when the project's scaffold is behind the binary.
/// Quiet (None) when the project is uninitialized or up to date.
#[must_use]
pub fn version_skew_warning(root: &Path, binary_version: &str) -> Option<String> {
    let state = super::state::ProjectState::load(root).ok()?;
    is_older(&state.scaffold_version, binary_version).then(|| {
        format!(
            "codeflow: scaffold {} is behind binary {} — run `codeflow update`",
            crate::git::display_line(&state.scaffold_version),
            binary_version
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_ordering() {
        assert!(is_older("1.9.0", "2.0.0"));
        assert!(is_older("2.0.0-dev", "2.0.0"));
        assert!(!is_older("2.0.0", "2.0.0"));
        assert!(!is_older("2.1.0", "2.0.9"));
        assert!(is_older("2.0.0-alpha", "2.0.0-beta"));
        assert!(is_older("2.0.0-rc.2", "2.0.0-rc.10"));
        assert!(is_older("2.0.0-1", "2.0.0-alpha"));
        assert!(is_older("2.0.0-alpha", "2.0.0-alpha.1"));
        assert!(!is_older("2.0.0+one", "2.0.0+two"));
        assert!(is_older("2.0.0+two", "2.0.1+one"));
    }
}
