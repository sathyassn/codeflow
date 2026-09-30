//! Agent policy authority comes from protected remote-tracking refs, not local edits.
use std::fmt::Write as _;
use std::path::Path;

use git2::Repository;
use serde_json::Value;

use super::{policy_schema, Policy, PolicyLevel};

/// The policy and project settings read at their recorded authority.
pub struct LandedPolicy {
    pub policy: Policy,
    pub project: Option<toml::Value>,
    pub source: String,
    pub local_differs: bool,
}

/// Read the authority without fetching or changing repository state.
///
/// # Errors
/// A populated tracking namespace must contain every required authority ref.
pub fn load(root: &Path) -> Result<LandedPolicy, String> {
    let Ok(repo) = Repository::discover(root) else {
        return Ok(working(root, "working copy (unborn HEAD; no remote)"));
    };
    let remotes = repo.remotes().map_err(|e| e.to_string())?;
    let names: Vec<_> = remotes.iter().flatten().flatten().collect();
    let remote = if names.contains(&"origin") {
        Some("origin")
    } else {
        match names.as_slice() {
            [] => None,
            [name] => Some(*name),
            _ => {
                return Err(
                    "policy source is ambiguous; the operator configures an origin remote".into(),
                )
            }
        }
    };
    let populated = if let Some(name) = remote {
        repo.references_glob(&format!("refs/remotes/{name}/*"))
            .map_err(|e| e.to_string())?
            .next()
            .transpose()
            .map_err(|e| e.to_string())?
            .is_some()
    } else {
        false
    };
    if !populated {
        let reason = remote.map_or_else(
            || "no remote".into(),
            |r| format!("before the first fetch of {r}"),
        );
        if repo.head().is_err_and(|e| {
            matches!(
                e.code(),
                git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
            )
        }) {
            return Ok(working(
                root,
                &format!("working copy (unborn HEAD; {reason})"),
            ));
        }
        let (policy, project) = at(&repo, "HEAD")?;
        return Ok(finish(root, policy, project, format!("HEAD ({reason})")));
    }
    let remote = remote.ok_or("populated tracking namespace has no configured remote")?;
    let default_ref = format!("refs/remotes/{remote}/HEAD");
    let default = repo.find_reference(&default_ref).and_then(|r| r.resolve())
        .map_err(|_| format!("missing policy source {default_ref}; run git fetch {remote}; if HEAD is still missing, the operator runs git remote set-head {remote} --auto"))?;
    let source = default.name().map_err(|e| e.to_string())?.to_owned();
    let (mut policy, mut project) = at(&repo, &source)?;
    let branch = super::repo::current_branch(&repo);
    let target = declared_target(&repo, remote, &branch, &policy)?;
    let mut sources = source.clone();
    if let Some(target) = target {
        let target_ref = format!("refs/remotes/{remote}/{target}");
        if target_ref != source {
            let (line, line_project) = at(&repo, &target_ref)
                .map_err(|error| format!("{error}; run git fetch {remote}"))?;
            policy = stricter(policy, &line)?;
            project = line_project.or(project);
            let _ = write!(sources, " + {target_ref} (stricter policy levels)");
        }
    }
    Ok(finish(root, policy, project, sources))
}

fn working(root: &Path, source: &str) -> LandedPolicy {
    LandedPolicy {
        policy: Policy::load_effective(root).0,
        project: super::policy::read_project_toml(root),
        source: source.into(),
        local_differs: false,
    }
}

fn finish(
    root: &Path,
    policy: Policy,
    project: Option<toml::Value>,
    source: String,
) -> LandedPolicy {
    let local_differs = serde_json::to_value(Policy::load(root)).ok()
        != serde_json::to_value(&policy).ok()
        || super::policy::read_project_toml(root) != project;
    LandedPolicy {
        policy,
        project,
        source,
        local_differs,
    }
}

