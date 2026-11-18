//! Object-only path checks shared by staged changes and commit ranges.
use std::path::Path;

use git2::{Delta, Repository};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use super::{policy::GitPolicy, PolicyLevel, Violation};
use crate::workgraph::{
    acceptance::{read_merge, MergeReading},
    history,
};

/// The shipped secret-by-nature names. A policy can narrow a rule with an
/// exception but omitting a name does not remove its denial.
pub const DEFAULT_NAMES: &[&str] = &[
    ".env",
    ".env.*",
    "*.env",
    "*.pem",
    "*.key",
    "*.p12",
    "*.pfx",
    "*.jks",
    "*.keystore",
    "id_rsa*",
    "id_dsa*",
    "id_ecdsa*",
    "id_ed25519*",
    ".netrc",
    ".pgpass",
    "*.tfstate",
    "*.tfstate.*",
    "kubeconfig",
    ".kube/config",
    ".aws/credentials",
    "application_default_credentials.json",
    "credentials.json",
    ".credentials.json",
];

/// A final-component glob, or a sequence of component globs at any depth.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum BlockedName {
    Pattern(String),
    Rule(NameRule),
}

/// A named denial with root-anchored exceptions and their explanation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NameRule {
    pub pattern: String,
    pub except_under: Vec<String>,
    pub reason: String,
}

/// A root-anchored protected prefix with explained exceptions.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProtectedPath {
    pub prefix: String,
    pub allow: Vec<PathAllowance>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PathAllowance {
    pub path: String,
    pub reason: String,
}

/// Typed, normalized policy suitable for composition and matching.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathPolicy {
    pub level: PolicyLevel,
    pub names: Vec<NameRule>,
    pub protected: Vec<ProtectedPath>,
}

#[must_use]
pub fn default_names() -> Vec<BlockedName> {
    DEFAULT_NAMES
        .iter()
        .map(|s| BlockedName::Pattern((*s).into()))
        .collect()
}

/// One comparison spelling, independent of the host filesystem.
///
/// # Errors
/// Refuses ambiguous, non-UTF-8, or non-portable components.
pub fn normalize(bytes: &[u8]) -> Result<String, String> {
    let name = crate::git::GitName::from_bytes(bytes);
    let text = name.rule_text().map_err(|_| "path is not UTF-8")?;
    let text = text.nfc().collect::<String>().to_ascii_lowercase();
    if text.contains('\\')
        || text
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == ".." || c.ends_with(['.', ' ']))
    {
        return Err(
            "path has an empty, dot, parent, trailing dot/space, or backslash component".into(),
        );
    }
    Ok(text)
}

fn prefix(text: &str) -> Result<String, String> {
    if text == "." || text == "/" || text.is_empty() {
        return Ok(String::new());
    }
    normalize(text.strip_suffix('/').unwrap_or(text).as_bytes())
}

/// Root-anchored match on component boundaries; an empty prefix waives the rule.
#[must_use]
pub fn under(path: &str, prefix: &str) -> bool {
    prefix.is_empty()
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|s| s.starts_with('/'))
}

impl PathPolicy {
    /// Expand defaults and validate every declaration, including inactive policy.
    ///
    /// # Errors
    /// A pattern, prefix or exception is invalid or unexplained.
    pub fn from_git(git: &GitPolicy) -> Result<Self, String> {
        let mut names = Vec::new();
        for entry in &git.blocked_names {
            let mut rule = match entry {
                BlockedName::Pattern(pattern) => NameRule {
                    pattern: pattern.clone(),
                    except_under: vec![],
                    reason: String::new(),
                },
                BlockedName::Rule(rule) => {
                    if rule.reason.trim().is_empty() {
                        return Err("blocked_names exception needs a reason".into());
                    }
                    rule.clone()
                }
            };
            rule.pattern = normalize(rule.pattern.as_bytes())?;
            for component in rule.pattern.split('/') {
                glob::Pattern::new(component).map_err(|e| e.to_string())?;
            }
            rule.except_under = rule
                .except_under
                .iter()
                .map(|p| prefix(p))
                .collect::<Result<_, _>>()?;
            // Repeated declarations are concurrent constraints, not last-writer wins.
            names.push(rule);
        }
        for name in DEFAULT_NAMES {
            if !names.iter().any(|r| r.pattern == *name) {
                names.push(NameRule {
                    pattern: (*name).into(),
                    except_under: vec![],
                    reason: String::new(),
                });
            }
        }
        let mut protected = git.protected_paths.clone();
        for rule in &mut protected {
            rule.prefix = prefix(&rule.prefix)?;
            for allow in &mut rule.allow {
                if allow.reason.trim().is_empty() {
                    return Err("protected_paths allowance needs a reason".into());
                }
                allow.path = prefix(&allow.path)?;
            }
        }
        Ok(Self {
            level: git.path_policy,
            names,
            protected,
        })
    }

    fn name_hit(&self, path: &str) -> bool {
        self.names.iter().any(|rule| {
            pattern_matches(&rule.pattern, path)
                && !rule.except_under.iter().any(|p| under(path, p))
        })
    }

    fn protected_hit(&self, path: &str) -> bool {
        self.protected.iter().any(|rule| {
            under(path, &rule.prefix) && !rule.allow.iter().any(|a| under(path, &a.path))
        })
    }

    /// Root exceptions are whole-rule waivers, made visible by doctor.
    #[must_use]
    pub fn waivers(&self) -> Vec<String> {
        self.names
            .iter()
            .filter(|r| r.except_under.iter().any(String::is_empty))
            .map(|r| format!("blocked_names {}: {}", r.pattern, r.reason))
            .chain(self.protected.iter().flat_map(|r| {
                r.allow
                    .iter()
                    .filter(|a| a.path.is_empty())
                    .map(|a| format!("protected_paths {}: {}", r.prefix, a.reason))
            }))
            .collect()
    }
}

fn pattern_matches(pattern: &str, path: &str) -> bool {
    let components: Vec<_> = path.split('/').collect();
    let pattern: Vec<_> = pattern.split('/').collect();
    let matches = |parts: &[&str]| {
        pattern
            .iter()
            .zip(parts)
            .all(|(p, c)| glob::Pattern::new(p).is_ok_and(|p| p.matches(c)))
    };
    if pattern.len() == 1 {
        matches(&components[components.len() - 1..])
    } else {
        components.windows(pattern.len()).any(matches)
    }
}

/// The gate cannot prevent repair of its own inputs. Call after normalization.
#[must_use]
pub fn exempt(path: &str) -> bool {
    path.rsplit('/').next() == Some(".gitignore")
        || matches!(
            path,
            ".codeflow/policy.json"
                | ".codeflow/project.toml"
                | ".codeflow/bootstrap.sh"
                | ".codeflow/bootstrap.ps1"
                | ".codeflow/ci-setup.sh"
                | ".github/workflows/codeflow-policy.yml"
                | ".github/workflows/codeflow-ci.yml"
        )
        || path
            .strip_prefix(".codeflow/git-hooks/")
            .is_some_and(|s| !s.contains('/'))
}

