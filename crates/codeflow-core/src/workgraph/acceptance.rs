//! Acceptance bound to the reviewed commit (SPC-013 R-52, R-53, R-60).
//!
//! The structural rules of an acceptance block live in
//! [`super::record_text`]; this module adds what needs git: the block names
//! the commit that was reviewed, and nothing but the record's status and
//! Closeout changed after it. Task landings and release imports identify
//! the source of that same reviewed span ([`bind_completion`]); `task status
//! complete` and `codeflow ci` share that judge; a waiver names the planning amendment on the
//! target or in its own reviewed range that changed that criterion. The
//! task may amend its own criteria until its completion lands on the
//! target; a landed task's criteria stay frozen when it is reopened. A
//! range touching the
//! adopter-facing path set belongs to a task with a journey criterion.
//!
//! The checker proves structure and binding only ([`SCOPE_NOTE`]). It does
//! not prove the evidence is honest, and the review reference is
//! recheckable, not authenticated.

use crate::workgraph::history;

use git2::{Oid, Repository};

use super::classify::is_planning_path;
use super::lifecycle::{Graph, RecordView};
use super::record_text::{
    acceptance_blocks, frontmatter_len, hidden_tag_message, outcome_word, scan_record,
    section_span, AcceptanceBlock, Criterion, CriterionKey,
};
use super::work_start::RecordKind;

/// The acceptance block names the reviewed commit, and waivers name their
/// amendment (R-60). Level: `git.work_records`.
pub const BINDING_RULE: &str = "work.acceptance_binding";
/// Criteria changes outside the own-task or planning allowance, including
/// changes to reopened criteria (R-52). Always blocks.
pub const FROZEN_RULE: &str = "work.criteria_frozen";
/// An imported record lacks a verified source authority. Always blocks.
pub const IMPORT_RULE: &str = "work.import_authority";
/// A criterion a task pull request removes from its own record (TSK-451):
/// a note whose message is `<task> <criterion>`, which `codeflow ci` holds
/// against the pull request body, since removing one without a reason there
/// is refused.
pub const REMOVAL_RULE: &str = "work.criteria_removal";
/// The selected criteria version, distinct from the run's policy authority.
pub const AUTHORITY_NOTE: &str = "work.criteria_authority";
/// An adopter-facing range belongs to a task with a journey criterion, and a
/// leaf serving the epic's journey says what ran (R-53). Level:
/// `git.work_records`.
pub const JOURNEY_RULE: &str = "work.journey_criterion";

/// What every report of these rules says about itself (R-60).
pub const SCOPE_NOTE: &str = "the acceptance check proves structure and binding only: it does not \
     prove that the evidence is honest, and the review reference is recheckable, not authenticated";

/// One finding of these rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub rule: &'static str,
    pub message: String,
    /// Informational evidence, never a refusal.
    pub note: bool,
    /// The record path of the epic whose own acceptance block this finding
    /// binds, so its remedy is the epic's, never a task's.
    pub epic_record: Option<String>,
}

pub(super) fn finding(rule: &'static str, message: String) -> Finding {
    Finding {
        rule,
        message,
        note: false,
        epic_record: None,
    }
}

/// The active (not superseded) acceptance block of a record, when exactly
/// one parses. Structure is judged elsewhere; binding needs a parsed block.
pub(super) fn active_block(record: &RecordView) -> Option<AcceptanceBlock> {
    let mut blocks = record
        .active_blocks()
        .into_iter()
        .filter_map(|block| block.parsed.ok());
    let block = blocks.next()?;
    blocks.next().is_none().then_some(block)
}

/// A commit named by its object id, never by a ref name: a branch named like
/// the id cannot stand in for the reviewed commit or the amendment.
pub(crate) fn commit_of(repo: &Repository, value: &str) -> Option<Oid> {
    super::work_start::commit_by_object_id(repo, value)
        .ok()
        .map(|commit| commit.id())
}

fn is_first_parent_ancestor(repo: &Repository, ancestor: Oid, mut tip: Oid) -> bool {
    let _judgment = history::judgment();
    loop {
        if tip == ancestor {
            return true;
        }
        let Ok(parent) = repo
            .find_commit(tip)
            .and_then(|commit| history::parent_id(repo, &commit, 0))
        else {
            return false;
        };
        tip = parent;
    }
}

/// Whether `ancestor` is `of` or one of its ancestors; an error when the
/// history cannot be read, never `false`.
pub(super) fn is_ancestor_or_same(
    repo: &Repository,
    ancestor: Oid,
    of: Oid,
) -> Result<bool, String> {
    history::check(repo).map_err(|error| error.to_string())?;
    if ancestor == of {
        return Ok(true);
    }
    history::descendant_of(repo, of, ancestor)
        .map_err(|error| format!("cannot read ancestry from {of} to {ancestor}: {error}"))
}

/// A missing common ancestor is absence only when both histories can be read.
pub(crate) fn common_base(repo: &Repository, left: Oid, right: Oid) -> Result<Option<Oid>, String> {
    let unreadable = |error: git2::Error| {
        format!("cannot read the common ancestry of {left} and {right}: {error}")
    };
    match history::merge_base(repo, left, right) {
        Ok(base) => Ok(Some(base)),
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            let mut walk = history::revwalk(repo).map_err(unreadable)?;
            walk.push(left).map_err(unreadable)?;
            walk.push(right).map_err(unreadable)?;
            for commit in walk {
                commit.map_err(unreadable)?;
            }
            Ok(None)
        }
        Err(error) => Err(unreadable(error)),
    }
}

pub(super) fn blob_at(
    repo: &Repository,
    commit: Oid,
    path: &str,
) -> Result<Option<String>, String> {
    let tree = repo
        .find_commit(commit)
        .and_then(|commit| commit.tree())
        .map_err(|error| format!("cannot read {path} at {commit}: {error}"))?;
    let entry = match tree.get_path(std::path::Path::new(path)) {
        Ok(entry) => entry,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read {path} at {commit}: {error}")),
    };
    let blob = repo
        .find_blob(entry.id())
        .map_err(|error| format!("cannot read {path} at {commit}: {error}"))?;
    let text = std::str::from_utf8(blob.content())
        .map_err(|error| format!("cannot decode {path} at {commit}: {error}"))?;
    Ok(Some(text.to_string()))
}

/// The UTF-8 text of `path` at the commit `revision` names in the
/// repository at or above `root`; `None` only when the commit's tree has no
/// such path.
///
/// # Errors
///
/// When the repository, the revision, the tree or the blob cannot be read,
/// or the blob is not UTF-8 (issue 79): never read as an absent file.
pub fn text_at(
    root: &std::path::Path,
    revision: &str,
    path: &str,
) -> Result<Option<String>, String> {
    let repo = crate::hooks::repo::open(root)?
        .ok_or_else(|| format!("cannot read {path} at {revision}: no repository"))?;
    let commit = repo
        .revparse_single(revision)
        .and_then(|object| object.peel_to_commit())
        .map_err(|error| format!("cannot read {path} at {revision}: {error}"))?;
    blob_at(&repo, commit.id(), path)
}

fn record_at(
    repo: &Repository,
    commit: Oid,
    record: &RecordView,
) -> Result<Option<RecordView>, String> {
    blob_at(repo, commit, &record.path)?
        .map(|content| RecordView::parse(record.kind, &record.path, &content))
        .transpose()
}

fn first_parent(repo: &Repository, at: Oid) -> Result<Option<Oid>, String> {
    let commit = repo
        .find_commit(at)
        .map_err(|error| format!("cannot read commit {at}: {error}"))?;
    Ok(history::parents(repo, &commit)
        .map_err(|error| error.to_string())?
        .into_iter()
        .next())
}

/// The span of the `## Blocker` section when it has exactly the shape
/// `task status <id> blocked` writes (TSK-340, issue #198): the heading, one
/// empty line, the `reason`, `owner` and `revisit` items with a value each,
/// and nothing after them but empty lines. Any other text in that section,
/// another item, a paragraph or a criterion, keeps it reviewed.
fn generated_blocker(
    lines: &[&str],
    scanned: &[super::record_text::ScannedLine],
) -> Option<(usize, usize)> {
    let (start, end) = section_span(scanned, "## Blocker")?;
    let section = &lines[start..end];
    let item = |at: usize, key: &str| {
        section
            .get(at)
            .and_then(|line| line.strip_prefix(key))
            .is_some_and(|value| !value.is_empty() && !value.contains('\r'))
    };
    let generated = section.first() == Some(&"## Blocker")
        && section.get(1) == Some(&"")
        && item(2, "- reason: ")
        && item(3, "- owner: ")
        && item(4, "- revisit: ")
        && section.iter().skip(5).all(|line| line.is_empty())
        && scanned[start..end]
            .iter()
            .all(|line| line.kind == super::record_text::LineKind::Text);
    generated.then_some((start, end))
}

/// A record's text without its frontmatter `status:` field, its
/// `## Closeout` section and a `## Blocker` section of the generated shape
/// ([`generated_blocker`]): what must not change after the reviewed commit.
/// The Closeout is the section the record parser finds (the one the
/// acceptance block is read from), so a heading inside a comment, an HTML
/// block or a fence neither ends nor opens it.
fn reviewed_part(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let frontmatter = frontmatter_len(&lines);
    let scanned = scan_record(&lines);
    let closeout = section_span(&scanned, "## Closeout");
    let blocker = generated_blocker(&lines, &scanned);
    let mut reviewed: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(index, line)| {
            let status_field = *index > 0 && *index < frontmatter && line.starts_with("status:");
            let within = |span: Option<(usize, usize)>| {
                span.is_some_and(|(start, end)| (start..end).contains(index))
            };
            !status_field && !within(closeout) && !within(blocker)
        })
        .map(|(_, line)| *line)
        .collect();
    // Empty logical lines are Markdown section spacing, including the blank
    // separator inserted before Closeout. Physical LF/CRLF was removed above;
    // a remaining CR or any other content byte is never removed here.
    while reviewed.last().is_some_and(|line| line.is_empty()) {
        reviewed.pop();
    }
    reviewed.join("\n")
}

/// Where a completion lands: the commit that introduced the active block,
/// or the working tree `task status complete` is about to commit on top of
/// `head`, with every path it changes.
#[derive(Debug, Clone, Copy)]
pub enum Landing<'a> {
    /// A commit of the range (C in the binding rule).
    Commit(Oid),
    /// The working tree over `head`; `changed` lists staged, unstaged and
    /// untracked paths, the record's own included.
    Worktree {
        head: Oid,
        changed: &'a [crate::git::GitName],
    },
}

impl Landing<'_> {
    /// The commit the completion sits on: C itself, or the working tree's
    /// `HEAD`.
    fn commit(&self) -> Oid {
        match *self {
            Self::Commit(oid) | Self::Worktree { head: oid, .. } => oid,
        }
    }
}

/// Which transport can carry a reviewed task onto the completion's line.
/// Release imports are attributed by `release_line` before entering this judge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Direct work, including every direct release-line change.
    Direct,
    /// An ordinary task line may carry review through a clean task landing.
    TaskLanding,
}

/// The two kinds of target authority a run hands the binding (TSK-220),
/// kept as separate types in their own module so neither can be built from
/// the other or from points the range itself supplies.
mod authority {
    use git2::Oid;

    /// The fixed target tips one run is judged against (TSK-220): the base
    /// `codeflow ci` is given (in hosted CI, the pull request's base) and the
    /// pre-push hook's candidate authority, or, in the status verb, the task's
    /// target as `work start` anchors it. They come from the run, never from
    /// the range under judgement, and they are the only source of both sets
    /// below. An imported record gets a separate source judgment only after
    /// `imported_records` proves its home landing; the receiving head never
    /// supplies that authority.
    #[derive(Clone, Debug, Default)]
    pub struct RunBases(Vec<Oid>);

    impl RunBases {
        /// The run's fixed target tips, deduplicated in order.
        #[must_use]
        pub fn new(tips: impl IntoIterator<Item = Oid>) -> Self {
            let mut bases: Vec<Oid> = Vec::new();
            for tip in tips {
                if !bases.contains(&tip) {
                    bases.push(tip);
                }
            }
            Self(bases)
        }

        /// The tips whose first-parent lines may carry a clean merge after a
        /// review. `with_own_line` may add a planning or line range's own
        /// head.
        #[must_use]
        pub fn stacking(&self) -> StackingTips {
            StackingTips(self.0.clone(), Vec::new())
        }

        /// The bases that alone may supply a reopened task's criteria.
        #[must_use]
        pub fn criteria(&self) -> CriteriaBases {
            CriteriaBases {
                bases: self.0.clone(),
                imported_home: None,
                judged_lines: Vec::new(),
            }
        }
    }

    /// Stacking authority: target tips whose first-parent lines may carry a
    /// clean merge after a review. [`RunBases::stacking`] builds it;
    /// `with_own_line` or a verified import's home extends it. It never
    /// supplies criteria.
    #[derive(Clone, Debug, Default)]
    pub struct StackingTips(Vec<Oid>, Vec<Oid>);

    impl StackingTips {
        pub(super) fn tips(&self) -> &[Oid] {
            &self.0
        }

        /// The receiving lines a task landing merge may sit on: these tips
        /// and any verified source line a caller added for landings only
        /// ([`Self::with_landing_line`]), which never stacks a clean merge.
        pub(super) fn landing_tips(&self) -> Vec<Oid> {
            self.0.iter().chain(&self.1).copied().collect()
        }

        /// These tips, and `tip` as a receiving line for task landings only:
        /// a verified source line (a release import's attributed source).
        /// It adds no clean-merge stacking authority.
        #[must_use]
        pub fn with_landing_line(mut self, tip: Oid) -> Self {
            if !self.1.contains(&tip) {
                self.1.push(tip);
            }
            self
        }

        /// These tips and a planning or line range's own head, for a task that
        /// targets that line (`with_own_line`).
        pub(super) fn with_line_head(mut self, head: Oid) -> Self {
            if !self.0.contains(&head) {
                self.0.push(head);
            }
            self
        }
    }

    /// Criteria authority: the run's fixed bases, the only points besides the
    /// range anchor allowed to supply a reopened task's criteria. Only
    /// [`RunBases::criteria`] builds it, so a range's own line head, which
    /// stacks merges, can never join it.
    #[derive(Clone, Debug, Default)]
    pub struct CriteriaBases {
        bases: Vec<Oid>,
        imported_home: Option<(String, Oid)>,
        judged_lines: Vec<(String, Oid)>,
    }

    impl CriteriaBases {
        pub(super) fn bases(&self) -> &[Oid] {
            &self.bases
        }

        /// The historical home tip already proved by an import landing.
        /// It substitutes only for that missing ref, never another target
        /// named by a retargeted record, and adds no criteria base.
        pub(super) fn with_imported_home(mut self, home: &str, tip: Oid) -> Self {
            self.imported_home = Some((home.to_string(), tip));
            self
        }

        pub(super) fn imported_home(&self, name: &str) -> Option<Oid> {
            self.imported_home
                .as_ref()
                .filter(|(home, _)| home == super::super::work_start::logical_target(name))
                .map(|(_, tip)| *tip)
        }

        /// A verified epic line this run judges as part of one pull request
        /// into its target, with the range head (TSK-433): the range's own
        /// line, or a verified line that joined it. A tip of such a line the
        /// head contains is the range under judgment, so it is no outside
        /// evidence of a landing; it adds no criteria base.
        pub(super) fn with_judged_line(mut self, line: &str, head: Oid) -> Self {
            self.judged_lines.push((
                super::super::work_start::logical_target(line).to_string(),
                head,
            ));
            self
        }

        /// The range head, when `name` is a judged line ([`Self::with_judged_line`]).
        pub(super) fn judged_line_head(&self, name: &str) -> Option<Oid> {
            let name = super::super::work_start::logical_target(name);
            self.judged_lines
                .iter()
                .find(|(line, _)| line == name)
                .map(|(_, head)| *head)
        }
    }
}
pub use authority::{CriteriaBases, RunBases, StackingTips};

mod imports;
mod receiving_merges;
pub(crate) use imports::clear_cache as clear_import_cache;
pub use imports::{imported_record_paths, imported_records, ImportedRecord};

/// The source and completion endpoints established by the binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompletionProof {
    /// The commit named by the acceptance review.
    pub reviewed: Oid,
    /// The clean merge that carried the reviewed task, absent for direct work.
    pub landing_merge: Option<Oid>,
    /// The reviewed span's end: the landing's second parent, or the direct head.
    pub span_head: Oid,
    /// The completing commit, or HEAD when checking an uncommitted completion.
    pub completion_tail: Oid,
}

/// Findings and, only when every binding check passes, its source proof.
#[derive(Debug, Default)]
pub struct BindingResult {
    /// Refusals from the binding, waivers and leaf journey checks.
    pub findings: Vec<Finding>,
    /// Absent for a refused binding or a record with no active acceptance block.
    pub proof: Option<CompletionProof>,
}

impl From<Vec<Finding>> for BindingResult {
    fn from(findings: Vec<Finding>) -> Self {
        Self {
            findings,
            proof: None,
        }
    }
}

/// Bind a completion to its reviewed source. Only this record's status and
/// Closeout may differ in the reviewed segment. A clean task landing can
/// transport that segment onto a line; a reopening range reviews its own
/// work and takes no landing's review. `base` pins the range's anchored old review when supplied.
/// A waiver names the criterion's record-only amendment on its target;
/// `bind_completion_with_amendment` adds the task's own range.
#[must_use]
pub fn bind_completion(
    repo: &Repository,
    task: &RecordView,
    graph: &Graph,
    landing: Landing<'_>,
    default_target: Option<Oid>,
    transport: Transport,
    base: Option<Oid>,
) -> BindingResult {
    let run = RunBases::new(default_target);
    bind_completion_with_amendment(
        repo,
        task,
        graph,
        landing,
        default_target,
        transport,
        base,
        None,
        &run.stacking(),
        &run.criteria(),
    )
}

/// Whether the checked-out branch is the task's declared integration target.
/// Both must be known: two unknowns (a HEAD name that is not valid UTF-8 and no
/// declared target) are not a match.
fn head_is_declared_target(repo: &Repository, task: &RecordView) -> bool {
    repo.head().ok().is_some_and(|head| {
        task.integration_target.as_deref().is_some_and(|declared| {
            crate::git::name::reference_shorthand(&head).bytes() == declared.as_bytes()
        })
    })
}

fn task_reopen_history(
    repo: &Repository,
    task: &RecordView,
    landing: Landing<'_>,
    anchor: Option<Oid>,
) -> Result<std::collections::BTreeSet<String>, String> {
    match anchor {
        Some(anchor) => super::lifecycle::reopened_in_range(
            repo,
            &anchor.to_string(),
            Some(&landing.commit().to_string()),
            &Graph::default().with(task.clone()),
        ),
        None => Ok(std::collections::BTreeSet::new()),
    }
}

/// [`bind_completion`] with the own-task waiver allowance (TSK-184): in the
/// task's own pull request, before the task was ever complete on its
/// target, a waiver may also name a record-only amendment commit of this
/// record in that range, after `own_range_base` and strictly before the
/// reviewed commit. Once the pull request lands, that commit is on the
/// target, so a line and the release judge accept the same waiver under
/// the target rule. `None`, or a range that reopens a landed completion,
/// keeps the target rule alone; a range that reopens only a completion it
/// made itself, before any completion landed, keeps the allowance (TSK-340).
///
/// After its review, a task pull request's chain may stack a clean merge
/// only when the merge brings a commit on the first-parent line of one of
/// the `stacking` tips; a reopened task's criteria come only from the range
/// anchor and the `criteria` bases (TSK-220). Both derive from the run's
/// [`RunBases`], never from a local branch or its upstream configuration.
#[allow(clippy::too_many_arguments)] // bind_completion's inputs, the own-range base and the two authorities.
#[allow(clippy::too_many_lines)] // Keep reopened-range, review and waiver refusals in their judgment order.
pub(crate) fn bind_completion_with_amendment(
    repo: &Repository,
    task: &RecordView,
    graph: &Graph,
    landing: Landing<'_>,
    default_target: Option<Oid>,
    transport: Transport,
    base: Option<Oid>,
    own_range_base: Option<Oid>,
    stacking: &StackingTips,
    criteria: &CriteriaBases,
) -> BindingResult {
    let Some(block) = active_block(task) else {
        return BindingResult::default();
    };
    let target = match task_target(repo, task, default_target) {
        Ok(target) => target,
        Err(error) => return vec![finding(BINDING_RULE, error)].into(),
    };
    // On the target itself there is no PR range to anchor. The status verb
    // still checks the transition and reviewed span, including uncommitted
    // changes; a named fix branch must instead review inside its range.
    let anchor = match (base, target) {
        (Some(base), _) => Some(base),
        (None, Some(tip)) => match common_base(repo, tip, landing.commit()) {
            Ok(anchor) => anchor,
            Err(error) => return vec![finding(BINDING_RULE, error)].into(),
        },
        (None, None) => None,
    };
    let on_target = base.is_none()
        && matches!(landing, Landing::Worktree { .. })
        && head_is_declared_target(repo, task);
    // A completion reopened before this range is recovered from the history
    // below. When that completion is on the target at the anchored base, a
    // separate pull request reopened it (R-119 keeps that valid): the range
    // does not reopen the task, and the fix lands as any task does, so a
    // task landing may carry its review. Otherwise this range completed and
    // reopened the task itself.
    let reopened_history = match task_reopen_history(repo, task, landing, anchor) {
        Ok(history) => history,
        Err(error) => return vec![finding(BINDING_RULE, error)].into(),
    };
    let mut recovered = false;
    let mut planned = false;
    let mut unreadable = None;
    let reopen = (!on_target).then_some(anchor).flatten().and_then(|anchor| {
        // By identity, wherever the anchored target holds the record
        // (TSK-220); unreadable falls to the recovery below, which keeps
        // the completed criteria.
        let uid = record_uid(&task.content);
        let Presence::Present(old) =
            presence_at(repo, anchor, &task.id, uid.as_deref(), &own_file_name(task))
        else {
            return None;
        };
        let reopened = old.status == "complete"
            && (matches!(landing, Landing::Worktree { .. })
                || super::lifecycle::is_recompletion(Some(&old), task)
                || reopened_history.contains(&task.id));
        reopened.then_some((anchor, *old))
    });
    // A failed recovery is a finding, never a range with no reopen.
    let reopen = match reopen {
        Some(found) => Some(found),
        None => match recovered_completion(repo, task, &block, landing, anchor, criteria) {
            Ok(Some(found)) => {
                unreadable = found.refusal;
                recovered = true;
                planned = found.planned;
                Some((found.at, found.old))
            }
            Ok(None) => None,
            Err(error) => return vec![finding(BINDING_RULE, error)].into(),
        },
    };
    let reopen_before_range = match reopen.as_ref().zip(anchor) {
        Some(((at, _), base)) => match is_ancestor_or_same(repo, *at, base) {
            Ok(before) => before,
            Err(error) => return vec![finding(BINDING_RULE, error)].into(),
        },
        None => false,
    };
    // A reopening range reviews its own work: no task landing carries a
    // review into it (R-60). Its criteria and its own-range waiver route
    // follow whether the reopened completion landed (issue #67, TSK-340).
    let reopens_range = reopen
        .as_ref()
        .is_some_and(|_| !recovered || !reopen_before_range);
    let mut proof = None;
    let mut findings = Vec::new();
    findings.extend(unreadable.map(|message| finding(BINDING_RULE, message)));
    if let Some((_, old)) = &reopen {
        findings.extend(
            super::lifecycle::reopen_problems(Some(old), task)
                .into_iter()
                .map(|message| finding(BINDING_RULE, format!("{}: {message}", task.id))),
        );
    }
    let mut bind = |message: String| findings.push(finding(BINDING_RULE, message));
    match commit_of(repo, &block.reviewed) {
        None => bind(format!(
            "{}: reviewed commit {} is not in this repository",
            task.id, block.reviewed
        )),
        Some(reviewed) => {
            // The review lies after the reopened completion. One exception:
            // a completion made and reopened inside this range (not on the
            // target at the range base) stood last at `at`, and a review of
            // `at` covers every change the range made before the reopen;
            // the binding below still refuses any later change but this
            // record's status and Closeout (AC-2).
            let completed_in_range = recovered && anchor.is_some() && !reopen_before_range;
            // A reopening task pull request that merged its target after
            // the review has moved its anchor past the review. Its fix range
            // then starts where the review left the target (TSK-340): the
            // merge-base of the review and the anchor, which must hold the
            // very completion the anchor holds; the stacking walk below
            // judges every commit after the review.
            let left = match (reopen.as_ref(), transport) {
                (Some((at, old)), Transport::TaskLanding) if !recovered => {
                    match reopen_left_at(repo, task, old, *at, reviewed) {
                        Ok(left) => Some(left),
                        Err(error) => {
                            bind(format!("{}: {error}", task.id));
                            return findings.into();
                        }
                    }
                }
                (Some((at, _)), _) => Some(*at),
                (None, _) => None,
            };
            let review_before_reopen = match left {
                Some(at) => match is_ancestor_or_same(repo, at, reviewed) {
                    Ok(contains) => (reviewed == at && !completed_in_range) || !contains,
                    Err(error) => {
                        bind(format!("{}: {error}", task.id));
                        return findings.into();
                    }
                },
                None => false,
            };
            let problem = if review_before_reopen {
                Some(format!("a reopened task's reviewed commit {reviewed} must lie inside the fix range, after its anchored base"))
            } else {
                // A reopening range reviews its own work: no earlier task
                // landing carries a review into it. Its completion span
                // (TSK-340) still ends where an ordinary task pull request's
                // does, so after its review it may take the clean merges of
                // its target that any task pull request may.
                let carried = match (reopens_range, transport) {
                    (false, transport) => Carried::Transport(transport),
                    (true, Transport::TaskLanding) => Carried::ReopenedStack,
                    (true, Transport::Direct) => Carried::Transport(Transport::Direct),
                };
                // Receiving authority is the run's pinned bases and verified
                // line heads only: a target the record declares, resolved
                // through a ref, never adds one (TSK-340 review round 4).
                match binding_source(
                    repo,
                    task,
                    landing,
                    reviewed,
                    carried,
                    stacking,
                    default_target,
                    anchor,
                ) {
                    Ok(bound) => {
                        proof = Some(bound);
                        None
                    }
                    Err(problem) => Some(problem),
                }
            };
            if let Some(problem) = problem {
                bind(format!("{}: {problem}", task.id));
            }
        }
    }
    // The own-range waiver route follows the completion span (TSK-340,
    // issue #278): a range that reopens only a completion it made itself,
    // before any completion of the task landed, is still the task's own
    // pull request and keeps the route, as it keeps its criteria (R-119).
    // A reopen of a landed completion keeps its criteria frozen, so its
    // waiver names a planning amendment on the target.
    let own_route = if !reopens_range || (recovered && !reopen_before_range && planned) {
        own_range_base.map_or(OwnRange::None, OwnRange::Base)
    } else {
        OwnRange::LandedReopen
    };
    match archive_problem(
        repo,
        task,
        // The anchor's completion when the range reopened it; a completion
        // the range made and reopened itself is in its own history.
        reopen.as_ref().filter(|_| !recovered).map(|(_, old)| old),
        anchor,
        landing,
    ) {
        Ok(None) => {}
        Ok(Some(problem)) => {
            bind(format!("{}: {problem}", task.id));
        }
        Err(error) => bind(format!("{}: {error}", task.id)),
    }
    // "On the target" means on a receiving line the run pins (its bases,
    // a verified line head, a release import's attributed source), never a
    // target the record declares resolved through a ref (TSK-340 review
    // round 4).
    let pinned = stacking.landing_tips();
    for (id, result) in &block.criteria {
        if result.outcome == "waived" {
            if let Some(problem) = waiver_problem(
                repo,
                task,
                id,
                &result.evidence,
                landing,
                &pinned,
                own_route,
            ) {
                bind(format!("{}: {id} waiver {problem}", task.id));
            }
        }
    }
    findings.extend(leaf_journey(task, graph, &block));
    BindingResult {
        proof: proof.filter(|_| findings.is_empty()),
        findings,
    }
}

