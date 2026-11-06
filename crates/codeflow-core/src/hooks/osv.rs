//! The pinned osv-scanner runner of `codeflow scan` (TSK-1176 AC-6 and
//! AC-7, moved from TSK-430), beside the gitleaks runner of TSK-412.
//!
//! It gives the verdict of the managed `security review` job's dependency
//! audit for the same base and head. The audit reads the head's tree, with
//! every `osv-scanner.toml` replaced by the base's copies, so a range cannot
//! suppress its own advisory; a head file that a case-insensitive disk would
//! open under that name (`OSV-Scanner.toml`, or `osv-scanner.toml.` on
//! Windows) is left out too. Every path is copied only after each of its
//! components is checked, so a crafted tree cannot write outside the
//! audit's private folder. `--no-ignore` keeps a `.gitignore` the range
//! edits from hiding a lockfile; and when the base holds
//! `.codeflow/audit-exceptions.sh`, that hook, run from the base's files,
//! accepts findings exactly as the hosted step lets it. The verdict comes
//! from osv-scanner's exit status: 0 no advisories, 1 advisories, 128 no
//! package sources (unless it also could not extract a file), anything else
//! an error.
//!
//! Like the gitleaks runner it never downloads the scanner: it runs the
//! `osv-scanner` on `PATH` only when the file's SHA-256 is the pinned
//! release binary for this platform and it then reports the pinned version.
//! osv-scanner itself asks the OSV.dev service for advisories, so the audit
//! needs the network; with none it fails, and at `block` that refuses.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::gitleaks::file_sha256;
use super::gitleaks::range::{blob, resolve, Scratch};

/// The osv-scanner version the managed CI audit pins (`OSV_VERSION` in
/// `assets/base/ci/codeflow-ci.yml`; a test holds the two equal).
pub const VERSION: &str = "2.4.0";

/// Where the osv-scanner releases are published.
pub const RELEASES: &str = "https://github.com/google/osv-scanner/releases";

/// One platform's pinned release binary, as the release's
/// `osv-scanner_SHA256SUMS` lists it. The release publishes bare binaries,
/// so the file checksum is the binary's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pin {
    /// `std::env::consts::OS` of the platform.
    pub os: &'static str,
    /// `std::env::consts::ARCH` of the platform.
    pub arch: &'static str,
    /// The release asset's file name.
    pub asset: &'static str,
    /// Its SHA-256.
    pub sha256: &'static str,
}

/// The pinned release binaries of [`VERSION`]. The `linux` `x86_64` one is
/// the binary the managed CI workflow checks.
pub const PINS: &[Pin] = &[
    Pin {
        os: "linux",
        arch: "x86_64",
        asset: "osv-scanner_linux_amd64",
        sha256: "15314940c10d26af9c6649f150b8a47c1262e8fc7e17b1d1029b0e479e8ed8a0",
    },
    Pin {
        os: "linux",
        arch: "aarch64",
        asset: "osv-scanner_linux_arm64",
        sha256: "44e580752910f0ff36ec99aff59af20f65df1e859aa31e5605a8f0d055b496e9",
    },
    Pin {
        os: "macos",
        arch: "x86_64",
        asset: "osv-scanner_darwin_amd64",
        sha256: "088119325156321c34c456ac3703d6013538fd71cbac82b891ab34db491e4d66",
    },
    Pin {
        os: "macos",
        arch: "aarch64",
        asset: "osv-scanner_darwin_arm64",
        sha256: "9ca3185ad63e9ab54f7cb90f46a7362be02d80e37f0123d095a54355ea202f5d",
    },
    Pin {
        os: "windows",
        arch: "x86_64",
        asset: "osv-scanner_windows_amd64.exe",
        sha256: "0cdd113610126d5dfd5e12ad0e0b4f3e879291ff19bb43b0c52ed2f2c2df1a37",
    },
    Pin {
        os: "windows",
        arch: "aarch64",
        asset: "osv-scanner_windows_arm64.exe",
        sha256: "1ce89d7d8ef083634648ef0f193fe1254f36f46f4bdc93d61178adacc2e60da0",
    },
];

