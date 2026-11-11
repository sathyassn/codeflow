//! Enforcement-path protection for native file-edit tools.
//! Native Grok captures, a documented Codex schema, and synthetic path edge cases.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};

use super::{PolicyLevel, Violation};
use crate::security::actions;
use crate::security::pattern::{
    match_extended_glob, GlobMatch, MAX_GLOB_PATH_BYTES, PATH_PAST_BOUND,
};

/// An input which the edit guard cannot safely interpret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditError(pub String);

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EditError {}

/// Target metadata only. Content never participates in enforcement-path matching.
#[derive(Debug, PartialEq, Eq)]
pub struct EditRequest {
    pub cwd: Option<PathBuf>,
    pub paths: Vec<PathBuf>,
}

/// Runtime authority supplied by the CLI adapter, never by `tool_input`.
/// `cwd`, `root`, `home`, and `git_common_dir`, when present, must be absolute.
pub struct EditContext<'a> {
    pub cwd: &'a Path,
    pub root: &'a Path,
    pub home: &'a Path,
    pub git_common_dir: Option<&'a Path>,
    pub level: PolicyLevel,
}

/// Parse explicitly supported hook forms. Unknown tools are outside this hook.
/// Grok edit forms have native captures; Codex still awaits native qualification.
///
/// # Errors
/// Returns an error for malformed or ambiguous supported edit payloads.
pub fn parse_payload(input: &str) -> Result<Option<EditRequest>, EditError> {
    let value: Value = serde_json::from_str(input).map_err(|e| EditError(e.to_string()))?;
    let object = value
        .as_object()
        .ok_or_else(|| EditError("expected hook object".into()))?;
    for field in ["toolInputTruncated", "tool_input_truncated"] {
        if object.get(field) == Some(&Value::Bool(true)) {
            return Err(EditError(format!(
                "{field} is true; cannot inspect a truncated edit input"
            )));
        }
    }
    let tool = alias(object, &["tool_name", "toolName"])?
        .and_then(Value::as_str)
        .ok_or_else(|| EditError("tool_name must be a string".into()))?;
    if !matches!(
        tool,
        "apply_patch" | "write" | "search_replace" | "Write" | "Edit"
    ) {
        return Ok(None);
    }
    let cwd = match object.get("cwd") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if !s.is_empty() => Some(PathBuf::from(s)),
        _ => {
            return Err(EditError(
                "cwd must be a nonempty string when present".into(),
            ))
        }
    };
    let input = alias(object, &["tool_input", "toolInput"])?
        .ok_or_else(|| EditError("edit call has no tool_input".into()))?;
    let paths = if tool == "apply_patch" {
        let patch = match input {
            Value::String(s) => s.as_str(),
            Value::Object(fields) => alias(fields, &["command", "patch", "input"])?
                .and_then(Value::as_str)
                .ok_or_else(|| EditError("apply_patch input must carry patch text".into()))?,
            _ => {
                return Err(EditError(
                    "apply_patch input must be patch text or an object".into(),
                ))
            }
        };
        patch_paths(patch)?
    } else {
        let fields = input
            .as_object()
            .ok_or_else(|| EditError("file edit input must be an object".into()))?;
        let path = alias(fields, &["file_path", "filePath", "path"])?
            .and_then(Value::as_str)
            .ok_or_else(|| EditError("file edit input must name its path".into()))?;
        vec![checked_path(path)?]
    };
    Ok(Some(EditRequest { cwd, paths }))
}

// Refuse conflicting aliases: choosing the first can inspect a different target
// from the one the harness executes. Equal duplicate aliases are harmless.
fn alias<'a>(
    object: &'a Map<String, Value>,
    names: &[&str],
) -> Result<Option<&'a Value>, EditError> {
    let mut selected = None;
    for name in names {
        if let Some(value) = object.get(*name) {
            if selected.is_some_and(|old| old != value) {
                return Err(EditError(format!(
                    "conflicting aliases: {}",
                    names.join(", ")
                )));
            }
            selected = Some(value);
        }
    }
    Ok(selected)
}

fn checked_path(path: &str) -> Result<PathBuf, EditError> {
    if path.is_empty() || path.contains('\0') {
        return Err(EditError(
            "edit target must be nonempty and contain no NUL".into(),
        ));
    }
    Ok(PathBuf::from(path))
}

/// Extract every native apply-patch operation target, including rename sources
/// and destinations. Hunk content is not searched for strings resembling paths.
fn patch_paths(patch: &str) -> Result<Vec<PathBuf>, EditError> {
    // Native apply_patch accepts LF and CRLF records. Match that grammar
    // explicitly so a CRLF header still names the path the tool will edit.
    let mut lines = patch.split_inclusive('\n').map(|record| {
        record
            .strip_suffix("\r\n")
            .or_else(|| record.strip_suffix('\n'))
            .unwrap_or(record)
    });
    if lines.next() != Some("*** Begin Patch") {
        return Err(EditError("patch has no Begin Patch marker".into()));
    }
    let mut paths = Vec::new();
    let mut may_move = false;
    let mut ended = false;
    for line in lines {
        if ended {
            if !line.is_empty() {
                return Err(EditError("content follows End Patch".into()));
            }
            continue;
        }
        if line == "*** End Patch" {
            ended = true;
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Update File: ") {
            paths.push(checked_path(path)?);
            may_move = true;
        } else if let Some(path) = line
            .strip_prefix("*** Add File: ")
            .or_else(|| line.strip_prefix("*** Delete File: "))
        {
            paths.push(checked_path(path)?);
            may_move = false;
        } else if let Some(path) = line.strip_prefix("*** Move to: ") {
            if !may_move {
                return Err(EditError(
                    "Move to must immediately follow Update File".into(),
                ));
            }
            paths.push(checked_path(path)?);
            may_move = false;
        } else {
            if paths.is_empty() || (line.starts_with("*** ") && line != "*** End of File") {
                return Err(EditError("unrecognized patch operation".into()));
            }
            may_move = false;
        }
    }
    if !ended || paths.is_empty() {
        return Err(EditError("patch is incomplete or names no files".into()));
    }
    Ok(paths)
}

/// Shared entry for interpreter literal references. It does not read file
/// contents. An absent home or uninspectable path is explicit uncertainty,
/// never proof that the path is ordinary. Policy handling stays with the caller.
pub(crate) fn enforcement_path(
    path: &str,
    cwd: &Path,
    root: &Path,
    home: Option<&Path>,
) -> Result<bool, String> {
    let home = home.ok_or_else(|| "cannot resolve the home for enforcement paths".to_string())?;
    let repo = super::RepoInfo::discover(root)?;
    let ctx = EditContext {
        cwd,
        root,
        home,
        git_common_dir: repo.as_ref().map(|repo| repo.common_dir.as_path()),
        level: PolicyLevel::Block,
    };
    let request = EditRequest {
        cwd: None,
        paths: vec![checked_path(path).map_err(|e| e.to_string())?],
    };
    // A shell startup file refuses under its own rule; it is not an
    // enforcement path, so the interpreter check must not call it one.
    evaluate(&request, &ctx)
        .map(|violations| {
            violations
                .iter()
                .any(|v| v.rule != crate::security::startup::RULE)
        })
        .map_err(|e| e.to_string())
}

/// The path an edit of `path` opens. On Windows a namespace or UNC spelling
/// is mapped by its prefix class before any join, to the rooted drive path
/// or complete UNC path Win32 opens ([`crate::portable_path::edit_target`]),
/// and any other namespace spelling refuses: the standard library reads
/// `\??\C:\x` as rooted, so a join would put the cwd's drive in front of
/// it (TSK-1400). The joined path is mapped once more, for a join onto a
/// namespace or UNC cwd. A drive-rooted path without a letter (`\x`) takes
/// the cwd's drive, as Win32 takes the writer's current drive.
fn absolute_target(path: &Path, cwd: &Path, home: &Path) -> Result<PathBuf, EditError> {
    let written = match path.strip_prefix("~") {
        Ok(relative) => home.join(relative),
        Err(_) => path.to_path_buf(),
    };
    let joined = match crate::portable_path::edit_target(&written).map_err(EditError)? {
        Some(mapped) => mapped,
        None if written.is_absolute() => written,
        None => cwd.join(&written),
    };
    Ok(crate::portable_path::edit_target(&joined)
        .map_err(EditError)?
        .unwrap_or(joined))
}

/// Why a UNC target outside the session's chosen directories refuses, with
/// the remedy (TSK-1400). A share can export any directory, the home
/// included, under any name, so the guard cannot map it to a local path.
const UNMAPPED_SHARE: &str = "cannot check an edit on a network share outside the home, the repository and the working directory: the share can name a protected directory under another path; use its drive-letter path instead";

/// `path` as slashed, lower-case UNC text, after its namespace or
/// administrative share is mapped; `None` when it is no UNC path or on
/// other platforms. The text is [`folded_share`]'s, read without the disk.
fn share_text(path: &Path) -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    let mapped = crate::portable_path::edit_target(path)
        .ok()
        .flatten()
        .unwrap_or_else(|| path.to_path_buf());
    folded_share(&crate::portable_path::slashed(&mapped).to_ascii_lowercase())
}

/// UNC `text` with `/` separators, its empty and `.` names dropped and
/// each `..` name taking the name before it away, never the server or the
/// share, as Win32 normalizes a UNC path. A name of dots and spaces only
/// counts as `..`, since Win32 trims trailing dots and spaces. `None` when
/// `text` is no UNC path.
fn folded_share(text: &str) -> Option<String> {
    let rest = text.strip_prefix("//")?;
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        if parts.len() < 2 {
            parts.push(part);
        } else if part.len() > 1 && part.bytes().all(|byte| matches!(byte, b'.' | b' ')) {
            if parts.len() > 2 {
                parts.pop();
            }
        } else if !part.is_empty() && part != "." {
            parts.push(part);
        }
    }
    Some(format!("//{}", parts.join("/")))
}

/// Whether `target`, UNC text from [`share_text`], lies outside every
/// directory in `chosen` (the same form). Text without a UNC prefix is not
/// a share and is never outside.
fn outside_chosen_shares(target: &str, chosen: &[String]) -> bool {
    target.starts_with("//")
        && !chosen.iter().any(|dir| {
            target
                .strip_prefix(dir.as_str())
                .is_some_and(|tail| tail.is_empty() || tail.starts_with('/'))
        })
}

/// Return a policy finding per refused target, with the existing rule and remedy.
/// Errors must remain distinct from an allowed edit at the hook adapter boundary.
///
/// # Errors
/// Returns an error when the context or a target cannot be resolved safely.
pub fn evaluate(request: &EditRequest, ctx: &EditContext<'_>) -> Result<Vec<Violation>, EditError> {
    let cwd = request.cwd.as_deref().unwrap_or(ctx.cwd);
    for path in [cwd, ctx.root, ctx.home]
        .into_iter()
        .chain(ctx.git_common_dir)
    {
        if !path.is_absolute() {
            return Err(EditError(format!(
                "edit context must be absolute: {}",
                path.display()
            )));
        }
    }
    let protected = if ctx.level.is_active() {
        enforcement_patterns(ctx)?
    } else {
        Vec::new()
    };
    let authority_dirs = if ctx.level.is_active() {
        authority_directories(ctx.home)?
    } else {
        Vec::new()
    };
    let mut seen = BTreeSet::new();
    let mut violations = Vec::new();
    let startup = crate::security::startup::StartupEnv::with_home_at(ctx.home, cwd);
    // The UNC directories the session chose: a share target outside them
    // cannot be mapped to a local path (TSK-1400).
    let chosen_shares: Vec<String> = [ctx.cwd, ctx.root, ctx.home]
        .into_iter()
        .filter_map(share_text)
        .collect();
    for path in &request.paths {
        let absolute = absolute_target(path, cwd, ctx.home)?;
        // A Windows drive-relative path (`C:name`) keeps its drive and
        // drops the cwd when joined. Windows opens it from the writer's
        // current directory on that drive, which the guard cannot read, so
        // it cannot be judged and refuses (TSK-1400). Every other target is
        // absolute here, a namespace spelling included, since
        // [`absolute_target`] maps it to a drive or UNC path or refuses.
        if !absolute.is_absolute() {
            return Err(EditError(format!(
                "cannot check the drive-relative edit path {}: Windows opens it from that drive's current directory, which the guard cannot read",
                absolute.display()
            )));
        }
        // Before any read, a short-name lookup included, so the guard never
        // contacts a host a target names before it refuses.
        if share_text(&absolute)
            .is_some_and(|text| outside_chosen_shares(&text, &chosen_shares))
        {
            return Err(EditError(UNMAPPED_SHARE.into()));
        }
        // First, so a path past the bound reads nothing on disk (TSK-1258).
        let lexical = normalized(&absolute, false)?;
        let resolved = normalized(&absolute, true)?;
        if share_text(&resolved)
            .is_some_and(|text| outside_chosen_shares(&text, &chosen_shares))
        {
            return Err(EditError(UNMAPPED_SHARE.into()));
        }
        // Shell startup files refuse whatever the integrity level (TSK-242),
        // judged on the mapped target and its normalized and resolved
        // spellings, never only on the path as written (TSK-1400).
        if let Some(file) = [&absolute, &lexical, &resolved]
            .into_iter()
            .find_map(|candidate| crate::security::startup::class_target(candidate, &startup))
        {
            violations.push(Violation::always_blocking(
                crate::security::startup::RULE,
                format!("file edit targets the shell startup file `{file}`"),
                crate::security::startup::SANCTIONED,
            ));
        }
        for candidate in [&lexical, &resolved] {
            let text = path_text(candidate)?;
            let authority = repository_authority_target(candidate, ctx.root, false);
            let enforcement = ctx.level.is_active()
                && !authority
                && repository_enforcement_target(candidate, ctx.root, false).map_err(EditError)?;
            let mut covered = in_authority_directory(candidate, &authority_dirs);
            if ctx.level.is_active() && !authority && !covered {
                for pattern in &protected {
                    if covers(&text, pattern)? {
                        covered = true;
                        break;
                    }
                }
            }
            if (authority || (ctx.level.is_active() && (covered || enforcement)))
                && seen.insert(lexical.clone())
            {
                violations.push(Violation::new(
                    "git.hook_integrity",
                    if authority {
                        PolicyLevel::Block
                    } else {
                        ctx.level
                    },
                    format!(
                        "file edit targets the enforcement path `{}`",
                        lexical.display()
                    ),
                    crate::remedy::HOOK_INTEGRITY.remedy(),
                ));
            }
        }
    }
    Ok(violations)
}

