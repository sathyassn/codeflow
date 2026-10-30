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

fn check_entry(
    repo: &Repository,
    policy: &PathPolicy,
    report: &mut Report,
    raw: &[u8],
    deleted: bool,
    mode: u32,
    id: git2::Oid,
) -> Result<(), String> {
    let shown = crate::git::GitName::from_bytes(raw).display().to_string();
    let path = match normalize(raw) {
        Ok(path) => path,
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
        let blob = repo
            .find_blob(id)
            .map_err(|e| format!("cannot read symlink {shown}: {e}"))?;
        // "./docs" and "docs/" name the same path as "docs".
        let content = blob.content();
        let content = content.strip_prefix(b"./").unwrap_or(content);
        let target = normalize(content.strip_suffix(b"/").unwrap_or(content));
        match target {
            Ok(target)
                if !(target.starts_with('/') || target.as_bytes().get(1..3) == Some(b":/")) =>
            {
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
        check_entry(
            repo,
            policy,
            report,
            file.path_bytes().ok_or("diff has no path")?,
            deleted,
            file.mode().into(),
            file.id(),
        )?;
    }
    Ok(())
}

/// Judge the index Git will commit, including `GIT_INDEX_FILE`. Never follow links.
///
/// # Errors
/// Policy, HEAD, index or an introduced object cannot be read.
pub fn staged(repo: &Repository, policy: &PathPolicy) -> Result<Report, String> {
    let mut report = Report::default();
    if !policy.level.is_active() {
        return Ok(report);
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
    check_diff(repo, policy, &mut report, &diff)?;
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
    let mut report = Report::default();
    if !policy.level.is_active() {
        return Ok(report);
    }
    let _judgment = history::judgment();
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
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
            match read_merge(&repo, &commit) {
                MergeReading::Clean => {}
                MergeReading::Unknown(error) => return Err(error),
                MergeReading::Changed(paths) => {
                    let mut index = git2::Index::new().map_err(|e| e.to_string())?;
                    index.read_tree(&tree).map_err(|e| e.to_string())?;
                    let entries: std::collections::BTreeMap<_, _> =
                        index.iter().map(|e| (e.path.clone(), e)).collect();
                    for path in paths {
                        let entry = entries.get(&path);
                        check_entry(
                            &repo,
                            policy,
                            &mut report,
                            &path,
                            entry.is_none(),
                            entry.map_or(0, |e| e.mode),
                            entry.map_or(git2::Oid::ZERO_SHA1, |e| e.id),
                        )?;
                    }
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
            check_diff(&repo, policy, &mut report, &diff)?;
        }
    }
    Ok(report)
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
                "credentials.json"
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
}
