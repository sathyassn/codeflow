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
    /// read, a git too old to pin attributes, a failed binary, any output
    /// gitleaks wrote to stderr, a git it ran that did not exit with 0, or a report
    /// that could not be read.
    pub fn scan_staged(&self, verified: &Verified, root: &Path) -> Result<Vec<Finding>, String> {
        let attributes = TextAttributes::for_repository(root)?;
        let folder = ScanFolder::new(&git_dir(root)?)?;
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
            // At this level gitleaks writes only errors, so any output on
            // stderr is a failed scan, whatever it says.
            .args(["--log-level", "error", "--no-color"])
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
        pin_git_output(&mut command, &folder.attributes_config());
        folder.apply(&mut command);
        let out = command
            .output()
            .map_err(|error| format!("gitleaks did not run: {error}"))?;
        let code = out.status.code();
        if code != Some(0) && code != Some(FINDINGS_EXIT) {
            return Err(format!(
                "gitleaks exited with {}{}",
                out.status,
                first_line(&out.stderr).map_or_else(String::new, |line| format!(": {line}"))
            ));
        }
        if !out.stderr.is_empty() {
            return Err(format!(
                "gitleaks reported an error: {}",
                first_line(&out.stderr)
                    .unwrap_or_else(|| "output with no visible text".to_string())
            ));
        }
        if let Some(why) = git_run_failure(&folder.events()?) {
            return Err(format!(
                "gitleaks did not read the whole staged diff: {why}"
            ));
        }
        let findings = parse_report(&out.stdout)?;
        if code == Some(FINDINGS_EXIT) && findings.is_empty() {
            return Err("gitleaks reported findings but its report lists none".to_string());
        }
        Ok(findings)
    }
}

/// The first line with visible text that gitleaks wrote to stderr, with
/// control characters escaped, to name what failed. Nothing is decided from
/// its text: any output at all, blank or not, has already failed the scan.
fn first_line(stderr: &[u8]) -> Option<String> {
    let line = stderr
        .split(|byte| *byte == b'\n')
        .find(|line| !line.iter().all(u8::is_ascii_whitespace))?;
    Some(String::from_utf8_lossy(line).escape_debug().to_string())
}

/// The arguments gitleaks 8.30.1 gives the git that produces the staged
/// diff, after `git -C <source>`.
const STAGED_DIFF_ARGS: [&str; 5] = ["diff", "-U0", "--no-ext-diff", "--staged", "."];

/// The trace file's name in the scan's private folder.
const EVENTS: &str = "events";
/// The attributes file's name in the scan's private folder.
const ATTRIBUTES: &str = "attributes";

/// The scan's private folder. It holds git's own record of the git
/// processes gitleaks starts (its trace2 event stream) and the attributes
/// file that shows every staged line as text.
///
/// gitleaks exits 0 with an empty report when the git it runs fails part
/// way through, and reports that only in a debug log whose lines can hold
/// unescaped paths and configuration values. Each trace2 event is one JSON
/// object written by git itself, so no path, file content or message can
/// forge one, and a git that dies before it exits leaves no exit event.
///
/// Both files are input the scan trusts, so no other user may be able to
/// replace them or any folder above them. The folder is made inside the
/// repository's own git directory, never under `TMPDIR`, `TMP` or `TEMP`:
/// anyone who can rename entries there or above it can already replace the
/// hooks, configuration and index the scan reads, so the folder rests on no
/// trust the commit does not already rest on, on every platform.
///
/// On Unix the folder is created with mode 0700 (given to `mkdir`, which a
/// umask can only narrow), then opened without following a link, and it
/// must be a directory this user owns with that mode. Both files are created
/// inside that held directory (`openat`, exclusive, no link followed, mode
/// 0600), never by resolving its path again. Elsewhere both are created
/// exclusively and take the git directory's access list. Events are read
/// back through the trace file's own handle, and on Unix the scan fails if
/// the folder's or the trace's path no longer names what was made.
struct ScanFolder {
    folder: tempfile::TempDir,
    events: std::fs::File,
    #[cfg(unix)]
    held: std::fs::File,
}

