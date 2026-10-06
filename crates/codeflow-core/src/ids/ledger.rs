//! The registry's history read as a ledger (SPC-013 R-7 to R-9, R-108,
//! R-109): the first addition of every path, the commits that break the
//! append-only rule, and the damage the tip carries now.
//!
//! Every commit on `codeflow/registry` must either add new `ids/` files and
//! nothing else, or be a typed restore that returns named files to the bytes
//! of their first addition. The root commit is judged by the same rule.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::entry::{Entry, Kind, RegId};
use super::git::{z_records, Git};
use super::IdsError;

/// The first addition of one registry path.
#[derive(Debug, Clone)]
pub struct FirstAdd {
    /// The commit that first added the path.
    pub commit: String,
    /// The blob it added.
    pub blob: String,
    /// The parsed entry, `None` when the file was not a valid entry.
    pub entry: Option<Entry>,
}

/// One commit that breaks the append-only rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub commit: String,
    pub message: String,
}

impl Finding {
    fn new(commit: &str, message: String) -> Finding {
        Finding {
            commit: commit.to_string(),
            message,
        }
    }
}

/// The registry history as a ledger.
#[derive(Debug, Clone, Default)]
pub struct Ledger {
    /// The tip read, `None` for an absent registry.
    pub tip: Option<String>,
    /// Commits in the history.
    pub commits: usize,
    first: BTreeMap<String, FirstAdd>,
    /// Commits that break the rule, oldest first.
    pub violations: Vec<Finding>,
    /// Current damage on the tip (R-9): a path ever added that is absent
    /// or differs from its first addition.
    pub damage: Vec<String>,
}

struct RawCommit {
    sha: String,
    parents: usize,
    subject: String,
    changes: Vec<Change>,
}

struct Change {
    mode: String,
    blob: String,
    status: char,
    path: String,
}

