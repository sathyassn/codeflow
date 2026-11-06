//! The range scan behind `codeflow scan` (TSK-1176 AC-6 and AC-7, moved
//! from TSK-430): the pinned gitleaks over the commits `head` holds and
//! `base` does not, read the way the managed `secret scan` job reads
//! a pull request.
//!
//! The base is the trusted commit here, as the default-branch authority is
//! in the hosted job: `.gitleaksignore`, `.gitleaks.toml` (or a
//! `.gitleaks.json` beside it), every file that configuration extends,
//! `.gitattributes` and `git.secret_scan_inline_allow` are all read from the
//! base commit, never from the working tree or the range. The same preflight
//! refuses what gitleaks cannot read reliably: an octopus merge, and a path
//! holding a backslash, a double quote or a control character that a commit
//! in the range changes or that either side of a merge in it changes. The
//! same log options read every introduced commit, a file added and later
//! deleted included, and the same `CodeFlow` 3.0.0 pipeline prose is the only
//! finding dropped. An inline `gitleaks:allow` never hides a finding: no
//! commit in the range is held by its base, so the hosted job would not count
//! one either.
//!
//! What differs from the hosted job: gitleaks runs from `PATH` after its
//! checksum matched the pin, never downloaded; `GITLEAKS_CONFIG` and
//! `GITLEAKS_CONFIG_TOML` from the caller's shell are ignored (the hosted
//! job reads them only from its own workflow env); the advisory
//! legacy-fingerprint warnings are not repeated; and the clone's own
//! `info/attributes` still applies. On a platform without a POSIX shell the
//! wrapper that drops git's `remerge` header lines cannot run, so a range
//! holding a merge commit is refused there instead of read differently.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use super::{
    first_line, git_dir, git_refusal, remove_caller_settings, version_and_promisor, Engine,
    ScanFolder, Verified, FINDINGS_EXIT,
};

/// The git settings the hosted job's git wrapper pins for gitleaks' log,
/// plus rename detection at git's default, so a caller's configuration
/// cannot change the patch gitleaks parses.
const RANGE_PINS: [(&str, &str); 15] = [
    ("core.quotePath", "true"),
    ("diff.noprefix", "false"),
    ("diff.mnemonicPrefix", "false"),
    ("diff.srcPrefix", "a/"),
    ("diff.dstPrefix", "b/"),
    ("diff.relative", "false"),
    ("diff.renames", "true"),
    ("color.ui", "false"),
    ("color.diff", "false"),
    ("format.pretty", "medium"),
    ("log.date", "default"),
    ("log.decorate", "false"),
    ("log.abbrevCommit", "false"),
    ("log.showSignature", "false"),
    ("log.showRoot", "true"),
];

/// The log options of the hosted job, before the range.
const LOG_OPTS: &str =
    "--full-history --diff-filter=uxdb --remerge-diff --text --no-ext-diff --no-textconv";

/// The only finding the hosted job drops: the security-stage prose `CodeFlow`
/// 3.0.0 seeded in the pipeline workflow, which `generic-api-key` mistakes
/// for a key. The text is split so this file does not hold it.
fn known_false_positive(finding: &serde_json::Value) -> bool {
    let text = |key: &str| finding.get(key).and_then(serde_json::Value::as_str);
    let prose = format!("{}{}", "vulnerable/", "malicious");
    text("RuleID") == Some("generic-api-key")
        && text("Secret") == Some(prose.as_str())
        && text("Match") == Some(format!("authz gaps, {prose} ").as_str())
        && matches!(
            text("File"),
            Some(
                ".claude/workflows/pipeline.workflow.js"
                    | ".codeflow/.baseline/.claude/workflows/pipeline.workflow.js"
            )
        )
}

/// One finding of the range scan. Its secret is never kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeFinding {
    /// The gitleaks rule id, such as `generic-api-key`.
    pub rule: String,
    /// The file, as gitleaks names it.
    pub file: String,
    /// The line in that file at the commit.
    pub line: u64,
    /// The commit that introduced it.
    pub commit: String,
}

