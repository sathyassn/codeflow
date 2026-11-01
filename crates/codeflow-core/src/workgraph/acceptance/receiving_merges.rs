//! Product work introduced by a line merge remains the receiving line's work.
use std::collections::BTreeSet;

use super::{
    finding, history, is_ancestor_or_same, owned_paths, Finding, Graph, Oid, Repository,
    BINDING_RULE,
};

pub(super) fn findings(
    repo: &Repository,
    base: Oid,
    head: Oid,
    line: Option<&str>,
    graph: &Graph,
    reviewed: &[Oid],
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
    for id in walk {
        let commit = repo
            .find_commit(id.map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        let parents = history::parents(repo, &commit).map_err(|error| error.to_string())?;
        if parents.len() > 1 {
            merges.push((commit.id(), parents[0]));
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
    let project = super::super::amendment::project_at(repo, base)?;
    let mut found = Vec::new();
    for (merge, first) in merges {
        // The range's base is already the receiving line's work: a task
        // branch that re-merges its target line brings nothing new from it.
        let mut hide: Vec<String> = reviewed
            .iter()
            .chain([&first, &base])
            .map(ToString::to_string)
            .collect();
        for home in &homes {
            if let Some(source) = source_before(repo, *home, merge)? {
                hide.push(source.to_string());
            }
        }
        let refs: Vec<_> = hide.iter().map(String::as_str).collect();
        for path in owned_paths(root, &merge.to_string(), &refs)? {
            if super::super::classify::path_sets().adopter_facing_any_case(&path, &project) {
                found.push(finding(BINDING_RULE, format!("receiving line `{line}` introduces {path} through merge {merge} outside its verified source and reviewed task heads; carry that product work through the receiving task's pull request and review the result before landing")));
            }
        }
    }
    Ok(found)
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