impl Ledger {
    /// Read the ledger of `rev`. An unresolvable `rev` reads as empty.
    ///
    /// # Errors
    ///
    /// Returns an error when git fails on an existing history.
    pub fn read(git: &Git, rev: &str) -> Result<Ledger, IdsError> {
        let Some(tip) = git.rev(rev) else {
            return Ok(Ledger::default());
        };
        let commits = raw_history(git, &tip)?;
        let added: Vec<String> = commits
            .iter()
            .flat_map(|commit| commit.changes.iter())
            .filter(|change| matches!(change.status, 'A' | 'M'))
            .filter(|change| RegId::from_registry_path(&change.path).is_some())
            .map(|change| change.blob.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let blobs = git.blobs(&added)?;
        let mut ledger = Ledger {
            tip: Some(tip.clone()),
            commits: commits.len(),
            ..Ledger::default()
        };
        for commit in &commits {
            ledger.apply(commit, &blobs);
        }
        let tree = git.tree(&tip, &[])?;
        let on_tip: BTreeMap<&str, &str> = tree
            .iter()
            .map(|(_, blob, path)| (path.as_str(), blob.as_str()))
            .collect();
        for (path, first) in &ledger.first {
            let Some(id) = RegId::from_registry_path(path) else {
                continue;
            };
            match on_tip.get(path.as_str()) {
                None => ledger.damage.push(format!(
                    "damaged: {path} (added in {}) is absent from the tip (R-9); the binding stands in history, repair with `codeflow ids restore {id}`",
                    short(&first.commit)
                )),
                Some(blob) if *blob != first.blob => ledger.damage.push(format!(
                    "damaged: {path} differs from its first addition in {} (R-9); repair with `codeflow ids restore {id}`",
                    short(&first.commit)
                )),
                Some(_) => {}
            }
        }
        Ok(ledger)
    }

    /// Judge one commit, then record every path it adds for the first time.
    /// The bookkeeping never depends on the verdict: a number a broken
    /// commit introduced (a counterfeit restore, a merge tree) stays used,
    /// so it can never be issued again (R-7, R-9).
    fn apply(&mut self, commit: &RawCommit, blobs: &std::collections::HashMap<String, Vec<u8>>) {
        let fresh: Vec<&Change> = commit
            .changes
            .iter()
            .filter(|change| change.status == 'A' && !self.first.contains_key(&change.path))
            .collect();
        self.judge(commit, blobs);
        for change in fresh {
            let entry = blobs
                .get(&change.blob)
                .and_then(|bytes| Entry::parse(&change.path, &String::from_utf8_lossy(bytes)).ok());
            self.first.insert(
                change.path.clone(),
                FirstAdd {
                    commit: commit.sha.clone(),
                    blob: change.blob.clone(),
                    entry,
                },
            );
        }
    }

    fn judge(&mut self, commit: &RawCommit, blobs: &std::collections::HashMap<String, Vec<u8>>) {
        let sha = commit.sha.as_str();
        if commit.parents > 1 {
            self.violations.push(Finding::new(
                sha,
                "merge commit on the registry (R-8)".to_string(),
            ));
            return;
        }
        if commit.changes.is_empty() {
            self.violations.push(Finding::new(
                sha,
                "commit changes nothing (R-8)".to_string(),
            ));
            return;
        }
        if let Some(named) = commit.subject.strip_prefix("restore: ") {
            let named: BTreeSet<&str> = named.split(' ').filter(|part| !part.is_empty()).collect();
            let problems = self.restore_problems(commit, &named, blobs);
            for problem in problems {
                self.violations.push(Finding::new(sha, problem));
            }
            return;
        }
        for change in &commit.changes {
            let path = change.path.as_str();
            match change.status {
                'D' => self
                    .violations
                    .push(Finding::new(sha, format!("deletes {path} (R-8)"))),
                'A' if self.first.contains_key(path) => {
                    let first = short(&self.first[path].commit).to_string();
                    self.violations.push(Finding::new(
                        sha,
                        format!("re-adds {path}, first added in {first}, outside a typed restore (R-8, R-108)"),
                    ));
                }
                'A' => self.check_addition(sha, change, blobs),
                status => self.violations.push(Finding::new(
                    sha,
                    format!("modifies {path} (status {status}) outside a typed restore (R-8)"),
                )),
            }
        }
    }

    /// Judge a new file an addition commit brings: an `ids/` path in mode
    /// 100644 holding a valid entry.
    fn check_addition(
        &mut self,
        sha: &str,
        change: &Change,
        blobs: &std::collections::HashMap<String, Vec<u8>>,
    ) {
        let path = change.path.as_str();
        if RegId::from_registry_path(path).is_none() {
            self.violations.push(Finding::new(
                sha,
                format!("adds {path}, which is not an ids/ file (R-8, R-109)"),
            ));
            return;
        }
        if change.mode != "100644" {
            self.violations.push(Finding::new(
                sha,
                format!("adds {path} with mode {} (R-8)", change.mode),
            ));
        }
        let text = blobs
            .get(&change.blob)
            .map(|bytes| String::from_utf8_lossy(bytes).to_string())
            .unwrap_or_default();
        if let Err(problems) = Entry::parse(path, &text) {
            for problem in problems {
                self.violations.push(Finding::new(sha, problem));
            }
        }
    }

    fn restore_problems(
        &self,
        commit: &RawCommit,
        named: &BTreeSet<&str>,
        blobs: &std::collections::HashMap<String, Vec<u8>>,
    ) -> Vec<String> {
        let mut problems = Vec::new();
        for change in &commit.changes {
            let path = change.path.as_str();
            let Some(first) = self.first.get(path) else {
                problems.push(format!(
                    "restore touches {path}, which was never added (R-108)"
                ));
                continue;
            };
            let id = RegId::from_registry_path(path).map(|id| id.to_string());
            if !id.as_deref().is_some_and(|id| named.contains(id)) {
                problems.push(format!(
                    "restore touches {path}, which its subject does not name (R-108)"
                ));
            }
            if !matches!(change.status, 'A' | 'M') {
                problems.push(format!(
                    "restore removes or retypes {path} (status {}) (R-108)",
                    change.status
                ));
                continue;
            }
            if change.blob != first.blob {
                let text = blobs
                    .get(&change.blob)
                    .map(|bytes| String::from_utf8_lossy(bytes).to_string())
                    .unwrap_or_default();
                let rebinds = match (Entry::parse(path, &text).ok(), first.entry.as_ref()) {
                    (Some(new), Some(old)) => new.uid != old.uid,
                    _ => false,
                };
                if rebinds {
                    problems.push(format!(
                        "restore would bind {} to another uid (R-108)",
                        id.unwrap_or_else(|| path.to_string())
                    ));
                } else {
                    problems.push(format!(
                        "restore does not return {path} to the bytes of its first addition in {} (R-108)",
                        short(&first.commit)
                    ));
                }
            }
        }
        problems
    }

    /// Whether the tip is currently damaged.
    #[must_use]
    pub fn is_damaged(&self) -> bool {
        !self.damage.is_empty()
    }

    /// Whether the registry exists.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.tip.is_some()
    }