/// What the range scan read and found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeReport {
    /// The resolved base commit.
    pub base: String,
    /// The resolved head commit.
    pub head: String,
    /// How many commits the range holds.
    pub commits: usize,
    /// `true` when no commit in the range changes a file, so nothing ran.
    pub nothing_to_scan: bool,
    /// Findings of the 3.0.0 pipeline prose that were dropped.
    pub known_allowed: usize,
    /// Every other finding; any one fails the scan.
    pub findings: Vec<RangeFinding>,
}

/// A git process for the range scan, run in `root`: the repository is the
/// one `root` names (inherited location variables are removed), caller
/// configuration is removed, replace refs are ignored, missing objects are
/// never fetched and the output settings are pinned. Attributes come from
/// `attributes_from` when it is given.
fn range_git(root: &Path, attributes_from: Option<&str>, extra: &[(&str, OsString)]) -> Command {
    let mut command = crate::git::judging_command();
    pin_range(&mut command, attributes_from, extra);
    command.current_dir(root).stdin(Stdio::null());
    command
}

fn pin_range(command: &mut Command, attributes_from: Option<&str>, extra: &[(&str, OsString)]) {
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
        "GIT_DIFF_OPTS",
        "GIT_EXTERNAL_DIFF",
    ] {
        command.env_remove(name);
    }
    command.env("GIT_NO_REPLACE_OBJECTS", "1");
    crate::git::local_objects_only(command);
    remove_caller_settings(command);
    if let Some(tree) = attributes_from {
        command
            .env("GIT_ATTR_SOURCE", tree)
            .env("GIT_ATTR_NOSYSTEM", "1");
    }
    let pins = RANGE_PINS
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

/// Run a range git query; an exit code outside `codes` is an error naming
/// the subcommand.
fn run(root: &Path, args: &[&str], codes: &[i32]) -> Result<Output, String> {
    let out = range_git(root, None, &[])
        .args(args)
        .output()
        .map_err(|error| format!("git did not run: {error}"))?;
    if out.status.code().is_some_and(|code| codes.contains(&code)) {
        Ok(out)
    } else {
        Err(format!(
            "`git {}` exited with {}{}",
            args.first().copied().unwrap_or_default(),
            out.status,
            first_line(&out.stderr).map_or_else(String::new, |line| format!(": {line}"))
        ))
    }
}

/// The words of git's ASCII output (object ids and fixed words), split on
/// the space and line break git writes between them. Output that is not
/// UTF-8 is refused, never decoded with replacements.
fn ascii_words(out: &[u8]) -> Result<Vec<String>, String> {
    let text = std::str::from_utf8(out)
        .map_err(|_| "git printed object ids that are not text".to_string())?;
    Ok(text
        .split(['\n', ' '])
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect())
}

/// The full commit id `rev` names, read with replace refs ignored.
///
/// # Errors
/// When `rev` names no commit in the repository at `root`.
pub fn resolve(root: &Path, rev: &str) -> Result<String, String> {
    if rev.starts_with('-') {
        return Err(format!(
            "'{}' is not a revision",
            crate::git::display_text(rev)
        ));
    }
    let out = run(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ],
        &[0, 1],
    )?;
    let words = ascii_words(&out.stdout)?;
    if out.status.success() && words.len() == 1 {
        Ok(words[0].clone())
    } else {
        Err(format!(
            "'{}' names no commit in this repository; fetch it first",
            crate::git::display_text(rev)
        ))
    }
}

/// The regular file at `path` in `commit`, or `None` when the commit holds
/// nothing there.
///
/// # Errors
/// When the entry is not a regular file, or git fails.
pub fn blob(root: &Path, commit: &str, path: &str) -> Result<Option<Vec<u8>>, String> {
    let listing = range_git(root, None, &[])
        .env("GIT_LITERAL_PATHSPECS", "1")
        .args(["ls-tree", "-z", commit, "--", path])
        .output()
        .map_err(|error| format!("git did not run: {error}"))?;
    if !listing.status.success() {
        return Err(format!("cannot read the tree of {commit}"));
    }
    let entry = listing.stdout.split(|b| *b == 0).next().unwrap_or_default();
    if entry.is_empty() {
        return Ok(None);
    }
    let meta = entry.split(|b| *b == b'\t').next().unwrap_or_default();
    let fields: Vec<&[u8]> = meta.split(|b| *b == b' ').collect();
    if fields.len() != 3 || fields[1] != b"blob" || !matches!(fields[0], b"100644" | b"100755") {
        return Err(format!(
            "{} on {commit} is not a regular file; commit the file itself",
            crate::git::display_text(path)
        ));
    }
    let oid =
        std::str::from_utf8(fields[2]).map_err(|_| format!("cannot read the tree of {commit}"))?;
    let out = run(root, &["cat-file", "blob", oid], &[0])?;
    Ok(Some(out.stdout))
}

