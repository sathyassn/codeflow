//! Trusted-base project checks (TSK-1103): the project-owned declaration
//! `.codeflow/base-checks.json`, which names checks the managed CI runs from
//! the trusted commit's copies before it judges a pull request, and the
//! project-owned CI inputs `codeflow ci` notes when a range edits them.
//!
//! `codeflow update` never writes the declaration. Its reader is strict: a
//! duplicate key, an unknown key, a path that leaves the repository, a name
//! used twice or a timeout outside its bounds is refused, so the step that
//! runs the checks fails closed on a declaration it cannot read exactly.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

/// The declaration's path from the project root.
pub const DECLARATION: &str = ".codeflow/base-checks.json";
/// The dependency-audit exception hook TSK-294 runs from the trusted commit.
pub const AUDIT_EXCEPTIONS_HOOK: &str = ".codeflow/audit-exceptions.sh";
/// The timeout a check gets when it declares none.
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 300;
/// The longest timeout a check may declare; the GitHub verdict job itself
/// stops at 20 minutes.
pub const MAX_TIMEOUT_SECONDS: u64 = 900;
/// The only declaration format version.
pub const VERSION: u32 = 1;

/// Git file modes as `git ls-tree` prints them.
pub const MODE_FILE: i32 = 0o100_644;
/// An executable regular file.
pub const MODE_EXECUTABLE: i32 = 0o100_755;
/// A folder.
pub const MODE_TREE: i32 = 0o040_000;
/// A symbolic link.
pub const MODE_LINK: i32 = 0o120_000;
/// A submodule commit (gitlink).
pub const MODE_GITLINK: i32 = 0o160_000;

/// The parsed declaration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    /// The format version; only [`VERSION`] is read.
    pub version: u32,
    /// The checks, run in this order.
    #[serde(default)]
    pub checks: Vec<Check>,
    /// Files the project's dependency-audit exception hook reads, such as
    /// its register, so `codeflow ci` can name a change to them.
    #[serde(default)]
    pub audit_exception_inputs: Vec<String>,
}

/// One declared check.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    /// A short name, lowercase letters, digits and hyphens.
    pub name: String,
    /// The script, a path from the project root.
    pub script: String,
    /// The interpreter, a bare command name found on PATH (`bash`, `sh`,
    /// `python3`, `node`).
    pub interpreter: String,
    /// Files or folders the script reads, exported from the same commit.
    #[serde(default)]
    pub inputs: Vec<String>,
    /// Seconds before the check is stopped and fails.
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

impl Check {
    /// The timeout this check runs under.
    #[must_use]
    pub fn timeout(&self) -> u64 {
        self.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS)
    }
}

impl Declaration {
    /// Every path a declared check runs from: each script, then its inputs.
    #[must_use]
    pub fn check_paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        for check in &self.checks {
            paths.push(check.script.clone());
            paths.extend(check.inputs.iter().cloned());
        }
        paths
    }
}

/// Parse and validate a declaration's bytes.
///
/// # Errors
/// A message naming the first problem: invalid JSON, a duplicate or unknown
/// key, another version, or a field the rules below refuse.
pub fn parse(bytes: &[u8]) -> Result<Declaration, String> {
    let declaration: Declaration =
        crate::strict_json::parse_strict_json(bytes).map_err(|error| error.to_string())?;
    if declaration.version != VERSION {
        return Err(format!(
            "version {} is not one this codeflow reads (only {VERSION})",
            declaration.version
        ));
    }
    let mut names = BTreeSet::new();
    for check in &declaration.checks {
        let name = &check.name;
        let valid_name = !name.is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !name.starts_with('-');
        if !valid_name {
            return Err(format!(
                "check name {name:?} is not 1 to 64 lowercase letters, digits and hyphens"
            ));
        }
        if !names.insert(name.as_str()) {
            return Err(format!("check name {name:?} is used twice"));
        }
        repo_path(&check.script).map_err(|why| format!("check {name}: script {why}"))?;
        for input in &check.inputs {
            repo_path(input).map_err(|why| format!("check {name}: input {why}"))?;
        }
        let interpreter = &check.interpreter;
        if interpreter.is_empty()
            || !interpreter
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
        {
            return Err(format!(
                "check {name}: interpreter {interpreter:?} is not a bare command name such as bash or python3"
            ));
        }
        if let Some(seconds) = check.timeout_seconds {
            if seconds == 0 || seconds > MAX_TIMEOUT_SECONDS {
                return Err(format!(
                    "check {name}: timeout_seconds {seconds} is not between 1 and {MAX_TIMEOUT_SECONDS}"
                ));
            }
        }
    }
    for input in &declaration.audit_exception_inputs {
        repo_path(input).map_err(|why| format!("audit_exception_inputs: {why}"))?;
    }
    Ok(declaration)
}

/// Refuse a path that is not a plain relative path inside the repository.
fn repo_path(path: &str) -> Result<(), String> {
    let refuse = |why: &str| Err(format!("{path:?} {why}"));
    if path.is_empty() {
        return refuse("is empty");
    }
    if path.starts_with('/') || path.contains('\\') || path.contains(':') {
        return refuse("is not a relative path written with forward slashes");
    }
    if path.chars().any(char::is_control) {
        return refuse("holds a control character");
    }
    if path
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return refuse("has an empty, `.` or `..` part");
    }
    if path == ".git" || path.starts_with(".git/") {
        return refuse("is inside .git");
    }
    Ok(())
}