/// Why the record's archive is not one this history made, if it is not
/// (TSK-340). The blocks the reference record archived (the reopened
/// completion, or the record at the anchor) stay byte for byte; the first
/// block the range archives is the reference's active one, which
/// `reopen_problems` checks; and the archived blocks after it are exactly
/// the completions the range's history made, in the order it made them,
/// none missing and none added, each with real provenance
/// ([`historical_binding`]). A dropped, invented or reordered completion is
/// refused.
fn archive_problem(
    repo: &Repository,
    task: &RecordView,
    reopened: Option<&RecordView>,
    anchor: Option<Oid>,
    landing: Landing<'_>,
) -> Result<Option<String>, String> {
    let archive = task.superseded_blocks();
    if archive.is_empty() {
        return Ok(None);
    }
    let at_anchor = match (reopened, anchor) {
        (None, Some(anchor)) => record_at(repo, anchor, task)?,
        _ => None,
    };
    let reference = reopened.or(at_anchor.as_ref());
    let kept = reference
        .map(RecordView::superseded_blocks)
        .unwrap_or_default();
    if archive.len() < kept.len()
        || kept
            .iter()
            .zip(&archive)
            .any(|(old, new)| old.inner != new.inner)
    {
        // A validated redaction is judged by its own rule; reopen_problems
        // names the reordered or rewritten archive.
        if reopened.is_none()
            && reference.is_none_or(|old| super::redaction::preserves(old, task).is_err())
        {
            return Ok(Some(
                "the `acceptance_superseded:` blocks the anchored record archived stay unchanged and first".into(),
            ));
        }
        return Ok(None);
    }
    let normal = |block: &AcceptanceBlock| {
        let mut block = block.clone();
        block.superseded = false;
        block.reason = None;
        block
    };
    let mut tail = Vec::new();
    for block in &archive[kept.len()..] {
        let Ok(parsed) = &block.parsed else {
            return Ok(Some("an archived acceptance block does not parse".into()));
        };
        tail.push(normal(parsed));
    }
    let mut made = completions_made(repo, task, anchor, landing)?;
    // The current completion is never archived: it is the last occurrence
    // when the record never left `complete` after it and holds the same
    // completion. Only that one occurrence is set aside.
    let current = active_block(task);
    if made.last().is_some_and(|last| {
        !last.reopened_after
            && Some(&last.block) == current.as_ref()
            && record_at(repo, last.at, task)
                .ok()
                .flatten()
                .is_some_and(|then| then.superseded_blocks() == archive)
    }) {
        made.pop();
    }
    // The first block the range archives may be the completion the
    // reference held before this range (at the anchor): `reopen_problems`
    // checks it against that record, and the range's history does not hold
    // it. A completion the range made itself is judged with the others.
    let reference_active = reference.and_then(active_block);
    if !tail.is_empty()
        && tail.first() == reference_active.as_ref()
        && made.first().map(|made| &made.block) != tail.first()
    {
        tail.remove(0);
    }
    for (
        index,
        Made {
            block: held, at, ..
        },
    ) in made.iter().enumerate()
    {
        match tail.get(index) {
            Some(archived) if archived == held => {}
            Some(archived) => {
                return Ok(Some(format!(
                    "the archive must hold every completion this range made, in order: it has the block reviewed at {} where the completion reviewed at {} (made at {at}) belongs",
                    archived.reviewed, held.reviewed
                )))
            }
            None => {
                return Ok(Some(format!(
                    "the archive must hold every completion this range made, in order: the completion reviewed at {} (made at {at}) is missing",
                    held.reviewed
                )))
            }
        }
        if let Some(problem) = historical_binding(repo, *at, held) {
            return Ok(Some(format!(
                "the archived completion reviewed at {}, made at {at}, has no real provenance: {problem}",
                held.reviewed
            )));
        }
    }
    if let Some(extra) = tail.get(made.len()) {
        return Ok(Some(format!(
            "the archived acceptance block reviewed at {} is not a completion this range made; a reopen archives only a completion that was made",
            extra.reviewed
        )));
    }
    Ok(None)
}

/// One completion the range's history made: the commit that introduced
/// it, its active block, and whether the record left `complete` after it.
struct Made {
    block: AcceptanceBlock,
    at: Oid,
    reopened_after: bool,
}

/// Every completion of `task`'s record the range's history made, from
/// `anchor` (excluded) to the landing, in the order it made them. A
/// completion is an occurrence, never a block's content: a commit whose
/// record is complete while no parent holds that same completion (active
/// block and archive) introduces one, so `complete -> todo -> complete`
/// with an identical block is two completions. A history that cannot be
/// read is an error, never an empty list.
fn completions_made(
    repo: &Repository,
    task: &RecordView,
    anchor: Option<Oid>,
    landing: Landing<'_>,
) -> Result<Vec<Made>, String> {
    let _judgment = history::judgment();
    let unreadable =
        |error: git2::Error| format!("cannot read the history of {}: {error}", task.path);
    let mut walk = history::revwalk(repo).map_err(unreadable)?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)
        .map_err(unreadable)?;
    walk.push(landing.commit()).map_err(unreadable)?;
    if let Some(anchor) = anchor {
        walk.hide(anchor).map_err(unreadable)?;
    }
    let state = |record: &RecordView| {
        (record.status == "complete")
            .then(|| active_block(record))
            .flatten()
            .map(|active| (active, record.superseded_blocks()))
    };
    let mut made: Vec<Made> = Vec::new();
    for oid in walk {
        let oid = oid.map_err(unreadable)?;
        let Some(record) = record_at(repo, oid, task)? else {
            continue;
        };
        let Some((active, archive)) = state(&record) else {
            if let Some(last) = made.last_mut() {
                last.reopened_after = true;
            }
            continue;
        };
        let commit = repo
            .find_commit(oid)
            .map_err(|error| format!("cannot read commit {oid}: {error}"))?;
        let mut held_before = false;
        for parent in history::parents(repo, &commit).map_err(|error| error.to_string())? {
            if let Some(then) = record_at(repo, parent, task)? {
                if state(&then).is_some_and(|(was, kept)| was == active && kept == archive) {
                    held_before = true;
                }
            }
        }
        if !held_before {
            made.push(Made {
                block: active,
                at: oid,
                reopened_after: false,
            });
        }
    }
    Ok(made)
}

/// Why a completion the history made at `at` is not a real one, if it is
/// not: its reviewed commit is in this repository and is `at` or an
/// ancestor of it, and each waiver names a commit here that `at` contains.
/// Its reviewed span is not judged again: a reopen is how a completion
/// whose span failed is repaired (the binding refusal's own remedy), so an
/// archived completion records what was claimed, and only the active
/// completion carries the full binding. A history that cannot be read
/// refuses.
fn historical_binding(repo: &Repository, at: Oid, block: &AcceptanceBlock) -> Option<String> {
    let Some(reviewed) = commit_of(repo, &block.reviewed) else {
        return Some(format!(
            "reviewed commit {} is not in this repository",
            block.reviewed
        ));
    };
    match is_ancestor_or_same(repo, reviewed, at) {
        Ok(true) => {}
        Ok(false) => {
            return Some(format!(
                "reviewed commit {reviewed} is not the completion or an ancestor of it"
            ))
        }
        Err(error) => return Some(error),
    }
    for (id, result) in &block.criteria {
        if result.outcome != "waived" {
            continue;
        }
        let Some(amendment) = commit_of(repo, &result.evidence) else {
            return Some(format!("{id} waiver names a commit that is not here"));
        };
        match is_ancestor_or_same(repo, amendment, at) {
            Ok(true) => {}
            Ok(false) => {
                return Some(format!(
                    "{id} waiver names {amendment}, which the completion does not contain"
                ))
            }
            Err(error) => return Some(error),
        }
    }
    None
}

/// Where a reopening task pull request left the completion `old` that the
/// range's anchor `at` holds: `at` itself when it is the review's ancestor,
/// otherwise the merge-base of `at` and the review, when that commit holds
/// the same completion (status, active block and archive) of this task's
/// record. Any other history keeps `at`, so the review stays refused as
/// lying before the fix range.
fn reopen_left_at(
    repo: &Repository,
    task: &RecordView,
    old: &RecordView,
    at: Oid,
    reviewed: Oid,
) -> Result<Oid, String> {
    if is_ancestor_or_same(repo, at, reviewed)? {
        return Ok(at);
    }
    let Some(left) = common_base(repo, at, reviewed)? else {
        return Ok(at);
    };
    let same = record_at(repo, left, task)?.is_some_and(|then| {
        then.status == "complete"
            && old.status == "complete"
            && active_block(&then).is_some()
            && active_block(&then) == active_block(old)
            && then.superseded_blocks() == old.superseded_blocks()
    });
    Ok(if same { left } else { at })
}

/// Bind an epic's own acceptance block (SPC-013 R-33, R-60), which proves
/// the criteria no live task serves: its reviewed commit is in this
/// repository and is the completion's commit or its ancestor, with only
/// this record's status and Closeout changed after it, and each waiver
/// names a planning amendment of that criterion of this epic that the
/// reviewed commit contains. The amendment rides in the epic's batched
/// amendment on its line, so the reviewed candidate, not one target branch,
/// is where it must be found. An epic without an own block binds nothing.
#[must_use]
pub fn bind_epic_completion(
    repo: &Repository,
    epic: &RecordView,
    landing: Landing<'_>,
) -> Vec<Finding> {
    let Some(block) = active_block(epic) else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    let mut bind = |message: String| {
        findings.push(Finding {
            epic_record: Some(epic.path.clone()),
            ..finding(BINDING_RULE, format!("{}: {message}", epic.id))
        });
    };
    let reviewed = commit_of(repo, &block.reviewed);
    match reviewed {
        None => bind(format!(
            "reviewed commit {} is not in this repository",
            block.reviewed
        )),
        Some(reviewed) => {
            if let Some(problem) =
                reviewed_span_problem(repo, epic, landing, reviewed, Stacking::Never)
            {
                bind(problem);
            }
        }
    }
    for (id, result) in &block.criteria {
        if result.outcome == "waived" {
            if let Some(problem) = epic_waiver_problem(repo, epic, id, &result.evidence, reviewed) {
                bind(format!("{id} waiver {problem}"));
            }
        }
    }
    findings
}

/// Why a waiver in an epic's own block does not name the planning
/// amendment of criterion `id` of `epic` inside the reviewed commit, if it
/// does not.
fn epic_waiver_problem(
    repo: &Repository,
    epic: &RecordView,
    id: &str,
    evidence: &str,
    reviewed: Option<Oid>,
) -> Option<String> {
    let named = evidence;
    let Some(amendment) = commit_of(repo, named) else {
        return Some(format!("names {named}, which is not a commit here"));
    };
    let Some(reviewed) = reviewed else {
        return Some(format!(
            "names {named}, which cannot be checked without the reviewed commit"
        ));
    };
    let contains = match is_ancestor_or_same(repo, amendment, reviewed) {
        Ok(contains) => contains,
        Err(error) => return Some(error),
    };
    if !contains {
        return Some(format!(
            "names {named}, which the reviewed commit {reviewed} does not contain; a waiver is a planning amendment the review saw"
        ));
    }
    let Ok(commit) = repo.find_commit(amendment) else {
        return Some(format!("names {named}, which cannot be read"));
    };
    let parent = match history::parents(repo, &commit) {
        Ok(parents) => parents.first().copied(),
        Err(error) => return Some(error.to_string()),
    };
    match non_planning_change(repo, parent, amendment) {
        Ok(None) => {}
        Ok(Some(path)) => {
            return Some(format!(
                "names {named}, which also changes {path}; a planning amendment changes planning records only"
            ));
        }
        Err(_) => return Some(format!("names {named}, whose change cannot be read")),
    }
    let criterion = |oid: Option<Oid>| {
        oid.map(|oid| record_at(repo, oid, epic))
            .transpose()
            .ok()
            .flatten()
            .flatten()
            .and_then(|record| {
                record
                    .criteria
                    .items
                    .into_iter()
                    .find(|item| item.id == id)
                    .map(|item| item.text)
            })
    };
    match (criterion(parent), criterion(Some(amendment))) {
        (Some(before), Some(after)) if before != after => None,
        _ => Some(format!(
            "names {named}, which does not amend {id} of {}",
            epic.id
        )),
    }
}

/// The binding findings for every epic a range completes or whose active
/// block it changes. A planning or line range (`amendable`) binds each at
/// the commit that introduced its block; any other range owns all its
/// changes and binds at `head`.
pub(super) fn epic_completions_in_range(
    repo: &Repository,
    before: &Graph,
    after: &Graph,
    head: Oid,
    amendable: bool,
) -> Result<Vec<Finding>, String> {
    let mut findings = Vec::new();
    for epic in after
        .records
        .values()
        .filter(|record| record.kind == RecordKind::Epic && record.status == "complete")
    {
        let Some(block) = active_block(epic) else {
            continue;
        };
        let unchanged = before.records.get(&epic.id).is_some_and(|then| {
            then.status == "complete" && active_block(then).as_ref() == Some(&block)
        });
        if unchanged {
            continue;
        }
        let origin = if amendable {
            introduced_at(repo, epic, &block, head)?
        } else {
            head
        };
        findings.extend(bind_epic_completion(repo, epic, Landing::Commit(origin)));
    }
    Ok(findings)
}

/// Find the prior completed record when a range or target checkout starts
/// after the task was reopened. The range base can be `todo`, so it cannot
/// itself supply the acceptance block that the reopen must preserve.
fn previous_completion(
    repo: &Repository,
    task: &RecordView,
    block: &AcceptanceBlock,
    landing: Landing<'_>,
) -> Result<Option<(Oid, RecordView)>, String> {
    let mut at = landing.commit();
    let inherited = match landing {
        Landing::Commit(_) => true,
        Landing::Worktree { head, .. } => {
            record_at(repo, head, task)?.as_ref().is_some_and(|record| {
                record.status == "complete"
                    && active_block(record).as_ref() == Some(block)
                    && record.superseded_blocks() == task.superseded_blocks()
            })
        }
    };
    if inherited {
        let introduced = introduced_at(repo, task, block, at)?;
        let Some(parent) = first_parent(repo, introduced)? else {
            return Ok(None);
        };
        at = parent;
    }
    let mut crossed_reopen = false;
    loop {
        let Some(record) = record_at(repo, at, task)? else {
            return Ok(None);
        };
        if record.status == "complete" {
            return Ok(crossed_reopen.then_some((at, record)));
        }
        crossed_reopen = true;
        let Some(parent) = first_parent(repo, at)? else {
            return Ok(None);
        };
        at = parent;
    }
}

/// Every path the working tree changes against `HEAD`: staged, unstaged and
/// untracked, ignored files excepted. The reviewed result is a commit, so
/// these are what a completion made here adds to it.
///
/// # Errors
///
/// Returns the git error when the working tree's state cannot be read.
pub fn worktree_changes(repo: &Repository) -> Result<Vec<crate::git::GitName>, git2::Error> {
    let mut options = git2::StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false);
    Ok(repo
        .statuses(Some(&mut options))?
        .iter()
        .map(|entry| crate::git::GitName::from_bytes(entry.path_bytes()))
        .collect())
}

/// What may carry a review to the completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Carried {
    /// The caller's transport, for a range that does not reopen the task.
    Transport(Transport),
    /// A reopening task pull request: clean merges of its target after the
    /// review stack on it, but no task landing carries a review (TSK-340).
    ReopenedStack,
}

/// Apply the one reviewed-span check, resolving task-landing transport only
/// when the source and completion do not share a directly reviewed span.
/// `stacking` holds the target tips a clean merge after the review must
/// come from.
#[allow(clippy::too_many_arguments)] // Reviewed-span inputs and the landed adoption anchor.
fn binding_source(
    repo: &Repository,
    task: &RecordView,
    landing: Landing<'_>,
    reviewed: Oid,
    carried: Carried,
    stacking: &StackingTips,
    default_target: Option<Oid>,
    anchor: Option<Oid>,
) -> Result<CompletionProof, String> {
    let proof = CompletionProof {
        reviewed,
        landing_merge: None,
        span_head: landing.commit(),
        completion_tail: landing.commit(),
    };
    let receiving = stacking;
    let stacking = match carried {
        Carried::Transport(Transport::TaskLanding) | Carried::ReopenedStack => {
            Stacking::OnTarget(receiving)
        }
        Carried::Transport(Transport::Direct) => Stacking::Never,
    };
    let Some(direct) = reviewed_span_problem(repo, task, landing, reviewed, stacking) else {
        return Ok(proof);
    };
    if carried != Carried::Transport(Transport::TaskLanding) {
        return Err(direct);
    }
    match task_landing(
        repo,
        task,
        landing,
        reviewed,
        default_target,
        anchor,
        receiving,
    ) {
        Landed::NoMerge => Err(direct),
        Landed::Span { merge, head } => {
            if let Some(problem) =
                reviewed_span_problem(repo, task, Landing::Commit(head), reviewed, Stacking::Never)
            {
                return Err(format!("the landing merge {merge} brings {head}, and {problem}; review the result that landed"));
            }
            Ok(CompletionProof {
                landing_merge: Some(merge),
                span_head: head,
                ..proof
            })
        }
        Landed::Refused(problem) => Err(problem),
        Landed::NotALanding(why) => Err(format!("{direct} ({why})")),
    }
}

/// Whether a reviewed span may stack clean merges of the target.
#[derive(Debug, Clone, Copy)]
enum Stacking<'a> {
    /// Direct work, a landed head and an epic.
    Never,
    /// A task pull request's own chain, judged against these target tips.
    OnTarget(&'a StackingTips),
}

/// The only binding predicate: the reviewed commit is the source head or
/// its ancestor, with only the task's status and Closeout changed afterward.
/// With `stacking`, a task pull request's own chain may also re-merge its
/// integration target cleanly after the review (TSK-184), a reopening one
/// included (TSK-340); direct work and a completion bound at a head
/// (SPC-013 R-120) never stack.
fn reviewed_span_problem(
    repo: &Repository,
    task: &RecordView,
    landing: Landing<'_>,
    reviewed: Oid,
    stacking: Stacking<'_>,
) -> Option<String> {
    if let Landing::Worktree { changed, .. } = landing {
        let outside: Vec<String> = changed
            .iter()
            .filter(|path| path.bytes() != task.path.as_bytes())
            .map(|path| path.display().to_string())
            .collect();
        if !outside.is_empty() {
            return Some(format!(
                "uncommitted changes outside the record were never reviewed ({}); commit, remove or ignore them, then review the result again",
                outside.join(", ")
            ));
        }
    }
    let at = landing.commit();
    // The record as the completion wrote it: a later planning amendment on
    // the line is not part of this completion.
    let completed = match landing {
        Landing::Commit(oid) => match blob_at(repo, oid, &task.path) {
            Ok(Some(content)) => content,
            Ok(None) => return Some(format!("{} is missing at completion {oid}", task.path)),
            Err(error) => return Some(error),
        },
        Landing::Worktree { .. } => task.content.clone(),
    };
    let contains = match is_ancestor_or_same(repo, reviewed, at) {
        Ok(contains) => contains,
        Err(error) => return Some(error),
    };
    if !contains {
        return Some(format!(
            "reviewed commit {reviewed} is not the head or an ancestor of it; review the result that lands"
        ));
    }
    // Reviewed stacking: after the review, the task branch may re-merge
    // its integration target cleanly, and its own commits may change only
    // this record's status and Closeout. Anything else is judged as the
    // whole change from the reviewed commit to C, naming the commit that
    // stopped the stack.
    let stack = match stacking {
        Stacking::OnTarget(tips) => stacked(repo, task, reviewed, at, tips),
        Stacking::Never => Stacked::No(None),
    };
    match stack {
        Stacked::Clean => {
            let Ok(Some(then)) = blob_at(repo, reviewed, &task.path) else {
                return Some(format!(
                    "{} is not in the reviewed commit, so its scope was never reviewed",
                    task.path
                ));
            };
            (reviewed_part(&then) != reviewed_part(&completed))
                .then(|| "the record changed outside its status and Closeout after the reviewed commit; review the result again".into())
        }
        Stacked::Unclean(merge) => Some(format!(
            "merge {merge} is not a clean re-merge from the task's integration target; review the result again"
        )),
        // Where stacking does not apply, the whole change from the review
        // decides. A stopped stack refuses even when that change nets out
        // (SPC-013 R-60: the task's own later commits change only this
        // record's status and Closeout), leading with the net change when
        // there is one and naming the commit that stopped it.
        Stacked::No(None) => later_change_finding(repo, &task.path, &completed, reviewed, at, None),
        Stacked::No(Some(stop)) => {
            let detail = match stop {
                Stop::Commit { commit, changed } => format!("commit {commit}: {changed}"),
                Stop::Other(why) => why,
            };
            Some(
                later_change_finding(repo, &task.path, &completed, reviewed, at, Some(&detail))
                    .unwrap_or_else(|| format!("the history after the reviewed commit {reviewed} does not stack on it ({detail}); review the result again")),
            )
        }
    }
}

/// How C's first-parent chain back to the reviewed commit stacks on it.
enum Stacked {
    /// Every commit between is a clean re-merge from the task's integration
    /// target or changes only this record's status and Closeout.
    Clean,
    /// A merge from the integration target whose tree is not the clean
    /// re-merge of its parents.
    Unclean(Oid),
    /// The chain does not stack on the review, and where it stopped. `None`
    /// where stacking does not apply.
    No(Option<Stop>),
}

