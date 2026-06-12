//! Version-skew detection (charter §10): scaffold version ≡ binary version
//! at install time; every CLI invocation cheaply compares the recorded
//! scaffold version against the running binary and warns when behind.

use std::path::Path;

/// Loose semver: numeric core compared first; a pre-release sorts before its
/// release (`2.0.0-dev < 2.0.0`); unparseable versions compare as strings.
fn parse(version: &str) -> (Vec<u64>, Option<String>) {
    let (core, pre) = match version.split_once('-') {
        Some((c, p)) => (c, Some(p.to_string())),
        None => (version, None),
    };
    let nums: Vec<u64> = core.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    (nums, pre)
}

/// Is `a` strictly older than `b`?
#[must_use]
pub fn is_older(a: &str, b: &str) -> bool {
    let (na, pa) = parse(a);
    let (nb, pb) = parse(b);
    if na != nb {
        return na < nb;
    }
    match (pa, pb) {
        (Some(_), None) => true, // pre-release < release
        (None, _) => false,
        (Some(x), Some(y)) => x < y,
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
            state.scaffold_version, binary_version
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
    }
}