/// Where the runner looks for osv-scanner and what it must match.
#[derive(Debug, Clone)]
pub struct Scanner {
    /// The folders searched, in order: the absolute entries of `PATH`.
    pub search: Vec<PathBuf>,
    /// The pin for this platform, if any.
    pub pin: Option<Pin>,
    /// The platform, as `os arch`.
    pub platform: String,
}

/// A binary that matched the pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// Where it was found.
    pub path: PathBuf,
    /// Its SHA-256.
    pub sha256: String,
}

impl Scanner {
    /// The runner searching the absolute entries of `path`, a `PATH` value,
    /// with the pin for the platform this binary was built for.
    #[must_use]
    pub fn with_path(path: Option<&std::ffi::OsStr>) -> Self {
        Self::for_platform(path, std::env::consts::OS, std::env::consts::ARCH)
    }

    /// The runner for `os` and `arch`, as `std::env::consts` names them.
    #[must_use]
    pub fn for_platform(path: Option<&std::ffi::OsStr>, os: &str, arch: &str) -> Self {
        let search = path
            .map(|path| {
                std::env::split_paths(path)
                    .filter(|dir| dir.is_absolute())
                    .collect()
            })
            .unwrap_or_default();
        Self {
            search,
            pin: PINS
                .iter()
                .copied()
                .find(|pin| pin.os == os && pin.arch == arch),
            platform: format!("{os} {arch}"),
        }
    }

    /// The pin in words, for a refusal or a doctor line.
    #[must_use]
    pub fn describe(&self) -> String {
        match self.pin {
            Some(pin) => format!(
                "osv-scanner {VERSION} for {} ({}, sha256 {})",
                self.platform, pin.asset, pin.sha256
            ),
            None => format!(
                "osv-scanner {VERSION} has no pinned binary for {}",
                self.platform
            ),
        }
    }

    /// The folders searched, in words.
    #[must_use]
    pub fn searched(&self) -> String {
        if self.search.is_empty() {
            "searched no folder (PATH has no absolute entry)".to_string()
        } else {
            format!(
                "searched {}",
                self.search
                    .iter()
                    .map(|dir| crate::git::display_path(dir))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }

    /// The first `osv-scanner` file in the searched folders.
    #[must_use]
    pub fn locate(&self) -> Option<PathBuf> {
        let name = if cfg!(windows) {
            "osv-scanner.exe"
        } else {
            "osv-scanner"
        };
        self.search
            .iter()
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    }

    /// Find the binary and check it against the pin. It runs, to print its
    /// version, only after its checksum matched.
    ///
    /// # Errors
    /// Why the scanner cannot run, in words.
    pub fn verify(&self) -> Result<Verified, String> {
        let Some(pin) = self.pin else {
            return Err("no pinned osv-scanner binary for this platform".to_string());
        };
        let path = self
            .locate()
            .ok_or_else(|| "osv-scanner is not on PATH".to_string())?;
        let shown = crate::git::display_path(&path);
        let sha256 = file_sha256(&path)
            .map_err(|error| format!("cannot read {shown} to check its checksum: {error}"))?;
        if sha256 != pin.sha256 {
            return Err(format!(
                "{shown} has sha256 {sha256}, which is not the pinned binary; it was not run"
            ));
        }
        let printed = match Command::new(&path)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
        {
            Ok(out) => out.stdout,
            Err(error) => {
                return Err(format!(
                    "{shown} matches the pinned checksum but did not run: {error}"
                ))
            }
        };
        // The first line, exactly as the pinned release prints it.
        let expected = format!("osv-scanner version: {VERSION}");
        let first = printed
            .split(|byte| *byte == b'\n')
            .next()
            .unwrap_or_default();
        if first != expected.as_bytes() {
            return Err(format!(
                "{shown} matches the pinned checksum but does not report {expected}"
            ));
        }
        Ok(Verified { path, sha256 })
    }
}

/// The dependency audit's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Audit {
    /// osv-scanner found no advisories.
    Clean,
    /// No lockfile or manifest to audit.
    NoSources,
    /// Every advisory is accepted by the base's project exception hook.
    Accepted(Vec<String>),
    /// Advisories found, or a scan error: the exit code and the output.
    Failed {
        /// osv-scanner's exit code.
        code: i32,
        /// What it printed, and what the exception hook said.
        output: String,
    },
}

/// The base commit's audit levels: `git.security_review` and
/// `git.dep_audit`, each `block`, `warn` or `off`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Levels {
    /// `git.security_review`.
    pub security_review: String,
    /// `git.dep_audit`.
    pub dep_audit: String,
}