/// Where a chain stopped stacking on the review.
enum Stop {
    /// A non-merge commit that changes more than this record's status and
    /// Closeout, and what it changed.
    Commit { commit: Oid, changed: String },
    /// A merge from outside the target, or history that cannot be read.
    Other(String),
}

/// Walk C's first-parent chain back to the reviewed commit. A merge whose
/// second parent lies on the first-parent line of one of the run's target
/// tips (`stacking`), with the tree git's clean merge of its parents
/// gives, brings only target work: it stacks. A non-merge commit stacks only
/// by changing this record's status and Closeout. Anything else stops the
/// stack and is named: a merge from elsewhere, a merge of more than two
/// parents, a history overlay (grafts or replace refs) that could fake the
/// parents walked, a shallow boundary on the chain, or history that cannot
/// be read.
fn stacked(
    repo: &Repository,
    task: &RecordView,
    reviewed: Oid,
    at: Oid,
    stacking: &StackingTips,
) -> Stacked {
    let authorities = stacking.tips();
    let stop = |why: String| Stacked::No(Some(Stop::Other(why)));
    if let Err(error) = history::check(repo) {
        return stop(error.to_string());
    }
    let tips = || {
        authorities
            .iter()
            .map(Oid::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut cursor = at;
    while cursor != reviewed {
        let unreadable = || {
            stop(format!(
                "the history from {at} to the reviewed commit cannot be read at {cursor}"
            ))
        };
        let Ok(commit) = repo.find_commit(cursor) else {
            return unreadable();
        };
        let parents = match history::parents(repo, &commit) {
            Ok(parents) => parents,
            Err(error) => return stop(error.to_string()),
        };
        let Some(parent) = parents.first().copied() else {
            return unreadable();
        };
        let contains = match is_ancestor_or_same(repo, reviewed, parent) {
            Ok(contains) => contains,
            Err(error) => return stop(error),
        };
        if !contains {
            return stop(format!(
                "{cursor} brings the reviewed commit through a parent other than its first, so the reviewed commit is not on the head's first-parent chain"
            ));
        }
        match parents.len() {
            1 => {
                let changed = match blob_at(repo, cursor, &task.path) {
                    Ok(None) => Some(format!("{} was removed", task.path)),
                    Err(error) => Some(error),
                    Ok(Some(content)) => later_change(repo, &task.path, &content, parent, cursor),
                };
                if let Some(changed) = changed {
                    return Stacked::No(Some(Stop::Commit {
                        commit: cursor,
                        changed,
                    }));
                }
            }
            2 => {
                let Ok(side) = history::parent_id(repo, &commit, 1) else {
                    return unreadable();
                };
                if authorities.is_empty() {
                    return stop(format!(
                        "merge {cursor} cannot be checked: this run names no target tip"
                    ));
                }
                if !authorities
                    .iter()
                    .any(|tip| is_first_parent_ancestor(repo, side, *tip))
                {
                    return stop(format!(
                        "merge {cursor} brings {side}, which is not on the first-parent line of the target tip this run is judged against ({})",
                        tips()
                    ));
                }
                if !is_clean_remerge(repo, &commit).unwrap_or(false) {
                    return Stacked::Unclean(cursor);
                }
            }
            _ => {
                return stop(format!(
                    "merge {cursor} has more than two parents, so it is not a merge of the target"
                ))
            }
        }
        cursor = parent;
    }
    Stacked::Clean
}

/// Provenance of a possible task landing.
enum Landed {
    /// No merge on C's first-parent chain has `reviewed` as its second
    /// parent.
    NoMerge,
    /// The clean merge transports this task head; its reviewed span is
    /// checked by the same predicate as direct work.
    Span { merge: Oid, head: Oid },
    /// A landing merge exists, and this is why it does not carry the review.
    Refused(String),
    /// The merge that brings the review is on no receiving line the run
    /// trusts, so the direct span is the judgment, and this says why.
    NotALanding(String),
}

/// How a binding finding names the commit it binds a completion at when
/// that commit changes more than planning records. A completion bound at a
/// range head it was not made in names that head instead (TSK-433 AC-7).
const COMPLETION_CHANGES: &str = "the completion also changes";

/// Resolve the task-landing provenance. This checks transport structure,
/// never infers a line's parent and never grants a review verdict itself.
#[allow(clippy::too_many_lines)] // Find, authorize and check the landing merge in one judgment order.
fn task_landing(
    repo: &Repository,
    task: &RecordView,
    landing: Landing<'_>,
    reviewed: Oid,
    default_target: Option<Oid>,
    anchor: Option<Oid>,
    receiving: &StackingTips,
) -> Landed {
    let _judgment = history::judgment();
    if let Err(error) = history::check(repo) {
        return Landed::Refused(error.to_string());
    }
    let unreadable = |what: &str| Landed::Refused(format!("{what} cannot be read"));
    let (start, completion) = match landing {
        Landing::Worktree { head, changed } => (
            Some(head),
            changed
                .iter()
                .find(|path| {
                    path.bytes() != task.path.as_bytes()
                        && path
                            .rule_text()
                            .map_or(true, |text| !is_planning_path(text))
                })
                .map(|path| path.display().to_string()),
        ),
        Landing::Commit(at) => {
            let parent = match first_parent(repo, at) {
                Ok(parent) => parent,
                Err(error) => return Landed::Refused(error),
            };
            match non_planning_change(repo, parent, at) {
                Ok(outside) => (parent, outside),
                Err(_) => return unreadable("the completing commit's change"),
            }
        }
    };
    let mut after_merge = Vec::new();
    let mut at = start;
    // The landing merge is the commit of C's first-parent chain that brought
    // `reviewed` onto it: its first parent does not hold `reviewed`.
    let merge = loop {
        let Some(oid) = at.filter(|oid| *oid != reviewed) else {
            return Landed::NoMerge;
        };
        match is_ancestor_or_same(repo, reviewed, oid) {
            Ok(true) => {}
            Ok(false) => return Landed::NoMerge,
            Err(error) => return Landed::Refused(error),
        }
        let Ok(commit) = repo.find_commit(oid) else {
            return unreadable("the first-parent chain");
        };
        let parents = match history::parents(repo, &commit) {
            Ok(parents) => parents,
            Err(error) => return Landed::Refused(error.to_string()),
        };
        let first = parents.first().copied();
        let contains = match first
            .map(|first| is_ancestor_or_same(repo, reviewed, first))
            .transpose()
        {
            Ok(contains) => contains.unwrap_or(false), // A root commit has no first parent.
            Err(error) => return Landed::Refused(error),
        };
        if !contains {
            if parents.len() == 2 {
                break commit;
            }
            return Landed::NoMerge;
        }
        if parents.len() < 2 {
            match non_planning_change(repo, first, oid) {
                Ok(None) => {}
                Ok(Some(path)) => after_merge.push((oid, path)),
                Err(_) => return unreadable("the first-parent chain"),
            }
        }
        at = first;
    };
    // The reviewed commit is the merge's second parent, or an ancestor of it
    // after which only this record's status and Closeout changed (R-60).
    let Ok(second) = history::parent_id(repo, &merge, 1) else {
        return unreadable("the landing merge");
    };
    if let Some(path) = completion {
        return Landed::Refused(format!(
            "{COMPLETION_CHANGES} {path}; a completion after the landing merge {} of the reviewed commit {reviewed} changes planning records only",
            merge.id()
        ));
    }
    // Only a merge on a trusted receiving line is a landing (TSK-340).
    if let Some(why) = untrusted_landing(repo, merge.id(), reviewed, receiving) {
        return Landed::NotALanding(why);
    }
    if let Err(error) = check_after_landing(
        repo,
        task,
        merge.id(),
        anchor.or(start).unwrap_or(merge.id()),
        default_target,
        reviewed,
        &after_merge,
    ) {
        return Landed::Refused(error);
    }
    match is_clean_remerge(repo, &merge) {
        Ok(true) => Landed::Span { merge: merge.id(), head: second },
        Ok(false) => Landed::Refused(format!(
            "the landing merge {} of the reviewed commit {reviewed} is not the clean re-merge of its parents; review the result that landed",
            merge.id()
        )),
        Err(_) => unreadable("the landing merge"),
    }
}

/// Why `merge` is no landing, if it is not: it lies on the first-parent
/// line of none of the receiving lines this run trusts (TSK-340 review
/// round 3). Merge shape alone never proves one.
fn untrusted_landing(
    repo: &Repository,
    merge: Oid,
    reviewed: Oid,
    receiving: &StackingTips,
) -> Option<String> {
    let tips = receiving.landing_tips();
    (!tips
        .iter()
        .any(|tip| is_first_parent_ancestor(repo, merge, *tip)))
    .then(|| {
        format!(
            "the merge {merge} that brings the reviewed commit {reviewed} is not on the first-parent line of a target this run trusts ({}), so it is not a landing",
            tips.iter().map(Oid::to_string).collect::<Vec<_>>().join(", ")
        )
    })
}

/// A direct non-planning change after landing needs both a landed adoption
/// and unchanged acceptance blocks. Check each commit even if later reverted.
fn check_after_landing(
    repo: &Repository,
    task: &RecordView,
    merge: Oid,
    head: Oid,
    default_target: Option<Oid>,
    reviewed: Oid,
    commits: &[(Oid, String)],
) -> Result<(), String> {
    if commits.is_empty() {
        return Ok(());
    }
    let adopted = landed_adoptions(repo, task, merge, head, default_target)?;
    for (commit, path) in commits {
        if !adopted
            .iter()
            .any(|entry| entry.commit == commit.to_string())
        {
            return Err(format!(
                "{commit} changes {path} after the landing merge {merge} of the reviewed commit {reviewed}; only merges, planning records and landed line adoptions may follow it"
            ));
        }
        let parent = first_parent(repo, *commit)?.ok_or("adopted commit has no parent")?;
        let blocks = |at| -> Result<_, String> {
            Ok(record_at(repo, at, task)?.map(|record| {
                acceptance_blocks(&record.body)
                    .into_iter()
                    .map(|block| block.inner)
                    .collect::<Vec<_>>()
            }))
        };
        if blocks(parent)? != blocks(*commit)? {
            return Err(format!(
                "adopted commit {commit} changes {}'s acceptance block after the landing merge",
                task.id
            ));
        }
    }
    Ok(())
}

/// Use the line classifier's landed-entry proof, including its read errors.
fn landed_adoptions(
    repo: &Repository,
    task: &RecordView,
    merge: Oid,
    head: Oid,
    default_target: Option<Oid>,
) -> Result<Vec<super::line_adoption::LineAdoption>, String> {
    let Some(line) = task
        .integration_target
        .as_deref()
        .filter(|line| line.starts_with("integration/"))
    else {
        return Ok(Vec::new());
    };
    // `head` is the landed range anchor, or the pre-completion head when
    // no anchor exists. The default target bounds the line's history; without it,
    // start that history at the task landing's first parent.
    let target = match default_target {
        Some(target) => target,
        None => first_parent(repo, merge)?.ok_or("the landing merge has no first parent")?,
    };
    let report = super::line_adoption::check_range(
        repo.workdir().unwrap_or_else(|| repo.path()),
        line,
        &target.to_string(),
        &merge.to_string(),
        &head.to_string(),
    )?;
    if let Some(error) = report.record_error {
        return Err(format!("{line}'s line_adoptions cannot be read: {error}"));
    }
    Ok(report.adopted)
}

/// Whether `merge`'s tree is what merging its two parents gives with no
/// conflict: a merge that added or dropped anything of its own (an evil
/// merge) or resolved a conflict landed a result nobody reviewed. An
/// octopus merge, or one the engine cannot read, is an error, never clean.
pub(super) fn is_clean_remerge(
    repo: &Repository,
    merge: &git2::Commit<'_>,
) -> Result<bool, git2::Error> {
    match read_merge(repo, merge) {
        MergeReading::Clean => Ok(true),
        MergeReading::Changed(_) => Ok(false),
        MergeReading::Unknown(reason) => Err(git2::Error::from_str(&reason)),
    }
}

/// How a merge reads against the automatic remerge of its parents (SPC-013
/// R-53, R-60, TSK-234): the one reading the binding and the journey rule
/// share. Cleanliness is necessary to carry a review, never sufficient: the
/// binding's authority and ancestry checks still apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeReading {
    /// Two parents, and the in-process remerge of them has no conflict and
    /// equals the merge's tree: the merge added nothing of its own.
    Clean,
    /// Two parents, and the merge's tree differs from the remerge: the
    /// paths that differ, conflicted paths included, as the raw names git
    /// records ([`crate::git::GitName`] displays one for a message). The
    /// resolution is the range's own work, which a review must cover.
    Changed(Vec<Vec<u8>>),
    /// The merge cannot be read that way (an octopus merge, a missing
    /// object, an engine error): the reason. It never counts as clean and
    /// never drops out of the journey rule.
    Unknown(String),
}

/// The index entry flag bits that hold a conflict stage (1 to 3); stage 0
/// is a merged entry.
const STAGE: u16 = 0x3000;

/// Read `merge` against the clean automatic remerge of its two parents.
pub(crate) fn read_merge(repo: &Repository, merge: &git2::Commit<'_>) -> MergeReading {
    let _judgment = history::judgment();
    let id = merge.id();
    let parents = match history::parents(repo, merge) {
        Ok(parents) => parents,
        Err(error) => return MergeReading::Unknown(error.to_string()),
    };
    if parents.len() != 2 {
        return MergeReading::Unknown(format!(
            "merge {id:.9} has {} parents; only a two-parent merge can be read against the remerge of its parents, so redo it as two-parent merges",
            parents.len()
        ));
    }
    let unreadable = |error: git2::Error| {
        MergeReading::Unknown(format!(
            "merge {id:.9} cannot be remerged: {}",
            error.message()
        ))
    };
    let (first, second) = match (
        history::parent(repo, merge, 0),
        history::parent(repo, merge, 1),
    ) {
        (Ok(first), Ok(second)) => (first, second),
        (Err(error), _) | (_, Err(error)) => return unreadable(error),
    };
    let merged = match history::merge_commits(repo, &first, &second) {
        Ok(merged) => merged,
        Err(error) => return unreadable(error),
    };
    let mut recorded = match git2::Index::new() {
        Ok(index) => index,
        Err(error) => return unreadable(error),
    };
    if let Err(error) = merge.tree().and_then(|tree| recorded.read_tree(&tree)) {
        return unreadable(error);
    }
    // Entries compare and are returned by their raw names, so two names
    // that read alike once converted stay two.
    // Stage 0 holds a merged entry; stages 1 to 3 hold a conflict's sides.
    let entries = |index: &git2::Index| {
        index
            .iter()
            .filter(|entry| entry.flags & STAGE == 0)
            .map(|entry| (entry.path.clone(), (entry.id, entry.mode)))
            .collect::<std::collections::BTreeMap<Vec<u8>, (Oid, u32)>>()
    };
    let remerged = entries(&merged);
    let ours = entries(&recorded);
    let mut changed: std::collections::BTreeSet<Vec<u8>> = remerged
        .iter()
        .filter(|(name, entry)| ours.get(*name) != Some(*entry))
        .map(|(name, _)| name.clone())
        .chain(
            ours.keys()
                .filter(|name| !remerged.contains_key(*name))
                .cloned(),
        )
        .collect();
    if merged.has_conflicts() {
        match merged.conflicts() {
            Ok(conflicts) => {
                for conflict in conflicts.flatten() {
                    for side in [conflict.ancestor, conflict.our, conflict.their]
                        .into_iter()
                        .flatten()
                    {
                        changed.insert(side.path.clone());
                    }
                }
            }
            Err(error) => return unreadable(error),
        }
    }
    if changed.is_empty() {
        MergeReading::Clean
    } else {
        MergeReading::Changed(changed.into_iter().collect())
    }
}

/// The paths a range answers for (SPC-013 R-53, TSK-234): those its own
/// non-merge commits change, read per commit so a change that cancels out
/// before the head still counts, and those each merge changes against the
/// automatic remerge of its parents ([`MergeReading`]). A clean merge adds
/// nothing, so work imported unchanged from a target stays out. The range
/// is the commits `head` reaches and none of `hide` reaches: the target,
/// and for a stacked branch the predecessor's reviewed head.
///
/// # Errors
///
/// Returns a message when a revision or the history cannot be read, a
/// merge reads as [`MergeReading::Unknown`], or a name the range changes is
/// not UTF-8, which path rules cannot match: each refuses, never drops out.
pub fn owned_paths(
    repo_root: &std::path::Path,
    head: &str,
    hide: &[&str],
) -> Result<Vec<String>, String> {
    let _judgment = history::judgment();
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let oid = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("{revision}: {}", error.message()))
    };
    let unreadable = |error: git2::Error| format!("cannot read the range: {}", error.message());
    let mut walk = history::revwalk(&repo).map_err(unreadable)?;
    walk.push(oid(head)?).map_err(unreadable)?;
    for point in hide {
        walk.hide(oid(point)?).map_err(unreadable)?;
    }
    let mut paths = std::collections::BTreeSet::new();
    for commit in walk {
        let commit = repo
            .find_commit(commit.map_err(unreadable)?)
            .map_err(unreadable)?;
        let parents = history::parents(&repo, &commit).map_err(unreadable)?;
        if parents.len() > 1 {
            match read_merge(&repo, &commit) {
                MergeReading::Clean => {}
                MergeReading::Changed(changed) => {
                    for name in changed {
                        paths.insert(rule_name(&name)?);
                    }
                }
                MergeReading::Unknown(reason) => return Err(reason),
            }
            continue;
        }
        let parent = parents
            .first()
            .map(|id| repo.find_commit(*id).and_then(|parent| parent.tree()))
            .transpose()
            .map_err(unreadable)?;
        let tree = commit.tree().map_err(unreadable)?;
        let diff = repo
            .diff_tree_to_tree(parent.as_ref(), Some(&tree), None)
            .map_err(unreadable)?;
        for delta in diff.deltas() {
            for file in [delta.old_file().path(), delta.new_file().path()]
                .into_iter()
                .flatten()
            {
                paths.insert(rule_name(file.as_os_str().as_encoded_bytes())?);
            }
        }
    }
    Ok(paths.into_iter().collect())
}

/// Why the diff from `reviewed` to `head` is more than the status and
/// Closeout of the record at `path`, which reads `completed` at `head`, if
/// it is: the first other path that changed, in its short form.
pub(super) fn later_change(
    repo: &Repository,
    path: &str,
    completed: &str,
    reviewed: Oid,
    head: Oid,
) -> Option<String> {
    later_change_judged(repo, path, completed, reviewed, head).map(|later| match later {
        LaterChange::Paths { first, .. } => format!("{first} changed"),
        LaterChange::Other(problem) => problem,
    })
}

/// What [`later_change_judged`] found beyond the record's status and
/// Closeout.
enum LaterChange {
    /// Other paths changed: how many, and the first in byte order.
    Paths { count: usize, first: String },
    /// The trees, the diff or the record itself.
    Other(String),
}

/// The finding for a completion whose reviewed span is more than its
/// record's status and Closeout, ending "; review the result again". It
/// says what it compared and how much (TSK-433 AC-8, issue #320): the count
/// of other paths between the reviewed commit and `at`, and the first one.
/// `detail` names the commit that stopped a stack, when one did.
fn later_change_finding(
    repo: &Repository,
    path: &str,
    completed: &str,
    reviewed: Oid,
    at: Oid,
    detail: Option<&str>,
) -> Option<String> {
    let detail = detail
        .map(|detail| format!(" ({detail})"))
        .unwrap_or_default();
    later_change_judged(repo, path, completed, reviewed, at).map(|later| match later {
        LaterChange::Paths { count, first } => format!(
            "{count} {} changed between the reviewed commit {reviewed} and {at}, first: {first}{detail}; review the result again",
            if count == 1 { "path" } else { "paths" }
        ),
        LaterChange::Other(problem) => {
            format!("{problem} after the reviewed commit {reviewed}{detail}; review the result again")
        }
    })
}

fn later_change_judged(
    repo: &Repository,
    path: &str,
    completed: &str,
    reviewed: Oid,
    head: Oid,
) -> Option<LaterChange> {
    let tree = |oid: Oid| repo.find_commit(oid).and_then(|commit| commit.tree()).ok();
    let (Some(before), Some(after)) = (tree(reviewed), tree(head)) else {
        return Some(LaterChange::Other("the trees cannot be read".to_string()));
    };
    let Ok(diff) = repo.diff_tree_to_tree(Some(&before), Some(&after), None) else {
        return Some(LaterChange::Other("the diff cannot be read".to_string()));
    };
    let other: Vec<String> = crate::git::diff_paths(&diff)
        .iter()
        .filter(|changed| changed.bytes() != path.as_bytes())
        .map(|changed| changed.display().to_string())
        .collect();
    if let Some(first) = other.first() {
        return Some(LaterChange::Paths {
            count: other.len(),
            first: first.clone(),
        });
    }
    let Ok(Some(then)) = blob_at(repo, reviewed, path) else {
        return Some(LaterChange::Other(format!(
            "{path} is not in the reviewed commit, so its scope was never reviewed; it appeared"
        )));
    };
    (reviewed_part(&then) != reviewed_part(completed)).then(|| {
        LaterChange::Other("the record changed outside its status and Closeout".to_string())
    })
}

/// The tip of the line `task` belongs to: its declared integration target
/// as a local or remote-tracking branch, or `default_target` when it
/// declares none. A declared target that does not resolve is `None`.
fn task_target(
    repo: &Repository,
    task: &RecordView,
    default_target: Option<Oid>,
) -> Result<Option<Oid>, String> {
    match task
        .integration_target
        .as_deref()
        .filter(|target| !target.is_empty())
    {
        Some(target) => super::work_start::target_reference(repo, target)
            .map(|commit| commit.map(|commit| commit.id()))
            .map_err(|error| error.to_string()),
        None => Ok(default_target),
    }
}

/// The completion this range reopened, recovered from the history below
/// the landing, with the criteria a re-completion must keep. The recovered
/// completion supplies the archive and the old review boundary; whether a
/// completion of the task landed, and which criteria the target holds, are
/// one judgement ([`super::landing`], TSK-234). A task whose completion
/// never landed is still in its own pull request, which may change its
/// criteria with the reopen, whether the target records the task (issue
/// #67) or not (TSK-217). A landed task keeps the criteria the newest
/// judged version holds, which a later planning amendment may have set.
/// When the landing cannot be told (a history that is not complete or
/// readable, an ambiguous identity, or a ref outside the range that shows
/// a completion), the criteria stay and a changed set gets the returned
/// refusal. Only the anchor and the run's `criteria` bases supply the
/// criteria to keep (TSK-220): a branch, a remote-tracking ref or a range's
/// own line can only make the answer stricter.
fn recovered_completion(
    repo: &Repository,
    task: &RecordView,
    block: &AcceptanceBlock,
    landing: Landing<'_>,
    anchor: Option<Oid>,
    criteria: &CriteriaBases,
) -> Result<Option<Recovered>, String> {
    use super::landing::{Authority, Landing as Landed};
    let Some((at, mut old)) = previous_completion(repo, task, block, landing)? else {
        return Ok(None);
    };
    let mut refusal = None;
    let mut planned = false;
    if let Some(anchor) = anchor {
        let range = Range {
            anchor,
            head: landing.commit(),
        };
        let judgement = judge_reopened(repo, task, range, &old, criteria, at);
        let unknown = judgement.landing_unknown();
        let changed = |old: &RecordView| old.criteria.signature() != task.criteria.signature();
        match (judgement.landing, judgement.criteria) {
            // No completion of the task landed, so the range is still the
            // task's own pull request before its first landing, and R-52's
            // own-task allowance applies (issue #67). CI prints the change
            // against the target for the reviewer.
            (Landed::Planned, _) => {
                old.criteria = task.criteria.clone();
                planned = true;
            }
            (Landed::Landed { .. }, Authority::Held(record)) => old.criteria = record.criteria,
            (Landed::Landed { .. }, Authority::Unknown(reason)) => {
                if changed(&old) {
                    refusal = Some(format!(
                        "{}: a completion of this task landed, but which criteria it keeps cannot be told ({reason}), so its criteria stay as they were",
                        task.id
                    ));
                }
            }
            (Landed::Landed { .. }, Authority::Absent) => {}
            (landed, authority) => {
                if let Authority::Held(record) = authority {
                    old.criteria = record.criteria;
                }
                if changed(&old) {
                    let why = unknown.unwrap_or_default();
                    refusal = Some(match landed {
                        Landed::Elsewhere { holder, .. } => format!(
                            "{}: {why}, so the task may have landed and its criteria stay as they were; if `{holder}` is a stale copy, delete it, or merge it if it is newer work on this task, and run this again",
                            task.id
                        ),
                        Landed::Unjudged { .. } => format!(
                            "{}: {why}, so its criteria stay as they were; a reopened task keeps its criteria once a completion of it has landed",
                            task.id
                        ),
                        _ => format!(
                            "{}: cannot tell whether a target holds this task, so its criteria stay as they were: {why}",
                            task.id
                        ),
                    });
                }
            }
        }
    }
    if let Some(anchor) = anchor {
        if let Some(retained) = super::redaction::retained_completion(repo, anchor, &old)? {
            old = retained;
        }
    }
    Ok(Some(Recovered {
        at,
        old,
        refusal,
        planned,
    }))
}