/// Whether a path holds a character gitleaks' patch parser cannot read
/// reliably: a backslash, a double quote or a control character.
fn odd(name: &[u8]) -> bool {
    name.iter()
        .any(|byte| *byte < 32 || *byte == 127 || matches!(byte, b'"' | b'\\'))
}

fn named(name: &[u8]) -> String {
    crate::git::name::GitName::from_bytes(name)
        .display()
        .to_string()
}

const UNREADABLE: &str =
    "gitleaks cannot read a backslash, double quote or control character in a path reliably";
const REWRITE: &str = "rewrite the range's commits so that none uses the name; renaming it in a later commit leaves it in this one";

/// The commits `head` holds and `base` does not, each with its parents.
fn commits(root: &Path, base: &str, head: &str) -> Result<Vec<Vec<String>>, String> {
    let out = run(
        root,
        &["rev-list", "--parents", head, "--not", base, "--"],
        &[0],
    )?;
    out.stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(ascii_words)
        .collect()
}

/// The hosted job's preflight over the range: an octopus merge is refused,
/// and so is a path gitleaks cannot read that a commit adds, changes,
/// renames or copies, or that either side of a merge changes since a merge
/// base.
fn preflight(root: &Path, base: &str, head: &str, list: &[Vec<String>]) -> Result<(), String> {
    for ids in list {
        if ids.len() > 3 {
            return Err(format!(
                "octopus merge {} is in the range, and git cannot show what it adds; rewrite the history to merge its branches one at a time",
                ids[0]
            ));
        }
    }
    let listing = run(
        root,
        &[
            "log",
            "-z",
            "--format=%x01%H",
            "--name-status",
            "--diff-filter=uxdb",
            "--remerge-diff",
            "--no-ext-diff",
            head,
            "--not",
            base,
            "--",
        ],
        &[0],
    )?;
    let mut commit = String::new();
    let mut names = 0;
    for token in listing.stdout.split(|b| *b == 0) {
        if names > 0 {
            names -= 1;
            if odd(token) {
                return Err(format!(
                    "commit {commit} adds, changes or renames the path \"{}\"; {UNREADABLE}; {REWRITE}",
                    named(token)
                ));
            }
            continue;
        }
        let token = token.strip_prefix(b"\n").unwrap_or(token);
        let status = |letters: &[u8]| {
            token.first().is_some_and(|first| letters.contains(first))
                && token[1..].iter().all(u8::is_ascii_digit)
        };
        if let Some(id) = token.strip_prefix(b"\x01") {
            commit = ascii_words(id)?.concat();
        } else if status(b"RC") {
            names = 2;
        } else if token.first().is_some_and(u8::is_ascii_uppercase)
            && status(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ")
        {
            names = 1;
        } else if !token.is_empty() {
            return Err(format!(
                "cannot read git's list of the paths that commit {commit} changes"
            ));
        }
    }
    let empty =
        ascii_words(&run(root, &["hash-object", "-t", "tree", "--stdin"], &[0])?.stdout)?.concat();
    for ids in list.iter().filter(|ids| ids.len() == 3) {
        let (merge, parents) = (&ids[0], &ids[1..]);
        let found = run(
            root,
            &["merge-base", "--all", &parents[0], &parents[1], "--"],
            &[0, 1],
        )?;
        let mut bases = ascii_words(&found.stdout)?;
        if bases.is_empty() {
            bases.push(empty.clone());
        }
        for merge_base in &bases {
            for parent in parents {
                let diff = run(
                    root,
                    &[
                        "diff-tree",
                        "-r",
                        "-z",
                        "--name-only",
                        merge_base,
                        parent,
                        "--",
                    ],
                    &[0],
                )?;
                if let Some(name) = diff.stdout.split(|b| *b == 0).find(|name| odd(name)) {
                    return Err(format!(
                        "merge {merge} joins a change to the path \"{}\" made on its parent {parent}; {UNREADABLE}; rebase onto the base instead of merging it in, or {REWRITE}",
                        named(name)
                    ));
                }
            }
        }
    }
    Ok(())
}

/// `path` relative to the repository root, normalized; `None` when it
/// leaves the repository.
fn normalized(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// Whether `part`, one `/`-separated component of a path git lists, names
/// a plain entry inside its folder. Empty, `.` and `..` are refused, and
/// so is anything the platform parses as more than one plain name: a
/// Windows drive prefix, root or `\` separator. On Windows a `:` (an
/// alternate data stream) and a trailing dot or space (which Windows
/// drops, so `..` could hide as `.. `) are refused too.
fn plain_component(part: &str) -> bool {
    if part.is_empty() || part == "." || part == ".." {
        return false;
    }
    if cfg!(windows) && (part.contains(['\\', ':']) || part.ends_with(['.', ' '])) {
        return false;
    }
    let mut components = Path::new(part).components();
    matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(name)), None) if name == std::ffi::OsStr::new(part)
    )
}

