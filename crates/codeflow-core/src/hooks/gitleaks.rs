//! The gitleaks engine of the staged secret scan (TSK-412, issue 251).
//!
//! With `git.secret_scan_engine: gitleaks`, the commit-time scan runs the
//! gitleaks release the managed CI scan pins over the staged diff, after the
//! built-in patterns, so a match the CI rules see is caught while the commit
//! can still be reworded. The engine never downloads anything: it looks for
//! `gitleaks` on `PATH` and runs it only when the file's SHA-256 is the pinned
//! release binary for this platform and it then reports the pinned version.
//! Anything else is a finding at the `git.secret_scan` level, so at `block`
//! a missing, unpinned or mismatched binary refuses the commit.
//!
//! The pin binds the verdict to the CI rules; it is not a defence against a
//! process that can write to a `PATH` directory. Such a process could replace
//! the binary between the check and the run, or replace `git`, which this
//! hook and gitleaks both run unverified.
//!
//! The working tree's `.gitleaksignore` and `.gitleaks.toml` are the only
//! copies before a commit exists, so gitleaks reads them there; the CI scan
//! reads its own from the trusted commit, so a commit cannot weaken it. A
//! staged finding's fingerprint is `<file>:<rule>:<line>`, with no commit,
//! so a commit-bound `.gitleaksignore` entry never covers it. An inline
//! `gitleaks:allow` on a staged line is ignored, and git attributes come from
//! the empty tree with `diff` set everywhere, so staged content can neither
//! exempt itself nor hide from the scan.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The gitleaks version the managed CI scan pins (`GITLEAKS_VERSION` in
/// `assets/base/ci/codeflow-ci.yml`; a test holds the two equal).
pub const VERSION: &str = "8.30.1";

/// The exit code gitleaks is asked to use for findings. Its own errors exit
/// 1 and a crash exits 2, so any other code is an incomplete scan.
const FINDINGS_EXIT: i32 = 7;

/// One platform's pinned release binary: the archive the gitleaks release
/// publishes, its checksum from the release's checksums file, and the
/// checksum of the binary inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pin {
    /// `std::env::consts::OS` of the platform.
    pub os: &'static str,
    /// `std::env::consts::ARCH` of the platform.
    pub arch: &'static str,
    /// The release archive's file name.
    pub archive: &'static str,
    /// The archive's SHA-256, as the release's checksums file lists it.
    pub archive_sha256: &'static str,
    /// The SHA-256 of the binary the archive holds.
    pub binary_sha256: &'static str,
}

/// The pinned release binaries of [`VERSION`]. The `linux` `x86_64`
/// archive checksum is the one the managed CI workflow checks.
pub const PINS: &[Pin] = &[
    Pin {
        os: "linux",
        arch: "x86_64",
        archive: "gitleaks_8.30.1_linux_x64.tar.gz",
        archive_sha256: "551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb",
        binary_sha256: "88f91962aa2f93ac6ab281d553b9e125f5197bbbce38f9f2437f7299c32e5509",
    },
    Pin {
        os: "linux",
        arch: "aarch64",
        archive: "gitleaks_8.30.1_linux_arm64.tar.gz",
        archive_sha256: "e4a487ee7ccd7d3a7f7ec08657610aa3606637dab924210b3aee62570fb4b080",
        binary_sha256: "00e91bbe655bd7c47753e8cfe61cb76ea1a5d7e7702fe161ee40102b46b3823b",
    },
    Pin {
        os: "macos",
        arch: "x86_64",
        archive: "gitleaks_8.30.1_darwin_x64.tar.gz",
        archive_sha256: "dfe101a4db2255fc85120ac7f3d25e4342c3c20cf749f2c20a18081af1952709",
        binary_sha256: "cee01fea7173f1b779dff188e1c26ecbcb4027d394acc573b23aaf0be260e291",
    },
    Pin {
        os: "macos",
        arch: "aarch64",
        archive: "gitleaks_8.30.1_darwin_arm64.tar.gz",
        archive_sha256: "b40ab0ae55c505963e365f271a8d3846efbc170aa17f2607f13df610a9aeb6a5",
        binary_sha256: "ba52fb1bfabbcde42f032afad3d6e0b19dff8ed105229a16e7caa338bbc0e84f",
    },
    Pin {
        os: "windows",
        arch: "x86_64",
        archive: "gitleaks_8.30.1_windows_x64.zip",
        archive_sha256: "d29144deff3a68aa93ced33dddf84b7fdc26070add4aa0f4513094c8332afc4e",
        binary_sha256: "17157e2ee8b76fc8b1d8bee607a250e34b8a8023c8bc81822d4b5ee4d78fcb7c",
    },
    Pin {
        os: "windows",
        arch: "aarch64",
        archive: "gitleaks_8.30.1_windows_arm64.zip",
        archive_sha256: "b95f5e4f5c425cedca7ee203d9afd29597e692c4924a12ed42f970537c72cc0f",
        binary_sha256: "200df852fdecbedb19a33960657333cba5e231740bc8968972b507b50f93b194",
    },
];