/// A completion a range reopened, recovered from the history below its
/// landing ([`recovered_completion`]).
struct Recovered {
    /// The commit that last held the completion.
    at: Oid,
    /// The completed record, with the criteria a re-completion must keep.
    old: RecordView,
    /// Why the criteria to keep could not be decided, when they could not.
    refusal: Option<String>,
    /// No completion of the task landed: the range is still the task's own
    /// pull request before its first landing (R-52, R-119).
    planned: bool,
}

/// The commits a completion is judged over: those `head` reaches and
/// `anchor` does not, and the anchor itself.
#[derive(Clone, Copy)]
pub(super) struct Range {
    pub(super) anchor: Oid,
    pub(super) head: Oid,
}

/// Whether a tree holds a task, found by identity ([`presence_at`]).
enum Presence {
    /// No record there carries the task's id or uid.
    Absent,
    /// The record that carries it.
    Present(Box<RecordView>),
    /// A task record there cannot be read or does not parse, so it may be
    /// a renamed copy of this task.
    Unreadable,
}

/// The judgement of a reopened `task` (TSK-217, TSK-220, TSK-234), read at
/// every reference point the verb or `codeflow ci` could judge it against,
/// so neither an older range base, a stale local branch, an upstream on
/// another remote, nor a retargeted record makes a landed task look new.
///
/// The judged points are the range anchor and each of the run's `criteria`
/// bases (the base `codeflow ci` is given and the pre-push candidate
/// authority; in the verb, the task's target as `work start` anchors it). A
/// base the range reaches beyond the anchor is the branch's own history,
/// read only for a record that does not parse. A range's own line head is
/// stacking authority and never reaches here.
///
/// The targets are every integration target the task's record has named:
/// the declared one, the recovered completion's, and each version of the
/// record in the range, the anchor's version included. Each of their tips,
/// and the default target's, is a local branch or a remote-tracking ref
/// this clone can move: one that shows a completion the judged points do
/// not makes the landing unknown, which keeps the criteria and never
/// supplies new ones. A required target with no tip here makes the whole
/// judgement unreadable, never planned. An import already proved at a
/// published landing can supply its historical home tip when that ref was
/// deleted; that tip is still outside evidence, never a new criteria base.
fn judge_reopened(
    repo: &Repository,
    task: &RecordView,
    range: Range,
    recovered: &RecordView,
    criteria: &CriteriaBases,
    at: Oid,
) -> super::landing::TaskJudgement {
    use super::landing::{Identity, Outside, RecordStore, TaskJudgement};
    let identity = Identity::of(task);
    let mut store = RecordStore::new(repo);
    let mut targets = Vec::new();
    let mut add = |name: Option<&str>| {
        let name = name.filter(|name| !name.is_empty());
        if let Some(name) = name {
            if !targets.iter().any(|known| known == name) {
                targets.push(name.to_string());
            }
        }
    };
    add(task.integration_target.as_deref());
    add(recovered.integration_target.as_deref());
    match named_targets(repo, &mut store, &identity, range) {
        Ok(names) => names.iter().for_each(|name| add(Some(name))),
        Err(reason) => return TaskJudgement::unreadable(reason),
    }
    let mut tips: Vec<(String, Oid)> = Vec::new();
    for name in &targets {
        let mut found = match target_tips(repo, name) {
            Ok(found) => found,
            Err(reason) => return TaskJudgement::unreadable(reason),
        };
        if found.is_empty() {
            if let Some(tip) = criteria.imported_home(name) {
                found.push((name.clone(), tip));
            }
        }
        if found.is_empty() {
            return TaskJudgement::unreadable(format!(
                "the target `{0}`, which this task's record names, does not resolve here; fetch it (`git fetch origin {0}`)",
                crate::git::display_text(name)
            ));
        }
        // The epic line's pull request into its target judges the line's
        // history up to its head, so a tip of that line the head contains
        // is the range under judgment, not a target it is not judged
        // against (TSK-433). A tip beyond the head stays outside evidence.
        if let Some(head) = criteria.judged_line_head(name) {
            let mut outside = Vec::new();
            for (reference, tip) in found {
                match is_ancestor_or_same(repo, tip, head) {
                    Ok(true) => {}
                    Ok(false) => outside.push((reference, tip)),
                    Err(reason) => return TaskJudgement::unreadable(reason),
                }
            }
            found = outside;
        }
        tips.extend(found);
    }
    match default_target_name(repo) {
        Ok(name) => {
            let found = match target_tips(repo, &name.name) {
                Ok(found) => found,
                Err(reason) => return TaskJudgement::unreadable(reason),
            };
            if found.is_empty() {
                return TaskJudgement::unreadable(format!(
                    "the default target `{0}`{1} does not resolve here; fetch it (`git fetch origin {0}`)",
                    // `origin/HEAD` names it, as the remote said (TSK-388).
                    crate::git::display_text(&name.name),
                    if name.from_origin_head { ", which `origin/HEAD` names," } else { "" }
                ));
            }
            tips.extend(found);
        }
        Err(reason) => return TaskJudgement::unreadable(reason),
    }
    let (mut judged, own_history) = match judged_points(repo, range, criteria) {
        Ok(points) => points,
        Err(reason) => return TaskJudgement::unreadable(reason),
    };
    let mut seen = std::collections::HashSet::new();
    judged.retain(|point| seen.insert(*point));
    tips.retain(|(_, point)| seen.insert(*point));
    let own: Vec<Oid> = own_history.iter().map(|(_, point)| *point).collect();
    store.judge(
        &identity,
        &judged,
        &Outside {
            head: range.head,
            tips: &tips,
            at: Some(at),
            own: &own,
        },
    )
}

/// The judgement of `task` for a pull request over `range`, against the
/// run's `criteria` bases, with `store` shared by every task the run
/// judges: the criteria freeze of a task pull request and the planning
/// amendment's report read it (TSK-234). Only refs outside the range that
/// add a completing version refuse; target tips are the reopen's concern.
pub(super) fn judge_in_range(
    store: &mut super::landing::RecordStore<'_>,
    repo: &Repository,
    task: &RecordView,
    range: Range,
    criteria: &CriteriaBases,
) -> super::landing::TaskJudgement {
    let (mut judged, own_history) = match judged_points(repo, range, criteria) {
        Ok(points) => points,
        Err(reason) => return super::landing::TaskJudgement::unreadable(reason),
    };
    let mut seen = std::collections::HashSet::new();
    judged.retain(|point| seen.insert(*point));
    let own: Vec<Oid> = own_history.iter().map(|(_, point)| *point).collect();
    store.judge(
        &super::landing::Identity::of(task),
        &judged,
        &super::landing::Outside {
            head: range.head,
            tips: &[],
            at: None,
            own: &own,
        },
    )
}

/// Whether a record of a task shows a completion of it: it is complete,
/// keeps an acceptance block (active or superseded), or records a reopen.
/// A record only reaches a target this way by a completion landing there,
/// so on a target it is evidence that the task landed (issue #67).
pub(super) fn shows_completion(record: &RecordView) -> bool {
    record.status == "complete"
        || !acceptance_blocks(&record.body).is_empty()
        || !super::record_text::reopen_reasons(&record.body).is_empty()
}

/// The judged points, then the bases in the range's own history, each
/// with its name ([`judged_points`]).
type JudgedPoints = (Vec<Oid>, Vec<(String, Oid)>);

/// The points a run judges a task against (TSK-220): the range anchor, then
/// each of the run's `criteria` bases. A base the range's head reaches
/// beyond the anchor is the branch's own history, not a fixed target point,
/// so it is returned apart, as a tip that can only make an answer stricter.
fn judged_points(
    repo: &Repository,
    range: Range,
    criteria: &CriteriaBases,
) -> Result<JudgedPoints, String> {
    let mut judged = vec![range.anchor];
    let mut tips: Vec<(String, Oid)> = Vec::new();
    for base in criteria.bases().iter().copied() {
        let in_range = if base == range.anchor {
            false
        } else {
            is_ancestor_or_same(repo, base, range.head)?
        };
        if in_range {
            tips.push((base.to_string(), base));
        } else {
            judged.push(base);
        }
    }
    Ok((judged, tips))
}

/// The integration targets that versions of the task's record name in
/// `range`, the anchor's version included, in first-seen order from the
/// head back. A history that cannot be walked is the reason returned, so
/// the caller fails closed.
fn named_targets(
    repo: &Repository,
    store: &mut super::landing::RecordStore<'_>,
    task: &super::landing::Identity,
    range: Range,
) -> Result<Vec<String>, String> {
    let unwalkable = |error: git2::Error| {
        format!(
            "cannot read the history from {} to {}: {error}",
            range.anchor, range.head
        )
    };
    let mut walk = history::revwalk(repo).map_err(unwalkable)?;
    walk.push(range.head).map_err(unwalkable)?;
    walk.hide(range.anchor).map_err(unwalkable)?;
    let mut commits = walk.collect::<Result<Vec<_>, _>>().map_err(unwalkable)?;
    commits.push(range.anchor);
    let mut names = Vec::new();
    for commit in commits {
        for record in store.records_at(commit, task)? {
            if let Some(target) = record.integration_target {
                if !names.contains(&target) {
                    names.push(target);
                }
            }
        }
    }
    Ok(names)
}

/// The default target this check reads, and whether `origin/HEAD` named it.
struct DefaultName {
    name: String,
    from_origin_head: bool,
}

/// The default target: the branch `origin/HEAD` names when this clone
/// records one, else the first of `main` and `master` that resolves
/// ([`super::work_start::default_work_target`]). Any other default branch is
/// found only through `origin/HEAD`, so its absence is the refusal returned.
fn default_target_name(repo: &Repository) -> Result<DefaultName, String> {
    let origin_head = match repo.find_reference("refs/remotes/origin/HEAD") {
        // OS text rule (issue 79): the default target decides what a task
        // is measured against. One whose name is not valid UTF-8 cannot be
        // named in a record, and falling back to `main` would measure
        // against the wrong branch, so the answer is a refusal.
        Ok(head)
            if head
                .symbolic_target_bytes()
                .is_some_and(|b| std::str::from_utf8(b).is_err()) =>
        {
            return Err(
                "origin/HEAD names a default branch whose name is not valid UTF-8".to_string(),
            );
        }
        Ok(head) => crate::git::name::symbolic_target(&head).and_then(|target| {
            target
                .strip_prefix(b"refs/remotes/origin/")
                .and_then(|name| name.rule_text().ok().map(str::to_string))
        }),
        Err(error) if error.code() == git2::ErrorCode::NotFound => None,
        Err(error) => return Err(format!("cannot read origin/HEAD: {error}")),
    };
    if let Some(name) = origin_head {
        return Ok(DefaultName {
            name,
            from_origin_head: true,
        });
    }
    repo.workdir()
        .map(super::work_start::default_work_target).transpose().map_err(|error| error.to_string())?.flatten()
        .map(|name| DefaultName {
            name: name.strip_prefix("origin/").unwrap_or(&name).to_string(),
            from_origin_head: false,
        })
        .ok_or_else(|| {
            "no default target is found here: discovery reads `origin/HEAD`, then `main` and `master`, and none resolves. \
             If the default branch is `main` or `master`, fetch it (`git fetch origin main`); \
             for another default branch, record it (`git remote set-head origin --auto`)"
                .to_string()
        })
}

/// Every tip of the target `name` this clone knows, with the reference
/// that names it: the configured upstream of its local branch (on any
/// remote), its `origin` tracking ref, and the local branch, in that
/// order. Empty when none resolves.
pub(super) fn target_tips(repo: &Repository, name: &str) -> Result<Vec<(String, Oid)>, String> {
    let mut names = Vec::new();
    match repo.branch_upstream_name(&format!("refs/heads/{name}")) {
        Ok(upstream) => names.push(
            upstream
                .as_str()
                .map_err(|error| format!("cannot decode configured upstream: {error}"))?
                .to_string(),
        ),
        Err(error) if error.code() == git2::ErrorCode::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let mut candidates = super::work_start::target_reference_names(name).unwrap_or_default();
    candidates.reverse();
    names.extend(candidates);
    let mut tips = Vec::new();
    for reference in names {
        let found = match repo.find_reference(&reference) {
            Ok(found) => found,
            Err(error) if error.code() == git2::ErrorCode::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        let commit = found.peel_to_commit().map_err(|error| error.to_string())?;
        tips.push((reference, commit.id()));
    }
    Ok(tips)
}

/// The task records in the supported layouts. A missing optional directory
/// or an entry of another kind is absent; a referenced object must be readable.
fn task_entries(repo: &Repository, at: Oid) -> Result<Vec<(String, Oid)>, String> {
    let tree = repo
        .find_commit(at)
        .and_then(|commit| commit.tree())
        .map_err(|error| format!("cannot read the tree of {at}: {error}"))?;
    let Some(records) = task_subtree(repo, &tree, "project-management")? else {
        return Ok(Vec::new());
    };
    task_entries_in(repo, &records)
}

fn task_subtree<'repo>(
    repo: &'repo Repository,
    tree: &git2::Tree<'_>,
    name: &str,
) -> Result<Option<git2::Tree<'repo>>, String> {
    tree.get_name(name)
        .filter(|entry| entry.kind() == Some(git2::ObjectType::Tree))
        .map(|entry| {
            repo.find_tree(entry.id()).map_err(|error| {
                format!(
                    "the directory {name} ({}) cannot be read: {error}",
                    entry.id()
                )
            })
        })
        .transpose()
}

/// The task records under one `project-management` tree, as path and blob.
/// Traverse epic names as bytes; a supported task path requiring undecodable
/// text refuses rather than disappearing from the inventory.
///
/// # Errors
///
/// Returns a message when a task directory the tree names cannot be read:
/// a missing directory is no record, an unreadable one is no answer.
pub(super) fn task_entries_in(
    repo: &Repository,
    records: &git2::Tree<'_>,
) -> Result<Vec<(String, Oid)>, String> {
    let mut directories = Vec::new();
    if let Some(tasks) = task_subtree(repo, records, "tasks")? {
        directories.push((b"project-management/tasks".to_vec(), tasks));
    }
    if let Some(epics) = task_subtree(repo, records, "epics")? {
        for epic in &epics {
            if epic.kind() != Some(git2::ObjectType::Tree) {
                continue;
            }
            let epic_tree = repo.find_tree(epic.id()).map_err(|error| {
                format!(
                    "the epic directory {} ({}) cannot be read: {error}",
                    crate::git::GitName::from_bytes(epic.name_bytes()).display(),
                    epic.id()
                )
            })?;
            if let Some(tasks) = task_subtree(repo, &epic_tree, "tasks")? {
                let mut directory = b"project-management/epics/".to_vec();
                directory.extend_from_slice(epic.name_bytes());
                directory.extend_from_slice(b"/tasks");
                directories.push((directory, tasks));
            }
        }
    }
    let mut entries = Vec::new();
    for (directory, tasks) in directories {
        for entry in &tasks {
            let entry_name = crate::git::GitName::from_bytes(entry.name_bytes());
            let Ok(name) = entry_name.rule_text() else {
                continue; // The task filename grammar itself requires ASCII TSK-NNN.md.
            };
            let candidate = format!("project-management/tasks/{name}");
            if super::work_start::record_kind_for_tree_path(&candidate) != Some(RecordKind::Task) {
                continue;
            }
            let mut path = directory.clone();
            path.push(b'/');
            path.extend_from_slice(entry.name_bytes());
            let name = crate::git::GitName::from_bytes(&path);
            let path = name.rule_text().map_err(|_| {
                format!(
                    "the task record {} is in an epic directory whose name is not UTF-8, so its task records cannot be read",
                    name.display()
                )
            })?;
            entries.push((path.to_string(), entry.id()));
        }
    }
    Ok(entries)
}

/// Whether `record` is the task with `id` or `uid`, by parsed identity.
pub(super) fn is_same_task(record: &RecordView, id: &str, uid: Option<&str>) -> bool {
    record.id == id || (uid.is_some() && record_uid(&record.content).as_deref() == uid)
}

/// Whether the tree at `at` holds a task record with `id` or `uid`, read
/// in the task directories ([`task_entries`]). Identity is the parsed `id`
/// and `uid` frontmatter values. Any unreadable or unparseable task record
/// makes the answer unreadable: it may be a renamed copy of this task, so
/// its identity cannot be excluded by its filename.
fn presence_at(
    repo: &Repository,
    at: Oid,
    id: &str,
    uid: Option<&str>,
    _own_file: &str,
) -> Presence {
    // Every failure is the same answer: the record may be this task.
    let Ok(entries) = task_entries(repo, at) else {
        return Presence::Unreadable;
    };
    let mut unreadable = false;
    let mut present = None;
    for (path, blob) in entries {
        let Ok(blob) = repo.find_blob(blob) else {
            unreadable = true;
            continue;
        };
        let Ok(content) = std::str::from_utf8(blob.content()) else {
            unreadable = true;
            continue;
        };
        match RecordView::parse(RecordKind::Task, &path, content) {
            Ok(record) if is_same_task(&record, id, uid) => {
                present.get_or_insert(Box::new(record));
            }
            Err(_) => unreadable = true,
            Ok(_) => {}
        }
    }
    if unreadable {
        Presence::Unreadable
    } else {
        present.map_or(Presence::Absent, Presence::Present)
    }
}

/// The file name of the task's own record, which [`presence_at`] treats as
/// the task's even when it does not parse.
pub(super) fn own_file_name(task: &RecordView) -> String {
    std::path::Path::new(&task.path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

/// A record's `uid` as the frontmatter parser reads it, so every YAML
/// form of one value (plain, single or double quoted) is the same uid.
pub(super) fn record_uid(content: &str) -> Option<String> {
    let (data, _) = crate::validate::parse_frontmatter(content.as_bytes()).ok()?;
    let uid = crate::validate::get_string_field(&data, "uid");
    (!uid.is_empty()).then(|| uid.clone())
}

/// Whether a waiver may name a commit of the task's own range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnRange {
    /// The task's own pull request, from this base (TSK-184).
    Base(Oid),
    /// No own range: a line, a release range, or a run with no own branch.
    None,
    /// A range that reopens a landed completion, whose criteria stay frozen
    /// (R-119): only the target route applies, and the refusal says so.
    LandedReopen,
}

/// What "on the target" requires of a waiver's commit (TSK-340 AC-7, issue
/// #354): a receiving line the run pins holds it, never a ref this clone can
/// move.
const ON_TARGET: &str = "on the target means a receiving line this run pins holds the commit (the pull request's base in `codeflow ci`, `--base` in `task status`)";

/// The sanctioned route to a waiver a refusal names (TSK-340 AC-7): the
/// planning amendment R-60 accepts, which the review must contain.
const WAIVER_ROUTE: &str = "amend the criterion through a planning pull request into the target, merge the target into this branch before the review, and name that planning commit";

/// Why a waiver's commit is not the planning amendment for this record and
/// criterion on the target, if it is not.
#[allow(clippy::too_many_lines)] // Read the waiver's target, range and record proofs in one refusal path.
fn waiver_problem(
    repo: &Repository,
    task: &RecordView,
    id: &str,
    evidence: &str,
    landing: Landing<'_>,
    pinned: &[Oid],
    own_range: OwnRange,
) -> Option<String> {
    let Some(amendment) = commit_of(repo, evidence) else {
        return Some(format!(
            "names {}, which is not a commit here",
            evidence.trim()
        ));
    };
    // Only a real completion commit can be named as the head; the verb's
    // `HEAD` is the completion's parent and may itself be the amendment.
    if matches!(landing, Landing::Commit(completion) if completion == amendment) {
        return Some(
            "names the pull request head; a waiver is a planning amendment on the target".into(),
        );
    }
    if pinned.is_empty() {
        return Some(format!(
            "names {}, which cannot be checked: this run pins no target; {ON_TARGET}",
            evidence.trim()
        ));
    }
    let mut off_target = true;
    for tip in pinned {
        match is_ancestor_or_same(repo, amendment, *tip) {
            Ok(true) => off_target = false,
            Ok(false) => {}
            Err(error) => return Some(error),
        }
    }
    if off_target {
        // Without the task's own range (a line, a release range, a range
        // that reopens a landed completion), the target route is the only
        // one.
        let own_range_base = match own_range {
            OwnRange::Base(base) => base,
            OwnRange::None => {
                return Some(format!(
                    "names {}, which is not on the target; {ON_TARGET}; {WAIVER_ROUTE}",
                    evidence.trim()
                ))
            }
            OwnRange::LandedReopen => {
                return Some(format!(
                    "names {}, which is not on the target; a reopened task's waiver must name a planning commit on the target that amends this criterion, since a completion of it landed and its criteria are frozen; {ON_TARGET}; {WAIVER_ROUTE}",
                    evidence.trim()
                ))
            }
        };
        let reviewed = active_block(task).and_then(|block| commit_of(repo, &block.reviewed));
        let before_base = match is_ancestor_or_same(repo, amendment, own_range_base) {
            Ok(before_base) => before_base,
            Err(error) => return Some(error),
        };
        let before_review = match reviewed
            .map(|reviewed| is_ancestor_or_same(repo, amendment, reviewed))
            .transpose()
        {
            Ok(before_review) => before_review.unwrap_or(false), // No review cannot authorize a waiver.
            Err(error) => return Some(error),
        };
        if before_base || reviewed == Some(amendment) || !before_review {
            return Some(format!(
                "names {}, which is not on the target or a record-only amendment strictly before the reviewed revision in this task's range; {ON_TARGET}; {WAIVER_ROUTE}",
                evidence.trim()
            ));
        }
        let Ok(commit) = repo.find_commit(amendment) else {
            return Some("amendment cannot be read".into());
        };
        let paths = super::lifecycle::changed_paths(
            repo,
            &history::parent_id(repo, &commit, 0).map_or_else(|_| String::new(), |p| p.to_string()),
            Some(&amendment.to_string()),
        );
        if paths.is_err()
            || paths
                .is_ok_and(|paths| paths.is_empty() || paths.iter().any(|path| path != &task.path))
        {
            return Some(format!(
                "names {}, which must change only this task's record to count as an amendment in its own range, and is not on the target; {ON_TARGET}; {WAIVER_ROUTE}",
                evidence.trim()
            ));
        }
    }
    let contains = match is_ancestor_or_same(repo, amendment, landing.commit()) {
        Ok(contains) => contains,
        Err(error) => return Some(error),
    };
    if !contains {
        return Some(format!(
            "names {}, which the completion does not contain; rebase onto the amendment",
            evidence.trim()
        ));
    }
    if !off_target {
        // R-60 (TSK-340 review): a waiver is an amendment the review saw. A
        // planning amendment on the target counts only when the reviewed
        // commit contains it, wherever the completion is made, so no merge
        // after the review, whichever way its parents run, brings a later
        // amendment in as a waiver. Merge topology never grants this.
        let reviewed = active_block(task).and_then(|block| commit_of(repo, &block.reviewed));
        let seen = match reviewed
            .map(|reviewed| is_ancestor_or_same(repo, amendment, reviewed))
            .transpose()
        {
            Ok(seen) => seen.unwrap_or(false), // No review cannot authorize a waiver.
            Err(error) => return Some(error),
        };
        if !seen {
            return Some(format!(
                "names {}, which the reviewed commit does not contain; a waiver names a planning amendment made before the review",
                evidence.trim()
            ));
        }
    }
    let Ok(commit) = repo.find_commit(amendment) else {
        return Some(format!("names {}, which cannot be read", evidence.trim()));
    };
    let parent = match history::parents(repo, &commit) {
        Ok(parents) => parents.first().copied(),
        Err(error) => return Some(error.to_string()),
    };
    // What the amendment brought onto the target (for a merge, against the
    // target-side parent) changes planning records only (R-52, R-60).
    match non_planning_change(repo, parent, amendment) {
        Ok(None) => {}
        Ok(Some(path)) => {
            return Some(format!(
                "names {}, which also changes {path}; a planning amendment changes planning records only",
                evidence.trim()
            ));
        }
        Err(_) => {
            return Some(format!(
                "names {}, whose change cannot be read",
                evidence.trim()
            ));
        }
    }
    let criterion = |oid: Option<Oid>| {
        oid.map(|oid| record_at(repo, oid, task))
            .transpose()
            .ok()
            .flatten()
            .flatten()
            .map(|record| {
                record
                    .criteria
                    .items
                    .into_iter()
                    .find(|item| item.id == id)
                    .map(|item| item.text)
            })
    };
    match (criterion(parent), criterion(Some(amendment))) {
        (Some(before), Some(after)) if before != after => None,
        _ => Some(format!(
            "names {}, which does not amend {id} of {}",
            evidence.trim(),
            task.id
        )),
    }
}

/// The first path the change from `parent` (none for a root commit) to
/// `commit` touches outside the planning records; an error when a tree or
/// the diff cannot be read.
pub(super) fn non_planning_change(
    repo: &Repository,
    parent: Option<Oid>,
    commit: Oid,
) -> Result<Option<String>, git2::Error> {
    let tree = |oid: Oid| repo.find_commit(oid).and_then(|commit| commit.tree());
    let after = tree(commit)?;
    let before = parent.map(tree).transpose()?;
    let diff = repo.diff_tree_to_tree(before.as_ref(), Some(&after), None)?;
    // OS text rule (issue 79): a planning path is valid text with no
    // backslash, so any other name is outside the planning records.
    let outside = crate::git::diff_paths(&diff)
        .into_iter()
        .find(|path| {
            path.rule_text()
                .map_or(true, |text| text.contains('\\') || !is_planning_path(text))
        })
        .map(|path| path.display().to_string());
    Ok(outside)
}

/// A name the range changes, as text path rules match: the name itself,
/// which must be UTF-8. Any rendering of other bytes can change whether a
/// pattern matches it, so such a name refuses (TSK-234 review round 10).
///
/// # Errors
///
/// Returns the refusal for a name that is not UTF-8.
pub fn rule_name(raw: &[u8]) -> Result<String, String> {
    std::str::from_utf8(raw).map(str::to_string).map_err(|_| {
        format!(
            "the range changes `{}`, whose name is not UTF-8, so path rules cannot match it; rename it",
            crate::git::GitName::from_bytes(raw).display()
        )
    })
}

/// The epic journey criterion a task serves, when it has no journey of its
/// own.
fn served_journey<'g>(task: &RecordView, graph: &'g Graph) -> Option<(&'g str, &'g Criterion)> {
    if task.criteria.items.iter().any(Criterion::is_journey) {
        return None;
    }
    task.criteria.items.iter().find_map(|item| {
        let (epic_id, criterion_id) = item.serves()?;
        let epic = graph
            .records
            .get(&epic_id)
            .filter(|record| record.kind == RecordKind::Epic)?;
        epic.criteria
            .items
            .iter()
            .find(|criterion| criterion.id == criterion_id && criterion.is_journey())
            .map(|criterion| (epic.id.as_str(), criterion))
    })
}

