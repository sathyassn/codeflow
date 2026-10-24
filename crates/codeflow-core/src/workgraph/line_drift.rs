//! Read-only integration-line facts, shared by orient and line management.
//!
//! Fetching belongs to the caller. All ancestry goes through guarded history;
//! remote-tracking reflogs prove a fetch, never the remote branch's creation.
use std::path::Path;

use git2::{Oid, Repository};

use super::{history, work_start};

/// Facts at one integration ref and one protected target tip.
#[derive(Debug, Clone)]
pub struct LineDrift {
    pub line: String,
    pub reference: String,
    pub target: String,
    pub behind: usize,
    /// UTC seconds at the newest own merge of a target ancestor.
    pub last_sync: Option<i64>,
    /// UTC seconds from an actual local ref-creation reflog entry.
    pub created: Option<i64>,
    /// UTC seconds of the oldest divergent first-parent commit, not creation.
    pub oldest_divergent_commit: Option<i64>,
    /// Earliest creation date among nonterminal task records on this line.
    pub oldest_unlanded_task: Option<String>,
    /// Nonterminal task records on this line skipped for a missing or
    /// invalid `created`; one bad record never hides the line's other facts.
    pub undated_tasks: Vec<String>,
    pub local: bool,
}

impl LineDrift {
    /// Whether the project interval has elapsed. Fresh and undated lines do
    /// not acquire a reminder from inherited or divergent commit dates.
    #[must_use]
    pub fn sync_due(&self, now: i64, interval_days: u32) -> bool {
        interval_days > 0
            && self.oldest_divergent_commit.is_some()
            && self
                .last_sync
                .or(self.created)
                .is_some_and(|date| now.saturating_sub(date) > i64::from(interval_days) * 86_400)
    }
}

/// Read every integration ref in `remote`'s fetched namespace, or local
/// branches when `remote` is `None`. Does not fetch or change refs.
///
/// # Errors
/// Refuses overlays, incomplete history, ambiguous targets and unreadable records.
pub fn read(root: &Path, remote: Option<&str>) -> Result<Vec<LineDrift>, String> {
    let Some(repo) = crate::hooks::repo::open(root)? else {
        return Ok(Vec::new());
    };
    let _judgment = history::judgment();
    history::check(&repo).map_err(|e| e.to_string())?;
    let prefix = remote.map_or_else(|| "refs/heads/".into(), |r| format!("refs/remotes/{r}/"));
    let references = repo
        .references_glob(&format!("{prefix}integration/*"))
        .map_err(|e| e.to_string())?;
    let mut refs = references
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if refs.is_empty() {
        return Ok(Vec::new());
    }
    refs.sort_by(|a, b| a.name_bytes().cmp(b.name_bytes()));
    let target_ref = target_reference(&repo, &prefix, remote)?;
    let target = repo
        .find_reference(&target_ref)
        .and_then(|r| r.peel_to_commit())
        .map_err(|e| e.to_string())?;
    refs.into_iter()
        .map(|reference| {
            let reference_name = crate::git::name::reference_name(&reference);
            let name = reference_name.rule_text().map_err(|e| e.to_string())?;
            let line = name
                .strip_prefix(&prefix)
                .ok_or("integration ref outside its namespace")?;
            let tip = reference.peel_to_commit().map_err(|e| e.to_string())?;
            read_line(
                &repo,
                line,
                name,
                &target_ref,
                tip.id(),
                target.id(),
                remote.is_none(),
            )
        })
        .collect()
}