impl ScanFolder {
    fn new(git_dir: &Path) -> Result<Self, String> {
        let unmade = |error: std::io::Error| {
            format!(
                "cannot make the scan's private folder in {}: {error}",
                git_dir.display()
            )
        };
        let mut builder = tempfile::Builder::new();
        builder.prefix("codeflow-gitleaks-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        let folder = builder.tempdir_in(git_dir).map_err(unmade)?;
        #[cfg(unix)]
        {
            let held = private::open_folder(folder.path()).map_err(unmade)?;
            let events = private::create_in(&held, EVENTS).map_err(unmade)?;
            let mut attributes = private::create_in(&held, ATTRIBUTES).map_err(unmade)?;
            std::io::Write::write_all(&mut attributes, b"* diff\n").map_err(unmade)?;
            Ok(Self {
                folder,
                events,
                held,
            })
        }
        #[cfg(not(unix))]
        {
            let create = |name: &str| {
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(folder.path().join(name))
            };
            let events = create(EVENTS).map_err(unmade)?;
            let mut attributes = create(ATTRIBUTES).map_err(unmade)?;
            std::io::Write::write_all(&mut attributes, b"* diff\n").map_err(unmade)?;
            Ok(Self { folder, events })
        }
    }

    fn path(&self, name: &str) -> std::path::PathBuf {
        self.folder.path().join(name)
    }

    /// The pinned `core.attributesFile`: this folder's attributes file.
    fn attributes_config(&self) -> [(&'static str, OsString); 1] {
        [(
            "core.attributesFile",
            self.path(ATTRIBUTES).into_os_string(),
        )]
    }

    /// Send the events of every git under `command` to this folder's trace,
    /// and no other trace2 output: an inherited `GIT_TRACE2_*` setting is
    /// removed, and the normal and performance targets are turned off
    /// (`0`), which outranks a target a configuration file sets.
    fn apply(&self, command: &mut Command) {
        for (name, _) in std::env::vars_os() {
            if name.as_encoded_bytes().starts_with(b"GIT_TRACE2") {
                command.env_remove(&name);
            }
        }
        command
            .env("GIT_TRACE2_EVENT", self.path(EVENTS))
            .env("GIT_TRACE2", "0")
            .env("GIT_TRACE2_PERF", "0");
    }

    /// The events in the trace file made by [`ScanFolder::new`], read
    /// through its own handle. On Unix the folder's and the trace's paths
    /// must still name what was made.
    fn events(&self) -> Result<Vec<u8>, String> {
        let unreadable =
            |error: std::io::Error| format!("cannot read git's trace of the scan: {error}");
        #[cfg(unix)]
        {
            if !private::names(&self.held, self.folder.path()).map_err(unreadable)? {
                return Err("the scan's private folder was replaced while it ran".to_string());
            }
            if !private::names(&self.events, &self.path(EVENTS)).map_err(unreadable)? {
                return Err("git's trace of the scan was replaced while it ran".to_string());
            }
        }
        let mut file = &self.events;
        let mut events = Vec::new();
        std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(0)).map_err(unreadable)?;
        std::io::Read::read_to_end(&mut file, &mut events).map_err(unreadable)?;
        Ok(events)
    }
}

/// Creation bound to a held directory, for the scan's private folder.
#[cfg(unix)]
mod private {
    use std::ffi::CString;
    use std::fs::File;
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    /// Open the folder at `path` without following a link: it must be a
    /// directory this user owns with mode 0700.
    pub(super) fn open_folder(path: &Path) -> io::Result<File> {
        let name = CString::new(path.as_os_str().as_bytes())?;
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        // SAFETY: a NUL-terminated path; a successful descriptor is owned once.
        let descriptor = unsafe { libc::open(name.as_ptr(), flags) };
        if descriptor == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: open returned this owned descriptor.
        let folder = unsafe { File::from_raw_fd(descriptor) };
        let metadata = folder.metadata()?;
        // SAFETY: geteuid has no preconditions.
        let me = unsafe { libc::geteuid() };
        if !metadata.is_dir() || metadata.uid() != me || metadata.mode() & 0o777 != 0o700 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "it is not a folder only this user can open",
            ));
        }
        Ok(folder)
    }

    /// Create `name` inside the held `folder`: exclusive, never through a
    /// link, mode 0600, without resolving the folder's path again.
    pub(super) fn create_in(folder: &File, name: &str) -> io::Result<File> {
        let name = CString::new(name)?;
        let flags =
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        // SAFETY: a live directory descriptor and a NUL-terminated basename;
        // the mode is supplied because O_CREAT is set. A successful
        // descriptor is owned once.
        let descriptor = unsafe {
            libc::openat(
                folder.as_raw_fd(),
                name.as_ptr(),
                flags,
                0o600 as libc::c_uint,
            )
        };
        if descriptor == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openat returned this owned descriptor.
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }

    /// Whether `path`, not followed if a link, is the file `held` was opened on.
    pub(super) fn names(held: &File, path: &Path) -> io::Result<bool> {
        let held = held.metadata()?;
        let named = std::fs::symlink_metadata(path)?;
        Ok((held.dev(), held.ino()) == (named.dev(), named.ino()))
    }
}