/// A leaf serving the epic's journey links the evidence that ran
/// (`verified | <link>`) or states the narrower path it verified
/// (`narrower | <path>`); it never leaves the journey unsaid (R-53).
fn leaf_journey(task: &RecordView, graph: &Graph, block: &AcceptanceBlock) -> Option<Finding> {
    let (epic, criterion) = served_journey(task, graph)?;
    let detail = block
        .journey
        .split_once('|')
        .map_or("", |(_, detail)| detail.trim());
    let said =
        matches!(outcome_word(&block.journey), "verified" | "narrower") && !detail.is_empty();
    (!said).then(|| {
        finding(
            JOURNEY_RULE,
            format!(
                "{} serves {epic} {} (a journey); its `journey` links the evidence that ran (`verified | <link>`) or states the narrower path (`narrower | <path>`)",
                task.id, criterion.id
            ),
        )
    })
}

/// Why `task_id` may not carry an adopter-facing range: it has no journey
/// criterion and serves no epic journey criterion (R-53).
#[must_use]
pub fn journey_requirement(graph: &Graph, task_id: &str) -> Option<String> {
    use std::fmt::Write as _;
    let task = graph
        .records
        .get(task_id)
        .filter(|record| record.kind == RecordKind::Task)?;
    let own = task.criteria.items.iter().any(Criterion::is_journey);
    (!own && served_journey(task, graph).is_none()).then(|| {
        let inner: Vec<&str> = task
            .criteria
            .items
            .iter()
            .filter(|criterion| {
                criterion.has_inner_journey() && criterion.tag_hidden_by("(journey)").is_none()
            })
            .map(|criterion| criterion.id.as_str())
            .collect();
        let mut note = if inner.is_empty() {
            String::new()
        } else {
            format!(
                "; {} {} `(journey)` inside its text, and the tag counts only where it opens or closes the criterion (R-50)",
                inner.join(", "),
                if inner.len() == 1 { "carries" } else { "carry" }
            )
        };
        for (name, tag, hiding) in hidden_tags(task, graph) {
            let _ = write!(note, "; {}", hidden_tag_message(&name, tag, hiding));
        }
        format!(
            "{task_id} changes the adopter-facing path set but has no `(journey)` criterion and serves no epic journey criterion{note}"
        )
    })
}

/// The tags a sub-bullet keeps from counting on the task's criteria (its
/// `(journey)` and `(serves ...)`) and on the epic criteria it serves (their
/// `(journey)`): each criterion's name (`AC-n`, or `EPC-NNN AC-n` for a
/// served one), the tag and the hiding sub-bullet's first line.
fn hidden_tags<'g>(task: &'g RecordView, graph: &'g Graph) -> Vec<(String, &'static str, &'g str)> {
    let own = task.criteria.items.iter().flat_map(|criterion| {
        [("(journey)", "(journey)"), ("(serves ", "(serves ...)")]
            .into_iter()
            .filter_map(|(tag, label)| {
                criterion
                    .tag_hidden_by(tag)
                    .map(|hiding| (criterion.id.clone(), label, hiding))
            })
    });
    let served = task.criteria.items.iter().filter_map(|item| {
        let (epic_id, criterion_id) = item.serves()?;
        let epic = graph
            .records
            .get(&epic_id)
            .filter(|record| record.kind == RecordKind::Epic)?;
        let criterion = epic
            .criteria
            .items
            .iter()
            .find(|criterion| criterion.id == criterion_id)?;
        criterion
            .tag_hidden_by("(journey)")
            .map(|hiding| (format!("{} {}", epic.id, criterion.id), "(journey)", hiding))
    });
    own.chain(served).collect()
}

/// Records of the range whose criteria differ from the criteria the judged
/// target holds for them (R-52, TSK-234), read through one judgement
/// (`judging`): the newest judged version, or the newest version in the
/// judged history when the target no longer holds the record, so a record
/// the target deleted is not new. The change of each task in `exempt` (the
/// pull request's own task) is printed as its delta while no completion of
/// it has landed; once one has, or when that cannot be told, it is refused,
/// since a landed task's criteria change only through a planning amendment
/// that names its epic. The text correction of each task in `correctable`
/// (ADR-0080), a completed standalone task, is printed as its delta when it
/// keeps the criteria set and every tag, and refused otherwise. Any other
/// change is refused. A predecessor's record
/// that is its record at the reviewed head this branch stacks on, byte for
/// byte (`stacked`, SPC-013 R-42, issue #69), changed in that predecessor's
/// own reviewed pull request, which judges it: it is printed as a note,
/// never frozen here.
#[must_use]
pub fn frozen_criteria(
    head: &Graph,
    target: &Graph,
    changed_paths: &[String],
    exempt: &std::collections::BTreeSet<String>,
    correctable: &std::collections::BTreeSet<String>,
    stacked: &[StackedHead],
    judging: &mut Judging<'_>,
) -> Vec<Finding> {
    use super::landing::{Authority, Landing as Landed};
    let mut found = Vec::new();
    for record in head
        .records
        .values()
        .filter(|record| record.kind == RecordKind::Task && changed_paths.contains(&record.path))
    {
        let same = |other: &RecordView| other.criteria.signature() == record.criteria.signature();
        // Equality is judged against the authority, never one base: a newer
        // run base may hold criteria the target tip does not.
        let tip = target.records.get(&record.id);
        let own = exempt.contains(&record.id);
        if !own {
            if let Some(stack) = stacked
                .iter()
                .find(|stack| stack.task_id == record.id && stack.record.content == record.content)
            {
                found.push(Finding {
                    epic_record: None,
                    rule: FROZEN_RULE,
                    message: format!(
                        "{} changes its criteria in its own reviewed pull request, at {}, which this branch stacks on; that pull request's check judges the change",
                        record.id, stack.pin
                    ),
                    note: true,
                });
                continue;
            }
        }
        let judgement = judging.judge(record);
        found.push(authority_note(record, &judgement.criteria_source));
        let unknown = judgement.landing_unknown();
        let authority = match &judgement.criteria {
            Authority::Held(kept) => Some(kept.as_ref()),
            Authority::Absent => None,
            Authority::Unknown(reason) => {
                found.push(finding(
                    FROZEN_RULE,
                    if tip.is_none() {
                        format!(
                            "{} is a record this branch adds, but whether the target held it cannot be told ({reason}), so its criteria cannot be judged new",
                            record.id
                        )
                    } else {
                        format!(
                            "{} changes its record, but which criteria the target holds for it cannot be told ({reason}), so its criteria cannot be judged unchanged",
                            record.id
                        )
                    },
                ));
                continue;
            }
        };
        if authority.is_some_and(same) {
            // The signature leaves out the checkbox mark, so ticking a box
            // is no change; a correction still keeps the form and the
            // structure, so adding or removing the mark, or a list item
            // that is no criterion, with the text unchanged is refused,
            // never passed over.
            if correctable.contains(&record.id) {
                found.extend(
                    correction_structure_problem(record, authority)
                        .map(|problem| correction_refusal(record, &problem)),
                );
            }
            continue;
        }
        if own {
            match judgement.landing {
                Landed::Planned => {
                    let old = authority
                        .map(|r| r.criteria.items.as_slice())
                        .unwrap_or_default();
                    let delta = criteria_delta(old, &record.criteria.items);
                    if !delta.is_empty() {
                        found.push(Finding {
                            epic_record: None,
                            rule: FROZEN_RULE,
                            message: format!("{} criteria delta: {}", record.id, delta.join("; ")),
                            note: true,
                        });
                    }
                    found.extend(removals(record, old));
                }
                Landed::Landed { witness } => found.push(finding(
                    FROZEN_RULE,
                    format!(
                        "{} changes its criteria, but a completion of it landed (its record at {witness:.9} shows one), so a reopened task keeps its criteria; change them through a planning amendment that names its epic (SPC-013 R-52, R-119, ADR-0078)",
                        record.id
                    ),
                )),
                _ => found.push(finding(
                    FROZEN_RULE,
                    format!(
                        "{} changes its criteria, but whether a completion of it landed cannot be told ({}), so its criteria stay as they were",
                        record.id,
                        unknown.unwrap_or_default()
                    ),
                )),
            }
        } else if correctable.contains(&record.id) && authority.is_some() {
            found.push(correction_finding(record, authority));
        } else if authority.is_some() {
            found.push(finding(FROZEN_RULE, frozen_message(record)));
        }
    }
    found
}

/// A task pull request's findings for its own task `id` (TSK-451): its
/// criteria as [`frozen_criteria`] judges them, and what it changes outside
/// its own record.
fn own_task_findings(
    (head, target): (&Graph, &Graph),
    paths: &[String],
    id: &str,
    correctable: &std::collections::BTreeSet<String>,
    stacked: &[StackedHead],
    judging: &mut Judging<'_>,
) -> Vec<Finding> {
    let mut found = frozen_criteria(
        head,
        target,
        paths,
        &std::collections::BTreeSet::from([id.to_string()]),
        correctable,
        stacked,
        judging,
    );
    found.extend(outside_own_record(
        (head, target),
        paths,
        id,
        correctable,
        stacked,
    ));
    found
}

/// One [`REMOVAL_RULE`] note per criterion of `old` that `record` no longer
/// holds (TSK-451).
fn removals(record: &RecordView, old: &[Criterion]) -> Vec<Finding> {
    old.iter()
        .filter(|item| !record.criteria.items.iter().any(|next| next.id == item.id))
        .map(|item| Finding {
            epic_record: None,
            rule: REMOVAL_RULE,
            message: format!("{} {}", record.id, item.id),
            note: true,
        })
        .collect()
}

/// What a task pull request for `own` changes in another task's or an
/// epic's record, which only a planning amendment may change (TSK-451): any
/// change to its content, or its removal. A record the range adds is new
/// work, not an amendment; a predecessor's record kept as the reviewed head
/// this branch stacks on has it (`stacked`) is that pull request's change;
/// and a record in `correctable` (ADR-0080) is judged by
/// [`frozen_criteria`].
fn outside_own_record(
    (head, target): (&Graph, &Graph),
    changed_paths: &[String],
    own: &str,
    correctable: &std::collections::BTreeSet<String>,
    stacked: &[StackedHead],
) -> Vec<Finding> {
    let mut found = Vec::new();
    for before in target.records.values().filter(|record| {
        record.id != own
            && matches!(record.kind, RecordKind::Epic | RecordKind::Task)
            && !correctable.contains(&record.id)
    }) {
        let after = head.records.get(&before.id);
        let touched = changed_paths.contains(&before.path)
            || after.is_some_and(|record| changed_paths.contains(&record.path));
        if !touched
            || after.is_some_and(|record| {
                record.content == before.content
                    || stacked.iter().any(|stack| {
                        stack.task_id == record.id && stack.record.content == record.content
                    })
            })
        {
            continue;
        }
        let change = match after {
            None => format!("{} removes its record", before.id),
            Some(record) if before.status != record.status => format!(
                "{} changes its status from {} to {}",
                record.id, before.status, record.status
            ),
            Some(record) if before.criteria.signature() != record.criteria.signature() => {
                format!("{} changes its criteria", record.id)
            }
            Some(record) => format!("{} changes its record", record.id),
        };
        found.push(finding(
            FROZEN_RULE,
            format!("{change} in {own}'s pull request; a task pull request amends only its own record, and another task's or an epic's record changes through a planning amendment that names its epic (ADR-0078)"),
        ));
    }
    found
}

fn authority_note(record: &RecordView, source: &str) -> Finding {
    Finding {
        epic_record: None,
        rule: AUTHORITY_NOTE,
        message: format!("{} {source}", record.id),
        note: true,
    }
}

/// The frozen finding's message for a task whose criteria another task's
/// range changed.
fn frozen_message(record: &RecordView) -> String {
    format!("{} changes its criteria on this branch; another task's criteria change by its own PR or by a planning amendment that names its epic (ADR-0078)", record.id)
}

/// The refusal of a correction (ADR-0080) that is more than a text change.
fn correction_refusal(record: &RecordView, problem: &str) -> Finding {
    finding(
        FROZEN_RULE,
        format!("{}; a correction of a completed standalone task changes only the text of its existing criteria, and {problem}", frozen_message(record)),
    )
}

/// What a correction (ADR-0080) may not change in `record` against the
/// `authority` criteria: the criteria set, the tags and form, and the
/// section's structure.
fn correction_structure_problem(
    record: &RecordView,
    authority: Option<&RecordView>,
) -> Option<String> {
    let old = authority
        .map(|r| r.criteria.items.as_slice())
        .unwrap_or_default();
    let errors = authority
        .map(|r| r.criteria.errors.as_slice())
        .unwrap_or_default();
    correction_problem(old, &record.criteria.items).or_else(|| {
        (record.criteria.errors.as_slice() != errors).then(|| {
            format!(
                "this changes the structure of its criteria section ({})",
                record.criteria.errors.join("; ")
            )
        })
    })
}

/// The finding of a correction (ADR-0080) of `record`'s criteria against
/// the `authority` criteria: the delta as a note when only the text
/// changed, a refusal when anything else did.
fn correction_finding(record: &RecordView, authority: Option<&RecordView>) -> Finding {
    if let Some(problem) = correction_structure_problem(record, authority) {
        return correction_refusal(record, &problem);
    }
    let old = authority
        .map(|r| r.criteria.items.as_slice())
        .unwrap_or_default();
    Finding {
        epic_record: None,
        rule: FROZEN_RULE,
        message: format!(
            "{} {CORRECTION}: {}",
            record.id,
            criteria_delta(old, &record.criteria.items).join("; ")
        ),
        note: true,
    }
}

/// One run's judgement of tasks for a pull request (TSK-234): the range,
/// the run's criteria bases and one record store shared by every task the
/// run judges, so history is read once.
pub struct Judging<'r> {
    store: super::landing::RecordStore<'r>,
    repo: &'r Repository,
    range: Range,
    criteria: CriteriaBases,
}

impl<'r> Judging<'r> {
    pub(super) fn new(repo: &'r Repository, range: Range, criteria: CriteriaBases) -> Self {
        Self {
            store: super::landing::RecordStore::new(repo),
            repo,
            range,
            criteria,
        }
    }

    /// The judgement of `task` ([`super::landing`]).
    pub(super) fn judge(&mut self, task: &RecordView) -> super::landing::TaskJudgement {
        judge_in_range(&mut self.store, self.repo, task, self.range, &self.criteria)
    }
}

/// What the note of a text correction (ADR-0080) says it is, after the
/// task id.
pub const CORRECTION: &str = "criteria correction (a completed standalone task, text only), delta";

/// Whether `found` is the note of a text correction (ADR-0080), which
/// `codeflow ci` prints with its own remedy.
#[must_use]
pub fn is_correction(found: &Finding) -> bool {
    found.note && found.rule == FROZEN_RULE && found.message.contains(CORRECTION)
}

/// Why `new` is not a text correction of `old` (ADR-0080): the same
/// criteria in the same order, each with the tags and form it had. `None`
/// when only the text of existing criteria changes.
pub(super) fn correction_problem(old: &[Criterion], new: &[Criterion]) -> Option<String> {
    let ids = |items: &[Criterion]| {
        items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    if ids(old) != ids(new) {
        return Some(format!(
            "this changes the criteria set itself (the target has {}; this range has {})",
            ids(old),
            ids(new)
        ));
    }
    old.iter()
        .zip(new)
        .find(|(before, after)| before.tags() != after.tags())
        .map(|(before, _)| {
            format!(
                "this changes the tags of {} (`(journey)`, `(after release)`, `(serves ...)` or the checkbox form)",
                before.id
            )
        })
}

/// The completed standalone tasks whose criteria text this range may
/// correct (ADR-0080): the range changes planning records only, as a
/// planning amendment may carry them, and each task is `named` by the
/// pull request, or is the task `named` follows up (`follow_up_of`, read
/// at the target when `named` is there, so the range cannot authorise
/// itself), complete and standalone at the target and at the head, and not
/// reopened by the range.
fn correctable(
    repo: &Repository,
    target: &Graph,
    head: &Graph,
    changed_paths: &[String],
    named: &str,
    tips: (Oid, Oid),
    reopened: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeSet<String> {
    if !records_only(repo, tips, changed_paths) {
        return std::collections::BTreeSet::new();
    }
    let follows = target
        .records
        .get(named)
        .or_else(|| head.records.get(named))
        .and_then(|record| record.follow_up_of.as_deref());
    head.records
        .values()
        .filter(|record| record.id == named || follows == Some(record.id.as_str()))
        .filter(|record| !reopened.contains(&record.id))
        .filter(|record| {
            standalone_complete(record)
                && target
                    .records
                    .get(&record.id)
                    .is_some_and(standalone_complete)
        })
        .map(|record| record.id.clone())
        .collect()
}

/// A task record that is complete and belongs to no epic.
fn standalone_complete(record: &RecordView) -> bool {
    record.kind == RecordKind::Task && record.status == "complete" && record.epic_id.is_none()
}

/// Whether the change of `changed_paths` from the target (`tips.0`) to the
/// head (`tips.1`) carries planning records only, as a planning amendment
/// may carry them (ADR-0078): records and plans, with no symbolic link,
/// submodule or unreadable name.
fn records_only(repo: &Repository, tips: (Oid, Oid), changed_paths: &[String]) -> bool {
    changed_paths.iter().all(|path| is_planning_path(path))
        && super::amendment::range_problem(repo, tips.0, tips.1, changed_paths).is_none()
}

/// Whether this range is a validated redaction of the named task. Its
/// redaction review is separate from the preserved completion's review.
///
/// # Errors
/// Returns a diagnostic when the range, record or redaction cannot be read.
pub fn records_redaction(
    root: &std::path::Path,
    base: &str,
    head: &str,
    id: &str,
) -> Result<bool, String> {
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    let resolve = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| error.to_string())
    };
    let (base, head) = (resolve(base)?, resolve(head)?);
    let anchor = history::merge_base(&repo, base, head).map_err(|e| e.to_string())?;
    let before = Graph::from_revision(&repo, &anchor.to_string())?;
    let after = Graph::from_revision(&repo, &head.to_string())?;
    match after.records.get(id) {
        Some(record) => {
            super::redaction::check(&repo, anchor, head, before.records.get(id), record)
        }
        None => Ok(false),
    }
}

/// Whether the range from `base` (the target tip) to `head`, which changes
/// `changed_paths`, only corrects the records of a completed standalone
/// task that `task_id` names or follows up (ADR-0080), and so starts no
/// work. The range is judged on its own diff: the change from the
/// merge-base of `base` and `head` through to `head`, the same range
/// `changed_paths` lists. A task record the target changed after that
/// merge-base, which the head does not touch, is not a change of this
/// range, so a branch behind the target keeps the skip, and a task record
/// that the range's own diff changes is a change of it even when the
/// target tip holds the same bytes. The range qualifies when all of these
/// hold: it changes planning records only; the correctable set (complete
/// and standalone at the target and at the head, and not reopened by the
/// range) is not empty; every task record the range's own diff adds,
/// removes or changes is in that set; at least one changed path is the
/// record of a task in that set; and, when `task_id` is not itself in that
/// set, it is a follow-up the target already holds and the range's own
/// diff leaves its record unchanged. `task_id` may be that follow-up in
/// any status, including blocked, cancelled or awaiting selection, so a
/// range that also changes the follow-up's own record, or that changes no
/// record of the completed task (a plan note, or another task's
/// description), is not a correction and keeps the anchored preflight for
/// tracked work (R-72). The criteria and binding rules still judge a
/// correction.
///
/// # Errors
///
/// Returns a message when the repository, a revision, the merge-base or
/// the records cannot be read.
pub fn records_correction(
    repo_root: &std::path::Path,
    base: &str,
    head: &str,
    task_id: &str,
    changed_paths: &[crate::git::GitName],
) -> Result<bool, String> {
    // OS text rule (issue 79): the paths are git's exact bytes. A name that
    // is not UTF-8 is never a planning record, so the range is more than a
    // correction, never judged on a lookalike.
    let Ok(texts) = changed_paths
        .iter()
        .map(|path| path.rule_text().map(str::to_string))
        .collect::<Result<Vec<String>, _>>()
    else {
        return Ok(false);
    };
    let changed_paths = texts.as_slice();
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let oid = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("{revision}: {}", error.message()))
    };
    let (base_oid, head_oid) = (oid(base)?, oid(head)?);
    let target = Graph::from_revision(&repo, base)?;
    let at_head = Graph::from_revision(&repo, head)?;
    let anchor = history::merge_base(&repo, base_oid, head_oid).map_err(|e| e.to_string())?;
    let anchored = Graph::from_revision(&repo, &anchor.to_string())?;
    if let Some(record) = at_head.records.get(task_id) {
        if super::redaction::check(
            &repo,
            anchor,
            head_oid,
            anchored.records.get(task_id),
            record,
        )? {
            return Ok(true);
        }
    }
    let reopened = reopened_ids(&repo, base, head, &at_head)?;
    let correcting = correctable(
        &repo,
        &target,
        &at_head,
        changed_paths,
        task_id,
        (base_oid, head_oid),
        &reopened,
    );
    if correcting.is_empty() {
        return Ok(false);
    }
    // What the range changes is read against the merge-base, never the
    // target tip: the target may have moved other records since the branch
    // point, or reached the same bytes the range did.
    let anchor = history::merge_base(&repo, base_oid, head_oid)
        .map_err(|error| error.message().to_string())?;
    let at_anchor = Graph::from_revision(&repo, &anchor.to_string())?;
    let touched = |id: &str| match (at_anchor.records.get(id), at_head.records.get(id)) {
        (None, None) => false,
        (Some(before), Some(after)) => before.content != after.content || before.path != after.path,
        _ => true,
    };
    // A follow-up named only by the range is read from the head, so the
    // range could authorise itself; only a follow-up the target holds, with
    // its record unchanged by the range, takes the skip.
    if !correcting.contains(task_id) && (!target.records.contains_key(task_id) || touched(task_id))
    {
        return Ok(false);
    }
    // Every task record the range's diff touches is a correcting one, so a
    // plan note, or a change to the follow-up's own record or to another
    // task, is more than a correction.
    let task_ids: std::collections::BTreeSet<&String> = at_anchor
        .records
        .iter()
        .chain(&at_head.records)
        .filter(|(_, record)| record.kind == RecordKind::Task)
        .map(|(id, _)| id)
        .collect();
    if task_ids
        .iter()
        .any(|id| touched(id) && !correcting.contains(*id))
    {
        return Ok(false);
    }
    // At least one changed path is a correcting task's record.
    Ok(correcting.iter().any(|id| {
        [&at_anchor, &at_head]
            .iter()
            .filter_map(|graph| graph.records.get(id))
            .any(|record| changed_paths.contains(&record.path))
    }))
}