/// A private folder that files read from git are copied into.
///
/// A path git lists comes from tree objects, and a crafted tree can hold
/// entries named `..` (git lists them as `../name`) or, read on Windows,
/// names with `\`. Each path is therefore checked one component at a time
/// before anything is written, and each folder written into must resolve
/// inside this one. Anything else is refused, and the caller fails closed.
pub(crate) struct Scratch {
    /// The folder, resolved.
    root: PathBuf,
    /// Folders already checked to resolve inside `root`.
    checked: std::collections::HashSet<PathBuf>,
}

impl Scratch {
    /// Make `root` (and its parents) and resolve it.
    ///
    /// # Errors
    /// Why the folder cannot be made.
    pub(crate) fn new(root: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(root)
            .and_then(|()| root.canonicalize())
            .map(|root| Self {
                root,
                checked: std::collections::HashSet::new(),
            })
            .map_err(|error| format!("cannot make the private folder: {error}"))
    }

    /// The folder, resolved.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Where `path`, as git lists it, goes in the folder.
    ///
    /// # Errors
    /// `path` is absolute or holds an empty, `.`, `..` or non-plain
    /// component.
    pub(crate) fn target(&self, path: &str) -> Result<PathBuf, String> {
        let mut target = self.root.clone();
        for part in path.split('/') {
            if !plain_component(part) {
                return Err(outside(path));
            }
            target.push(part);
        }
        Ok(target)
    }

    /// Write `data` at `path`, as git lists it, making its folders.
    ///
    /// # Errors
    /// The path is refused by [`Scratch::target`], a folder on the way
    /// resolves outside the folder, the file is a link, or the write fails.
    pub(crate) fn write(&mut self, path: &str, data: &[u8]) -> Result<(), String> {
        let target = self.target(path)?;
        let parent = target.parent().unwrap_or(&self.root).to_path_buf();
        if !self.checked.contains(&parent) {
            std::fs::create_dir_all(&parent)
                .map_err(|error| format!("cannot copy {path}: {error}"))?;
            let real = parent
                .canonicalize()
                .map_err(|error| format!("cannot copy {path}: {error}"))?;
            if !real.starts_with(&self.root) {
                return Err(outside(path));
            }
            self.checked.insert(parent);
        }
        if std::fs::symlink_metadata(&target).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(outside(path));
        }
        std::fs::write(&target, data).map_err(|error| format!("cannot copy {path}: {error}"))
    }
}

/// The refusal for a path that would be written outside its folder.
fn outside(path: &str) -> String {
    format!(
        "the commit lists the path \"{}\", which would be written outside the private folder it is copied into; nothing was copied there, and the scan fails closed",
        crate::git::display_text(path)
    )
}

