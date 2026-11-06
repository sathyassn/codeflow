//! The local policy authority: the default branch of the configured remote,
//! resolved to one commit (TSK-1077). No line, task record or working copy
//! supplies a policy, an exception or a level, and no state is guessed.
use std::path::Path;

use git2::Repository;

use super::{Policy, PolicyLevel};

/// The policy and project settings read at their recorded authority.
pub struct LandedPolicy {
    pub policy: Policy,
    pub project: Option<toml::Value>,
    /// Where the policy was read, as messages show it.
    pub source: String,
    /// The authority ref and the commit it resolved to; `None` for the
    /// labelled states that have no remote default branch (initialization
    /// and local only).
    pub authority: Option<Authority>,
    pub local_differs: bool,
    /// Whether the policy was read from the remote's default branch as last
    /// fetched, with plain ref files on the way. `false` in the labelled
    /// states and when the ref-file checks found a reason;
    /// `security.text_guard: relaxed` is honoured only when this is `true`
    /// (TSK-397). These refs are local state: a process that can write the
    /// repository's `.git` directory can set them, which TSK-1057 owns.
    pub remote_tracked: bool,
    /// Why the default-branch refs are not taken as the remote's state, when
    /// the ref-file checks found a reason (TSK-397 security review SEC01-2).
    pub tracking_doubt: Option<String>,
    /// The commit the policy and project settings were read at: the
    /// authority's, or `HEAD`'s in the labelled states; `None` when they
    /// came from the working copy. The size and ignored-file rules
    /// (TSK-323) read their project kind and ignore rules here.
    pub commit: Option<git2::Oid>,
}

/// The default branch the policy was read from, bound to one commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Authority {
    /// The fully qualified tracking ref, such as `refs/remotes/origin/main`.
    pub reference: String,
    /// The commit that ref named when it was read.
    pub sha: String,
}

/// Read the authority without changing repository state.
///
/// The authority is `refs/remotes/<remote>/HEAD`, which must name a
/// tracking branch of that remote; its commit is read once, and the policy
/// and project settings come from that commit's tree. An unset or
/// malformed remote `HEAD`, an empty tracking namespace for a remote that
/// has refs, and any read, parse or object error refuse with their remedy.
/// A proven absence of `.codeflow/policy.json` in that tree selects the
/// shipped defaults. Two states have no authority and are labelled: a
/// remote that advertises no refs (initialization) and no remote at all
/// (local only); both read `HEAD`, or the working copy before the first
/// commit. Telling a remote with refs from one with none is the only case
/// that asks the remote (`git ls-remote`, bounded and non-interactive).
///
/// # Errors
/// The authority cannot be resolved or read. The error is in display form
/// ([`crate::git::display_message`]): it can quote a ref name or a policy
/// value from the remote, and the guards, orient and doctor print it
/// (TSK-388).
pub fn load(root: &Path) -> Result<LandedPolicy, String> {
    load_raw(root).map_err(|error| crate::git::display_message(&error))
}