/// The change from `old` to `new` criteria, one entry per criterion
/// removed, changed or added.
pub(super) fn criteria_delta(old: &[Criterion], new: &[Criterion]) -> Vec<String> {
    let mut delta = Vec::new();
    for item in old {
        match new.iter().find(|next| next.id == item.id) {
            None => delta.push(format!(
                "{} removed: {} (give the reason in the PR body)",
                item.id, item.text
            )),
            Some(next) if next.text != item.text => delta.push(format!(
                "{} changed: {} -> {}",
                item.id, item.text, next.text
            )),
            Some(next) => {
                let (before, after) = (CriterionKey::of(item), CriterionKey::of(next));
                if before != after {
                    delta.push(format!(
                        "{} changed its tags with the same text: {} -> {}",
                        item.id,
                        before.tag_list(),
                        after.tag_list()
                    ));
                }
            }
        }
    }
    for item in new
        .iter()
        .filter(|item| !old.iter().any(|prior| prior.id == item.id))
    {
        delta.push(format!("{} added: {}", item.id, item.text));
    }
    delta
}

/// Enforce reopen equality before any range-class amendment exemption.
/// The prospective graph may include an uncommitted status transition.
///
/// # Errors
/// Returns an error if the range cannot be read.
pub fn reopened_criteria(
    repo: &Repository,
    base: &str,
    head: &str,
    after: &Graph,
) -> Result<Vec<Finding>, String> {
    let (reopened, unknown) = reopened_judged(repo, base, head, after)?;
    Ok(reopened
        .into_iter()
        .map(|id| finding(FROZEN_RULE, reopened_message(&id)))
        .chain(unknown.into_iter().map(|(id, reason)| {
            finding(
                FROZEN_RULE,
                format!(
                    "{id}: the target does not hold this task, and whether a completion of it landed before cannot be told ({reason}); {REOPENED_CRITERIA}"
                ),
            )
        }))
        .collect())
}

/// Why a reopened task's changed criteria are refused: one statement for
/// the reopen transition (R-119) and the range freeze (R-52).
pub(super) const REOPENED_CRITERIA: &str = "a reopened task keeps its criteria as the anchored target has them; change them in the epic's batched amendment, a planning pull request on the target";

/// The frozen finding's message for a reopened task whose criteria changed.
pub(super) fn reopened_message(id: &str) -> String {
    format!("{id}: {REOPENED_CRITERIA}")
}

/// The tasks complete at `base` whose criteria differ at `head` and which
/// the range reopened: no longer complete, a block superseded, or a commit
/// of the range moving them from complete. The range is its own diff, the
/// merge-base of `base` and `head` through `head`: a task whose record the
/// range leaves as the merge-base holds it, or whose record is not complete
/// there, is not reopened by it, so a target that moved on after the
/// merge-base reopens nothing for a branch behind it. A task `base` no
/// longer holds is judged against the newest version in its history, so
/// deleting a landed record and adding it again reopened is still a reopen;
/// when that history cannot tell, such a task counts as reopened too.
///
/// # Errors
/// Returns an error if the range cannot be read.
pub(super) fn reopened_ids(
    repo: &Repository,
    base: &str,
    head: &str,
    after: &Graph,
) -> Result<std::collections::BTreeSet<String>, String> {
    let (reopened, unknown) = reopened_judged(repo, base, head, after)?;
    Ok(reopened.into_iter().chain(unknown.into_keys()).collect())
}

/// The tasks the range reopened with changed criteria, and the tasks
/// `base` no longer holds whose history cannot tell, with the reason
/// ([`reopened_ids`]).
#[allow(clippy::too_many_lines)] // Reads target recovery and transitions in one judged range.
fn reopened_judged(
    repo: &Repository,
    base: &str,
    head: &str,
    after: &Graph,
) -> Result<
    (
        std::collections::BTreeSet<String>,
        std::collections::BTreeMap<String, String>,
    ),
    String,
> {
    let _judgment = history::judgment();
    let target = Graph::from_revision(repo, base)?;
    let oid = |rev: &str| {
        repo.revparse_single(rev)
            .and_then(|o| o.peel_to_commit())
            .map(|c| c.id())
            .map_err(|e| e.to_string())
    };
    let base_oid = oid(base)?;
    // A reopen is the range's own change, so a task is a candidate only
    // when the range's own diff (the merge-base through the head) changes
    // its record from a complete one. A task the target changed after the
    // merge-base, which the range does not touch, is not reopened by this
    // range, whatever the target tip holds (R-119, as `records_correction`
    // judges a correction). Without a merge-base nothing is excluded; a
    // merge-base that cannot be read is an error, never none (issue 79).
    let at_anchor = match history::merge_base(repo, base_oid, oid(head)?) {
        Ok(anchor) => Some(Graph::from_revision(repo, &anchor.to_string())?),
        Err(error) if error.code() == git2::ErrorCode::NotFound => None,
        Err(error) => {
            return Err(format!(
                "cannot read the merge-base of {base} and {head}: {}",
                error.message()
            ))
        }
    };
    let mut store = super::landing::RecordStore::new(repo);
    let mut unknown = std::collections::BTreeMap::new();
    let mut candidates: Vec<(&RecordView, RecordView)> = Vec::new();
    for record in after
        .records
        .values()
        .filter(|record| record.kind == RecordKind::Task)
    {
        if let Some(before) = at_anchor
            .as_ref()
            .and_then(|graph| graph.records.get(&record.id))
        {
            let changed = before.content != record.content || before.path != record.path;
            if !changed || before.status != "complete" {
                continue;
            }
        }
        let old = match target.records.get(&record.id) {
            Some(old) => old.clone(),
            None => match store.recovered(&super::landing::Identity::of(record), &[base_oid]) {
                Ok(Some(old)) => old,
                Ok(None) => continue,
                Err(reason) => {
                    // Only a record shaped like a reopen can be one.
                    let reopen_shaped = record.status != "complete"
                        || acceptance_blocks(&record.body)
                            .iter()
                            .any(super::record_text::FencedAcceptance::is_superseded);
                    if reopen_shaped {
                        unknown.insert(record.id.clone(), reason);
                    }
                    continue;
                }
            },
        };
        if old.status == "complete" && old.criteria.signature() != record.criteria.signature() {
            candidates.push((record, old));
        }
    }
    if candidates.is_empty() {
        return Ok((std::collections::BTreeSet::new(), unknown));
    }
    let mut walk = history::revwalk(repo).map_err(|e| e.to_string())?;
    walk.push(oid(head)?).map_err(|e| e.to_string())?;
    walk.hide(base_oid).map_err(|e| e.to_string())?;
    let mut reopened = std::collections::BTreeSet::new();
    for (record, old) in &candidates {
        if record.status != "complete"
            || acceptance_blocks(&record.body)
                .iter()
                .filter(|block| block.is_superseded())
                .count()
                > acceptance_blocks(&old.body)
                    .iter()
                    .filter(|block| block.is_superseded())
                    .count()
        {
            reopened.insert(record.id.clone());
        }
    }
    for oid in walk {
        let oid = oid.map_err(|e| e.to_string())?;
        for (record, _) in &candidates {
            if let Some(content) = blob_at(repo, oid, &record.path)? {
                let then = RecordView::parse(RecordKind::Task, &record.path, &content)?;
                if then.status != "complete" {
                    let commit = repo.find_commit(oid).map_err(|e| e.to_string())?;
                    let mut transitioned = false;
                    for parent in
                        history::parents(repo, &commit).map_err(|error| error.to_string())?
                    {
                        if record_at(repo, parent, record)?
                            .is_some_and(|prior| prior.status == "complete")
                        {
                            transitioned = true;
                        }
                    }
                    if transitioned {
                        reopened.insert(record.id.clone());
                    }
                }
            }
        }
    }
    Ok((reopened, unknown))
}

/// The binding findings of a range: every task record it completes, or
/// whose active block or archived review it changes, with provenance read
/// before applying the shared binding rule.
/// `default_target` stands in for a task that declares no integration
/// target when its waivers are judged; `run` holds the fixed target tips
/// the run is judged against, and `line` is the branch the range judges,
/// whose own line a planning or line range adds to the stacking tips (never
/// the criteria bases) for the tasks that target it.
///
/// `stacked` are the reviewed predecessor heads a pushed task branch stacks
/// on (issue #69): a predecessor completed there, unchanged since, is bound
/// as its own pull request binds it.
///
/// # Errors
///
/// Returns a message when a revision, the merge-base or a tree cannot be
/// read.
#[allow(clippy::too_many_arguments)] // The range, the run's authorities and the stacked heads.
#[allow(clippy::too_many_lines)] // Keep import, reopening and local completion judgments in order.
pub fn completions_in_range(
    repo: &Repository,
    base: &str,
    head: &str,
    default_target: Option<Oid>,
    criteria: &Criteria,
    run: &RunBases,
    line: Option<&str>,
    stacked: &[StackedHead],
) -> Result<Vec<Finding>, String> {
    let _judgment = history::judgment();
    let amendable = criteria.amends();
    // The task a task pull request is for may waive a criterion by a
    // record-only amendment in its own range (TSK-184); no other record may.
    let own_task = match criteria {
        Criteria::OwnTask(id) => Some(id.as_str()),
        _ => None,
    };
    let oid = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("{revision}: {}", error.message()))
    };
    let head_oid = oid(head)?;
    let base_oid = oid(base)?;
    let anchor = history::merge_base(repo, base_oid, head_oid)
        .map_err(|error| error.message().to_string())?;
    let before = Graph::from_revision(repo, &anchor.to_string())?;
    let after = Graph::from_revision(repo, &head_oid.to_string())?;
    let imported = match imported_records(repo, base_oid, head_oid, &after) {
        Ok(imported) => imported,
        Err(reason) => return Ok(vec![finding(IMPORT_RULE, reason)]),
    };
    let mut local = after.clone();
    for id in imported.keys() {
        local.records.remove(id);
    }
    let reopened_in_history =
        super::lifecycle::reopened_in_range(repo, &anchor.to_string(), Some(head), &after)?;

    let (redacted, mut findings) = super::redaction::judge_range(
        repo,
        anchor,
        head_oid,
        &before,
        &local,
        *criteria == Criteria::Amendable,
    );
    let mut reviewed = Vec::new();
    for task in after
        .records
        .values()
        .filter(|record| record.kind == RecordKind::Task && record.status == "complete")
    {
        if redacted.contains(&task.id) || imported.contains_key(&task.id) {
            continue;
        }
        let block = active_block(task);
        // A predecessor completed in its own reviewed pull request, whose
        // head this branch stacks on unchanged (issue #69), is judged as
        // that pull request judges it: bound at the stacked head, from that
        // head's own merge-base with the base.
        if let Some(stack) = stacked.iter().find(|stack| {
            stack.task_id == task.id
                && stack.record.status == "complete"
                && stack.record.content == task.content
        }) {
            findings.extend(bind_stacked(
                repo,
                stack,
                base_oid,
                default_target,
                &with_own_line(run.stacking(), line, task, None),
                &run.criteria(),
            )?);
            continue;
        }
        let reopened = super::lifecycle::is_recompletion(before.records.get(&task.id), task)
            || reopened_in_history.contains(&task.id);
        let unchanged = before
            .records
            .get(&task.id)
            .is_some_and(|then| then.status == "complete" && active_block(then) == block);
        if !unchanged || reopened {
            // A task range owns all its changes. A planning/line range
            // carries each completion from its introduction on that line.
            // Recompletion owns the entire fix range even if its block is
            // byte-identical to an earlier review.
            let introduced = match block.as_ref() {
                Some(block) => introduced_at(repo, task, block, head_oid)?,
                None => head_oid,
            };
            let source_base = if amendable {
                source_landing_base(repo, head_oid, introduced)?
            } else {
                None
            };
            let origin = if amendable && (!reopened || source_base.is_some()) {
                introduced
            } else {
                head_oid
            };
            if amendable {
                findings.push(Finding {
                    epic_record: None,
                    rule: BINDING_RULE,
                    message: format!("{} completion bound at {origin}", task.id),
                    note: true,
                });
            }
            let bound = bind_completion_with_amendment(
                repo,
                task,
                &after,
                Landing::Commit(origin),
                default_target,
                Transport::TaskLanding,
                Some(source_base.unwrap_or(anchor)),
                (own_task == Some(task.id.as_str())).then_some(anchor),
                &with_own_line(run.stacking(), line, task, amendable.then_some(head_oid)),
                &judged_line(repo, run.criteria(), criteria, line, task, head_oid),
            );
            if let Some(proof) = bound.proof {
                reviewed.push(proof.reviewed);
            }
            // A completion made before the range head but bound at it (a
            // frozen range) did not make the head's change: name the head.
            let at_head = (origin == head_oid && introduced != head_oid)
                .then(|| format!("the range head {head_oid} also changes"));
            findings.extend(bound.findings.into_iter().map(|mut found| {
                if let Some(at_head) = &at_head {
                    found.message = found.message.replace(COMPLETION_CHANGES, at_head);
                }
                found
            }));
        }
    }
    findings.extend(epic_completions_in_range(
        repo, &before, &local, head_oid, amendable,
    )?);
    findings.extend(receiving_merges::findings(
        repo, base_oid, head_oid, line, &after, &reviewed,
    )?);
    Ok(findings)
}

/// The target tips a completion in a range may stack merges from: the run's
/// stacking tips, and, in a planning or line range (`line_head`), the range
/// head's own line, but only for a task whose declared target is that line
/// (`line`, the branch the range judges). A task bound for another target
/// never takes that line's merges as its target's, and the line head is
/// stacking authority only, never a criteria base (TSK-220).
fn with_own_line(
    stacking: StackingTips,
    line: Option<&str>,
    task: &RecordView,
    line_head: Option<Oid>,
) -> StackingTips {
    let mut tips = stacking;
    let declared = task
        .integration_target
        .as_deref()
        .filter(|target| !target.is_empty());
    if let (Some(head), Some(line), Some(declared)) = (line_head, line, declared) {
        if super::work_start::logical_target(declared) == super::work_start::logical_target(line) {
            tips = tips.with_line_head(head);
        }
    }
    tips
}

/// The criteria bases for `task` in a range, marking the line the task
/// declares as judged when the range is a verified epic line into its
/// target (`Criteria::Amendable` on an `integration/` line) and the task
/// declares either that line or a joined line (TSK-433). A joined line is
/// another epic line whose every tip the range head contains passes the
/// epic-line proof ([`super::work_start::check_epic_line`]) into the default
/// target; its task gets the judged line marked too. One that fails the
/// proof, or cannot be read, stays unmarked and keeps today's refusal. The mark adds no criteria base: it only keeps the
/// line's own tips that the range head contains from counting as a target
/// the run is not judged against. A planning range, a task range or a task
/// of another target keeps the run's bases alone.
fn judged_line(
    repo: &Repository,
    bases: CriteriaBases,
    criteria: &Criteria,
    line: Option<&str>,
    task: &RecordView,
    head: Oid,
) -> CriteriaBases {
    let declared = task
        .integration_target
        .as_deref()
        .filter(|target| !target.is_empty());
    let (Criteria::Amendable, Some(line), Some(declared)) = (criteria, line, declared) else {
        return bases;
    };
    if !line.starts_with("integration/") {
        return bases;
    }
    let declared = super::work_start::logical_target(declared);
    if declared == super::work_start::logical_target(line) {
        return bases.with_judged_line(line, head);
    }
    // A joined line's task may name the judged line too, as a target it
    // had before it moved to its own line (TSK-422): both lines' tips the
    // head contains are the range. Both marks rest on proofs of the lines'
    // histories, never on the record's text: the joined line at each tip
    // the head contains, and the judged line at the head itself.
    if declared.starts_with("integration/")
        && joined_line(repo, declared, head)
        && epic_line_proved(repo, line, head)
    {
        return bases
            .with_judged_line(declared, head)
            .with_judged_line(line, head);
    }
    bases
}

/// Whether `joined`, an epic line other than the judged one, joined it: at
/// least one of its tips is in the range head, and every such tip passes
/// the epic-line proof into the default target. Any read failure answers
/// no, which keeps the joined tips outside evidence (fail closed).
fn joined_line(repo: &Repository, joined: &str, head: Oid) -> bool {
    let Ok(tips) = target_tips(repo, joined) else {
        return false;
    };
    let mut contained = false;
    for (_, tip) in tips {
        match is_ancestor_or_same(repo, tip, head) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(_) => return false,
        }
        contained = true;
        if !epic_line_proved(repo, joined, tip) {
            return false;
        }
    }
    contained
}

/// Whether `line` at `tip` passes the epic-line proof into the default
/// target ([`super::work_start::check_epic_line`]): an epic that exists, a
/// task of it targeting the line, and a first-parent history of merges or
/// landed adoptions. Any read failure answers no (fail closed).
fn epic_line_proved(repo: &Repository, line: &str, tip: Oid) -> bool {
    let Ok(default) = default_target_name(repo) else {
        return false;
    };
    let Ok(Some(default_tip)) = imports::home_tip(repo, &default.name) else {
        return false;
    };
    let root = repo.workdir().unwrap_or_else(|| repo.path());
    super::work_start::check_epic_line(
        root,
        line,
        &default.name,
        &default_tip.to_string(),
        &tip.to_string(),
    )
    .is_ok()
}

/// The anchored base of the task landing that carried `introduced` onto
/// the source's first-parent chain. The caller supplies an ordinary line or
/// an already verified import source; this never qualifies a release import.
pub(super) fn source_landing_base(
    repo: &Repository,
    source: Oid,
    introduced: Oid,
) -> Result<Option<Oid>, String> {
    let _judgment = history::judgment();
    let mut at = source;
    while at != introduced {
        let commit = repo
            .find_commit(at)
            .map_err(|error| format!("cannot read the source landing at {at}: {error}"))?;
        let Some(first) = history::parents(repo, &commit)
            .map_err(|error| error.to_string())?
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        if !is_ancestor_or_same(repo, introduced, first)? {
            if history::parents(repo, &commit)
                .map_err(|error| error.to_string())?
                .len()
                == 2
            {
                let second =
                    history::parent_id(repo, &commit, 1).map_err(|error| error.to_string())?;
                if is_ancestor_or_same(repo, introduced, second)? {
                    return common_base(repo, first, introduced);
                }
            }
            return Ok(None);
        }
        at = first;
    }
    Ok(None)
}

/// The commit of the range that introduced `task`'s completion with
/// `block`: walk from `head` while a parent still holds it, the first
/// parent before the second. It ends at the first commit whose parents do
/// not hold it; the range's merge-base never does, or the range would not
/// bind this completion.
pub(super) fn introduced_at(
    repo: &Repository,
    task: &RecordView,
    block: &AcceptanceBlock,
    head: Oid,
) -> Result<Oid, String> {
    let _judgment = history::judgment();
    let mut at = head;
    loop {
        let commit = repo
            .find_commit(at)
            .map_err(|error| format!("cannot read completion history at {at}: {error}"))?;
        let mut previous = None;
        for parent in history::parents(repo, &commit)
            .map_err(|error| error.to_string())?
            .into_iter()
            .take(2)
        {
            if record_at(repo, parent, task)?.is_some_and(|record| {
                record.status == "complete"
                    && active_block(&record).as_ref() == Some(block)
                    && record.superseded_blocks() == task.superseded_blocks()
            }) {
                previous = Some(parent);
                break;
            }
        }
        match previous {
            Some(parent) => at = parent,
            None => return Ok(at),
        }
    }
}

/// [`journey_requirement`] for the task as the `head` revision has it.
///
/// # Errors
///
/// Returns a message when the repository or the revision cannot be read.
pub fn journey_requirement_at(
    repo_root: &std::path::Path,
    head: &str,
    task_id: &str,
) -> Result<Option<String>, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let graph = Graph::from_revision(&repo, head)?;
    Ok(journey_requirement(&graph, task_id))
}

/// Whether a range may change task criteria (R-52): a planning amendment
/// or a validated epic integration line. The caller decides it from the
/// pull request's validated class, never from the branch prefix alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Criteria {
    /// Criteria stay as the target has them.
    Frozen,
    /// Only the named, not previously complete task may amend its criteria;
    /// in a range of planning records only, the text of the completed
    /// standalone task it follows up may also be corrected (ADR-0080).
    OwnTask(String),
    /// The named task is complete at the target, on a branch that does not
    /// carry it. Criteria stay frozen, except that a range of planning
    /// records only may correct the text of its criteria, or of the task it
    /// follows up, when that task is standalone (ADR-0080).
    Complete(String),
    /// A validated epic line or the workspace root branch may change them.
    Amendable,
    /// A planning amendment (ADR-0078) may change them, and every change is
    /// reported per epic. `Some` holds the epics its `Task:` line names: a
    /// change to a record of any other epic is refused. A push has no body
    /// yet, so it carries `None` and its report names the epics it finds.
    Amendment(Option<Vec<String>>),
}

impl Criteria {
    /// Whether the range may change criteria and carries completions from
    /// their source on the line.
    #[must_use]
    pub fn amends(&self) -> bool {
        matches!(self, Self::Amendable | Self::Amendment(_))
    }
}

/// Select the own-task amendment only for a task not complete at the
/// target. A task complete there keeps its criteria frozen, and on a branch
/// that does not carry it (`own_branch` false) its text correction route
/// applies (ADR-0080); on its own branch, a range is its reopen and keeps
/// them frozen (R-119).
///
/// # Errors
/// Returns an error when the target records cannot be read.
pub fn task_criteria(
    root: &std::path::Path,
    base: &str,
    task_id: &str,
    own_branch: bool,
) -> Result<Criteria, String> {
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    let graph = Graph::from_revision(&repo, base)?;
    let complete = graph
        .records
        .get(task_id)
        .is_some_and(|record| record.status == "complete");
    Ok(match (complete, own_branch) {
        (false, _) => Criteria::OwnTask(task_id.to_string()),
        (true, true) => Criteria::Frozen,
        (true, false) => Criteria::Complete(task_id.to_string()),
    })
}

/// The findings of a pull request from `base` (the target tip) to `head`:
/// criteria frozen unless `criteria` amends them ([`Criteria::amends`]), a
/// planning amendment's changes reported per epic, and every completion in
/// the range bound. A frozen range is a task pull request and owns its
/// changes; an amending one carries completions from their source on the
/// line. Both apply the same binding rule.
///
/// # Errors
///
/// Returns a message when a revision, the merge-base or a tree cannot be
/// read.
pub fn pull_request_findings(
    repo_root: &std::path::Path,
    base: &str,
    head: &str,
    criteria: &Criteria,
) -> Result<Vec<Finding>, String> {
    pull_request_findings_judged(repo_root, base, head, criteria, None, None, &[])
}