impl Levels {
    /// The audit's level: `block` when either key blocks, `off` when both
    /// are off, else `warn`, as the hosted step decides.
    #[must_use]
    pub fn level(&self) -> &'static str {
        if self.security_review == "block" || self.dep_audit == "block" {
            "block"
        } else if self.security_review == "off" && self.dep_audit == "off" {
            "off"
        } else {
            "warn"
        }
    }
}

/// Read both levels from the base's `.codeflow/policy.json`. As in the
/// hosted step, a missing file, a missing key or another value fails closed.
///
/// # Errors
/// Why the levels cannot be read.
pub fn levels(root: &Path, base: &str) -> Result<Levels, String> {
    let policy = blob(root, base, ".codeflow/policy.json")?.ok_or_else(|| {
        format!("cannot read .codeflow/policy.json at the base {base}; the dependency audit fails closed until it is committed there")
    })?;
    let data: serde_json::Value = serde_json::from_slice(&policy).map_err(|_| {
        format!("cannot read .codeflow/policy.json at the base {base}; the dependency audit fails closed")
    })?;
    let key = |name: &str| -> Result<String, String> {
        match data.get("git").and_then(|git| git.get(name)).and_then(serde_json::Value::as_str) {
            Some(value @ ("block" | "warn" | "off")) => Ok(value.to_string()),
            _ => Err(format!(
                "git.{name} in .codeflow/policy.json at the base {base} is missing or not block, warn or off; the dependency audit fails closed"
            )),
        }
    };
    Ok(Levels {
        security_review: key("security_review")?,
        dep_audit: key("dep_audit")?,
    })
}

const SUPPRESSIONS: &str = "osv-scanner.toml";
const EXCEPTIONS: &str = ".codeflow/audit-exceptions.sh";

/// Whether `name`, one path component, may open as `osv-scanner.toml` on
/// this disk. macOS and Windows match names without case by default
/// (Unicode case, so `ſ` is an `s`), and Windows drops trailing dots and
/// spaces, so a range's `OSV-Scanner.toml` or `osv-scanner.toml.` is the
/// file osv-scanner reads from that folder. The name is compared after
/// lower- then upper-casing, which folds every such spelling.
fn opens_as_suppressions(name: &str) -> bool {
    name.trim_end_matches(['.', ' '])
        .to_lowercase()
        .to_uppercase()
        == SUPPRESSIONS.to_uppercase()
}