#[derive(Debug, Default)]
pub struct Report {
    pub violations: Vec<Violation>,
    pub notes: Vec<String>,
}

impl Report {
    fn refuse(&mut self, policy: &PathPolicy, message: String) {
        self.violations.push(Violation::new(
            "git.path_policy",
            policy.level,
            message,
            crate::remedy::PATH_POLICY.remedy(),
        ));
    }
}

const LINK: u32 = 0o120_000;
const GITLINK: u32 = 0o160_000;

/// One entry a change introduces, as the index or a commit tree records it.
struct Entry<'a> {
    raw: &'a [u8],
    deleted: bool,
    mode: u32,
    id: git2::Oid,
}

fn check_entry(
    repo: &Repository,
    policy: &PathPolicy,
    content: Option<&ContentRules>,
    report: &mut Report,
    entry: &Entry<'_>,
) -> Result<(), String> {
    let shown = crate::git::GitName::from_bytes(entry.raw)
        .display()
        .to_string();
    let path = normalize(entry.raw);
    if policy.level.is_active() {
        check_path(repo, policy, report, &shown, path.as_deref(), entry)?;
    }
    if let Some(content) = content {
        content.check(repo, report, &shown, path.as_deref(), entry)?;
    }
    Ok(())
}

fn check_content(
    repo: &Repository,
    content: &ContentRules,
    report: &mut Report,
    entry: &Entry<'_>,
) -> Result<(), String> {
    let shown = crate::git::GitName::from_bytes(entry.raw)
        .display()
        .to_string();
    content.check(repo, report, &shown, normalize(entry.raw).as_deref(), entry)
}

/// The files a merge commit records that no parent holds at that path with
/// that content and mode: every blob the merge itself introduced, whether
/// a hand resolution or the automatic merge produced it.
fn merge_introduced(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    parents: &[git2::Oid],
) -> Result<std::collections::BTreeMap<Vec<u8>, (u32, git2::Oid)>, String> {
    let mut introduced: Option<std::collections::BTreeMap<Vec<u8>, (u32, git2::Oid)>> = None;
    for parent in parents {
        let parent = repo
            .find_commit(*parent)
            .and_then(|c| c.tree())
            .map_err(|e| e.to_string())?;
        let mut options = git2::DiffOptions::new();
        options.include_typechange(true);
        let diff = repo
            .diff_tree_to_tree(Some(&parent), Some(tree), Some(&mut options))
            .map_err(|e| e.to_string())?;
        let mut changed = std::collections::BTreeMap::new();
        for delta in diff.deltas().filter(|d| d.status() != Delta::Deleted) {
            let file = delta.new_file();
            let raw = file.path_bytes().ok_or("diff has no path")?.to_vec();
            changed.insert(raw, (u32::from(file.mode()), file.id()));
        }
        introduced = Some(match introduced {
            None => changed,
            Some(mut kept) => {
                kept.retain(|path, _| changed.contains_key(path));
                kept
            }
        });
    }
    Ok(introduced.unwrap_or_default())
}

fn check_path(
    repo: &Repository,
    policy: &PathPolicy,
    report: &mut Report,
    shown: &str,
    path: Result<&str, &String>,
    entry: &Entry<'_>,
) -> Result<(), String> {
    let Entry {
        deleted, mode, id, ..
    } = *entry;
    let path = match path {
        Ok(path) => path.to_string(),
        Err(error) => {
            report.refuse(policy, format!("{shown}: {error}"));
            return Ok(());
        }
    };
    // A gate input skips the name and prefix rules so the gate can repair its
    // own inputs, but only as a regular file: a link is still judged by its
    // target below, and any other entry type refuses.
    let input = exempt(&path);
    if input && !deleted && mode != LINK && !matches!(mode, 0o100_644 | 0o100_755 | 0o100_664) {
        let kind = if mode == GITLINK {
            "a gitlink".to_string()
        } else {
            format!("an entry of mode {mode:o}")
        };
        report.refuse(
            policy,
            format!("gate input {shown} is {kind}; it must be a regular file or a safe symlink"),
        );
        return Ok(());
    }
    if !input && policy.protected_hit(&path) {
        report.refuse(policy, format!("protected path changed: {shown}"));
    }
    if !input && policy.name_hit(&path) {
        if deleted {
            report.notes.push(format!(
                "deleted blocked name {shown}; its earlier content remains in history"
            ));
        } else {
            report.refuse(policy, format!("blocked name changed: {shown}"));
        }
    }
    if !deleted && mode == LINK {
        let blob = repo.find_blob(id).map_err(|e| {
            format!(
                "cannot read symlink {shown}: {}",
                crate::git::missing_contents::object_error(repo, entry.raw, id, &e)
            )
        })?;
        // "./docs" and "docs/" name the same path as "docs".
        let content = blob.content();
        let content = content.strip_prefix(b"./").unwrap_or(content);
        let target = normalize(content.strip_suffix(b"/").unwrap_or(content));
        // A letter then a colon is a Windows drive path, rooted ("C:/x") or
        // drive-relative ("C:x"); either can leave the tree.
        let drive =
            |t: &str| matches!(t.as_bytes(), [letter, b':', ..] if letter.is_ascii_alphabetic());
        match target {
            Ok(target) if !(target.starts_with('/') || drive(&target)) => {
                let resolved = path.rsplit_once('/').map_or_else(
                    || target.clone(),
                    |(parent, _)| format!("{parent}/{target}"),
                );
                if !exempt(&resolved)
                    && (policy.name_hit(&resolved) || policy.protected_hit(&resolved))
                {
                    report.refuse(
                        policy,
                        format!("symlink {shown} targets blocked path {resolved}"),
                    );
                }
            }
            _ => report.refuse(
                policy,
                format!("symlink {shown} has an absolute, parent, or invalid target"),
            ),
        }
    }
    Ok(())
}

fn check_diff(
    repo: &Repository,
    policy: &PathPolicy,
    content: Option<&ContentRules>,
    report: &mut Report,
    diff: &git2::Diff<'_>,
) -> Result<(), String> {
    for delta in diff.deltas() {
        let deleted = delta.status() == Delta::Deleted;
        let file = if deleted {
            delta.old_file()
        } else {
            delta.new_file()
        };
        let entry = Entry {
            raw: file.path_bytes().ok_or("diff has no path")?,
            deleted,
            mode: file.mode().into(),
            id: file.id(),
        };
        check_entry(repo, policy, content, report, &entry)?;
    }
    Ok(())
}

/// Judge the index Git will commit, including `GIT_INDEX_FILE`. Never follow links.
///
/// # Errors
/// Policy, HEAD, index or an introduced object cannot be read.
pub fn staged(repo: &Repository, policy: &PathPolicy) -> Result<Report, String> {
    staged_with(repo, policy, None)
}