/// The absolute git directory of the repository at `root`, exactly as git
/// prints it, less the one line break it ends with. Its bytes are kept as
/// they are, so a folder name holding a line break is read whole; output
/// without that line break, an empty path, or on Windows a path that is not
/// UTF-8, is refused.
fn git_dir(root: &Path) -> Result<std::path::PathBuf, String> {
    let out = crate::git::command()
        .args(["rev-parse", "--absolute-git-dir"])
        .env("GIT_NO_LAZY_FETCH", "1")
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("git did not run: {error}"))?;
    if !out.status.success() {
        return Err(format!(
            "`git rev-parse --absolute-git-dir` exited with {}",
            out.status
        ));
    }
    let unusable = || "git printed a git directory the scan cannot use".to_string();
    let mut printed = out.stdout;
    if printed.pop() != Some(b'\n') || printed.is_empty() {
        return Err(unusable());
    }
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(printed)
    };
    #[cfg(not(unix))]
    let path = OsString::from(String::from_utf8(printed).map_err(|_| unusable())?);
    Ok(std::path::PathBuf::from(path))
}

/// Why the git processes gitleaks ran did not read the whole staged diff,
/// from their trace2 events, if they did not. Exactly one of them must be
/// the staged diff, and every one must record an exit with code 0: a git
/// that failed records another code, and one killed by a signal records
/// none. An unreadable event is a failure too.
fn git_run_failure(events: &[u8]) -> Option<String> {
    let mut started: Vec<(String, bool)> = Vec::new();
    let mut exits: std::collections::HashMap<String, Option<i64>> =
        std::collections::HashMap::new();
    for line in events.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_slice::<serde_json::Value>(line) else {
            return Some("git's trace of the scan could not be read".to_string());
        };
        let Some(sid) = event.get("sid").and_then(serde_json::Value::as_str) else {
            return Some("git's trace of the scan could not be read".to_string());
        };
        match event.get("event").and_then(serde_json::Value::as_str) {
            Some("start") => {
                let argv: Vec<&str> = event
                    .get("argv")
                    .and_then(serde_json::Value::as_array)
                    .map(|argv| argv.iter().filter_map(serde_json::Value::as_str).collect())
                    .unwrap_or_default();
                let staged_diff =
                    argv.len() == 8 && argv[1] == "-C" && argv[3..] == STAGED_DIFF_ARGS;
                started.push((sid.to_string(), staged_diff));
            }
            Some("exit") => {
                exits.insert(
                    sid.to_string(),
                    event.get("code").and_then(serde_json::Value::as_i64),
                );
            }
            _ => {}
        }
    }
    let diffs = started
        .iter()
        .filter(|(_, staged_diff)| *staged_diff)
        .count();
    if diffs != 1 {
        return Some(format!(
            "git's trace shows {diffs} staged diffs where gitleaks runs one"
        ));
    }
    for (sid, staged_diff) in &started {
        let what = if *staged_diff {
            "the staged diff"
        } else {
            "a git process gitleaks started"
        };
        match exits.get(sid) {
            Some(Some(0)) => {}
            Some(Some(code)) => return Some(format!("{what} exited with {code}")),
            _ => {
                return Some(format!(
                    "{what} ended without exiting, as when it is killed"
                ))
            }
        }
    }
    None
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