/// Write the regular files of `commit` into `into`, leaving out every file
/// that may open as `osv-scanner.toml` when `suppressions` is false. Links
/// and submodules are left out, and a path that would leave `into` fails
/// the export.
fn export(root: &Path, commit: &str, into: &mut Scratch, suppressions: bool) -> Result<(), String> {
    let mut list = crate::git::judging_command();
    let listing = list
        .args(["ls-tree", "-r", "-z", "--full-tree", commit])
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("git did not run: {error}"))?;
    if !listing.status.success() {
        return Err(format!("cannot list the files of {commit}"));
    }
    let mut entries: Vec<(String, String)> = Vec::new();
    for record in listing.stdout.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let Some(tab) = record.iter().position(|b| *b == b'\t') else {
            return Err(format!("cannot read the file list of {commit}"));
        };
        let meta = std::str::from_utf8(&record[..tab])
            .map_err(|_| format!("cannot read the file list of {commit}"))?;
        let path = String::from_utf8(record[tab + 1..].to_vec()).map_err(|_| {
            format!("{commit} holds a path that is not UTF-8; the audit cannot copy it")
        })?;
        let fields: Vec<&str> = meta.split(' ').collect();
        if fields.len() != 3 || fields[1] != "blob" || !matches!(fields[0], "100644" | "100755") {
            continue;
        }
        let name = path.rsplit('/').next().unwrap_or_default();
        if !suppressions && opens_as_suppressions(name) {
            continue;
        }
        // Checked before git is asked for any content.
        into.target(&path)?;
        entries.push((fields[2].to_string(), path));
    }
    let mut command = crate::git::judging_command();
    let mut child = command
        .args(["cat-file", "--batch"])
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("git did not run: {error}"))?;
    let mut input = child.stdin.take().ok_or("git cat-file has no input")?;
    let ids = entries.iter().fold(String::new(), |mut ids, (oid, _)| {
        ids.push_str(oid);
        ids.push('\n');
        ids
    });
    let writer = std::thread::spawn(move || {
        use std::io::Write as _;
        input.write_all(ids.as_bytes())
    });
    let mut stdout =
        std::io::BufReader::new(child.stdout.take().ok_or("git cat-file has no output")?);
    for (_, path) in &entries {
        let mut header = Vec::new();
        std::io::BufRead::read_until(&mut stdout, b'\n', &mut header)
            .map_err(|error| format!("cannot read {path} from {commit}: {error}"))?;
        // `<oid> blob <size>\n`, ASCII as git writes it.
        let size: usize = std::str::from_utf8(&header)
            .map_err(|_| format!("cannot read {path} from {commit}"))?
            .strip_suffix('\n')
            .ok_or_else(|| format!("cannot read {path} from {commit}"))?
            .split(' ')
            .nth(2)
            .and_then(|size| size.parse().ok())
            .ok_or_else(|| format!("cannot read {path} from {commit}"))?;
        let mut data = vec![0; size + 1];
        stdout
            .read_exact(&mut data)
            .map_err(|error| format!("cannot read {path} from {commit}: {error}"))?;
        data.pop();
        into.write(path, &data)
            .map_err(|why| format!("{why} (the dependency audit)"))?;
    }
    let _ = writer.join();
    let status = child
        .wait()
        .map_err(|error| format!("git did not finish: {error}"))?;
    if !status.success() {
        return Err(format!("cannot read the files of {commit}"));
    }
    Ok(())
}

/// Put the base's `osv-scanner.toml` files into the exported head tree.
/// Only the exact name is placed, as the hosted audit on Linux reads it;
/// the export already left out every head file that could open as one.
fn base_suppressions(root: &Path, base: &str, tree: &mut Scratch) -> Result<Vec<String>, String> {
    let mut list = crate::git::judging_command();
    let listing = list
        .args(["ls-tree", "-r", "-z", "--full-tree", "--name-only", base])
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("git did not run: {error}"))?;
    if !listing.status.success() {
        return Err(format!("cannot list the files of the base {base}"));
    }
    let mut written = Vec::new();
    for path in listing.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let path = String::from_utf8(path.to_vec()).map_err(|_| {
            format!("the base {base} holds a path that is not UTF-8; the audit cannot place it")
        })?;
        if path.rsplit('/').next() != Some(SUPPRESSIONS) {
            continue;
        }
        let target = tree.target(&path)?;
        if let Some(parent) = target.parent() {
            if parent.exists() && !parent.is_dir() {
                return Err(format!(
                    "the range replaces a folder holding {path} with a file"
                ));
            }
        }
        let data = blob(root, base, &path)?.unwrap_or_default();
        tree.write(&path, &data)
            .map_err(|why| format!("{why} (placing the base's suppressions)"))?;
        written.push(path);
    }
    Ok(written)
}

