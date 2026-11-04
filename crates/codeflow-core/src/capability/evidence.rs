//! Typed `verified_by` entries: one grammar and one resolver.
//!
//! An entry is `test: <path>[::<name>]`, `journey: <path>[::<name>]`,
//! `record: docs/verification/<path>` or `gap: <proof>: <reason>`. The catalog
//! loader checks the grammar on every record, from a checkout or a tree. The
//! docs lint resolves the references against a checkout. Resolution reads
//! files through repository-confined handles and never runs anything an entry
//! names.

use crate::contained::Tree;
use std::io::ErrorKind;

/// The kinds an entry can declare.
pub const EVIDENCE_KINDS: &[&str] = &["test", "journey", "record", "gap"];
/// The proof a declared gap says is still owed.
pub const GAP_PROOFS: &[&str] = &["test", "journey", "hosted", "native"];
/// Where a native or hosted verification record lives.
pub const VERIFICATION_RECORD_DIR: &str = "docs/verification/";
/// The largest file read to find a named test. A larger file is refused, and
/// naming the file alone never reads it. This is a constant for now because the
/// limits registry has not reached this line; it moves there under TSK-1299.
pub const CAPABILITY_MAX_EVIDENCE_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// The file type bits of a Git index mode, and their value for a regular file.
const GIT_FILE_TYPE_MASK: u32 = 0o170_000;
const GIT_REGULAR_FILE: u32 = 0o100_000;

const FORMAT: &str = "expected test: <path>[::<name>], journey: <path>[::<name>], \
                      record: docs/verification/<path> or gap: <proof>: <reason>";

/// A test file, with an optional test or test module inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestIdentity<'a> {
    pub path: &'a str,
    pub name: Option<&'a str>,
}

/// One parsed `verified_by` entry, borrowing from the entry's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence<'a> {
    Test(TestIdentity<'a>),
    /// A test that drives the whole path a user takes. It resolves as a test.
    Journey(TestIdentity<'a>),
    Record(&'a str),
    Gap {
        proof: &'a str,
        reason: &'a str,
    },
}

impl Evidence<'_> {
    /// Whether this entry proves anything; a declared gap does not.
    #[must_use]
    pub fn is_proof(&self) -> bool {
        !matches!(self, Evidence::Gap { .. })
    }
}

/// Whether an entry declares one of the evidence kinds. A legacy fence may hold
/// a free-form tag that does not.
#[must_use]
pub fn is_typed_evidence(entry: &str) -> bool {
    entry
        .split_once(':')
        .is_some_and(|(kind, _)| EVIDENCE_KINDS.contains(&kind))
}

/// Parse one `verified_by` entry without opening anything.
///
/// # Errors
/// Returns the reason an entry does not follow the grammar.
pub fn parse_evidence(entry: &str) -> Result<Evidence<'_>, String> {
    let Some((kind, reference)) = entry.split_once(':') else {
        return Err(FORMAT.to_string());
    };
    let reference = reference.trim_ascii();
    if reference.is_empty() {
        return Err(format!("the reference is empty; {FORMAT}"));
    }
    match kind {
        "test" => parse_test_identity(reference).map(Evidence::Test),
        "journey" => parse_test_identity(reference).map(Evidence::Journey),
        "record" => parse_record(reference).map(Evidence::Record),
        "gap" => parse_gap(reference),
        _ => Err(FORMAT.to_string()),
    }
}

fn parse_test_identity(reference: &str) -> Result<TestIdentity<'_>, String> {
    if reference.contains(char::is_whitespace) {
        return Err(
            "names a command or free text; a test is a repository path, optionally with ::<test name>"
                .to_string(),
        );
    }
    let (path, name) = match reference.split_once("::") {
        Some((path, name)) => (path, Some(name)),
        None => (reference, None),
    };
    check_relative_path(path)?;
    if let Some(name) = name {
        let valid = name.split("::").all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        });
        if !valid {
            return Err(format!(
                "{name:?} is not a test name; use letters, digits, _ and -, with :: between segments"
            ));
        }
    }
    Ok(TestIdentity { path, name })
}

fn parse_record(reference: &str) -> Result<&str, String> {
    if !reference.starts_with(VERIFICATION_RECORD_DIR) {
        return Err(format!(
            "a record is a file under {VERIFICATION_RECORD_DIR}"
        ));
    }
    if reference.contains(char::is_whitespace) {
        return Err("a record reference is a path, without spaces".to_string());
    }
    check_relative_path(reference)?;
    Ok(reference)
}

fn parse_gap(reference: &str) -> Result<Evidence<'_>, String> {
    let shape = || {
        format!(
            "a gap is gap: <proof>: <reason>, where <proof> is one of {}",
            GAP_PROOFS.join(", ")
        )
    };
    let (proof, reason) = reference.split_once(':').ok_or_else(shape)?;
    let reason = reason.trim();
    if !GAP_PROOFS.contains(&proof) || reason.is_empty() {
        return Err(shape());
    }
    Ok(Evidence::Gap { proof, reason })
}

