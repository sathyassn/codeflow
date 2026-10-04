//! `codeflow update --pin <version>`: the reviewed release digest beside the
//! pinned version (sathyassn/codeflow#47).
//!
//! The managed CI installers download the release the target pins in
//! `.codeflow/project.toml` (`scaffold_version`). The release's own
//! `sha256.sum` proves a download was not corrupted, but anyone who can
//! replace a release asset can replace that file too. This writes, next to
//! the version, a `[scaffold_sha256]` table that records the digest of each
//! release archive and the version they belong to:
//!
//! ```toml
//! [scaffold_sha256]
//! aarch64-apple-darwin = "<sha256>"
//! version = "3.1.0"
//! x86_64-apple-darwin = "<sha256>"
//! x86_64-unknown-linux-gnu = "<sha256>"
//! ```
//!
//! The table lands in a reviewed pull request with the version, and from then
//! on the installers require the archive to match it (a stale version, a
//! missing triple or another digest fails closed). The digests come from a
//! release whose archives this downloads and checks against its
//! `sha256.sum`; nothing else in the project changes.

use std::fmt;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::hash::sha256_hex;
use super::state::{read_beneath_root, state_text, write_record, PROJECT_TOML};
use super::version::is_older;

/// The release triples `CodeFlow` publishes for macOS and Linux, the ones the
/// CI installers download, in the order the table lists them.
pub const TRIPLES: [&str; 3] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
];

/// The table in `.codeflow/project.toml` that pins the digests.
pub const TABLE: &str = "scaffold_sha256";

/// Where releases are downloaded from unless `CODEFLOW_RELEASE_URL` says
/// otherwise, as in the CI installers.
pub const DEFAULT_RELEASE_URL: &str = "https://github.com/sathyassn/codeflow/releases/download";

/// What `codeflow update --pin` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinReport {
    /// The version pinned before, if any.
    pub previous: Option<String>,
    /// The version now pinned.
    pub version: String,
    /// Each triple and the digest pinned for it.
    pub digests: Vec<(String, String)>,
}

impl fmt::Display for PinReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.previous {
            Some(previous) if previous != &self.version => writeln!(
                f,
                "codeflow update --pin: scaffold_version {previous} -> {} in {PROJECT_TOML}",
                self.version
            )?,
            _ => writeln!(
                f,
                "codeflow update --pin: scaffold_version {} in {PROJECT_TOML}",
                self.version
            )?,
        }
        writeln!(
            f,
            "  [{TABLE}] pins the release archives, each checked against the release's sha256.sum:"
        )?;
        for (triple, digest) in &self.digests {
            writeln!(f, "    {triple} = {digest}")?;
        }
        writeln!(
            f,
            "  next: review the digests and land this change on its own; CI then installs only these archives"
        )?;
        write!(
            f,
            "  then: with codeflow {} installed, run `codeflow update` on a new branch",
            self.version
        )
    }
}

/// The digest table a project state pins, as read from its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinnedDigests {
    /// No `[scaffold_sha256]` table.
    Absent,
    /// A table whose `version` is `version`, with a valid digest for each
    /// triple in `listed` and none for each in `missing`.
    Table {
        version: Option<String>,
        listed: Vec<&'static str>,
        missing: Vec<&'static str>,
    },
    /// A table the installers refuse to read, with the reason: declared
    /// twice, a key listed twice, or written in a form they do not read.
    Unreadable(String),
}

/// Why the installers refuse a table: it is declared twice.
pub const TWICE: &str = "is declared twice";

/// Why the installers refuse a state with an escaped key or table name: a
/// backslash escape for a letter could spell the table's name without that
/// letter, so they cannot tell the table is absent.
pub const ESCAPED: &str =
    "may be hidden behind an escaped key, which the CI installers do not read";

/// Why the installers refuse a state with a multi-line string: its lines
/// could read as a table or an entry that TOML does not declare.
pub const MULTILINE: &str =
    "may be hidden by a multi-line string, which the CI installers do not read";