/// Where the gitleaks releases are published.
pub const RELEASES: &str = "https://github.com/gitleaks/gitleaks/releases";

/// The binary the engine must find: the version and, when this platform
/// has a pin, its checksum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expected {
    /// The version `gitleaks version` must print.
    pub version: String,
    /// The binary's SHA-256; `None` when no pin covers this platform.
    pub sha256: Option<String>,
    /// The archive that holds it, for the install route.
    pub archive: Option<String>,
    /// The platform, as `os arch`.
    pub platform: String,
}

impl Expected {
    /// The pin for the platform this binary was built for.
    #[must_use]
    pub fn for_this_platform() -> Self {
        Self::for_platform(std::env::consts::OS, std::env::consts::ARCH)
    }

    /// The pin for `os` and `arch`, as `std::env::consts` names them.
    #[must_use]
    pub fn for_platform(os: &str, arch: &str) -> Self {
        let pin = PINS.iter().find(|pin| pin.os == os && pin.arch == arch);
        Self {
            version: VERSION.to_string(),
            sha256: pin.map(|pin| pin.binary_sha256.to_string()),
            archive: pin.map(|pin| pin.archive.to_string()),
            platform: format!("{os} {arch}"),
        }
    }

    /// The pin in words, for a refusal or a doctor line.
    #[must_use]
    pub fn describe(&self) -> String {
        match (&self.sha256, &self.archive) {
            (Some(sha), Some(archive)) => format!(
                "gitleaks {} for {} ({archive}, binary sha256 {sha})",
                self.version, self.platform
            ),
            _ => format!(
                "gitleaks {} has no pinned binary for {}",
                self.version, self.platform
            ),
        }
    }
}

/// Where the engine looks for gitleaks and what it must match.
#[derive(Debug, Clone)]
pub struct Engine {
    /// The folders searched, in order: the absolute entries of `PATH`.
    pub search: Vec<PathBuf>,
    /// The pinned binary.
    pub expected: Expected,
}

/// A binary that matched the pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// Where it was found.
    pub path: PathBuf,
    /// Its SHA-256.
    pub sha256: String,
}

/// Why the engine cannot run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unusable {
    /// No pin covers this platform.
    Unpinned,
    /// No `gitleaks` in any searched folder.
    Missing,
    /// The found file could not be read.
    Unreadable {
        /// The file.
        path: PathBuf,
        /// The read error.
        error: String,
    },
    /// The found file is not the pinned binary.
    Checksum {
        /// The file.
        path: PathBuf,
        /// Its SHA-256.
        found: String,
    },
    /// The pinned checksum matched, but the binary reported another version
    /// or none.
    Version {
        /// The file.
        path: PathBuf,
        /// What `gitleaks version` printed, or why it could not run.
        found: String,
    },
}

impl Unusable {
    /// What was found, in words; the pin and the doctor check are added by
    /// the caller.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Unpinned => "no pinned gitleaks binary for this platform".to_string(),
            Self::Missing => "gitleaks is not on PATH".to_string(),
            Self::Unreadable { path, error } => format!(
                "cannot read {} to check its checksum: {error}",
                crate::git::display_path(path)
            ),
            Self::Checksum { path, found } => format!(
                "{} has sha256 {found}, which is not the pinned binary; it was not run",
                crate::git::display_path(path)
            ),
            Self::Version { path, found } => format!(
                "{} matches the pinned checksum but reports version {}",
                crate::git::display_path(path),
                crate::git::display_text(found)
            ),
        }
    }
}

