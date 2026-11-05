//! Product work introduced by a line merge remains the receiving line's work.
use std::collections::BTreeSet;

use super::{
    finding, history, is_ancestor_or_same, owned_paths_except, Finding, Graph, Oid, Repository,
    BINDING_RULE,
};

/// The tip of a task branch's own target line, given only for the push of
/// that task branch (TSK-1149). A merge whose other parents are all already
/// on this tip is the branch's own sync of its line: its hand-resolved
/// hunks are the task's unreviewed work, which `codeflow ci` judges at
/// ready and at landing, not on a push for durability.
#[derive(Clone, Copy, Debug)]
pub(super) struct LineSyncTip(pub(super) Oid);

/// A bound task review, with a base only when the judged branch carries
/// that same task. Otherwise the receiving first-parent chain bounds its work.
#[derive(Clone, Copy, Debug)]
pub(super) struct ReviewedTaskHead {
    pub(super) head: Oid,
    pub(super) branch_base: Option<Oid>,
    /// For an archived completion, the commit that made it. It credits only
    /// where it lies on the task's own chain (see [`made_on_own_chain`]).
    pub(super) made_at: Option<Oid>,
}

pub(super) fn findings(
    repo: &Repository,
    base: Oid,
    head: Oid,
    line: Option<&str>,
    graph: &Graph,
    reviewed: &[ReviewedTaskHead],
    sync_tip: Option<LineSyncTip>,
) -> Result<Vec<Finding>, String> {
    let Some(line) = line.filter(|line| super::super::line_adoption::epic_id(line).is_ok()) else {
        return Ok(Vec::new());
    };
    let root = repo.workdir().unwrap_or_else(|| repo.path());
    let mut walk = history::revwalk(repo).map_err(|error| error.to_string())?;
    walk.push(head)
        .and_then(|()| walk.hide(base))
        .and_then(|()| walk.simplify_first_parent())
        .map_err(|error| error.to_string())?;
    let mut merges = Vec::new();
    let mut receiving_commits = BTreeSet::new();
    for id in walk {
        let commit = repo
            .find_commit(id.map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        receiving_commits.insert(commit.id());
        let parents = history::parents(repo, &commit).map_err(|error| error.to_string())?;
        if parents.len() > 1 {
            merges.push((commit.id(), parents));
        }
    }
    if merges.is_empty() {
        return Ok(Vec::new());
    }
    let default = super::default_target_name(repo)?;
    let target = super::imports::home_tip(repo, &default.name)?.ok_or_else(|| {
        format!(
            "published home `{}` is unavailable; fetch its full history",
            default.name
        )
    })?;
    let mut homes = BTreeSet::from([target]);
    let mut checked = BTreeSet::new();
    for task in graph.records.values() {
        let Some(home) = task.integration_target.as_deref() else {
            continue;
        };
        let home = super::super::work_start::logical_target(home);
        if home == line || !home.starts_with("integration/") || !checked.insert(home) {
            continue;
        }
        if let Some(tip) = super::imports::home_tip(repo, home)? {
            if super::super::work_start::check_epic_line(
                root,
                home,
                &default.name,
                &target.to_string(),
                &tip.to_string(),
            )
            .is_ok()
            {
                homes.insert(tip);
            }
        }
    }
    let reviewed_own = reviewed_own_commits(repo, base, head, &receiving_commits, reviewed)?;
    let project = super::super::amendment::project_at(repo, base)?;
    let mut found = Vec::new();
    for (merge, parents) in merges {
        let first = parents[0];
        if let Some(LineSyncTip(tip)) = sync_tip {
            if line_sync(repo, &parents, tip)? {
                continue;
            }
        }
        // The range's base is already the receiving line's work: a task
        // branch that re-merges its target line brings nothing new from it.
        let mut hide = vec![first.to_string(), base.to_string()];
        for home in &homes {
            if let Some(source) = source_before(repo, *home, merge)? {
                hide.push(source.to_string());
            }
        }
        let refs: Vec<_> = hide.iter().map(String::as_str).collect();
        let paths = owned_paths_except(root, &merge.to_string(), &refs, &reviewed_own)
            .map_err(|reason| format!("cannot read receiving merge {merge}: {reason}"))?;
        for path in paths {
            if super::super::classify::path_sets().adopter_facing_any_case(&path, &project) {
                found.push(finding(BINDING_RULE, format!("receiving line `{line}` introduces {path} through merge {merge} outside its verified source and reviewed task heads; carry that product work through the receiving task's pull request and review the result before landing")));
            }
        }
    }
    Ok(found)
}

/// Walk backward from each reviewed head through its first parents, stopping
/// before the receiving first-parent chain or any ancestor of the run's base.
/// Task commits previously landed through a side parent retain their credit.
/// When judging the matching unlanded task branch, stop at its supplied base
/// instead: the judged first-parent chain belongs to that task itself.
fn reviewed_own_commits(
    repo: &Repository,
    base: Oid,
    head: Oid,
    receiving_commits: &BTreeSet<Oid>,
    reviewed: &[ReviewedTaskHead],
) -> Result<BTreeSet<Oid>, String> {
    let mut own = BTreeSet::new();
    for task in reviewed {
        if let Some(at) = task.made_at {
            if !made_on_own_chain(
                repo,
                base,
                at,
                task.branch_base.is_some(),
                receiving_commits,
            )? {
                continue;
            }
        }
        let reviewed = task.head;
        if !is_ancestor_or_same(repo, reviewed, head)? {
            return Err(format!(
                "reviewed commit {reviewed} is not in the judged head {head}"
            ));
        }
        let branch_base = task.branch_base.unwrap_or(base);
        let mut at = reviewed;
        while !(task.branch_base.is_none() && receiving_commits.contains(&at)
            || is_ancestor_or_same(repo, at, branch_base)?)
        {
            let commit = repo
                .find_commit(at)
                .map_err(|error| format!("cannot read reviewed task work at {at}: {error}"))?;
            let parents = history::parents(repo, &commit)
                .map_err(|error| format!("cannot read reviewed task work at {at}: {error}"))?;
            match parents.as_slice() {
                [_] => {}
                [_, _] => {
                    if let super::MergeReading::Unknown(reason) = super::read_merge(repo, &commit) {
                        return Err(format!("cannot prove reviewed task work at {at}: {reason}"));
                    }
                }
                _ => return Err(format!("commit {at} has {} parents; cannot prove the task's own work for reviewed commit {reviewed}", parents.len())),
            }
            own.insert(at);
            at = parents[0];
        }
    }
    Ok(own)
}

/// Whether an archived completion made at `at` was made on a task's own
/// chain: on the judged first-parent chain itself, or, when the judged
/// branch is a receiving line (`on_task_branch` false), on the first-parent
/// chain a receiving merge landed. A completion that only arrived through
/// a task branch's own merge, such as one made on another task's tip and
/// merged in, is not on its chain and credits nothing.
fn made_on_own_chain(
    repo: &Repository,
    base: Oid,
    at: Oid,
    on_task_branch: bool,
    receiving_commits: &BTreeSet<Oid>,
) -> Result<bool, String> {
    if receiving_commits.contains(&at) {
        return Ok(true);
    }
    if on_task_branch {
        return Ok(false);
    }
    let unreadable = |error: git2::Error| format!("cannot read the chain of {at}: {error}");
    for merge in receiving_commits {
        let commit = repo.find_commit(*merge).map_err(unreadable)?;
        let parents = history::parents(repo, &commit).map_err(unreadable)?;
        for side in parents.iter().skip(1) {
            let mut step = *side;
            while !receiving_commits.contains(&step) && !is_ancestor_or_same(repo, step, base)? {
                if step == at {
                    return Ok(true);
                }
                let commit = repo.find_commit(step).map_err(unreadable)?;
                match history::parents(repo, &commit).map_err(unreadable)?.first() {
                    Some(parent) => step = *parent,
                    None => break,
                }
            }
        }
    }
    Ok(false)
}

/// Whether a merge only brings in its own target line: every parent other
/// than the first is the line's tip or an ancestor of it. The facts are the
/// merge's parents and the line's tip, so a merge of anything the line does
/// not hold, `main` included, is judged as before.
fn line_sync(repo: &Repository, parents: &[Oid], tip: Oid) -> Result<bool, String> {
    for parent in &parents[1..] {
        if !is_ancestor_or_same(repo, *parent, tip)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A home imported later from this receiver cannot authorize the receiver's
/// merge. Use only the earlier source version on that home's first-parent line.
fn source_before(repo: &Repository, mut source: Oid, merge: Oid) -> Result<Option<Oid>, String> {
    loop {
        if source != merge && is_ancestor_or_same(repo, source, merge)? {
            return Ok(Some(source));
        }
        let Some(parent) = super::first_parent(repo, source)? else {
            return Ok(None);
        };
        source = parent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(repo: &Repository, parent: Oid, path: &[u8], text: &[u8]) -> Oid {
        repo.reference("refs/heads/main", parent, true, "fixture")
            .unwrap();
        let mut index = repo.index().unwrap();
        index
            .read_tree(&repo.find_commit(parent).unwrap().tree().unwrap())
            .unwrap();
        index.write().unwrap();
        crate::git::add_commit(repo, &[(path, text)])
    }

    fn merge(repo: &Repository, first: Oid, second: Oid) -> Oid {
        let first = repo.find_commit(first).unwrap();
        let second = repo.find_commit(second).unwrap();
        let tree = repo
            .merge_commits(&first, &second, None)
            .unwrap()
            .write_tree_to(repo)
            .unwrap();
        let who = git2::Signature::now("Test", "test@example.test").unwrap();
        repo.commit(
            None,
            &who,
            &who,
            "fixture merge",
            &repo.find_tree(tree).unwrap(),
            &[&first, &second],
        )
        .unwrap()
    }

    fn task(id: u32) -> String {
        format!("---\nid: TSK-{id:03}\nstatus: todo\nintegration_target: integration/EPC-001-line\n---\n\n## Acceptance Criteria\n\n- AC-1 (journey) When run, the system shall work.\n\n## Closeout\n\n")
    }

    fn two_tasks(review: Option<Oid>) -> (tempfile::TempDir, Repository, Oid, Oid, Oid) {
        let dir = tempfile::tempdir().unwrap();
        let (repo, base) = crate::git::repo_with_tree(
            dir.path(),
            &[
                (b"project-management/tasks/TSK-001.md", task(1).as_bytes()),
                (b"project-management/tasks/TSK-002.md", task(2).as_bytes()),
            ],
        );
        let earlier = commit(&repo, base, b"src/earlier.rs", b"earlier work\n");
        let landed = merge(&repo, base, earlier);
        let later = commit(&repo, base, b"src/later.rs", b"later work\n");
        let sync = merge(&repo, later, landed);
        let reviewed = commit(&repo, sync, b"src/later.rs", b"reviewed later work\n");
        let completion_review = review.unwrap_or(reviewed);
        let completed = format!("{}```yaml\nacceptance:\n  reviewed: {completion_review}\n  review: https://example.test/review\n  criteria:\n    AC-1: verified | fixture passed\n  journey: verified | fixture ran\n  not_verified: none\n  follow_ups: none\n  verdict: approved\n```\n", task(2).replace("status: todo", "status: complete"));
        let completion = commit(
            &repo,
            reviewed,
            b"project-management/tasks/TSK-002.md",
            completed.as_bytes(),
        );
        let head = merge(&repo, landed, completion);
        repo.reference("refs/heads/main", base, true, "published default")
            .unwrap();
        repo.reference(
            "refs/heads/integration/EPC-001-line",
            head,
            true,
            "fixture line",
        )
        .unwrap();
        (dir, repo, base, landed, head)
    }

    fn range(repo: &Repository, base: Oid, head: Oid) -> Vec<Finding> {
        super::super::completions_in_range(
            repo,
            &base.to_string(),
            &head.to_string(),
            Some(base),
            &super::super::Criteria::Amendable,
            &super::super::RunBases::new([base, head]),
            Some("integration/EPC-001-line"),
            &[],
        )
        .unwrap()
    }

    #[test]
    fn later_review_does_not_cover_an_earlier_tasks_merge() {
        let (_dir, repo, base, landed, head) = two_tasks(None);
        let found = range(&repo, base, head);
        assert!(
            !found
                .iter()
                .any(|finding| !finding.note && finding.message.contains("TSK-002")),
            "later task must have a valid completion: {found:?}"
        );
        assert!(
            found
                .iter()
                .any(|finding| finding.message.contains("src/earlier.rs")
                    && finding.message.contains(&landed.to_string())),
            "earlier task must still owe its own completion: {found:?}"
        );
        assert!(
            !found
                .iter()
                .any(|finding| finding.message.contains("src/later.rs")),
            "later task owns and reviewed its own work: {found:?}"
        );
    }

    fn judge(
        repo: &Repository,
        base: Oid,
        head: Oid,
        reviewed: &[Oid],
    ) -> Result<Vec<Finding>, String> {
        repo.reference("refs/heads/main", base, true, "published default")
            .unwrap();
        let reviewed: Vec<_> = reviewed
            .iter()
            .map(|head| ReviewedTaskHead {
                head: *head,
                branch_base: None,
                made_at: None,
            })
            .collect();
        findings(
            repo,
            base,
            head,
            Some("integration/EPC-001-line"),
            &Graph::default(),
            &reviewed,
            None,
        )
    }

    #[test]
    fn a_tasks_own_commits_before_and_after_a_line_sync_stay_reviewed() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, base) = crate::git::repo_with_tree(dir.path(), &[(b"README.md", b"base")]);
        let line = commit(
            &repo,
            base,
            b"project-management/tasks/TSK-001.md",
            b"records only",
        );
        let own = commit(&repo, base, b"src/own.rs", b"own work");
        let sync = merge(&repo, own, line);
        let reviewed = commit(&repo, sync, b"src/own.rs", b"own revision");
        let head = merge(&repo, line, reviewed);
        assert!(judge(&repo, base, head, &[reviewed]).unwrap().is_empty());
    }

    #[test]
    fn an_unreadable_completion_cannot_cover_product_work() {
        let missing = Oid::from_str(&"f".repeat(40)).unwrap();
        let (_dir, repo, base, _landed, head) = two_tasks(Some(missing));
        let found = range(&repo, base, head);
        assert!(
            found
                .iter()
                .any(|finding| !finding.note && finding.message.contains(&missing.to_string())),
            "unreadable review must refuse and name its commit: {found:?}"
        );
        assert!(
            found
                .iter()
                .any(|finding| !finding.note && finding.message.contains("src/later.rs")),
            "an unreadable completion grants no product exemption: {found:?}"
        );
    }

    #[test]
    fn an_ambiguous_reviewed_merge_refuses_and_names_its_commit() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, base) = crate::git::repo_with_tree(dir.path(), &[(b"README.md", b"base")]);
        let a = commit(&repo, base, b"src/a.rs", b"a");
        let b = commit(&repo, base, b"src/b.rs", b"b");
        let merged = merge(&repo, a, b);
        let parents = [base, a, b].map(|id| repo.find_commit(id).unwrap());
        let who = git2::Signature::now("Test", "test@example.test").unwrap();
        let ambiguous = repo
            .commit(
                None,
                &who,
                &who,
                "ambiguous merge",
                &repo.find_commit(merged).unwrap().tree().unwrap(),
                &parents.iter().collect::<Vec<_>>(),
            )
            .unwrap();
        let head = merge(&repo, base, ambiguous);
        let error = judge(&repo, base, head, &[ambiguous]).unwrap_err();
        assert!(error.contains(&ambiguous.to_string()), "{error}");
        assert!(error.contains("3 parents"), "{error}");
    }

    #[test]
    fn a_records_only_landing_needs_no_product_completion() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, base) = crate::git::repo_with_tree(dir.path(), &[(b"README.md", b"base")]);
        let records = commit(
            &repo,
            base,
            b"project-management/tasks/TSK-001.md",
            b"records only",
        );
        let head = merge(&repo, base, records);
        assert!(judge(&repo, base, head, &[]).unwrap().is_empty());
    }

    #[test]
    fn a_clean_sync_of_a_verified_source_adds_no_product_work() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, base) = crate::git::repo_with_tree(dir.path(), &[(b"README.md", b"base")]);
        let line = commit(
            &repo,
            base,
            b"project-management/tasks/TSK-001.md",
            b"records only",
        );
        let source = commit(&repo, base, b"src/source.rs", b"published work");
        let head = merge(&repo, line, source);
        let found = findings(
            &repo,
            base,
            head,
            Some("integration/EPC-001-line"),
            &Graph::default(),
            &[],
            None,
        )
        .unwrap();
        assert!(
            found.is_empty(),
            "clean sync adds no work of its own: {found:?}"
        );
    }

    #[test]
    fn a_review_on_the_line_cannot_cover_an_earlier_merge_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, base) = crate::git::repo_with_tree(
            dir.path(),
            &[(b"project-management/tasks/TSK-002.md", task(2).as_bytes())],
        );
        let first = commit(
            &repo,
            base,
            b"project-management/tasks/TSK-001.md",
            task(1).as_bytes(),
        );
        let second = commit(
            &repo,
            base,
            b"project-management/tasks/TSK-003.md",
            task(3).as_bytes(),
        );
        let clean = merge(&repo, first, second);
        let resolved = commit(
            &repo,
            clean,
            b"src/hidden.rs",
            b"uncompleted merge resolution",
        );
        let who = git2::Signature::now("Test", "test@example.test").unwrap();
        let reviewed = repo
            .commit(
                None,
                &who,
                &who,
                "merge with own work",
                &repo.find_commit(resolved).unwrap().tree().unwrap(),
                &[
                    &repo.find_commit(first).unwrap(),
                    &repo.find_commit(second).unwrap(),
                ],
            )
            .unwrap();
        let completed = format!("{}```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/review\n  criteria:\n    AC-1: verified | fixture passed\n  journey: verified | fixture ran\n  not_verified: none\n  follow_ups: none\n  verdict: approved\n```\n", task(2).replace("status: todo", "status: complete"));
        let head = commit(
            &repo,
            reviewed,
            b"project-management/tasks/TSK-002.md",
            completed.as_bytes(),
        );
        repo.reference("refs/heads/main", base, true, "published default")
            .unwrap();
        repo.reference(
            "refs/heads/integration/EPC-001-line",
            head,
            true,
            "receiving line",
        )
        .unwrap();
        let found = super::super::completions_in_range(
            &repo,
            &base.to_string(),
            &head.to_string(),
            Some(base),
            &super::super::Criteria::Amendable,
            &super::super::RunBases::new([base, head]),
            Some("integration/EPC-001-line"),
            &[],
        )
        .unwrap();
        assert!(
            found.iter().any(|finding| !finding.note
                && finding.message.contains("src/hidden.rs")
                && finding.message.contains(&reviewed.to_string())),
            "the line review cannot vouch for the earlier merge resolution: {found:?}"
        );
    }

    #[test]
    fn an_unlanded_task_review_uses_its_own_branch_base() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, base) = crate::git::repo_with_tree(dir.path(), &[(b"README.md", b"base")]);
        let line = commit(
            &repo,
            base,
            b"project-management/tasks/TSK-001.md",
            b"line record",
        );
        let own = commit(&repo, base, b"src/own.rs", b"own work");
        let sync = merge(&repo, own, line);
        let reviewed = commit(&repo, sync, b"src/own.rs", b"reviewed own work");
        let head = commit(
            &repo,
            reviewed,
            b"project-management/tasks/TSK-002.md",
            b"completion",
        );
        repo.reference("refs/heads/main", base, true, "published default")
            .unwrap();
        let reviewed = [ReviewedTaskHead {
            head: reviewed,
            branch_base: Some(line),
            made_at: None,
        }];
        let found = findings(
            &repo,
            line,
            head,
            Some("integration/EPC-001-line"),
            &Graph::default(),
            &reviewed,
            None,
        )
        .unwrap();
        assert!(
            found.is_empty(),
            "the task branch's own review still applies before landing: {found:?}"
        );
    }

    #[test]
    fn a_completed_task_branch_gets_its_base_through_the_pull_request_check() {
        let (_dir, repo, _base, line, landed) = two_tasks(None);
        let head =
            super::super::history::parent_id(&repo, &repo.find_commit(landed).unwrap(), 1).unwrap();
        repo.reference(
            "refs/heads/integration/EPC-001-line",
            line,
            true,
            "receiving line before landing",
        )
        .unwrap();
        let found = super::super::pull_request_findings_into(
            repo.workdir().unwrap(),
            &line.to_string(),
            &head.to_string(),
            &super::super::Criteria::OwnTask("TSK-002".into()),
            None,
            (
                Some("task/TSK-002-own-work"),
                Some("integration/EPC-001-line"),
                None,
            ),
            &[],
            super::super::Scope::default(),
        )
        .unwrap();
        assert!(
            !found.iter().any(|finding| !finding.note),
            "the completed task branch must keep its own review: {found:?}"
        );
    }

    #[test]
    fn a_task_branch_review_covers_its_hand_resolved_sync_of_the_line() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, base) = crate::git::repo_with_tree(
            dir.path(),
            &[
                (b"project-management/tasks/TSK-002.md", task(2).as_bytes()),
                (b"src/shared.rs", b"base\n"),
            ],
        );
        let line_work = commit(&repo, base, b"src/shared.rs", b"line\n");
        let line = merge(&repo, base, line_work);
        let own = commit(&repo, base, b"src/shared.rs", b"own\n");
        // Both sides changed the file, so this sync's resolution is the task's
        // own work, which only a walk bounded by the branch's base can credit.
        let resolution = commit(&repo, own, b"src/shared.rs", b"resolved\n");
        let who = git2::Signature::now("Test", "test@example.test").unwrap();
        let sync = repo
            .commit(
                None,
                &who,
                &who,
                "hand-resolved sync of the line",
                &repo.find_commit(resolution).unwrap().tree().unwrap(),
                &[
                    &repo.find_commit(own).unwrap(),
                    &repo.find_commit(line).unwrap(),
                ],
            )
            .unwrap();
        assert!(
            matches!(
                super::super::read_merge(&repo, &repo.find_commit(sync).unwrap()),
                super::super::MergeReading::Changed(_)
            ),
            "the sync must read as hand-resolved"
        );
        let reviewed = commit(&repo, sync, b"src/own.rs", b"reviewed own work\n");
        let completed = format!("{}```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/review\n  criteria:\n    AC-1: verified | fixture passed\n  journey: verified | fixture ran\n  not_verified: none\n  follow_ups: none\n  verdict: approved\n```\n", task(2).replace("status: todo", "status: complete"));
        let head = commit(
            &repo,
            reviewed,
            b"project-management/tasks/TSK-002.md",
            completed.as_bytes(),
        );
        repo.reference("refs/heads/main", base, true, "published default")
            .unwrap();
        repo.reference(
            "refs/heads/integration/EPC-001-line",
            line,
            true,
            "receiving line before landing",
        )
        .unwrap();
        let found = super::super::pull_request_findings_into(
            repo.workdir().unwrap(),
            &line.to_string(),
            &head.to_string(),
            &super::super::Criteria::OwnTask("TSK-002".into()),
            None,
            (
                Some("task/TSK-002-own-work"),
                Some("integration/EPC-001-line"),
                None,
            ),
            &[],
            super::super::Scope::default(),
        )
        .unwrap();
        assert!(
            !found.iter().any(|finding| !finding.note),
            "the task's review covers its own hand-resolved sync: {found:?}"
        );
    }

    #[test]
    fn a_late_completion_keeps_its_original_work_after_syncing_the_line() {
        let (_dir, repo, base, first_landing, before) = two_tasks(None);
        let original =
            super::super::history::parent_id(&repo, &repo.find_commit(first_landing).unwrap(), 1)
                .unwrap();
        let reviewed = merge(&repo, original, before);
        let completed = format!("{}```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/review\n  criteria:\n    AC-1: verified | original task work reviewed\n  journey: verified | late completion fixture ran\n  not_verified: none\n  follow_ups: none\n  verdict: approved\n```\n", task(1).replace("status: todo", "status: complete"));
        let completion = commit(
            &repo,
            reviewed,
            b"project-management/tasks/TSK-001.md",
            completed.as_bytes(),
        );
        let head = merge(&repo, before, completion);
        repo.reference("refs/heads/main", base, true, "published default")
            .unwrap();
        repo.reference(
            "refs/heads/integration/EPC-001-line",
            head,
            true,
            "receiving line",
        )
        .unwrap();
        let found = range(&repo, base, head);
        assert!(
            !found.iter().any(|finding| !finding.note),
            "both tasks now have their own valid completion: {found:?}"
        );
    }

    fn block(key: &str, reviewed: Oid, reason: bool) -> String {
        let reason = if reason {
            "  reason: regression found\n"
        } else {
            ""
        };
        format!("```yaml\n{key}:\n{reason}  reviewed: {reviewed}\n  review: https://example.test/review\n  criteria:\n    AC-1: verified | fixture passed\n  journey: verified | fixture ran\n  not_verified: none\n  follow_ups: none\n  verdict: approved\n```\n")
    }

    /// How the forged in-range completion of TSK-002 reaches its branch.
    #[derive(Clone, Copy)]
    enum Forge {
        /// On TSK-002's own chain, after it merged TSK-001's unlanded tip,
        /// naming that tip.
        NamesOther,
        /// The same, naming TSK-002's own commit (the control).
        NamesOwn,
        /// Committed on a side branch built on TSK-001's tip, naming that
        /// tip, and merged into TSK-002's branch.
        SideBranch,
    }

    /// A forged completion of TSK-002 that a reopen archives, then a
    /// genuine completion. TSK-001's unlanded work comes in through
    /// TSK-002's branch and must stay refused.
    fn forged_archive(forge: Forge) -> Vec<Finding> {
        let dir = tempfile::tempdir().unwrap();
        let (repo, base) = crate::git::repo_with_tree(
            dir.path(),
            &[
                (b"project-management/tasks/TSK-001.md", task(1).as_bytes()),
                (b"project-management/tasks/TSK-002.md", task(2).as_bytes()),
            ],
        );
        let earlier = commit(&repo, base, b"src/earlier.rs", b"unreviewed other work\n");
        let later = commit(&repo, base, b"src/later.rs", b"later work\n");
        let done = task(2).replace("status: todo", "status: complete");
        let (named, first) = match forge {
            Forge::NamesOther | Forge::NamesOwn => {
                let imported = merge(&repo, later, earlier);
                let named = if matches!(forge, Forge::NamesOther) {
                    earlier
                } else {
                    later
                };
                let first = commit(
                    &repo,
                    imported,
                    b"project-management/tasks/TSK-002.md",
                    format!("{done}{}", block("acceptance", named, false)).as_bytes(),
                );
                (named, first)
            }
            Forge::SideBranch => {
                let forged = commit(
                    &repo,
                    earlier,
                    b"project-management/tasks/TSK-002.md",
                    format!("{done}{}", block("acceptance", earlier, false)).as_bytes(),
                );
                (earlier, merge(&repo, later, forged))
            }
        };
        let archived = block("acceptance_superseded", named, true);
        let reopened = commit(
            &repo,
            first,
            b"project-management/tasks/TSK-002.md",
            format!("{}{archived}", task(2)).as_bytes(),
        );
        let reviewed = commit(&repo, reopened, b"src/later.rs", b"reviewed later work\n");
        let completion = commit(
            &repo,
            reviewed,
            b"project-management/tasks/TSK-002.md",
            format!("{done}{archived}{}", block("acceptance", reviewed, false)).as_bytes(),
        );
        let head = merge(&repo, base, completion);
        repo.reference("refs/heads/main", base, true, "published default")
            .unwrap();
        repo.reference(
            "refs/heads/integration/EPC-001-line",
            head,
            true,
            "fixture line",
        )
        .unwrap();
        let found = range(&repo, base, head);
        drop(dir);
        found
    }

    fn refuses_earlier(found: &[Finding]) -> bool {
        found
            .iter()
            .any(|finding| !finding.note && finding.message.contains("src/earlier.rs"))
    }

    #[test]
    fn a_forged_archive_cannot_credit_another_tasks_work() {
        let found = forged_archive(Forge::NamesOther);
        assert!(
            refuses_earlier(&found),
            "another task's unreviewed work must stay refused: {found:?}"
        );
    }

    #[test]
    fn an_archive_naming_its_own_commit_does_not_cover_an_import() {
        let found = forged_archive(Forge::NamesOwn);
        assert!(
            refuses_earlier(&found),
            "imported work stays refused: {found:?}"
        );
    }

    #[test]
    fn a_forged_archive_on_a_merged_side_branch_cannot_credit_another_tasks_work() {
        let found = forged_archive(Forge::SideBranch);
        assert!(
            refuses_earlier(&found),
            "a completion made on another task's tip stays refused: {found:?}"
        );
    }
}
