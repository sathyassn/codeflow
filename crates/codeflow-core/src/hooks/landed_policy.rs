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
    load_raw(root, true).map_err(|error| crate::git::display_message(&error))
}

/// [`load`] without asking the remote: where the remote's tracking refs are
/// missing, it refuses instead of asking whether the remote holds any. The
/// remote query limits read here (TSK-1301), so working out how long to wait
/// for a remote never waits for one first.
///
/// # Errors
/// As [`load`], and when the tracking namespace is empty.
pub fn load_local(root: &Path) -> Result<LandedPolicy, String> {
    load_raw(root, false).map_err(|error| crate::git::display_message(&error))
}

fn load_raw(root: &Path, ask: bool) -> Result<LandedPolicy, String> {
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
    // A remote name is configuration text and can be a URL with a
    // credential, so messages show it only when it is a plain name, and a
    // ref name only with the remote shown that way (TSK-416 review round 7).
    let shown = crate::security::git::shown_remote(root, remote);
    if !populated && !ask {
        return Err(format!(
            "refs/remotes/{shown}/ holds no refs, and the remote was not asked"
        ));
    }
    if !populated {
        // The local namespace cannot tell a remote that was never fetched,
        // or whose tracking refs were deleted, from one that holds nothing
        // yet, so the remote is asked. Only a remote that answers with no
        // refs is initialization; any other answer or a failure refuses.
        let fetch = fetch_step(root, remote);
        return match advertises_refs(&repo, root, remote) {
            Ok(false) => unestablished(
                &repo,
                root,
                &format!("initialization: {shown} advertises no refs"),
            ),
            Ok(true) => Err(format!(
                "cannot read the policy authority: {shown} has refs but refs/remotes/{shown}/ holds none here; {fetch}"
            )),
            Err(why) => Err(format!(
                "cannot read the policy authority: refs/remotes/{shown}/ holds no refs and {shown} could not be asked whether it has any ({why}); {fetch} (git waits on its own terms, outside the guard); once its refs are here the guards read the policy locally and ask {shown} nothing"
            )),
        };
    }
    let authority = default_branch(&repo, root, remote)?;
    let oid = git2::Oid::from_str(&authority.sha).map_err(|e| e.to_string())?;
    // The remote names its default branch: the source is shown escaped,
    // with the remote's name withheld unless it is plain.
    let shown = crate::security::git::shown_ref(root, &authority.reference);
    let (policy, project) = at_commit(&repo, oid, &shown)?;
    let mut landed = finish(root, policy, project, shown)?;
    landed.tracking_doubt = ref_file_doubt(&repo, root, remote, &authority);
    landed.remote_tracked = landed.tracking_doubt.is_none();
    landed.authority = Some(authority);
    landed.commit = Some(oid);
    Ok(landed)
}

/// Whether `remote` advertises any ref, asked at its one configured URL.
/// The URL comes from the repository's configuration, so the question runs
/// no configured command (see
/// [`crate::git::remote_query::advertises_any_ref`]); a remote with no URL,
/// several URLs, or one that is not UTF-8 is not asked. The session guards
/// ask it while the tracking namespace is empty, inside the harness hook
/// timeout, so each question runs within the guard's lookup wait
/// ([`QueryLimits::guard`](crate::git::remote_query::QueryLimits::guard)),
/// never a policy wait, and a remote that does not answer in time refuses
/// with the fetch that removes the need to ask (TSK-1301).
fn advertises_refs(repo: &Repository, root: &Path, remote: &str) -> Result<bool, String> {
    let config = repo.config().map_err(|e| e.to_string())?;
    let key = format!("remote.{remote}.url");
    // Messages show the key with its subsection withheld, as every guard
    // message does, and not libgit2's error, which can quote the key
    // (TSK-416 review round 6).
    let shown = crate::security::git::shown_config_key(&key);
    let mut urls = Vec::new();
    let mut entries = config
        .multivar(&key, None)
        .map_err(|_| format!("cannot read {shown}"))?;
    while let Some(entry) = entries.next() {
        let entry = entry.map_err(|_| format!("cannot read {shown}"))?;
        let url = entry
            .value()
            .map_err(|_| format!("{shown} is not valid UTF-8"))?;
        urls.push(url.to_string());
    }
    match urls.as_slice() {
        [url] => crate::git::remote_query::advertises_any_ref(
            root,
            url,
            crate::git::remote_query::QueryLimits::guard(),
        ),
        [] => Err(format!("{shown} is not set")),
        _ => Err(format!("{shown} has several values")),
    }
}

