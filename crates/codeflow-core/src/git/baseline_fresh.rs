//! The fresh read of the policy authority that `codeflow policy baseline
//! init` and `refresh` record (TSK-1057). It asks the remote and fetches
//! objects with the caller's configured limits, so it lives outside the
//! guard code (`hooks/limits.rs` `guard_code_names_no_configured_wait`):
//! only the operator's command calls it, never a guard.
use std::path::Path;

use crate::hooks::landed_policy;
use crate::hooks::policy_baseline::{common_dir, configured_url, digest, Fresh};

/// Ask the authority remote for its default branch and commit, fetch that
/// commit's objects (no ref is written), require the branch tip the fetch
/// reports to be that commit, and read the policy from it. The
/// local tracking refs are never read for the policy: they only have to
/// agree on which branch is the default.
///
/// # Errors
/// The remote cannot be selected, verified, asked or fetched; its answer
/// cannot be read; the local remote `HEAD` names another branch; the fetched
/// tip is another commit; or the policy at the commit is invalid.
pub fn fresh(root: &Path, limits: super::remote_query::QueryLimits) -> Result<Fresh, String> {
    let Some(repo) = crate::hooks::repo::open(root)? else {
        return Err("this is not a repository with a commit".into());
    };
    let remote = landed_policy::authority_remote(&repo)?
        .ok_or("the repository has no remote, so there is no policy authority to record")?;
    let url = configured_url(&repo, &remote)?;
    super::remote_identity::verify(root, &remote, &url, limits)?;
    let answer = super::remote_query::ls_remote(root, &["--symref", &url, "HEAD"], limits)?;
    let (branch, commit) = parse_head(&answer)?;
    let authority = format!("refs/remotes/{remote}/{branch}");
    let local = landed_policy::remote_head_target(&repo, &remote)?;
    if local != authority {
        return Err(format!(
            "{remote} names {} as its default branch, and refs/remotes/{remote}/HEAD names {}; the operator runs git remote set-head {remote} -a and retries",
            super::display_text(&format!("refs/heads/{branch}")),
            super::display_text(&local)
        ));
    }
    let remote_ref = format!("refs/heads/{branch}");
    super::remote_query::fetch_objects(root, &url, &[&remote_ref], limits)?;
    let tip = super::remote_query::fetch_tip(root, &url, &remote_ref, limits)?;
    require_tip(&remote, &commit, &tip)?;
    let oid = git2::Oid::from_str(&commit).map_err(|e| e.to_string())?;
    if repo.find_commit(oid).is_err() {
        return Err(format!(
            "{remote} moved while it was read: commit {commit} did not arrive; retry"
        ));
    }
    landed_policy::checked_policy_at(&repo, oid)?;
    let policy_text = landed_policy::policy_text_at(root, &commit)?;
    Ok(Fresh {
        common_dir: common_dir(&repo)?,
        policy_digest: digest(policy_text.as_deref().unwrap_or("")),
        remote,
        remote_url: url,
        authority,
        commit,
        policy_text,
    })
}

/// The branch tip the fetch reported must be the commit the remote named as
/// its default: a policy is read only at a commit both answers agree on, so
/// a remote whose answers differ binds no record (TSK-1057).
fn require_tip(remote: &str, commit: &str, tip: &str) -> Result<(), String> {
    if tip.eq_ignore_ascii_case(commit) {
        Ok(())
    } else {
        Err(format!(
            "{remote} named commit {commit} as its default branch, and its fetch sent {tip}; nothing was recorded; retry"
        ))
    }
}