/// The operator-authority directory, `.codeflow/authority`, under the
/// guard's home and under the account's home from the password database,
/// with ASCII case folded. The SSH configuration allow (TSK-1184) is read
/// under the account's home, which can differ from `HOME`, and a volume
/// that folds case opens `.codeflow/Authority` for `.codeflow/authority`,
/// so the exact-spelling pattern under `HOME` alone does not cover it.
///
/// Each directory is spelled as [`evaluate`] spells a target, so both
/// sides of the comparison use one spelling (TSK-1400). It is joined onto
/// the home as given, onto the home resolved through [`normalized`] (or,
/// when that walk fails, through [`crate::portable_path::canonicalize`],
/// which lists no directory), and onto the canonical home whenever that
/// call succeeds; each of those directories is then kept as given, lexically
/// normalized and, when its own walk succeeds, resolved. The resolved home
/// is always kept, because a target that reaches the directory through a
/// link is judged in that spelling. A home that cannot be resolved either
/// way refuses every edit, and so does a lexical spelling that cannot be
/// made. A home that does not exist resolves to its written spelling.
fn authority_directories(home: &Path) -> Result<Vec<PathBuf>, EditError> {
    let mut homes = vec![home.to_path_buf()];
    homes.extend(crate::git::remote_identity::account_home().ok());
    let mut dirs = BTreeSet::new();
    for home in &homes {
        let canonical = crate::portable_path::canonicalize(home).ok();
        let resolved = match normalized(home, true) {
            Ok(resolved) => resolved,
            Err(error) => canonical.clone().ok_or_else(|| {
                EditError(format!(
                    "the home {} cannot be resolved, so no edit can be checked against the operator-authority directory under it and every edit is refused: {error}",
                    home.display()
                ))
            })?,
        };
        for base in [Some(home.clone()), Some(resolved), canonical]
            .into_iter()
            .flatten()
        {
            let dir = base.join(".codeflow").join("authority");
            dirs.insert(folded_path(&dir));
            dirs.insert(folded_path(&normalized(&dir, false)?));
            if let Ok(resolved) = normalized(&dir, true) {
                dirs.insert(folded_path(&resolved));
            }
        }
    }
    Ok(dirs.into_iter().collect())
}

/// `path` with ASCII letters lowercased, compared by component.
fn folded_path(path: &Path) -> PathBuf {
    PathBuf::from(path.as_os_str().to_ascii_lowercase())
}

/// Whether `candidate` is one of `dirs` or inside one, ignoring ASCII case,
/// whether or not the directory exists yet.
fn in_authority_directory(candidate: &Path, dirs: &[PathBuf]) -> bool {
    let candidate = folded_path(candidate);
    dirs.iter().any(|dir| candidate.starts_with(dir))
}

fn enforcement_patterns(ctx: &EditContext<'_>) -> Result<Vec<String>, EditError> {
    let mut result = BTreeSet::new();
    let mut reduced = BTreeSet::new();
    for rule in &actions::table().claude_edit_denies {
        let pattern = rule
            .strip_prefix("Edit(")
            .and_then(|s| s.strip_suffix(')'))
            .ok_or_else(|| EditError(format!("invalid enforcement pattern: {rule}")))?;
        let (base, relative) = if let Some(relative) = pattern.strip_prefix("~/") {
            (ctx.home, relative)
        } else if let Some(relative) = pattern.strip_prefix('/') {
            (ctx.root, relative)
        } else {
            return Err(EditError(format!(
                "enforcement pattern has no supported anchor: {rule}"
            )));
        };
        for resolve in [false, true] {
            // A linked worktree's .git is a file, not a directory. Keep its
            // lexical spelling below, and resolve real Git paths through the
            // supplied common dir instead of failing every unrelated edit.
            if resolve
                && base == ctx.root
                && relative.starts_with(".git/")
                && ctx.git_common_dir.is_some()
                && {
                    let marker = ctx.root.join(".git");
                    let unreadable =
                        |error| EditError(format!("cannot inspect Git marker: {error}"));
                    !crate::absence::proven_absent(&marker).map_err(unreadable)?
                        && std::fs::metadata(&marker).map_err(unreadable)?.is_file()
                }
            {
                continue;
            }
            // Patterns name literal enforcement directories followed by globs.
            // Resolve their literal prefix as well, so a protected directory or
            // file which is a symlink cannot be edited through its other spelling.
            match resolved_pattern(base, relative, resolve) {
                Ok(pattern) => {
                    result.insert(pattern);
                }
                Err(error) if resolve => {
                    reduced.insert(error.to_string());
                }
                Err(error) => return Err(error),
            }
        }
        if base == ctx.root {
            if let (Some(common), Some(relative)) =
                (ctx.git_common_dir, relative.strip_prefix(".git/"))
            {
                for resolve in [false, true] {
                    match resolved_pattern(common, relative, resolve) {
                        Ok(pattern) => {
                            result.insert(pattern);
                        }
                        Err(error) if resolve => {
                            reduced.insert(error.to_string());
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
        }
    }
    if !reduced.is_empty() {
        eprintln!(
            "codeflow edit-guard: reduced alias coverage; lexical enforcement remains active: {}",
            // The reasons quote checkout paths (TSK-388).
            crate::git::display_message(&reduced.into_iter().collect::<Vec<_>>().join("; "))
        );
    }
    Ok(result.into_iter().collect())
}

fn resolved_pattern(base: &Path, relative: &str, resolve: bool) -> Result<String, EditError> {
    let parts: Vec<_> = relative.split('/').collect();
    let split = parts
        .iter()
        .position(|p| p.contains(['*', '?', '[']))
        .unwrap_or(parts.len());
    let mut literal = base.to_path_buf();
    for part in &parts[..split] {
        literal.push(part);
    }
    let literal = normalized(&literal, resolve)?;
    let mut pattern = path_text(&literal)?;
    for part in &parts[split..] {
        if !pattern.ends_with('/') {
            pattern.push('/');
        }
        pattern.push_str(part);
    }
    Ok(pattern)
}

/// Whether `pattern` covers `path`. A match that cannot finish inside the
/// glob matcher's bound is an error naming the bound, so the edit is refused
/// with its cause (TSK-1258) and never allowed.
fn covers(path: &str, pattern: &str) -> Result<bool, EditError> {
    match match_extended_glob(path, pattern) {
        GlobMatch::Match => Ok(true),
        GlobMatch::Unbounded(cause) => Err(EditError(format!(
            "cannot check the edit path against the protected paths: {cause}"
        ))),
        GlobMatch::NoMatch => Ok(pattern
            .strip_suffix("/**")
            .is_some_and(|parent| path == parent)),
    }
}

// OS text rule (issue 79, `docs/architecture.md`): kept strict. The rules that
// protect a path are text globs, and this text decides whether an edit is
// refused, so a path that is not valid UTF-8 is refused instead of matched
// by a lossy spelling that might miss the protected pattern.
fn path_text(path: &Path) -> Result<String, EditError> {
    path.to_str()
        .ok_or_else(|| EditError("non-UTF-8 enforcement path".into()))?;
    Ok(crate::portable_path::slashed(path))
}

// APFS realpath retains the caller's case. Recover the directory entry's
// spelling only when its identity matches, so suffix rules stay precise.
#[cfg(target_os = "macos")]
pub(crate) fn normalize_case(
    path: &mut PathBuf,
    metadata: &std::fs::Metadata,
) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    use unicode_normalization::UnicodeNormalization as _;
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let Some(name) = path.file_name() else {
        return Ok(());
    };
    let entries = std::fs::read_dir(parent)
        .map_err(|e| format!("cannot read directory {}: {e}", parent.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read directory entry: {e}"))?;
        let entry_name = entry.file_name();
        let same_name = entry_name
            .as_encoded_bytes()
            .eq_ignore_ascii_case(name.as_encoded_bytes())
            || entry_name
                .to_str()
                .zip(name.to_str())
                .is_some_and(|(actual, requested)| {
                    actual.nfc().collect::<String>().to_lowercase()
                        == requested.nfc().collect::<String>().to_lowercase()
                });
        if same_name {
            let candidate = std::fs::symlink_metadata(entry.path())
                .map_err(|e| format!("cannot inspect {}: {e}", entry.path().display()))?;
            if candidate.dev() == metadata.dev() && candidate.ino() == metadata.ino() {
                *path = entry.path();
                return Ok(());
            }
        }
    }
    Err(format!(
        "cannot recover directory entry spelling for {}",
        path.display()
    ))
}

// Windows names one entry by its long name, an 8.3 short name (`RUNNER~1`)
// and any letter case; a harness cwd and git's paths often differ in that
// way. The canonical path names it once, in the plain drive form git writes.
#[cfg(windows)]
pub(crate) fn normalize_case(
    path: &mut PathBuf,
    _metadata: &std::fs::Metadata,
) -> Result<(), String> {
    *path = crate::portable_path::canonicalize(&*path)
        .map_err(|e| format!("cannot resolve {}: {e}", path.display()))?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", windows)))]
#[allow(clippy::unnecessary_wraps)] // The shared interface propagates platform-specific read failures.
pub(crate) fn normalize_case(
    _path: &mut PathBuf,
    _metadata: &std::fs::Metadata,
) -> Result<(), String> {
    Ok(())
}

// Resolve one component at a time. Lexically deleting `alias/..` before
// following `alias` is wrong when alias is a symlink into another directory.
// Missing suffixes are kept, so adding a new file beneath an existing symlink
// is judged against that symlink's destination. A suffix that cannot exist
// (beneath a file, or a name the platform refuses, such as Windows `*` or
// `"`) is kept the same way. Broken links and permissions are errors, not an
// implicit allow.
pub(crate) fn normalized(path: &Path, resolve: bool) -> Result<PathBuf, EditError> {
    let key = (path.to_path_buf(), resolve);
    if let Some(cached) = NORMALIZED.with(|cache| {
        cache
            .borrow()
            .as_ref()
            .and_then(|paths| paths.get(&key).cloned())
    }) {
        return cached;
    }
    let result = normalize_uncached(path, resolve);
    NORMALIZED.with(|cache| {
        if let Some(paths) = cache.borrow_mut().as_mut() {
            paths.insert(key, result.clone());
        }
    });
    result
}

fn normalize_uncached(path: &Path, resolve: bool) -> Result<PathBuf, EditError> {
    // Refused before any read, with the cause the glob matcher would give, so
    // the host's path limit cannot change the verdict or its reason (TSK-1258).
    if path.as_os_str().len() > MAX_GLOB_PATH_BYTES {
        return Err(EditError(format!(
            "cannot check the edit path against the protected paths: {PATH_PAST_BOUND}"
        )));
    }
    let path = crate::portable_path::without_verbatim(
        crate::portable_path::guard_path(path).map_err(EditError)?,
    );
    let mut result = PathBuf::new();
    // The length of `result` at the first name proven to hold nothing. Nothing
    // can exist below it, so the names under it are kept without a read, until
    // a `..` climbs back out of it. One walk per name was quadratic (TSK-1258).
    let mut absent_at = None;
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => result.push(prefix.as_os_str()),
            Component::RootDir => result.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
                if absent_at.is_some_and(|length| result.as_os_str().len() < length) {
                    absent_at = None;
                }
            }
            Component::Normal(part) => {
                let name = crate::git::GitName::from_os_str(part);
                let spelling = if cfg!(windows) {
                    Some(crate::portable_path::filesystem_spelling(
                        name.rule_text().map_err(|_| {
                            EditError("cannot normalize a Windows path component as text".into())
                        })?,
                    ))
                } else {
                    None
                };
                let part = spelling.as_deref().map_or(part, std::ffi::OsStr::new);
                // A normal component can look like a drive prefix when read
                // alone (`quoted/C:/tail` on Windows). Pushing it directly
                // would discard the preceding path. Keep it relative, even
                // when the platform cannot create that filename.
                if matches!(
                    Path::new(part).components().next(),
                    Some(Component::Prefix(_))
                ) {
                    let mut relative = std::ffi::OsString::from(".");
                    relative.push(std::path::MAIN_SEPARATOR_STR);
                    relative.push(part);
                    result.push(relative);
                } else {
                    result.push(part);
                }
                if resolve && absent_at.is_none() {
                    let (resolved, absent) = resolved_component(result)?;
                    result = resolved;
                    if absent {
                        absent_at = Some(result.as_os_str().len());
                    }
                }
            }
        }
    }
    Ok(result)
}

/// Branch alternatives commonly share all but their last component. Resolve
/// those prefixes once in the same invocation snapshot as complete operands.
/// The flag says this read proved that nothing can exist at the path. A cached
/// path gives `false`, which costs the caller one more read at most.
fn resolved_component(mut path: PathBuf) -> Result<(PathBuf, bool), EditError> {
    let key = (path.clone(), true);
    if let Some(cached) = NORMALIZED.with(|cache| {
        cache
            .borrow()
            .as_ref()
            .and_then(|paths| paths.get(&key).cloned())
    }) {
        return cached.map(|path| (path, false));
    }
    let result = (|| {
        if crate::absence::cannot_exist(&path)
            .map_err(|e| EditError(format!("cannot inspect {}: {e}", path.display())))?
        {
            return Ok((path, true));
        }
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                crate::portable_path::canonicalize(&path)
                    .map(|path| (path, false))
                    .map_err(|e| EditError(format!("cannot resolve {}: {e}", path.display())))
            }
            Ok(metadata) => {
                normalize_case(&mut path, &metadata).map_err(EditError)?;
                Ok((path, false))
            }
            Err(e) => Err(EditError(format!("cannot inspect {}: {e}", path.display()))),
        }
    })();
    NORMALIZED.with(|cache| {
        if let Some(paths) = cache.borrow_mut().as_mut() {
            paths.insert(key, result.clone().map(|(path, _)| path));
        }
    });
    result
}

