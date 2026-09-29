//! Merged pull requests read from this clone's history (TSK-149).
//!
//! A pull request is a merge commit whose subject is GitHub's
//! `Merge pull request #N from <branch>`, reachable from any local branch or
//! remote-tracking ref. Everything here is read in process from Git objects;
//! nothing asks the host.

use std::collections::BTreeMap;
use std::path::Path;

use git2::{Commit, DiffOptions, Patch, Repository};

use super::Window;
use crate::hooks::rfc3339_from_unix;

/// The directories that hold the work records.
const RECORD_DIRS: &[&str] = &[
    "project-management/epics/",
    "project-management/specs/",
    "project-management/tasks/",
];

/// Where a changed line sits in a record: a frontmatter key, or a body
/// section named by its `## ` heading. Lines above the first section are
/// the title region.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Region {
    Front(String),
    Section(String),
    Title,
    Delimiter,
}

/// The regions a record-status pull request may touch: the status, the
/// acceptance evidence that moves with it, and the closeout.
fn status_region(region: &Region) -> bool {
    match region {
        Region::Front(key) => key == "status" || key == "acceptance",
        Region::Section(name) => name == "Closeout" || name == "Acceptance Criteria",
        Region::Title | Region::Delimiter => false,
    }
}

/// One merged pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedPr {
    /// The pull request number.
    pub number: u64,
    /// Its head branch, without a leading `<owner>/`.
    pub branch: String,
    /// The branch prefix without its `/` (`task`, `plan`, ...), or `other`.
    pub kind: String,
    /// The logical change it belongs to: the task id of a `task/` branch,
    /// otherwise the branch.
    pub change: String,
    /// It changed only record status: every changed line is a record's
    /// status, acceptance evidence or closeout, and a status changed.
    pub status_only: bool,
    /// When it merged (RFC 3339 UTC).
    pub merged_at: String,
    /// When its earliest own commit was authored (RFC 3339 UTC).
    pub started_at: String,
}

/// The merged pull requests in `window`, by number. `prefixes` are the
/// policy's branch prefixes (`task/`, ...).
///
/// # Errors
///
/// Returns a message when `repo_root` is not a readable Git repository.
pub fn merged_prs(
    repo_root: &Path,
    prefixes: &[String],
    window: &Window,
) -> Result<Vec<MergedPr>, String> {
    let repo = Repository::discover(repo_root).map_err(|e| format!("not a git repository: {e}"))?;
    let mut walk = repo.revwalk().map_err(|e| e.to_string())?;
    for glob in ["refs/heads/*", "refs/remotes/*"] {
        walk.push_glob(glob).map_err(|e| e.to_string())?;
    }
    // The first merge seen for a number, by commit time then id, so a
    // number copied by a rewritten history resolves the same way each run.
    let mut found: BTreeMap<u64, (i64, String, git2::Oid, String)> = BTreeMap::new();
    for oid in walk {
        let Ok(oid) = oid else { continue };
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        if commit.parent_count() < 2 {
            continue;
        }
        let Some((number, head)) = commit.summary().ok().flatten().and_then(parse_subject) else {
            continue;
        };
        let merged = commit.time().seconds();
        if !in_window(window, number, &unix_text(merged)) {
            continue;
        }
        let key = (merged, oid.to_string(), oid, head.to_string());
        match found.get(&number) {
            Some(kept) if (kept.0, &kept.1) <= (key.0, &key.1) => {}
            _ => {
                found.insert(number, key);
            }
        }
    }
    let mut out = Vec::with_capacity(found.len());
    for (number, (merged, _, oid, head)) in found {
        let commit = repo.find_commit(oid).map_err(|e| e.to_string())?;
        let branch = strip_owner(&head, prefixes);
        let kind = prefixes
            .iter()
            .find(|prefix| branch.starts_with(prefix.as_str()))
            .map_or_else(
                || "other".to_string(),
                |p| p.trim_end_matches('/').to_string(),
            );
        out.push(MergedPr {
            number,
            change: logical_change(&branch),
            status_only: status_only(&repo, &commit),
            merged_at: unix_text(merged),
            started_at: unix_text(started(&repo, &commit).unwrap_or(merged)),
            kind,
            branch,
        });
    }
    Ok(out)
}