/// One gitleaks finding on the staged content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The gitleaks rule id, such as `generic-api-key`.
    pub rule: String,
    /// The file, as gitleaks names it.
    pub file: String,
    /// The line in the staged file.
    pub line: u64,
}

impl Engine {
    /// The engine as the hook runs it: `PATH` from the environment and the
    /// pin for this platform.
    #[must_use]
    pub fn from_env() -> Self {
        Self::with_path(std::env::var_os("PATH").as_deref())
    }

    /// The engine searching the absolute entries of `path`, a `PATH` value.
    /// A relative entry, such as `.` or an empty one, would resolve against
    /// the project, so it is skipped.
    #[must_use]
    pub fn with_path(path: Option<&std::ffi::OsStr>) -> Self {
        let search = path
            .map(|path| {
                std::env::split_paths(path)
                    .filter(|dir| dir.is_absolute())
                    .collect()
            })
            .unwrap_or_default();
        Self {
            search,
            expected: Expected::for_this_platform(),
        }
    }

    /// The first `gitleaks` file in the searched folders.
    #[must_use]
    pub fn locate(&self) -> Option<PathBuf> {
        let name = if cfg!(windows) {
            "gitleaks.exe"
        } else {
            "gitleaks"
        };
        self.search
            .iter()
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    }

    /// Find the binary and check it against the pin. The binary runs, to
    /// print its version, only after its checksum matched.
    ///
    /// # Errors
    /// Returns why the engine cannot run.
    pub fn verify(&self) -> Result<Verified, Unusable> {
        let Some(expected) = self.expected.sha256.as_deref() else {
            return Err(Unusable::Unpinned);
        };
        let path = self.locate().ok_or(Unusable::Missing)?;
        let sha256 = file_sha256(&path).map_err(|error| Unusable::Unreadable {
            path: path.clone(),
            error: error.to_string(),
        })?;
        if sha256 != expected {
            return Err(Unusable::Checksum {
                path,
                found: sha256,
            });
        }
        let version = match Command::new(&path).arg("version").output() {
            Ok(out) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            }
            Ok(out) => format!("none (it exited with {})", out.status),
            Err(error) => format!("none (it did not run: {error})"),
        };
        if version != self.expected.version {
            return Err(Unusable::Version {
                path,
                found: version,
            });
        }
        Ok(Verified { path, sha256 })
    }

    /// Run the verified binary over the staged diff of the repository at
    /// `root`, reading `.gitleaksignore` and `.gitleaks.toml` from there.
    ///
    /// # Errors
    /// Returns why the scan is incomplete: a staged path gitleaks cannot
    /// read, a git too old to pin attributes, a failed binary or a report
    /// that could not be read.
    pub fn scan_staged(&self, verified: &Verified, root: &Path) -> Result<Vec<Finding>, String> {
        let attributes = TextAttributes::for_repository(root)?;
        if let Some(path) = unreadable_staged_path(root)? {
            return Err(format!(
                "gitleaks cannot read a path with a backslash, double quote or \
                 control character reliably, and {path} has one; rename it"
            ));
        }
        let mut command = Command::new(&verified.path);
        command
            .current_dir(root)
            .args(["git", "--staged", "--no-banner", "--redact"])
            // A gitleaks:allow comment on a staged line is new content. The
            // CI scan counts only one the trusted commit already holds, so
            // here it never hides a finding.
            .arg("--ignore-gitleaks-allow")
            // gitleaks exits 0 with an empty report when git fails part way
            // through, and says so only in its log, which is read below.
            .args(["--log-level", "debug", "--no-color"])
            .args(["--report-format", "json", "--report-path", "-"])
            .args(["--exit-code", &FINDINGS_EXIT.to_string()])
            .arg("--gitleaks-ignore-path")
            .arg(root)
            .arg(root)
            // The rules come from the working tree's .gitleaks.toml or the
            // defaults, as in CI, never from a variable of this shell.
            .env_remove("GITLEAKS_CONFIG")
            .env_remove("GITLEAKS_CONFIG_TOML");
        attributes.apply(&mut command);
        pin_git_output(&mut command, &attributes.config());
        let out = command
            .output()
            .map_err(|error| format!("gitleaks did not run: {error}"))?;
        let code = out.status.code();
        let failure = logged_failure(&out.stderr);
        if code != Some(0) && code != Some(FINDINGS_EXIT) {
            return Err(format!(
                "gitleaks exited with {}{}",
                out.status,
                failure.map_or_else(String::new, |line| format!(": {line}"))
            ));
        }
        if let Some(line) = failure {
            return Err(format!(
                "gitleaks did not read the whole staged diff: {line}"
            ));
        }
        let findings = parse_report(&out.stdout)?;
        if code == Some(FINDINGS_EXIT) && findings.is_empty() {
            return Err("gitleaks reported findings but its report lists none".to_string());
        }
        Ok(findings)
    }
}

