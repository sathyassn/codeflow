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
        // The value is not shown: it is free text the project wrote, and can
        // hold a secret (TSK-1029).
        write!(f, "is not a parseable version")
    }
}

/// Check an existing pin before init or update writes any project file.
pub(super) fn check_write_version(
    pinned: &str,
    binary: &str,
    command: &'static str,
) -> Result<(), super::ScaffoldError> {
    use super::Shown;
    let project_file = Shown::path(super::state::PROJECT_TOML);
    let command = Shown::fixed(command);
    for (name, value, location, remedy) in [
        (
            "scaffold_version",
            pinned,
            crate::shown!(" in {}", project_file),
            "correct it",
        ),
        (
            "binary version",
            binary,
            Shown::default(),
            "install a build with a valid version",
        ),
    ] {
        // The value is not shown: a pin that does not parse is free text the
        // project wrote, and can hold a secret (TSK-1029).
        if !is_parseable(value) {
            return Err(super::ScaffoldError::invalid(
                Shown::fixed(name),
                crate::shown!(
                    "the value{} is not a parseable version; {} and retry; {} wrote no project files",
                    location,
                    Shown::fixed(remedy),
                    command
                ),
            ));
        }
    }
    // Both values parsed above, so the comparison is always `Ok`.
    if is_older(binary, pinned) == Ok(true) {
        let pinned = Shown::version(pinned);
        return Err(super::ScaffoldError::invalid(
            crate::shown!("{} version", command),
            crate::shown!(
                "binary {} is older than scaffold_version {} in {}; install the pinned version {} and retry; {} wrote no project files",
                Shown::version(binary),
                pinned,
                project_file,
                pinned,
                command
            ),
        ));
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

/// `version` as a report may print it (TSK-1029). The strict grammar still
/// admits an opaque suffix a project wrote, such as
/// `999.0.0+opaque-value`, so only `MAJOR.MINOR.PATCH` is printed, plus a
/// prerelease in `CodeFlow`'s own release form (`alpha.N`, `beta.N`,
/// `rc.N`). Any other prerelease, and any build metadata, is replaced by
/// `(with a suffix)`; a value that does not parse is never printed.
#[must_use]
pub fn shown(version: &str) -> String {
    if !is_parseable(version) {
        return "(a value that is not a version)".to_string();
    }
    let (rest, build) = match version.split_once('+') {
        Some((rest, _)) => (rest, true),
        None => (version, false),
    };
    let (core, prerelease) = match rest.split_once('-') {
        Some((core, prerelease)) => (core, Some(prerelease)),
        None => (rest, None),
    };
    let release_form = |label: &str| {
        label.split_once('.').is_some_and(|(name, number)| {
            matches!(name, "alpha" | "beta" | "rc")
                && !number.is_empty()
                && number.bytes().all(|b| b.is_ascii_digit())
        })
    };
    match prerelease {
        Some(label) if !build && release_form(label) => format!("{core}-{label}"),
        None if !build => core.to_string(),
        _ => format!("{core} (with a suffix)"),
    }
}

/// One-line warning when the project's scaffold is behind the binary.
/// Quiet (None) when the project is uninitialized or up to date.
#[must_use]
pub fn version_skew_warning(root: &Path, binary_version: &str) -> Option<super::Shown> {
    use super::Shown;
    let state = super::state::ProjectState::load(root).ok()?;
    // An unparseable pin stays quiet here: this runs on every invocation,
    // and `init`, `update` and `doctor` already name it where it matters.
    (is_older(&state.scaffold_version, binary_version) == Ok(true)).then(|| {
        crate::shown!(
            "codeflow: scaffold {} is behind binary {} — run `codeflow update`",
            Shown::version(&state.scaffold_version),
            Shown::version(binary_version)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TSK-1029: only `CodeFlow`'s own release forms are shown in full.
    #[test]
    fn a_version_is_shown_without_a_project_suffix() {
        for (version, expected) in [
            ("3.2.0", "3.2.0"),
            ("3.2.0-rc.1", "3.2.0-rc.1"),
            ("3.2.0-alpha.12", "3.2.0-alpha.12"),
            ("3.2.0-beta.3", "3.2.0-beta.3"),
            ("999.0.0+opaque-31337", "999.0.0 (with a suffix)"),
            ("1.0.0-opaque31337", "1.0.0 (with a suffix)"),
            ("1.0.0-rc.1.opaque", "1.0.0 (with a suffix)"),
            ("1.0.0-rc.1+build", "1.0.0 (with a suffix)"),
            ("10.0", "(a value that is not a version)"),
        ] {
            assert_eq!(shown(version), expected, "{version}");
        }
    }

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

    /// The shipped CI templates order pins with a shell function. It must
    /// agree with `is_older` on every row, and refuse what the parser rejects.
    #[cfg(unix)]
    #[test]
    fn the_ci_shell_function_orders_versions_as_is_older_does() {
        const CI: &str = include_str!("../../../../assets/base/ci/ci-generic.sh");
        let start = CI.find("codeflow_older() {").expect("function");
        let end = start + CI[start..].find("\n}\n").expect("end") + 3;
        let function = &CI[start..end];
        let max = "18446744073709551615.0.0";
        let rows = [
            ("3.2.0-rc.2", "3.2.0-rc.10"),
            ("3.2.0-rc.10", "3.2.0-rc.2"),
            ("3.2.0+build-1", "3.2.0"),
            ("3.2.0", "3.2.0+build-1"),
            ("18446744073709551614.0.0", max),
            ("18446744073709551615.0.0", "18446744073709551614.0.0"),
            ("1.9.0", "2.0.0"),
            ("2.0.0", "1.9.0"),
            ("2.0.0", "2.0.0"),
            ("2.0.0-dev", "2.0.0"),
            ("2.0.0", "2.0.0-dev"),
            ("2.0.0-alpha", "2.0.0-beta"),
            ("2.0.0-1", "2.0.0-alpha"),
            ("2.0.0-alpha", "2.0.0-1"),
            ("2.0.0-alpha", "2.0.0-alpha.1"),
            ("2.0.0-alpha.1", "2.0.0-alpha"),
            ("2.0.0+two", "2.0.1+one"),
            (
                "2.0.0-rc.99999999999999999999999",
                "2.0.0-rc.100000000000000000000000",
            ),
            ("2.10.0", "2.9.0"),
            ("2.9.0", "2.10.0"),
        ];
        let run = |a: &str, b: &str| {
            std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("{function}\ncodeflow_older \"$1\" \"$2\""))
                .arg("sh")
                .args([a, b])
                .output()
                .unwrap()
                .status
                .code()
        };
        for (a, b) in rows {
            let expect = i32::from(!is_older(a, b).unwrap());
            assert_eq!(run(a, b), Some(expect), "{a} vs {b}");
        }
        for bad in [
            "3.2",
            "10.0",
            "3.2.0x",
            "v3.2.0",
            "",
            "3.2.0-",
            "01.0.0",
            "3.2.0-01",
            "18446744073709551616.0.0",
            "3.2.0+",
            "3.2.0+a+b",
        ] {
            assert!(!is_parseable(bad), "{bad}");
            assert_eq!(run(bad, "9.0.0"), Some(2), "{bad:?} left");
            assert_eq!(run("9.0.0", bad), Some(2), "{bad:?} right");
        }
    }
}