fn load_raw(root: &Path) -> Result<LandedPolicy, String> {
    let Some(repo) = super::repo::open(root)? else {
        return working(root, "working copy (unborn HEAD; local only: no remote)");
    };
    let remotes = repo.remotes().map_err(|e| e.to_string())?;
    // OS text rule (issue 79, `docs/architecture.md`): kept strict. The remote
    // picks the policy source, so a remote name that is not valid UTF-8 stops
    // the read instead of dropping out of the list, where the policy would be
    // read from `HEAD` as if no remote existed.
    let names = super::ref_authority::utf8_remote_names(&remotes)?;
    let names: Vec<_> = names.iter().map(String::as_str).collect();
    let remote = if names.contains(&"origin") {
        "origin"
    } else {
        match names.as_slice() {
            [] => return unestablished(&repo, root, "local only: no remote"),
            [name] => name,
            _ => {
                return Err(
                    "policy source is ambiguous; the operator configures an origin remote".into(),
                )
            }
        }
    };
    let populated = repo
        .references_glob(&format!("refs/remotes/{remote}/*"))
        .map_err(|e| e.to_string())?
        .next()
        .transpose()
        .map_err(|e| e.to_string())?
        .is_some();
    if !populated {
        // The local namespace cannot tell a remote that was never fetched,
        // or whose tracking refs were deleted, from one that holds nothing
        // yet, so the remote is asked. Only a remote that answers with no
        // refs is initialization; any other answer or a failure refuses.
        return match advertises_refs(&repo, root, remote) {
            Ok(false) => unestablished(
                &repo,
                root,
                &format!("initialization: {remote} advertises no refs"),
            ),
            Ok(true) => Err(format!(
                "cannot read the policy authority: {remote} has refs but refs/remotes/{remote}/ holds none here; run git fetch {remote}"
            )),
            Err(why) => Err(format!(
                "cannot read the policy authority: refs/remotes/{remote}/ holds no refs and {remote} could not be asked whether it has any ({why}); run git fetch {remote}"
            )),
        };
    }
    let authority = default_branch(&repo, remote)?;
    let oid = git2::Oid::from_str(&authority.sha).map_err(|e| e.to_string())?;
    // The remote names its default branch: the source is shown escaped.
    let shown = crate::git::display_text(&authority.reference);
    let (policy, project) = at_commit(&repo, oid, &shown)?;
    let mut landed = finish(root, policy, project, shown)?;
    landed.tracking_doubt = ref_file_doubt(&repo, remote, &authority);
    landed.remote_tracked = landed.tracking_doubt.is_none();
    landed.authority = Some(authority);
    landed.commit = Some(oid);
    Ok(landed)
}

/// Whether `remote` advertises any ref, asked at its one configured URL.
/// The URL comes from the repository's configuration, so the question runs
/// no configured command (see
/// [`crate::git::remote_query::advertises_any_ref`]); a remote with no URL,
/// several URLs, or one that is not UTF-8 is not asked.
fn advertises_refs(repo: &Repository, root: &Path, remote: &str) -> Result<bool, String> {
    let config = repo.config().map_err(|e| e.to_string())?;
    let key = format!("remote.{remote}.url");
    let mut urls = Vec::new();
    let mut entries = config
        .multivar(&key, None)
        .map_err(|e| format!("cannot read {key}: {e}"))?;
    while let Some(entry) = entries.next() {
        let entry = entry.map_err(|e| format!("cannot read {key}: {e}"))?;
        let url = entry
            .value()
            .map_err(|_| format!("{key} is not valid UTF-8"))?;
        urls.push(url.to_string());
    }
    match urls.as_slice() {
        [url] => crate::git::remote_query::advertises_any_ref(root, url),
        [] => Err(format!("{key} is not set")),
        _ => Err(format!("{key} has several values")),
    }
}

/// `refs/remotes/<remote>/HEAD`, which must name a tracking branch of the
/// remote, and the commit that branch names now.
fn default_branch(repo: &Repository, remote: &str) -> Result<Authority, String> {
    let head = format!("refs/remotes/{remote}/HEAD");
    let set_head = format!(
        "the operator runs git remote set-head {remote} -a (git fetch {remote} records it too, on git 2.48 or later); no other branch is taken as the default"
    );
    let reference = match repo.find_reference(&head) {
        Ok(reference) => reference,
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            return Err(format!(
                "the policy authority is unset: {head} does not exist, so the default branch of {remote} is unknown; {set_head}"
            ))
        }
        Err(error) => {
            return Err(format!(
                "cannot read policy source {head}: {error}; run git fetch {remote}"
            ))
        }
    };
    let not_branch = |what: &str| {
        format!("the policy authority {head} {what}, not a branch of {remote}; {set_head}")
    };
    let Some(target) = crate::git::name::symbolic_target(&reference) else {
        return Err(not_branch("names a commit directly"));
    };
    // OS text rule (issue 79, `docs/architecture.md`): kept strict. The name
    // picks which tracking branch supplies policy, an authority decision, so
    // a name that is not valid UTF-8 stops with the recovery text and is
    // never replaced by a lossy spelling.
    let target = target
        .rule_text()
        .map_err(|e| format!("cannot read policy source {head}: {e}; {set_head}"))?
        .to_owned();
    let prefix = format!("refs/remotes/{remote}/");
    if !target
        .strip_prefix(&prefix)
        .is_some_and(|branch| !branch.is_empty() && branch != "HEAD")
    {
        return Err(not_branch(&format!(
            "names {}",
            crate::git::display_text(&target)
        )));
    }
    let shown = crate::git::display_text(&target);
    let branch = repo.find_reference(&target).map_err(|error| {
        format!("cannot read policy source {shown}: {error}; run git fetch {remote}")
    })?;
    let Some(oid) = branch.target() else {
        return Err(not_branch(&format!(
            "names {shown}, which is itself symbolic"
        )));
    };
    let commit = repo.find_commit(oid).map_err(|error| {
        format!("cannot read policy source {shown} at {oid}: {error}; run git fetch {remote}")
    })?;
    Ok(Authority {
        reference: target,
        sha: commit.id().to_string(),
    })
}