/// What the gitleaks log says went wrong, if anything: its first error or
/// fatal line, or that it aborted the git command it ran. gitleaks exits 0
/// with an empty report when git fails part way through (for example on an
/// unreadable object), and the managed CI scan reads its log for the same
/// reason. Debug lines can quote scanned text, so only an error line is
/// ever shown.
///
/// A record is `<time> <level> <message> <fields>`, as gitleaks' console
/// log writes it. Only a line that starts with a time is a record, and only
/// its level and the start of its message are read: a path, fingerprint or
/// other field that happens to hold `ERR` or `command aborted` is data, not
/// an event.
fn logged_failure(stderr: &[u8]) -> Option<String> {
    let mut aborted = false;
    for raw in stderr.split(|byte| *byte == b'\n') {
        let line = without_colour(raw);
        let Some((time, rest)) = split_field(&line) else {
            continue;
        };
        if !is_log_time(time) {
            continue;
        }
        let Some((level, message)) = split_field(rest) else {
            continue;
        };
        if level == b"ERR" || level == b"FTL" {
            return Some(String::from_utf8_lossy(&line).into_owned());
        }
        // The event gitleaks logs when the git command it waits on fails.
        if level == b"DBG"
            && (message == b"command aborted" || message.starts_with(b"command aborted "))
        {
            aborted = true;
        }
    }
    aborted.then(|| "gitleaks aborted the git command it ran".to_string())
}

/// The first space-separated field of `line` and what follows it.
fn split_field(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let at = line.iter().position(|byte| *byte == b' ')?;
    Some((&line[..at], &line[at + 1..]))
}

/// Whether `field` is the console log's time, such as `9:05AM` or `12:03PM`.
fn is_log_time(field: &[u8]) -> bool {
    let Some(clock) = field
        .strip_suffix(b"AM")
        .or_else(|| field.strip_suffix(b"PM"))
    else {
        return false;
    };
    match clock.iter().position(|byte| *byte == b':') {
        Some(colon) => {
            let (hours, minutes) = (&clock[..colon], &clock[colon + 1..]);
            (1..=2).contains(&hours.len())
                && minutes.len() == 2
                && hours.iter().chain(minutes).all(u8::is_ascii_digit)
        }
        None => false,
    }
}

/// `line` without ANSI colour sequences (`ESC [ ... m`).
fn without_colour(line: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(line.len());
    let mut bytes = line.iter().copied();
    while let Some(byte) = bytes.next() {
        if byte == 0x1b {
            for next in bytes.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(byte);
        }
    }
    out
}