/// [`staged`] with the size and ignored-file rules read at the authority.
///
/// # Errors
/// Policy, HEAD, index or an introduced object cannot be read.
pub fn staged_with(
    repo: &Repository,
    policy: &PathPolicy,
    content: Option<&ContentRules>,
) -> Result<Report, String> {
    let mut report = Report::default();
    if !policy.level.is_active() && content.is_none() {
        return Ok(report);
    }
    if let Some(content) = content {
        content.preamble(&mut report);
    }
    let index = git2::Index::open(&super::conflict_markers::effective_index(repo))
        .map_err(|e| e.to_string())?;
    let tree = match repo.head() {
        Ok(head) => Some(head.peel_to_tree().map_err(|e| e.to_string())?),
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => None,
        Err(e) => return Err(e.to_string()),
    };
    let mut options = git2::DiffOptions::new();
    options.include_typechange(true);
    let diff = repo
        .diff_tree_to_index(tree.as_ref(), Some(&index), Some(&mut options))
        .map_err(|e| e.to_string())?;
    check_diff(repo, policy, content, &mut report, &diff)?;
    Ok(report)
}

/// Judge each introduced commit, including changes later deleted. Merge paths
/// come from the shared remerge reader, never a first-parent approximation.
///
/// # Errors
/// Unreadable history, objects, or merges (including octopus merges) refuse.
pub fn range(
    root: &Path,
    base: Option<&str>,
    head: &str,
    policy: &PathPolicy,
) -> Result<Report, String> {
    range_with(root, base, head, policy, None)
}

/// [`range`] with the size and ignored-file rules read at the authority.
///
/// # Errors
/// Unreadable history, objects, or merges (including octopus merges) refuse.
pub fn range_with(
    root: &Path,
    base: Option<&str>,
    head: &str,
    policy: &PathPolicy,
    content: Option<&ContentRules>,
) -> Result<Report, String> {
    let mut report = Report::default();
    if !policy.level.is_active() && content.is_none() {
        return Ok(report);
    }
    if let Some(content) = content {
        content.preamble(&mut report);
    }
    let _judgment = history::judgment();
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    crate::git::missing_contents::check_range(&repo, base, head, &[])?;
    let oid = |rev: &str| {
        repo.revparse_single(rev)
            .and_then(|o| o.peel_to_commit())
            .map(|c| c.id())
            .map_err(|e| e.to_string())
    };
    let mut walk = history::revwalk(&repo).map_err(|e| e.to_string())?;
    walk.push(oid(head)?).map_err(|e| e.to_string())?;
    if let Some(base) = base {
        walk.hide(oid(base)?).map_err(|e| e.to_string())?;
    }
    for id in walk {
        let commit = repo
            .find_commit(id.map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let tree = commit.tree().map_err(|e| e.to_string())?;
        let parents = history::parents(&repo, &commit).map_err(|e| e.to_string())?;
        if parents.len() > 1 {
            let reading = read_merge(&repo, &commit);
            if let MergeReading::Unknown(error) = &reading {
                return Err(error.clone());
            }
            // Content is judged on every file the merge records that no
            // parent holds, which a clean automatic merge can produce too,
            // and on every path a hand resolution chose, even where it took
            // one parent's blob.
            let mut judged = if content.is_some() {
                merge_introduced(&repo, &tree, &parents)?
            } else {
                std::collections::BTreeMap::new()
            };
            if let MergeReading::Changed(paths) = reading {
                let mut index = git2::Index::new().map_err(|e| e.to_string())?;
                index.read_tree(&tree).map_err(|e| e.to_string())?;
                let entries: std::collections::BTreeMap<_, _> =
                    index.iter().map(|e| (e.path.clone(), e)).collect();
                for path in paths {
                    let found = entries.get(&path);
                    let entry = Entry {
                        raw: &path,
                        deleted: found.is_none(),
                        mode: found.map_or(0, |e| e.mode),
                        id: found.map_or(git2::Oid::ZERO_SHA1, |e| e.id),
                    };
                    if policy.level.is_active() {
                        check_entry(&repo, policy, None, &mut report, &entry)?;
                    }
                    if let Some(found) = found {
                        judged.insert(path.clone(), (found.mode, found.id));
                    }
                }
            }
            if let Some(content) = content {
                for (raw, (mode, id)) in &judged {
                    let entry = Entry {
                        raw,
                        deleted: false,
                        mode: *mode,
                        id: *id,
                    };
                    check_content(&repo, content, &mut report, &entry)?;
                }
            }
        } else {
            let parent = parents
                .first()
                .map(|id| repo.find_commit(*id).and_then(|c| c.tree()))
                .transpose()
                .map_err(|e| e.to_string())?;
            let mut options = git2::DiffOptions::new();
            options.include_typechange(true);
            let diff = repo
                .diff_tree_to_tree(parent.as_ref(), Some(&tree), Some(&mut options))
                .map_err(|e| e.to_string())?;
            check_diff(&repo, policy, content, &mut report, &diff)?;
        }
    }
    Ok(report)
}

/// A new or changed file larger than this is reported: 1 MiB.
pub const WARN_BYTES: u64 = 1 << 20;

/// The size limit when the authority's policy names none: 10 MiB.
pub const DEFAULT_LIMIT_BYTES: u64 = 10 << 20;

/// The project settings key a fresh `codeflow init` writes, so the default
/// size limit refuses in a new scaffold; `update` never writes it.
pub const SIZE_RULE_KEY: &str = "file_size_rule";

/// The project settings file, read at the authority for [`SIZE_RULE_KEY`].
const PROJECT_FILE: &str = ".codeflow/project.toml";

/// `git.ignored_files`: new or changed files the authority's `.gitignore`
/// files exclude. Absent, the level is `warn` and nothing is allowed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IgnoredFiles {
    #[serde(default = "warn_level")]
    pub level: PolicyLevel,
    #[serde(default)]
    pub allow: Vec<PathAllowance>,
}

fn warn_level() -> PolicyLevel {
    PolicyLevel::Warn
}

impl Default for IgnoredFiles {
    fn default() -> Self {
        Self {
            level: PolicyLevel::Warn,
            allow: Vec::new(),
        }
    }
}

impl IgnoredFiles {
    /// The allowed prefixes in the comparison spelling.
    ///
    /// # Errors
    /// An entry has no reason or an invalid path.
    pub fn allowed(&self) -> Result<Vec<String>, String> {
        self.allow
            .iter()
            .map(|a| {
                if a.reason.trim().is_empty() {
                    Err("ignored_files allowance needs a reason".to_string())
                } else {
                    prefix(&a.path)
                }
            })
            .collect()
    }
}

/// `git.max_file_bytes`: an explicit size limit. Either field opts the
/// project in to refusal and replaces that part of the default.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MaxFileBytes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub except_under: Option<Vec<String>>,
}