fn check_relative_path(path: &str) -> Result<(), String> {
    let confined = !path.starts_with('/')
        && !path.contains(['\\', ':'])
        && !path.contains(char::is_control)
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    if confined {
        Ok(())
    } else {
        Err(format!(
            "{path:?} is not a repository-relative path; absolute paths, '.', '..', control characters and trailing slashes are refused"
        ))
    }
}

/// Check that an entry's reference exists in the checkout `tree` opens. A gap
/// always resolves: it declares that no proof exists.
///
/// # Errors
/// Returns why the reference does not resolve, or why it cannot be verified.
pub(crate) fn resolve_evidence(
    tree: &Tree,
    index: Option<&git2::Index>,
    evidence: &Evidence<'_>,
) -> Result<(), String> {
    match evidence {
        Evidence::Test(identity) | Evidence::Journey(identity) => {
            resolve_test(tree, index, identity)
        }
        Evidence::Record(path) => existing_file(tree, index, path),
        Evidence::Gap { .. } => Ok(()),
    }
}

fn resolve_test(
    tree: &Tree,
    index: Option<&git2::Index>,
    identity: &TestIdentity<'_>,
) -> Result<(), String> {
    let path = identity.path;
    let rust = std::path::Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"));
    let test_tree = in_test_tree(path);
    if !test_tree && (identity.name.is_none() || !rust) {
        return Err(format!(
            "{path} is a source file, which proves nothing by itself; name a file in the test tree, or a Rust test inside it with {path}::<name>"
        ));
    }
    let Some(name) = identity.name else {
        return existing_file(tree, index, path);
    };
    existing_file(tree, None, path)?;
    let bytes = tree
        .read(path, CAPABILITY_MAX_EVIDENCE_FILE_BYTES)
        .map_err(|error| format!("{path} cannot be read to find {name}: {error}"))?;
    let text = String::from_utf8_lossy(&bytes);
    let found = if rust {
        rust_test_defined(&text, name, test_tree)
    } else {
        name.split("::").all(|segment| has_word(&text, segment))
    };
    if found {
        Ok(())
    } else {
        Err(format!("{path} has no test named {name}"))
    }
}

/// A regular, non-empty file. A sandbox may deny reading a path by its name,
/// even its metadata; the Git index then proves a regular file is tracked there,
/// which a link or gitlink is not. Without that proof the entry is refused,
/// because it cannot be verified.
fn existing_file(tree: &Tree, index: Option<&git2::Index>, path: &str) -> Result<(), String> {
    match tree.metadata(path) {
        Ok(metadata) if !metadata.is_file() => Err(format!("{path} is not a regular file")),
        Ok(metadata) if metadata.len() == 0 => Err(format!("{path} is empty")),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Err(format!("{path} does not exist")),
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {
            let tracked = index.is_some_and(|index| {
                index
                    .get_path(std::path::Path::new(path), 0)
                    .is_some_and(|entry| entry.mode & GIT_FILE_TYPE_MASK == GIT_REGULAR_FILE)
            });
            if tracked {
                Ok(())
            } else {
                Err(format!("{path} cannot be read: {error}"))
            }
        }
        Err(error) => Err(format!("{path} cannot be read: {error}")),
    }
}

/// Whether a path follows a common test layout. Fixtures, snapshots and data
/// files can sit inside a test folder, but they are not tests.
fn in_test_tree(path: &str) -> bool {
    let (folders, file) = path.rsplit_once('/').unwrap_or(("", path));
    let mut folders = folders.split('/');
    if folders
        .clone()
        .any(|folder| ["fixtures", "testdata", "__snapshots__"].contains(&folder))
    {
        return false;
    }
    let (stem, extension) = file.rsplit_once('.').unwrap_or((file, ""));
    if ["md", "txt", "json", "toml", "yaml", "yml", "lock"].contains(&extension) {
        return false;
    }
    folders.any(|folder| ["tests", "test", "__tests__", "spec", "specs"].contains(&folder))
        || ["tests", "test"].contains(&stem)
        || stem.starts_with("test_")
        || ["_test", "_spec", ".test", ".spec", "Test", "Tests", "Spec"]
            .iter()
            .any(|suffix| stem.ends_with(suffix))
}

fn has_word(text: &str, word: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    text.match_indices(word).any(|(start, _)| {
        !text[..start].chars().next_back().is_some_and(is_word)
            && !text[start + word.len()..]
                .chars()
                .next()
                .is_some_and(is_word)
    })
}