/// Git settings that change how `git diff` names paths or colours output,
/// pinned for the gitleaks run as the managed CI scan pins them, so a
/// finding names the file a path allowlist matches. Rename and copy
/// detection is off, as in the staged-path check, so every added line in
/// the diff gitleaks reads sits under a path that check read: a rename from
/// a name the parser cannot read would otherwise put that name in the
/// header of the added lines and hide them.
///
/// A symlink's staged target is diffed with git's `default` driver, which
/// path attributes do not reach, so that driver is pinned too: never
/// binary, its textconv the identity (`cat`) and nothing cached, whatever
/// a configuration file or include sets. A submodule shows as its commit
/// line only.
const GIT_OUTPUT_PINS: [(&str, &str); 13] = [
    ("diff.renames", "false"),
    ("diff.default.binary", "false"),
    ("diff.default.textconv", "cat"),
    ("diff.default.cachetextconv", "false"),
    ("diff.submodule", "short"),
    ("core.quotePath", "true"),
    ("diff.noprefix", "false"),
    ("diff.mnemonicPrefix", "false"),
    ("diff.srcPrefix", "a/"),
    ("diff.dstPrefix", "b/"),
    ("diff.relative", "false"),
    ("color.ui", "false"),
    ("color.diff", "false"),
];

/// Give a git process of the scan a configuration this hook controls.
///
/// Command-line configuration a caller passes down (`git -c`, which git
/// hands on as `GIT_CONFIG_PARAMETERS`) outranks the numbered
/// `GIT_CONFIG_*` settings, so it is removed, with any inherited numbered
/// settings, before the pins are set; `diff.renames=copies` from a caller
/// would otherwise turn rename detection back on. `GIT_DIFF_OPTS` and
/// `GIT_EXTERNAL_DIFF` would change the diff itself, and replace refs would
/// change what HEAD holds, so neither reaches the scan. The pins then
/// outrank every configuration file. The variables that choose the
/// repository, worktree and index are kept, so the scan reads the commit
/// being made, and the configuration files stay in scope for everything
/// the pins do not set, such as `safe.directory`.
fn pin_git_output(command: &mut Command, extra: &[(&str, OsString)]) {
    command
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_DIFF_OPTS")
        .env_remove("GIT_EXTERNAL_DIFF")
        .env("GIT_NO_REPLACE_OBJECTS", "1");
    for (name, _) in std::env::vars_os() {
        let bytes = name.as_encoded_bytes();
        if bytes.starts_with(b"GIT_CONFIG_KEY_") || bytes.starts_with(b"GIT_CONFIG_VALUE_") {
            command.env_remove(&name);
        }
    }
    let pins = GIT_OUTPUT_PINS
        .iter()
        .map(|(key, value)| (*key, OsString::from(value)))
        .chain(extra.iter().cloned());
    let mut n = 0;
    for (key, value) in pins {
        command
            .env(format!("GIT_CONFIG_KEY_{n}"), key)
            .env(format!("GIT_CONFIG_VALUE_{n}"), value);
        n += 1;
    }
    command.env("GIT_CONFIG_COUNT", n.to_string());
}

/// The first staged path gitleaks' patch parser cannot read reliably: one
/// holding a backslash, a double quote or a control character, which git
/// quotes in a diff header. The pinned parser skips a file whose name ends
/// in a backslash without an error, so its lines would go unscanned. The CI
/// scan refuses the same characters. Deleted files are left out, since
/// gitleaks is not shown them. This check holds whatever the path policy
/// says, and for an automatic merge, whose staged paths no path check reads.
fn unreadable_staged_path(root: &Path) -> Result<Option<String>, String> {
    let mut command = crate::git::command();
    // The same configuration as the gitleaks child, so both read one diff.
    pin_git_output(&mut command, &[]);
    let out = command
        .args([
            "diff",
            "--cached",
            "--name-only",
            "-z",
            "--no-renames",
            "--diff-filter=d",
        ])
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("git did not run: {error}"))?;
    if !out.status.success() {
        return Err(format!(
            "cannot list the staged paths: `git diff --cached` exited with {}",
            out.status
        ));
    }
    Ok(out
        .stdout
        .split(|byte| *byte == 0)
        .find(|path| {
            path.iter()
                .any(|byte| matches!(byte, b'\\' | b'"') || byte.is_ascii_control())
        })
        .map(|path| {
            let name = crate::git::name::GitName::from_bytes(path);
            match name.rule_text() {
                Ok(text) => text.to_string(),
                Err(not_utf8) => not_utf8.display().to_string(),
            }
        }))
}

/// The oldest git that reads attributes from a named tree
/// (`GIT_ATTR_SOURCE`), the version the managed CI scan requires too.
const MIN_GIT: (u32, u32) = (2, 41);

