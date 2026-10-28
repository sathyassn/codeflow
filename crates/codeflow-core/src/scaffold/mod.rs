//! The scaffold engine — `codeflow init` / `codeflow update` (charter §4, §10, D22).
//!
//! This is the trust-critical core of the product: it writes files into a
//! user's repository and must never destroy their work. Three invariants hold
//! everywhere in this module:
//!
//! 1. **Never clobber.** An existing file is only overwritten when codeflow
//!    can prove it is unmodified (hash matches the installed record) or the
//!    user passed `--force`. Conflicting updates land next to the file as
//!    `<path>.new`, never on top of it.
//! 2. **Never silent.** Every file the engine touched, skipped, merged, or
//!    refused to touch appears in the printed report.
//! 3. **Ownership classes drive behavior** (charter §4.3):
//!    fully-managed (hash + baseline + 3-way merge), managed-region (marked
//!    block / known JSON keys only), user-owned (write once; additive new-key
//!    sync for schema-versioned JSON).
//!
//! Assets are resolved through [`assets::AssetSource`] so the engine is
//! independent of how they are shipped: the CLI provides a rust-embed source
//! (disk in debug builds, embedded in release); tests provide a directory
//! source over fixtures.

pub mod assets;
pub mod detect;
pub mod diff_redact;
pub mod init;
pub mod json_edit;
pub mod manifest;
pub mod portal;
pub mod pr_template;
pub mod prior_release;
pub mod region;
pub mod release_pin;
pub mod report;
pub mod rule_map;
pub mod settings_merge;
pub mod state;
pub mod template;
pub mod update;

mod gitutil;
mod hash;
pub(crate) mod version;

/// The starter ADR is useful only when `CodeFlow` is creating a project's first
/// decision record. Brownfield repositories already have an architecture and
/// an ADR sequence; adding another `ADR-0001` would make the doc graph invalid
/// and invent a decision after the fact.
const INITIAL_STACK_ADR: &str = "docs/decisions/ADR-0001-stack-choice.md";

fn should_skip_initial_stack_adr(
    root: &std::path::Path,
    dest: &str,
) -> Result<bool, ScaffoldError> {
    if dest != INITIAL_STACK_ADR || path_exists(&root.join(dest))? {
        return Ok(false);
    }

    let decisions = root.join("docs/decisions");
    let entries = match std::fs::read_dir(&decisions) {
        Ok(entries) => entries,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && crate::absence::proven_absent(&decisions)
                    .map_err(|error| ScaffoldError::io(&decisions, error))? =>
        {
            return Ok(false)
        }
        Err(error) => return Err(ScaffoldError::io(&decisions, error)),
    };

    for entry in entries {
        let entry = entry.map_err(|error| ScaffoldError::io(&decisions, error))?;
        if !entry
            .file_type()
            .map_err(|error| ScaffoldError::io(entry.path(), error))?
            .is_file()
        {
            continue;
        }
        if is_adr_name(&entry.file_name()) {
            return Ok(true);
        }
    }

    Ok(false)
}

fn is_adr_name(name: &std::ffi::OsStr) -> bool {
    crate::git::GitName::from_os_str(name).starts_with(b"ADR-")
        && std::path::Path::new(name)
            .extension()
            .is_some_and(|extension| extension == "md")
}

/// Existence is false only when the leaf and its ancestors prove absence.
fn path_exists(path: &std::path::Path) -> Result<bool, ScaffoldError> {
    match path
        .try_exists()
        .map_err(|error| ScaffoldError::io(path, error))?
    {
        true => Ok(true),
        false
            if crate::absence::proven_absent(path)
                .map_err(|error| ScaffoldError::io(path, error))? =>
        {
            Ok(false)
        }
        false => Err(ScaffoldError::io(
            path,
            std::io::Error::other("path exists but its target cannot be resolved"),
        )),
    }
}

pub use assets::{AssetSource, DirSource};
pub use hash::sha256_hex;
pub use init::{init, InitAnswers, InitOptions};
pub use manifest::{ManifestEntry, Ownership, RegionFormat, ScaffoldManifest, Tier};
pub use report::{Action, FileReport, Report};
pub use update::{update, UpdateOptions};
pub use version::version_skew_warning;