/// Why the default branch's refs are not taken as the remote's state, or
/// `None` (TSK-397 security review SEC01-2). [`default_branch`] already
/// holds the remote `HEAD` to a direct tracking branch of the remote; this
/// adds that neither loose ref file, nor a directory on its way, is a
/// symbolic link. A packed ref is ordinary git state and is read as it is.
/// The checks narrow what a local edit can pass off as the remote's state;
/// they do not authenticate it: whoever can write `.git` can still forge a
/// plain ref (TSK-1057).
fn ref_file_doubt(repo: &Repository, remote: &str, authority: &Authority) -> Option<String> {
    let base = repo.commondir().to_path_buf();
    linked_ref_file(&base, &format!("refs/remotes/{remote}/HEAD"))
        .or_else(|| linked_ref_file(&base, &authority.reference))
}

/// A reason when the loose file for ref `name` under `base`, or a
/// directory on its way, is a symbolic link, or the file is not a regular
/// file. An absent path is a packed ref, which is not questioned.
fn linked_ref_file(base: &Path, name: &str) -> Option<String> {
    let mut path = base.to_path_buf();
    let parts: Vec<&str> = name.split('/').collect();
    for (index, part) in parts.iter().enumerate() {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Some(format!(
                    "the ref file for {} passes through a symbolic link at {}",
                    crate::git::display_text(name),
                    path.display()
                ))
            }
            Ok(meta) if index + 1 == parts.len() && !meta.is_file() => {
                return Some(format!(
                    "the ref file for {} is not a regular file",
                    crate::git::display_text(name)
                ))
            }
            Ok(_) => {}
            // A loose ref file that does not exist is a packed ref.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => {
                return Some(format!(
                    "the ref file for {} cannot be inspected: {error}",
                    crate::git::display_text(name)
                ))
            }
        }
    }
    None
}

/// A state with no remote default branch: `HEAD`'s policy, or the working
/// copy before the first commit, labelled with `reason`.
fn unestablished(repo: &Repository, root: &Path, reason: &str) -> Result<LandedPolicy, String> {
    match repo.head() {
        Ok(head) => {
            let oid = head
                .peel_to_commit()
                .map_err(|e| format!("cannot read policy source HEAD: {e}"))?
                .id();
            let (policy, project) = at_commit(repo, oid, "HEAD")?;
            let mut landed = finish(root, policy, project, format!("HEAD ({reason})"))?;
            landed.commit = Some(oid);
            Ok(landed)
        }
        Err(error)
            if matches!(
                error.code(),
                git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
            ) =>
        {
            working(root, &format!("working copy (unborn HEAD; {reason})"))
        }
        Err(error) => Err(format!("cannot read policy source HEAD: {error}")),
    }
}

fn working(root: &Path, source: &str) -> Result<LandedPolicy, String> {
    Ok(LandedPolicy {
        policy: Policy::load_effective(root)?.0,
        project: super::policy::read_project_toml(root)?,
        source: source.into(),
        authority: None,
        local_differs: false,
        remote_tracked: false,
        tracking_doubt: None,
        commit: None,
    })
}