/// The mode and object id of `path` in `tree`, or `None` when the tree holds
/// nothing there. A parent that is not a folder reads as absent, as git
/// reads it.
///
/// # Errors
/// A tree git cannot read.
pub fn entry_in(
    tree: &git2::Tree<'_>,
    path: &str,
) -> Result<Option<(i32, git2::Oid)>, String> {
    match tree.get_path(Path::new(path)) {
        Ok(entry) => Ok(Some((entry.filemode(), entry.id()))),
        Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(error) => Err(format!("cannot read {path}: {}", error.message())),
    }
}

/// The tree of commit `rev` in `repo`.
///
/// # Errors
/// A revision that does not name a readable commit.
pub fn tree_at<'r>(repo: &'r git2::Repository, rev: &str) -> Result<git2::Tree<'r>, String> {
    repo.revparse_single(rev)
        .and_then(|object| object.peel_to_commit())
        .and_then(|commit| commit.tree())
        .map_err(|error| format!("cannot read commit {rev}: {}", error.message()))
}

/// The declaration a commit holds: `None` when it holds none.
///
/// # Errors
/// A declaration that is not a regular file (a symbolic link, a folder or a
/// gitlink), that cannot be read, or that [`parse`] refuses.
pub fn read_in(repo: &git2::Repository, tree: &git2::Tree<'_>) -> Result<Option<Declaration>, String> {
    let Some((mode, id)) = entry_in(tree, DECLARATION)? else {
        return Ok(None);
    };
    if mode != MODE_FILE && mode != MODE_EXECUTABLE {
        return Err(format!("{DECLARATION} is not a regular file ({})", describe_mode(mode)));
    }
    let blob = repo
        .find_blob(id)
        .map_err(|error| format!("cannot read {DECLARATION}: {}", error.message()))?;
    parse(blob.content())
        .map(Some)
        .map_err(|why| format!("{DECLARATION} cannot be read: {why}"))
}

/// The declaration in the working tree at `root`, for `codeflow doctor`.
///
/// # Errors
/// A declaration that is a link or not a regular file, cannot be read, or
/// that [`parse`] refuses.
pub fn read_worktree(root: &Path) -> Result<Option<Declaration>, String> {
    let path = root.join(DECLARATION);
    match crate::absence::proven_absent(&path) {
        Ok(true) => return Ok(None),
        Ok(false) => {}
        Err(error) => return Err(format!("cannot inspect {DECLARATION}: {error}")),
    }
    let text = crate::bounded_file::read_regular_to_string(&path)
        .map_err(|error| format!("cannot read {DECLARATION}: {error}"))?;
    parse(text.as_bytes())
        .map(Some)
        .map_err(|why| format!("{DECLARATION} cannot be read: {why}"))
}

/// A mode in words, for a refusal.
#[must_use]
pub fn describe_mode(mode: i32) -> &'static str {
    match mode {
        MODE_LINK => "a symbolic link",
        MODE_GITLINK => "a gitlink (submodule)",
        MODE_TREE => "a folder",
        MODE_FILE | MODE_EXECUTABLE => "a regular file",
        _ => "an entry of an unknown kind",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"version": 1, "checks": [{"name": "boundary", "script": ".codeflow/checks/boundary.sh", "interpreter": "bash", "inputs": ["docs/boundary.txt"], "timeout_seconds": 60}], "audit_exception_inputs": ["docs/register.csv"]}"#;

    #[test]
    fn a_declaration_parses_with_its_defaults() {
        let declaration = parse(GOOD.as_bytes()).unwrap();
        assert_eq!(declaration.checks[0].timeout(), 60);
        assert_eq!(
            declaration.check_paths(),
            vec![".codeflow/checks/boundary.sh", "docs/boundary.txt"]
        );
        let bare = parse(br#"{"version": 1}"#).unwrap();
        assert!(bare.checks.is_empty());
        let one = parse(br#"{"version": 1, "checks": [{"name": "a", "script": "a.sh", "interpreter": "sh"}]}"#)
            .unwrap();
        assert_eq!(one.checks[0].timeout(), DEFAULT_TIMEOUT_SECONDS);
    }

    #[test]
    fn a_declaration_it_cannot_read_exactly_is_refused() {
        for (text, why) in [
            ("{", "EOF"),
            (r#"{"version": 2}"#, "version 2"),
            (r#"{"version": 1, "version": 1}"#, "duplicate"),
            (r#"{"version": 1, "extra": 1}"#, "unknown field"),
            (r#"{"version": 1, "checks": [{"name": "A", "script": "a", "interpreter": "sh"}]}"#, "check name"),
            (r#"{"version": 1, "checks": [{"name": "a", "script": "a", "interpreter": "sh"}, {"name": "a", "script": "b", "interpreter": "sh"}]}"#, "used twice"),
            (r#"{"version": 1, "checks": [{"name": "a", "script": "../a", "interpreter": "sh"}]}"#, "`..`"),
            (r#"{"version": 1, "checks": [{"name": "a", "script": "/etc/a", "interpreter": "sh"}]}"#, "relative"),
            (r#"{"version": 1, "checks": [{"name": "a", "script": ".git/hooks/x", "interpreter": "sh"}]}"#, "inside .git"),
            (r#"{"version": 1, "checks": [{"name": "a", "script": "a", "interpreter": "/bin/sh"}]}"#, "bare command"),
            (r#"{"version": 1, "checks": [{"name": "a", "script": "a", "interpreter": "sh", "timeout_seconds": 0}]}"#, "between 1"),
            (r#"{"version": 1, "checks": [{"name": "a", "script": "a", "interpreter": "sh", "timeout_seconds": 901}]}"#, "between 1"),
            (r#"{"version": 1, "audit_exception_inputs": ["a//b"]}"#, "empty"),
        ] {
            let error = parse(text.as_bytes()).unwrap_err();
            assert!(error.contains(why), "{text}: {error}");
        }
    }
}