/// The `[extend] path` of a gitleaks configuration, keys matched without
/// case as gitleaks reads them; `None` when it has none or does not parse
/// (gitleaks then fails to load it).
fn extended(text: &str) -> Option<String> {
    let value: serde_json::Value = toml::from_str::<toml::Value>(text)
        .ok()
        .and_then(|value| serde_json::to_value(value).ok())
        .or_else(|| serde_json::from_str(text).ok())?;
    let lookup = |value: &serde_json::Value, key: &str| {
        value
            .as_object()?
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.clone())
    };
    let extend = lookup(&value, "extend")?;
    let path = lookup(&extend, "path")?;
    path.as_str()
        .filter(|path| !path.is_empty())
        .map(str::to_string)
}

/// The base commit's scan inputs, written into the scan's private folder:
/// the ignore file, the configuration and every file it extends.
fn write_inputs(root: &Path, base: &str, folder: &ScanFolder) -> Result<(), String> {
    let policy = blob(root, base, ".codeflow/policy.json")?;
    if let Some(policy) = policy {
        let data: serde_json::Value = serde_json::from_slice(&policy).map_err(|_| {
            "cannot read git.secret_scan_inline_allow from the base's policy; commit a JSON object with trusted or never".to_string()
        })?;
        let allow = data
            .get("git")
            .and_then(|git| git.get("secret_scan_inline_allow"))
            .unwrap_or(&serde_json::Value::Null);
        if !(allow.is_null() || allow == "trusted" || allow == "never") {
            return Err(
                "git.secret_scan_inline_allow on the base must be trusted or never".to_string(),
            );
        }
    }
    let mut ignore = folder.create("ignore")?;
    if let Some(data) = blob(root, base, ".gitleaksignore")? {
        ignore
            .write_all(&data)
            .map_err(|error| format!("cannot write the ignore file: {error}"))?;
    }
    let tree = folder.path("tree");
    std::fs::create_dir(&tree)
        .map_err(|error| format!("cannot make the scan's tree folder: {error}"))?;
    let mut tree = Scratch::new(&tree)?;
    let config = match blob(root, base, ".gitleaks.toml")? {
        Some(toml) => Some(match blob(root, base, ".gitleaks.json")? {
            Some(json) => (json, ".gitleaks.json".to_string()),
            None => (toml, ".gitleaks.toml".to_string()),
        }),
        None => None,
    };
    let mut file = folder.create("config.toml")?;
    let Some((data, mut owner)) = config else {
        return file
            .write_all(b"[extend]\nuseDefault = true\n")
            .map_err(|error| format!("cannot write the configuration: {error}"));
    };
    file.write_all(&data)
        .map_err(|error| format!("cannot write the configuration: {error}"))?;
    let workspace = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut text = String::from_utf8_lossy(&data).to_string();
    let mut seen: Vec<String> = Vec::new();
    while let Some(path) = extended(&text) {
        if Path::new(&path).is_absolute() {
            let real = Path::new(&path)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&path));
            if real.starts_with(&workspace) || Path::new(&path).starts_with(root) {
                return Err(format!(
                    "{owner} extends {}, a file in the checkout that the range can change; name it relative to the repository root",
                    crate::git::display_text(&path)
                ));
            }
            if seen.contains(&path) || !Path::new(&path).is_file() {
                return Ok(());
            }
            seen.push(path.clone());
            text = std::fs::read_to_string(&path).map_err(|error| {
                format!(
                    "cannot read {}, which {owner} extends: {error}",
                    crate::git::display_text(&path)
                )
            })?;
            owner = path;
            continue;
        }
        let Some(rel) = normalized(&path) else {
            return Err(format!(
                "{owner} names {}, outside the repository; use an absolute path",
                crate::git::display_text(&path)
            ));
        };
        if seen.contains(&rel) {
            return Ok(());
        }
        let Some(data) = blob(root, base, &rel)? else {
            return Err(format!(
                "{owner} names {}, which the base {base} does not hold; commit it there first",
                crate::git::display_text(&rel)
            ));
        };
        tree.write(&rel, &data)?;
        seen.push(rel.clone());
        text = String::from_utf8_lossy(&data).to_string();
        owner = rel;
    }
    Ok(())
}

