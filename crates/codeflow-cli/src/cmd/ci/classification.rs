//! Pull request classification (TSK-104, SPC-013 R-64, R-70 to R-72, R-78).
//!
//! A tracked range names its task, a planning range names its epic, and an
//! integration line names the epic verified by its history. Every PR names
//! exactly one unit, including projects without durable tracking. The one
//! exception is the workspace root branch the target's `git.root_branch`
//! names (ADR-0074): it collects small workspace edits and carries no
//! `Task:` line.

use std::path::Path;

use codeflow_core::hooks::{GitPolicy, PolicyLevel, Violation};
use codeflow_core::workgraph::acceptance::{journey_requirement_at, JOURNEY_RULE};
use codeflow_core::workgraph::classify::{
    is_planning_path, is_spike_path, path_sets, ProjectPaths,
};
use codeflow_core::workgraph::{
    check_epic_line, declared_work_target, declared_work_target_at_revision,
    durable_work_tracking_enabled, durable_work_tracking_enabled_at, resolve_work_target_checked,
    task_id_from_branch,
};

/// The value of one `Task:` line in a pull request body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TaskLine {
    /// `Task: TSK-NNN`.
    Tracked(String),
    /// An epic for a planning change or a verified integration line.
    Epic(String),
    /// A named unit where durable tracking is inactive.
    Unit(String),
    /// Anything else after `Task:`.
    Malformed(String),
}

/// Every `Task:` line of `body` outside HTML comments and fenced code. A
/// leading list marker (`- `) is allowed.
pub(super) fn task_lines(body: &str) -> Vec<TaskLine> {
    let visible = super::strip_html_comments(body, true);
    let mut in_fence = false;
    let mut lines = Vec::new();
    for raw in visible.lines() {
        let line = raw.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let line = line.strip_prefix("- ").unwrap_or(line);
        let Some(value) = line.strip_prefix("Task:") else {
            continue;
        };
        let value = value.trim();
        let value = value
            .strip_prefix('`')
            .and_then(|value| value.strip_suffix('`'))
            .unwrap_or(value)
            .trim();
        lines.push(
            if value.is_empty()
                || value.eq_ignore_ascii_case("none")
                || value.to_ascii_lowercase().starts_with("none:")
                || value.contains(['|', '<', '>', '`'])
                || value == "TSK-NNN"
                || value == "EPC-NNN"
            {
                TaskLine::Malformed(value.to_string())
            } else if codeflow_core::workgraph::is_valid_task_format_id(value) {
                TaskLine::Tracked(value.to_string())
            } else if codeflow_core::workgraph::is_valid_epic_format_id(value) {
                TaskLine::Epic(value.to_string())
            } else if value.starts_with("TSK-") || value.starts_with("EPC-") {
                TaskLine::Malformed(value.to_string())
            } else {
                TaskLine::Unit(value.to_string())
            },
        );
    }
    lines
}

/// The class a pull request resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Class {
    /// Tracked work for the task named in the body.
    Tracked { task_id: String },
    /// The range touches only records and plans.
    PlanningOnly,
    /// An epic's integration line landing on its target; each task on it was
    /// classified when it landed on the line.
    EpicLine(String),
    /// The workspace root branch the target's `git.root_branch` names
    /// (ADR-0074); small edits land on it directly, and each nested project
    /// lands through its own repository.
    RootBranch(String),
}

/// What the classifier reads.
pub(super) struct Input<'a> {
    pub body: &'a str,
    pub branch: &'a str,
    /// Every path the range touches.
    pub files: &'a [String],
    /// The task id the branch carries on a work prefix, if any.
    pub branch_task: Option<String>,
    /// For an `integration/` head: the epic it lands, or why it is not a
    /// verified epic line.
    pub epic_line: Option<Result<String, String>>,
    /// Whether the head is the branch the target's `git.root_branch` names.
    pub root_branch: bool,
}