/// Run the verified scanner over `head`'s tree with the base's
/// suppressions and exception hook, as the hosted dependency audit does.
///
/// # Errors
/// Why the audit could not be set up; a failed scan is an [`Audit::Failed`].
pub fn audit(verified: &Verified, root: &Path, base: &str, head: &str) -> Result<Audit, String> {
    let base = resolve(root, base)?;
    let head = resolve(root, head)?;
    let work = tempfile::Builder::new()
        .prefix("codeflow-osv-")
        .tempdir()
        .map_err(|error| format!("cannot make the audit's folder: {error}"))?;
    let mut head_files = Scratch::new(&work.path().join("head"))?;
    export(root, &head, &mut head_files, false)?;
    base_suppressions(root, &base, &mut head_files)?;
    // Resolved, so osv-scanner's absolute paths start with it.
    let tree = head_files.root().to_path_buf();
    let hook = blob(root, &base, EXCEPTIONS)?;
    let mut command = Command::new(&verified.path);
    command
        .current_dir(&tree)
        .args(["scan", "-r", "--no-ignore"]);
    if hook.is_some() {
        command.args(["--verbosity", "error", "--format", "json"]);
    }
    let out = command
        .arg(".")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("osv-scanner did not run: {error}"))?;
    let code = out.status.code().unwrap_or(-1);
    // Shown to a person, and searched only for osv-scanner's own phrase.
    let shown = |bytes: &[u8]| String::from_utf8_lossy(bytes).to_string();
    let diag = if hook.is_some() {
        shown(&out.stderr)
    } else {
        format!("{}{}", shown(&out.stdout), shown(&out.stderr))
    };
    if code == 0 {
        return Ok(Audit::Clean);
    }
    if code == 128
        && !diag
            .to_ascii_lowercase()
            .contains("error during extraction")
    {
        return Ok(Audit::NoSources);
    }
    let mut output = diag;
    if code == 1 && hook.is_some() {
        match run_exceptions(root, &base, &tree, work.path(), &out.stdout, &out.stderr) {
            Ok(accepted) => return Ok(Audit::Accepted(accepted)),
            Err(why) => {
                output.push_str(&why);
                output.push('\n');
            }
        }
    }
    Ok(Audit::Failed { code, output })
}

/// The findings of a JSON report, one line each: id, package, version,
/// ecosystem, lockfile and aliases, tab separated, as the hosted step lists
/// them for the exception hook.
fn finding_lines(report: &[u8], tree: &Path) -> Result<Vec<String>, String> {
    let value: serde_json::Value = serde_json::from_slice(report).map_err(|_| {
        "osv-scanner's JSON report cannot be read; no project exception applies".to_string()
    })?;
    let here = format!("{}/", tree.display());
    let text = |value: &serde_json::Value, key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let escape = |field: String| {
        field
            .replace('\\', "\\\\")
            .replace('\t', "\\t")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    };
    let mut lines = Vec::new();
    for result in value
        .get("results")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        let source = text(
            result.get("source").unwrap_or(&serde_json::Value::Null),
            "path",
        );
        let source = source.strip_prefix(&here).unwrap_or(&source).to_string();
        for package in result
            .get("packages")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            let p = package.get("package").cloned().unwrap_or_default();
            for vuln in package
                .get("vulnerabilities")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                let aliases = vuln
                    .get("aliases")
                    .and_then(serde_json::Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(serde_json::Value::as_str)
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default();
                let fields = [
                    text(vuln, "id"),
                    text(&p, "name"),
                    text(&p, "version"),
                    text(&p, "ecosystem"),
                    source.clone(),
                    aliases,
                ];
                lines.push(
                    fields
                        .into_iter()
                        .map(escape)
                        .collect::<Vec<_>>()
                        .join("\t"),
                );
            }
        }
    }
    lines.sort();
    lines.dedup();
    Ok(lines)
}