    /// The first-added entry for `id`: the binding, whatever the tip holds.
    #[must_use]
    pub fn entry(&self, id: &RegId) -> Option<&Entry> {
        self.first
            .get(&id.registry_path())
            .and_then(|first| first.entry.as_ref())
    }

    /// Whether `id` was ever added, valid or not. A held number is never
    /// reissued.
    #[must_use]
    pub fn holds(&self, id: &RegId) -> bool {
        self.first.contains_key(&id.registry_path())
    }

    /// The first addition of `id`.
    #[must_use]
    pub fn first_add(&self, id: &RegId) -> Option<&FirstAdd> {
        self.first.get(&id.registry_path())
    }

    /// Every id ever added.
    pub fn ids(&self) -> impl Iterator<Item = RegId> + '_ {
        self.first
            .keys()
            .filter_map(|path| RegId::from_registry_path(path))
    }

    /// The ids bound to `uid`.
    #[must_use]
    pub fn ids_for_uid(&self, uid: &str) -> Vec<RegId> {
        self.first
            .values()
            .filter_map(|first| first.entry.as_ref())
            .filter(|entry| entry.uid == uid)
            .map(|entry| entry.id.clone())
            .collect()
    }

    /// The highest canonical sequence ever added for `kind` (R-7, R-111:
    /// legacy ids never count).
    #[must_use]
    pub fn max_seq(&self, kind: Kind) -> u64 {
        self.ids()
            .filter(|id| id.kind() == kind)
            .filter_map(|id| id.seq())
            .max()
            .unwrap_or(0)
    }

    /// The violations in commits of `rev`'s history that `exclude` does not
    /// reach: the pushed range when `exclude` is the remote tip.
    ///
    /// # Errors
    ///
    /// Returns an error when git fails.
    pub fn range_violations(
        &self,
        git: &Git,
        exclude: Option<&str>,
    ) -> Result<Vec<Finding>, IdsError> {
        let Some(tip) = &self.tip else {
            return Ok(Vec::new());
        };
        let Some(exclude) = exclude.filter(|sha| git.rev(sha).is_some()) else {
            return Ok(self.violations.clone());
        };
        let in_range: HashSet<String> = git
            .run(&["rev-list", tip, &format!("^{exclude}")])?
            .split_terminator('\n')
            .map(str::to_string)
            .collect();
        Ok(self
            .violations
            .iter()
            .filter(|finding| in_range.contains(&finding.commit))
            .cloned()
            .collect())
    }
}

/// A short sha for messages.
#[must_use]
pub fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

/// The registry history, oldest first, with each commit's changes as
/// NUL-delimited raw records. A merge lists its changes against its first
/// parent, so a file only the merge tree introduces is still seen.
fn raw_history(git: &Git, tip: &str) -> Result<Vec<RawCommit>, IdsError> {
    let log = git.run_bytes(&[
        "log",
        "--reverse",
        "--topo-order",
        "--no-renames",
        "--root",
        "--raw",
        "-z",
        "--diff-merges=first-parent",
        "--no-abbrev",
        "--format=%x1e%H%x1f%P%x1f%s",
        tip,
    ])?;
    let mut commits = Vec::new();
    for record in z_records(&log) {
        let mut fields = record.iter().map(String::as_str);
        let header = fields.next().unwrap_or_default();
        let mut parts = header.split('\x1f');
        let sha = parts.next().unwrap_or_default().to_string();
        let parents = parts
            .next()
            .unwrap_or_default()
            .split(' ')
            .filter(|part| !part.is_empty())
            .count();
        let subject = parts.next().unwrap_or_default().to_string();
        let mut changes = Vec::new();
        while let Some(meta) = fields.next() {
            let Some(meta) = meta.strip_prefix(':') else {
                continue;
            };
            let Some(path) = fields.next() else {
                break;
            };
            let meta: Vec<&str> = meta.split(' ').filter(|part| !part.is_empty()).collect();
            let [_old_mode, mode, _old_blob, blob, status] = meta.as_slice() else {
                continue;
            };
            changes.push(Change {
                mode: (*mode).to_string(),
                blob: (*blob).to_string(),
                status: status.chars().next().unwrap_or('?'),
                path: path.to_string(),
            });
        }
        commits.push(RawCommit {
            sha,
            parents,
            subject,
            changes,
        });
    }
    Ok(commits)
}