/// Errors produced by the scaffold engine.
#[derive(Debug, thiserror::Error)]
pub enum ScaffoldError {
    #[error("io error on {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("scaffold manifest missing from assets ({0})")]
    ManifestMissing(String),
    #[error("scaffold manifest invalid: {0}")]
    ManifestInvalid(String),
    #[error("refusing to follow a symlink at {path} — scaffold IO stays beneath the repo root")]
    UnsafeSymlink { path: std::path::PathBuf },
    #[error("not a codeflow project (no .codeflow/project.toml) — run `codeflow init`")]
    NotInitialized,
    #[error("invalid {what}: {detail}")]
    InvalidState { what: String, detail: String },
    #[error("git: {0}")]
    Git(String),
    /// A JSON error from a file the project writes; any value the parser
    /// quotes is replaced ([`unquoted`], TSK-1029).
    #[error("json: {}", unquoted(&.0.to_string()))]
    Json(#[from] serde_json::Error),
}

/// A parse error for a TOML file the project writes, named by its place:
/// the line and column and the parser's message with any quoted text
/// replaced. The parser's own rendering quotes the offending source line,
/// which can hold a secret, so it is never passed on (TSK-1029).
pub(crate) fn toml_error(text: &str, error: &toml::de::Error) -> String {
    let message = unquoted(error.message());
    match error.span() {
        Some(span) => {
            let before = &text[..span.start.min(text.len())];
            let line = before.matches('\n').count() + 1;
            let column = before
                .rsplit('\n')
                .next()
                .map_or(0, |tail| tail.chars().count())
                + 1;
            format!("TOML parse error at line {line}, column {column}: {message}")
        }
        None => format!("TOML parse error: {message}"),
    }
}

/// `message` with every value a parser quotes replaced by `<value>`: a span
/// in double quotes, one in single quotes that opens after a space or at
/// the start (an apostrophe inside a word is kept), and a backticked span
/// serde uses for a value or an unknown name (`unknown variant`, `unknown
/// field`, `invalid type: integer` and the like). Any other backticked name
/// is a schema name and stays. An unclosed quote hides the rest
/// (TSK-1029).
#[must_use]
pub fn unquoted(message: &str) -> String {
    /// The words serde puts before a backticked value or project-written
    /// name.
    const VALUE_BEFORE: [&str; 8] = [
        "unknown variant ",
        "unknown field ",
        "integer ",
        "floating point ",
        "boolean ",
        "char ",
        "string ",
        "map key ",
    ];
    let mut out = String::with_capacity(message.len());
    let mut quote: Option<char> = None;
    let mut previous = ' ';
    for c in message.chars() {
        match quote {
            Some(open) if c == open => quote = None,
            Some(_) => {}
            None if c == '"' || (c == '\'' && previous == ' ') => {
                quote = Some(c);
                out.push_str("<value>");
            }
            None if c == '`' && VALUE_BEFORE.iter().any(|word| out.ends_with(word)) => {
                quote = Some(c);
                out.push_str("<value>");
            }
            None => out.push(c),
        }
        previous = c;
    }
    out
}

impl ScaffoldError {
    fn io(path: impl Into<std::path::PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

// Every case needs a Unix file name or link, so the module is Unix only.
#[cfg(all(test, unix))]
mod r22_tests {
    use super::*;

    #[test]
    fn r22_initial_adr_recognizes_non_utf8_names() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let decisions = dir.path().join("docs/decisions");
        std::fs::create_dir_all(&decisions).unwrap();
        assert!(!should_skip_initial_stack_adr(dir.path(), INITIAL_STACK_ADR).unwrap());
        let name = std::ffi::OsStr::from_bytes(b"ADR-0002-\xff.md");
        assert!(is_adr_name(name));
        assert!(!is_adr_name(std::ffi::OsStr::from_bytes(b"other-\xff.md")));
        // This macOS test environment rejects non-UTF-8 file names. Exercise creation on Linux.
        #[cfg(target_os = "linux")]
        {
            std::fs::write(decisions.join(name), "decision").unwrap();
            assert!(should_skip_initial_stack_adr(dir.path(), INITIAL_STACK_ADR).unwrap());
        }
    }

    #[test]
    fn r22_initial_adr_refuses_dangling_decisions() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!should_skip_initial_stack_adr(dir.path(), INITIAL_STACK_ADR).unwrap());
        std::fs::create_dir(dir.path().join("docs")).unwrap();
        std::os::unix::fs::symlink("missing", dir.path().join("docs/decisions")).unwrap();
        assert!(should_skip_initial_stack_adr(dir.path(), INITIAL_STACK_ADR).is_err());
    }

    #[test]
    fn r22_scaffold_existence_requires_resolvable_paths() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!path_exists(&dir.path().join("missing/leaf")).unwrap());
        std::fs::write(dir.path().join("file"), "existing").unwrap();
        assert!(path_exists(&dir.path().join("file")).unwrap());
        assert!(path_exists(&dir.path().join("file/leaf")).is_err());
        std::os::unix::fs::symlink("missing", dir.path().join("link")).unwrap();
        assert!(path_exists(&dir.path().join("link")).is_err());
        assert!(path_exists(&dir.path().join("link/leaf")).is_err());
    }
}

#[cfg(test)]
mod parse_error_tests {
    use super::{toml_error, unquoted};

    /// A synthetic value no message may show.
    const SECRET: &str = "opaque-neighbour-value-31337";

    #[test]
    fn a_toml_error_names_its_place_never_the_source_line() {
        let text = format!("a = 1\nbroken = \"{SECRET}\n");
        let error = toml::from_str::<toml::Table>(&text).unwrap_err();
        assert!(
            error.to_string().contains(SECRET),
            "the parser quotes the line"
        );
        let shown = toml_error(&text, &error);
        assert!(!shown.contains(SECRET), "{shown}");
        assert!(
            shown.starts_with("TOML parse error at line 2, column "),
            "{shown}"
        );
    }

    #[test]
    fn a_typed_error_loses_its_quoted_value() {
        let error = serde_json::from_str::<bool>(&format!("\"{SECRET}\"")).unwrap_err();
        let shown = unquoted(&error.to_string());
        assert!(!shown.contains(SECRET), "{shown}");
        assert!(
            shown.contains("<value>") && shown.contains("line 1"),
            "{shown}"
        );
        assert_eq!(unquoted("can't read `key`"), "can't read `key`");
        assert_eq!(unquoted("bad 'x' here"), "bad <value> here");
    }
}