/// Remove the settings a caller passes down (`git -c`, numbered
/// `GIT_CONFIG_*` settings), which would outrank the scan's own.
fn remove_caller_settings(command: &mut Command) {
    command
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_CONFIG_COUNT");
    for (name, _) in std::env::vars_os() {
        let bytes = name.as_encoded_bytes();
        if bytes.starts_with(b"GIT_CONFIG_KEY_") || bytes.starts_with(b"GIT_CONFIG_VALUE_") {
            command.env_remove(&name);
        }
    }
}

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
        .env_remove("GIT_DIFF_OPTS")
        .env_remove("GIT_EXTERNAL_DIFF")
        .env("GIT_NO_REPLACE_OBJECTS", "1");
    // Missing objects of a partial clone are an error, never a fetch: the
    // scan makes no network request, and gitleaks' own git inherits this.
    crate::git::local_objects_only(command);
    remove_caller_settings(command);
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

/// The oldest git that can be kept from fetching a repository's missing
/// objects. `GIT_NO_LAZY_FETCH` arrives in 2.45, but 2.45's diff prefetch
/// still calls the promisor fetch directly; the guard before the fetch
/// subprocess is in 2.46's `fetch_objects`. An older git ignores the setting,
/// which is harmless unless the repository has a promisor remote.
const MIN_GIT_PARTIAL_CLONE: (u32, u32) = (2, 46);

/// The reason a repository that fetches missing objects on demand (a
/// promisor remote) cannot be read by this git, or `None`. A git whose
/// version cannot be read is refused too.
fn promisor_refusal(version: &str, promisor: bool) -> Option<String> {
    if promisor && git_version(version).is_none_or(|found| found < MIN_GIT_PARTIAL_CLONE) {
        return Some(format!(
            "this repository fetches missing objects on demand (a promisor \
             remote), and {version} cannot be kept from doing that during the \
             scan; upgrade git to {}.{} or later",
            MIN_GIT_PARTIAL_CLONE.0, MIN_GIT_PARTIAL_CLONE.1
        ));
    }
    None
}

/// Whether the scan must be refused for this git: it is too old for the
/// attribute pinning, or the repository has a promisor remote and git cannot
/// be kept from fetching. Returns the refusal reason.
fn git_refusal(version: &str, promisor: bool) -> Option<String> {
    if git_version(version).is_none_or(|found| found < MIN_GIT) {
        return Some(format!(
            "{version} cannot read attributes from a named tree, so a \
             .gitattributes entry could hide staged lines from gitleaks; \
             upgrade git to {}.{} or later",
            MIN_GIT.0, MIN_GIT.1
        ));
    }
    promisor_refusal(version, promisor)
}

/// A read-only git query for the capability decision, in the scan's view of
/// the repository: the repository, worktree and index variables are kept,
/// and a caller's `GIT_CONFIG` (which only `git config` reads) and its
/// command-line and numbered settings are removed, as the scan removes them
/// for `git diff`.
fn capability_query(root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    let mut command = crate::git::command();
    command.env_remove("GIT_CONFIG");
    remove_caller_settings(&mut command);
    crate::git::local_objects_only(&mut command);
    command
        .args(args)
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("git did not run: {error}"))
}

/// The git version text, and whether the repository has a promisor remote
/// (`extensions.partialClone` or a `remote.<name>.promisor`). Git's exit 1
/// from `git config` means no match; any other failure is an error, so an
/// undecidable repository is never taken as a plain one.
fn version_and_promisor(root: &Path) -> Result<(String, bool), String> {
    let version = capability_query(root, &["version"])?;
    if !version.status.success() {
        return Err(format!("`git version` exited with {}", version.status));
    }
    let version = std::str::from_utf8(&version.stdout)
        .map_err(|_| "`git version` printed text that is not UTF-8".to_string())?;
    let version = version.strip_suffix('\n').unwrap_or(version);
    let version = version.strip_suffix('\r').unwrap_or(version).to_string();
    let config = capability_query(
        root,
        &[
            "config",
            "--get-regexp",
            r"^(extensions\.partialclone|remote\..*\.promisor)$",
        ],
    )?;
    match config.status.code() {
        Some(0) => Ok((version, !config.stdout.is_empty())),
        Some(1) => Ok((version, false)),
        _ => Err(format!(
            "cannot tell whether this repository fetches missing objects on \
             demand: `git config` exited with {}",
            config.status
        )),
    }
}