fn finish(
    root: &Path,
    policy: Policy,
    project: Option<toml::Value>,
    source: String,
) -> Result<LandedPolicy, String> {
    let local_differs = !same_inputs(&Policy::load(root)?, &policy)?
        || super::policy::read_project_toml(root)? != project;
    Ok(LandedPolicy {
        policy,
        project,
        source,
        authority: None,
        local_differs,
        remote_tracked: false,
        tracking_doubt: None,
        commit: None,
    })
}

/// Whether two policies hold the same settings. Path keys compare by their
/// expanded rules, so shipped defaults written back by composition, rule
/// order, repeats and allowance reasons do not count as a difference.
fn same_inputs(local: &Policy, authority: &Policy) -> Result<bool, String> {
    type Rules = std::collections::BTreeSet<(String, std::collections::BTreeSet<String>)>;
    fn canonical(git: &super::policy::GitPolicy) -> Result<(PolicyLevel, Rules, Rules), String> {
        let paths = super::path_policy::PathPolicy::from_git(git)?;
        let names = paths
            .names
            .into_iter()
            .map(|r| (r.pattern, r.except_under.into_iter().collect()))
            .collect();
        let protected = paths
            .protected
            .into_iter()
            .map(|r| (r.prefix, r.allow.into_iter().map(|a| a.path).collect()))
            .collect();
        Ok((paths.level, names, protected))
    }
    let plain = |policy: &Policy| {
        let mut policy = policy.clone();
        policy.git.blocked_names.clear();
        policy.git.protected_paths.clear();
        serde_json::to_value(policy).map_err(|e| e.to_string())
    };
    Ok(plain(local)? == plain(authority)? && canonical(&local.git)? == canonical(&authority.git)?)
}

/// The `.codeflow/policy.json` text recorded at `rev`, read as git data
/// without touching the working copy; `None` only when that tree proves the
/// file absent. `codeflow ci` judges a range with it, as the hosted job does
/// from its base checkout.
///
/// # Errors
/// The repository or `rev` cannot be read, the path holds something other
/// than a regular file, or the file is not UTF-8.
pub fn policy_text_at(root: &Path, rev: &str) -> Result<Option<String>, String> {
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    let shown = crate::git::display_text(rev);
    let tree = repo
        .revparse_single(rev)
        .and_then(|o| o.peel_to_tree())
        .map_err(|e| format!("cannot read policy source {shown}: {e}"))?;
    file_in(&repo, &tree, ".codeflow/policy.json", &shown)
}

/// The policy an authority commit holds, checked against the schema before
/// it is read, or the shipped defaults when the commit has no policy file.
/// The size and ignored-file rules read their keys here, so a value the
/// schema refuses, such as `null` for a rule object, stops the check
/// instead of reading as an absent key.
///
/// # Errors
/// The commit or its policy cannot be read, or the policy is invalid.
pub fn checked_policy_at(repo: &Repository, commit: git2::Oid) -> Result<Policy, String> {
    let rev = commit.to_string();
    let short = &rev[..rev.len().min(9)];
    let tree = repo
        .find_commit(commit)
        .and_then(|commit| commit.tree())
        .map_err(|e| format!("cannot read policy source {short}: {e}"))?;
    let Some(text) = file_in(repo, &tree, ".codeflow/policy.json", short)? else {
        return Ok(Policy::default());
    };
    super::policy_schema::validate_policy_str(&text).map_err(|errors| {
        let shown: Vec<_> = errors.iter().map(ToString::to_string).collect();
        format!("invalid policy at {short}: {}", shown.join("; "))
    })?;
    serde_json::from_str(&text).map_err(|e| format!("invalid policy at {short}: {e}"))
}