/// Git attributes for the gitleaks run, so the staged diff it reads shows
/// every staged line as text, as the CI scan's `--text --no-textconv` does.
///
/// gitleaks reads `git diff --staged`, which leaves out a file git judges
/// binary and runs a file's `textconv` filter. A committed or staged
/// `.gitattributes` could use either (`-diff`, `binary`, `diff=<driver>`)
/// to hide a line from the scan. So attributes are read from the empty tree
/// instead of the working tree, and a temporary attributes file sets `diff`
/// on every path, which makes git show even a file holding a NUL byte as
/// text. The clone's own `info/attributes` still applies: nothing committed
/// or staged can change it.
struct TextAttributes {
    empty_tree: String,
    file: tempfile::NamedTempFile,
}

impl TextAttributes {
    fn for_repository(root: &Path) -> Result<Self, String> {
        let git = |args: &[&str]| -> Result<String, String> {
            let out = crate::git::command()
                .args(args)
                .current_dir(root)
                .stdin(std::process::Stdio::null())
                .output()
                .map_err(|error| format!("git did not run: {error}"))?;
            if !out.status.success() {
                return Err(format!(
                    "`git {}` exited with {}",
                    args.join(" "),
                    out.status
                ));
            }
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        };
        let version = git(&["version"])?;
        if git_version(&version).is_none_or(|found| found < MIN_GIT) {
            return Err(format!(
                "{version} cannot read attributes from a named tree, so a \
                 .gitattributes entry could hide staged lines from gitleaks; \
                 upgrade git to {}.{} or later",
                MIN_GIT.0, MIN_GIT.1
            ));
        }
        let empty_tree = git(&["hash-object", "-t", "tree", "--stdin"])?;
        let unwritable =
            |error: std::io::Error| format!("cannot write an attributes file: {error}");
        let mut file = tempfile::Builder::new()
            .prefix("codeflow-gitleaks-attributes")
            .tempfile()
            .map_err(unwritable)?;
        std::io::Write::write_all(&mut file, b"* diff\n").map_err(unwritable)?;
        Ok(Self { empty_tree, file })
    }

    fn apply(&self, command: &mut Command) {
        command
            .env("GIT_ATTR_SOURCE", &self.empty_tree)
            .env("GIT_ATTR_NOSYSTEM", "1");
    }

    fn config(&self) -> [(&'static str, OsString); 1] {
        [(
            "core.attributesFile",
            self.file.path().as_os_str().to_os_string(),
        )]
    }
}

/// The major and minor version in `git version` output, such as
/// `git version 2.45.1.windows.1` or `git version 2.39.5 (Apple Git-154)`.
fn git_version(output: &str) -> Option<(u32, u32)> {
    let number = output
        .strip_prefix("git version ")?
        .split_whitespace()
        .next()?;
    let mut parts = number.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// The findings of a gitleaks JSON report.
fn parse_report(stdout: &[u8]) -> Result<Vec<Finding>, String> {
    let unreadable = |why: &str| format!("cannot read the gitleaks report: {why}");
    let value: serde_json::Value =
        serde_json::from_slice(stdout).map_err(|error| unreadable(&error.to_string()))?;
    let entries = value
        .as_array()
        .ok_or_else(|| unreadable("it is not a list"))?;
    entries
        .iter()
        .map(|entry| {
            let text = |key: &str| {
                entry
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| unreadable(&format!("a finding has no {key}")))
            };
            Ok(Finding {
                rule: text("RuleID")?,
                file: text("File")?,
                line: entry
                    .get("StartLine")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| unreadable("a finding has no StartLine"))?,
            })
        })
        .collect()
}