/// Why the installers refuse a line whose first character after spaces and
/// tabs is not printable ASCII: a byte-order mark, a no-break or other
/// Unicode space or a control character in front of the table's header
/// would otherwise hide it from a line reader that reads bytes.
pub const UNREAD_START: &str =
    "may be hidden by a line that starts with a character the CI installers do not read";

/// Why the installers refuse a control character other than a tab
/// anywhere: TOML allows none, and one inside a header could hide it.
pub const UNREAD_CONTROL: &str =
    "may be hidden by a control character, which the CI installers do not read";

/// Why the installers refuse a header or key name holding a character
/// outside printable ASCII: an invisible one inside the table's name would
/// otherwise make the header read as another table.
pub const NON_ASCII_NAME: &str = "may be hidden behind a name with a character outside printable ASCII, which the CI installers do not read";

/// Why the installers refuse a carriage return anywhere but at a line's
/// end: awk reads only a line feed as a line break, so bare carriage
/// returns fold the file into one line it would read as holding no table.
pub const UNREAD_CR: &str = "may be hidden by a carriage return inside a line, which the CI installers do not read as a line break";

/// Why the installers refuse a table holding anything but plain entries.
pub const STRAY: &str =
    "holds a line other than key = \"value\", which the CI installers do not read";

/// Why the installers refuse a table written another way.
pub const OTHER_FORM: &str = "is written in a form the CI installers do not read (a quoted header, an inline or dotted table, or a sub-table)";

/// The `[scaffold_sha256]` header the installers read: the bare name in
/// brackets, spaces or tabs around it, and an optional trailing comment.
static HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[[:space:]]*\[[[:space:]]*scaffold_sha256[[:space:]]*\][[:space:]]*(#.*)?$")
        .expect("header pattern")
});

/// Whether `line` is the digest table's header as the installers read it.
#[must_use]
pub fn is_table_header(line: &str) -> bool {
    HEADER.is_match(line)
}

/// The first checks the installers' byte reader makes on a line: a refusal
/// reason, `None` for a full-line comment, or the line without its leading
/// POSIX `[[:space:]]` (the C locale the installers run awk in).
fn byte_line(line: &str) -> Result<Option<&str>, &'static str> {
    // `lines` drops a carriage return before the line feed, as the awk rule
    // `/\r[^\n]/` allows one at the end.
    let body = line.strip_suffix('\r').unwrap_or(line);
    if body.contains('\r') {
        return Err(UNREAD_CR);
    }
    // Any other control character but a tab, a NUL included (awk sees it
    // as byte 1).
    if body.chars().any(|c| c.is_ascii_control() && c != '\t') {
        return Err(UNREAD_CONTROL);
    }
    let trimmed = line.trim_start_matches([' ', '\t', '\n', '\x0B', '\x0C', '\r']);
    if trimmed.starts_with('#') {
        return Ok(None);
    }
    // A NUL reaches awk as byte 1, which this rule refuses too.
    if trimmed
        .chars()
        .next()
        .is_some_and(|c| !c.is_ascii_graphic())
    {
        return Err(UNREAD_START);
    }
    Ok(Some(trimmed))
}

/// The first line at which the installers stop reading `state` as a digest
/// table they can use: its number from 1, and a label naming it with its
/// escapes shown, or `None` when they read it.
#[must_use]
pub fn unread_line(state: &str) -> Option<(usize, String)> {
    let mut end = 0;
    for (index, line) in state.split_inclusive('\n').enumerate() {
        end += line.len();
        if matches!(pinned_digests(&state[..end]), PinnedDigests::Unreadable(_)) {
            let shown: String = format!("{:?}", line.trim_end_matches(['\n', '\r']))
                .chars()
                .take(120)
                .collect();
            return Some((index + 1, format!("line {} ({shown})", index + 1)));
        }
    }
    None
}