/// Resolve the class, or the reason the pull request has none.
pub(super) fn classify(input: &Input<'_>) -> Result<Class, String> {
    let lines = task_lines(input.body);
    if lines.len() > 1 {
        return Err(format!(
            "the body has {} `Task:` lines; a pull request has exactly one class",
            lines.len()
        ));
    }
    match lines.into_iter().next() {
        Some(TaskLine::Tracked(task_id)) => {
            if let Some(carried) = input.branch_task.as_deref() {
                if carried != task_id {
                    return Err(format!(
                        "`Task: {task_id}` names another task than branch '{}' ({carried})",
                        input.branch
                    ));
                }
            }
            Ok(Class::Tracked { task_id })
        }
        Some(TaskLine::Epic(epic)) => {
            if input.branch_task.is_some() {
                return Err("a task branch must name its own task".into());
            }
            if input.branch.starts_with("integration/") {
                return match &input.epic_line {
                    Some(Ok(actual)) if actual == &epic => Ok(Class::EpicLine(epic)),
                    Some(Ok(actual)) => {
                        Err(format!("Task: {epic} does not match the line's {actual}"))
                    }
                    Some(Err(reason)) => Err(reason.clone()),
                    None => Err(format!("'{}' is not a verified epic line", input.branch)),
                };
            }
            if let Some(path) = input.files.iter().find(|path| !is_planning_path(path)) {
                return Err(format!(
                    "a planning-only pull request touches a product path: {path}"
                ));
            }
            Ok(Class::PlanningOnly)
        }
        Some(TaskLine::Malformed(value) | TaskLine::Unit(value)) => Err(format!(
            "`Task: {value}` is neither `TSK-NNN` nor `EPC-NNN`"
        )),
        None if input.root_branch => Ok(Class::RootBranch(input.branch.to_string())),
        None => Err(
            "no `Task:` line; name the task, or the epic for a planning change or integration line"
                .into(),
        ),
    }
}

/// The range a pull request is judged on: the base as named and as a
/// commit, and the head.
pub(super) struct Range<'a> {
    pub base: &'a str,
    pub head: &'a str,
    /// The branch the range lands on: `--into`, the host's pull request
    /// target, a named base, else the default work target; never the
    /// commit that bounds the range (a SHA base names no branch).
    pub target: &'a str,
}