/// The first absolute `git` on `PATH`, the one gitleaks would run.
#[cfg(unix)]
fn real_git() -> Result<PathBuf, String> {
    let Some(path) = std::env::var_os("PATH") else {
        return Err("PATH is not set, so git cannot be found".to_string());
    };
    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("git"))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| "git is not on PATH".to_string())
}

/// On Unix, a `git` in the private folder that drops the `remerge` header
/// lines git writes into a conflicted merge's patch, as the hosted job's
/// wrapper does: gitleaks' parser would name that file `b/<path>`, so an
/// exemption for a real `b/` path could cover it.
#[cfg(unix)]
fn write_wrapper(folder: &ScanFolder) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    let git = real_git()?;
    let quoted = format!("'{}'", git.display().to_string().replace('\'', r"'\''"));
    let bin = folder.path("bin");
    std::fs::create_dir(&bin)
        .map_err(|error| format!("cannot make the wrapper folder: {error}"))?;
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = -C ] && [ \"$3\" = log ]; then\n  {quoted} \"$@\" | sed '/^remerge /d'\n  exit 0\nfi\nexec {quoted} \"$@\"\n"
    );
    let path = bin.join("git");
    std::fs::write(&path, script)
        .map_err(|error| format!("cannot write the git wrapper: {error}"))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("cannot write the git wrapper: {error}"))?;
    Ok(bin)
}

/// Why the git processes gitleaks ran did not read the whole range: none
/// was a `log`, or one did not exit with 0 (one killed records no exit).
/// gitleaks' remote URL lookup (`git ls-remote --quiet --get-url`) is the
/// one process left out.
fn range_run_failure(events: &[u8]) -> Option<String> {
    let mut started: Vec<(String, bool)> = Vec::new();
    let mut exits: std::collections::HashMap<String, Option<i64>> =
        std::collections::HashMap::new();
    for line in events
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
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
                let log = argv.len() > 3 && argv[1] == "-C" && argv[3] == "log";
                // gitleaks asks for the remote's URL, to link findings, from
                // its working folder, which is no repository; that lookup
                // reads no object and its failure changes no finding.
                if argv.get(1..) == Some(&["ls-remote", "--quiet", "--get-url"][..]) {
                    continue;
                }
                started.push((sid.to_string(), log));
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
    if !started.iter().any(|(_, log)| *log) {
        return Some("git's trace shows no log of the range".to_string());
    }
    for (sid, log) in &started {
        let what = if *log {
            "the log of the range"
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

/// The gitleaks command the hosted job runs over `range`, reading only the
/// scan folder's inputs, with git pinned and, on Unix, the remerge header
/// filter first on `PATH`.
fn gitleaks_command(
    verified: &Verified,
    folder: &ScanFolder,
    source: &Path,
    range: &str,
    base: &str,
    report_path: &Path,
) -> Result<Command, String> {
    let mut command = Command::new(&verified.path);
    command
        .current_dir(folder.path("tree"))
        .arg("detect")
        .arg("--source")
        .arg(source)
        .arg("--config")
        .arg(folder.path("config.toml"))
        .arg("--log-opts")
        .arg(format!("{LOG_OPTS} {range} --"))
        .arg("--gitleaks-ignore-path")
        .arg(folder.path("ignore"))
        .args(["--ignore-gitleaks-allow", "--no-banner", "--no-color"])
        .args(["--log-level", "error"])
        .args(["--exit-code", &FINDINGS_EXIT.to_string()])
        .args(["--report-format", "json", "--report-path"])
        .arg(report_path)
        .env_remove("GITLEAKS_CONFIG")
        .env_remove("GITLEAKS_CONFIG_TOML")
        .stdin(Stdio::null());
    pin_range(&mut command, Some(base), &folder.attributes_config());
    folder.apply(&mut command);
    #[cfg(unix)]
    {
        let bin = write_wrapper(folder)?;
        let mut path = vec![bin];
        if let Some(inherited) = std::env::var_os("PATH") {
            path.extend(std::env::split_paths(&inherited));
        }
        command.env(
            "PATH",
            std::env::join_paths(path).map_err(|error| format!("cannot set PATH: {error}"))?,
        );
    }
    Ok(command)
}

/// Add the findings of a gitleaks JSON report to `report`, dropping the
/// known false positive. `found` is whether gitleaks exited with its
/// findings code, which a report listing nothing contradicts.
fn read_findings(raw: &[u8], found: bool, report: &mut RangeReport) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_slice(raw)
        .map_err(|error| format!("cannot read the gitleaks report: {error}"))?;
    let entries = value
        .as_array()
        .ok_or_else(|| "cannot read the gitleaks report: it is not a list".to_string())?;
    for entry in entries {
        if known_false_positive(entry) {
            report.known_allowed += 1;
            continue;
        }
        let text = |key: &str| {
            entry
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("cannot read the gitleaks report: a finding has no {key}"))
        };
        report.findings.push(RangeFinding {
            rule: text("RuleID")?,
            file: text("File")?,
            line: entry
                .get("StartLine")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    "cannot read the gitleaks report: a finding has no StartLine".to_string()
                })?,
            commit: text("Commit")?,
        });
    }
    if found && entries.is_empty() {
        return Err("gitleaks reported findings but its report lists none".to_string());
    }
    Ok(())
}

