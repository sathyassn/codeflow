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
    remote_head_advice: Option<String>,
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
    // OS text rule (issue 79, `docs/architecture.md`): kept strict. The remote
    // picks the policy source, so a remote name that is not valid UTF-8 stops
    // the read instead of dropping out of the list, where the policy would be
    // read from `HEAD` as if no remote existed.
    let names = super::ref_authority::utf8_remote_names(&remotes)?;
    let names: Vec<_> = names.iter().map(String::as_str).collect();
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
    let (defaults, fallback) = default_sources(&repo, remote)?;
    let source = &defaults[0];
    let (mut policy, mut project) = at(&repo, source)?;
    let branch = super::repo::current_branch(&repo);
    let mut targets = std::collections::BTreeSet::new();
    targets.extend(declared_target(&repo, remote, &branch, &policy, source)?);
    for source in defaults.iter().skip(1) {
        let (candidate, candidate_project) = at(&repo, source)?;
        // Resolve each candidate's declaration before merging levels: adding
        // another default candidate must not hide an existing stricter target.
        targets.extend(declared_target(&repo, remote, &branch, &candidate, source)?);
        policy = stricter(policy, &candidate)?;
        if project.is_some() && candidate_project.is_some() && project != candidate_project {
            return Err("fallback policy sources have conflicting project settings; the operator reconciles them or establishes remote HEAD".into());
        }
        project = project.or(candidate_project);
    }
    let mut sources = defaults
        .iter()
        .map(|source| {
            if fallback {
                format!("{source} (remote HEAD not set)")
            } else {
                source.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" + ");
    for target in targets {
        let target_ref = format!("refs/remotes/{remote}/{target}");
        if !defaults.contains(&target_ref) {
            let (line, line_project) = at(&repo, &target_ref)
                .map_err(|error| format!("{error}; run git fetch {remote}"))?;
            policy = stricter(policy, &line)?;
            project = line_project.or(project);
            let _ = write!(sources, " + {target_ref} (stricter policy levels)");
        }
    }
    let mut authority = finish(root, policy, project, sources);
    if fallback {
        authority.remote_head_advice = Some(format!(
            "operator advice: git remote set-head {remote} --auto; without remote HEAD the default is assumed to be main or master; custom defaults need set-head"
        ));
    }
    Ok(authority)
}

fn default_sources(repo: &Repository, remote: &str) -> Result<(Vec<String>, bool), String> {
    let head = format!("refs/remotes/{remote}/HEAD");
    let recovery = |error| {
        format!("cannot read policy source {head}: {error}; run git fetch {remote}; the operator can repair the default with git remote set-head {remote} --auto")
    };
    match repo.find_reference(&head) {
        Ok(reference) => {
            // An existing HEAD names authority even if its target needs fetching.
            let reference = reference.resolve().map_err(recovery)?;
            // OS text rule (issue 79, `docs/architecture.md`): kept strict. The
            // name picks which tracking branch supplies policy, an authority
            // decision, so a name that is not valid UTF-8 stops with the
            // recovery text and is never replaced by a lossy spelling.
            Ok((vec![reference.name().map_err(recovery)?.to_owned()], false))
        }
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            // Never let a local policy choose which tracking branch is trusted.
            let tried = [
                format!("refs/remotes/{remote}/main"),
                format!("refs/remotes/{remote}/master"),
            ];
            let mut sources = Vec::new();
            for name in &tried {
                match repo.find_reference(name) {
                    Ok(_) => sources.push(name.clone()),
                    Err(error) if error.code() == git2::ErrorCode::NotFound => {}
                    Err(error) => return Err(recovery(error)),
                }
            }
            if sources.is_empty() {
                return Err(format!("missing policy source {head}; tried {} and {}; run git fetch {remote}; for another default branch the operator runs git remote set-head {remote} --auto", tried[0], tried[1]));
            }
            Ok((sources, true))
        }
        Err(error) => Err(recovery(error)),
    }
}

fn working(root: &Path, source: &str) -> LandedPolicy {
    LandedPolicy {
        policy: Policy::load_effective(root).0,
        project: super::policy::read_project_toml(root),
        source: source.into(),
        local_differs: false,
        remote_head_advice: None,
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
        remote_head_advice: None,
    }
}

/// The `.codeflow/policy.json` text recorded at `rev`, read as git data
/// without touching the working copy; `None` when that tree has no policy.
/// `codeflow ci` judges a range with it, as the hosted job does from its
/// base checkout.
///
/// # Errors
/// The repository or `rev` cannot be read, or the file is not UTF-8.
pub fn policy_text_at(root: &Path, rev: &str) -> Result<Option<String>, String> {
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    read_at(&repo, rev, ".codeflow/policy.json")
}