/// [`pull_request_findings`] for a run that also names a candidate
/// authority (the pre-push hook's destination default tip) and the branch
/// it judges: a clean merge after a review may come from the first-parent
/// line of the base or of that authority, and in a planning or line range
/// from the range's own line for the tasks that target `branch`; never from
/// what a local branch or its configuration says (TSK-220).
///
/// `pins` are the reviewed predecessor heads a pushed task branch stacks on
/// (SPC-013 R-42, issue #69), as `work_start::stacked_pins` honoured them
/// after a review row named each; the commits up to each are that
/// predecessor's reviewed pull request, judged as that pull request is.
///
/// # Errors
///
/// Returns a message when a revision, the merge-base or a tree cannot be
/// read.
#[allow(clippy::too_many_lines)] // Keep import, reopening and local completion judgments in order.
pub fn pull_request_findings_judged(
    repo_root: &std::path::Path,
    base: &str,
    head: &str,
    criteria: &Criteria,
    authority: Option<&str>,
    branch: Option<&str>,
    pins: &[super::work_start::ReviewedPin],
) -> Result<Vec<Finding>, String> {
    let _judgment = history::judgment();
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let stacked = stacked_heads(&repo, pins)?;
    let oid = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("{revision}: {}", error.message()))
    };
    let target_tip = oid(base)?;
    let candidate = authority.map(oid).transpose()?;
    let run = RunBases::new(std::iter::once(target_tip).chain(candidate));
    let head_tip = oid(head)?;
    let mut at_head = Graph::from_revision(&repo, head)?;
    let imported = match imported_records(&repo, target_tip, head_tip, &at_head) {
        Ok(imported) => imported,
        Err(reason) => return Ok(vec![finding(IMPORT_RULE, reason)]),
    };
    let anchor = history::merge_base(&repo, target_tip, head_tip).map_err(|e| e.to_string())?;
    let before = Graph::from_revision(&repo, &anchor.to_string())?;
    let (redacted, _) = super::redaction::judge_range(
        &repo,
        anchor,
        head_tip,
        &before,
        &at_head,
        *criteria == Criteria::Amendable,
    );
    for id in redacted {
        at_head.records.remove(&id);
    }
    let mut local_head = at_head.clone();
    for id in imported.keys() {
        local_head.records.remove(id);
    }
    // Resolution 43: a reopened task keeps its criteria, whatever the
    // range's class allows.
    let mut found = reopened_criteria(&repo, base, head, &local_head)?;
    if *criteria != Criteria::Amendable {
        let mut paths = super::lifecycle::changed_paths(&repo, &anchor.to_string(), Some(head))?;
        paths.retain(|path| !imported.values().any(|source| source.path == *path));
        let at_target = Graph::from_revision(&repo, base)?;
        let range = Range {
            anchor,
            head: head_tip,
        };
        let mut judging = Judging::new(&repo, range, run.criteria());
        // ADR-0080: the named task, or the task it follows up, may have the
        // text of its completed standalone criteria corrected.
        let reopened = reopened_ids(&repo, base, head, &local_head)?;
        let correcting = |named: &str| {
            correctable(
                &repo,
                &at_target,
                &at_head,
                &paths,
                named,
                (target_tip, head_tip),
                &reopened,
            )
        };
        let none = std::collections::BTreeSet::new;
        match criteria {
            Criteria::Amendment(named) => found.extend(super::amendment::findings(
                &repo,
                &at_target,
                &at_head,
                &paths,
                named.as_deref(),
                target_tip,
                &mut judging,
            )),
            Criteria::OwnTask(id) => found.extend(own_task_findings(
                (&at_head, &at_target),
                &paths,
                id,
                &correcting(id),
                &stacked,
                &mut judging,
            )),
            Criteria::Complete(id) => found.extend(frozen_criteria(
                &at_head,
                &at_target,
                &paths,
                &none(),
                &correcting(id),
                &stacked,
                &mut judging,
            )),
            _ => found.extend(frozen_criteria(
                &at_head,
                &at_target,
                &paths,
                &none(),
                &none(),
                &stacked,
                &mut judging,
            )),
        }
    }
    found.extend(stacked_records_kept(&at_head, &stacked));
    found.extend(completions_in_range(
        &repo,
        base,
        head,
        Some(target_tip),
        criteria,
        &run,
        branch,
        &stacked,
    )?);
    Ok(found)
}

/// Bind a predecessor's completion as its own reviewed pull request binds
/// it (issue #69): at the stacked head, from that head's own merge-base
/// with `base`, with that pull request's own-range waiver route.
fn bind_stacked(
    repo: &Repository,
    stack: &StackedHead,
    base: Oid,
    default_target: Option<Oid>,
    stacking: &StackingTips,
    criteria: &CriteriaBases,
) -> Result<Vec<Finding>, String> {
    let own_anchor =
        history::merge_base(repo, base, stack.pin).map_err(|error| error.message().to_string())?;
    let mut findings = vec![Finding {
        epic_record: None,
        rule: BINDING_RULE,
        message: format!(
            "{} completion is its own reviewed pull request's, bound at {}, which this branch stacks on",
            stack.task_id, stack.pin
        ),
        note: true,
    }];
    findings.extend(
        bind_completion_with_amendment(
            repo,
            &stack.record,
            &stack.graph,
            Landing::Commit(stack.pin),
            default_target,
            Transport::TaskLanding,
            Some(own_anchor),
            Some(own_anchor),
            stacking,
            criteria,
        )
        .findings,
    );
    Ok(findings)
}

/// A branch stacked on a predecessor's reviewed head carries that
/// predecessor's record as the head has it (issue #69): the commits up to
/// the pin are the predecessor's pull request, and the successor's own
/// commits never change, revert or remove what that pull request recorded.
/// Each record that differs from its pinned version is refused.
fn stacked_records_kept(head: &Graph, stacked: &[StackedHead]) -> Vec<Finding> {
    stacked
        .iter()
        .filter(|stack| {
            head.records
                .get(&stack.task_id)
                .is_none_or(|record| record.content != stack.record.content)
        })
        .map(|stack| {
            finding(
                FROZEN_RULE,
                format!(
                    "{0} differs on this branch from its reviewed head {1:.9}, which this branch stacks on; a successor never changes its predecessor's record: restore {0}'s record as {1:.9} has it, or merge the predecessor's newer reviewed head",
                    stack.task_id, stack.pin
                ),
            )
        })
        .collect()
}

/// A reviewed predecessor head a range stacks on, with that predecessor's
/// record and the records graph there (issue #69). Built only from a
/// [`super::work_start::ReviewedPin`], which a review row named.
pub struct StackedHead {
    task_id: String,
    pin: Oid,
    record: RecordView,
    graph: Graph,
}