/// The branch `git.root_branch` names in the policy at `base`. It is read at
/// the target, so a pull request cannot name its own head as the root.
pub(super) fn root_branch_at(root: &Path, base: &str) -> Option<String> {
    let out = codeflow_core::git::command()
        .arg("-C")
        .arg(root)
        .args(["show", &format!("{base}:.codeflow/policy.json")])
        .output()
        .ok()
        .filter(|out| out.status.success())?;
    let policy: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let name = policy["git"]["root_branch"].as_str()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Whether durable work tracking is on at the head or at the target, so a
/// pull request cannot switch it off for itself.
pub(super) fn tracking_on(root: &Path, range: Option<&Range<'_>>) -> Result<bool, String> {
    let at_target = range.map(|range| durable_work_tracking_enabled_at(root, range.base));
    match (durable_work_tracking_enabled(root), at_target) {
        (Err(error), _) => Err(error.to_string()),
        (_, Some(Err(error))) => Err(error),
        (Ok(head), target) => Ok(head || matches!(target, Some(Ok(true)))),
    }
}

/// Run the classification for `codeflow ci`. Classification judges the
/// whole range against the target's own state: tracking is on when it is on
/// at the target or at the head, and the paths are the range's diff from
/// the merge-base, merge resolutions included. Returns the validated class,
/// or `None` when the pull request was not classified.
pub(super) fn dispatch(
    root: &Path,
    git: &GitPolicy,
    body: &str,
    branch: &str,
    range: Option<&Range<'_>>,
    tagged: &mut Vec<super::TaggedViolation>,
    ran: &mut Vec<&str>,
) -> Option<Class> {
    match tracking_on(root, range) {
        Ok(true) => {}
        Ok(false) => {
            ran.push("classification");
            let lines = task_lines(body);
            let valid = match lines.as_slice() {
                [TaskLine::Tracked(id)] => {
                    task_id_from_branch(root, branch).is_none_or(|carried| carried == *id)
                }
                [TaskLine::Epic(_) | TaskLine::Unit(_)] => {
                    task_id_from_branch(root, branch).is_none()
                }
                [] => range.is_some_and(|range| {
                    root_branch_at(root, range.base).as_deref() == Some(branch)
                }),
                _ => false,
            };
            if !valid {
                push(tagged, RULE, "the PR must have exactly one non-empty, non-placeholder Task: unit name matching its branch".into(), HINT);
            }
            return None;
        }
        Err(error) => {
            tagged.push(super::TaggedViolation {
                sha: None,
                violation: super::tracking_state_violation(error),
            });
            ran.push("classification");
            return None;
        }
    }
    ran.push("classification");
    let Some(range) = range else {
        push(
            tagged,
            RULE,
            "the range could not be read, so the pull request cannot be classified".to_string(),
            "pass --base and --head so CI can read the range",
        );
        return None;
    };
    let changes = match range_changes(root, range.base, range.head) {
        Ok(changes) => changes,
        Err(error) => {
            push(
                tagged,
                RULE,
                format!("cannot list the paths the range changes: {error}"),
                "pass --base and --head so CI can read the range",
            );
            return None;
        }
    };
    let files: Vec<String> = changes.iter().map(|(_, path)| path.clone()).collect();
    selection_check(root, branch, range, &files, tagged);
    let input = Input {
        body,
        branch,
        files: &files,
        branch_task: task_id_from_branch(root, branch),
        epic_line: branch
            .starts_with("integration/")
            .then(|| check_epic_line(root, branch, range.target, range.base, range.head)),
        root_branch: root_branch_at(root, range.base).as_deref() == Some(branch),
    };
    let class = match classify(&input) {
        Ok(class) => class,
        Err(reason) => {
            push(
                tagged,
                RULE,
                format!("unclassified pull request: {reason}"),
                HINT,
            );
            return None;
        }
    };
    match &class {
        Class::Tracked { task_id } => {
            println!("codeflow ci: pull request class: tracked {task_id} (from the Task: line)");
            let own_branch = input.branch_task.as_deref() == Some(task_id.as_str());
            let added: Vec<&str> = changes
                .iter()
                .filter(|(status, _)| status == "A")
                .map(|(_, path)| path.as_str())
                .collect();
            let level = git.work_planning_level();
            let anchor = Anchor {
                own_branch,
                level,
                branch,
                head: range.head,
            };
            tracked(root, task_id, anchor, &files, &added, tagged);
            journey(root, git, task_id, range.head, &files, tagged);
        }
        Class::PlanningOnly => println!("codeflow ci: pull request class: planning-only"),
        Class::EpicLine(epic) => {
            println!("codeflow ci: pull request class: epic integration line of {epic}");
        }
        Class::RootBranch(name) => {
            println!("codeflow ci: pull request class: workspace root branch {name}");
        }
    }
    Some(class)
}

/// A selection lands only by a planning pull request (SPC-013 R-43): a range
/// that removes a task's `awaiting_selection` off a `plan/` branch blocks.
fn selection_check(
    root: &Path,
    branch: &str,
    range: &Range<'_>,
    files: &[String],
    tagged: &mut Vec<super::TaggedViolation>,
) {
    let touches_records = files
        .iter()
        .any(|file| file.starts_with("project-management/"));
    if touches_records && !branch.starts_with("plan/") {
        match codeflow_core::workgraph::readiness::selections_in_range(root, range.base, range.head)
        {
            Ok(selected) => {
                for task_id in selected {
                    push(
                        tagged,
                        "work.selection",
                        format!("{task_id}'s awaiting_selection is removed on '{branch}'"),
                        "a selection lands only by a planning pull request from a plan/ branch (SPC-013 R-43)",
                    );
                }
            }
            Err(error) => push(
                tagged,
                "work.selection",
                format!("cannot read the records of the range: {error}"),
                "pass --base and --head so CI can read the range",
            ),
        }
    }
}

const RULE: &str = "work.classification";
const HINT: &str = "add exactly one Task: TSK-NNN, or Task: EPC-NNN for the epic's planning change or integration line; where durable tracking is inactive, name the tracked unit";

fn push(tagged: &mut Vec<super::TaggedViolation>, rule: &str, message: String, hint: &str) {
    tagged.push(super::TaggedViolation {
        sha: None,
        violation: Violation::always_blocking(rule, message, hint),
    });
}

/// Tracked work: the record cannot authorise itself (R-78), the anchored
/// preflight holds for the named task (R-72), and a spike lands only its
/// findings (R-64). When the branch carries the same task, `codeflow ci`'s
/// own-branch preflight already reports an anchor failure (with the visible
/// workgraph check), so it is not reported twice here.
/// How a tracked task's anchor failure is reported: not at all on its own
/// branch (the own-branch preflight reports it), else at the
/// `git.work_planning` level (TSK-133).
#[derive(Clone, Copy)]
struct Anchor<'a> {
    own_branch: bool,
    level: PolicyLevel,
    /// The branch under judgement and the head its records are read at:
    /// CI judges a pull request head from a base checkout.
    branch: &'a str,
    head: &'a str,
}