/// Refuse before any git object read when the repository fetches missing
/// objects on demand and this git cannot be kept from doing so, or when that
/// cannot be decided. Every caller of the commit-time scan that reads staged
/// objects (the staged inventory, attribute lookup, the scan itself) calls
/// this first. It makes no network request.
///
/// # Errors
///
/// The refusal reason, naming the git version needed.
pub fn require_offline_objects(root: &Path) -> Result<(), String> {
    let (version, promisor) = version_and_promisor(root)?;
    promisor_refusal(&version, promisor).map_or(Ok(()), Err)
}

/// Give a git process that reads staged or indexed objects the scan's pins: no rename
/// detection through configuration, no caller configuration, no lazy fetch.
pub fn pin_object_reader(command: &mut Command) {
    pin_git_output(command, &[]);
}

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
/// or staged can change it. The attributes file is made in the scan's
/// private folder (see [`ScanFolder`]).
struct TextAttributes {
    empty_tree: String,
}

impl TextAttributes {
    fn for_repository(root: &Path) -> Result<Self, String> {
        let (version, promisor) = version_and_promisor(root)?;
        if let Some(reason) = git_refusal(&version, promisor) {
            return Err(reason);
        }
        let out = capability_query(root, &["hash-object", "-t", "tree", "--stdin"])?;
        if !out.status.success() {
            return Err(format!(
                "`git hash-object -t tree --stdin` exited with {}",
                out.status
            ));
        }
        let empty_tree = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(Self { empty_tree })
    }