pub(crate) const AUTHORITY_PATH: &str = "remote-tracking policy metadata";

/// How a shell write names a user or system git configuration file, which
/// [`repository_authority_target`] also covers (TSK-242 review round 12: the
/// shell sentence named remote-tracking metadata for it).
pub(crate) const GIT_CONFIG_AUTHORITY: &str = "a user or system git configuration file";

fn global_git_config_paths(root: &Path) -> Vec<PathBuf> {
    // Every home a shell of this user may read, as the startup class
    // reads them: `HOME`, and `USERPROFILE` where it names another
    // directory (TSK-242 review round 14).
    let homes = crate::portable_path::user_homes();
    let mut paths = Vec::new();
    for home in &homes {
        paths.push(home.join(".gitconfig"));
    }
    match std::env::var_os("XDG_CONFIG_HOME") {
        Some(xdg) => paths.push(PathBuf::from(xdg).join("git/config")),
        None => paths.extend(homes.iter().map(|home| home.join(".config/git/config"))),
    }
    // /dev/null deliberately disables global config; writes cannot change its contents.
    for name in ["GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM"] {
        if let Some(path) = std::env::var_os(name)
            .filter(|s| !s.is_empty() && s != std::ffi::OsStr::new("/dev/null"))
        {
            paths.push(root.join(path));
        }
    }
    paths
}

/// A glob fallback must not hide a configured authority file beneath its
/// enclosing directory, even when that file has an ordinary name.
pub(crate) fn holds_global_git_config(target: &Path, root: &Path) -> bool {
    let paths = global_git_config_paths(root);
    [false, true].into_iter().any(|resolve| {
        normalized(target, resolve).map_or(true, |target| {
            paths.iter().any(|path| {
                normalized(path, resolve).map_or(true, |path| path.starts_with(&target))
            })
        })
    })
}

// File edits cannot establish which config keys are safe. The Git command
// checker still permits ordinary `git config --global user.name ...` updates.
pub(crate) fn global_git_config_target(target: &Path, root: &Path) -> bool {
    let paths = global_git_config_paths(root);
    // git reads its system file under the prefix it was built with
    // (`/etc/gitconfig`, `/opt/homebrew/etc/gitconfig`, a source build's
    // `~/etc/gitconfig`), so a `gitconfig` in any `etc` directory counts
    // (TSK-242 review round eleven).
    let system_file = |path: &Path| {
        path.file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("gitconfig"))
            && path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|dir| dir.eq_ignore_ascii_case("etc"))
    };
    [false, true].into_iter().any(|resolve| {
        normalized(target, resolve).is_ok_and(|target| {
            system_file(&target)
                || paths
                    .iter()
                    .any(|path| normalized(path, resolve).is_ok_and(|path| path == target))
        })
    })
}

/// `path` is `prefix` or lies below it, each component compared without
/// regard to ASCII case. Git's administrative names are lower case, and on a
/// case-insensitive filesystem a spelling such as `.git/Refs/Remotes` that
/// does not exist yet, which [`normalize_case`] cannot correct, still
/// creates the protected directory.
fn under_ignoring_case(path: &Path, prefix: &Path) -> bool {
    let mut parts = path.components();
    prefix.components().all(|want| {
        parts.next().is_some_and(|got| {
            got.as_os_str()
                .as_encoded_bytes()
                .eq_ignore_ascii_case(want.as_os_str().as_encoded_bytes())
        })
    })
}

/// `path` lies under `dir` and a component below it holds a non-ASCII byte.
/// A filesystem such as APFS may fold it onto a protected git name
/// (`remoteſ` names the same entry as `remotes`), and a component that does
/// not exist yet cannot be checked against the disk, so the path is judged
/// protected, failing closed (TSK-381 SF-3).
fn non_ascii_below(path: &Path, dir: &Path) -> bool {
    under_ignoring_case(path, dir)
        && path
            .components()
            .skip(dir.components().count())
            .any(|part| !part.as_os_str().as_encoded_bytes().is_ascii())
}

/// `a` and `b` name one path, components compared as [`under_ignoring_case`]
/// compares them.
fn same_ignoring_case(a: &Path, b: &Path) -> bool {
    a.components().count() == b.components().count() && under_ignoring_case(a, b)
}

/// Authority metadata cannot be relaxed by a policy read from that metadata.
pub(crate) fn repository_authority_target(target: &Path, root: &Path, ancestors: bool) -> bool {
    let read = |root: &Path| {
        if global_git_config_target(target, root) {
            return true;
        }
        let Ok(repo) = git2::Repository::discover(root) else {
            return false;
        };
        let mut paths = vec![
            (PathBuf::from("refs/remotes"), true),
            (PathBuf::from("packed-refs"), false),
            (PathBuf::from("config"), false),
            (PathBuf::from("config.worktree"), false),
            // The coordination ledger (ADR-0086): CodeFlow writes it from
            // inside its own process; an agent's direct edit is refused.
            (PathBuf::from("codeflow/coord"), true),
        ];
        if let Ok(names) = repo.worktrees() {
            // OS text rule (issue 79, `docs/architecture.md`): the names are kept
            // as bytes. A worktree whose folder name is not valid UTF-8 still has
            // a `config.worktree` that authority rests on, so it must stay in the
            // protected list instead of dropping out of it.
            for name in crate::git::name::names_of(&names) {
                // A folder the platform cannot hold as a path cannot be listed, so
                // the target is judged protected rather than left out.
                let Ok(folder) = name.os_path() else {
                    return true;
                };
                paths.push((
                    Path::new("worktrees").join(folder).join("config.worktree"),
                    false,
                ));
            }
        }
        for resolve in [false, true] {
            let (Ok(target), Ok(common)) = (
                normalized(target, resolve),
                normalized(repo.commondir(), resolve),
            ) else {
                continue;
            };
            if !under_ignoring_case(&target, &common) {
                continue;
            }
            if non_ascii_below(&target, &common) {
                return true;
            }
            for (name, directory) in &paths {
                let Ok(protected) = normalized(&common.join(name), resolve) else {
                    continue;
                };
                if same_ignoring_case(&target, &protected)
                    || (*directory && under_ignoring_case(&target, &protected))
                    || (ancestors && under_ignoring_case(&protected, &target))
                {
                    return true;
                }
            }
        }
        false
    };
    if read(root) {
        return true;
    }
    super::repo::target_roots(target, root).is_ok_and(|roots| roots.iter().any(|owner| read(owner)))
}

/// Inventory includes outside the guard's fixed protected-path class.
/// Doctor reports these potentially writable files and failed reads; ordinary
/// path checks do not launch a configuration probe per operand.
pub(crate) fn included_git_config_files(
    root: &Path,
) -> Result<Vec<crate::security::git_config::Include>, String> {
    crate::security::git_config::read(root, &[], &crate::security::git_config::Environment::new())
        .map(|config| config.includes)
}

/// Every checkout sharing this repository: its own working tree, the main
/// working tree and each registered linked worktree.
pub(crate) fn checkout_roots(repo: &git2::Repository) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    if let Some(workdir) = repo.workdir() {
        roots.push(workdir.to_path_buf());
    }
    let main = git2::Repository::open(repo.commondir()).map_err(|e| {
        format!(
            "cannot open main checkout {}: {e}",
            repo.commondir().display()
        )
    })?;
    if let Some(workdir) = main.workdir() {
        roots.push(workdir.to_path_buf());
    }
    roots.extend(
        super::repo::registrations(repo)?
            .into_iter()
            .map(|admin| super::repo::checked_registration(&admin))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten(),
    );
    Ok(roots)
}