/// The pasted command for `remote`, rendered from its raw name so a name
/// holding `$(…)` or starting with `-` reaches git as the remote (TSK-416
/// review round 2); `None` when messages withhold the name
/// ([`crate::security::git::remote_name_shown`], review round 7).
fn remote_command(
    root: &Path,
    command: &'static str,
    remote: &str,
    flag: Option<&'static str>,
) -> Option<String> {
    if !crate::security::git::remote_name_shown(root, remote) {
        return None;
    }
    let pasted = crate::git::Pasted::new(command).value(remote);
    match flag {
        Some(flag) => pasted.trailing_flag(flag).render(),
        None => pasted.render(),
    }
}

/// "run git fetch <remote>"; for a remote whose name messages withhold,
/// "run git fetch --all", which fetches it without naming it (TSK-416
/// review round 7).
fn fetch_step(root: &Path, remote: &str) -> String {
    remote_command(root, "git fetch", remote, None).map_or_else(
        || "run git fetch --all, which fetches that remote without naming it".into(),
        |command| format!("run {command}"),
    )
}

/// `refs/remotes/<remote>/HEAD`, which must name a tracking branch of the
/// remote, and the commit that branch names now.
fn default_branch(repo: &Repository, root: &Path, remote: &str) -> Result<Authority, String> {
    let head = format!("refs/remotes/{remote}/HEAD");
    // Names in prose are shown escaped (TSK-388), with a remote name that is
    // not plain withheld, and libgit2's errors, which quote the ref, are not
    // shown (TSK-416 review round 7); each command is rendered from the raw
    // remote name (review round 2).
    let shown_remote = crate::security::git::shown_remote(root, remote);
    let shown_head = crate::security::git::shown_ref(root, &head);
    let fetch = fetch_step(root, remote);
    let set_head = match (
        remote_command(root, "git remote set-head", remote, Some("-a")),
        remote_command(root, "git fetch", remote, None),
    ) {
        (Some(set_head), Some(fetch)) => format!(
            "the operator runs {set_head} ({fetch} records it too, on git 2.48 or later); no other branch is taken as the default"
        ),
        _ => format!(
            "the operator runs git fetch --all, which records it on git 2.48 or later, or {}, then runs git remote set-head with that name and -a; no other branch is taken as the default",
            crate::security::git::WITHHELD_REMOTE_STEP
        ),
    };
    let reference = match repo.find_reference(&head) {
        Ok(reference) => reference,
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            return Err(format!(
                "the policy authority is unset: {shown_head} does not exist, so the default branch of {shown_remote} is unknown; {set_head}"
            ))
        }
        Err(error) => {
            return Err(format!(
                "cannot read policy source {shown_head}{}; {fetch}",
                git_error(root, remote, &error)
            ))
        }
    };
    let not_branch = |what: &str| {
        format!(
            "the policy authority {shown_head} {what}, not a branch of {shown_remote}; {set_head}"
        )
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
        .map_err(|_| {
            format!(
                "cannot read policy source {shown_head}: it names a ref that is not valid UTF-8; {set_head}"
            )
        })?
        .to_owned();
    let prefix = format!("refs/remotes/{remote}/");
    if !target
        .strip_prefix(&prefix)
        .is_some_and(|branch| !branch.is_empty() && branch != "HEAD")
    {
        return Err(not_branch(&format!(
            "names {}",
            crate::security::git::shown_ref(root, &target)
        )));
    }
    let shown = crate::security::git::shown_ref(root, &target);
    let branch = repo.find_reference(&target).map_err(|error| {
        format!(
            "cannot read policy source {shown}{}; {fetch}",
            git_error(root, remote, &error)
        )
    })?;
    let Some(oid) = branch.target() else {
        return Err(not_branch(&format!(
            "names {shown}, which is itself symbolic"
        )));
    };
    let commit = repo.find_commit(oid).map_err(|error| {
        format!(
            "cannot read policy source {shown} at {oid}{}; {fetch}",
            git_error(root, remote, &error)
        )
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
fn ref_file_doubt(
    repo: &Repository,
    root: &Path,
    remote: &str,
    authority: &Authority,
) -> Option<String> {
    let base = repo.commondir().to_path_buf();
    linked_ref_file(root, &base, &format!("refs/remotes/{remote}/HEAD"))
        .or_else(|| linked_ref_file(root, &base, &authority.reference))
}

/// A reason when the loose file for ref `name` under `base`, or a
/// directory on its way, is a symbolic link, or the file is not a regular
/// file. An absent path is a packed ref, which is not questioned. The ref,
/// and the path on its way, are shown as
/// [`crate::security::git::shown_ref`] shows a ref, since both hold the
/// remote's name (TSK-416 review round 7).
fn linked_ref_file(root: &Path, base: &Path, name: &str) -> Option<String> {
    let mut path = base.to_path_buf();
    let parts: Vec<&str> = name.split('/').collect();
    let shown = crate::security::git::shown_ref(root, name);
    for (index, part) in parts.iter().enumerate() {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Some(format!(
                    "the ref file for {shown} passes through a symbolic link at {} in {}",
                    crate::security::git::shown_ref(root, &parts[..=index].join("/")),
                    base.display()
                ))
            }
            Ok(meta) if index + 1 == parts.len() && !meta.is_file() => {
                return Some(format!("the ref file for {shown} is not a regular file"))
            }
            Ok(_) => {}
            // A loose ref file that does not exist is a packed ref.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => {
                return Some(format!(
                    "the ref file for {shown} cannot be inspected: {error}"
                ))
            }
        }
    }
    None
}