impl Engine {
    /// Run the verified binary over `base..head` in the repository at
    /// `root`, as the hosted `secret scan` job runs it for a pull
    /// request whose base is `base`.
    ///
    /// # Errors
    /// Returns why the range cannot be scanned or the scan is incomplete: a
    /// revision that names no commit, a git too old to read attributes from
    /// the base, the preflight's refusal, an unreadable base input, a failed
    /// binary, any output gitleaks wrote to stderr, a git it ran that did not
    /// exit with 0, or a report that could not be read.
    pub fn scan_range(
        &self,
        verified: &Verified,
        root: &Path,
        base: &str,
        head: &str,
    ) -> Result<RangeReport, String> {
        let (version, promisor) = version_and_promisor(root)?;
        if let Some(reason) = git_refusal(&version, promisor) {
            return Err(reason);
        }
        let base = resolve(root, base)?;
        let head = resolve(root, head)?;
        let list = commits(root, &base, &head)?;
        preflight(root, &base, &head, &list)?;
        let mut report = RangeReport {
            base: base.clone(),
            head: head.clone(),
            commits: list.len(),
            nothing_to_scan: false,
            known_allowed: 0,
            findings: Vec::new(),
        };
        let range = format!("{base}..{head}");
        let files = run(
            root,
            &[
                "log",
                "--format=",
                "--name-only",
                "--diff-filter=uxdb",
                "--remerge-diff",
                "--no-ext-diff",
                &range,
                "--",
            ],
            &[0],
        )?;
        if files.stdout.iter().all(u8::is_ascii_whitespace) {
            report.nothing_to_scan = true;
            return Ok(report);
        }
        #[cfg(not(unix))]
        if list.iter().any(|ids| ids.len() > 2) {
            return Err("the range holds a merge commit, which this platform cannot read the way the hosted scan does (it needs a POSIX shell to drop git's remerge headers); let the hosted job judge it, or scan from Linux or macOS".to_string());
        }
        let folder = ScanFolder::new(&git_dir(root)?)?;
        write_inputs(root, &base, &folder)?;
        let source = folder.path("source");
        std::fs::create_dir(&source)
            .map_err(|error| format!("cannot make the scan's source folder: {error}"))?;
        std::fs::write(
            source.join(".git"),
            format!("gitdir: {}\n", git_dir(root)?.display()),
        )
        .map_err(|error| format!("cannot write the scan's source folder: {error}"))?;
        let report_path = folder.path("report.json");
        let mut command =
            gitleaks_command(verified, &folder, &source, &range, &base, &report_path)?;
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
        if let Some(why) = range_run_failure(&folder.events()?) {
            return Err(format!("gitleaks did not read the whole range: {why}"));
        }
        folder.intact()?;
        let raw = std::fs::read(&report_path)
            .map_err(|error| format!("cannot read the gitleaks report: {error}"))?;
        read_findings(&raw, code == Some(FINDINGS_EXIT), &mut report)?;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_gitleaks_cannot_read_are_odd() {
        assert!(odd(b"a\\b"));
        assert!(odd(b"a\"b"));
        assert!(odd(b"a\nb"));
        assert!(!odd(b"plain name/with spaces"));
        assert!(!odd("caf\u{e9}".as_bytes()));
    }

    #[test]
    fn extend_paths_are_read_without_case_and_normalized() {
        assert_eq!(
            extended("[Extend]\nPath = \"rules/base.toml\"\n").as_deref(),
            Some("rules/base.toml")
        );
        assert_eq!(extended("[extend]\nuseDefault = true\n"), None);
        assert_eq!(
            normalized("./rules/../rules/a.toml").as_deref(),
            Some("rules/a.toml")
        );
        assert_eq!(normalized("../outside.toml"), None);
    }

    /// Security review F1 and its sibling sweep: every path a tree can list
    /// that is not plain names inside the folder is refused, and a folder
    /// that resolves outside it is refused before the write.
    #[test]
    fn a_scratch_folder_takes_only_plain_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let mut scratch = Scratch::new(&dir.path().join("inner")).unwrap();
        for path in [
            "",
            "/etc/passwd",
            "../x",
            "a/../../x",
            "a//b",
            "./a",
            "a/./b",
            "a/",
            "..",
        ] {
            assert!(scratch.target(path).is_err(), "{path:?} was accepted");
            assert!(scratch.write(path, b"x").is_err(), "{path:?} was written");
        }
        if cfg!(windows) {
            for path in ["a\\..\\..\\x", "C:x", "a/b:stream", "a/.. ", "a/b."] {
                assert!(scratch.target(path).is_err(), "{path:?} was accepted");
            }
        }
        scratch.write("a/b/c.txt", b"ok").unwrap();
        assert_eq!(
            std::fs::read(scratch.root().join("a").join("b").join("c.txt")).unwrap(),
            b"ok"
        );
        #[cfg(unix)]
        {
            let away = dir.path().join("away");
            std::fs::create_dir(&away).unwrap();
            std::os::unix::fs::symlink(&away, scratch.root().join("link")).unwrap();
            assert!(scratch.write("link/x.txt", b"x").is_err());
            assert!(!away.join("x.txt").exists());
        }
    }