impl MaxFileBytes {
    /// The limit and the exempt prefixes in the comparison spelling.
    ///
    /// # Errors
    /// The limit is zero or a prefix is invalid.
    pub fn resolved(&self) -> Result<(u64, Vec<String>), String> {
        let limit = self.limit.unwrap_or(DEFAULT_LIMIT_BYTES);
        if limit == 0 {
            return Err("max_file_bytes limit must be at least 1".into());
        }
        let except = self
            .except_under
            .iter()
            .flatten()
            .map(|p| prefix(p))
            .collect::<Result<_, _>>()?;
        Ok((limit, except))
    }
}

/// The `.gitignore` files of one authority tree, by the directory that
/// holds each, in the comparison spelling ("" for the root).
#[derive(Default)]
struct IgnoreRules {
    dirs: std::collections::BTreeMap<String, Vec<ignore::gitignore::Gitignore>>,
}

impl IgnoreRules {
    fn read(repo: &Repository, commit: git2::Oid) -> Result<Self, String> {
        let mut rules = Self::default();
        let tree = repo
            .find_commit(commit)
            .and_then(|c| c.tree())
            .map_err(|e| format!("cannot read the authority tree {commit}: {e}"))?;
        let mut failure = None;
        crate::git::walk_tree(repo, &tree, &mut |name, entry| {
            let regular = matches!(entry.filemode(), 0o100_644 | 0o100_755 | 0o100_664);
            if regular && entry.name_bytes().eq_ignore_ascii_case(b".gitignore") {
                if let Err(error) = rules.add(repo, name, entry.id()) {
                    failure = Some(error);
                    return crate::git::Walk::Stop;
                }
            }
            crate::git::Walk::Continue
        })
        .map_err(|e| format!("cannot read the authority tree {commit}: {e}"))?;
        failure.map_or(Ok(rules), Err)
    }

    fn add(
        &mut self,
        repo: &Repository,
        name: &crate::git::GitName,
        id: git2::Oid,
    ) -> Result<(), String> {
        let shown = name.display().to_string();
        let path = normalize(name.bytes()).map_err(|e| format!("{shown}: {e}"))?;
        let dir = path.rsplit_once('/').map_or("", |(d, _)| d).to_string();
        let blob = repo.find_blob(id).map_err(|e| {
            format!(
                "cannot read {shown}: {}",
                crate::git::missing_contents::object_error(repo, name.bytes(), id, &e)
            )
        })?;
        let text =
            std::str::from_utf8(blob.content()).map_err(|_| format!("{shown} is not UTF-8"))?;
        // Git skips a byte order mark at the start of the file.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        // Matched case-insensitively, so a case variant of an excluded name
        // is excluded too. Paths are passed relative to `dir`.
        let mut builder = ignore::gitignore::GitignoreBuilder::new("");
        builder
            .case_insensitive(true)
            .map_err(|e| format!("{shown}: {e}"))?;
        // Lines end at a line feed; a carriage return before it is dropped.
        // Each line is composed (NFC) to match the paths it is compared with.
        for line in text.split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            builder
                .add_line(
                    Some(std::path::PathBuf::from(&shown)),
                    &line.nfc().collect::<String>(),
                )
                .map_err(|e| format!("{shown}: {e}"))?;
        }
        let matcher = builder.build().map_err(|e| format!("{shown}: {e}"))?;
        self.dirs.entry(dir).or_default().push(matcher);
        Ok(())
    }

    /// The nearest `.gitignore` with a line matching `components`, as git
    /// reads them: `Some(Ok(rule))` excludes, `Some(Err(()))` re-includes.
    /// Where two files at one level disagree (case variants of the name),
    /// exclusion wins.
    fn decide(&self, components: &[&str], is_dir: bool) -> Option<Result<String, ()>> {
        for depth in (0..components.len()).rev() {
            let Some(matchers) = self.dirs.get(&components[..depth].join("/")) else {
                continue;
            };
            let relative = components[depth..].join("/");
            let mut kept = false;
            for matcher in matchers {
                match matcher.matched(&relative, is_dir) {
                    ignore::Match::Ignore(glob) => {
                        let from = glob
                            .from()
                            .map_or_else(String::new, |f| format!(" in {}", f.display()));
                        return Some(Ok(format!("`{}`{from}", glob.original())));
                    }
                    ignore::Match::Whitelist(_) => kept = true,
                    ignore::Match::None => {}
                }
            }
            if kept {
                return Some(Err(()));
            }
        }
        None
    }

    /// The rule that excludes `path`, if any. A file beneath an excluded
    /// directory is excluded whatever a deeper `.gitignore` says, since git
    /// never reads one there.
    fn excludes(&self, path: &str) -> Option<String> {
        let components: Vec<&str> = path.split('/').collect();
        for end in 1..components.len() {
            if let Some(Ok(rule)) = self.decide(&components[..end], true) {
                return Some(rule);
            }
        }
        self.decide(&components, false).and_then(Result::ok)
    }
}

/// The size and ignored-file rules as the policy authority holds them: its
/// policy keys, its project kind and the `.gitignore` files of its trees.
/// Nothing is read from the working tree or the change being judged.
pub struct ContentRules {
    ignored_level: PolicyLevel,
    ignored_allow: Vec<String>,
    /// One rule set per authority tree; any tree's exclusion counts.
    ignore: Vec<Result<IgnoreRules, String>>,
    limit: u64,
    except_under: Vec<String>,
    refuse: bool,
}

impl ContentRules {
    /// Read the rules at the authority `commit`, whose policy is `git`.
    /// With no commit (an unborn repository), `project` is the project
    /// settings the authority holds and no ignore rule applies.
    ///
    /// # Errors
    /// A declaration is invalid, or the authority commit or its project
    /// settings cannot be read. An unreadable `.gitignore` is reported at
    /// the `git.ignored_files` level instead.
    pub fn at(
        repo: &Repository,
        git: &GitPolicy,
        commit: Option<git2::Oid>,
        project: Option<&toml::Value>,
    ) -> Result<Self, String> {
        let ignored = git.ignored_files.clone().unwrap_or_default();
        let (limit, except_under) = git.max_file_bytes.clone().unwrap_or_default().resolved()?;
        let refuse = git.max_file_bytes.is_some()
            || match commit {
                Some(commit) => new_scaffold(project_at(repo, commit)?.as_ref()),
                None => new_scaffold(project),
            };
        let ignore = match commit {
            Some(commit) if ignored.level.is_active() => vec![IgnoreRules::read(repo, commit)],
            _ => Vec::new(),
        };
        Ok(Self {
            ignored_level: ignored.level,
            ignored_allow: ignored.allowed()?,
            ignore,
            limit,
            except_under,
            refuse,
        })
    }