/// libgit2's error as ": <message>" when messages show the name of
/// `remote`; else nothing, since the message can quote a ref that holds
/// the name (TSK-416 review round 7).
fn git_error(root: &Path, remote: &str, error: &git2::Error) -> String {
    if crate::security::git::remote_name_shown(root, remote) {
        format!(": {}", crate::git::display_text(error.message()))
    } else {
        String::new()
    }
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
        repo.remote("origin", "file:///nonexistent/origin.git")
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

    /// TSK-416 review round 6: a remote with no URL or several is refused
    /// with the key's subsection, the remote's name, withheld. The
    /// credential is synthetic.
    #[test]
    fn a_remote_url_refusal_holds_no_remote_name() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let remote = "https://u:OPAQUECANARY@h.invalid";
        let key = format!("remote.{remote}.url");
        let unset = advertises_refs(&repo, dir.path(), remote).unwrap_err();
        let mut config = repo.config().unwrap();
        config.set_multivar(&key, "^$", "/one").unwrap();
        config.set_multivar(&key, "^$", "/two").unwrap();
        let several = advertises_refs(&repo, dir.path(), remote).unwrap_err();
        assert!(several.contains("has several values"), "{several}");
        for error in [unset, several] {
            assert!(error.contains("remote.<name>.url"), "{error}");
            assert!(!error.contains("OPAQUECANARY"), "{error}");
            assert!(!error.contains("h.invalid"), "{error}");
        }
    }

    /// TSK-416 review round 7: the sole remote's name is the subsection of
    /// its url key and can be a URL with a credential. Reading the policy
    /// authority names that remote by its place among the configured
    /// remotes in every message, its tracking refs and steps included,
    /// whether its refs are missing, its default branch is unset or the
    /// policy is read from it. The credentials are synthetic.
    #[test]
    fn the_policy_authority_never_prints_a_credential_remote_name() {
        let check = |text: &str, case: &str| {
            assert!(!text.contains("OPAQUECANARY"), "{case}: {text}");
            assert!(!text.contains("host.invalid"), "{case}: {text}");
            assert!(text.contains("<remote 1, name withheld>"), "{case}: {text}");
        };
        for name in [
            "https://user:OPAQUECANARY@host.invalid",
            "user-OPAQUECANARY@host.invalid",
        ] {
            let (dir, commit) = repository(
                format!("[remote \"{name}\"]\n\turl = file:///nonexistent/remote.git\n").as_bytes(),
            );
            let root = dir.path();
            let repo = Repository::open(root).unwrap();
            repo.remote_delete("origin").unwrap();
            check(
                &load_local(root).err().unwrap(),
                &format!("{name}: no refs, not asked"),
            );
            check(
                &load(root).err().unwrap(),
                &format!("{name}: no refs, asked"),
            );
            if name.contains(':') {
                // A ref name cannot hold `:`, so no tracking ref exists.
                continue;
            }
            let branch = format!("refs/remotes/{name}/main");
            repo.reference(&branch, commit, false, "fixture").unwrap();
            check(
                &load_local(root).err().unwrap(),
                &format!("{name}: default branch unset"),
            );
            repo.reference_symbolic(
                &format!("refs/remotes/{name}/HEAD"),
                &branch,
                false,
                "fixture",
            )
            .unwrap();
            let landed = load_local(root).unwrap();
            check(&landed.source, &format!("{name}: policy source"));
            assert_eq!(landed.source, "refs/remotes/<remote 1, name withheld>/main");
            // A symbolic link on the way to the ref file is a doubt that
            // names the ref the same way.
            #[cfg(unix)]
            {
                let common = repo.commondir().to_path_buf();
                let remote_dir = common.join("refs").join("remotes").join(name);
                let moved = common.join("moved-remote");
                std::fs::rename(&remote_dir, &moved).unwrap();
                std::os::unix::fs::symlink(&moved, &remote_dir).unwrap();
                let doubted = load_local(root).unwrap();
                let doubt = doubted.tracking_doubt.unwrap_or_default();
                check(&doubt, &format!("{name}: tracking doubt"));
            }
        }
    }

    /// The arguments `sh` hands `git` for the printed `command`.
    #[cfg(unix)]
    fn shell_argv(command: &str) -> Vec<String> {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("git() {{ printf '%s\\0' \"$@\"; }}; {command}"))
            .output()
            .unwrap();
        assert!(out.status.success(), "{command}");
        String::from_utf8(out.stdout)
            .unwrap()
            .split_terminator('\0')
            .map(str::to_string)
            .collect()
    }

    /// TSK-416 review round 2, finding 1: the sole remote's name reaches
    /// the printed `git fetch` and `git remote set-head` as one argument:
    /// pasting runs no substitution, and a name that starts with `-`
    /// follows `--`, with `-a` before it. For `-x`, git runs the printed
    /// steps in order and records the default branch. Since review round 7
    /// only a plain name is pasted: `o$(id)` is withheld, and its steps
    /// name no remote.
    #[test]
    #[cfg(unix)]
    fn the_policy_authority_steps_keep_a_hostile_remote_as_one_argument() {
        for (remote, fetch, set_head) in [
            ("o$(id)", vec!["fetch", "--all"], vec![]),
            (
                "-x",
                vec!["fetch", "--", "-x"],
                vec!["remote", "set-head", "-a", "--", "-x"],
            ),
        ] {
            let (dir, _) = repository(b"");
            let root = dir.path();
            let run = |args: &[&str]| {
                let out = crate::git::command()
                    .arg("-C")
                    .arg(root)
                    .args(args)
                    .output()
                    .unwrap();
                assert!(
                    out.status.success(),
                    "{args:?}: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            };
            run(&["remote", "remove", "origin"]);
            let source = root.to_str().unwrap();
            run(&["config", &format!("remote.{remote}.url"), source]);
            run(&[
                "config",
                &format!("remote.{remote}.fetch"),
                &format!("+refs/heads/*:refs/remotes/{remote}/*"),
            ]);
            let repo = Repository::open(root).unwrap();
            let error = default_branch(&repo, root, remote).unwrap_err();
            if set_head.is_empty() {
                assert!(!error.contains(remote), "{error}");
                assert!(error.contains("<remote 1, name withheld>"), "{error}");
                assert!(
                    error.contains("the operator runs git fetch --all"),
                    "{error}"
                );
                assert!(
                    error.contains(crate::security::git::WITHHELD_REMOTE_STEP),
                    "{error}"
                );
                let step = fetch_step(root, remote);
                assert!(!step.contains(remote), "{step}");
                let line = step
                    .strip_prefix("run ")
                    .and_then(|rest| rest.split(", which").next())
                    .unwrap_or_else(|| panic!("{step}"));
                assert_eq!(shell_argv(line), fetch, "{step}");
                continue;
            }
            let set_head_line = error
                .split("the operator runs ")
                .nth(1)
                .and_then(|rest| rest.split(" (").next())
                .unwrap_or_else(|| panic!("{error}"));
            let fetch_line = error
                .split(" (")
                .nth(1)
                .and_then(|rest| rest.split(" records it too").next())
                .unwrap_or_else(|| panic!("{error}"));
            assert_eq!(shell_argv(fetch_line), fetch, "{error}");
            assert_eq!(shell_argv(set_head_line), set_head, "{error}");
            assert_eq!(
                fetch_step(root, remote),
                format!("run {fetch_line}"),
                "the fetch step the other refusals print"
            );
            for line in [fetch_line, set_head_line] {
                let arguments = shell_argv(line);
                run(&arguments.iter().map(String::as_str).collect::<Vec<_>>());
            }
            let repo = Repository::open(root).unwrap();
            assert!(default_branch(&repo, root, remote).is_ok(), "{error}");
        }
    }
}