fn unix_text(seconds: i64) -> String {
    rfc3339_from_unix(u64::try_from(seconds).unwrap_or(0))
}

/// Whether pull request `number`, merged at `merged_at`, is in `window`.
fn in_window(window: &Window, number: u64, merged_at: &str) -> bool {
    match window {
        Window::Numbers { first, last } => (*first..=*last).contains(&number),
        Window::Dates { .. } => {
            let (start, end) = window.span_of_dates();
            merged_at >= start.as_str() && end.as_deref().is_none_or(|end| merged_at <= end)
        }
    }
}

/// `#N` and the head branch of GitHub's merge subject.
fn parse_subject(subject: &str) -> Option<(u64, &str)> {
    let rest = subject.strip_prefix("Merge pull request #")?;
    let (number, rest) = rest.split_once(' ')?;
    let head = rest.strip_prefix("from ")?.split_whitespace().next()?;
    Some((number.parse().ok()?, head))
}

/// GitHub names a head branch `<owner>/<branch>` in some merge subjects;
/// drop the owner when what follows it is a sanctioned branch.
fn strip_owner(head: &str, prefixes: &[String]) -> String {
    let sanctioned = |name: &str| prefixes.iter().any(|p| name.starts_with(p.as_str()));
    if !sanctioned(head) {
        if let Some((_, rest)) = head.split_once('/') {
            if sanctioned(rest) {
                return rest.to_string();
            }
        }
    }
    head.to_string()
}

/// The task id of a `task/<ID>-<slug>` branch, otherwise the branch.
fn logical_change(branch: &str) -> String {
    let Some(rest) = branch.strip_prefix("task/") else {
        return branch.to_string();
    };
    let mut parts = rest.splitn(3, '-');
    match (parts.next(), parts.next()) {
        (Some(kind), Some(digits))
            if !kind.is_empty()
                && kind.chars().all(|c| c.is_ascii_uppercase())
                && !digits.is_empty()
                && digits.chars().all(|c| c.is_ascii_digit()) =>
        {
            format!("{kind}-{digits}")
        }
        _ => branch.to_string(),
    }
}

/// The earliest author time among the commits the merge brought in.
fn started(repo: &Repository, merge: &Commit<'_>) -> Option<i64> {
    let mut walk = repo.revwalk().ok()?;
    walk.push(merge.parent_id(1).ok()?).ok()?;
    walk.hide(merge.parent_id(0).ok()?).ok()?;
    walk.filter_map(Result::ok)
        .filter_map(|oid| repo.find_commit(oid).ok())
        .map(|commit| commit.author().when().seconds())
        .min()
}

/// Whether the merge changed only record status (see [`MergedPr`]).
fn status_only(repo: &Repository, merge: &Commit<'_>) -> bool {
    let (Ok(first), Ok(tree)) = (merge.parent(0), merge.tree()) else {
        return false;
    };
    let Ok(base) = first.tree() else {
        return false;
    };
    let mut options = DiffOptions::new();
    options.context_lines(0);
    let Ok(diff) = repo.diff_tree_to_tree(Some(&base), Some(&tree), Some(&mut options)) else {
        return false;
    };
    if diff.deltas().len() == 0 {
        return false;
    }
    let mut status_seen = false;
    for index in 0..diff.deltas().len() {
        let Some(delta) = diff.get_delta(index) else {
            return false;
        };
        let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) else {
            return false;
        };
        let path = path.to_string_lossy();
        let is_record =
            RECORD_DIRS.iter().any(|dir| path.starts_with(dir)) && path.ends_with(".md");
        // A record added or removed is planning, not a status move.
        if !is_record || delta.old_file().id().is_zero() || delta.new_file().id().is_zero() {
            return false;
        }
        let (Ok(old), Ok(new)) = (
            repo.find_blob(delta.old_file().id()),
            repo.find_blob(delta.new_file().id()),
        ) else {
            return false;
        };
        let old_regions = regions(&String::from_utf8_lossy(old.content()));
        let new_regions = regions(&String::from_utf8_lossy(new.content()));
        let Ok(Some(lines)) = Patch::from_diff(&diff, index) else {
            return false;
        };
        for hunk in 0..lines.num_hunks() {
            let Ok((hunk, _)) = lines.hunk(hunk) else {
                return false;
            };
            let old_lines = line_range(hunk.old_start(), hunk.old_lines());
            let new_lines = line_range(hunk.new_start(), hunk.new_lines());
            let touched = old_lines
                .map(|line| old_regions.get(line))
                .chain(new_lines.map(|line| new_regions.get(line)));
            for region in touched {
                let Some(region) = region else {
                    return false;
                };
                if !status_region(region) {
                    return false;
                }
                status_seen |= *region == Region::Front("status".to_string());
            }
        }
    }
    status_seen
}

