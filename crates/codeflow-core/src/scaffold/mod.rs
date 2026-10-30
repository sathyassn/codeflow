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
pub mod shown;
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
pub use shown::Shown;
pub use update::{update, UpdateOptions};
pub use version::version_skew_warning;

/// Errors produced by the scaffold engine. Every text a variant holds is a
/// [`Shown`], a path or a [`ParsePlace`], and no variant keeps a source:
/// [`std::error::Error::source`] is `None`, and `Debug` writes the same text
/// as `Display`, so neither an error chain nor a `{:?}` can carry what a
/// project wrote (TSK-1029, the display boundary in [`shown`]).
#[derive(thiserror::Error)]
pub enum ScaffoldError {
    /// An I/O failure, shown by its path and the failure's category. The
    /// `io::Error` itself is not kept: a custom error's message can hold any
    /// text, and a field named `source` would become the error's source.
    #[error("io error on {}: {kind}", .path.display())]
    Io {
        path: std::path::PathBuf,
        kind: Shown,
    },
    #[error("scaffold manifest missing from assets ({0})")]
    ManifestMissing(Shown),
    #[error("scaffold manifest invalid: {0}")]
    ManifestInvalid(Shown),
    #[error("refusing to follow a symlink at {} — scaffold IO stays beneath the repo root", .path.display())]
    UnsafeSymlink { path: std::path::PathBuf },
    #[error("not a codeflow project (no .codeflow/project.toml) — run `codeflow init`")]
    NotInitialized,
    #[error("invalid {what}: {detail}")]
    InvalidState { what: Shown, detail: Shown },
    #[error("git: {0}")]
    Git(Shown),
    /// A JSON file the project writes did not parse. Only the place is
    /// kept: the parser's own error is dropped at the conversion, so
    /// neither this error's text nor its `Debug` form nor an error chain
    /// can carry what the project wrote (TSK-1029).
    #[error("{0}")]
    Json(ParsePlace),
}

impl From<serde_json::Error> for ScaffoldError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(ParsePlace::json(&error))
    }
}

/// Where a file the project writes failed to parse: its format, the kind
/// of failure, and a line and column. It holds nothing else, and has no
/// source, because a parser's message and rendering quote keys, values and
/// source lines the project wrote, and a value can hold any delimiter the
/// message would put around it (TSK-1029).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsePlace {
    /// `JSON` or `TOML`.
    pub format: &'static str,
    /// A fixed description of the failure, never parser text.
    pub kind: &'static str,
    /// The 1-based line and column, when the parser gave a position.
    pub position: Option<(usize, usize)>,
}

impl ParsePlace {
    /// The place of a JSON error.
    #[must_use]
    pub fn json(error: &serde_json::Error) -> Self {
        let kind = match error.classify() {
            serde_json::error::Category::Io => "read error",
            serde_json::error::Category::Syntax => "syntax error",
            serde_json::error::Category::Data => {
                "error: a key or value does not fit the expected shape"
            }
            serde_json::error::Category::Eof => "error: the input ends early",
        };
        let position = (error.line() > 0).then(|| (error.line(), error.column()));
        Self {
            format: "JSON",
            kind,
            position,
        }
    }

    /// The place of a TOML error in `text`.
    #[must_use]
    pub fn toml(text: &str, error: &toml::de::Error) -> Self {
        let position = error
            .span()
            .and_then(|span| text.get(..span.start))
            .map(|before| {
                let line = before.matches('\n').count() + 1;
                let column = before
                    .rsplit('\n')
                    .next()
                    .map_or(0, |tail| tail.chars().count())
                    + 1;
                (line, column)
            });
        Self {
            format: "TOML",
            kind: "parse error",
            position,
        }
    }
}

impl std::fmt::Display for ParsePlace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.format, self.kind)?;
        match self.position {
            Some((line, column)) => write!(f, " at line {line}, column {column}"),
            None => write!(f, " (the parser gave no position)"),
        }
    }
}

/// A TOML error for a file the project writes, named by its line and
/// column only ([`ParsePlace`], TSK-1029).
pub(crate) fn toml_error(text: &str, error: &toml::de::Error) -> String {
    ParsePlace::toml(text, error).to_string()
}

/// A JSON error for a file the project writes, named by its kind and its
/// line and column only ([`ParsePlace`], TSK-1029).
#[must_use]
pub fn json_error(error: &serde_json::Error) -> String {
    ParsePlace::json(error).to_string()
}

impl ScaffoldError {
    /// An invalid file or value: `what` names it, `detail` says why.
    #[must_use]
    pub fn invalid(what: Shown, detail: Shown) -> Self {
        Self::InvalidState { what, detail }
    }

    // Taken by value so `map_err(|e| ScaffoldError::io(path, e))` reads
    // plainly; only the category is kept.
    #[allow(clippy::needless_pass_by_value)]
    fn io(path: impl Into<std::path::PathBuf>, error: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            kind: Shown::io(&error),
        }
    }
}