    /// The rules at the authority commit `rev`, with the policy that commit
    /// holds and the shipped defaults where it holds none, as `codeflow ci`
    /// reads them. Nothing is read from the working copy.
    ///
    /// # Errors
    /// The repository, `rev` or its policy cannot be read, the policy fails
    /// the schema, or as [`ContentRules::at`].
    pub fn at_authority(root: &Path, rev: &str) -> Result<Self, String> {
        let repo = Repository::discover(root).map_err(|e| e.to_string())?;
        let commit = Self::commit(&repo, rev)?;
        let git = super::landed_policy::checked_policy_at(&repo, commit)?.git;
        Self::at(&repo, &git, Some(commit), None)
    }

    fn commit(repo: &Repository, rev: &str) -> Result<git2::Oid, String> {
        repo.revparse_single(rev)
            .and_then(|o| o.peel_to_commit())
            .map(|c| c.id())
            .map_err(|e| {
                format!(
                    "cannot read the authority commit {}: {e}",
                    crate::git::display_text(rev)
                )
            })
    }

    /// Whether a file over the limit is refused, rather than reported.
    #[must_use]
    pub fn refuses(&self) -> bool {
        self.refuse
    }

    fn preamble(&self, report: &mut Report) {
        if !self.ignored_level.is_active() {
            return;
        }
        for error in self.ignore.iter().filter_map(|rules| rules.as_ref().err()) {
            report.violations.push(Violation::new(
                "git.ignored_files",
                self.ignored_level,
                format!("cannot read the authority's ignore rules: {error}"),
                crate::remedy::PATH_POLICY.remedy(),
            ));
        }
    }

    fn ignored(&self, report: &mut Report, shown: &str, path: Result<&str, &String>) {
        if !self.ignored_level.is_active() {
            return;
        }
        let finding = match path {
            Err(error) => Some(format!(
                "{shown}: {error}; the ignore rules cannot judge it"
            )),
            Ok(path) if !self.ignored_allow.iter().any(|a| under(path, a)) => self
                .ignore
                .iter()
                // An unreadable rule set is reported once, by the preamble.
                .filter_map(|rules| rules.as_ref().ok())
                .find_map(|rules| rules.excludes(path))
                .map(|rule| format!("ignored file changed: {shown} matches {rule}")),
            Ok(_) => None,
        };
        if let Some(message) = finding {
            report.violations.push(Violation::new(
                "git.ignored_files",
                self.ignored_level,
                message,
                crate::remedy::PATH_POLICY.remedy(),
            ));
        }
    }

    fn check(
        &self,
        repo: &Repository,
        report: &mut Report,
        shown: &str,
        path: Result<&str, &String>,
        entry: &Entry<'_>,
    ) -> Result<(), String> {
        // Gate inputs are never judged, so these rules cannot lock them. A
        // path that does not normalize is sized with no exception.
        if entry.deleted || entry.mode == GITLINK || path.is_ok_and(exempt) {
            return Ok(());
        }
        self.ignored(report, shown, path);
        // `except_under` names directories: a file beneath one is exempt,
        // and a prefix that is the file's own path exempts nothing.
        let beneath = |p: &str, e: &str| {
            e.is_empty() || p.strip_prefix(e).is_some_and(|s| s.starts_with('/'))
        };
        if path.is_ok_and(|p| self.except_under.iter().any(|e| beneath(p, e))) {
            return Ok(());
        }
        // The blob git records: for a Git LFS path, the pointer. An empty
        // blob need not be stored (`git add -N` names it), so it sizes as 0.
        let empty =
            git2::Oid::hash_object(git2::ObjectType::Blob, b"").map_err(|e| e.to_string())?;
        let size = if entry.id == empty {
            0
        } else {
            let (size, _) = repo
                .odb()
                .and_then(|odb| odb.read_header(entry.id))
                .map_err(|e| {
                    format!(
                        "cannot size {shown}: {}",
                        crate::git::missing_contents::object_error(repo, entry.raw, entry.id, &e)
                    )
                })?;
            u64::try_from(size).unwrap_or(u64::MAX)
        };
        let (level, text, routes) = if size > self.limit {
            let text = format!(
                "file too large: {shown} is {}, over the limit of {}",
                bytes(size),
                bytes(self.limit)
            );
            (PolicyLevel::Block, text, format!("; {ROUTES}"))
        } else if size > WARN_BYTES {
            let text = format!(
                "large file: {shown} is {}, over {}; keep large binaries in Git LFS or a release asset",
                bytes(size),
                bytes(WARN_BYTES)
            );
            (PolicyLevel::Warn, text, String::new())
        } else {
            return Ok(());
        };
        if self.refuse {
            report.violations.push(Violation::new(
                "git.max_file_bytes",
                level,
                format!("{text}{routes}"),
                crate::remedy::PATH_POLICY.remedy(),
            ));
        } else {
            report.notes.push(format!("{text}; {REPORTED}"));
        }
        Ok(())
    }
}

/// The admission routes a size refusal names (TSK-323, decision 8).
pub const ROUTES: &str = "admit it by tracking the path with Git LFS (the gate sizes the pointer git records), by a `limit` or `except_under` change to git.max_file_bytes in .codeflow/policy.json on the default branch through a reviewed pull request, or, from 4.2.0, by a per-path admitted row in the exemption register";

/// Why a size finding is only a report line in an updated project.
const REPORTED: &str = "reported only, since git.max_file_bytes is not set on the default branch; set it there to refuse such files";

fn bytes(size: u64) -> String {
    let tenths = size.saturating_mul(10) / (1 << 20);
    format!("{size} bytes ({}.{} MiB)", tenths / 10, tenths % 10)
}

/// Whether project settings mark a new scaffold: the size key holds any
/// value but `report`, so an unexpected value refuses.
fn new_scaffold(project: Option<&toml::Value>) -> bool {
    project
        .and_then(|p| p.get(SIZE_RULE_KEY))
        .is_some_and(|v| v.as_str() != Some("report"))
}