/// The text of the regular file at `path` in `tree`: `None` when the tree
/// proves it absent; an error for a link, a gitlink, a directory, an
/// unreadable object or bytes that are not UTF-8.
fn file_in(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    path: &str,
    shown: &str,
) -> Result<Option<String>, String> {
    let entry = match tree.get_path(Path::new(path)) {
        Ok(entry) => entry,
        Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read policy source {shown}: {path}: {e}")),
    };
    if entry.kind() != Some(git2::ObjectType::Blob) || entry.filemode() == 0o120_000 {
        return Err(format!(
            "cannot read policy source {shown}: {path} is not a regular file"
        ));
    }
    // The size is read from the object header before the object is loaded,
    // so an oversized blob is never copied (TSK-397 security review SEC01-3).
    let (size, _) = repo
        .odb()
        .and_then(|odb| odb.read_header(entry.id()))
        .map_err(|e| format!("cannot read policy source {shown}: {path}: {e}"))?;
    if size as u64 > super::policy::SETTINGS_READ_LIMIT {
        return Err(format!(
            "cannot read policy source {shown}: {path} is larger than {} bytes",
            super::policy::SETTINGS_READ_LIMIT
        ));
    }
    let blob = repo
        .find_blob(entry.id())
        .map_err(|e| format!("cannot read policy source {shown}: {path}: {e}"))?;
    String::from_utf8(blob.content().to_vec())
        .map(Some)
        .map_err(|e| format!("cannot read policy source {shown}: {path}: {e}"))
}

/// The policy and project settings in the tree of commit `oid`; `shown`
/// names it in messages.
fn at_commit(
    repo: &Repository,
    oid: git2::Oid,
    shown: &str,
) -> Result<(Policy, Option<toml::Value>), String> {
    let tree = repo
        .find_commit(oid)
        .and_then(|commit| commit.tree())
        .map_err(|e| format!("cannot read policy source {shown}: {e}"))?;
    let policy = file_in(repo, &tree, ".codeflow/policy.json", shown)?
        .map_or_else(|| Ok(Policy::default()), |s| serde_json::from_str(&s))
        .map_err(|e| format!("invalid policy at {shown}: {e}"))?;
    let project = file_in(repo, &tree, ".codeflow/project.toml", shown)?
        .map(|s| toml::from_str(&s))
        .transpose()
        .map_err(|e| format!("invalid project settings at {shown}: {e}"))?;
    Ok((policy, project))
}

/// Recovery command shared by diagnostics and installed hook contracts.
pub const INSTALL: &str = "curl -fsSL https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-cli-installer.sh | sh";

/// Compact authority diagnostic shared by doctor and orient: the authority
/// ref and the commit it resolved to, or the labelled state.
///
/// # Errors
/// Returns the missing or unreadable policy authority and its recovery.
pub fn diagnostic(root: &Path) -> Result<String, String> {
    let authority = load(root)?;
    let at = authority
        .authority
        .as_ref()
        .map_or_else(String::new, |a| format!(" at {}", a.sha));
    let residual = if authority.authority.is_none() {
        "; residual: no remote default branch; local commits can change policy; only dangerous_commands has a binary floor"
    } else {
        ""
    };
    let drift = if authority.local_differs {
        "; local policy differs; inspect git diff HEAD -- .codeflow/policy.json .codeflow/project.toml and the named authority ref"
    } else {
        ""
    };
    let paths = compose_paths(
        super::path_policy::PathPolicy::from_git(&authority.policy.git)?,
        &super::path_policy::PathPolicy::from_git(&Policy::load(root)?.git)?,
        PathPolicySource::WorkingCopy,
    );
    let waivers = paths.waivers();
    let path_note = format!(
        "; path policy: denials union, exceptions intersect; {} whole-rule waivers{}",
        waivers.len(),
        if waivers.is_empty() {
            String::new()
        } else {
            format!(": {}", crate::git::display_message(&waivers.join("; ")))
        }
    );
    Ok(format!(
        "policy source: {}{at}{residual}{drift}{path_note}; refresh with git fetch",
        authority.source
    ))
}

/// Whether the second policy is another landed authority or an untrusted
/// working copy. Working copies may add denials but cannot grant
/// exceptions.
#[derive(Clone, Copy, Debug)]
pub enum PathPolicySource {
    Landed,
    WorkingCopy,
}

