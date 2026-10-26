//! The recovery inventory behind `codeflow work status`: for each worktree
//! of a clone, the work only it holds, read from git. Uncommitted changes,
//! commits no remote-tracking branch has, and commits no branch at all has
//! (a detached HEAD's own commits). A worktree whose folder is gone, or
//! whose HEAD names an object that is missing, is named as not recoverable.
//! Reads only.

use std::path::{Path, PathBuf};

/// The most entries shown per list.
const SHOWN: usize = 5;

/// One worktree's work that exists nowhere else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeWork {
    pub path: PathBuf,
    /// The branch checked out, or `None` when HEAD is detached.
    pub branch: Option<String>,
    /// The task id the branch carries.
    pub task: Option<String>,
    /// `git status --porcelain` lines.
    pub uncommitted: Vec<String>,
    /// Commits on the branch that no remote-tracking ref holds.
    pub unpushed: Vec<String>,
    /// Commits on a detached HEAD that no branch or remote holds.
    pub no_branch: Vec<String>,
    /// What cannot be recovered, and why.
    pub not_recoverable: Vec<String>,
}

impl WorktreeWork {
    /// Whether this worktree holds anything to report.
    #[must_use]
    pub fn holds_work(&self) -> bool {
        !self.uncommitted.is_empty()
            || !self.unpushed.is_empty()
            || !self.no_branch.is_empty()
            || !self.not_recoverable.is_empty()
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    // `--no-optional-locks`: a status read never refreshes the index or
    // takes a lock; the inventory writes nothing, Git metadata included.
    let output = crate::git::command()
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// The task id a branch name carries (`task/TSK-123-slug` gives `TSK-123`).
#[must_use]
pub fn task_of(branch: &str) -> Option<String> {
    let start = branch.find("TSK-")?;
    let digits: String = branch[start + 4..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (digits.len() >= 3).then(|| format!("TSK-{digits}"))
}

/// Inventory every worktree of the clone that holds `start`.
///
/// # Errors
///
/// The reason the worktree list cannot be read.
pub fn inventory(start: &Path) -> Result<Vec<WorktreeWork>, String> {
    let listing = git(start, &["worktree", "list", "--porcelain", "-z"])?;
    let mut out = Vec::new();
    for record in listing
        .split("\0\0")
        .filter(|record| !record.trim_matches('\0').is_empty())
    {
        let mut work = WorktreeWork::default();
        let mut head = None;
        let mut prunable = None;
        let mut bare = false;
        for field in record.split('\0') {
            if let Some(path) = field.strip_prefix("worktree ") {
                work.path = PathBuf::from(path);
            } else if let Some(sha) = field.strip_prefix("HEAD ") {
                head = Some(sha.to_string());
            } else if let Some(branch) = field.strip_prefix("branch ") {
                let branch = branch.strip_prefix("refs/heads/").unwrap_or(branch);
                work.task = task_of(branch);
                work.branch = Some(branch.to_string());
            } else if field == "bare" {
                bare = true;
            } else if let Some(reason) = field.strip_prefix("prunable") {
                prunable = Some(reason.trim().to_string());
            }
        }
        if bare {
            continue;
        }
        if let Some(reason) = prunable {
            let why = if reason.is_empty() {
                "its folder is gone".to_string()
            } else {
                reason
            };
            work.not_recoverable.push(format!(
                "{why}: any uncommitted changes it held are not recoverable; `git worktree prune` clears the registration"
            ));
            out.push(work);
            continue;
        }
        inspect(&mut work, head.as_deref());
        out.push(work);
    }
    Ok(out)
}

fn inspect(work: &mut WorktreeWork, head: Option<&str>) {
    let dir = work.path.clone();
    if let Some(head) = head {
        if git(&dir, &["cat-file", "-e", &format!("{head}^{{commit}}")]).is_err() {
            work.not_recoverable.push(format!(
                "HEAD names {} but that commit is missing from the object store: not recoverable",
                &head[..head.len().min(12)]
            ));
            return;
        }
    }
    match git(
        &dir,
        &["status", "--porcelain=v1", "--untracked-files=normal"],
    ) {
        Ok(text) => work.uncommitted = text.lines().map(str::to_string).collect(),
        Err(error) => work
            .not_recoverable
            .push(format!("its status cannot be read ({error})")),
    }
    let log = |exclude: &[&str]| -> Result<Vec<String>, String> {
        let mut args = vec!["log", "--format=%h %s", "HEAD", "--not"];
        args.extend_from_slice(exclude);
        git(&dir, &args).map(|text| text.lines().map(str::to_string).collect())
    };
    let result = if work.branch.is_some() {
        log(&["--remotes"]).map(|commits| work.unpushed = commits)
    } else {
        log(&["--branches", "--remotes"]).map(|commits| work.no_branch = commits)
    };
    if let Err(error) = result {
        work.not_recoverable.push(format!(
            "its history cannot be walked, some objects may be missing ({error}): not recoverable"
        ));
    }
}

/// Render the inventory, grouped by task, worktrees with nothing to report
/// left out and counted.
#[must_use]
pub fn render(inventory: &[WorktreeWork]) -> crate::git::TextBlock {
    use std::collections::BTreeMap;
    let mut out = crate::git::TextBlock::new();
    let holding: Vec<&WorktreeWork> = inventory.iter().filter(|w| w.holds_work()).collect();
    let mut groups: BTreeMap<String, Vec<&WorktreeWork>> = BTreeMap::new();
    for work in &holding {
        groups
            .entry(work.task.clone().unwrap_or_else(|| "no task".into()))
            .or_default()
            .push(work);
    }
    let _ = writeln!(
        out,
        "work status: {} of {} worktrees hold work found nowhere else",
        holding.len(),
        inventory.len()
    );
    let list = |out: &mut crate::git::TextBlock, label: &str, items: &[String]| {
        if items.is_empty() {
            return;
        }
        let shown: Vec<&str> = items.iter().take(SHOWN).map(String::as_str).collect();
        let more = items.len().saturating_sub(SHOWN);
        let _ = writeln!(
            out,
            "    {label}: {} ({}{})",
            items.len(),
            crate::git::display_message(&shown.join("; ")),
            if more > 0 {
                format!("; {more} more")
            } else {
                String::new()
            }
        );
    };
    for (task, works) in groups {
        let _ = writeln!(out, "{task}");
        for work in works {
            let head = work
                .branch
                .as_deref()
                .map_or_else(|| "detached HEAD".to_string(), crate::git::display_message);
            let _ = writeln!(
                out,
                "  {} ({head})",
                crate::git::display_message(&work.path.display().to_string())
            );
            list(&mut out, "uncommitted", &work.uncommitted);
            list(&mut out, "unpushed commits", &work.unpushed);
            list(&mut out, "commits on no branch", &work.no_branch);
            for reason in &work.not_recoverable {
                let _ = writeln!(
                    out,
                    "    not recoverable: {}",
                    crate::git::display_message(reason)
                );
            }
        }
    }
    out
}