    #[test]
    fn only_the_pipeline_prose_is_a_known_false_positive() {
        let prose = format!("{}{}", "vulnerable/", "malicious");
        let finding = serde_json::json!({
            "RuleID": "generic-api-key",
            "Secret": prose,
            "Match": format!("authz gaps, {prose} "),
            "File": ".claude/workflows/pipeline.workflow.js",
        });
        assert!(known_false_positive(&finding));
        let mut other = finding.clone();
        other["File"] = serde_json::json!("src/pipeline.workflow.js");
        assert!(!known_false_positive(&other));
    }

    #[test]
    fn a_range_scan_needs_a_clean_log_in_gits_trace() {
        let start = r#"{"event":"start","sid":"a","argv":["git","-C","/s","log","-p"]}"#;
        let exit = |code: i32| format!(r#"{{"event":"exit","sid":"a","code":{code}}}"#);
        assert_eq!(
            range_run_failure(format!("{start}\n{}\n", exit(0)).as_bytes()),
            None
        );
        assert!(range_run_failure(format!("{start}\n{}\n", exit(128)).as_bytes()).is_some());
        assert!(range_run_failure(start.as_bytes()).is_some());
        assert!(range_run_failure(b"").is_some());
        let lookup =
            r#"{"event":"start","sid":"b","argv":["git","ls-remote","--quiet","--get-url"]}"#;
        let failed = r#"{"event":"exit","sid":"b","code":128}"#;
        assert_eq!(
            range_run_failure(format!("{start}\n{}\n{lookup}\n{failed}\n", exit(0)).as_bytes()),
            None
        );
    }
}