/// Union denials and intersect the areas their exceptions cover. A missing
/// declaration never removes an existing denial. Defaults are expanded before
/// composition; only landed policy can supply exceptions to a new rule.
#[must_use]
pub fn compose_paths(
    mut base: super::path_policy::PathPolicy,
    other: &super::path_policy::PathPolicy,
    source: PathPolicySource,
) -> super::path_policy::PathPolicy {
    use super::path_policy::{under, PathAllowance};
    if base.level.lower(other.level) == base.level {
        base.level = other.level;
    }
    for incoming in &other.names {
        let mut found = false;
        for rule in base
            .names
            .iter_mut()
            .filter(|r| r.pattern == incoming.pattern)
        {
            found = true;
            rule.except_under = rule
                .except_under
                .iter()
                .flat_map(|a| {
                    incoming.except_under.iter().filter_map(move |b| {
                        if under(a, b) {
                            Some(a.clone())
                        } else if under(b, a) {
                            Some(b.clone())
                        } else {
                            None
                        }
                    })
                })
                .collect();
        }
        if !found {
            let mut rule = incoming.clone();
            if !matches!(source, PathPolicySource::Landed) {
                rule.except_under.clear();
            }
            base.names.push(rule);
        }
    }
    for incoming in &other.protected {
        let mut found = false;
        for rule in base
            .protected
            .iter_mut()
            .filter(|r| r.prefix == incoming.prefix)
        {
            found = true;
            rule.allow = rule
                .allow
                .iter()
                .flat_map(|a| {
                    incoming.allow.iter().filter_map(move |b| {
                        let path = if under(&a.path, &b.path) {
                            &a.path
                        } else if under(&b.path, &a.path) {
                            &b.path
                        } else {
                            return None;
                        };
                        Some(PathAllowance {
                            path: path.clone(),
                            reason: format!("{}; {}", a.reason, b.reason),
                        })
                    })
                })
                .collect();
        }
        if !found {
            let mut rule = incoming.clone();
            if !matches!(source, PathPolicySource::Landed) {
                rule.allow.clear();
            }
            base.protected.push(rule);
        }
    }
    base
}

/// Read path authority for pre-commit, then retain any stricter local rules.
///
/// # Errors
/// Either policy source or a path declaration cannot be read.
pub fn staged_paths(
    root: &Path,
    local: &super::policy::GitPolicy,
) -> Result<super::path_policy::PathPolicy, String> {
    let landed = load(root)?;
    Ok(compose_paths(
        super::path_policy::PathPolicy::from_git(&landed.policy.git)?,
        &super::path_policy::PathPolicy::from_git(local)?,
        PathPolicySource::WorkingCopy,
    ))
}

/// [`staged_paths`] and the size and ignored-file rules from one read of
/// the authority: the policy, project kind and ignore rules of the one
/// commit the policy was read at. The content rules take nothing from the
/// working copy, a line or a guessed branch.
///
/// # Errors
/// Either policy source, a path declaration or the authority commit cannot
/// be read.
pub fn staged_rules(
    root: &Path,
    local: &super::policy::GitPolicy,
) -> Result<
    (
        super::path_policy::PathPolicy,
        super::path_policy::ContentRules,
    ),
    String,
> {
    let landed = load(root)?;
    let paths = compose_paths(
        super::path_policy::PathPolicy::from_git(&landed.policy.git)?,
        &super::path_policy::PathPolicy::from_git(local)?,
        PathPolicySource::WorkingCopy,
    );
    let Some(repo) = super::repo::open(root)? else {
        return Err("the repository cannot be opened".into());
    };
    let content = match landed.commit {
        Some(commit) => {
            let policy = checked_policy_at(&repo, commit)?;
            super::path_policy::ContentRules::at(&repo, &policy.git, Some(commit), None)?
        }
        // Before the first commit: the working copy's settings, the only
        // ones that exist, with no ignore rule (labelled local only or
        // initialization).
        None => super::path_policy::ContentRules::at(
            &repo,
            &landed.policy.git,
            None,
            landed.project.as_ref(),
        )?,
    };
    Ok((paths, content))
}