/// Zero-based line indexes of a hunk side (`start` is one-based).
fn line_range(start: u32, count: u32) -> std::ops::Range<usize> {
    let start = usize::try_from(start.max(1) - 1).unwrap_or(0);
    start..start + usize::try_from(count).unwrap_or(0)
}

/// The region of each line of a record.
fn regions(text: &str) -> Vec<Region> {
    let mut out = Vec::new();
    let mut delimiters = 0;
    let mut key = String::new();
    let mut section: Option<String> = None;
    for line in text.lines() {
        if delimiters < 2 && line.trim_end() == "---" {
            delimiters += 1;
            out.push(Region::Delimiter);
            continue;
        }
        if delimiters == 1 {
            // A top-level key starts at column 0; nested lines and list
            // items belong to the key above them.
            if !line.starts_with([' ', '\t', '-', '#']) {
                if let Some((name, _)) = line.split_once(':') {
                    key = name.trim().to_string();
                }
            }
            out.push(Region::Front(key.clone()));
            continue;
        }
        if let Some(name) = line.strip_prefix("## ") {
            section = Some(name.trim().to_string());
        }
        out.push(section.clone().map_or(Region::Title, Region::Section));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects_name_the_number_and_the_head_branch() {
        assert_eq!(
            parse_subject("Merge pull request #745 from task/TSK-147-warnings"),
            Some((745, "task/TSK-147-warnings"))
        );
        assert_eq!(
            parse_subject("Merge pull request #12 from owner/fix/x"),
            Some((12, "owner/fix/x"))
        );
        assert_eq!(parse_subject("Merge branch 'main' into feat/x"), None);
        assert_eq!(parse_subject("Merge pull request #x from a"), None);
    }

    #[test]
    fn an_owner_is_dropped_only_before_a_sanctioned_branch() {
        let prefixes = vec!["task/".to_string(), "fix/".to_string()];
        assert_eq!(strip_owner("owner/fix/x", &prefixes), "fix/x");
        assert_eq!(strip_owner("fix/x", &prefixes), "fix/x");
        assert_eq!(strip_owner("owner/wip", &prefixes), "owner/wip");
    }

    #[test]
    fn a_task_branch_is_its_task_and_any_other_branch_is_itself() {
        assert_eq!(logical_change("task/TSK-134-single-gate"), "TSK-134");
        assert_eq!(logical_change("task/TSK-134-gate-evidence"), "TSK-134");
        assert_eq!(logical_change("plan/tsk-064-scope"), "plan/tsk-064-scope");
        assert_eq!(logical_change("task/no-id"), "task/no-id");
    }

    #[test]
    fn a_record_splits_into_frontmatter_keys_and_sections() {
        let record = "---\nid: TSK-1\nstatus: todo  # a comment\ndepends_on:\n  - TSK-0\n---\n\n# TSK-1: t\n\n## Acceptance Criteria\n\n- AC-1\n\n## Closeout\n\nDone.\n";
        let got = regions(record);
        assert_eq!(got[0], Region::Delimiter);
        assert_eq!(got[2], Region::Front("status".into()));
        assert_eq!(got[4], Region::Front("depends_on".into()));
        assert_eq!(got[5], Region::Delimiter);
        assert_eq!(got[7], Region::Title);
        assert_eq!(got[11], Region::Section("Acceptance Criteria".into()));
        assert_eq!(got[15], Region::Section("Closeout".into()));
    }
}