fn target_reference(
    repo: &Repository,
    prefix: &str,
    remote: Option<&str>,
) -> Result<String, String> {
    if remote.is_some() {
        match repo.find_reference(&format!("{prefix}HEAD")) {
            Ok(reference) => {
                let resolved = reference.resolve().map_err(|e| e.to_string())?;
                let reference_name = crate::git::name::reference_name(&resolved);
                let name = reference_name.rule_text().map_err(|e| e.to_string())?;
                if !name.starts_with(prefix) {
                    return Err("remote HEAD points outside its tracking namespace".into());
                }
                return Ok(name.into());
            }
            Err(error) if error.code() == git2::ErrorCode::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    for branch in ["main", "master"] {
        let name = format!("{prefix}{branch}");
        match repo.find_reference(&name) {
            Ok(_) => return Ok(name),
            Err(error) if error.code() == git2::ErrorCode::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("protected target unavailable; fetch the default branch or create a local main/master target".into())
}

fn read_line(
    repo: &Repository,
    line: &str,
    reference: &str,
    target_ref: &str,
    tip: Oid,
    target: Oid,
    local: bool,
) -> Result<LineDrift, String> {
    let result = || -> Result<LineDrift, git2::Error> {
        let base = history::merge_base(repo, tip, target)?;
        let (_, behind) = history::ahead_behind(repo, tip, target)?;
        let mut walk = history::revwalk(repo)?;
        walk.push(tip)?;
        walk.hide(base)?;
        walk.simplify_first_parent()?;
        let mut last_sync = None;
        let mut oldest = None;
        for oid in walk {
            let commit = repo.find_commit(oid?)?;
            let date = commit.time().seconds();
            oldest = Some(oldest.map_or(date, |old: i64| old.min(date)));
            for parent in history::parents(repo, &commit)?.iter().skip(1) {
                if *parent == target || history::descendant_of(repo, target, *parent)? {
                    last_sync = Some(last_sync.map_or(date, |last: i64| last.max(date)));
                }
            }
        }
        let created = if local {
            creation(repo, reference)?
        } else {
            None
        };
        Ok(LineDrift {
            line: line.into(),
            reference: reference.into(),
            target: target_ref.into(),
            behind,
            last_sync,
            created,
            oldest_divergent_commit: oldest,
            oldest_unlanded_task: None,
            undated_tasks: Vec::new(),
            local,
        })
    };
    let mut row = result().map_err(|e| format!("{line}: {e}"))?;
    let tree = repo
        .find_commit(tip)
        .and_then(|c| c.tree())
        .map_err(|e| e.to_string())?;
    let records = work_start::records_from_tree_matching(repo, &tree, |_, kind| {
        kind == work_start::RecordKind::Task
    })
    .map_err(|e| e.to_string())?;
    for record in records.values().filter(|r| {
        !matches!(r.status.as_str(), "complete" | "cancelled")
            && r.integration_target
                .as_deref()
                .map(work_start::logical_target)
                == Some(line)
    }) {
        let Some(day) = record
            .created
            .as_deref()
            .and_then(|date| date.get(..10))
            .filter(|d| date_seconds(d).is_some())
        else {
            row.undated_tasks.push(record.id.clone());
            continue;
        };
        if row
            .oldest_unlanded_task
            .as_deref()
            .is_none_or(|old| day < old)
        {
            row.oldest_unlanded_task = Some(day.into());
        }
    }
    Ok(row)
}

fn creation(repo: &Repository, reference: &str) -> Result<Option<i64>, git2::Error> {
    let log = match repo.reflog(reference) {
        Ok(log) => log,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    // A copy or rename carries the old ref's reflog, including its creation
    // entry. That entry does not prove when this ref was created.
    if log.iter().any(|entry| {
        entry.message().ok().flatten().is_some_and(|message| {
            message.starts_with("Branch: copied ") || message.starts_with("Branch: renamed ")
        })
    }) {
        return Ok(None);
    }
    Ok(log
        .iter()
        .next_back()
        .filter(|entry| {
            entry.id_old().is_zero()
                && entry
                    .message()
                    .ok()
                    .flatten()
                    .is_some_and(|m| m.starts_with("branch: Created from "))
        })
        .map(|entry| entry.committer().when().seconds()))
}

/// Parse a calendar date, rejecting invalid dates rather than inventing an age.
pub(crate) fn date_seconds(date: &str) -> Option<i64> {
    let mut fields = date.split('-');
    let year: i64 = fields.next()?.parse().ok()?;
    let month: i64 = fields.next()?.parse().ok()?;
    let day: i64 = fields.next()?.parse().ok()?;
    if fields.next().is_some()
        || !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
    {
        return None;
    }
    let y = year - i64::from(month <= 2);
    let era = y / 400;
    let yoe = y - era * 400;
    let m = month + if month > 2 { -3 } else { 9 };
    let days =
        era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + (153 * m + 2) / 5 + day - 1 - 719_468;
    let seconds = days * 86_400;
    (date_string(seconds) == date).then_some(seconds)
}

pub(crate) fn date_string(seconds: i64) -> String {
    let time =
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds.try_into().unwrap_or(0));
    super::rfc3339_at(time)[..10].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(repo: &Repository, parents: &[Oid], time: i64) -> Oid {
        let tree = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree).unwrap();
        let sig = git2::Signature::new(
            "Fixture",
            "fixture@example.invalid",
            &git2::Time::new(time, 0),
        )
        .unwrap();
        let parents: Vec<_> = parents
            .iter()
            .map(|oid| repo.find_commit(*oid).unwrap())
            .collect();
        repo.commit(
            None,
            &sig,
            &sig,
            "fixture",
            &tree,
            &parents.iter().collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn two_lines_use_their_own_merge_dates_and_no_fetch() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let base = commit(&repo, &[], 1_700_000_000);
        let target = commit(&repo, &[base], 1_700_000_001);
        let own = commit(&repo, &[base], 1_700_000_002);
        let sync = commit(&repo, &[own, target], 1_700_200_000);
        let advanced = commit(&repo, &[target], 1_700_300_000);
        for (name, oid) in [
            ("main", advanced),
            ("integration/fresh", target),
            ("integration/synced", sync),
        ] {
            repo.reference(&format!("refs/heads/{name}"), oid, false, "fixture ref")
                .unwrap();
        }
        let rows = read(dir.path(), None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].behind, 1);
        assert_eq!(rows[0].last_sync, None);
        assert_eq!(rows[0].oldest_divergent_commit, None);
        assert!(!rows[0].sync_due(1_701_000_000, 1));
        assert_eq!(rows[1].behind, 1);
        assert_eq!(rows[1].last_sync, Some(1_700_200_000));
        assert!(rows[1].sync_due(1_701_000_000, 1));
        assert!(!rows[1].sync_due(1_701_000_000, 0));
        assert!(!dir.path().join(".git/FETCH_HEAD").exists());
    }

    #[test]
    fn newest_sync_uses_commit_date_not_walk_order() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let base = commit(&repo, &[], 1_700_000_000);
        let target = commit(&repo, &[base], 1_700_000_001);
        let own = commit(&repo, &[base], 1_700_000_002);
        let first = commit(&repo, &[own, target], 1_700_200_000);
        let second_target = commit(&repo, &[target], 1_700_000_003);
        let second = commit(&repo, &[first, second_target], 1_700_100_000);
        let row = read_line(
            &repo,
            "integration/line",
            "refs/heads/integration/line",
            "refs/heads/main",
            second,
            second_target,
            false,
        )
        .unwrap();
        assert_eq!(row.last_sync, Some(1_700_200_000));
    }

    #[test]
    fn interval_needs_creation_or_sync_and_a_divergent_commit() {
        let mut row = LineDrift {
            line: "integration/a".into(),
            reference: String::new(),
            target: String::new(),
            behind: 1,
            last_sync: None,
            created: None,
            oldest_divergent_commit: Some(0),
            oldest_unlanded_task: None,
            undated_tasks: Vec::new(),
            local: true,
        };
        assert!(!row.sync_due(10 * 86_400, 1));
        row.created = Some(0);
        assert!(row.sync_due(10 * 86_400, 1));
        assert!(!row.sync_due(86_400, 1));
        row.last_sync = Some(10 * 86_400);
        assert!(!row.sync_due(10 * 86_400, 1));
        row.oldest_divergent_commit = None;
        assert!(!row.sync_due(20 * 86_400, 1));
    }

    #[test]
    fn dates_validate_leap_days_and_round_trip() {
        assert_eq!(date_seconds("1970-01-01"), Some(0));
        assert_eq!(date_seconds("2024-01-01"), Some(1_704_067_200));
        assert!(date_seconds("2024-02-29").is_some());
        for invalid in [
            "2023-02-29",
            "2024-13-01",
            "2024-00-00",
            "2024-04-31",
            "2024-1-1",
            "9999999999999-01-01",
        ] {
            assert_eq!(date_seconds(invalid), None, "{invalid}");
        }
    }
}