/// Run the base's exception hook over the findings; `Ok` with the accepted
/// lines only when it accepts every one.
fn run_exceptions(
    root: &Path,
    base: &str,
    tree: &Path,
    work: &Path,
    report: &[u8],
    stderr: &[u8],
) -> Result<Vec<String>, String> {
    // As the hosted step's `[ -s scan.err ]`: any byte on stderr.
    if !stderr.is_empty() {
        return Err("osv-scanner logged errors, so its scan may be incomplete; no project exception applies".to_string());
    }
    let findings = finding_lines(report, tree)?;
    if findings.is_empty() {
        return Err("osv-scanner reported advisories but none could be listed from its JSON report; no project exception applies".to_string());
    }
    let mut trusted = Scratch::new(&work.join("trusted"))?;
    export(root, base, &mut trusted, true)
        .map_err(|why| format!("cannot export {EXCEPTIONS} and the project files from the base: {why}; no project exception applies"))?;
    let trusted = trusted.root().to_path_buf();
    let list = work.join("findings.tsv");
    std::fs::write(
        &list,
        findings.iter().fold(String::new(), |mut list, line| {
            list.push_str(line);
            list.push('\n');
            list
        }),
    )
    .map_err(|error| format!("cannot write the findings list: {error}"))?;
    let out = Command::new("bash")
        .arg(EXCEPTIONS)
        .current_dir(&trusted)
        .env("CODEFLOW_AUDIT_FINDINGS", &list)
        .env("CODEFLOW_TRUSTED_SHA", base)
        .env("CODEFLOW_TRUSTED_ROOT", &trusted)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("the project exception hook did not run: {error}"))?;
    if !out.status.success() {
        return Err(format!(
            "the project exception hook {EXCEPTIONS} failed ({}); it accepts nothing",
            out.status
        ));
    }
    // Each printed line must equal a finding exactly, as `grep -Fx` does.
    let printed: Vec<&[u8]> = out.stdout.split(|byte| *byte == b'\n').collect();
    let accepted: Vec<String> = findings
        .iter()
        .filter(|line| printed.contains(&line.as_bytes()))
        .cloned()
        .collect();
    let remaining: Vec<&String> = findings
        .iter()
        .filter(|line| !accepted.contains(line))
        .collect();
    if remaining.is_empty() {
        Ok(accepted)
    } else {
        Err(remaining.iter().fold(String::new(), |mut list, line| {
            list.push_str("not accepted: ");
            list.push_str(line);
            list.push('\n');
            list
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_linux_pin_is_the_managed_ci_pin() {
        let workflow = include_str!("../../../../assets/base/ci/codeflow-ci.yml");
        assert!(workflow.contains(&format!("OSV_VERSION={VERSION}")));
        let linux = PINS
            .iter()
            .find(|pin| pin.os == "linux" && pin.arch == "x86_64")
            .unwrap();
        assert!(workflow.contains(&format!("OSV_SHA256={}", linux.sha256)));
        assert!(workflow.contains(linux.asset));
    }

    #[test]
    fn the_level_blocks_when_either_key_blocks_and_skips_only_when_both_are_off() {
        let levels = |sr: &str, da: &str| Levels {
            security_review: sr.into(),
            dep_audit: da.into(),
        };
        assert_eq!(levels("warn", "block").level(), "block");
        assert_eq!(levels("off", "off").level(), "off");
        assert_eq!(levels("off", "warn").level(), "warn");
    }

    #[test]
    fn an_unpinned_platform_cannot_verify() {
        let scanner = Scanner::for_platform(None, "plan9", "mips");
        assert!(scanner.verify().unwrap_err().contains("no pinned"));
        assert!(scanner.describe().contains("no pinned binary"));
    }

    /// Run git in `repo`, feeding `input`; its trimmed output.
    fn git(repo: &Path, args: &[&str], input: &str) -> String {
        use std::io::Write as _;
        let mut child = crate::git::judging_command()
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(repo)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    /// A repository and a commit whose root tree holds `entries`, each
    /// `(path, content)`, built with plumbing so any name git stores fits.
    fn commit_of(repo: &Path, entries: &[(&str, &str)]) -> String {
        fn tree(repo: &Path, entries: &[(Vec<&str>, &str)]) -> String {
            use std::fmt::Write as _;
            let mut lines = String::new();
            let mut done: Vec<&str> = Vec::new();
            for (parts, content) in entries {
                if let [name] = parts.as_slice() {
                    let blob = git(repo, &["hash-object", "-w", "--stdin"], content);
                    writeln!(lines, "100644 blob {blob}\t{name}").unwrap();
                } else if !done.contains(&parts[0]) {
                    done.push(parts[0]);
                    let inner: Vec<(Vec<&str>, &str)> = entries
                        .iter()
                        .filter(|(p, _)| p.len() > 1 && p[0] == parts[0])
                        .map(|(p, c)| (p[1..].to_vec(), *c))
                        .collect();
                    let sub = tree(repo, &inner);
                    writeln!(lines, "040000 tree {sub}\t{}", parts[0]).unwrap();
                }
            }
            git(repo, &["mktree"], &lines)
        }
        let split: Vec<(Vec<&str>, &str)> = entries
            .iter()
            .map(|(path, content)| (path.split('/').collect(), *content))
            .collect();
        let root = tree(repo, &split);
        git(repo, &["commit-tree", &root, "-m", "fixture"], "")
    }

    fn new_repo(dir: &Path) -> PathBuf {
        let repo = dir.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q"], "");
        repo
    }

    /// Every file under `dir`, relative, with `/` between components.
    fn files_under(dir: &Path) -> Vec<String> {
        let mut found = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(next) = stack.pop() {
            for entry in std::fs::read_dir(&next).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let rel = path.strip_prefix(dir).unwrap();
                    found.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        found.sort();
        found
    }

    /// Security review F1: a tree entry named `..` makes git list
    /// `../../pwned.txt`; the export refuses it before writing anything,
    /// and nothing lands outside its folder.
    #[test]
    fn a_tree_entry_named_dot_dot_is_refused_and_nothing_is_written_outside() {
        let dir = tempfile::tempdir().unwrap();
        let repo = new_repo(dir.path());
        let commit = commit_of(
            &repo,
            &[("Cargo.lock", "# lock\n"), ("../../pwned.txt", "pwned\n")],
        );
        let listed = git(&repo, &["ls-tree", "-r", "--name-only", &commit], "");
        assert!(
            listed.lines().any(|line| line == "../../pwned.txt"),
            "{listed}"
        );
        let outer = dir.path().join("work");
        let mut into = Scratch::new(&outer.join("a").join("head")).unwrap();
        let error = export(&repo, &commit, &mut into, false).unwrap_err();
        assert!(error.contains("outside the private folder"), "{error}");
        assert_eq!(files_under(&outer), Vec::<String>::new());
        assert!(!dir.path().join("pwned.txt").exists());
    }

    /// Security review F2: on a case-insensitive disk osv-scanner opens a
    /// head's `OSV-Scanner.toml` as `osv-scanner.toml`, so the export
    /// leaves out every spelling that may open as one, at every level.
    #[test]
    fn a_head_suppression_in_another_case_is_never_exported() {
        let dir = tempfile::tempdir().unwrap();
        let repo = new_repo(dir.path());
        let head = commit_of(
            &repo,
            &[
                ("Cargo.lock", "# lock\n"),
                ("OSV-Scanner.toml", "[[IgnoredVulns]]\nid = \"X\"\n"),
                ("sub/Cargo.lock", "# lock\n"),
                ("sub/osv-SCANNER.TOML", "[[IgnoredVulns]]\nid = \"X\"\n"),
                (
                    "sub/deep/o\u{17f}v-scanner.toml",
                    "[[IgnoredVulns]]\nid = \"X\"\n",
                ),
                (
                    "sub/deep/osv-scanner.toml. ",
                    "[[IgnoredVulns]]\nid = \"X\"\n",
                ),
            ],
        );
        let base = commit_of(&repo, &[("sub/osv-scanner.toml", "# base\n")]);
        let mut into = Scratch::new(&dir.path().join("head")).unwrap();
        export(&repo, &head, &mut into, false).unwrap();
        base_suppressions(&repo, &base, &mut into).unwrap();
        let files = files_under(into.root());
        assert_eq!(
            files,
            ["Cargo.lock", "sub/Cargo.lock", "sub/osv-scanner.toml"],
            "a head suppression was exported"
        );
        assert_eq!(
            std::fs::read_to_string(into.root().join("sub/osv-scanner.toml")).unwrap(),
            "# base\n"
        );
    }

    #[test]
    fn every_spelling_a_disk_may_open_as_the_suppression_file_matches() {
        for name in [
            "osv-scanner.toml",
            "OSV-Scanner.toml",
            "osv-scanner.toml.",
            "osv-scanner.toml . ",
            "o\u{17f}v-scanner.toml",
        ] {
            assert!(opens_as_suppressions(name), "{name}");
        }
        for name in [
            "osv-scanner.tom",
            "xosv-scanner.toml",
            "osv-scanner.toml.bak",
        ] {
            assert!(!opens_as_suppressions(name), "{name}");
        }
    }

    #[test]
    fn findings_are_listed_as_the_hosted_step_lists_them() {
        let tree = Path::new("/t");
        let report = r#"{"results":[{"source":{"path":"/t/a/Cargo.lock"},"packages":[{"package":{"name":"p","version":"1","ecosystem":"crates.io"},"vulnerabilities":[{"id":"RUSTSEC-1","aliases":["CVE-1","GHSA-1"]}]}]}]}"#;
        assert_eq!(
            finding_lines(report.as_bytes(), tree).unwrap(),
            ["RUSTSEC-1\tp\t1\tcrates.io\ta/Cargo.lock\tCVE-1,GHSA-1"]
        );
    }
}