/// The default branch and its commit from `git ls-remote --symref <url> HEAD`.
fn parse_head(answer: &str) -> Result<(String, String), String> {
    let mut branch = None;
    let mut commit = None;
    for line in answer.lines() {
        if let Some(rest) = line.strip_prefix("ref: ") {
            if let Some((target, "HEAD")) = rest.split_once('\t') {
                branch = target.strip_prefix("refs/heads/").map(str::to_owned);
            }
        } else if let Some((sha, "HEAD")) = line.split_once('\t') {
            commit = Some(sha.to_owned());
        }
    }
    let unread = "the remote did not report its default branch as a branch and commit";
    let branch = branch.filter(|b| !b.is_empty()).ok_or(unread)?;
    let commit = commit
        .filter(|c| c.len() >= 40 && c.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or(unread)?;
    Ok((branch, commit))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use git2::{Oid, Repository, Signature};

    use super::*;

    const STRICT: &str = r#"{"schema_version": 1, "git": {"push_to_protected": "block", "protected_branches": ["main", "release"]}}"#;
    const WEAK: &str = r#"{"schema_version": 1, "git": {"push_to_protected": "warn", "protected_branches": ["main"]}}"#;

    /// A root commit whose tree holds `policy` at `.codeflow/policy.json`.
    fn commit(repo: &Repository, policy: &str) -> Oid {
        let blob = repo.blob(policy.as_bytes()).unwrap();
        let mut inner = repo.treebuilder(None).unwrap();
        inner.insert("policy.json", blob, 0o100_644).unwrap();
        let inner = inner.write().unwrap();
        let mut outer = repo.treebuilder(None).unwrap();
        outer.insert(".codeflow", inner, 0o040_000).unwrap();
        let tree = repo.find_tree(outer.write().unwrap()).unwrap();
        let sig = Signature::now("Test", "test@example.invalid").unwrap();
        repo.commit(None, &sig, &sig, "test: policy", &tree, &[])
            .unwrap()
    }

    /// A bare remote whose `main` holds the strict policy, and a clone whose
    /// tracking ref was weakened before the first run.
    fn poisoned_clone() -> (tempfile::TempDir, PathBuf, Oid) {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().canonicalize().unwrap();
        let remote = parent.join("remote.git");
        let bare = Repository::init_bare(&remote).unwrap();
        let strong = commit(&bare, STRICT);
        bare.reference("refs/heads/main", strong, true, "test")
            .unwrap();
        bare.set_head("refs/heads/main").unwrap();
        let url = format!("file://{}", remote.display());
        codeflow_fixture::clone(&parent, &url, "work")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .run();
        let work = parent.join("work");
        let repo = Repository::open(&work).unwrap();
        let weak = commit(&repo, WEAK);
        repo.reference("refs/remotes/origin/main", weak, true, "test: poison")
            .unwrap();
        (dir, work, strong)
    }

    /// Poisoned initialization: the fresh read asks the remote, so it yields
    /// the authority's strict policy, never the weakened local ref's.
    #[test]
    fn initialization_reads_the_remote_never_the_weakened_tracking_ref() {
        let (_dir, work, strong) = poisoned_clone();
        let fresh = fresh(&work, super::super::remote_query::QueryLimits::guard()).unwrap();
        assert_eq!(fresh.commit, strong.to_string());
        assert_eq!(fresh.policy_text.as_deref(), Some(STRICT));
        assert_eq!(fresh.authority, "refs/remotes/origin/main");
        // Nothing was written to the tracking ref.
        let repo = Repository::open(&work).unwrap();
        let tracked = repo.refname_to_id("refs/remotes/origin/main").unwrap();
        assert_ne!(tracked, strong);
    }

    /// A fetched tip that differs from the commit the remote named binds
    /// nothing; the porcelain answer is read for its one `FETCH_HEAD` line.
    #[test]
    fn a_fetched_tip_other_than_the_named_commit_is_refused() {
        let named = "a".repeat(40);
        let other = "b".repeat(40);
        let zero = "0".repeat(40);
        assert!(require_tip("origin", &named, &named).is_ok());
        assert!(require_tip("origin", &named, &named.to_uppercase()).is_ok());
        let refused = require_tip("origin", &named, &other).unwrap_err();
        assert!(refused.contains(&other), "{refused}");
        let fetched = |answer: &str| super::super::remote_query::fetched_tip(answer);
        assert_eq!(
            fetched(&format!("* {zero} {other} FETCH_HEAD\n")),
            Some(other.clone())
        );
        assert_eq!(fetched(""), None, "no line");
        assert_eq!(
            fetched(&format!(
                "* {zero} {named} FETCH_HEAD\n* {zero} {other} FETCH_HEAD\n"
            )),
            None,
            "two lines"
        );
        assert_eq!(fetched(&format!("* {zero} zz FETCH_HEAD\n")), None);
        assert_eq!(
            fetched(&format!("* {zero} {other} refs/remotes/origin/main\n")),
            None,
            "a tracking ref is not the fetch's answer"
        );
    }

    #[test]
    fn the_head_answer_is_read_strictly() {
        let sha = "a".repeat(40);
        let answer = format!("ref: refs/heads/main\tHEAD\n{sha}\tHEAD\n");
        assert_eq!(parse_head(&answer).unwrap(), ("main".into(), sha.clone()));
        assert!(parse_head(&format!("{sha}\tHEAD\n")).is_err());
        assert!(parse_head("ref: refs/tags/v1\tHEAD\nzz\tHEAD\n").is_err());
    }
}