/// The SHA-256 of a file, in lower-case hex.
///
/// # Errors
/// Returns the read error.
pub fn file_sha256(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    Ok(super::source_identity::hex_digest(&Sha256::digest(
        std::fs::read(path)?,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pin_is_the_managed_ci_pin() {
        let ci = include_str!("../../../../assets/base/ci/codeflow-ci.yml");
        let versions: Vec<&str> = ci
            .lines()
            .filter_map(|line| line.trim().strip_prefix("GITLEAKS_VERSION="))
            .collect();
        assert_eq!(versions, [VERSION], "the CI workflow pins one version");
        let archive = ci
            .lines()
            .find_map(|line| line.trim().strip_prefix("GITLEAKS_SHA256="))
            .expect("the CI workflow pins the archive checksum");
        let linux = PINS
            .iter()
            .find(|pin| pin.os == "linux" && pin.arch == "x86_64")
            .expect("a linux x86_64 pin");
        assert_eq!(linux.archive_sha256, archive);
        for pin in PINS {
            assert!(
                pin.archive.starts_with(&format!("gitleaks_{VERSION}_")),
                "{pin:?}"
            );
            for sha in [pin.archive_sha256, pin.binary_sha256] {
                assert!(
                    sha.len() == 64
                        && sha
                            .bytes()
                            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
                    "{pin:?}"
                );
            }
        }
    }

    #[test]
    fn an_unpinned_platform_has_no_checksum() {
        let expected = Expected::for_platform("plan9", "mips");
        assert_eq!(expected.sha256, None);
        let engine = Engine {
            search: Vec::new(),
            expected,
        };
        assert_eq!(engine.verify(), Err(Unusable::Unpinned));
    }

    #[test]
    fn only_log_events_count_as_a_failure_never_their_data() {
        // Real events.
        for log in [
            "11:17AM ERR [git] fatal: unable to read files to diff",
            "9:05PM FTL failed to load config",
            "\u{1b}[90m11:17AM\u{1b}[0m \u{1b}[31mFTL\u{1b}[0m failed",
            "11:17AM DBG command aborted error=\"exit status 128\" cmd=\"git diff\"",
        ] {
            assert!(logged_failure(log.as_bytes()).is_some(), "{log}");
        }
        // The same words in a path, fingerprint, field or other message.
        for log in [
            "12:03PM DBG skipping finding: global fingerprint finding=REDACTED \
             fingerprint=\"docs/command aborted.json:generic-api-key:2\"",
            "12:03PM DBG executing: git -C /tmp/command aborted diff --staged .",
            "12:03PM DBG found .gitleaksignore file path=\"ERR/command aborted\"",
            "12:03PM INF command aborted",
            "12:03PM DBG commands aborted",
            "ERR at the start of a line that is not a record",
            "command aborted",
            "x 11:17AM ERR shifted",
        ] {
            assert_eq!(logged_failure(log.as_bytes()), None, "{log}");
        }
    }

    #[test]
    fn git_versions_are_read_from_each_platform_format() {
        assert_eq!(git_version("git version 2.53.0"), Some((2, 53)));
        assert_eq!(
            git_version("git version 2.39.5 (Apple Git-154)"),
            Some((2, 39))
        );
        assert_eq!(git_version("git version 2.45.1.windows.1"), Some((2, 45)));
        assert_eq!(git_version("hub version 2.53"), None);
        assert!(git_version("git version 2.40.9").unwrap() < MIN_GIT);
        assert!(git_version("git version 2.41.0").unwrap() >= MIN_GIT);
        assert!(git_version("git version 3.0.0").unwrap() >= MIN_GIT);
    }

    #[test]
    fn relative_path_entries_are_not_searched() {
        let path = std::env::join_paths(["", ".", "bin", "/usr/bin"]).unwrap();
        let engine = Engine::with_path(Some(&path));
        assert_eq!(engine.search, [PathBuf::from("/usr/bin")]);
    }

    #[test]
    fn the_report_names_rule_file_and_line() {
        let report = br#"[{"RuleID":"generic-api-key","File":"docs/a.json","StartLine":2,"Secret":"REDACTED"}]"#;
        assert_eq!(
            parse_report(report).unwrap(),
            [Finding {
                rule: "generic-api-key".into(),
                file: "docs/a.json".into(),
                line: 2
            }]
        );
        assert_eq!(parse_report(b"[]").unwrap(), []);
        assert!(parse_report(b"{}").is_err());
        assert!(parse_report(br#"[{"RuleID":"x"}]"#).is_err());
        assert!(parse_report(b"").is_err());
    }
}
