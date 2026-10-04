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

use super::hash::sha256_hex;
use super::state::{read_beneath_root, write_record, PROJECT_TOML};
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
}

/// Reads the `[scaffold_sha256]` table from a project state's text. A state
/// that does not parse reads as absent; its own checks report it.
#[must_use]
pub fn pinned_digests(state: &str) -> PinnedDigests {
    let Ok(value) = toml::from_str::<toml::Value>(state) else {
        return PinnedDigests::Absent;
    };
    let Some(table) = value.get(TABLE).and_then(toml::Value::as_table) else {
        return PinnedDigests::Absent;
    };
    let version = table
        .get("version")
        .and_then(toml::Value::as_str)
        .map(str::to_string);
    let (listed, missing) = TRIPLES.iter().partition(|triple| {
        table
            .get(**triple)
            .and_then(toml::Value::as_str)
            .is_some_and(is_digest)
    });
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
    let out = toml::to_string_pretty(&state)
        .map_err(|error| format!("cannot write {PROJECT_TOML}: {error}"))?;
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
    }
}