/// Reads the `[scaffold_sha256]` table from a project state's text exactly
/// as the CI installers' awk reader does, line by line, so doctor reports
/// what CI will check. The installers accept one strict form and fail
/// closed on anything else rather than reading it as absent: a
/// `[scaffold_sha256]` header, then only `key = "value"` lines (a bare or
/// double-quoted key without dots, a double-quoted value without escapes, an
/// optional trailing comment), blank lines and comment lines, up to the next
/// header. Anywhere in the state, a multi-line string, an escaped table name
/// or key, or another table name or key naming the table (a quoted header,
/// an inline or dotted table, a sub-table) is refused, as are a second
/// header and a key listed twice. A table name runs to its first `]` and a
/// key to its first `=`, so what follows them counts only when it holds
/// three quotes in a row. `codeflow update --pin` writes the form they read.
#[must_use]
pub fn pinned_digests(state: &str) -> PinnedDigests {
    static ENTRY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r#"^[[:space:]]*"?([0-9A-Za-z_-]+)"?[[:space:]]*=[[:space:]]*"[^"\\]*"[[:space:]]*(#.*)?$"#,
        )
        .expect("entry pattern")
    });
    let (header, entry) = (&*HEADER, &*ENTRY);
    let (mut inside, mut table) = (false, false);
    let mut version = None;
    let mut values = std::collections::BTreeMap::new();
    let mut bad: Option<String> = None;
    let mut flag = |reason: String| {
        bad.get_or_insert(reason);
    };
    for line in state.lines() {
        let trimmed = match byte_line(line) {
            Err(reason) => {
                flag(reason.to_string());
                continue;
            }
            Ok(None) => continue,
            Ok(Some(trimmed)) => trimmed,
        };
        if line.contains("\"\"\"") || line.contains("'''") {
            flag(MULTILINE.to_string());
        }
        if trimmed.starts_with('[') {
            inside = header.is_match(line);
            // The table's name runs to the first `]`; a comment after it
            // never counts.
            let name = line.split(']').next().unwrap_or(line);
            if inside {
                if table {
                    flag(TWICE.to_string());
                }
                table = true;
            } else {
                if name.contains(TABLE) {
                    flag(OTHER_FORM.to_string());
                }
                if name.contains('\\') {
                    flag(ESCAPED.to_string());
                }
                if !name.is_ascii() {
                    flag(NON_ASCII_NAME.to_string());
                }
            }
            continue;
        }
        // A key runs to the first `=`; its value and comment never count.
        if let Some((name, _)) = line.split_once('=') {
            if name.contains(TABLE) {
                flag(OTHER_FORM.to_string());
            }
            if name.contains('\\') {
                flag(ESCAPED.to_string());
            }
            if !name.is_ascii() {
                flag(NON_ASCII_NAME.to_string());
            }
        }
        if !inside || trimmed.is_empty() {
            continue;
        }
        let Some(key) = entry.captures(line) else {
            flag(STRAY.to_string());
            continue;
        };
        if (&key[1] == "version" && version.is_some()) || values.contains_key(&key[1]) {
            flag(format!("lists {} twice", &key[1]));
        }
        let rest = line
            .split_once('=')
            .map_or("", |(_, rest)| rest)
            .trim_start();
        let value = rest
            .strip_prefix('"')
            .and_then(|quoted| quoted.split('"').next())
            .unwrap_or_default()
            .to_string();
        match &key[1] {
            "version" => version = Some(value),
            triple => {
                values.insert(triple.to_string(), value);
            }
        }
    }
    if let Some(reason) = bad {
        return PinnedDigests::Unreadable(reason);
    }
    if !table {
        return PinnedDigests::Absent;
    }
    let version = version.filter(|v| !v.is_empty());
    let (listed, missing) = TRIPLES
        .iter()
        .partition(|triple| values.get(**triple).is_some_and(|v| is_digest(v)));
    PinnedDigests::Table {
        version,
        listed,
        missing,
    }
}