fn anchor_failure(
    tagged: &mut Vec<super::TaggedViolation>,
    level: PolicyLevel,
    message: String,
    hint: codeflow_core::remedy::Remedy,
) {
    tagged.push(super::TaggedViolation {
        sha: None,
        violation: Violation::new("work.stable_planning_anchor", level, message, hint),
    });
}

fn tracked(
    root: &Path,
    task_id: &str,
    anchor: Anchor<'_>,
    files: &[String],
    added: &[&str],
    tagged: &mut Vec<super::TaggedViolation>,
) {
    let Anchor { branch, head, .. } = anchor;
    let added_records: Vec<_> = added
        .iter()
        .filter(|path| {
            path.starts_with("project-management/")
                && Path::new(path)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                && !path.starts_with("project-management/templates/")
        })
        .collect();
    if added_records
        .iter()
        .any(|path| !is_record_of(path, task_id))
        || (added_records.len() > 1)
    {
        push(
            tagged,
            RULE,
            "a task PR may add only its own standalone task record".into(),
            "put the other records in the epic amendment",
        );
    }
    // The record at the head first: a standalone record is on its branch,
    // not in a base checkout.
    let declared = declared_work_target_at_revision(root, branch, head)
        .ok()
        .flatten()
        .or_else(|| declared_work_target(root, task_id));
    // The own-branch preflight prints any resolution note and reports a
    // diverged target; this check reports it only for another task's claim.
    let target = match resolve_work_target_checked(root, declared.as_deref()) {
        Ok(resolved) => resolved.map_or_else(|| "main".to_string(), |r| r.target),
        Err(_) if anchor.own_branch => return,
        Err(error) => {
            anchor_failure(
                tagged,
                anchor.level,
                error.to_string(),
                codeflow_core::remedy::WORK_START_RECONCILE.with(&[("id", task_id)]),
            );
            return;
        }
    };
    match codeflow_core::workgraph::work_start::check_work_admission(
        root, task_id, &target, branch, head,
    ) {
        Ok(report) => {
            let spike =
                report.work_type.as_deref() == Some("spike") || branch.starts_with("spike/");
            if spike {
                if let Some(path) = files.iter().find(|path| !is_spike_path(path, task_id)) {
                    push(
                        tagged,
                        RULE,
                        format!("spike {task_id} changes {path}; a spike lands only findings under docs/research/ and its own record"),
                        "move the product change to a task of its own",
                    );
                }
            }
        }
        Err(_) if anchor.own_branch => {}
        Err(error) => anchor_failure(
            tagged,
            anchor.level,
            error.to_string(),
            codeflow_core::remedy::WORK_START_MERGE_PLANNING
                .with(&[("target", &target), ("id", task_id)]),
        ),
    }
}

/// A range touching the adopter-facing path set belongs to a task with a
/// journey criterion or one serving its epic's journey (R-53), at the
/// `git.work_records` level. The task is read at the head.
fn journey(
    root: &Path,
    git: &GitPolicy,
    task_id: &str,
    head: &str,
    files: &[String],
    tagged: &mut Vec<super::TaggedViolation>,
) {
    let project = ProjectPaths::load(root);
    let sets = path_sets();
    let Some((path, member)) = files.iter().find_map(|path| {
        sets.adopter_facing_member(path, &project)
            .map(|member| (path, member))
    }) else {
        return;
    };
    let message = match journey_requirement_at(root, head, task_id) {
        Ok(problem) => problem.map(|problem| format!("{problem} ({path} is {member})")),
        Err(error) => Some(format!("cannot read the task at the head: {error}")),
    };
    if let Some(message) = message {
        tagged.push(super::TaggedViolation {
            sha: None,
            violation: Violation::new(
                JOURNEY_RULE,
                git.work_records_level(),
                message,
                codeflow_core::remedy::JOURNEY_CRITERION
                    .with(&[("path", &format!("project-management/tasks/{task_id}.md"))]),
            ),
        });
    }
}