fn project_at(repo: &Repository, commit: git2::Oid) -> Result<Option<toml::Value>, String> {
    let tree = repo
        .find_commit(commit)
        .and_then(|c| c.tree())
        .map_err(|e| format!("cannot read the authority tree {commit}: {e}"))?;
    let entry = match crate::git::missing_contents::tree_entry(repo, &tree, Path::new(PROJECT_FILE))
    {
        Ok(Some(entry)) => entry,
        Ok(None) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let blob = repo.find_blob(entry.id()).map_err(|e| {
        crate::git::missing_contents::object_error(repo, PROJECT_FILE.as_bytes(), entry.id(), &e)
    })?;
    let text = std::str::from_utf8(blob.content())
        .map_err(|_| format!("project settings at {commit} are not UTF-8"))?;
    toml::from_str(text)
        .map(Some)
        .map_err(|e| format!("invalid project settings at {commit}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::super::landed_policy::{compose_paths, PathPolicySource};
    use super::*;
    use serde_json::json;

    fn policy(value: serde_json::Value) -> PathPolicy {
        PathPolicy::from_git(&serde_json::from_value(value).unwrap()).unwrap()
    }

    #[test]
    fn component_patterns_prefixes_and_unicode_share_one_comparison() {
        let p = policy(
            json!({"blocked_names": ["*.PEM", ".AWS/CREDENTIALS", "café.key",
            {"pattern":"*.fixture", "except_under":["Tests/Public/"], "reason":"public data"}],
            "protected_paths": [{"prefix":"Deploy/Private", "allow":[{"path":"deploy/private/examples", "reason":"public"}]}]}),
        );
        for (path, name, protected) in [
            ("foo/bar/CERT.PEM", true, false),
            ("foo/.aws/credentials", true, false),
            ("foo/.aws/credentials/child", true, false),
            ("foo/.aws/credentials-extra", false, false),
            ("CAfe\u{301}.KEY", true, false),
            ("tests/public/ok.fixture", false, false),
            ("other/tests/public/no.fixture", true, false),
            ("tests/publicity/no.fixture", true, false),
            ("Deploy/Private/x", false, true),
            ("deploy/private/examples/x", false, false),
            ("deep/deploy/private/x", false, false),
            ("deploy/privately/x", false, false),
        ] {
            let path = normalize(path.as_bytes()).unwrap();
            assert_eq!(p.name_hit(&path), name, "{path}");
            assert_eq!(p.protected_hit(&path), protected, "{path}");
        }
        let p = policy(json!({"blocked_names":["cafe\u{301}.fixture"]}));
        assert!(p.name_hit("café.fixture"));
        // The fold deliberately does not fold non-ASCII case.
        assert!(!p.name_hit("CAFÉ.fixture"));
    }

    #[test]
    fn ambiguous_components_refuse_without_lossy_conversion() {
        for path in [
            b"".as_slice(),
            b"a//b",
            b"./b",
            b"a/../b",
            b"a./b",
            b"a /b",
            b"a\\b",
            b"a/\xff",
            b"/a",
            b"a/",
        ] {
            assert!(normalize(path).is_err(), "{path:?}");
        }
    }

    #[test]
    fn exemptions_are_nfc_and_ascii_folded_and_root_anchored() {
        for path in [
            ".codeflow/policy.json",
            ".codeflow/project.toml",
            ".codeflow/bootstrap.sh",
            ".codeflow/bootstrap.ps1",
            ".codeflow/ci-setup.sh",
            ".codeflow/git-hooks/pre-commit",
            ".github/workflows/codeflow-policy.yml",
            ".github/workflows/codeflow-ci.yml",
            ".gitignore",
            "deep/.gitignore",
        ] {
            assert!(
                exempt(&normalize(path.to_uppercase().as_bytes()).unwrap()),
                "{path}"
            );
        }
        assert!(!exempt("deep/.codeflow/policy.json"));
        assert!(!exempt(".codeflow/git-hooks/deep/file"));
    }

    #[test]
    fn composition_unions_denials_intersects_exception_areas_and_keeps_removed_rules() {
        let a = policy(
            json!({"blocked_names":[{"pattern":"*.pem", "except_under":["fixtures"], "reason":"public"}, "target-only"],
            "protected_paths":[{"prefix":"private", "allow":[{"path":"private/examples", "reason":"public"}]}]}),
        );
        let b = policy(
            json!({"blocked_names":[{"pattern":"*.PEM", "except_under":["fixtures/narrow", "new"], "reason":"local"},
                {"pattern":"local-only", "except_under":["/"], "reason":"branch waiver"}],
            "protected_paths":[{"prefix":"private", "allow":[{"path":"private/examples/narrow", "reason":"narrow"}]}]}),
        );
        let merged = compose_paths(a, &b, PathPolicySource::WorkingCopy);
        for path in ["target-only", "local-only", "new/a.pem", "fixtures/a.pem"] {
            assert!(merged.name_hit(path), "{path}");
        }
        assert!(!merged.name_hit("fixtures/narrow/a.pem"));
        assert!(merged.protected_hit("private/examples/x"));
        assert!(!merged.protected_hit("private/examples/narrow/x"));
        let removed = compose_paths(
            merged,
            &policy(json!({"blocked_names":[],"protected_paths":[]})),
            PathPolicySource::WorkingCopy,
        );
        assert!(removed.name_hit("target-only"));
        assert!(removed.protected_hit("private/file"));
    }

    #[test]
    fn defaults_are_exact_and_target_exceptions_can_narrow_them() {
        assert_eq!(
            DEFAULT_NAMES,
            [
                ".env",
                ".env.*",
                "*.env",
                "*.pem",
                "*.key",
                "*.p12",
                "*.pfx",
                "*.jks",
                "*.keystore",
                "id_rsa*",
                "id_dsa*",
                "id_ecdsa*",
                "id_ed25519*",
                ".netrc",
                ".pgpass",
                "*.tfstate",
                "*.tfstate.*",
                "kubeconfig",
                ".kube/config",
                ".aws/credentials",
                "application_default_credentials.json",
                "credentials.json",
                ".credentials.json"
            ]
        );
        assert_eq!(GitPolicy::default().blocked_names, default_names());
        let p = policy(
            json!({"blocked_names":[{"pattern":"*.pem", "except_under":["fixtures"], "reason":"public certificate"}]}),
        );
        assert!(!p.name_hit("fixtures/certificate.pem"));
        assert!(p.name_hit("elsewhere/certificate.pem"));
        assert!(p.name_hit("deploy/id_rsa"));
    }

    #[test]
    fn root_waivers_are_named_and_malformed_rules_refuse() {
        let p = policy(
            json!({"blocked_names":[{"pattern":"*.pem", "except_under":["."], "reason":"public certificates"}],
            "protected_paths":[{"prefix":"private", "allow":[{"path":"/", "reason":"whole rule"}]}]}),
        );
        assert_eq!(p.waivers().len(), 2);
        assert!(!p.name_hit("a.pem"));
        assert!(!p.protected_hit("private/a"));
        for value in [
            json!({"blocked_names":["["]}),
            json!({"blocked_names":["../bad"]}),
            json!({"blocked_names":[{"pattern":"*.pem", "except_under":["x"], "reason":""}]}),
            json!({"protected_paths":[{"prefix":"a/../b", "allow":[]}]}),
            json!({"protected_paths":[{"prefix":"a", "allow":[{"path":"x", "reason":""}]}]}),
        ] {
            let git = serde_json::from_value(value).unwrap();
            assert!(PathPolicy::from_git(&git).is_err());
        }
    }

    /// All transport stays inside the test's temporary folder. The fetched
    /// branch is never checked out, so its promised blobs stay absent.
    struct PartialFixture {
        dir: tempfile::TempDir,
        repo: Repository,
        base: git2::Oid,
        head: git2::Oid,
        blobs: Vec<git2::Oid>,
    }

    impl PartialFixture {
        fn new(merge: bool) -> Self {
            Self::with_file(merge, "ordinary.txt", 0o100_644)
        }

        fn with_file(merge: bool, name: &str, mode: i32) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let output = crate::git::judging_command()
                .current_dir(dir.path())
                .args(["init", "--bare", "upstream.git"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let upstream = Repository::open_bare(dir.path().join("upstream.git")).unwrap();
            let mut config = upstream.config().unwrap();
            config.set_bool("uploadpack.allowFilter", true).unwrap();
            config
                .set_bool("uploadpack.allowAnySHA1InWant", true)
                .unwrap();
            let sig = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
            let empty = upstream.treebuilder(None).unwrap().write().unwrap();
            let commit = |tree, parents: &[git2::Oid]| {
                let tree = upstream.find_tree(tree).unwrap();
                let parents: Vec<_> = parents
                    .iter()
                    .map(|id| upstream.find_commit(*id).unwrap())
                    .collect();
                upstream
                    .commit(
                        None,
                        &sig,
                        &sig,
                        "fix: partial clone fixture",
                        &tree,
                        &parents.iter().collect::<Vec<_>>(),
                    )
                    .unwrap()
            };
            let base = commit(empty, &[]);
            let first = upstream.blob(b"ordinary content\n").unwrap();
            let second = upstream.blob(b"side content\n").unwrap();
            let mut entry = first;
            let mut entry_mode = mode;
            for component in name.rsplit('/') {
                let mut tree = upstream.treebuilder(None).unwrap();
                tree.insert(component, entry, entry_mode).unwrap();
                entry = tree.write().unwrap();
                entry_mode = 0o040_000;
            }
            let first_tree = upstream.find_tree(entry).unwrap();
            let mut tree = upstream.treebuilder(Some(&first_tree)).unwrap();
            let left = commit(first_tree.id(), &[base]);
            let head = if merge {
                let mut side = upstream.treebuilder(None).unwrap();
                side.insert("side.txt", second, 0o100_644).unwrap();
                let right = commit(side.write().unwrap(), &[base]);
                tree.insert("side.txt", second, 0o100_644).unwrap();
                commit(tree.write().unwrap(), &[left, right])
            } else {
                left
            };
            upstream
                .reference("refs/heads/fixture", head, true, "fixture")
                .unwrap();
            let repo = Repository::init(dir.path().join("clone")).unwrap();
            let url = format!("file://{}", upstream.path().display());
            repo.remote("origin", &url).unwrap();
            let mut config = repo.config().unwrap();
            config.set_bool("remote.origin.promisor", true).unwrap();
            config.set_str("extensions.partialClone", "origin").unwrap();
            let output = crate::git::judging_command()
                .current_dir(repo.workdir().unwrap())
                .env(
                    "GIT_CONFIG_GLOBAL",
                    if cfg!(windows) { "NUL" } else { "/dev/null" },
                )
                .env(
                    "GIT_CONFIG_SYSTEM",
                    if cfg!(windows) { "NUL" } else { "/dev/null" },
                )
                .args(["fetch", "--filter=blob:none", "origin", "fixture"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let blobs = if merge {
                vec![first, second]
            } else {
                vec![first]
            };
            for blob in &blobs {
                assert!(
                    repo.find_blob(*blob).is_err(),
                    "fixture unexpectedly fetched {blob}"
                );
            }
            Self {
                dir,
                repo,
                base,
                head,
                blobs,
            }
        }

        fn judge(&self) -> Result<Report, String> {
            let git = GitPolicy::default();
            let content = ContentRules::at(&self.repo, &git, Some(self.base), None).unwrap();
            range_with(
                self.repo.workdir().unwrap(),
                Some(&self.base.to_string()),
                &self.head.to_string(),
                &PathPolicy::from_git(&git).unwrap(),
                Some(&content),
            )
        }
    }

    #[test]
    fn partial_clone_range_names_missing_contents_and_fetch_command() {
        let mut failures = Vec::new();
        for merge in [false, true] {
            let fixture = PartialFixture::new(merge);
            let error = fixture.judge().unwrap_err();
            eprintln!("partial clone (merge={merge}): {error}");
            if !error.contains("partial clone")
                || !error.contains("ordinary.txt")
                || (merge && !error.contains("side.txt"))
            {
                failures.push(error.clone());
            }
            for blob in &fixture.blobs {
                if !error.contains(&blob.to_string()) {
                    failures.push(error.clone());
                }
            }
            if !error.contains("git cat-file") {
                failures.push(error.clone());
            } else {
                for command in error.split("git cat-file -e ").skip(1) {
                    let id: git2::Oid = command.get(..40).unwrap().parse().unwrap();
                    let output = crate::git::judging_command()
                        .current_dir(fixture.repo.workdir().unwrap())
                        .env("GIT_NO_LAZY_FETCH", "0")
                        .args(["cat-file", "-e", &id.to_string()])
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                assert!(fixture.judge().unwrap().violations.is_empty());
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn partial_clone_history_reader_never_fetches() {
        let fixture = PartialFixture::new(false);
        fixture
            .repo
            .config()
            .unwrap()
            .set_str(
                "remote.origin.url",
                &format!("file://{}/unreachable.git", fixture.dir.path().display()),
            )
            .unwrap();
        let trace = fixture.dir.path().join("trace");
        let output = history::command()
            .current_dir(fixture.repo.workdir().unwrap())
            .env("GIT_TRACE", &trace)
            .args([
                "diff-tree",
                "-p",
                "--no-ext-diff",
                "--no-textconv",
                &fixture.base.to_string(),
                &fixture.head.to_string(),
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let text = std::fs::read_to_string(trace).unwrap();
        eprintln!("offline history trace:\n{text}");
        assert!(
            !text.contains("fetch ") && !text.contains("upload-pack"),
            "{text}"
        );
    }

    #[test]
    fn full_clone_missing_blob_keeps_unreadable_refusal() {
        let fixture = PartialFixture::new(false);
        let source = Repository::open_bare(fixture.dir.path().join("upstream.git")).unwrap();
        let full_path = fixture.dir.path().join("full");
        let full = Repository::init(&full_path).unwrap();
        let source_objects = source.odb().unwrap();
        let full_objects = full.odb().unwrap();
        source_objects
            .foreach(|id| {
                let object = source_objects.read(*id).unwrap();
                full_objects.write(object.kind(), object.data()).unwrap();
                true
            })
            .unwrap();
        drop(full_objects);
        let id = fixture.blobs[0];
        let hex = id.to_string();
        let loose = full.path().join("objects").join(&hex[..2]).join(&hex[2..]);
        drop(full);
        std::fs::remove_file(loose).unwrap();
        let full = Repository::open(&full_path).unwrap();
        let native = full.odb().and_then(|odb| odb.read_header(id)).unwrap_err();
        let git = GitPolicy::default();
        let rules = ContentRules::at(&full, &git, Some(fixture.base), None).unwrap();
        let error = range_with(
            &full_path,
            Some(&fixture.base.to_string()),
            &fixture.head.to_string(),
            &PathPolicy::from_git(&git).unwrap(),
            Some(&rules),
        )
        .unwrap_err();
        assert_eq!(error, format!("cannot size ordinary.txt: {native}"));
        assert!(!error.contains("partial clone"));
    }

    #[test]
    fn partial_clone_unavailable_promisor_keeps_objects_unreadable() {
        let fixture = PartialFixture::new(false);
        fixture
            .repo
            .config()
            .unwrap()
            .set_str(
                "remote.origin.url",
                &format!("file://{}/unreachable.git", fixture.dir.path().display()),
            )
            .unwrap();
        let error = fixture.judge().unwrap_err();
        assert!(
            error.contains("cannot read") && error.contains("partial clone"),
            "{error}"
        );
        for id in &fixture.blobs {
            assert!(error.contains(&id.to_string()), "{error}");
            let output = crate::git::judging_command()
                .current_dir(fixture.repo.workdir().unwrap())
                .env("GIT_NO_LAZY_FETCH", "0")
                .args(["cat-file", "-e", &id.to_string()])
                .output()
                .unwrap();
            assert!(!output.status.success(), "offline retrieval must fail");
            assert!(fixture.judge().unwrap_err().contains(&id.to_string()));
        }
    }

    fn missing_policy_tree_refuses(cleared_partial: bool) {
        let fixture = PartialFixture::with_file(false, ".codeflow/policy.json", 0o100_644);
        let source = if cleared_partial {
            Repository::open(fixture.repo.workdir().unwrap()).unwrap()
        } else {
            Repository::open_bare(fixture.dir.path().join("upstream.git")).unwrap()
        };
        let tree = source.find_commit(fixture.head).unwrap().tree().unwrap();
        let missing = tree.get_name(".codeflow").unwrap().id();
        let copy = Repository::init(fixture.dir.path().join("copy")).unwrap();
        let objects = source.odb().unwrap();
        objects
            .foreach(|id| {
                if *id != missing {
                    let object = objects.read(*id).unwrap();
                    copy.odb()
                        .unwrap()
                        .write(object.kind(), object.data())
                        .unwrap();
                }
                true
            })
            .unwrap();
        let root = if cleared_partial {
            // Keep the real blob:none clone and its refs, but rewrite its
            // locally held objects as loose objects without the target tree.
            let root = fixture.repo.workdir().unwrap().to_path_buf();
            let object_dir = fixture.repo.path().join("objects");
            drop(objects);
            drop(tree);
            drop(source);
            drop(fixture.repo);
            std::fs::remove_dir_all(&object_dir).unwrap();
            std::fs::rename(copy.path().join("objects"), &object_dir).unwrap();
            let repo = Repository::open(&root).unwrap();
            let mut config = repo.config().unwrap();
            config.remove("extensions.partialclone").unwrap();
            config.remove("remote.origin.promisor").unwrap();
            root
        } else {
            copy.workdir().unwrap().to_path_buf()
        };
        drop(copy);
        let repo = Repository::open(&root).unwrap();
        assert!(repo.find_tree(missing).is_err());
        assert!(repo
            .config()
            .unwrap()
            .get_string("extensions.partialclone")
            .is_err());
        assert!(!repo
            .config()
            .unwrap()
            .get_bool("remote.origin.promisor")
            .unwrap_or(false));
        let result = super::super::landed_policy::checked_policy_at(&repo, fixture.head);
        match result {
            Err(error) => {
                eprintln!("cleared_partial={cleared_partial}, missing tree {missing}: {error}");
                assert!(error.contains(&missing.to_string()), "{error}");
            }
            Ok(_) => panic!("missing tree {missing} was treated as an absent policy (cleared_partial={cleared_partial})"),
        }
    }

    #[test]
    fn full_clone_missing_policy_tree_refuses() {
        missing_policy_tree_refuses(false);
    }

    #[test]
    fn cleared_partial_clone_missing_policy_tree_refuses() {
        missing_policy_tree_refuses(true);
    }

    #[test]
    fn partial_clone_object_readers_name_missing_paths() {
        for (reader, path, mode) in [
            ("size", "ordinary.txt", 0o100_644),
            ("symlink", "link", 0o120_000),
            ("ignore", ".gitignore", 0o100_644),
            ("project", ".codeflow/project.toml", 0o100_644),
            ("policy", ".codeflow/policy.json", 0o100_644),
            ("tracking", ".codeflow/project.toml", 0o100_644),
            ("record", "project-management/tasks/TSK-001.md", 0o100_644),
            ("release", ".codeflow/policy.json", 0o100_644),
        ] {
            let f = PartialFixture::with_file(false, path, mode);
            let root = f.repo.workdir().unwrap();
            let head = f.head.to_string();
            let id = f.blobs[0];
            let git = GitPolicy::default();
            let paths = PathPolicy::from_git(&git).unwrap();
            let mut report = Report::default();
            let entry = Entry {
                raw: path.as_bytes(),
                deleted: false,
                mode: u32::try_from(mode).unwrap(),
                id,
            };
            let error = match reader {
                "size" => ContentRules::at(&f.repo, &git, Some(f.base), None)
                    .unwrap()
                    .check(&f.repo, &mut report, path, Ok(path), &entry)
                    .unwrap_err(),
                "symlink" => {
                    check_path(&f.repo, &paths, &mut report, path, Ok(path), &entry).unwrap_err()
                }
                "ignore" => IgnoreRules::read(&f.repo, f.head).err().unwrap(),
                "project" => project_at(&f.repo, f.head).unwrap_err(),
                "policy" => super::super::landed_policy::policy_text_at(root, &head).unwrap_err(),
                "tracking" => {
                    crate::workgraph::durable_work_tracking_enabled_at(root, &head).unwrap_err()
                }
                "record" => crate::workgraph::declared_work_target_at_revision(
                    root,
                    &["task/".to_string()],
                    "task/TSK-001-fixture",
                    &head,
                )
                .unwrap_err()
                .to_string(),
                "release" => {
                    let destination = crate::workgraph::release_line::Destination {
                        default: Some(("main".into(), f.head)),
                        ..Default::default()
                    };
                    crate::workgraph::release_line::scope(
                        root,
                        &destination,
                        "feat/partial",
                        Some("main"),
                    )
                    .unwrap_err()
                }
                _ => unreachable!(),
            };
            assert!(error.contains("partial clone"), "{reader}: {error}");
            assert!(
                error.contains(path) && error.contains(&id.to_string()),
                "{reader}: {error}"
            );
            assert!(error.contains("git cat-file -e"), "{reader}: {error}");
        }
    }
}