#[cfg(test)]
thread_local! {
    static PROTECTED_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The enforcement paths of every checkout sharing this repository, each
/// with whether it protects a whole directory and the base its ancestors
/// must lie in: the first component under its own checkout (`.claude`,
/// `.codeflow`), or the common git directory. A checkout nested in another
/// checkout's `.claude` (`.claude/worktrees/<name>`) is therefore not an
/// ancestor of its own files by lying inside the outer `.claude`.
fn protected_paths(repo: &git2::Repository) -> Result<Vec<(PathBuf, bool, PathBuf)>, String> {
    #[cfg(test)]
    PROTECTED_BUILDS.with(|count| count.set(count.get() + 1));
    // Reuse the action table's repository paths for every checkout; home
    // paths remain scoped to the caller's home in enforcement_patterns.
    let patterns: Vec<_> = actions::table()
        .claude_edit_denies
        .iter()
        .filter_map(|rule| {
            rule.strip_prefix("Edit(/")
                .and_then(|s| s.strip_suffix(')'))
        })
        .map(|relative| {
            relative
                .strip_suffix("/**")
                .map_or((relative, false), |prefix| (prefix, true))
        })
        .collect();
    let mut protected = Vec::new();
    // Protect the whole administrative namespace, including names Git does
    // not list yet. Otherwise three individually allowed creates can plant
    // a registration that locks every checkout on the next invocation.
    protected.push((
        repo.commondir().join("worktrees"),
        true,
        repo.commondir().to_path_buf(),
    ));
    // The namespace itself contains submodule enforcement state. Individual
    // descendants keep the Git-directory owner's precise protection set.
    protected.push((
        repo.commondir().join("modules"),
        false,
        repo.commondir().to_path_buf(),
    ));
    for admin in super::repo::registrations(repo)? {
        let registration = registration_at(&admin)?;
        if registration.checkout.is_none() {
            let marker = registration.marker;
            let root = marker
                .parent()
                .ok_or("registration has no checkout parent")?
                .to_path_buf();
            protected.push((root.clone(), true, root));
        }
    }
    for root in checkout_roots(repo)? {
        // A linked checkout's pointer is an enforcement file in its own
        // right. Resolve this path like every other protected path, so an
        // alias outside the repository cannot hide the checkout spelling.
        // Its ancestor base is itself: an ordinary checkout root is not
        // an enforcement directory merely because it contains this file.
        let marker = root.join(".git");
        protected.push((marker.clone(), false, marker));
        for (relative, directory) in &patterns {
            // A linked checkout's .git is a pointer file. Git-owned paths
            // below it live in the shared administrative directory instead.
            if relative.starts_with(".git/") {
                continue;
            }
            let base = Path::new(relative)
                .components()
                .next()
                .map_or_else(|| root.clone(), |base| root.join(base));
            protected.push((root.join(relative), *directory, base));
        }
    }
    for (relative, directory) in &patterns {
        if let Some(name) = relative.strip_prefix(".git/") {
            protected.push((
                repo.commondir().join(name),
                *directory,
                repo.commondir().to_path_buf(),
            ));
        }
    }
    for (name, directory) in [
        ("HEAD", false),
        ("hooks", true),
        ("refs/remotes", true),
        // A direct write of a loose branch ref moves it without a ref
        // transaction, so no hook sees it (TSK-381 review).
        ("refs/heads", true),
        ("packed-refs", false),
        ("config", false),
        ("config.worktree", false),
        // An alternate object store adds objects Git reads as its own, so a
        // written list can make any content reachable (TSK-1002 Fable 13).
        ("objects/info/alternates", false),
        ("objects/info/http-alternates", false),
    ] {
        protected.push((
            repo.commondir().join(name),
            directory,
            repo.commondir().to_path_buf(),
        ));
    }
    Ok(protected)
}

/// One enforcement path, resolved both ways [`normalized`] reads paths
/// (lexically, and through symbolic links): the path, whether it protects
/// a whole directory, and the base its ancestors must lie in, resolved
/// when first needed. `None` where that reading fails.
struct ProtectedPath {
    path: [Result<PathBuf, String>; 2],
    directory: bool,
    /// Under the git administrative directory, so compared without ASCII
    /// case ([`under_ignoring_case`]).
    git_owned: bool,
    base: PathBuf,
    resolved_base: [std::cell::OnceCell<Result<PathBuf, String>>; 2],
}

type Protected = std::rc::Rc<Vec<ProtectedPath>>;

type ProtectedCache = std::collections::HashMap<PathBuf, Result<Option<Protected>, String>>;

struct CheckoutPaths {
    own: Option<[PathBuf; 2]>,
    roots: Vec<(PathBuf, [PathBuf; 2])>,
}

type CheckoutCache =
    std::collections::HashMap<PathBuf, Result<Option<std::rc::Rc<CheckoutPaths>>, String>>;

type RegistrationCache = std::collections::HashMap<
    PathBuf,
    std::collections::HashMap<PathBuf, Result<super::repo::CheckedRegistration, String>>,
>;

/// Registration facts share the common directory across main and linked
/// checkout readers. Errors are memoized too; no reader can bypass a conflict.
pub(super) fn registration_at(admin: &Path) -> Result<super::repo::CheckedRegistration, String> {
    let admin = normalized(admin, false).map_err(|error| error.to_string())?;
    let common = admin
        .parent()
        .and_then(Path::parent)
        .ok_or("worktree registration has no common directory")?
        .to_path_buf();
    REGISTRATIONS.with(|cache| match cache.borrow_mut().as_mut() {
        Some(map) => map
            .entry(common)
            .or_default()
            .entry(admin.clone())
            .or_insert_with(|| super::repo::read_registration(&admin))
            .clone(),
        None => super::repo::read_registration(&admin),
    })
}

type NormalizedCache = std::collections::HashMap<(PathBuf, bool), Result<PathBuf, EditError>>;

thread_local! {
    static NORMALIZED: std::cell::RefCell<Option<NormalizedCache>> =
        const { std::cell::RefCell::new(None) };
    static REGISTRATIONS: std::cell::RefCell<Option<RegistrationCache>> =
        const { std::cell::RefCell::new(None) };
    static PROTECTED: std::cell::RefCell<Option<ProtectedCache>> =
        const { std::cell::RefCell::new(None) };
    static CHECKOUTS: std::cell::RefCell<Option<CheckoutCache>> =
        const { std::cell::RefCell::new(None) };
}

/// While alive, each repository's enforcement paths are read and resolved
/// once on this thread. Normalized operands share that snapshot too, including
/// errors; dropping the outer scope discards it before the next command.
/// One guard evaluation judges many words against the
/// same repository, which the command being judged has not changed yet
/// (TSK-216 round 16).
pub struct RepoFactsScope {
    outer: bool,
}

impl RepoFactsScope {
    /// Start an invocation snapshot, retaining an outer scope when nested.
    #[must_use]
    pub fn enter() -> Self {
        let outer = PROTECTED.with(|cache| {
            let mut cache = cache.borrow_mut();
            let outer = cache.is_none();
            if outer {
                *cache = Some(std::collections::HashMap::new());
            }
            outer
        });
        if outer {
            NORMALIZED.with(|cache| *cache.borrow_mut() = Some(std::collections::HashMap::new()));
            super::repo::TARGET_ROOTS
                .with(|cache| *cache.borrow_mut() = Some(std::collections::HashMap::new()));
            super::repo::OWNERS
                .with(|cache| *cache.borrow_mut() = Some(std::collections::HashMap::new()));
            super::git_guard::LINE_READS
                .with(|cache| *cache.borrow_mut() = Some(super::git_guard::LineReads::default()));
            REGISTRATIONS
                .with(|cache| *cache.borrow_mut() = Some(std::collections::HashMap::new()));
            CHECKOUTS.with(|cache| *cache.borrow_mut() = Some(std::collections::HashMap::new()));
        }
        Self { outer }
    }
}

impl Drop for RepoFactsScope {
    fn drop(&mut self) {
        if self.outer {
            NORMALIZED.with(|cache| *cache.borrow_mut() = None);
            super::repo::TARGET_ROOTS.with(|cache| *cache.borrow_mut() = None);
            super::repo::OWNERS.with(|cache| *cache.borrow_mut() = None);
            super::git_guard::LINE_READS.with(|cache| *cache.borrow_mut() = None);
            REGISTRATIONS.with(|cache| *cache.borrow_mut() = None);
            PROTECTED.with(|cache| *cache.borrow_mut() = None);
            CHECKOUTS.with(|cache| *cache.borrow_mut() = None);
        }
    }
}

fn checkout_paths_at(root: &Path) -> Result<Option<std::rc::Rc<CheckoutPaths>>, String> {
    let read = || {
        let Some(repo) = super::repo::open(root)? else {
            return Ok(None);
        };
        let both = |path: &Path| -> Result<[PathBuf; 2], String> {
            Ok([
                normalized(path, false).map_err(|error| error.to_string())?,
                normalized(path, true).map_err(|error| error.to_string())?,
            ])
        };
        let own = repo.workdir().map(both).transpose()?;
        let roots = checkout_roots(&repo)?
            .into_iter()
            .map(|root| both(&root).map(|paths| (root, paths)))
            .collect::<Result<_, _>>()?;
        Ok(Some(std::rc::Rc::new(CheckoutPaths { own, roots })))
    };
    CHECKOUTS.with(|cache| match cache.borrow_mut().as_mut() {
        Some(map) => map.entry(root.to_path_buf()).or_insert_with(read).clone(),
        None => read(),
    })
}

/// An argv operand can name its current checkout as a project location.
/// Exact dot and dot-slash code literals also name a project location.
/// A code string can name the filesystem root as a fragment, such as
/// "/." in a runtime concatenation. Other ancestors still reach registered
/// checkouts. Neither exemption applies to enforcement files.
pub(crate) fn is_own_checkout_location(
    target: &Path,
    root: &Path,
    code_literal: Option<&str>,
) -> Result<bool, String> {
    let Some(checkouts) = checkout_paths_at(root)? else {
        return Ok(false);
    };
    let Some(own) = &checkouts.own else {
        return Ok(false);
    };
    let target = normalized(target, true).map_err(|error| error.to_string())?;
    Ok(
        ((code_literal.is_none() || matches!(code_literal, Some("." | "./"))) && target == own[1])
            || (code_literal.is_some() && target.has_root() && target.parent().is_none()),
    )
}

/// The enforcement paths of the repository at `root` ([`protected_paths`]),
/// resolved once per [`RepoFactsScope`] and otherwise on each call. `None`
/// outside a repository.
fn protected_at(root: &Path) -> Result<Option<Protected>, String> {
    let cached = |key: &Path| {
        PROTECTED.with(|cache| {
            cache
                .borrow()
                .as_ref()
                .and_then(|map| map.get(key))
                .cloned()
        })
    };
    if let Some(value) = cached(root) {
        return value;
    }
    let read = || {
        let Some(repo) = super::repo::open(root)? else {
            return Ok(None);
        };
        // Main and linked roots protect the same complete set. Validate the
        // caller's own discovery above, then reuse that set by common directory.
        let common = repo.commondir().to_path_buf();
        if let Some(value) = cached(&common) {
            return value;
        }
        let both = |path: &Path| {
            [false, true]
                .map(|resolve| normalized(path, resolve).map_err(|error| error.to_string()))
        };
        let resolved = protected_paths(&repo)?
            .iter()
            .map(|(path, directory, base)| ProtectedPath {
                path: both(path),
                directory: *directory,
                git_owned: path.starts_with(repo.commondir()),
                base: base.clone(),
                resolved_base: Default::default(),
            })
            .collect();
        let value = Ok(Some(std::rc::Rc::new(resolved)));
        PROTECTED.with(|cache| {
            if let Some(map) = cache.borrow_mut().as_mut() {
                map.insert(common, value.clone());
            }
        });
        value
    };
    let value = read();
    PROTECTED.with(|cache| {
        if let Some(map) = cache.borrow_mut().as_mut() {
            map.insert(root.to_path_buf(), value.clone());
        }
    });
    value
}

/// Resolve enforcement paths in every checkout sharing this repository.
/// Native edits protect files; shell writes also protect their ancestors.
pub(crate) fn repository_enforcement_target(
    target: &Path,
    root: &Path,
    ancestors: bool,
) -> Result<bool, String> {
    // An ancestor marker can change discovery for the entire session.
    for resolve in [false, true] {
        let target = normalized(target, resolve).map_err(|error| error.to_string())?;
        let session = normalized(root, resolve).map_err(|error| error.to_string())?;
        for ancestor in session.ancestors() {
            let marker = ancestor.join(".git");
            if target == marker
                || (target.starts_with(&marker)
                    && crate::absence::cannot_exist(&marker).map_err(|error| error.to_string())?)
            {
                return Ok(true);
            }
        }
    }
    let read = |root: &Path| -> Result<bool, String> {
        let Some(protected) = protected_at(root)? else {
            return Ok(false);
        };
        for (reading, resolve) in [false, true].into_iter().enumerate() {
            let target = normalized(target, resolve).map_err(|error| error.to_string())?;
            for entry in protected.iter() {
                let path = entry.path[reading].as_ref().map_err(Clone::clone)?;
                let under = |inner: &Path, outer: &Path| {
                    if entry.git_owned {
                        under_ignoring_case(inner, outer)
                    } else {
                        inner.starts_with(outer)
                    }
                };
                let same = if entry.git_owned {
                    same_ignoring_case(&target, path)
                } else {
                    target == *path
                };
                let base = || {
                    entry.resolved_base[reading]
                        .get_or_init(|| {
                            normalized(&entry.base, resolve).map_err(|error| error.to_string())
                        })
                        .as_ref()
                        .map_err(Clone::clone)
                };
                if entry.git_owned && non_ascii_below(&target, base()?) {
                    return Ok(true);
                }
                if same
                    || (entry.directory && under(&target, path))
                    || (ancestors && under(path, &target) && under(&target, base()?))
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    };

    for owner in super::repo::target_roots(target, root)? {
        let found = read(&owner)?;
        if found {
            return Ok(true);
        }
    }
    Ok(false)
}

/// How many entries a protected directory is walked for before the walk
/// gives up and stands for the whole directory.
const CANDIDATE_WALK_LIMIT: usize = 4096;

/// The paths a `find` starting at `start` may hand to its action that hold
/// enforcement state or a registered checkout, resolved through symlinks:
/// each enforcement path and registered checkout root under `start`, every
/// directory between `start` and them (`start` included), `start` itself
/// when it lies inside a protected path, and the files inside a protected
/// directory. A protected directory too large to walk is represented by
/// `<dir>/*`, which every check reads as inside it.
pub(crate) fn find_candidates(start: &Path, root: &Path) -> Result<Vec<PathBuf>, String> {
    let read = |root: &Path| -> Result<Vec<PathBuf>, String> {
        let Some(protected) = protected_at(root)? else {
            return Ok(Vec::new());
        };
        let start = normalized(start, true).map_err(|error| error.to_string())?;
        let mut out: Vec<PathBuf> = Vec::new();
        let mut push = |path: PathBuf| {
            if !out.contains(&path) {
                out.push(path);
            }
        };
        let mut held: Vec<(PathBuf, bool)> = protected
            .iter()
            .map(|entry| entry.path[1].clone().map(|path| (path, entry.directory)))
            .collect::<Result<_, _>>()?;
        if let Some(hidden) = super::repo::hidden_checkout_under(&start, root)? {
            held.push((hidden, false));
        }
        if let Some(checkouts) = checkout_paths_at(root)? {
            held.extend(
                checkouts
                    .roots
                    .iter()
                    .map(|(_, paths)| (paths[1].clone(), false)),
            );
        }
        for (path, directory) in held {
            if start.starts_with(&path) && (directory || start == path) {
                push(start.clone());
                continue;
            }
            if !path.starts_with(&start) {
                continue;
            }
            for ancestor in path.ancestors() {
                push(ancestor.to_path_buf());
                if ancestor == start {
                    break;
                }
            }
            if directory {
                let mut stack = vec![path.clone()];
                let mut seen = 0;
                while let Some(dir) = stack.pop() {
                    if crate::absence::proven_absent(&dir).map_err(|error| {
                        format!("cannot read protected directory {}: {error}", dir.display())
                    })? {
                        continue;
                    }
                    let entries = match std::fs::read_dir(&dir) {
                        Ok(entries) => entries,
                        Err(error) => {
                            return Err(format!(
                                "cannot read protected directory {}: {error}",
                                dir.display()
                            ))
                        }
                    };
                    for entry in entries {
                        let entry = entry.map_err(|error| {
                            format!("cannot read protected directory entry: {error}")
                        })?;
                        seen += 1;
                        if seen > CANDIDATE_WALK_LIMIT {
                            push(path.join("*"));
                            stack.clear();
                            break;
                        }
                        let entry_path = entry.path();
                        if entry
                            .file_type()
                            .map_err(|error| format!("cannot read protected file type: {error}"))?
                            .is_dir()
                        {
                            stack.push(entry_path.clone());
                        }
                        push(entry_path);
                    }
                }
            }
        }
        Ok(out)
    };

    let mut found_all = Vec::new();
    for owner in super::repo::target_roots(start, root)? {
        let found = read(&owner)?;
        found_all.extend(found);
    }
    Ok(found_all)
}

pub(crate) enum Checkout {
    Registered(PathBuf),
    Hidden(PathBuf),
}

/// The registered checkout of the repository at `root` that a recursive
/// delete of `target` would remove: one equal to `target` or inside it,
/// the main working tree included. Read lexically and with symlinks
/// resolved, so `..` and a symlinked spelling name the same checkout.
pub(crate) fn registered_checkout_under(
    target: &Path,
    root: &Path,
    except: Option<&Path>,
) -> Result<Option<Checkout>, String> {
    let read = |root: &Path| -> Result<Option<Checkout>, String> {
        let Some(checkouts) = checkout_paths_at(root)? else {
            return Ok(None);
        };
        let except: Vec<PathBuf> = except
            .into_iter()
            .flat_map(|path| [normalized(path, false), normalized(path, true)])
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        for (reading, resolve) in [false, true].into_iter().enumerate() {
            let target = normalized(target, resolve).map_err(|e| e.to_string())?;
            for (checkout, paths) in &checkouts.roots {
                let path = &paths[reading];
                if path.starts_with(&target) && !except.contains(path) {
                    return Ok(Some(Checkout::Registered(checkout.clone())));
                }
            }
        }
        Ok(None)
    };

    for owner in super::repo::target_roots(target, root)? {
        let found = read(&owner)?;
        if found.is_some() {
            return Ok(found);
        }
        if let Some(hidden) = super::repo::hidden_checkout_under(target, &owner)? {
            if except != Some(hidden.as_path()) {
                return Ok(Some(Checkout::Hidden(hidden)));
            }
        }
    }
    Ok(None)
}

/// Whether `target` holds an enforcement path of any checkout sharing the
/// repository at `root`, read lexically and with symlinks resolved. A
/// recursive change of `target` reaches that path whatever checkout base
/// it lies in, so a linked checkout's root holds its own files (TSK-216
/// round 3).
pub(crate) fn holds_enforcement_files(target: &Path, root: &Path) -> Result<bool, String> {
    let read = |root: &Path| -> Result<bool, String> {
        let Some(protected) = protected_at(root)? else {
            return Ok(false);
        };
        for (reading, resolve) in [false, true].into_iter().enumerate() {
            let target = normalized(target, resolve).map_err(|e| e.to_string())?;
            for entry in protected.iter() {
                if entry.path[reading]
                    .as_ref()
                    .map_err(Clone::clone)?
                    .starts_with(&target)
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    };

    for owner in super::repo::target_roots(target, root)? {
        let found = read(&owner)?;
        if found {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The working tree of the checkout holding `dir`, when it is one.
pub(crate) fn checkout_root_of(dir: &Path) -> Option<PathBuf> {
    super::repo::open(dir)
        .ok()??
        .workdir()
        .map(Path::to_path_buf)
}

/// Whether the checkout holding `dir` has another registered checkout
/// inside its working tree, as a main checkout with `.worktrees/<name>`
/// does. A command whose target cannot be resolved there may delete one.
pub(crate) fn holds_registered_worktrees(dir: &Path) -> Result<bool, String> {
    let Some(repo) = super::repo::open(dir)? else {
        return Ok(false);
    };
    let Some(workdir) = repo.workdir() else {
        return Ok(false);
    };
    registered_checkout_under(workdir, dir, Some(workdir)).map(|held| held.is_some())
}

#[cfg(test)]
mod tests {
    #[test]
    fn protected_paths_are_built_once_per_guard_invocation() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        for _ in 0..2 {
            let _scope = RepoFactsScope::enter();
            PROTECTED_BUILDS.with(|count| count.set(0));
            for name in ["build", "out"] {
                let target = dir.path().join(name);
                assert!(!repository_enforcement_target(&target, dir.path(), true).unwrap());
                assert!(!holds_enforcement_files(&target, dir.path()).unwrap());
                assert!(find_candidates(&target, dir.path()).unwrap().is_empty());
            }
            assert_eq!(PROTECTED_BUILDS.with(std::cell::Cell::get), 1);
        }
    }

    #[test]
    fn r17_repository_readers_distinguish_absence_from_broken_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(protected_at(root).unwrap().is_none());
        assert!(find_candidates(root, root).unwrap().is_empty());
        assert!(registered_checkout_under(root, root, None)
            .unwrap()
            .is_none());
        assert!(!holds_enforcement_files(root, root).unwrap());
        assert!(!holds_registered_worktrees(root).unwrap());
        git2::Repository::init(root).unwrap();
        std::fs::remove_file(root.join(".git/HEAD")).unwrap();
        assert!(protected_at(root).is_err());
        assert!(find_candidates(root, root).is_err());
        assert!(registered_checkout_under(root, root, None).is_err());
        assert!(holds_enforcement_files(root, root).is_err());
        assert!(holds_registered_worktrees(root).is_err());
    }

    #[test]
    fn r16_linked_git_pointer_preserves_ordinary_and_protected_targets() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let linked = dir.path().join("linked");
        let admin = repo.path().join("worktrees/linked");
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::write(
            admin.join("gitdir"),
            format!("{}\n", linked.join(".git").display()),
        )
        .unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::write(admin.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(
            linked.join(".git"),
            format!("gitdir: {}\n", admin.display()),
        )
        .unwrap();
        assert!(!repository_enforcement_target(&linked.join("out.txt"), dir.path(), true).unwrap());
        assert!(repository_enforcement_target(&repo.path().join("config"), &linked, true).unwrap());
    }

    #[test]
    fn r15_patch_paths_match_native_crlf_framing() {
        let paths =
            patch_paths("*** Begin Patch\n*** Add File: notes.md\r\n+body\n*** End Patch\n")
                .unwrap();
        assert_eq!(paths, [PathBuf::from("notes.md")]);
    }

    use super::*;
    use serde_json::json;

    fn synthetic(tool: &str, input: Value) -> EditRequest {
        let mut payload = json!({"tool_name": tool});
        payload["tool_input"] = input;
        parse_payload(&payload.to_string()).unwrap().unwrap()
    }

    struct Fixture {
        temp: tempfile::TempDir,
        root: PathBuf,
        home: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("repo");
            let home = temp.path().join("home");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir_all(&home).unwrap();
            Self { temp, root, home }
        }
        fn ctx(&self) -> EditContext<'_> {
            EditContext {
                cwd: &self.root,
                root: &self.root,
                home: &self.home,
                git_common_dir: None,
                level: PolicyLevel::Block,
            }
        }
        fn refused(&self, path: &str) -> bool {
            !evaluate(&synthetic("write", json!({"path": path})), &self.ctx())
                .unwrap()
                .is_empty()
        }
    }

    #[test]
    fn system_git_config_files_are_refused_under_any_prefix() {
        // TSK-242 review round eleven: git reads its system file under the
        // prefix it was built with, so a `gitconfig` in any `etc` directory
        // is refused with `/etc/gitconfig`. Nothing is written.
        let fixture = Fixture::new();
        let prefix = fixture.temp.path().join("homebrew/etc");
        std::fs::create_dir_all(&prefix).unwrap();
        for path in [
            "/etc/gitconfig".to_string(),
            "/opt/homebrew/etc/gitconfig".to_string(),
            "/usr/local/etc/gitconfig".to_string(),
            prefix.join("gitconfig").display().to_string(),
        ] {
            assert!(fixture.refused(&path), "{path}");
        }
        for path in ["etc/gitconfig.example", "docs/gitconfig", "notes.md"] {
            assert!(!fixture.refused(path), "{path}");
        }
    }

    /// TSK-1258: a path the glob matcher cannot judge inside its bound is
    /// refused with the bound named, not reported as an enforcement path and
    /// never allowed, and before any read of the target, so a host's path
    /// limit cannot stall it or change its cause. The short path shows that
    /// the count sees an inspection. Many short components keep each one a
    /// valid name.
    #[test]
    fn a_path_past_the_glob_bound_is_refused_with_its_cause() {
        const TARGET: &str = "overlength-target";
        let fixture = Fixture::new();
        let judge = |path: String| {
            crate::absence::reads_naming(TARGET, || {
                evaluate(&synthetic("write", json!({"path": path})), &fixture.ctx())
            })
        };
        let (short, inspected) = judge(format!("{TARGET}/a/a/"));
        assert!(short.is_ok_and(|violations| violations.is_empty()));
        assert!(inspected > 0, "the count sees the short target's reads");
        let (long, inspected) = judge(format!("{TARGET}/{}", "a/".repeat(20_000)));
        let error = long.expect_err("a path past the bound must be refused");
        assert!(
            error
                .to_string()
                .contains("longer than the glob matcher's bound"),
            "{error}"
        );
        assert_eq!(inspected, 0, "the long target is refused unread");
        assert!(!fixture.refused("notes.md"), "a normal path is unchanged");
    }

    /// TSK-1258: the reads one judgment spends do not grow with how deep a
    /// path runs below its first missing name. One read per ancestor for
    /// each name is quadratic, and it held a Windows test past 1200 seconds:
    /// Windows reads a path of about 32 KiB where Unix refuses it near 1 or 4 KiB,
    /// so this counts reads at a depth every host can reach.
    #[test]
    fn a_missing_path_costs_the_same_reads_at_any_depth() {
        let reads_at = |depth: usize| {
            let fixture = Fixture::new();
            let path = format!("notes/{}", "a/".repeat(depth));
            let before = crate::absence::reads();
            let violations = evaluate(&synthetic("write", json!({"path": path})), &fixture.ctx())
                .expect("an ordinary missing path is judged");
            assert!(violations.is_empty());
            crate::absence::reads() - before
        };
        let (shallow, deep) = (reads_at(100), reads_at(400));
        assert!(deep < shallow * 2, "{shallow} reads at 100, {deep} at 400");
    }

    /// TSK-1258: a path as deep as the glob bound admits is judged on every
    /// host, and a protected path keeps its refusal by rule at that depth. An
    /// inspection error fails this test instead of reading as a refusal. On
    /// Windows this path reaches the filesystem, so a walk that is not linear
    /// stalls this test, and a bound past the Windows limit fails it.
    #[test]
    fn a_path_at_the_glob_bound_is_judged() {
        let fixture = Fixture::new();
        let root = normalized(&fixture.root, true).unwrap();
        let space = crate::security::pattern::MAX_GLOB_PATH_BYTES
            - root.as_os_str().len()
            - "/notes/".len();
        let deep = "a/".repeat(space / 2);
        assert!(!fixture.refused(&format!("notes/{deep}")));
        let protected = format!(".git/hooks/{}", &deep[..deep.len() - 8]);
        let violations = evaluate(
            &synthetic("write", json!({"path": protected})),
            &fixture.ctx(),
        )
        .expect("a protected path inside the bound is judged");
        assert!(
            violations
                .iter()
                .any(|violation| violation.rule == "git.hook_integrity"),
            "{violations:?}"
        );
    }

    /// TSK-1258: a `..` that leaves a missing name returns to a path that
    /// exists, so the links beyond it are still followed.
    #[cfg(unix)]
    #[test]
    fn a_link_past_a_missing_name_is_still_resolved() {
        let fixture = Fixture::new();
        let root = normalized(&fixture.root, true).unwrap();
        let real = root.join("real");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, root.join("alias")).unwrap();
        assert_eq!(
            normalized(&root.join("missing/deeper/../../alias/file"), true).unwrap(),
            real.join("file")
        );
        assert_eq!(
            normalized(&root.join("missing/deeper/../alias/file"), true).unwrap(),
            root.join("missing/alias/file")
        );
    }

    /// TSK-1258 AC-3, reach of the cache-full condition: a guard call is one
    /// process whose glob cache starts empty, and the only patterns it
    /// compiles are the policy's own (the secret-read rules and the edit
    /// enforcement patterns, with a Git common directory, the largest case).
    /// While their distinct count stays under the cache size no guard call
    /// can fill the cache, so the stall of AC-1 needs a longer-lived process.
    #[test]
    fn one_guard_call_compiles_fewer_globs_than_the_cache_holds() {
        let fixture = Fixture::new();
        let common = fixture.temp.path().join("common");
        std::fs::create_dir_all(&common).unwrap();
        let ctx = EditContext {
            git_common_dir: Some(&common),
            ..fixture.ctx()
        };
        let enforcement = enforcement_patterns(&ctx).unwrap();
        let reads: BTreeSet<&str> = actions::table()
            .claude_read_groups
            .iter()
            .filter(|group| group.carveouts.is_empty())
            .flat_map(|group| group.denies.iter())
            .filter_map(|rule| rule.strip_prefix("Read(")?.strip_suffix(')'))
            .collect();
        eprintln!(
            "glob working set of one guard call: {} read patterns, {} edit patterns, cache holds {}",
            reads.len(),
            enforcement.len(),
            crate::security::pattern::GLOB_CACHE_ENTRIES
        );
        assert!(
            reads.len() + enforcement.len() < crate::security::pattern::GLOB_CACHE_ENTRIES,
            "{} read and {} edit patterns no longer fit the glob cache",
            reads.len(),
            enforcement.len()
        );
    }

    #[test]
    fn f9_truncated_inputs_are_refused() {
        for field in ["toolInputTruncated", "tool_input_truncated"] {
            let mut payload = json!({"toolName":"write","toolInput":{"file_path":"notes.md"}});
            payload[field] = json!(true);
            assert!(parse_payload(&payload.to_string())
                .unwrap_err()
                .to_string()
                .contains(field));
            payload[field] = json!(false);
            assert!(parse_payload(&payload.to_string()).unwrap().is_some());
        }
    }

    #[test]
    fn f10_native_fixtures_keep_duplicate_aliases() {
        for data in [
            include_str!("../../tests/fixtures/edit-hooks/grok-write.json"),
            include_str!("../../tests/fixtures/edit-hooks/grok-search-replace.json"),
        ] {
            let mut payload: Value = serde_json::from_str(data).unwrap();
            assert_eq!(
                payload["_fixture"]["provenance"],
                "native capture, grok 1.0.44"
            );
            assert_eq!(payload["toolInput"], payload["tool_input"]);
            assert!(parse_payload(&payload.to_string()).unwrap().is_some());
            payload["tool_input"]["file_path"] = json!("different.md");
            assert!(parse_payload(&payload.to_string()).is_err());
        }
    }

    /// A startup file refuses under the startup rule; the interpreter check
    /// that asks whether a word is an enforcement path must not count it
    /// (it would add a `git.hook_integrity` finding to the startup one).
    #[test]
    fn startup_files_are_not_enforcement_paths() {
        let f = Fixture::new();
        for startup in ["~/.zshrc", ".envrc", "sub/.envrc", "/etc/profile"] {
            assert!(
                !enforcement_path(startup, &f.root, &f.root, Some(&f.home)).unwrap(),
                "{startup}"
            );
        }
        assert!(enforcement_path("~/.codex/config.toml", &f.root, &f.root, Some(&f.home)).unwrap());
    }

    /// TSK-1184 delta 5: the operator-authority directory, where the SSH
    /// configuration allow lives, is covered in any ASCII case while it does
    /// not exist yet, and under the account's home from the password
    /// database as well as the guard's home.
    #[test]
    fn the_authority_directory_is_covered_in_any_case_and_under_the_account_home() {
        let f = Fixture::new();
        for path in [
            "~/.codeflow/authority/config.toml",
            "~/.codeflow/Authority/config.toml",
            "~/.CODEFLOW/AUTHORITY",
            "~/.codeflow/authority/new/file",
        ] {
            assert!(
                enforcement_path(path, &f.root, &f.root, Some(&f.home)).unwrap(),
                "{path}"
            );
        }
        for path in ["~/.codeflow/config.toml", "~/.codeflow/authority-notes"] {
            assert!(
                !enforcement_path(path, &f.root, &f.root, Some(&f.home)).unwrap(),
                "{path}"
            );
        }
        #[cfg(unix)]
        {
            let account = crate::git::remote_identity::account_home().unwrap();
            assert_ne!(account, f.home);
            for name in [
                ".codeflow/authority/config.toml",
                ".codeflow/Authority/config.toml",
            ] {
                let path = account.join(name);
                assert!(
                    enforcement_path(path.to_str().unwrap(), &f.root, &f.root, Some(&f.home))
                        .unwrap(),
                    "{}",
                    path.display()
                );
            }
        }
    }

    /// The guard's verdict on a write to `target` with `home` as its home.
    fn verdict_under(f: &Fixture, home: &Path, target: &Path) -> Result<Vec<Violation>, EditError> {
        let ctx = EditContext {
            cwd: &f.root,
            root: &f.root,
            home,
            git_common_dir: None,
            level: PolicyLevel::Block,
        };
        evaluate(&synthetic("write", json!({ "path": target })), &ctx)
    }

    /// Whether `target` is refused with `home` as the guard's home. An error
    /// refuses at the hook adapter, but it is not the precise refusal these
    /// tests want, so it fails the test with its message.
    fn refused_under(f: &Fixture, home: &Path, target: &Path) -> bool {
        match verdict_under(f, home, target) {
            Ok(violations) => !violations.is_empty(),
            Err(error) => panic!("home {}: {}: {error}", home.display(), target.display()),
        }
    }

    /// Whether the guard cannot judge `target` and returns an error naming
    /// `cause`, which the hook adapter turns into a refusal
    /// (`codeflow hook edit-guard` exits 2).
    #[cfg(windows)]
    fn unjudged_under(f: &Fixture, home: &Path, target: &Path, cause: &str) -> bool {
        match verdict_under(f, home, target) {
            Ok(violations) => panic!(
                "home {}: {}: judged, with {} findings",
                home.display(),
                target.display(),
                violations.len()
            ),
            Err(error) => error.to_string().contains(cause),
        }
    }

    /// TSK-1400: the authority directory is covered under any spelling of
    /// the home, in any case, and an ordinary file beside it stays writable.
    #[test]
    fn the_authority_directory_is_covered_whatever_the_homes_spelling() {
        let f = Fixture::new();
        let spelled = f.temp.path().join("absent").join("..").join("home");
        for name in [
            ".codeflow/authority/config.toml",
            ".codeflow/Authority/config.toml",
            ".CODEFLOW/AUTHORITY/new",
        ] {
            for target in [f.home.join(name), spelled.join(name)] {
                assert!(refused_under(&f, &spelled, &target), "{}", target.display());
            }
        }
        assert!(!refused_under(
            &f,
            &spelled,
            &f.home.join(".codeflow/config.toml")
        ));
    }

    /// Restores a directory's mode when a test ends, so its temporary tree
    /// can be removed even after a failed assertion.
    #[cfg(unix)]
    struct ModeGuard(PathBuf, u32);

    #[cfg(unix)]
    impl Drop for ModeGuard {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(self.1));
        }
    }

    /// TSK-1400: the home is given through a linked ancestor, the authority
    /// directory's parent cannot be listed, and the target names the
    /// directory through another link, so it is judged in the canonical
    /// spelling. The canonical home keeps that spelling covered.
    #[cfg(unix)]
    #[test]
    fn the_canonical_authority_directory_stays_covered_when_its_parent_cannot_be_listed() {
        use std::os::unix::fs::PermissionsExt as _;
        let f = Fixture::new();
        let real = f.temp.path().join("real");
        std::fs::create_dir_all(real.join("home/.codeflow/authority")).unwrap();
        std::os::unix::fs::symlink(&real, f.temp.path().join("link")).unwrap();
        let home = f.temp.path().join("link/home");
        let canonical = std::fs::canonicalize(real.join("home/.codeflow/authority")).unwrap();
        let door = f.temp.path().join("door");
        std::os::unix::fs::symlink(&canonical, &door).unwrap();
        let parent = real.join("home/.codeflow");
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o100)).unwrap();
        let _restore = ModeGuard(parent, 0o755);
        // The hook judges inside one invocation snapshot; so does this test.
        let _facts = RepoFactsScope::enter();
        // Named directly, the canonical path cannot be walked either, so
        // that edit refuses with an error; through `door` it resolves.
        let target = door.join("config.toml");
        assert!(refused_under(&f, &home, &target), "{}", target.display());
    }

    /// TSK-1400: a home that does not exist still lets an ordinary edit
    /// through and covers the directory under it; a home that is a broken
    /// link cannot be resolved, so every edit refuses.
    #[cfg(unix)]
    #[test]
    fn a_missing_home_is_judged_and_an_unresolvable_home_refuses() {
        let f = Fixture::new();
        let missing = f.temp.path().join("missing-home");
        assert!(!refused_under(&f, &missing, &f.root.join("notes.md")));
        assert!(refused_under(
            &f,
            &missing,
            &missing.join(".codeflow/Authority/config.toml")
        ));
        let broken = f.temp.path().join("broken-home");
        std::os::unix::fs::symlink(f.temp.path().join("nowhere"), &broken).unwrap();
        let ctx = EditContext {
            cwd: &f.root,
            root: &f.root,
            home: &broken,
            git_common_dir: None,
            level: PolicyLevel::Block,
        };
        let request = synthetic("write", json!({ "path": f.root.join("notes.md") }));
        let error = evaluate(&request, &ctx).unwrap_err().to_string();
        assert!(error.contains(&broken.display().to_string()), "{error}");
        assert!(error.contains("operator-authority directory"), "{error}");
        assert!(error.contains("every edit is refused"), "{error}");
    }

    /// TSK-1400 residual 1: a share can export the home under another
    /// name, so a UNC target is judged only inside a UNC directory the
    /// session chose; any other share refuses. Drive paths are no shares.
    #[test]
    fn a_share_target_is_judged_only_inside_a_chosen_share() {
        let chosen = ["//server/homes/u".to_string(), "//server/proj".to_string()];
        for target in [
            "//server/homes/u",
            "//server/homes/u/.codeflow/authority/config.toml",
            "//server/proj/src/main.rs",
            "c:/users/u/.codeflow/authority/config.toml",
        ] {
            assert!(!outside_chosen_shares(target, &chosen), "{target}");
        }
        for target in [
            "//server/homes/u2/x",
            "//server/homes/x",
            "//server/users/u/.codeflow/authority/config.toml",
            "//localhost/users/u/.codeflow/authority/config.toml",
            "//other/proj/src/main.rs",
            "//server/project/x",
        ] {
            assert!(outside_chosen_shares(target, &chosen), "{target}");
        }
        assert!(outside_chosen_shares("//server/proj/x", &[]));
        // A `..` name climbs within the share as Win32 reads it, so a
        // target cannot leave a chosen directory unseen.
        for (text, folded) in [
            ("//server/homes/u/../u2/x", "//server/homes/u2/x"),
            ("//server/homes/u/.. /u2/x", "//server/homes/u2/x"),
            ("//server/homes/u/./x/", "//server/homes/u/x"),
            ("//server/proj/../../x", "//server/proj/x"),
            ("//server/homes/u", "//server/homes/u"),
        ] {
            assert_eq!(folded_share(text).as_deref(), Some(folded), "{text}");
        }
        assert!(outside_chosen_shares(
            &folded_share("//server/homes/u/../u2/x").unwrap(),
            &chosen
        ));
        assert_eq!(folded_share("c:/users/u"), None);
    }

    /// Spellings of the authority directory under `plain`, the home as a
    /// plain drive path (`C:\...`): those the guard maps to the directory,
    /// and those it cannot map to one file, each with the cause its error
    /// names.
    #[cfg(windows)]
    fn windows_authority_spellings(plain: &str) -> (Vec<String>, Vec<(String, &'static str)>) {
        assert_eq!(&plain[1..3], r":\", "{plain}");
        // `drive` is the letter and `rest` the path after `C:\`.
        let (drive, rest) = (&plain[..1], &plain[3..]);
        // The same paths with `/` separators.
        let (plain_slashed, rest_slashed) = (plain.replace('\\', "/"), rest.replace('\\', "/"));
        let covered = vec![
            // Ordinary drive-absolute, with mixed separators.
            format!(r"{plain}/.codeflow\authority/config.toml"),
            // Drive-rooted without a letter: the cwd's drive, which the
            // fixture's home shares.
            format!(r"\{rest}\.codeflow\authority\config.toml"),
            format!("/{rest_slashed}/.codeflow/authority/config.toml"),
            format!(r"\{rest}/.codeflow\authority/config.toml"),
            // Device paths, normalized, with either separator.
            format!(r"\\./{plain}\.codeflow/authority\config.toml"),
            format!("//./{plain_slashed}/.codeflow/authority/config.toml"),
            format!(r"\\.\{plain}\.\.codeflow\authority\config.toml"),
            format!(r"\??\UNC\localhost\{drive}$\{rest}\.codeflow\authority\config.toml"),
            format!(r"{plain}\.codeflow\authority \config.toml"),
            format!(r"{plain}\.codeflow\authority. \config.toml"),
            format!(r"{plain}\.codeflow\authority\config.toml:notes"),
            format!(r"{plain}\.codeflow\authority:notes"),
            format!(r"\\?\{plain}\.codeflow\Authority\config.toml"),
            format!(r"\\.\{plain}\.codeflow\authority\config.toml"),
            format!(r"\??\{plain}\.codeflow\authority\config.toml"),
            format!(r"\\localhost\{drive}$\{rest}\.codeflow\authority\config.toml"),
            format!(r"\\127.0.0.1\{drive}$\{rest}\.codeflow\authority\config.toml"),
            format!(r"\\?\UNC\localhost\{drive}$\{rest}\.codeflow\authority\config.toml"),
        ];
        let verbatim = "cannot map a verbatim path";
        let stream = "alternate-stream";
        let unjudged = vec![
            (
                format!(r"\\?\{plain}\.codeflow\authority.\config.toml"),
                verbatim,
            ),
            (
                format!(r"\\?\{plain}\.codeflow\authority \config.toml"),
                verbatim,
            ),
            (
                format!(r"{plain}\.codeflow\authority\config.toml::$DATA"),
                stream,
            ),
            (
                format!(r"{plain}\.codeflow::$INDEX_ALLOCATION\authority\config.toml"),
                stream,
            ),
            (
                format!(r"\\?\GLOBALROOT\Device\HarddiskVolume1\{rest}\.codeflow\authority\x"),
                "device or verbatim namespace",
            ),
            (
                format!(r"{drive}:.codeflow\authority\config.toml"),
                "drive-relative",
            ),
            // A bare volume or a drive-relative name after a namespace
            // prefix names no rooted file (finding 1).
            (format!(r"\??\{drive}:"), "device or verbatim namespace"),
            (
                format!(r"\??\{drive}:.codeflow\authority\config.toml"),
                "device or verbatim namespace",
            ),
            (format!(r"\\?\{drive}:"), "device or verbatim namespace"),
            (format!(r"\\.\{drive}:"), "device or verbatim namespace"),
            (r"\??\UNC\localhost".to_string(), "incomplete UNC"),
            (r"\\?\UNC\localhost\".to_string(), "incomplete UNC"),
            (r"\\localhost".to_string(), "incomplete UNC"),
            // A `/` inside a verbatim or NT path is part of a name.
            (
                format!(r"\??\{plain}/.codeflow/authority/config.toml"),
                "contains `/`",
            ),
            (
                format!(r"\\?\{plain}/.codeflow/authority/config.toml"),
                "contains `/`",
            ),
            // Mixed namespace prefixes whose reading is unproved.
            (
                format!("/??/{plain_slashed}/.codeflow/authority/config.toml"),
                "NT namespace path written with `/`",
            ),
            (
                format!(r"\??/{plain}\.codeflow\authority\config.toml"),
                "NT namespace path written with `/`",
            ),
            (
                format!("//?/{plain_slashed}/.codeflow/authority/config.toml"),
                "verbatim path written with `/`",
            ),
            (
                format!(r"\\?/{plain}\.codeflow\authority\config.toml"),
                "verbatim path written with `/`",
            ),
            (r"\\?".to_string(), "device root"),
            // A device path can climb out of its drive.
            (
                format!(r"\\.\{plain}\x\..\.codeflow\authority\config.toml"),
                "`..` name",
            ),
            // Residual 1: a share outside the chosen directories can export
            // the home under any name.
            (
                format!(r"\\localhost\Users\{rest}\.codeflow\authority\config.toml"),
                "network share",
            ),
            (
                format!(r"\\?\UNC\localhost\share\{rest}\.codeflow\authority\x"),
                "network share",
            ),
        ];
        (covered, unjudged)
    }

    /// TSK-1400: the same coverage under the path spellings Windows accepts
    /// for one directory. Volumes with short names disabled give the long
    /// spelling back, which still runs every other form. A spelling the
    /// guard cannot map to one file (a verbatim name Win32 would change, a
    /// stream type, another namespace, a bare volume, a mixed namespace
    /// prefix, a device path that climbs, an incomplete share, a share
    /// outside the chosen directories, a drive-relative path) returns an
    /// error, which the hook adapter refuses.
    #[cfg(windows)]
    #[test]
    fn the_authority_directory_is_covered_under_windows_spellings_of_the_home() {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

        let f = Fixture::new();
        let home = f.temp.path().join("Long home directory name");
        std::fs::create_dir_all(home.join(".codeflow")).unwrap();
        let long = crate::portable_path::canonicalize(&home).unwrap();
        let wide: Vec<u16> = long.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: the input is terminated; a null output queries the size.
        let size = unsafe { GetShortPathNameW(wide.as_ptr(), std::ptr::null_mut(), 0) };
        assert!(size > 0, "{}", std::io::Error::last_os_error());
        let mut short = vec![0; size as usize];
        // SAFETY: the output has the exact size returned by the first call.
        let written = unsafe { GetShortPathNameW(wide.as_ptr(), short.as_mut_ptr(), size) };
        assert!(written > 0 && written < size);
        short.truncate(written as usize);
        let short = PathBuf::from(std::ffi::OsString::from_wide(&short));
        let text = |path: &Path| path.to_str().unwrap().to_string();
        let homes = [
            home.clone(),
            long.clone(),
            short.clone(),
            PathBuf::from(text(&short).replace('\\', "/")),
            PathBuf::from(text(&long).replace('\\', "/")),
            PathBuf::from(text(&long).to_lowercase()),
            PathBuf::from(text(&long).to_uppercase()),
            PathBuf::from(format!(r"\\?\{}", text(&long))),
            PathBuf::from(format!("{}.", text(&long))),
        ];
        let (covered, unjudged) = windows_authority_spellings(&text(&long));
        for spelling in &homes {
            let verbatim = text(spelling).starts_with(r"\\?\");
            for target in [
                spelling.join(r".codeflow\Authority\config.toml"),
                spelling.join(".codeflow/AUTHORITY/config.toml"),
                long.join(r".codeflow\Authority\config.toml"),
                short.join(".codeflow/Authority/config.toml"),
                home.join(".codeflow/Authority/new"),
            ]
            .into_iter()
            .chain(covered.iter().map(PathBuf::from))
            {
                assert!(
                    refused_under(&f, spelling, &target),
                    "home {}: {}",
                    spelling.display(),
                    target.display()
                );
            }
            // Under a verbatim home the trailing dot stays in the name, so
            // the path names another directory that no ordinary path can
            // reach; the guard cannot map it and refuses with an error.
            let dotted = spelling.join(r".codeflow\authority.\config.toml");
            if verbatim {
                assert!(
                    unjudged_under(&f, spelling, &dotted, "cannot map a verbatim path"),
                    "home {}: {}",
                    spelling.display(),
                    dotted.display()
                );
            } else {
                assert!(
                    refused_under(&f, spelling, &dotted),
                    "home {}: {}",
                    spelling.display(),
                    dotted.display()
                );
            }
            for (target, cause) in &unjudged {
                assert!(
                    unjudged_under(&f, spelling, Path::new(target), cause),
                    "home {}: {target}",
                    spelling.display()
                );
            }
            assert!(
                !refused_under(&f, spelling, &spelling.join(".codeflow/config.toml")),
                "home {}",
                spelling.display()
            );
        }
    }

    /// TSK-1400 finding 2: a namespace, share or trimmed spelling of a
    /// shell startup file refuses under the always-blocking startup rule
    /// with hook integrity off, because the rule judges the mapped target
    /// and its normalized spellings, not only the path as written.
    #[cfg(windows)]
    #[test]
    fn startup_files_refuse_under_namespace_spellings_with_integrity_off() {
        let f = Fixture::new();
        let home = crate::portable_path::canonicalize(&f.home).unwrap();
        let plain = home.to_str().unwrap().to_string();
        assert_eq!(&plain[1..3], r":\", "{plain}");
        let (drive, rest) = (&plain[..1], &plain[3..]);
        let ctx = EditContext {
            cwd: &f.root,
            root: &f.root,
            home: &home,
            git_common_dir: None,
            level: PolicyLevel::Off,
        };
        for target in [
            format!(r"\??\{plain}\.bashrc"),
            format!(r"\\?\{plain}\.bashrc"),
            format!(r"\\.\{plain}\.bashrc"),
            format!(r"\\localhost\{drive}$\{rest}\.bashrc"),
            format!(r"\??\UNC\localhost\{drive}$\{rest}\.bashrc"),
            format!(r"{plain}\.bashrc."),
            format!(r"\{rest}\.bashrc"),
        ] {
            let violations = evaluate(&synthetic("write", json!({ "path": target })), &ctx)
                .unwrap_or_else(|error| panic!("{target}: {error}"));
            assert!(
                violations
                    .iter()
                    .any(|v| v.rule == crate::security::startup::RULE && v.level_fixed),
                "{target}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn f8_uninspectable_pattern_does_not_block_unrelated_targets() {
        let f = Fixture::new();
        // A symlink loop gives deterministic metadata failure, even under root.
        std::os::unix::fs::symlink(".claude", f.home.join(".claude")).unwrap();
        assert!(!f.refused("notes.md"));
        assert!(!enforcement_path("src/lib.rs", &f.root, &f.root, Some(&f.home)).unwrap());
        assert!(evaluate(
            &synthetic(
                "write",
                json!({"path":f.home.join(".claude/settings.json")})
            ),
            &f.ctx()
        )
        .is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn f7_case_aliases_preserve_the_protected_suffix() {
        let f = Fixture::new();
        std::fs::create_dir(f.root.join(".codeflow")).unwrap();
        std::fs::write(f.root.join(".codeflow/policy.json"), "{}").unwrap();
        // The fixture must actually exercise a case-insensitive volume.
        assert!(f.root.join(".CODEFLOW/POLICY.JSON").exists());
        for path in [".CODEFLOW/policy.json", ".CODEFLOW/POLICY.JSON"] {
            assert!(f.refused(path), "{path}");
            assert!(enforcement_path(path, &f.root, &f.root, Some(&f.home)).unwrap());
        }
        assert!(!f.refused(".CODEFLOW/notes.md"));
        assert!(!f.refused(".CODEFLOW/policy.json.example"));
        std::fs::create_dir(f.root.join(".codex")).unwrap();
        assert!(f.refused(".CODEX/new.json"));
    }

    #[test]
    fn documented_hook_fixtures_protect_paths_and_allow_ordinary_edits() {
        let fixture = Fixture::new();
        // The root goes into JSON strings: a Windows path's backslashes are
        // escaped there.
        let root = serde_json::to_string(fixture.root.to_str().unwrap()).unwrap();
        let root = root.trim_matches('"');
        for data in [
            include_str!("../../tests/fixtures/edit-hooks/codex-apply-patch.json"),
            include_str!("../../tests/fixtures/edit-hooks/grok-write.json"),
            include_str!("../../tests/fixtures/edit-hooks/grok-search-replace.json"),
        ] {
            let protected = data
                .replace("/fixture/project", root)
                .replace("hello.txt", ".codex/config.toml")
                .replace("notes.txt", ".codeflow/policy.json");
            let request = parse_payload(&protected).unwrap().unwrap();
            assert!(!evaluate(&request, &fixture.ctx()).unwrap().is_empty());
            let ordinary = protected
                .replace(".codeflow/policy.json", "notes.md")
                .replace(".codex/config.toml", "notes.md");
            let request = parse_payload(&ordinary).unwrap().unwrap();
            assert!(evaluate(&request, &fixture.ctx()).unwrap().is_empty());
        }
    }

    /// TSK-242 AC-1: a native edit of a shell startup file refuses under
    /// `security.shell_startup` through every edit payload, in each
    /// spelling, whatever the integrity level; an ordinary file passes.
    #[test]
    fn startup_files_refuse_through_every_edit_payload() {
        let f = Fixture::new();
        let home = f.home.to_str().unwrap().to_string();
        let startup = |request: &EditRequest, ctx: &EditContext<'_>| {
            evaluate(request, ctx)
                .unwrap()
                .iter()
                .any(|v| v.rule == "security.shell_startup" && v.level_fixed)
        };
        let spellings = [
            "~/.zshrc".to_string(),
            format!("{home}/.bashrc"),
            format!("{home}/.ZSHENV"),
            format!("{home}/.config/fish/conf.d/x.fish"),
            format!("{home}/Documents/PowerShell/Microsoft.PowerShell_profile.ps1"),
            ".envrc".to_string(),
            "sub/.envrc".to_string(),
            "/etc/profile.d/x.sh".to_string(),
        ];
        for path in &spellings {
            let patch = format!("*** Begin Patch\n*** Add File: {path}\n+x\n*** End Patch\n");
            for request in [
                synthetic("Write", json!({"file_path": path})),
                synthetic(
                    "Edit",
                    json!({"file_path": path, "old_string": "a", "new_string": "b"}),
                ),
                synthetic("write", json!({"path": path})),
                synthetic("search_replace", json!({"path": path})),
                synthetic("apply_patch", json!(patch)),
            ] {
                assert!(startup(&request, &f.ctx()), "{path}");
                let off = EditContext {
                    level: PolicyLevel::Off,
                    ..f.ctx()
                };
                assert!(startup(&request, &off), "{path} with integrity off");
            }
        }
        // Relative from a home working directory, and through a link.
        let in_home = EditContext {
            cwd: &f.home,
            ..f.ctx()
        };
        assert!(startup(
            &synthetic("Write", json!({"file_path": ".zprofile"})),
            &in_home
        ));
        #[cfg(unix)]
        {
            std::fs::write(f.home.join(".zshrc"), "x").unwrap();
            std::os::unix::fs::symlink(f.home.join(".zshrc"), f.root.join("rc")).unwrap();
            assert!(startup(
                &synthetic("Write", json!({"file_path": "rc"})),
                &f.ctx()
            ));
        }
        for ordinary in ["notes.md", "src/zshrc.rs", "docs/profile.md"] {
            let request = synthetic("Write", json!({"file_path": ordinary}));
            assert!(
                evaluate(&request, &f.ctx()).unwrap().is_empty(),
                "{ordinary}"
            );
        }
    }

    #[test]
    fn synthetic_snake_and_camel_payloads_extract_paths() {
        for (tool, input) in [
            ("Write", json!({"file_path":"src/lib.rs"})),
            ("Edit", json!({"filePath":"src/lib.rs"})),
            ("write", json!({"path":"src/lib.rs"})),
            ("search_replace", json!({"file_path":"src/lib.rs"})),
        ] {
            let snake = synthetic(tool, input.clone());
            let camel = parse_payload(
                &json!({"toolName":tool,"toolInput":input,"cwd":"/repo"}).to_string(),
            )
            .unwrap()
            .unwrap();
            assert_eq!(snake.paths, camel.paths);
            assert_eq!(camel.paths, vec![PathBuf::from("src/lib.rs")]);
        }
    }

    #[test]
    fn synthetic_patch_includes_add_delete_update_and_move_targets() {
        let patch = "*** Begin Patch\n*** Add File: a\n+x\n*** Delete File: b\n*** Update File: c\n*** Move to: d\n@@\n-x\n+y\n*** End Patch\n";
        for input in [json!(patch), json!({"patch":patch}), json!({"input":patch})] {
            assert_eq!(
                synthetic("apply_patch", input).paths,
                ["a", "b", "c", "d"].map(PathBuf::from)
            );
        }
        assert!(patch_paths("*** Begin Patch\n*** Move to: x\n*** End Patch").is_err());
        assert!(patch_paths("*** Begin Patch\n*** Add File: a\n+x").is_err());
    }

    #[test]
    fn every_table_enforcement_pattern_has_a_refused_witness() {
        let f = Fixture::new();
        for rule in &actions::table().claude_edit_denies {
            let pattern = rule
                .strip_prefix("Edit(")
                .unwrap()
                .strip_suffix(')')
                .unwrap();
            let path = if let Some(relative) = pattern.strip_prefix("~/") {
                f.home.join(relative.replace("**", "witness.json"))
            } else {
                f.root.join(
                    pattern
                        .strip_prefix('/')
                        .unwrap()
                        .replace("**", "witness.json"),
                )
            };
            assert!(f.refused(path.to_str().unwrap()), "{rule}");
        }
    }

    #[test]
    fn paired_paths_and_levels_use_the_existing_rule() {
        let f = Fixture::new();
        for path in [
            ".codeflow/policy.json",
            "./.codeflow//policy.json",
            "src/../.codeflow/policy.json",
            ".codex/new.json",
        ] {
            assert!(f.refused(path), "{path}");
        }
        for path in [
            "src/lib.rs",
            "README.md",
            ".env.example",
            ".codeflow/policy.json.example",
            ".codex-notes/config.toml",
            "assets/base/settings/default.json",
        ] {
            assert!(!f.refused(path), "{path}");
        }
        let request = synthetic("write", json!({"path":".codeflow/policy.json"}));
        for level in [PolicyLevel::Block, PolicyLevel::Warn] {
            let mut ctx = f.ctx();
            ctx.level = level;
            let findings = evaluate(&request, &ctx).unwrap();
            assert_eq!(findings.len(), 1);
            assert_eq!(findings[0].rule, "git.hook_integrity");
            assert_eq!(findings[0].level, level);
        }
        for level in [PolicyLevel::Allow, PolicyLevel::Off] {
            let mut ctx = f.ctx();
            ctx.level = level;
            assert!(evaluate(&request, &ctx).unwrap().is_empty());
        }
    }

    #[test]
    fn protected_patch_destination_and_source_both_refuse() {
        let f = Fixture::new();
        for (source, destination) in [
            ("src/a", ".codeflow/policy.json"),
            (".codeflow/policy.json", "src/a"),
        ] {
            let patch = format!("*** Begin Patch\n*** Add File: docs/new.md\n+ordinary\n*** Update File: {source}\n*** Move to: {destination}\n@@\n-x\n+y\n*** End Patch\n");
            let request = synthetic("apply_patch", json!(patch));
            assert_eq!(evaluate(&request, &f.ctx()).unwrap().len(), 1);
        }
        let content_only = "*** Begin Patch\n*** Add File: docs/example.md\n+*** Delete File: .codeflow/policy.json\n*** End Patch\n";
        assert!(
            evaluate(&synthetic("apply_patch", json!(content_only)), &f.ctx())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn shared_interpreter_entry_matches_paths_without_contents() {
        let f = Fixture::new();
        assert!(enforcement_path("~/.codex/config.toml", &f.root, &f.root, Some(&f.home)).unwrap());
        assert!(
            enforcement_path(".codeflow/policy.json", &f.root, &f.root, Some(&f.home)).unwrap()
        );
        assert!(!enforcement_path("README.md", &f.root, &f.root, Some(&f.home)).unwrap());
        assert!(enforcement_path("README.md", &f.root, &f.root, None).is_err());
    }

    #[test]
    fn aliases_must_agree_and_edit_targets_must_exist_in_payload() {
        for payload in [
            json!({"tool_name":"write","toolName":"Bash","tool_input":{"path":"x"}}),
            json!({"toolName":"write","toolInput":{"path":"x","file_path":".codex/config.toml"}}),
            json!({"toolName":"write","toolInput":{}}),
            json!({"tool_name":"apply_patch","tool_input":{"patch":"bad"}}),
        ] {
            assert!(parse_payload(&payload.to_string()).is_err());
        }
        assert!(parse_payload(r#"{"tool_name":"Read"}"#).unwrap().is_none());
    }

    #[test]
    fn cwd_relative_targets_and_git_common_dir_are_covered() {
        let f = Fixture::new();
        let subdir = f.root.join("src");
        std::fs::create_dir(&subdir).unwrap();
        let request = parse_payload(&json!({"toolName":"write","toolInput":{"path":"../.codeflow/policy.json"},"cwd":subdir}).to_string()).unwrap().unwrap();
        assert_eq!(evaluate(&request, &f.ctx()).unwrap().len(), 1);
        let common = f.temp.path().join("main/.git");
        // A gitfile must name real repository metadata; a dangling/empty
        // administration directory is deliberately refused by discovery.
        git2::Repository::init(f.temp.path().join("main"))
            .unwrap()
            .set_workdir(&f.root, true)
            .unwrap();
        std::fs::write(
            f.root.join(".git"),
            format!("gitdir: {}\n", common.display()),
        )
        .unwrap();
        let mut ctx = f.ctx();
        ctx.git_common_dir = Some(&common);
        assert!(
            evaluate(&synthetic("write", json!({"path":"src/lib.rs"})), &ctx)
                .unwrap()
                .is_empty()
        );
        let request = synthetic("write", json!({"path":common.join("config")}));
        assert_eq!(evaluate(&request, &ctx).unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_ancestors_new_files_and_parent_traversal_are_covered() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        std::fs::create_dir_all(f.root.join(".codex/inside")).unwrap();
        symlink(f.root.join(".codex"), f.root.join("alias")).unwrap();
        assert!(f.refused("alias/new-file.json"));
        symlink(f.root.join(".codex/inside"), f.root.join("inner")).unwrap();
        assert!(f.refused("inner/../config.toml"));
        let ordinary = f.temp.path().join("ordinary");
        std::fs::create_dir(&ordinary).unwrap();
        symlink(&ordinary, f.root.join("ordinary-link")).unwrap();
        assert!(!f.refused("ordinary-link/new-file.rs"));
    }

    #[cfg(unix)]
    #[test]
    fn enforcement_file_symlink_protects_its_real_destination() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        std::fs::create_dir(f.root.join(".codeflow")).unwrap();
        let shared = f.temp.path().join("shared-policy.json");
        std::fs::write(&shared, "{}").unwrap();
        symlink(&shared, f.root.join(".codeflow/policy.json")).unwrap();
        assert!(f.refused(shared.to_str().unwrap()));
    }
    /// A repository whose linked worktree has the administrative folder name
    /// `caf\xe9`, which is not valid UTF-8, and the path of that folder. `None`
    /// when the file system refuses the name (APFS does), so the test runs where
    /// it can, as on Linux.
    #[cfg(unix)]
    fn repository_with_a_worktree_named_in_latin1() -> Option<(tempfile::TempDir, PathBuf)> {
        use std::os::unix::ffi::OsStrExt as _;
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let admin = dir
            .path()
            .join(".git")
            .join("worktrees")
            .join(std::ffi::OsStr::from_bytes(b"caf\xe9"));
        std::fs::create_dir_all(&admin).ok()?;
        let checkout = dir.path().join("linked");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::write(
            admin.join("gitdir"),
            format!("{}\n", checkout.join(".git").display()),
        )
        .unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::write(admin.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        // A registered checkout needs the inverse pointer too. Keep the
        // administrative name's exact bytes rather than its display form.
        let mut marker = b"gitdir: ".to_vec();
        marker.extend_from_slice(admin.as_os_str().as_bytes());
        marker.push(b'\n');
        std::fs::write(checkout.join(".git"), marker).unwrap();
        Some((dir, admin))
    }

    /// TSK-422 AC-8: an agent's direct edit of the coordination ledger is
    /// refused under the reach rule, in the main checkout and through a
    /// linked worktree, while `CodeFlow`'s other state beside it is not.
    #[test]
    fn the_coordination_ledger_is_refused_to_a_direct_edit() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let common = repo.commondir().to_path_buf();
        for file in [
            "events.jsonl",
            "counters.json",
            "coord.lock",
            "status/a.json",
        ] {
            let target = common.join("codeflow/coord").join(file);
            assert!(
                repository_authority_target(&target, dir.path(), false),
                "{file}"
            );
        }
        assert!(repository_authority_target(
            &common.join("codeflow"),
            dir.path(),
            true
        ));
        assert!(!repository_authority_target(
            &common.join("codeflow/registry-state.json"),
            dir.path(),
            false
        ));
    }

    /// Review finding on issue 79: a worktree whose folder name is not valid
    /// UTF-8 used to drop out of the protected list, leaving its
    /// `config.worktree` editable.
    #[cfg(unix)]
    #[test]
    fn a_worktree_named_in_latin1_keeps_its_config_protected() {
        let Some((dir, admin)) = repository_with_a_worktree_named_in_latin1() else {
            return;
        };
        let target = admin.join("config.worktree");
        assert!(repository_authority_target(&target, dir.path(), false));
        // TSK-381 SF-3: every path under a git-directory component with a
        // non-ASCII byte is refused, failing closed; an ordinary ASCII file
        // of the git directory stays outside the authority paths.
        assert!(repository_authority_target(
            &admin.join("other"),
            dir.path(),
            false
        ));
        assert!(!repository_authority_target(
            &dir.path().join(".git/description"),
            dir.path(),
            false
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_worktree_named_in_latin1_is_still_a_protected_checkout() {
        let Some((dir, _admin)) = repository_with_a_worktree_named_in_latin1() else {
            return;
        };
        let repo = git2::Repository::open(dir.path()).unwrap();
        let roots = checkout_roots(&repo).unwrap();
        assert!(
            roots.iter().any(|root| root.ends_with("linked")),
            "{roots:?}"
        );
    }

    /// Kept strict (issue 79): the rules that protect a path are text globs,
    /// so a path that is not valid UTF-8 is refused, not matched lossily.
    #[cfg(unix)]
    #[test]
    fn an_enforcement_path_that_is_not_utf8_is_refused() {
        use std::os::unix::ffi::OsStrExt as _;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/repo/caf\xe9.md"));
        let error = path_text(path).unwrap_err();
        assert!(error.0.contains("non-UTF-8"), "{}", error.0);
        assert!(path_text(Path::new("/repo/cafe.md")).is_ok());
    }
}

#[cfg(test)]
mod r22_tests {
    use super::*;
    #[test]
    fn r22_checkout_roots_refuses_unreadable_main() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        assert!(!checkout_roots(&repo).unwrap().is_empty());
        std::fs::remove_file(repo.path().join("HEAD")).unwrap();
        assert!(checkout_roots(&repo).is_err());
    }
    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn r22_normalize_case_refuses_unreadable_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mut path = dir.path().join("Present");
        std::fs::write(&path, "").unwrap();
        let metadata = std::fs::metadata(&path).unwrap();
        assert!(!format!("{:?}", normalize_case(&mut path, &metadata)).starts_with("Err("));
        assert!(path.ends_with("Present"));
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(dir.path()).unwrap();
        assert!(format!("{:?}", normalize_case(&mut path, &metadata)).starts_with("Err("));
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn r22_normalize_case_keeps_unicode_case_alias_readable() {
        let dir = tempfile::tempdir().unwrap();
        let actual = dir.path().join("CAFÉ");
        std::fs::write(&actual, "").unwrap();
        let mut alias = dir.path().join("café");
        if let Ok(metadata) = std::fs::metadata(&alias) {
            normalize_case(&mut alias, &metadata).unwrap();
            assert_eq!(alias, actual);
        }
    }
    #[test]
    fn r22_normalized_missing_optional_path_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        assert!(normalized(&dir.path().join("missing/new.txt"), true).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn normalized_snapshot_ends_with_the_guard_invocation() {
        let temp = tempfile::tempdir().unwrap();
        let root = normalized(temp.path(), true).unwrap();
        let target = root.join("alias");
        let first = root.join("first");
        let second = root.join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        std::os::unix::fs::symlink(&first, &target).unwrap();
        {
            let _outer = RepoFactsScope::enter();
            {
                let _nested = RepoFactsScope::enter();
                assert_eq!(normalized(&target, true).unwrap(), first);
            }
            std::fs::remove_file(&target).unwrap();
            std::os::unix::fs::symlink(&second, &target).unwrap();
            // Nested checks share one snapshot, even if the filesystem changes.
            assert_eq!(normalized(&target, true).unwrap(), first);
        }
        let _next = RepoFactsScope::enter();
        assert_eq!(normalized(&target, true).unwrap(), second);
    }

    #[test]
    fn normalized_interior_drive_text_keeps_its_parent() {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::portable_path::canonicalize(dir.path()).unwrap();
        git2::Repository::init(&root).unwrap();
        for suffix in ["quoted/C:/tail", "quoted/c:/tail", "quoted/C:/../ordinary"] {
            let path = root.join(suffix);
            for resolve in [false, true] {
                let result = normalized(&path, resolve).unwrap();
                assert!(result.starts_with(&root), "{path:?}: {result:?}");
                assert!(super::repository_enforcement_target(&path, &root, false)
                    .is_ok_and(|protected| !protected));
            }
        }
    }
}