/// The stacked heads of `pins`, each read at its pin.
fn stacked_heads(
    repo: &Repository,
    pins: &[super::work_start::ReviewedPin],
) -> Result<Vec<StackedHead>, String> {
    pins.iter()
        .map(|pin| {
            let graph = Graph::from_revision(repo, &pin.revision().to_string())?;
            let record = graph.records.get(pin.task_id()).cloned().ok_or_else(|| {
                format!(
                    "{} is not recorded at its reviewed head {}",
                    pin.task_id(),
                    pin.revision()
                )
            })?;
            Ok(StackedHead {
                task_id: pin.task_id().to_string(),
                pin: pin.revision(),
                record,
                graph,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    fn r22_remove_object(repo: &Repository, oid: Oid) {
        let hex = oid.to_string();
        std::fs::remove_file(repo.path().join("objects").join(&hex[..2]).join(&hex[2..])).unwrap();
    }

    #[test]
    fn r22_historical_presence_refuses_malformed_other_name() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, head) = crate::git::repo_with_tree(
            dir.path(),
            &[(
                b"project-management/tasks/TSK-999.md",
                b"---\nid: TSK-001\nstatus: [unclosed\n---\n",
            )],
        );
        assert!(matches!(
            presence_at(&repo, head, "TSK-001", None, "TSK-001.md"),
            Presence::Unreadable
        ));
    }

    #[test]
    fn r22_historical_presence_does_not_hide_unreadable_behind_a_match() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, head) = crate::git::repo_with_tree(
            dir.path(),
            &[
                (
                    b"project-management/tasks/TSK-001.md",
                    b"---\nid: TSK-001\nstatus: todo\n---\n",
                ),
                (
                    b"project-management/tasks/TSK-999.md",
                    b"---\nid: [unclosed\n---\n",
                ),
            ],
        );
        assert!(matches!(
            presence_at(&repo, head, "TSK-001", None, "TSK-001.md"),
            Presence::Unreadable
        ));
    }

    #[test]
    fn r22_historical_presence_refuses_missing_other_blob() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, head) = crate::git::repo_with_tree(
            dir.path(),
            &[(
                b"project-management/tasks/TSK-999.md",
                b"---\nid: TSK-001\nstatus: todo\n---\n",
            )],
        );
        let blob = repo
            .find_commit(head)
            .unwrap()
            .tree()
            .unwrap()
            .get_path(std::path::Path::new("project-management/tasks/TSK-999.md"))
            .unwrap()
            .id();
        r22_remove_object(&repo, blob);
        drop(repo);
        let repo = Repository::open(dir.path()).unwrap();
        assert!(matches!(
            presence_at(&repo, head, "TSK-001", None, "TSK-001.md"),
            Presence::Unreadable
        ));
    }

    #[test]
    fn r22_historical_readers_refuse_missing_referenced_tree() {
        for path in [
            "project-management",
            "project-management/tasks",
            "project-management/epics",
            "project-management/epics/EPC-001",
            "project-management/epics/EPC-001/tasks",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (repo, head) = crate::git::repo_with_tree(
                dir.path(),
                &[
                    (
                        b"project-management/tasks/TSK-001.md",
                        b"---\nid: TSK-001\nstatus: todo\n---\n",
                    ),
                    (
                        b"project-management/epics/EPC-001/tasks/TSK-002.md",
                        b"---\nid: TSK-002\nstatus: todo\n---\n",
                    ),
                ],
            );
            let tree = repo
                .find_commit(head)
                .unwrap()
                .tree()
                .unwrap()
                .get_path(std::path::Path::new(path))
                .unwrap()
                .id();
            r22_remove_object(&repo, tree);
            drop(repo);
            let repo = Repository::open(dir.path()).unwrap();
            assert!(
                matches!(
                    presence_at(&repo, head, "TSK-009", None, "TSK-009.md"),
                    Presence::Unreadable
                ),
                "{path}"
            );
        }
    }

    #[test]
    fn r22_historical_readers_refuse_non_utf8_epic_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, head) = crate::git::repo_with_tree(
            dir.path(),
            &[(
                b"project-management/epics/EPC-001\xff/tasks/TSK-001.md",
                b"---\nid: TSK-001\nstatus: todo\n---\n",
            )],
        );
        assert!(matches!(
            presence_at(&repo, head, "TSK-001", None, "TSK-001.md"),
            Presence::Unreadable
        ));
    }

    #[test]
    fn r22_historical_readers_keep_absent_directories_and_other_identity() {
        for files in [
            vec![],
            vec![(
                b"project-management".as_slice(),
                b"ordinary file".as_slice(),
            )],
            vec![(
                b"project-management/epics/EPC-001\xff/tasks/note.md".as_slice(),
                b"not a task record".as_slice(),
            )],
            vec![(
                b"project-management/tasks/TSK-002.md".as_slice(),
                b"---\nid: TSK-002\nstatus: todo\n---\n".as_slice(),
            )],
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (repo, head) = crate::git::repo_with_tree(dir.path(), &files);
            assert!(matches!(
                presence_at(&repo, head, "TSK-001", None, "TSK-001.md"),
                Presence::Absent
            ));
        }
    }

    #[test]
    fn r22_ancestry_read_failure_is_not_false() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, head) = crate::git::repo_with_tree(dir.path(), &[]);
        assert!(format!("{:?}", is_ancestor_or_same(&repo, head, head)).contains("true"));
        assert!(format!("{:?}", source_landing_base(&repo, head, head)).contains("None"));
        assert!(
            format!("{:?}", is_ancestor_or_same(&repo, Oid::ZERO_SHA1, head)).starts_with("Err(")
        );
        assert!(
            format!("{:?}", source_landing_base(&repo, Oid::ZERO_SHA1, head)).starts_with("Err(")
        );
    }

    #[test]
    fn r22_common_base_keeps_disjoint_history_but_refuses_missing_objects() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, first) = crate::git::repo_with_tree(dir.path(), &[]);
        let tree = repo.find_commit(first).unwrap().tree().unwrap();
        let signature = git2::Signature::now("fixture", "fixture@example.com").unwrap();
        let second = repo
            .commit(None, &signature, &signature, "unrelated root", &tree, &[])
            .unwrap();
        assert_eq!(common_base(&repo, first, second).unwrap(), None);
        assert!(!is_ancestor_or_same(&repo, first, second).unwrap());
        assert_eq!(source_landing_base(&repo, first, second).unwrap(), None);
        assert_eq!(common_base(&repo, first, first).unwrap(), Some(first));
        assert!(common_base(&repo, first, Oid::ZERO_SHA1).is_err());
        assert!(source_landing_base(&repo, Oid::ZERO_SHA1, first).is_err());
    }

    #[test]
    fn r22_the_judgement_refuses_an_unreadable_criteria_base() {
        let dir = tempfile::tempdir().unwrap();
        let text = "---\nid: TSK-001\nstatus: todo\nintegration_target: main\n---\n";
        let path = "project-management/tasks/TSK-001.md";
        let (repo, head) =
            crate::git::repo_with_tree(dir.path(), &[(path.as_bytes(), text.as_bytes())]);
        let task = RecordView::parse(RecordKind::Task, path, text).unwrap();
        let result = judge_reopened(
            &repo,
            &task,
            Range { anchor: head, head },
            &task,
            &RunBases::new([Oid::ZERO_SHA1]).criteria(),
            head,
        );
        assert!(matches!(
            result.landing,
            super::super::landing::Landing::Unreadable(_)
        ));
        assert!(matches!(
            judge_reopened(
                &repo,
                &task,
                Range { anchor: head, head },
                &task,
                &CriteriaBases::default(),
                head,
            )
            .landing,
            super::super::landing::Landing::Planned
        ));
    }

    #[test]
    fn r21_record_presence_refuses_undecodable_target() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let head = crate::git::add_commit(
            &repo,
            &[(
                b"project-management/tasks/TSK-001.md",
                b"---\nid: TSK-001\nstatus: todo\nintegration_target: caf\xff\n---\n",
            )],
        );
        assert!(matches!(
            super::presence_at(&repo, head, "TSK-001", None, "TSK-001.md"),
            super::Presence::Unreadable
        ));
    }

    use super::*;

    fn items(section: &str) -> Vec<Criterion> {
        super::super::record_text::parse_criteria(&format!("## Acceptance Criteria\n\n{section}"))
            .items
    }

    /// ADR-0080: a correction changes only the text of existing criteria;
    /// the set, its order and every tag the parser reads stay.
    #[test]
    fn a_correction_keeps_the_criteria_set_and_every_tag() {
        let old = items("- AC-1 When run, it shall work for Jane Doe.\n- AC-2 (journey) On a fresh project, it shall pass.\n- AC-3 It shall serve (serves EPC-001 AC-1).\n");
        let text = items("- AC-1 When run, it shall work for the owner.\n- AC-2 (journey) On a new project, it shall pass.\n- AC-3 It shall still serve (serves EPC-001 AC-1).\n");
        assert_eq!(correction_problem(&old, &text), None);
        for (changed, reason) in [
            ("- AC-1 a\n- AC-2 (journey) b\n", "criteria set"),
            (
                "- AC-1 a\n- AC-2 (journey) b\n- AC-3 c (serves EPC-001 AC-1)\n- AC-4 d\n",
                "criteria set",
            ),
            (
                "- AC-1 a\n- AC-3 c (serves EPC-001 AC-1)\n- AC-2 (journey) b\n",
                "criteria set",
            ),
            (
                "- AC-1 a\n- AC-2 (journey) b\n- AC-4 c (serves EPC-001 AC-1)\n",
                "criteria set",
            ),
            (
                "- AC-1 a\n- AC-2 b\n- AC-3 c (serves EPC-001 AC-1)\n",
                "tags of AC-2",
            ),
            (
                "- AC-1 a (after release)\n- AC-2 (journey) b\n- AC-3 c (serves EPC-001 AC-1)\n",
                "tags of AC-1",
            ),
            (
                "- AC-1 a\n- AC-2 (journey) b\n- AC-3 c (serves EPC-001 AC-2)\n",
                "tags of AC-3",
            ),
            (
                "- [x] AC-1 a\n- AC-2 (journey) b\n- AC-3 c (serves EPC-001 AC-1)\n",
                "tags of AC-1",
            ),
        ] {
            let problem = correction_problem(&old, &items(changed)).unwrap_or_default();
            assert!(problem.contains(reason), "{changed}: {problem}");
        }
    }

    #[test]
    fn the_judgement_requires_the_exact_declared_reference() {
        let dir = tempfile::tempdir().unwrap();
        let path = "project-management/tasks/TSK-001.md";
        let text = "---\nid: TSK-001\nstatus: todo\nintegration_target: \"release\u{a0}\"\n---\n\n## Acceptance Criteria\n\n- AC-1 Preserve identity.\n";
        let (repo, head) =
            crate::git::repo_with_tree(dir.path(), &[(path.as_bytes(), text.as_bytes())]);
        repo.reference("refs/heads/release", head, true, "fixture")
            .unwrap();
        let task = RecordView::parse(RecordKind::Task, path, text).unwrap();
        let range = Range { anchor: head, head };
        let result = judge_reopened(&repo, &task, range, &task, &CriteriaBases::default(), head);
        assert!(
            matches!(result.landing, super::super::landing::Landing::Unreadable(ref reason) if reason.contains("`release\u{a0}`")),
            "a lookalike reference must not satisfy the declared target"
        );
        repo.reference("refs/heads/release\u{a0}", head, true, "fixture")
            .unwrap();
        assert!(matches!(
            judge_reopened(&repo, &task, range, &task, &CriteriaBases::default(), head).landing,
            super::super::landing::Landing::Planned
        ));
    }

    #[test]
    fn record_uid_is_exact_after_yaml_decoding() {
        assert_eq!(
            record_uid("---\nuid: \"key\\u00a0\"\n---\n"),
            Some("key\u{a0}".into())
        );
    }

    #[test]
    fn only_the_status_line_and_the_closeout_are_outside_review() {
        let base = "---\nid: TSK-001\nstatus: todo  # x\n---\n\n# TSK-001\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n\n## Closeout\n\nPending.\n";
        let done = base
            .replace("status: todo  # x", "status: complete  # x")
            .replace("Pending.", "```yaml\nacceptance:\n## not a heading\n```\n");
        assert_eq!(reviewed_part(base), reviewed_part(&done));
        let criteria = base.replace("shall work", "shall work well");
        assert_ne!(reviewed_part(base), reviewed_part(&criteria));
        let title = base.replace("id: TSK-001", "id: TSK-001\ntitle: other");
        assert_ne!(reviewed_part(base), reviewed_part(&title));
        let after_closeout = format!("{base}\n## Notes\n\nNew.\n");
        assert_ne!(reviewed_part(base), reviewed_part(&after_closeout));
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let out = crate::git::command()
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A task is found by its id or uid in any supported layout; a task
    /// path that names it but does not parse is unreadable, never absent.
    #[test]
    fn presence_finds_a_task_by_identity_in_any_layout() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        let uid = "6f1c2b8e-3d4a-4f5b-9c6d-7e8f9a0b1c2d";
        let record = |id: &str| {
            format!(
                "---\nid: {id}\nuid: \"{uid}\"  # written once\nstatus: todo\n---\n\n# {id}: work\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n\n## Closeout\n\nPending.\n"
            )
        };
        // Every YAML form of one value is the same uid.
        for form in [uid.to_string(), format!("'{uid}'"), format!("\"{uid}\"")] {
            let text = format!("---\nid: TSK-001\nuid: {form}\nstatus: todo\n---\n");
            assert_eq!(record_uid(&text).as_deref(), Some(uid), "{form}");
        }
        let commit = |path: &str, text: &str| {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, text).unwrap();
            git(root, &["add", "-A"]);
            git(root, &["commit", "-qm", "record"]);
            Repository::open(root)
                .unwrap()
                .head()
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .id()
        };
        let legacy = commit(
            "project-management/epics/EPC-001/tasks/TSK-001.md",
            &record("TSK-001"),
        );
        let repo = Repository::open(root).unwrap();
        let at = |commit, id, uid, own| presence_at(&repo, commit, id, uid, own);
        assert!(matches!(
            at(legacy, "TSK-001", None, "TSK-001.md"),
            Presence::Present(_)
        ));
        // The same uid under another number is the same record.
        assert!(matches!(
            at(legacy, "TSK-009", Some(uid), "TSK-009.md"),
            Presence::Present(_)
        ));
        assert!(matches!(
            at(legacy, "TSK-002", None, "TSK-002.md"),
            Presence::Absent
        ));
        // A malformed record may be a renamed copy. Its identity cannot
        // be excluded, so even a different filename is unreadable.
        let mentions = commit(
            "project-management/tasks/TSK-099.md",
            "---\nid: TSK-099\nstatus: [unclosed\n---\nSee TSK-002.\n",
        );
        assert!(matches!(
            at(mentions, "TSK-002", None, "TSK-002.md"),
            Presence::Unreadable
        ));
        let broken = commit(
            "project-management/tasks/TSK-002.md",
            "---\nid: TSK-002\nstatus: [unclosed\n---\n",
        );
        assert!(matches!(
            at(broken, "TSK-002", None, "TSK-002.md"),
            Presence::Unreadable
        ));
        // The reader takes `.md` in any case, so a broken landed `.MD` is
        // the own file of a task whose current record is `.md` (TSK-220).
        let upper = commit(
            "project-management/tasks/TSK-003.MD",
            "---\nid: TSK-003\nstatus: [unclosed\n---\n",
        );
        assert!(matches!(
            at(upper, "TSK-003", None, "TSK-003.md"),
            Presence::Unreadable
        ));
    }

    /// The record store finds a task whose id and uid are spelled with YAML
    /// escapes, so no spelling hides a version from the history walks.
    #[test]
    fn the_record_store_reads_an_escaped_identity() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        let path = root.join("project-management/tasks/TSK-003.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "---\nid: \"\\x54SK-003\"\nuid: \"\\x36f1c2b8e-3d4a-4f5b-9c6d-7e8f9a0b1c2d\"\nstatus: todo\nintegration_target: main\n---\n\n# Work\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n\n## Closeout\n\nPending.\n",
        )
        .unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", "record"]);
        let repo = Repository::open(root).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap().id();
        for (id, uid) in [
            ("TSK-003", None),
            (
                "TSK-777",
                Some("6f1c2b8e-3d4a-4f5b-9c6d-7e8f9a0b1c2d".to_string()),
            ),
        ] {
            let mut store = super::super::landing::RecordStore::new(&repo);
            let identity = super::super::landing::Identity::named(id, uid, &format!("{id}.md"));
            let records = store.records_at(head, &identity).unwrap();
            assert_eq!(records.len(), 1, "{id}");
            assert_eq!(records[0].integration_target.as_deref(), Some("main"));
        }
    }

    /// The merge rule takes a landing merge only when its tree is the clean
    /// re-merge of its parents: a plain merge is; a merge that adds a file
    /// of its own (an evil merge) or resolves a conflict is not.
    #[test]
    fn a_landing_merge_is_its_clean_re_merge() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let write = |path: &str, text: &str| std::fs::write(root.join(path), text).unwrap();
        git(root, &["init", "-q", "-b", "line"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        write("a.txt", "a\n");
        write("b.txt", "b\n");
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "base"]);
        git(root, &["switch", "-qc", "task"]);
        write("a.txt", "task\n");
        git(root, &["commit", "-qam", "task"]);
        git(root, &["switch", "-q", "line"]);
        write("b.txt", "line\n");
        git(root, &["commit", "-qam", "line"]);
        let repo = Repository::open(root).unwrap();
        let clean = || {
            let tip = repo.head().unwrap().peel_to_commit().unwrap();
            is_clean_remerge(&repo, &tip).unwrap()
        };

        git(root, &["merge", "-q", "--no-ff", "-m", "clean", "task"]);
        assert!(clean(), "a plain merge");

        git(root, &["reset", "-q", "--hard", "HEAD^"]);
        git(root, &["merge", "-q", "--no-ff", "--no-commit", "task"]);
        write("evil.txt", "evil\n");
        git(root, &["add", "evil.txt"]);
        git(root, &["commit", "-qm", "evil"]);
        assert!(!clean(), "an evil merge");

        git(root, &["reset", "-q", "--hard", "HEAD^"]);
        write("a.txt", "line too\n");
        git(root, &["commit", "-qam", "conflicting"]);
        let merged = crate::git::command()
            .args(["merge", "-q", "--no-ff", "task"])
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(!merged.status.success(), "the merge conflicts");
        write("a.txt", "resolved\n");
        git(root, &["commit", "-qam", "resolved"]);
        assert!(!clean(), "a resolved conflict");
    }

    /// TSK-264 AC-12 (ADR-0082): a branch that added a changelog fragment
    /// merges a target that landed another one as its clean re-merge, so the
    /// review carries; two branches that insert into one shared pending
    /// section at the same place do not.
    #[test]
    fn two_fragment_branches_merge_as_their_clean_re_merge() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let write = |path: &str, text: &str| {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        let entry = |label: &str| {
            format!("<!-- codeflow:release-impact patch -->\n- **{label}.** Detail.\n")
        };
        let section = |entries: &[&str]| {
            let body: Vec<String> = entries.iter().map(|label| entry(label)).collect();
            format!(
                "# Changelog\n\n## [2.0.1]\n\n### Fixed\n\n{}\n## [2.0.0]\n\n- public\n",
                body.join("\n")
            )
        };
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        write("CHANGELOG.md", &section(&["Carried"]));
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "base"]);
        let repo = Repository::open(root).unwrap();
        let tip = || {
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            read_merge(&repo, &head)
        };

        for (name, label) in [("one", "One"), ("two", "Two")] {
            git(root, &["switch", "-qc", name, "main"]);
            write(
                &format!("changelog.d/{name}.md"),
                &format!("### Fixed\n\n{}", entry(label)),
            );
            git(root, &["add", "."]);
            git(root, &["commit", "-qm", name]);
        }
        git(root, &["switch", "-q", "two"]);
        git(root, &["merge", "-q", "--no-ff", "-m", "merge one", "one"]);
        assert_eq!(tip(), MergeReading::Clean, "two fragments");

        for (name, label) in [("shared-one", "One"), ("shared-two", "Two")] {
            git(root, &["switch", "-qc", name, "main"]);
            write("CHANGELOG.md", &section(&["Carried", label]));
            git(root, &["commit", "-qam", name]);
        }
        let merged = crate::git::command()
            .args(["merge", "-q", "--no-ff", "shared-one"])
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(!merged.status.success(), "one shared section conflicts");
        write("CHANGELOG.md", &section(&["Carried", "One", "Two"]));
        git(root, &["commit", "-qam", "resolved"]);
        assert!(
            matches!(tip(), MergeReading::Changed(paths) if paths == [b"CHANGELOG.md".to_vec()]),
            "a shared section"
        );
    }

    /// A `## Closeout` line inside a comment or a fence is not the Closeout,
    /// so the text after it stays under review; a `status:` line outside the
    /// frontmatter is reviewed text.
    #[test]
    fn only_the_parsed_closeout_is_outside_review() {
        let base = "---\nid: TSK-001\nstatus: todo\n---\n\n# TSK-001\n\n## Description\n\n{hidden}\n\nVisible scope.\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n\n## Closeout\n\nPending.\n";
        for hidden in [
            "<!--\n## Closeout\n-->",
            "````text\n```\n## Closeout\n```\n````",
            "~~~\n## Closeout\n~~~",
        ] {
            let before = base.replace("{hidden}", hidden);
            let narrowed = before.replace("Visible scope.", "Narrower scope.");
            assert_ne!(reviewed_part(&before), reviewed_part(&narrowed), "{hidden}");
            let done = before
                .replace("status: todo", "status: complete")
                .replace("Pending.", "Done.");
            assert_eq!(reviewed_part(&before), reviewed_part(&done), "{hidden}");
        }
        let body_status = base
            .replace("{hidden}", "status: todo")
            .replace("status: todo\n\nVisible", "status: done\n\nVisible");
        assert_ne!(
            reviewed_part(&base.replace("{hidden}", "status: todo")),
            reviewed_part(&body_status)
        );
    }

    /// Review finding on issue 79: an `origin/HEAD` that names a branch whose
    /// name is not valid UTF-8 fell back to `main`, so the task was measured
    /// against the wrong target. The default target is refused instead.
    #[test]
    fn a_default_target_that_is_not_utf8_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let repo = crate::git::repo_with_refs(
            dir.path(),
            &[b"refs/heads/main", b"refs/remotes/origin/caf\xe9"],
        );
        let origin = dir
            .path()
            .join(".git")
            .join("refs")
            .join("remotes")
            .join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        std::fs::write(origin.join("HEAD"), b"ref: refs/remotes/origin/caf\xe9\n").unwrap();
        let error = default_target_name(&repo).err().unwrap();
        assert!(error.contains("not valid UTF-8"), "{error}");
    }

    /// Issue 79: a path that is not valid UTF-8 under `project-management/`
    /// is no planning record, and was read as one through its lossy spelling.
    #[test]
    fn a_changed_path_that_is_not_utf8_is_outside_the_planning_records() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, first) = crate::git::repo_with_tree(
            dir.path(),
            &[(b"project-management/tasks/TSK-001.md", b"x")],
        );
        let second =
            crate::git::add_commit(&repo, &[(b"project-management/tasks/TSK-002\xff.md", b"y")]);
        let outside = non_planning_change(&repo, Some(first), second).unwrap();
        assert_eq!(
            outside.as_deref(),
            Some(r"project-management/tasks/TSK-002\xff.md")
        );
        let third =
            crate::git::add_commit(&repo, &[(b"project-management/tasks/TSK-003.md", b"z")]);
        assert_eq!(
            non_planning_change(&repo, Some(second), third).unwrap(),
            None
        );
    }

    /// A completion's changes outside its record are named by their exact
    /// path: `caf` plus an invalid byte and `caf` plus U+FFFD are two paths.
    #[test]
    fn two_changed_paths_that_differ_in_an_invalid_byte_stay_two() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, first) = crate::git::repo_with_tree(dir.path(), &[(b"a", b"x")]);
        let second = crate::git::add_commit(
            &repo,
            &[(b"caf\xe9", b"1"), ("caf\u{fffd}".as_bytes(), b"2")],
        );
        let tree = |oid| repo.find_commit(oid).unwrap().tree().unwrap();
        let diff = repo
            .diff_tree_to_tree(Some(&tree(first)), Some(&tree(second)), None)
            .unwrap();
        let paths = crate::git::diff_paths(&diff);
        assert_eq!(paths.len(), 2);
        assert_ne!(paths[0], paths[1]);
    }
    #[test]
    fn r16_blob_read_distinguishes_absence_from_failure() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, oid) = crate::git::repo_with_tree(dir.path(), &[(b"record.md", b"bad\xff")]);
        assert!(blob_at(&repo, oid, "record.md").is_err());
        assert!(blob_at(&repo, git2::Oid::ZERO_SHA1, "missing.md").is_err());
        assert!(blob_at(&repo, oid, "missing.md").unwrap().is_none());
    }
    /// TSK-340 AC-1 (issue #198): the Blocker `task status` writes and
    /// removes is not reviewed scope; any other Blocker text, the criteria,
    /// the deliverables and the rest of the record stay reviewed.
    #[test]
    fn r340_a_generated_blocker_is_not_reviewed_scope() {
        let reviewed = "---\nid: TSK-001\nstatus: todo\n---\n# TSK-001\n\n## Deliverables\n\n- src/lib.rs\n\n## Acceptance Criteria\n\n- AC-1 When run, it works.\n\n## Closeout\n\nPending.\n";
        let blocker = "## Blocker\n\n- reason: waits on the host\n- owner: ops\n- revisit: the host is ready\n\n## Closeout";
        let held = reviewed
            .replace("status: todo", "status: blocked")
            .replace("## Closeout", blocker);
        assert_eq!(reviewed_part(reviewed), reviewed_part(&held));
        for (what, forged) in [
            (
                "extra text",
                blocker.replace("ready\n", "ready\n\nAlso ship src/extra.rs.\n"),
            ),
            (
                "extra item",
                blocker.replace("- owner", "- AC-2 When run, it also deploys.\n- owner"),
            ),
            ("empty value", blocker.replace("- owner: ops", "- owner: ")),
            ("missing item", blocker.replace("- owner: ops\n", "")),
            (
                "hidden text",
                blocker.replace("ready\n", "ready\n<!--\nhidden\n-->\n"),
            ),
            (
                "other heading",
                blocker.replace("## Blocker", "## Blocker notes"),
            ),
        ] {
            let forged = reviewed.replace("## Closeout", &forged);
            assert_ne!(reviewed_part(reviewed), reviewed_part(&forged), "{what}");
        }
        for (what, changed) in [
            ("criteria", held.replace("it works", "it may work")),
            ("deliverables", held.replace("src/lib.rs", "src/other.rs")),
        ] {
            assert_ne!(reviewed_part(reviewed), reviewed_part(&changed), "{what}");
        }
    }

    /// TSK-340 AC-2 (issue #217): one chronological archive is accepted
    /// against the branch's last completion and against an earlier
    /// completion on the anchored target; a reordered, dropped or rewritten
    /// archived block is refused.
    #[test]
    fn r340_one_chronological_archive_reads_against_either_completion() {
        let fence = |inner: &str| format!("```yaml\n{inner}```\n");
        let block = |reviewed: char, reason: Option<&str>| {
            let head = match reason {
                Some(reason) => format!("acceptance_superseded:\n  reason: {reason}\n"),
                None => "acceptance:\n".to_string(),
            };
            fence(&format!(
                "{head}  reviewed: {}\n  review: session:r@sha256:00\n  criteria:\n    AC-1: verified | ran\n  journey: none | no journey criterion\n  not_verified: none\n  follow_ups: none: test\n  verdict: approved\n",
                reviewed.to_string().repeat(40)
            ))
        };
        let record = |status: &str, closeout: &[String]| {
            let content = format!(
                "---\nid: TSK-001\nstatus: {status}\n---\n# TSK-001\n\n## Acceptance Criteria\n\n- AC-1 When run, it works.\n\n## Closeout\n\n{}",
                closeout.join("\n")
            );
            RecordView::parse(
                RecordKind::Task,
                "project-management/tasks/TSK-001.md",
                &content,
            )
            .unwrap()
        };
        let a = |reason| block('a', reason);
        let b = |reason| block('b', reason);
        let target = record("complete", &[a(None)]);
        let line = record("complete", &[a(Some("first")), b(None)]);
        let reopened = record("todo", &[a(Some("first")), b(Some("second"))]);
        let problems = super::super::lifecycle::reopen_problems;
        assert_eq!(problems(Some(&line), &reopened), Vec::<String>::new());
        assert_eq!(problems(Some(&target), &reopened), Vec::<String>::new());
        let reordered = record("todo", &[b(Some("second")), a(Some("first"))]);
        assert!(problems(Some(&line), &reordered)[0].contains("chronological order"));
        assert!(!problems(Some(&target), &reordered).is_empty());
        let dropped = record("todo", &[b(Some("second"))]);
        assert!(!problems(Some(&line), &dropped).is_empty());
        let renamed = record("todo", &[a(Some("other")), b(Some("second"))]);
        assert!(problems(Some(&line), &renamed)[0].contains("chronological order"));
        let skipped = record("todo", &[b(Some("second")), b(Some("third"))]);
        assert!(!problems(Some(&target), &skipped).is_empty());
    }

    #[test]
    fn r16_reviewed_part_ignores_closeout_spacing_but_keeps_content_cr() {
        let before = "---\nstatus: planning\n---\n# Epic\n\nOutcome.\n";
        let done = format!(
            "{}\n## Closeout\n\nEvidence.\n",
            before.replace("status: planning", "status: complete")
        );
        assert_eq!(reviewed_part(before), reviewed_part(&done));
        assert_ne!(
            reviewed_part(before),
            reviewed_part(&before.replace("Outcome.\n", "Outcome.\r\r\n"))
        );
    }

    /// A graph of the given records, each `(kind, id, criteria section)`.
    fn journey_graph(records: &[(RecordKind, &str, &str)]) -> Graph {
        let mut graph = Graph::default();
        for (kind, id, criteria) in records {
            let (folder, epic) = match kind {
                RecordKind::Epic => ("epics", ""),
                _ => ("tasks", "epic_id: EPC-001\n"),
            };
            let path = format!("project-management/{folder}/{id}.md");
            let content = format!(
                "---\nid: {id}\nstatus: todo\n{epic}---\n\n# {id}\n\n## Acceptance Criteria\n\n{criteria}\n## Closeout\n"
            );
            let record = RecordView::parse(*kind, &path, &content).unwrap();
            graph.records.insert((*id).to_string(), record);
        }
        graph
    }

    /// Issue #199 (TSK-382 AC-1): an amendment note under a task or epic
    /// criterion is not criterion text, so a tag closing the head line
    /// still counts.
    #[test]
    fn an_amendment_note_keeps_the_closing_tag_of_task_and_epic_criteria() {
        let own = "- AC-1 On a fresh project, the command shall succeed (journey)\n  - Amended 2026-10-14 (plan version 5): narrowed to Linux.\n";
        let graph = journey_graph(&[(RecordKind::Task, "TSK-001", own)]);
        assert_eq!(journey_requirement(&graph, "TSK-001"), None);

        let epic = "- AC-1 On a fresh project, the release shall install (journey).\n  - Amended 2026-10-12 (plan version 4): reworded.\n  - Moved 2026-10-13 from AC-4.\n";
        for serving in [
            "- AC-1 The command shall run (serves EPC-001 AC-1)\n",
            "- AC-1 The command shall run (serves EPC-001 AC-1)\n  - Moved 2026-10-13 from AC-2.\n",
        ] {
            let graph = journey_graph(&[
                (RecordKind::Epic, "EPC-001", epic),
                (RecordKind::Task, "TSK-001", serving),
            ]);
            assert_eq!(journey_requirement(&graph, "TSK-001"), None, "{serving}");
        }
    }

    /// TSK-382 AC-3: a note that still hides a tag is named in the
    /// refusal, with the remedy.
    #[test]
    fn the_journey_refusal_names_the_note_that_hides_a_tag() {
        let hidden = "- AC-1 On a fresh project, the command shall succeed (journey)\n  - Note: reworded after review.\n";
        let graph = journey_graph(&[(RecordKind::Task, "TSK-001", hidden)]);
        let message = journey_requirement(&graph, "TSK-001").unwrap();
        assert!(
            message.contains(
                "the note \"- Note: reworded after review.\" under AC-1 hides its `(journey)` tag; move the tag to the start of the criterion (R-50)"
            ),
            "{message}"
        );
        assert!(!message.contains("inside its text"), "{message}");

        let in_note = "- AC-1 On a fresh project, the command shall succeed.\n  - Amended 2026-10-14: now a journey (journey)\n";
        let graph = journey_graph(&[(RecordKind::Task, "TSK-001", in_note)]);
        let message = journey_requirement(&graph, "TSK-001").unwrap();
        assert!(
            message.contains(
                "the note \"- Amended 2026-10-14: now a journey (journey)\" under AC-1 hides its `(journey)` tag"
            ),
            "{message}"
        );

        let epic = "- AC-1 On a fresh project, the release shall install (journey)\n  - Note: see the release plan.\n";
        let graph = journey_graph(&[
            (RecordKind::Epic, "EPC-001", epic),
            (
                RecordKind::Task,
                "TSK-001",
                "- AC-1 The command shall run (serves EPC-001 AC-1)\n",
            ),
        ]);
        let message = journey_requirement(&graph, "TSK-001").unwrap();
        assert!(
            message.contains(
                "the note \"- Note: see the release plan.\" under EPC-001 AC-1 hides its `(journey)` tag; move the tag to the start of the criterion (R-50)"
            ),
            "{message}"
        );

        // A tag inside the criterion's own text keeps the R-50 refusal.
        let inner =
            "- AC-1 When run, the (journey) path shall pass.\n  - Amended 2026-10-14: reworded.\n";
        let graph = journey_graph(&[(RecordKind::Task, "TSK-001", inner)]);
        let message = journey_requirement(&graph, "TSK-001").unwrap();
        assert!(
            message.contains("AC-1 carries `(journey)` inside its text"),
            "{message}"
        );
        assert!(!message.contains("the note"), "{message}");

        // Review round 1 (TSK-004 of the verdict): a `(serves ...)` only in
        // a note is named with the same remedy.
        let in_note =
            "- AC-1 The command shall run.\n  - Amended 2026-10-14: now (serves EPC-001 AC-1).\n";
        let graph = journey_graph(&[
            (
                RecordKind::Epic,
                "EPC-001",
                "- AC-1 (journey) On a fresh project, it shall install.\n",
            ),
            (RecordKind::Task, "TSK-001", in_note),
        ]);
        let message = journey_requirement(&graph, "TSK-001").unwrap();
        assert!(
            message.contains(
                "the note \"- Amended 2026-10-14: now (serves EPC-001 AC-1).\" under AC-1 hides its `(serves ...)` tag; move the tag to the start of the criterion (R-50)"
            ),
            "{message}"
        );
    }

    /// Review round 1: a reflow that keeps the text but moves a tag is a
    /// criteria delta, printed with the tags on each side.
    #[test]
    fn a_tag_change_with_the_same_text_is_a_criteria_delta() {
        let one_line =
            items("- AC-1 Prove the metric now (after release) - Amended 2026-10-14: the rest.\n");
        let split = items(
            "- AC-1 Prove the metric now (after release)\n  - Amended 2026-10-14: the rest.\n",
        );
        assert_eq!(
            criteria_delta(&one_line, &split),
            vec!["AC-1 changed its tags with the same text: no tag -> (after release)".to_string()]
        );
        assert!(criteria_delta(&split, &split).is_empty());
    }
    const ADOPTION_TASK: &str = "project-management/tasks/TSK-001.md";
    const ADOPTION_EPIC: &str = "project-management/epics/EPC-001.md";
    const ADOPTION_LINE: &str = "integration/EPC-001-line";

    struct AdoptionFixture {
        dir: tempfile::TempDir,
        base: Oid,
        reviewed: Oid,
        span: Oid,
        merge: Oid,
    }

    impl AdoptionFixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            git(root, &["init", "-q", "-b", "main"]);
            git(root, &["config", "user.email", "t@example.com"]);
            git(root, &["config", "user.name", "t"]);
            let mut f = Self {
                dir,
                base: Oid::ZERO_SHA1,
                reviewed: Oid::ZERO_SHA1,
                span: Oid::ZERO_SHA1,
                merge: Oid::ZERO_SHA1,
            };
            f.write(ADOPTION_TASK, &format!("---\nid: TSK-001\nstatus: todo\nintegration_target: {ADOPTION_LINE}\n---\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n\n## Closeout\n\nPending.\n"));
            f.write(ADOPTION_EPIC, "---\nid: EPC-001\nstatus: planning\n---\n");
            f.base = f.commit();
            git(f.dir.path(), &["branch", ADOPTION_LINE]);
            git(f.dir.path(), &["switch", "-qc", "task/TSK-001-work"]);
            f.write("src/work.rs", "// reviewed work\n");
            f.reviewed = f.commit();
            // A status-only tail on the task branch distinguishes span head
            // from the reviewed source in the public proof.
            let record = f
                .read(ADOPTION_TASK)
                .replace("status: todo", "status: in_progress");
            f.write(ADOPTION_TASK, &record);
            f.span = f.commit();
            git(f.dir.path(), &["switch", "-q", ADOPTION_LINE]);
            git(
                f.dir.path(),
                &[
                    "merge",
                    "-q",
                    "--no-ff",
                    "-m",
                    "land task",
                    "task/TSK-001-work",
                ],
            );
            f.merge = f.head();
            f
        }

        fn read(&self, path: &str) -> String {
            std::fs::read_to_string(self.dir.path().join(path)).unwrap()
        }

        fn write(&self, path: &str, text: &str) {
            let path = self.dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }

        fn head(&self) -> Oid {
            Repository::open(self.dir.path())
                .unwrap()
                .head()
                .unwrap()
                .target()
                .unwrap()
        }

        fn commit(&self) -> Oid {
            git(self.dir.path(), &["add", "-A"]);
            git(self.dir.path(), &["commit", "-qm", "fixture change"]);
            self.head()
        }

        fn adopt(&self, invalid: bool) -> Oid {
            self.write(".gitleaksignore", "# owner's exemption\n");
            let direct = self.commit();
            git(self.dir.path(), &["switch", "-qc", "plan/adopt"]);
            let entry = if invalid {
                "not-a-commit".to_string()
            } else {
                direct.to_string()
            };
            self.write(ADOPTION_EPIC, &format!("---\nid: EPC-001\nstatus: planning\nline_adoptions:\n  - commit: {entry}\n    reason: Owner exemption\n    review: https://example.test/review\n---\n"));
            self.commit();
            git(self.dir.path(), &["switch", "-q", ADOPTION_LINE]);
            git(
                self.dir.path(),
                &[
                    "merge",
                    "-q",
                    "--no-ff",
                    "-m",
                    "land adoption",
                    "plan/adopt",
                ],
            );
            direct
        }

        fn complete(&self) -> Oid {
            let record = self.read(ADOPTION_TASK).replace("status: in_progress", "status: complete").replace("Pending.", &format!("```yaml\nacceptance:\n  reviewed: {}\n  review: https://example.test/review\n  criteria:\n    AC-1: verified | test passed\n  journey: verified | fixture ran\n  not_verified: none\n  follow_ups: none\n  verdict: approved\n```", self.reviewed));
            self.write(ADOPTION_TASK, &record);
            self.commit()
        }

        fn bind(&self) -> BindingResult {
            let repo = Repository::open(self.dir.path()).unwrap();
            let task =
                RecordView::parse(RecordKind::Task, ADOPTION_TASK, &self.read(ADOPTION_TASK))
                    .unwrap();
            // The run pins the line the completion is made on (TSK-340:
            // only a pinned or verified line receives a landing).
            let run = RunBases::new([self.base, self.merge]);
            bind_completion_with_amendment(
                &repo,
                &task,
                &Graph::default(),
                Landing::Commit(self.head()),
                Some(self.base),
                Transport::TaskLanding,
                None, // This fixture completes on its checked-out branch, without a PR range.
                None,
                &run.stacking(),
                &run.criteria(),
            )
        }
    }

    #[test]
    fn binding_proof_names_the_review_landing_span_and_completion_tail() {
        let f = AdoptionFixture::new();
        f.adopt(false);
        let tail = f.complete();
        let bound = f.bind();
        assert!(bound.findings.is_empty(), "{:?}", bound.findings);
        assert_eq!(
            bound.proof,
            Some(CompletionProof {
                reviewed: f.reviewed,
                landing_merge: Some(f.merge),
                span_head: f.span,
                completion_tail: tail
            })
        );
    }

    #[test]
    fn direct_binding_proof_has_no_landing_merge() {
        let f = AdoptionFixture::new();
        git(f.dir.path(), &["switch", "-q", "task/TSK-001-work"]);
        let tail = f.complete();
        let bound = f.bind();
        assert!(bound.findings.is_empty(), "{:?}", bound.findings);
        assert_eq!(
            bound.proof,
            Some(CompletionProof {
                reviewed: f.reviewed,
                landing_merge: None,
                span_head: tail,
                completion_tail: tail
            })
        );
    }

    #[test]
    fn binding_propagates_an_unreadable_adoption_entry() {
        let f = AdoptionFixture::new();
        f.adopt(true);
        f.complete();
        let bound = f.bind();
        assert!(bound.proof.is_none());
        assert!(
            bound.findings.iter().any(|finding| finding
                .message
                .contains("line_adoptions[0].commit must be a full 40-hex commit id")),
            "{:?}",
            bound.findings
        );
    }

    #[test]
    fn binding_refuses_an_unreadable_entry_in_adoption_history() {
        let f = AdoptionFixture::new();
        let direct = f.adopt(true);
        git(f.dir.path(), &["switch", "-qc", "plan/repair"]);
        f.write(
            ADOPTION_EPIC,
            &f.read(ADOPTION_EPIC)
                .replace("not-a-commit", &direct.to_string()),
        );
        f.commit();
        git(f.dir.path(), &["switch", "-q", ADOPTION_LINE]);
        git(
            f.dir.path(),
            &[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                "repair adoption",
                "plan/repair",
            ],
        );
        f.complete();
        let bound = f.bind();
        assert!(bound.proof.is_none());
        assert!(
            bound.findings.iter().any(|finding| finding
                .message
                .contains("line_adoptions[0].commit must be a full 40-hex commit id")),
            "{:?}",
            bound.findings
        );
    }

    #[test]
    fn binding_refuses_an_adoption_over_overlaid_history() {
        for replace in [false, true] {
            let f = AdoptionFixture::new();
            f.adopt(false);
            let tail = f.complete();
            let repo = Repository::open(f.dir.path()).unwrap();
            if replace {
                repo.reference(
                    &format!("refs/replace/{tail}"),
                    f.base,
                    false,
                    "fixture overlay",
                )
                .unwrap();
            } else {
                std::fs::create_dir_all(repo.path().join("info")).unwrap();
                std::fs::write(
                    repo.path().join("info/grafts"),
                    format!("{tail} {}\n", f.base),
                )
                .unwrap();
            }
            let bound = f.bind();
            assert!(bound.proof.is_none());
            assert!(
                bound
                    .findings
                    .iter()
                    .any(|finding| finding.message.contains("cannot verify")),
                "{:?}",
                bound.findings
            );
            // Exercise the exact guarded reader the adoption consumer uses.
            let task =
                RecordView::parse(RecordKind::Task, ADOPTION_TASK, &f.read(ADOPTION_TASK)).unwrap();
            assert!(landed_adoptions(&repo, &task, f.merge, tail, Some(f.base))
                .unwrap_err()
                .contains("cannot verify"));
        }
    }
}