fn is_record_of(path: &str, task_id: &str) -> bool {
    path.starts_with("project-management/")
        && path
            .rsplit('/')
            .next()
            .is_some_and(|file| file == format!("{task_id}.md"))
}

/// Every path the range changes, from the merge-base of `base` and `head`
/// to `head`, with its status: one tree diff, so a change made while
/// resolving a merge counts, a rename is both its sides, and paths are read
/// NUL-delimited without display quoting. A failure is an error, never an
/// empty range.
pub(super) fn range_changes(
    root: &Path,
    base: &str,
    head: &str,
) -> Result<Vec<(String, String)>, String> {
    let out = codeflow_core::git::command()
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.quotePath=false",
            "diff",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--name-status",
            &format!("{base}...{head}"),
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    parse_name_status(&out.stdout)
}

fn parse_name_status(stdout: &[u8]) -> Result<Vec<(String, String)>, String> {
    let mut fields = stdout
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    let mut changes = Vec::new();
    while let Some(status) = fields.next() {
        let status = String::from_utf8_lossy(status).to_string();
        let path = fields
            .next()
            .ok_or_else(|| format!("git diff output ends after status {status}"))?;
        changes.push((status, String::from_utf8_lossy(path).to_string()));
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(body: &'a str, branch: &'a str, files: &'a [String]) -> Input<'a> {
        Input {
            body,
            branch,
            files,
            branch_task: None,
            epic_line: None,
            root_branch: false,
        }
    }

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn task_grammar_requires_one_named_matching_class() {
        let code = paths(&["src/lib.rs"]);
        let records = paths(&["docs/plan/next.md"]);
        assert_eq!(
            task_lines("<!-- Task: TSK-001 -->\n```\nTask: TSK-002\n```\n- Task: `TSK-003`"),
            [TaskLine::Tracked("TSK-003".into())]
        );
        assert_eq!(
            classify(&input("Task: EPC-001", "plan/next", &records)),
            Ok(Class::PlanningOnly)
        );
        assert!(classify(&input("Task: EPC-001", "plan/next", &code)).is_err());
        for body in [
            "",
            "Task:",
            "Task: none: typo",
            "Task: TSK-001\nTask: TSK-002",
            "Task: `TSK-NNN | EPC-NNN | <unit name>`",
        ] {
            assert!(
                classify(&input(body, "fix/change", &records)).is_err(),
                "{body}"
            );
        }
        let mut mismatch = input("Task: TSK-002", "task/TSK-001-work", &code);
        mismatch.branch_task = Some("TSK-001".into());
        assert!(classify(&mismatch).is_err());
        let mut line = input("Task: EPC-001", "integration/EPC-001-work", &code);
        line.epic_line = Some(Ok("EPC-001".into()));
        assert_eq!(classify(&line), Ok(Class::EpicLine("EPC-001".into())));
        let mut root = input("", "integration/workspace", &code);
        root.root_branch = true;
        assert_eq!(
            classify(&root),
            Ok(Class::RootBranch("integration/workspace".into()))
        );
    }

    #[test]
    fn only_the_named_root_branch_goes_without_a_task_line() {
        let code = paths(&["src/lib.rs"]);
        // Another integration/ head, or the root's name when the target's
        // policy does not name it, still needs its unit.
        assert!(classify(&input("", "integration/workspace", &code))
            .unwrap_err()
            .contains("no `Task:` line"));
        // The root branch is no epic line, so an epic name does not fit it.
        let mut named = input("Task: EPC-001", "integration/workspace", &code);
        named.root_branch = true;
        assert!(classify(&named)
            .unwrap_err()
            .contains("not a verified epic line"));
    }

    #[test]
    fn range_paths_are_read_nul_delimited_without_quoting() {
        let out = b"M\0src/\xcf\x80.rs\0A\0.claude/a\tb.md\0D\0src/old.rs\0A\0lib/new.rs\0";
        assert_eq!(
            parse_name_status(out).unwrap(),
            [
                ("M".to_string(), "src/\u{3c0}.rs".to_string()),
                ("A".to_string(), ".claude/a\tb.md".to_string()),
                ("D".to_string(), "src/old.rs".to_string()),
                ("A".to_string(), "lib/new.rs".to_string()),
            ]
        );
        assert!(parse_name_status(b"M\0").is_err());
    }
}