fn at(repo: &Repository, reference: &str) -> Result<(Policy, Option<toml::Value>), String> {
    let tree = repo
        .revparse_single(reference)
        .and_then(|o| o.peel_to_tree())
        .map_err(|e| format!("cannot read policy source {reference}: {e}"))?;
    let read = |path: &str| -> Result<Option<String>, String> {
        match tree.get_path(Path::new(path)) {
            Ok(entry) => {
                let blob = repo.find_blob(entry.id()).map_err(|e| e.to_string())?;
                String::from_utf8(blob.content().to_vec())
                    .map(Some)
                    .map_err(|e| e.to_string())
            }
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    };
    let policy = read(".codeflow/policy.json")?
        .map_or_else(|| Ok(Policy::default()), |s| serde_json::from_str(&s))
        .map_err(|e| format!("invalid policy at {reference}: {e}"))?;
    let project = read(".codeflow/project.toml")?
        .map(|s| toml::from_str(&s))
        .transpose()
        .map_err(|e| format!("invalid project settings at {reference}: {e}"))?;
    Ok((policy, project))
}

fn declared_target(
    repo: &Repository,
    remote: &str,
    branch: &str,
    policy: &Policy,
) -> Result<Option<String>, String> {
    if !policy.git.root_branch.is_empty() {
        return Ok(Some(policy.git.root_branch.clone()));
    }
    let Some((prefix, suffix)) = branch.split_once('/') else {
        return Ok(None);
    };
    if !policy
        .git
        .branch_prefixes
        .iter()
        .any(|p| p.trim_end_matches('/') == prefix)
    {
        return Ok(None);
    }
    // Discover only landed records. The record must live on the branch it declares.
    let mut targets = std::collections::BTreeSet::new();
    for reference in repo
        .references_glob(&format!("refs/remotes/{remote}/*"))
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let Ok(name) = reference.name() else { continue };
        let Ok(tree) = reference.peel_to_tree() else {
            continue;
        };
        let mut found = None;
        tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
            let Some(id) = entry.name().ok().and_then(|n| n.strip_suffix(".md")) else {
                return git2::TreeWalkResult::Ok;
            };
            if dir.starts_with("project-management/")
                && suffix.starts_with(&format!("{id}-"))
                && id.starts_with("TSK-")
            {
                if let Ok(blob) = repo.find_blob(entry.id()) {
                    if let Ok(text) = std::str::from_utf8(blob.content()) {
                        if let Some(front) = text
                            .strip_prefix("---\n")
                            .and_then(|s| s.split_once("\n---").map(|(f, _)| f))
                        {
                            if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(front) {
                                found = value["integration_target"].as_str().map(str::to_owned);
                            }
                        }
                    }
                }
            }
            git2::TreeWalkResult::Ok
        })
        .map_err(|e| e.to_string())?;
        if let Some(target) = found {
            // A default-branch record can name a missing target, which must fail closed.
            if name.ends_with(&format!("/{target}"))
                || repo
                    .find_reference(&format!("refs/remotes/{remote}/HEAD"))
                    .and_then(|r| r.resolve())
                    .is_ok_and(|r| r.name().ok() == Some(name))
            {
                targets.insert(target);
            }
        }
    }
    if targets.len() > 1 {
        return Err(
            "landed task records disagree on the policy target; the operator reconciles them"
                .into(),
        );
    }
    Ok(targets.into_iter().next())
}

fn stricter(base: Policy, line: &Policy) -> Result<Policy, String> {
    let mut base = serde_json::to_value(base).map_err(|e| e.to_string())?;
    let line = serde_json::to_value(line).map_err(|e| e.to_string())?;
    for key in policy_schema::schema().iter().filter(|key| match key.kind {
        policy_schema::KeyKind::Level => true,
        policy_schema::KeyKind::Enum(values) => values
            .iter()
            .all(|v| ["block", "warn", "allow", "off"].contains(v)),
        _ => false,
    }) {
        let pointer = format!("/{}", key.path.replace('.', "/"));
        if let (Some(a), Some(b)) = (base.pointer_mut(&pointer), line.pointer(&pointer)) {
            let left =
                serde_json::from_value::<PolicyLevel>(a.clone()).map_err(|e| e.to_string())?;
            let right =
                serde_json::from_value::<PolicyLevel>(b.clone()).map_err(|e| e.to_string())?;
            if left.lower(right) == left {
                *a = b.clone();
            }
        }
    }
    if let Some(Value::Array(additions)) = line.pointer("/git/protected_branches") {
        if let Some(Value::Array(existing)) = base.pointer_mut("/git/protected_branches") {
            for item in additions {
                if !existing.contains(item) {
                    existing.push(item.clone());
                }
            }
        }
    }
    serde_json::from_value(base).map_err(|e| e.to_string())
}

/// Recovery command shared by diagnostics and installed hook contracts.
pub const INSTALL: &str = "curl -fsSL https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-cli-installer.sh | sh";

/// Compact authority diagnostic shared by doctor and orient.
///
/// # Errors
/// Returns the missing or unreadable policy authority and its recovery.
pub fn diagnostic(root: &Path) -> Result<String, String> {
    let authority = load(root)?;
    let residual = if authority.source.starts_with("HEAD")
        || authority.source.starts_with("working copy")
    {
        "; residual: local commits can change policy; only dangerous_commands has a binary floor"
    } else {
        ""
    };
    let drift = if authority.local_differs {
        "; local policy differs; inspect git diff HEAD -- .codeflow/policy.json .codeflow/project.toml and the named authority refs"
    } else {
        ""
    };
    Ok(format!(
        "policy source: {}{residual}{drift}; refresh with git fetch",
        authority.source
    ))
}