#[cfg(test)]
mod tests {
    #[test]
    fn r21_landed_policy_discovery_error_refuses() {
        let root = tempfile::tempdir().unwrap();
        assert!(super::load(root.path()).is_ok());
        git2::Repository::init(root.path()).unwrap();
        assert!(super::load(root.path()).is_ok());
        std::fs::write(root.path().join(".git/config"), b"[broken\n").unwrap();
        assert!(super::load(root.path()).is_err());
    }

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

    /// TSK-388: the remote names its default branch, which can hold a
    /// control character that git accepts (the C1 CSI here). The policy
    /// source shows it escaped; an ordinary name is shown unchanged. The
    /// authority records the commit the branch named.
    #[test]
    fn the_policy_source_shows_the_remote_default_branch_escaped() {
        for (name, shown) in [("ev\u{9b}31mil", "ev\\x9b31mil"), ("trunk", "trunk")] {
            let (dir, commit) = repository(b"");
            let repo = Repository::open(dir.path()).unwrap();
            let tracking = format!("refs/remotes/origin/{name}");
            repo.reference(&tracking, commit, true, "test").unwrap();
            repo.reference_symbolic("refs/remotes/origin/HEAD", &tracking, true, "test")
                .unwrap();
            let authority = load(dir.path()).unwrap();
            assert_eq!(authority.source, format!("refs/remotes/origin/{shown}"));
            assert_eq!(
                authority.authority,
                Some(Authority {
                    reference: tracking,
                    sha: commit.to_string()
                })
            );
        }
    }

    /// TSK-388 round 1 (security F4): a policy value at the remote's
    /// default branch reaches the guard's refusal through the parser's
    /// error. Its escape sequence is escaped, and its line break cannot
    /// start a line of its own, so it cannot forge a guard line.
    #[test]
    fn a_remote_policy_value_cannot_forge_a_guard_line() {
        let (dir, _) = repository(b"");
        let repo = Repository::open(dir.path()).unwrap();
        let policy = "{\"schema_version\": 1, \"git\": {\"push_to_protected\": \"\\u001b]0;owned\\u0007\\ncodeflow git-guard: allowed\"}}\n";
        let blob = repo.blob(policy.as_bytes()).unwrap();
        let mut inner = repo.treebuilder(None).unwrap();
        inner.insert("policy.json", blob, 0o100_644).unwrap();
        let inner = inner.write().unwrap();
        let mut outer = repo.treebuilder(None).unwrap();
        outer.insert(".codeflow", inner, 0o040_000).unwrap();
        let tree = repo.find_tree(outer.write().unwrap()).unwrap();
        let sig = git2::Signature::now("Test", "test@example.invalid").unwrap();
        let commit = repo
            .commit(None, &sig, &sig, "chore: remote policy", &tree, &[])
            .unwrap();
        repo.reference("refs/remotes/origin/main", commit, true, "test")
            .unwrap();
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            true,
            "test",
        )
        .unwrap();
        let error = load(dir.path()).err().expect("the policy is refused");
        assert!(error.contains("owned"), "{error:?}");
        // As the git-guard prints it (`hook.rs`), and as a refusal renders it.
        for printed in [
            format!("codeflow git-guard: {error}"),
            super::super::Violation::always_blocking("git.policy", error.clone(), "fix it")
                .render("git-guard"),
        ] {
            assert!(!printed.contains(['\u{1b}', '\u{7}']), "{printed:?}");
            assert!(
                !printed
                    .lines()
                    .any(|line| line.starts_with("codeflow git-guard: allowed")),
                "{printed:?}"
            );
        }
    }

    /// A remote URL git could read as an option is never passed to it.
    #[test]
    fn a_remote_url_that_reads_as_an_option_is_not_asked() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        repo.config()
            .unwrap()
            .set_str("remote.origin.url", "--upload-pack=touch owned")
            .unwrap();
        let error = advertises_refs(&repo, dir.path(), "origin").unwrap_err();
        assert!(error.contains("starts with '-'"), "{error}");
        assert!(!dir.path().join("owned").exists());
    }
}