fn read_at(repo: &Repository, reference: &str, path: &str) -> Result<Option<String>, String> {
    let tree = repo
        .revparse_single(reference)
        .and_then(|o| o.peel_to_tree())
        .map_err(|e| format!("cannot read policy source {reference}: {e}"))?;
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
}

fn at(repo: &Repository, reference: &str) -> Result<(Policy, Option<toml::Value>), String> {
    let read = |path: &str| read_at(repo, reference, path);
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
    default_source: &str,
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
        // A record path is valid text; a name that is not UTF-8 is never one.
        crate::git::walk_tree(repo, &tree, &mut |path, entry| {
            let Ok(text) = path.rule_text() else {
                return crate::git::Walk::Continue;
            };
            let Some((dir, file)) = text.rsplit_once('/') else {
                return crate::git::Walk::Continue;
            };
            let Some(id) = file.strip_suffix(".md") else {
                return crate::git::Walk::Continue;
            };
            if (dir == "project-management" || dir.starts_with("project-management/"))
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
            crate::git::Walk::Continue
        })
        .map_err(|e| e.to_string())?;
        if let Some(target) = found {
            // A default-branch record can name a missing target, which must fail closed.
            if name.ends_with(&format!("/{target}")) || name == default_source {
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
    let advice = authority
        .remote_head_advice
        .map_or_else(String::new, |advice| format!("; {advice}"));
    Ok(format!(
        "policy source: {}{residual}{drift}; refresh with git fetch{advice}",
        authority.source
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repository with one commit, the remote `origin` and `extra` bytes
    /// appended to its configuration.
    fn repository(extra: &[u8]) -> (tempfile::TempDir, git2::Oid) {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let sig = git2::Signature::now("Test", "test@example.invalid").unwrap();
        let commit = repo
            .commit(Some("HEAD"), &sig, &sig, "test: seed", &tree, &[])
            .unwrap();
        drop(tree);
        repo.remote("origin", "https://example.invalid/origin.git")
            .unwrap();
        let mut config = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join(".git").join("config"))
            .unwrap();
        config.write_all(extra).unwrap();
        (dir, commit)
    }

    /// Review finding on issue 79: a remote name that is not valid UTF-8 used
    /// to drop out of the list, so the policy was read from `HEAD` as if no
    /// remote existed. The read now stops and says why.
    #[test]
    fn a_remote_name_that_is_not_utf8_stops_the_policy_read() {
        let (dir, _) = repository(b"[remote \"caf\xe9\"]\n\turl = https://example.invalid/x.git\n");
        let error = load(dir.path()).err().expect("the read is refused");
        assert!(error.contains("not valid UTF-8"), "{error}");
    }

    /// Kept strict: the default branch of the remote picks the policy source,
    /// so a name that is not valid UTF-8 is a recovery error, never a lossy
    /// spelling of another ref.
    #[test]
    fn a_remote_head_that_names_a_ref_that_is_not_utf8_stops_the_policy_read() {
        let (dir, commit) = repository(b"");
        let git = dir.path().join(".git");
        std::fs::create_dir_all(git.join("refs").join("remotes").join("origin")).unwrap();
        let mut head = b"ref: refs/remotes/origin/caf\xe9\n".to_vec();
        std::fs::write(
            git.join("refs").join("remotes").join("origin").join("HEAD"),
            &head,
        )
        .unwrap();
        head.clear();
        let mut packed = b"# pack-refs with: peeled fully-peeled sorted \n".to_vec();
        packed.extend_from_slice(format!("{commit} refs/remotes/origin/").as_bytes());
        packed.extend_from_slice(b"caf\xe9\n");
        std::fs::write(git.join("packed-refs"), packed).unwrap();
        let error = load(dir.path()).err().expect("the read is refused");
        assert!(error.contains("cannot read policy source"), "{error}");
    }

    /// Issue 79: git2's `Tree::walk` stops with an error at a directory whose
    /// name is not valid UTF-8, so a landed record beside one was never found.
    #[test]
    fn a_landed_record_is_found_beside_a_directory_that_is_not_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, commit) = crate::git::repo_with_tree(
            dir.path(),
            &[
                (
                    b"project-management/tasks/TSK-007.md",
                    b"---\nid: TSK-007\nintegration_target: integration/x\n---\n",
                ),
                (b"src/dir\xff/f.rs", b"x"),
            ],
        );
        repo.reference("refs/remotes/origin/integration/x", commit, true, "test")
            .unwrap();
        let mut policy = Policy::default();
        policy.git.root_branch = String::new();
        let target = declared_target(
            &repo,
            "origin",
            "task/TSK-007-late",
            &policy,
            "refs/remotes/origin/main",
        )
        .unwrap();
        assert_eq!(target.as_deref(), Some("integration/x"));
    }
}