impl std::fmt::Debug for ScaffoldError {
    /// The same text as `Display`: every field is inside the boundary, and
    /// a derived form would print each field's own `Debug`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ScaffoldError({self})")
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
    use super::{json_error, toml_error, ScaffoldError};

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
        let shown = json_error(&error);
        assert!(!shown.contains(SECRET), "{shown}");
        assert_eq!(
            shown,
            "JSON error: a key or value does not fit the expected shape at line 1, column 30"
        );
        let early = serde_json::from_str::<Vec<bool>>("[").unwrap_err();
        assert_eq!(
            json_error(&early),
            "JSON error: the input ends early at line 1, column 1"
        );
        let syntax = serde_json::from_str::<Vec<bool>>("[tru]").unwrap_err();
        assert!(
            json_error(&syntax).starts_with("JSON syntax error at line 1, column "),
            "{}",
            json_error(&syntax)
        );
    }

    /// A shape with a closed set of keys and one enum, as the project
    /// files update reads have.
    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Shape {
        mode: Option<Mode>,
        flag: Option<bool>,
    }

    #[derive(Debug, serde::Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum Mode {
        Block,
        Warn,
    }

    /// Everything an error chain can print: its text, its `Debug` form and
    /// every source's text, as the CLI prints a chain.
    fn chain(error: &dyn std::error::Error) -> String {
        use std::fmt::Write as _;
        let mut shown = format!("{error}\n{error:?}");
        let mut source = error.source();
        while let Some(cause) = source {
            let _ = write!(shown, "\nCaused by: {cause} {cause:?}");
            source = cause.source();
        }
        shown
    }

    /// Why `shown` fails, if it shows the value or does not name its line.
    fn misplaced(shown: &str, line: usize) -> Option<String> {
        if shown.contains(SECRET) {
            Some(format!("a project value is shown: {shown}"))
        } else if !shown.contains(&format!("line {line}, column ")) {
            Some(format!("the diagnostic does not name its place: {shown}"))
        } else {
            None
        }
    }

    /// TSK-1029 review: values a project writes can hold the delimiters
    /// the parser puts around them, so no message may depend on finding
    /// where a quoted value ends.
    #[test]
    fn adversarial_json_values_and_keys_never_show() {
        let mut failures = Vec::new();
        for (text, line) in [
            // An escaped double quote inside a string value.
            (format!("\n{{\"flag\": \"a\\\" {SECRET}\"}}"), 2),
            // A backtick inside an enum value serde puts in backticks.
            (format!("{{\"mode\": \"x` {SECRET}\"}}"), 1),
            // A backtick inside a key the project wrote.
            (format!("{{\"x` {SECRET}\": 1}}"), 1),
            // A single quote and a space inside an enum value.
            (format!("{{\"mode\": \"it' {SECRET}\"}}"), 1),
        ] {
            let error = serde_json::from_str::<Shape>(&text).unwrap_err();
            assert!(error.to_string().contains(SECRET), "{error}");
            failures.extend(misplaced(&chain(&ScaffoldError::from(error)), line));
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn adversarial_toml_values_and_keys_never_show() {
        let mut failures = Vec::new();
        for (text, line) in [
            // A duplicate key the project wrote.
            (format!("{SECRET} = 1\n{SECRET} = 2\n"), 2),
            // A backtick inside an enum value.
            (format!("mode = \"x` {SECRET}\"\n"), 1),
            // A backtick inside a quoted key the project wrote.
            (format!("\"x` {SECRET}\" = 1\n"), 1),
            // An escaped quote inside a string of the wrong type.
            (format!("\nflag = \"a\\\" {SECRET}\"\n"), 2),
        ] {
            let error = toml::from_str::<Shape>(&text).unwrap_err();
            assert!(error.to_string().contains(SECRET), "{error}");
            failures.extend(misplaced(&toml_error(&text, &error), line));
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// TSK-1029 review: an I/O error's message is any text its maker
    /// chose, so neither the error, its `Debug` form nor its source shows
    /// it; a kept PR template's headings and a report holding one stay out
    /// of `Debug` too.
    #[test]
    fn io_errors_and_kept_templates_never_show_their_text_in_debug() {
        use super::pr_template::KeptTemplate;
        use std::error::Error as _;
        let error = ScaffoldError::io(
            "docs/x.md",
            std::io::Error::other(format!("custom {SECRET}")),
        );
        assert!(error.source().is_none(), "the io::Error is not kept");
        let shown = chain(&error);
        assert!(!shown.contains(SECRET), "{shown}");
        assert!(shown.contains("docs/x.md"), "{shown}");
        let kept = KeptTemplate {
            path: ".github/pull_request_template.md".into(),
            headings: vec![format!("## {SECRET}")],
        };
        let mut report = super::Report::new(super::Shown::fixed("update"));
        report.pending_pr_template = Some(kept.clone());
        for debug in [format!("{kept:?}"), format!("{report:?}")] {
            assert!(!debug.contains(SECRET), "{debug}");
            assert!(debug.contains("headings: 1"), "{debug}");
        }
    }

    /// TSK-1029 review: the other public report and error types on the
    /// update path keep project text out of `Debug` too.
    #[test]
    fn update_report_and_error_types_never_show_project_text_in_debug() {
        use super::pr_template::Diagnosis;
        let unparseable = super::version::Unparseable(SECRET.into());
        let unreadable = crate::registry::RootUnreadable {
            path: "proj/.codeflow".into(),
            error: std::io::Error::other(format!("custom {SECRET}")),
        };
        let diagnosis = Diagnosis::Diagnosed {
            mapping: [("Summary".to_string(), format!("## {SECRET}"))].into(),
            unmatched: vec![SECRET.into()],
        };
        for debug in [
            format!("{unparseable:?}"),
            format!("{unreadable:?}"),
            format!("{diagnosis:?}"),
        ] {
            assert!(!debug.contains(SECRET), "{debug}");
        }
    }
}
