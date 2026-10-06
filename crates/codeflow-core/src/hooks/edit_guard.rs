//! Enforcement-path protection for native file-edit tools.
//! Native Grok captures, a documented Codex schema, and synthetic path edge cases.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};

use super::{PolicyLevel, Violation};
use crate::security::{actions, pattern::matches_extended_glob};

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
    evaluate(&request, &ctx)
        .map(|violations| !violations.is_empty())
        .map_err(|e| e.to_string())
}

fn absolute_target(path: &Path, cwd: &Path, home: &Path) -> PathBuf {
    if let Ok(relative) = path.strip_prefix("~") {
        return home.join(relative);
    }
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
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
    let mut seen = BTreeSet::new();
    let mut violations = Vec::new();
    for path in &request.paths {
        let absolute = absolute_target(path, cwd, ctx.home);
        let lexical = normalized(&absolute, false)?;
        let resolved = normalized(&absolute, true)?;
        for candidate in [&lexical, &resolved] {
            let text = path_text(candidate)?;
            let authority = repository_authority_target(candidate, ctx.root, false);
            if (authority
                || (ctx.level.is_active()
                    && (protected.iter().any(|pattern| covers(&text, pattern))
                        || repository_enforcement_target(candidate, ctx.root, false)
                            .map_err(EditError)?)))
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
            reduced.into_iter().collect::<Vec<_>>().join("; ")
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

fn covers(path: &str, pattern: &str) -> bool {
    matches_extended_glob(path, pattern)
        || pattern
            .strip_suffix("/**")
            .is_some_and(|parent| path == parent)
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
pub(crate) fn normalize_case(path: &mut PathBuf, metadata: &std::fs::Metadata) {
    use std::os::unix::fs::MetadataExt;
    let Some(parent) = path.parent() else {
        return;
    };
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_str()
            .is_some_and(|s| s.eq_ignore_ascii_case(name))
            && std::fs::symlink_metadata(entry.path())
                .is_ok_and(|m| m.dev() == metadata.dev() && m.ino() == metadata.ino())
        {
            *path = entry.path();
            return;
        }
    }
}

// Windows names one entry by its long name, an 8.3 short name (`RUNNER~1`)
// and any letter case; a harness cwd and git's paths often differ in that
// way. The canonical path names it once, in the plain drive form git writes.
#[cfg(windows)]
pub(crate) fn normalize_case(path: &mut PathBuf, _metadata: &std::fs::Metadata) {
    if let Ok(real) = crate::portable_path::canonicalize(path) {
        *path = real;
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn normalize_case(_path: &mut PathBuf, _metadata: &std::fs::Metadata) {}

// Resolve one component at a time. Lexically deleting `alias/..` before
// following `alias` is wrong when alias is a symlink into another directory.
// Missing suffixes are kept, so adding a new file beneath an existing symlink
// is judged against that symlink's destination. Broken links and permissions
// are errors, not an implicit allow.
fn normalized(path: &Path, resolve: bool) -> Result<PathBuf, EditError> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => result.push(prefix.as_os_str()),
            Component::RootDir => result.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::Normal(part) => {
                result.push(part);
                if resolve {
                    if crate::absence::proven_absent(&result).map_err(|e| {
                        EditError(format!("cannot inspect {}: {e}", result.display()))
                    })? {
                        continue;
                    }
                    match std::fs::symlink_metadata(&result) {
                        Ok(metadata) if metadata.file_type().is_symlink() => {
                            result = crate::portable_path::canonicalize(&result).map_err(|e| {
                                EditError(format!("cannot resolve {}: {e}", result.display()))
                            })?;
                        }
                        Ok(metadata) => {
                            normalize_case(&mut result, &metadata);
                        }
                        Err(e) => {
                            return Err(EditError(format!(
                                "cannot inspect {}: {e}",
                                result.display()
                            )))
                        }
                    }
                }
            }
        }
    }
    Ok(result)
}

pub(crate) const AUTHORITY_PATH: &str = "remote-tracking policy metadata";

// File edits cannot establish which config keys are safe. The Git command
// checker still permits ordinary `git config --global user.name ...` updates.
fn global_git_config_target(target: &Path, root: &Path) -> bool {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let mut paths = Vec::new();
    if let Some(home) = &home {
        paths.push(home.join(".gitconfig"));
    }
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|home| home.join(".config")));
    if let Some(xdg) = xdg {
        paths.push(xdg.join("git/config"));
    }
    // /dev/null deliberately disables global config; writes cannot change its contents.
    if let Some(path) = std::env::var_os("GIT_CONFIG_GLOBAL")
        .filter(|s| !s.is_empty() && s != std::ffi::OsStr::new("/dev/null"))
    {
        paths.push(root.join(path));
    }
    [false, true].into_iter().any(|resolve| {
        normalized(target, resolve).is_ok_and(|target| {
            paths
                .iter()
                .any(|path| normalized(path, resolve).is_ok_and(|path| path == target))
        })
    })
}