/// Whether `name` is a test function or test module in Rust source. Each
/// leading segment must be a module. Comments and strings never match, because
/// the declaration must start its line. Outside the test tree a module must be
/// under a `cfg` that enables it for tests.
fn rust_test_defined(text: &str, name: &str, test_tree: bool) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    let mut segments: Vec<&str> = name.split("::").collect();
    let Some(last) = segments.pop() else {
        return false;
    };
    let is_module = |segment: &str, line: usize| {
        declares(lines[line], "mod", segment)
            && (test_tree
                || attributes_above(&lines, line)
                    .iter()
                    .any(|attribute| attribute.starts_with("#[cfg") && enables_tests(attribute)))
    };
    let is_test = |line: usize| {
        declares(lines[line], "fn", last)
            && attributes_above(&lines, line)
                .iter()
                .any(|attribute| !attribute.starts_with("#[cfg") && attribute.contains("test"))
    };
    segments
        .iter()
        .all(|segment| (0..lines.len()).any(|line| is_module(segment, line)))
        && (0..lines.len()).any(|line| is_test(line) || is_module(last, line))
}

/// Whether a line declares `keyword name`, past any visibility and `async`.
fn declares(line: &str, keyword: &str, name: &str) -> bool {
    let mut rest = line.trim_start();
    if let Some(after) = rest.strip_prefix("pub") {
        rest = match after.trim_start().strip_prefix('(') {
            Some(scoped) => scoped.split_once(')').map_or("", |(_, rest)| rest),
            None => after,
        }
        .trim_start();
    }
    rest = rest.strip_prefix("async ").unwrap_or(rest).trim_start();
    rest.strip_prefix(keyword)
        .filter(|after| after.starts_with(char::is_whitespace))
        .map(str::trim_start)
        .and_then(|after| after.strip_prefix(name))
        .is_some_and(|after| {
            !after
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
}

fn enables_tests(attribute: &str) -> bool {
    attribute.contains("test") && !attribute.contains("not(test")
}

/// The attribute lines directly above a declaration, past doc comments.
fn attributes_above<'a>(lines: &[&'a str], declaration: usize) -> Vec<&'a str> {
    let mut attributes = Vec::new();
    for line in lines[..declaration].iter().rev() {
        let line = line.trim_start();
        if line.starts_with("#[") {
            attributes.push(line);
        } else if !line.starts_with("//") {
            break;
        }
    }
    attributes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_covers_common_layouts_and_excludes_fixtures_and_data() {
        for path in [
            "crates/a/tests/flow.rs",
            "crates/a/tests/ci_cli/acceptance_fix.rs",
            "evals/test_kit.py",
            "pkg/thing_test.go",
            "web/app.test.mjs",
            "web/app.spec.ts",
            "src/main/java/ThingTest.java",
            "crates/a/src/module/tests.rs",
        ] {
            assert!(in_test_tree(path), "{path}");
        }
        for path in [
            "crates/a/src/lib.rs",
            "crates/a/tests/fixtures/flow.rs",
            "tests/journey_gate.toml",
            "tests/README.md",
            "docs/testing.py",
            "latest.py",
        ] {
            assert!(!in_test_tree(path), "{path}");
        }
    }

    #[test]
    fn rust_names_match_only_declared_tests_and_test_modules() {
        let source = "\
// fn mentioned_in_a_comment() {}
const NOTE: &str = \"fn mentioned_in_a_string\";
fn helper() {}

#[cfg(test)]
mod tests {
    /// Documented.
    #[test]
    fn first_case() {}

    #[tokio::test]
    pub(crate) async fn second_case() {}
}
mod not_tests {}

#[cfg(not(test))]
mod production {}

#[cfg(test)]
fn test_helper() {}

#[cfg(all(test, unix))]
mod unix_tests {}
";
        for name in [
            "tests",
            "first_case",
            "tests::first_case",
            "tests::second_case",
            "unix_tests",
        ] {
            assert!(rust_test_defined(source, name, false), "{name}");
        }
        for name in [
            "helper",
            "first",
            "not_tests",
            "mentioned_in_a_comment",
            "mentioned_in_a_string",
            "not_tests::first_case",
            "absent::first_case",
            "production",
            "test_helper",
        ] {
            assert!(!rust_test_defined(source, name, false), "{name}");
        }
        assert!(rust_test_defined(source, "not_tests", true));
    }

    #[test]
    fn unicode_whitespace_around_a_reference_is_refused() {
        assert!(parse_evidence("test: tests/flow.rs").is_ok());
        assert!(parse_evidence("test:\u{a0}tests/flow.rs").is_err());
        assert!(parse_evidence("record: docs/verification/run.md\u{a0}").is_err());
    }

    #[test]
    fn other_languages_match_whole_words_only() {
        let source = "def test_create_user():\n    pass\n";
        assert!(has_word(source, "test_create_user"));
        assert!(!has_word(source, "test_create"));
        assert!(!has_word(source, "create_user"));
    }
}