/// A lowercase hex SHA-256, as the installers accept it.
fn is_digest(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A version the installers read from `scaffold_version`: a leading digit,
/// then digits, letters, `.`, `+` and `-` only.
fn is_version(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_digit())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'))
}

/// The release URL base: `CODEFLOW_RELEASE_URL` when set, else GitHub.
#[must_use]
pub fn release_url() -> String {
    std::env::var("CODEFLOW_RELEASE_URL")
        .ok()
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| DEFAULT_RELEASE_URL.to_string())
}

/// Downloads `url` with `curl -fsSL`, as the CI installers do, so a
/// `file://` release serves as well as an `https://` one.
///
/// # Errors
///
/// The reason curl could not run or fetch the URL.
pub fn fetch_with_curl(url: &str) -> Result<Vec<u8>, String> {
    let out = std::process::Command::new("curl")
        .args(["-fsSL", "--", url])
        .output()
        .map_err(|error| format!("cannot run curl to download {url}: {error}"))?;
    if !out.status.success() {
        return Err(format!(
            "cannot download {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

/// Pins `version` and its release digests in `root`'s
/// `.codeflow/project.toml`. `fetch` downloads a URL under `base`
/// (`<base>/v<version>/<file>`). Every archive must match the release's
/// `sha256.sum`; on any refusal nothing is written.
///
/// # Errors
///
/// A malformed version, a version older than the one pinned, an unreadable
/// state, a release without `sha256.sum` or an entry for each triple, an
/// archive that cannot be downloaded or does not match its entry, or a
/// failed write.
pub fn pin_release(
    root: &Path,
    version: &str,
    base: &str,
    fetch: &dyn Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<PinReport, String> {
    if !is_version(version) {
        return Err(format!(
            "`{version}` is not a codeflow version: it starts with a digit and holds only digits, letters, `.`, `+` and `-`"
        ));
    }
    let text = read_beneath_root(root, PROJECT_TOML)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("no {PROJECT_TOML}: run `codeflow init` first"))?;
    let mut state: toml::Table =
        toml::from_str(&text).map_err(|error| format!("{PROJECT_TOML} does not parse: {error}"))?;
    let previous = state
        .get("scaffold_version")
        .and_then(toml::Value::as_str)
        .map(str::to_string);
    if let Some(previous) = previous.as_deref() {
        if is_older(version, previous) {
            return Err(format!(
                "{version} is older than the pinned {previous}; the pin only rises"
            ));
        }
    }

    let release = format!("{}/v{version}", base.trim_end_matches('/'));
    let sums = fetch(&format!("{release}/sha256.sum")).map_err(|error| {
        format!("codeflow {version} has no published checksum file (sha256.sum): {error}")
    })?;
    let sums = String::from_utf8_lossy(&sums);
    let mut digests = Vec::new();
    for triple in TRIPLES {
        let asset = format!("codeflow-cli-{triple}.tar.xz");
        let listed = sums
            .lines()
            .find_map(|line| {
                let mut fields = line.split_whitespace();
                let digest = fields.next()?;
                let name = fields.next()?.trim_start_matches('*');
                (name == asset).then(|| digest.to_ascii_lowercase())
            })
            .filter(|digest| is_digest(digest))
            .ok_or_else(|| {
                format!("sha256.sum for codeflow {version} lists no checksum for {asset}")
            })?;
        let archive = fetch(&format!("{release}/{asset}"))
            .map_err(|error| format!("cannot download {asset} for codeflow {version}: {error}"))?;
        let actual = sha256_hex(&archive);
        if actual != listed {
            return Err(format!(
                "{asset} for codeflow {version} does not match its sha256.sum entry (listed {listed}, got {actual}); nothing was pinned"
            ));
        }
        digests.push((triple.to_string(), actual));
    }

    let mut table = toml::Table::new();
    table.insert("version".into(), toml::Value::String(version.to_string()));
    for (triple, digest) in &digests {
        table.insert(triple.clone(), toml::Value::String(digest.clone()));
    }
    state.insert(
        "scaffold_version".into(),
        toml::Value::String(version.to_string()),
    );
    state.insert(TABLE.into(), toml::Value::Table(table));
    let out =
        state_text(&state).map_err(|error| format!("cannot write {PROJECT_TOML}: {error}"))?;
    // A line the project wrote that the installers refuse (a key named
    // outside printable ASCII, say) stays as it was, and would fail every
    // CI install, so the pin stops before writing and names it.
    if let PinnedDigests::Unreadable(reason) = pinned_digests(&out) {
        // Name the line in the file as it stands; only when the file reads
        // and the rewrite does not, name the line of the rewrite.
        let line = unread_line(&text).map_or_else(
            || {
                unread_line(&out).map_or_else(String::new, |(_, shown)| {
                    format!(" at {shown} of the state the pin would write")
                })
            },
            |(_, shown)| format!(" at {shown}"),
        );
        return Err(format!(
            "the [{TABLE}] table in {PROJECT_TOML} {reason}{line}; rename, rewrite or remove that line and pin again (nothing was written)"
        ));
    }
    write_record(root, PROJECT_TOML, out.as_bytes()).map_err(|error| error.to_string())?;
    Ok(PinReport {
        previous,
        version: version.to_string(),
        digests,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fmt::Write as _;

    const DIGEST: &str = "ef27f48a56c6e714773db46585f85ba1a7a4d2982dbd5bfacdcd9e81c1c85b86";

    /// A project whose state pins `version`, with one key codeflow does
    /// not model.
    fn project(version: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        std::fs::write(
            dir.path().join(PROJECT_TOML),
            format!("scaffold_version = \"{version}\"\ntier = \"minimal\"\n\n[orient]\nenabled = false\n"),
        )
        .unwrap();
        dir
    }

    /// A release of `version` served from memory: each archive's bytes and
    /// a `sha256.sum` that lists them.
    fn release(version: &str) -> BTreeMap<String, Vec<u8>> {
        let mut files = BTreeMap::new();
        let mut sums = String::new();
        for triple in TRIPLES {
            let asset = format!("codeflow-cli-{triple}.tar.xz");
            let bytes = format!("{triple} {version}").into_bytes();
            writeln!(sums, "{} *{asset}", sha256_hex(&bytes)).unwrap();
            files.insert(format!("base/v{version}/{asset}"), bytes);
        }
        files.insert(format!("base/v{version}/sha256.sum"), sums.into_bytes());
        files
    }

    fn served(files: &BTreeMap<String, Vec<u8>>) -> impl Fn(&str) -> Result<Vec<u8>, String> + '_ {
        move |url: &str| files.get(url).cloned().ok_or_else(|| format!("404 {url}"))
    }

    #[test]
    fn pins_the_version_and_every_archive_digest_and_keeps_other_keys() {
        let dir = project("1.2.3");
        let files = release("1.3.0");
        let report = pin_release(dir.path(), "1.3.0", "base", &served(&files)).unwrap();
        assert_eq!(report.previous.as_deref(), Some("1.2.3"));
        assert_eq!(report.digests.len(), 3);
        let state = std::fs::read_to_string(dir.path().join(PROJECT_TOML)).unwrap();
        assert!(state.contains("scaffold_version = \"1.3.0\""), "{state}");
        assert!(state.contains("[orient]\nenabled = false"), "{state}");
        let PinnedDigests::Table {
            version,
            listed,
            missing,
        } = pinned_digests(&state)
        else {
            panic!("{state}")
        };
        assert_eq!(version.as_deref(), Some("1.3.0"));
        assert_eq!(listed, TRIPLES.to_vec());
        assert!(missing.is_empty());
        let shown = report.to_string();
        assert!(shown.contains("scaffold_version 1.2.3 -> 1.3.0"), "{shown}");
        assert!(
            shown.contains("run `codeflow update` on a new branch"),
            "{shown}"
        );
    }

    #[test]
    fn refuses_without_writing_anything() {
        let dir = project("1.2.3");
        let before = std::fs::read_to_string(dir.path().join(PROJECT_TOML)).unwrap();
        let files = release("1.3.0");
        let refused = |version: &str, files: &BTreeMap<String, Vec<u8>>| {
            pin_release(dir.path(), version, "base", &served(files)).unwrap_err()
        };
        assert!(refused("v1.3.0", &files).contains("is not a codeflow version"));
        assert!(refused("1.3.0; rm", &files).contains("is not a codeflow version"));
        assert!(refused("1.0.0", &release("1.0.0")).contains("older than the pinned 1.2.3"));
        assert!(refused("9.9.9", &files).contains("no published checksum file"));

        let mut tampered = files.clone();
        tampered
            .get_mut("base/v1.3.0/codeflow-cli-x86_64-apple-darwin.tar.xz")
            .unwrap()
            .push(b'!');
        assert!(refused("1.3.0", &tampered)
            .contains("codeflow-cli-x86_64-apple-darwin.tar.xz for codeflow 1.3.0 does not match"));

        let mut unlisted = files.clone();
        let sums = String::from_utf8(unlisted["base/v1.3.0/sha256.sum"].clone()).unwrap();
        let sums: String = sums
            .split_inclusive('\n')
            .filter(|line| !line.contains("aarch64"))
            .collect();
        unlisted.insert("base/v1.3.0/sha256.sum".into(), sums.into_bytes());
        assert!(refused("1.3.0", &unlisted)
            .contains("lists no checksum for codeflow-cli-aarch64-apple-darwin.tar.xz"));

        assert_eq!(
            std::fs::read_to_string(dir.path().join(PROJECT_TOML)).unwrap(),
            before
        );
    }

    #[test]
    fn the_same_version_pins_its_digests() {
        let dir = project("1.3.0");
        let files = release("1.3.0");
        let report = pin_release(dir.path(), "1.3.0", "base", &served(&files)).unwrap();
        assert!(report.to_string().contains("scaffold_version 1.3.0 in"));
    }

    #[test]
    fn reads_a_pinned_table() {
        assert_eq!(
            pinned_digests("scaffold_version = \"1\"\n"),
            PinnedDigests::Absent
        );
        assert_eq!(pinned_digests("not toml ["), PinnedDigests::Absent);
        let partial = format!(
            "[scaffold_sha256]\nversion = \"1.2.3\"\nx86_64-unknown-linux-gnu = \"{DIGEST}\"\nx86_64-apple-darwin = \"XYZ\"\n"
        );
        assert_eq!(
            pinned_digests(&partial),
            PinnedDigests::Table {
                version: Some("1.2.3".into()),
                listed: vec!["x86_64-unknown-linux-gnu"],
                missing: vec!["aarch64-apple-darwin", "x86_64-apple-darwin"],
            }
        );
        // Valid TOML the installers do not read fails closed as they do:
        // an inline or dotted table, a quoted header and a sub-table are
        // unreadable, never absent; single-quoted values read as missing.
        for unread in [
            format!("scaffold_sha256 = {{ version = \"1.2.3\", x86_64-unknown-linux-gnu = \"{DIGEST}\" }}\n"),
            format!("scaffold_sha256.version = \"1.2.3\"\nscaffold_sha256.x86_64-unknown-linux-gnu = \"{DIGEST}\"\n"),
            format!("[\"scaffold_sha256\"]\nversion = \"1.2.3\"\nx86_64-unknown-linux-gnu = \"{DIGEST}\"\n"),
            format!("[scaffold_sha256]\nversion = \"1.2.3\"\n[scaffold_sha256.extra]\nx86_64-unknown-linux-gnu = \"{DIGEST}\"\n"),
            "[tool]\nscaffold_sha256 = { version = \"1.2.3\" }\n".to_string(),
        ] {
            assert_eq!(
                pinned_digests(&unread),
                PinnedDigests::Unreadable(OTHER_FORM.into()),
                "{unread}"
            );
        }
    }

    #[test]
    fn refuses_a_line_starting_with_a_character_the_installers_do_not_read() {
        // A first character a byte reader does not read (a byte-order mark,
        // a no-break or em space, a control character) could hide the
        // header, so any is refused; one inside a value is read.
        for start in ["\u{feff}", "\u{a0}", "\u{2003}"] {
            assert_eq!(
                pinned_digests(&format!(
                    "{start}[scaffold_sha256]\nversion = \"1.2.3\"\nx86_64-unknown-linux-gnu = \"{DIGEST}\"\n"
                )),
                PinnedDigests::Unreadable(UNREAD_START.into()),
                "{start:?}"
            );
        }
        assert!(matches!(
            pinned_digests(&format!(
                "note = \"caf\u{e9} \u{2014} \u{a0}x\"\n[scaffold_sha256]\nversion = \"1.2.3\"\nx86_64-unknown-linux-gnu = \"{DIGEST}\"\n"
            )),
            PinnedDigests::Table { .. }
        ));
        // A control character anywhere (a NUL reaches awk as byte 1), and a
        // header or key name with a character outside printable ASCII, are
        // refused wherever they sit.
        for (text, reason) in [
            ("\u{1}[scaffold_sha256]\n".to_string(), UNREAD_CONTROL),
            ("\0[scaffold_sha256]\n".to_string(), UNREAD_CONTROL),
            ("[scaffold_sha\x00256]\n".to_string(), UNREAD_CONTROL),
            ("note = \"a\u{1}b\"\n".to_string(), UNREAD_CONTROL),
            ("[scaffold_sha\u{200b}256]\n".to_string(), NON_ASCII_NAME),
            ("\"caf\u{e9}\" = 1\n".to_string(), NON_ASCII_NAME),
        ] {
            assert_eq!(
                pinned_digests(&format!(
                    "{text}[scaffold_sha256]\nversion = \"1.2.3\"\nx86_64-unknown-linux-gnu = \"{DIGEST}\"\n"
                )),
                PinnedDigests::Unreadable(reason.into()),
                "{text:?}"
            );
        }
        // Bare carriage returns fold the file into one awk line, so one
        // inside a line is refused; one before each line feed is read.
        let table = format!(
            "[scaffold_sha256]\nversion = \"1.2.3\"\nx86_64-unknown-linux-gnu = \"{DIGEST}\"\n"
        );
        for folded in [
            table.replace('\n', "\r"),
            format!("# note\r{}", table.replace('\n', "\r")),
        ] {
            assert_eq!(
                pinned_digests(&folded),
                PinnedDigests::Unreadable(UNREAD_CR.into()),
                "{folded:?}"
            );
        }
        assert!(matches!(
            pinned_digests(&table.replace('\n', "\r\n")),
            PinnedDigests::Table { .. }
        ));
    }

    #[test]
    fn refuses_doubled_or_escaped_tables_and_ignores_comments() {
        // A second header or a key listed twice is refused too, and a
        // comment naming the table is not a declaration.
        let twice = format!(
            "[scaffold_sha256]\nversion = \"1.2.3\"\n[other]\n[scaffold_sha256]\nx86_64-unknown-linux-gnu = \"{DIGEST}\"\n"
        );
        assert_eq!(
            pinned_digests(&twice),
            PinnedDigests::Unreadable(TWICE.into())
        );
        for (doubled, key) in [
            (
                "version = \"1.2.3\"\nversion = \"1.2.4\"\n".to_string(),
                "version",
            ),
            (
                format!(
                    "x86_64-apple-darwin = \"{DIGEST}\"\n\"x86_64-apple-darwin\" = \"{DIGEST}\"\n"
                ),
                "x86_64-apple-darwin",
            ),
        ] {
            assert_eq!(
                pinned_digests(&format!("[scaffold_sha256]\n{doubled}")),
                PinnedDigests::Unreadable(format!("lists {key} twice"))
            );
        }
        assert_eq!(
            pinned_digests("# [scaffold_sha256] is written by codeflow update --pin\nscaffold_version = \"1\"\n"),
            PinnedDigests::Absent
        );
        // Values and trailing comments that name the table are not
        // declarations, inside the table or out of it.
        assert_eq!(
            pinned_digests("scaffold_version = \"1\" # scaffold_sha256 later\n[orient] # scaffold_sha256 too\nnote = \"scaffold_sha256\"\n"),
            PinnedDigests::Absent
        );
        assert_eq!(
            pinned_digests(&format!(
                "[scaffold_sha256]\nversion = \"1.2.3\" # scaffold_sha256 for 1.2.3\nx86_64-unknown-linux-gnu = \"{DIGEST}\"\n"
            )),
            PinnedDigests::Table {
                version: Some("1.2.3".into()),
                listed: vec!["x86_64-unknown-linux-gnu"],
                missing: vec!["aarch64-apple-darwin", "x86_64-apple-darwin"],
            }
        );
        // An escaped name could spell the table without its letters, so it
        // is refused rather than read as absent.
        for escaped in [
            format!("[\"\\u0073caffold_sha256\"]\nversion = \"1.2.3\"\nx86_64-unknown-linux-gnu = \"{DIGEST}\"\n"),
            "\"\\u0073caffold_sha256\" = { version = \"1.2.3\" }\n".to_string(),
            "\"\\u0073caffold_sha256\".version = \"1.2.3\"\n".to_string(),
        ] {
            assert_eq!(
                pinned_digests(&escaped),
                PinnedDigests::Unreadable(ESCAPED.into()),
                "{escaped}"
            );
        }
        let single = format!(
            "[ scaffold_sha256 ] # pinned\nversion = '1.2.3'\n\"x86_64-unknown-linux-gnu\" = '{DIGEST}'\n"
        );
        assert_eq!(
            pinned_digests(&single),
            PinnedDigests::Unreadable(STRAY.into())
        );
        // A multi-line string could fake an entry or a table, so any is
        // refused; inside the table only plain entries are read, so a
        // single-quoted or dotted key, an array or an escaped value is too.
        let faked = format!(
            "[scaffold_sha256]\nversion = \"1.2.3\"\nnotes = '''\nx86_64-unknown-linux-gnu = \"{DIGEST}\"\n'''\n'x86_64-unknown-linux-gnu' = \"{DIGEST}\"\n"
        );
        assert_eq!(
            pinned_digests(&faked),
            PinnedDigests::Unreadable(MULTILINE.into())
        );
        assert_eq!(
            pinned_digests("scaffold_version = \"3.1.0\"\nnotes = \"\"\"\nscaffold_sha256 is optional = later\n\"\"\"\n"),
            PinnedDigests::Unreadable(MULTILINE.into())
        );
        for stray in [
            format!("'x86_64-unknown-linux-gnu' = \"{DIGEST}\""),
            format!("x86_64-unknown-linux-gnu.note = \"{DIGEST}\""),
            "notes = [\"a\"]".to_string(),
            "version = \"1.2\\u002e3\"".to_string(),
        ] {
            assert_eq!(
                pinned_digests(&format!(
                    "[scaffold_sha256]\nversion = \"1.2.3\"\n{stray}\n"
                )),
                PinnedDigests::Unreadable(STRAY.into()),
                "{stray}"
            );
        }
        // The section ends at the next header, and a quoted key counts.
        let ended = format!(
            "[scaffold_sha256]\nversion = \"1.2.3\"\n\"x86_64-unknown-linux-gnu\" = \"{DIGEST}\"\n[other]\naarch64-apple-darwin = \"{DIGEST}\"\n"
        );
        assert_eq!(
            pinned_digests(&ended),
            PinnedDigests::Table {
                version: Some("1.2.3".into()),
                listed: vec!["x86_64-unknown-linux-gnu"],
                missing: vec!["aarch64-apple-darwin", "x86_64-apple-darwin"],
            }
        );
    }
}