/// Authority metadata cannot be relaxed by a policy read from that metadata.
pub(crate) fn repository_authority_target(target: &Path, root: &Path, ancestors: bool) -> bool {
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
        if !target.starts_with(&common) {
            continue;
        }
        for (name, directory) in &paths {
            let Ok(protected) = normalized(&common.join(name), resolve) else {
                continue;
            };
            if target == protected
                || (*directory && target.starts_with(&protected))
                || (ancestors && protected.starts_with(&target))
            {
                return true;
            }
        }
    }
    false
}

/// Every checkout sharing this repository: its own working tree, the main
/// working tree and each registered linked worktree.
fn checkout_roots(repo: &git2::Repository) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    if let Some(workdir) = repo.workdir() {
        roots.push(workdir.to_path_buf());
    }
    if let Ok(main) = git2::Repository::open(repo.commondir()) {
        if let Some(workdir) = main.workdir() {
            roots.push(workdir.to_path_buf());
        }
    }
    roots.extend(
        crate::git::linked_worktrees(repo)?
            .into_iter()
            .map(|worktree| worktree.path),
    );
    Ok(roots)
}

/// The enforcement paths of every checkout sharing this repository, each
/// with whether it protects a whole directory and the base its ancestors
/// must lie in: the first component under its own checkout (`.claude`,
/// `.codeflow`), or the common git directory. A checkout nested in another
/// checkout's `.claude` (`.claude/worktrees/<name>`) is therefore not an
/// ancestor of its own files by lying inside the outer `.claude`.
fn protected_paths(repo: &git2::Repository) -> Result<Vec<(PathBuf, bool, PathBuf)>, String> {
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
    for root in checkout_roots(repo)? {
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
        ("hooks", true),
        ("refs/remotes", true),
        ("packed-refs", false),
        ("config", false),
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
    base: PathBuf,
    resolved_base: [std::cell::OnceCell<Result<PathBuf, String>>; 2],
}

type Protected = std::rc::Rc<Vec<ProtectedPath>>;

type ProtectedCache = std::collections::HashMap<PathBuf, Result<Option<Protected>, String>>;

thread_local! {
    static PROTECTED: std::cell::RefCell<Option<ProtectedCache>> =
        const { std::cell::RefCell::new(None) };
}

/// While alive, each repository's enforcement paths are read and resolved
/// once on this thread. One guard evaluation judges many words against the
/// same repository, which the command being judged has not changed yet
/// (TSK-216 round 16).
pub(crate) struct RepoFactsScope {
    outer: bool,
}

impl RepoFactsScope {
    pub(crate) fn enter() -> Self {
        let outer = PROTECTED.with(|cache| {
            let mut cache = cache.borrow_mut();
            let outer = cache.is_none();
            if outer {
                *cache = Some(std::collections::HashMap::new());
            }
            outer
        });
        Self { outer }
    }
}

impl Drop for RepoFactsScope {
    fn drop(&mut self) {
        if self.outer {
            PROTECTED.with(|cache| *cache.borrow_mut() = None);
        }
    }
}

/// The enforcement paths of the repository at `root` ([`protected_paths`]),
/// resolved once per [`RepoFactsScope`] and otherwise on each call. `None`
/// outside a repository.
fn protected_at(root: &Path) -> Result<Option<Protected>, String> {
    let read = || {
        let Some(repo) = super::repo::open(root)? else {
            return Ok(None);
        };
        let both = |path: &Path| {
            [false, true]
                .map(|resolve| normalized(path, resolve).map_err(|error| error.to_string()))
        };
        let resolved = protected_paths(&repo)?
            .iter()
            .map(|(path, directory, base)| ProtectedPath {
                path: both(path),
                directory: *directory,
                base: base.clone(),
                resolved_base: Default::default(),
            })
            .collect();
        Ok(Some(std::rc::Rc::new(resolved)))
    };
    PROTECTED.with(|cache| {
        let mut cache = cache.borrow_mut();
        match cache.as_mut() {
            Some(map) => map.entry(root.to_path_buf()).or_insert_with(read).clone(),
            None => read(),
        }
    })
}

/// Resolve enforcement paths in every checkout sharing this repository.
/// Native edits protect files; shell writes also protect their ancestors.
pub(crate) fn repository_enforcement_target(
    target: &Path,
    root: &Path,
    ancestors: bool,
) -> Result<bool, String> {
    let Some(protected) = protected_at(root)? else {
        return Ok(false);
    };
    for (reading, resolve) in [false, true].into_iter().enumerate() {
        let target = normalized(target, resolve).map_err(|error| error.to_string())?;
        for entry in protected.iter() {
            let path = entry.path[reading].as_ref().map_err(Clone::clone)?;
            if target == *path
                || (entry.directory && target.starts_with(path))
                || (ancestors
                    && path.starts_with(&target)
                    && target.starts_with(
                        entry.resolved_base[reading]
                            .get_or_init(|| {
                                normalized(&entry.base, resolve).map_err(|error| error.to_string())
                            })
                            .as_ref()
                            .map_err(Clone::clone)?,
                    ))
            {
                return Ok(true);
            }
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
    let Some(repo) = super::repo::open(root)? else {
        return Ok(Vec::new());
    };
    let start = normalized(start, true).map_err(|error| error.to_string())?;
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |path: PathBuf| {
        if !out.contains(&path) {
            out.push(path);
        }
    };
    let mut held: Vec<(PathBuf, bool)> = protected_paths(&repo)?
        .into_iter()
        .map(|(path, directory, _)| (path, directory))
        .collect();
    held.extend(checkout_roots(&repo)?.into_iter().map(|path| (path, false)));
    for (path, directory) in held {
        let path = normalized(&path, true).map_err(|error| error.to_string())?;
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
}

/// The registered checkout of the repository at `root` that a recursive
/// delete of `target` would remove: one equal to `target` or inside it,
/// the main working tree included. Read lexically and with symlinks
/// resolved, so `..` and a symlinked spelling name the same checkout.
pub(crate) fn registered_checkout_under(
    target: &Path,
    root: &Path,
    except: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    let Some(repo) = super::repo::open(root)? else {
        return Ok(None);
    };
    let checkouts = checkout_roots(&repo)?;
    let except: Vec<PathBuf> = except
        .into_iter()
        .flat_map(|path| [normalized(path, false), normalized(path, true)])
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    for resolve in [false, true] {
        let target = normalized(target, resolve).map_err(|e| e.to_string())?;
        for checkout in &checkouts {
            let path = normalized(checkout, resolve).map_err(|e| e.to_string())?;
            if path.starts_with(&target) && !except.contains(&path) {
                return Ok(Some(checkout.clone()));
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
    let Some(repo) = super::repo::open(root)? else {
        return Ok(false);
    };
    let protected = protected_paths(&repo)?;
    for resolve in [false, true] {
        let target = normalized(target, resolve).map_err(|e| e.to_string())?;
        for (path, _, _) in &protected {
            if normalized(path, resolve)
                .map_err(|e| e.to_string())?
                .starts_with(&target)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// The working tree of the checkout holding `dir`, when it is one.
pub(crate) fn checkout_root_of(dir: &Path) -> Option<PathBuf> {
    git2::Repository::discover(dir)
        .ok()?
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
        git2::Repository::init(f.temp.path().join("main")).unwrap();
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
        Some((dir, admin))
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
        assert!(!repository_authority_target(
            &admin.join("other"),
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