    fn apply(&self, command: &mut Command) {
        command
            .env("GIT_ATTR_SOURCE", &self.empty_tree)
            .env("GIT_ATTR_NOSYSTEM", "1");
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
    fn only_a_clean_exit_of_the_staged_diff_in_gits_trace_is_complete() {
        let start = |sid: &str, argv: &[&str]| {
            serde_json::json!({"event": "start", "sid": sid, "argv": argv}).to_string()
        };
        let exit = |sid: &str, code: i64| {
            serde_json::json!({"event": "exit", "sid": sid, "code": code}).to_string()
        };
        // A checkout path holding a newline and a forged event stays one
        // JSON string inside one event.
        let root = "/tmp/x-\n{\"event\":\"exit\",\"sid\":\"b\",\"code\":0}";
        let diff = |sid: &str| {
            let mut argv = vec!["git", "-C", root];
            argv.extend(STAGED_DIFF_ARGS);
            start(sid, &argv)
        };
        let blob = |sid: &str| start(sid, &["git", "-C", root, "cat-file", "blob", ":a.zip"]);
        let read = |lines: &[String]| git_run_failure(lines.join("\n").as_bytes());
        assert_eq!(read(&[diff("a"), exit("a", 0)]), None);
        assert_eq!(
            read(&[diff("a"), blob("b"), exit("b", 0), exit("a", 0)]),
            None
        );
        for (lines, why) in [
            (vec![], "0 staged diffs"),
            (vec![blob("b"), exit("b", 0)], "0 staged diffs"),
            (
                vec![diff("a"), exit("a", 128)],
                "the staged diff exited with 128",
            ),
            (vec![diff("a")], "the staged diff ended without exiting"),
            (
                vec![diff("a"), blob("b"), exit("a", 0)],
                "a git process gitleaks started ended",
            ),
            (
                vec![diff("a"), exit("a", 0), diff("c"), exit("c", 0)],
                "2 staged diffs",
            ),
            (
                vec![diff("a"), "11:17AM ERR forged".to_string()],
                "could not be read",
            ),
        ] {
            let found = read(&lines).unwrap_or_default();
            assert!(found.contains(why), "{lines:?}: {found}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_scan_folder_is_private_and_bound_to_the_folder_it_made() {
        use std::os::unix::fs::PermissionsExt;
        let git_dir = tempfile::tempdir().unwrap();
        let made = ScanFolder::new(git_dir.path()).unwrap();
        let mode_of = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(made.folder.path().parent(), Some(git_dir.path()));
        assert_eq!(mode_of(made.folder.path()), 0o700);
        assert_eq!(mode_of(&made.path(EVENTS)), 0o600);
        assert_eq!(mode_of(&made.path(ATTRIBUTES)), 0o600);
        assert_eq!(std::fs::read(made.path(ATTRIBUTES)).unwrap(), b"* diff\n");
        // A file or link already there is never opened.
        let error = private::create_in(&made.held, EVENTS).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        std::os::unix::fs::symlink("/nonexistent", made.path("link")).unwrap();
        assert!(private::create_in(&made.held, "link").is_err());
        // Events git appends are read back through the handle.
        let mut appender = std::fs::OpenOptions::new()
            .append(true)
            .open(made.path(EVENTS))
            .unwrap();
        std::io::Write::write_all(&mut appender, b"{}\n").unwrap();
        assert_eq!(made.events().unwrap(), b"{}\n");
        // A file put in the trace's place fails the scan.
        std::fs::write(made.path("other"), b"forged\n").unwrap();
        std::fs::rename(made.path("other"), made.path(EVENTS)).unwrap();
        assert_eq!(
            made.events().unwrap_err(),
            "git's trace of the scan was replaced while it ran"
        );
    }

    /// The folder is moved away and another put at its path between its
    /// creation and the files': the files are still made in the folder that
    /// was created, and the scan fails.
    #[cfg(unix)]
    #[test]
    fn a_folder_substituted_during_creation_is_never_written_and_fails_the_scan() {
        let git_dir = tempfile::tempdir().unwrap();
        let made = tempfile::Builder::new().tempdir_in(git_dir.path()).unwrap();
        std::fs::set_permissions(
            made.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        let held = private::open_folder(made.path()).unwrap();
        let moved = git_dir.path().join("moved");
        std::fs::rename(made.path(), &moved).unwrap();
        std::fs::create_dir(made.path()).unwrap();
        let events = private::create_in(&held, EVENTS).unwrap();
        assert!(moved.join(EVENTS).exists());
        assert!(!made.path().join(EVENTS).exists());
        let folder = ScanFolder {
            folder: made,
            events,
            held,
        };
        assert_eq!(
            folder.events().unwrap_err(),
            "the scan's private folder was replaced while it ran"
        );
        // A folder open to others, or a link to one, is refused outright.
        let open = git_dir.path().join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::os::unix::fs::PermissionsExt::from_mode(0o777))
            .unwrap();
        assert!(private::open_folder(&open).is_err());
        let link = git_dir.path().join("link");
        std::os::unix::fs::symlink(&moved, &link).unwrap();
        assert!(private::open_folder(&link).is_err());
    }

    #[test]
    fn any_stderr_output_is_named_with_control_characters_escaped() {
        assert_eq!(first_line(b""), None);
        assert_eq!(first_line(b"\n  \n"), None);
        assert_eq!(
            first_line(b"\nERR [git] fatal: bad\x1b[31m\nnext").as_deref(),
            Some("ERR [git] fatal: bad\\u{1b}[31m")
        );
    }

    #[test]
    fn a_promisor_repository_needs_git_2_46() {
        for old in [
            "git version 2.41.0",
            "git version 2.44.0",
            "git version 2.45.2",
        ] {
            assert!(git_refusal(old, false).is_none(), "{old}");
            let reason = git_refusal(old, true).expect(old);
            assert!(reason.contains("2.46"), "{reason}");
            assert!(promisor_refusal(old, true).is_some(), "{old}");
        }
        assert!(promisor_refusal("git version 2.45.9", true).is_some());
        for new in [
            "git version 2.46.0",
            "git version 2.53.0 (Apple Git-1)",
            "git version 3.0.0",
        ] {
            assert!(git_refusal(new, true).is_none(), "{new}");
            assert!(promisor_refusal(new, true).is_none(), "{new}");
        }
        assert!(promisor_refusal("not git", true).is_some());
        assert!(promisor_refusal("not git", false).is_none());
        assert!(git_refusal("git version 2.40.1", false).is_some());
        assert!(git_refusal("not git", false).is_some());
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
        let absolute = tempfile::tempdir().unwrap();
        let path = std::env::join_paths([
            PathBuf::new(),
            PathBuf::from("."),
            PathBuf::from("bin"),
            absolute.path().to_path_buf(),
        ])
        .unwrap();
        let engine = Engine::with_path(Some(&path));
        assert_eq!(engine.search, [absolute.path().to_path_buf()]);
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
