//! Pull request classification (TSK-104, SPC-013 R-64, R-70 to R-72, R-78).
//!
//! Every product pull request into a project with durable work tracking has
//! exactly one class: tracked (`Task: TSK-NNN`, or the task id its branch
//! carries), a direct change (`Task: none: <reason>`), planning-only (records
//! and plans only), an epic's integration line landing on its target, or a
//! trusted automation profile (the slot TSK-107 fills). An unclassified pull
//! request blocks. The paths come from one table plus the project's own
//! policy, so the same rule holds in every project that installs it.

use std::path::Path;
use std::process::Command;

use codeflow_core::hooks::{GitPolicy, PolicyLevel, Violation};
use codeflow_core::workgraph::classify::{
    is_planning_path, is_spike_path, path_sets, ProjectPaths,
};
use codeflow_core::workgraph::{
    check_epic_line, check_work_start_anchored, declared_work_target,
    durable_work_tracking_enabled, durable_work_tracking_enabled_at, resolve_work_target_checked,
    task_id_from_branch,
};

/// The value of one `Task:` line in a pull request body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TaskLine {
    /// `Task: TSK-NNN`.
    Tracked(String),
    /// `Task: none: <reason>`.
    Direct(String),
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
        let value = value.trim().trim_matches('`').trim();
        lines.push(if let Some(reason) = value.strip_prefix("none:") {
            let reason = reason.trim();
            if reason.is_empty() {
                TaskLine::Malformed(value.to_string())
            } else {
                TaskLine::Direct(reason.to_string())
            }
        } else if codeflow_core::workgraph::is_valid_task_format_id(value) {
            TaskLine::Tracked(value.to_string())
        } else {
            TaskLine::Malformed(value.to_string())
        });
    }
    lines
}

/// The class a pull request resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Class {
    /// Tracked work for one task; `derived` when the id came from the branch.
    Tracked { task_id: String, derived: bool },
    /// A direct change with its stated reason.
    Direct { reason: String },
    /// The range touches only records and plans.
    PlanningOnly,
    /// An epic's integration line landing on its target; each task on it was
    /// classified when it landed on the line.
    EpicLine(String),
    /// A trusted automation profile (TSK-107 supplies the match).
    Automation { profile: String },
}

/// What the classifier reads.
pub(super) struct Input<'a> {
    pub body: &'a str,
    pub branch: &'a str,
    /// Every path the range touches.
    pub files: &'a [String],
    /// The task id the branch carries on a work prefix, if any.
    pub branch_task: Option<String>,
    /// The automation profile the workflow's actor and branch matched, if any.
    pub automation: Option<&'a str>,
    /// For an `integration/` head: the epic it lands, or why it is not a
    /// verified epic line.
    pub epic_line: Option<Result<String, String>>,
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
            Ok(Class::Tracked {
                task_id,
                derived: false,
            })
        }
        Some(TaskLine::Direct(reason)) => {
            if let Some(carried) = input.branch_task.as_deref() {
                return Err(format!(
                    "`Task: none` on branch '{}', which carries {carried}; the branch's task is the class",
                    input.branch
                ));
            }
            if input.branch.starts_with("spike/") {
                return Err(format!(
                    "`Task: none` on spike branch '{}'; a spike is tracked work, so name its task",
                    input.branch
                ));
            }
            Ok(Class::Direct { reason })
        }
        Some(TaskLine::Malformed(value)) => Err(format!(
            "`Task: {value}` is neither `TSK-NNN` nor `none: <reason>`"
        )),
        None => {
            if let Some(profile) = input.automation {
                return Ok(Class::Automation {
                    profile: profile.to_string(),
                });
            }
            if input.branch.starts_with("integration/") {
                return match &input.epic_line {
                    Some(Ok(epic)) => Ok(Class::EpicLine(epic.clone())),
                    Some(Err(reason)) => Err(reason.clone()),
                    None => Err(format!("'{}' is not a verified epic line", input.branch)),
                };
            }
            if let Some(task_id) = input.branch_task.clone() {
                return Ok(Class::Tracked {
                    task_id,
                    derived: true,
                });
            }
            if let Some(product) = input.files.iter().find(|path| !is_planning_path(path)) {
                return Err(if input.branch.starts_with("plan/") {
                    format!("a planning-only pull request touches a product path: {product}")
                } else {
                    format!(
                        "no `Task:` line and the range is not planning-only (it touches {product})"
                    )
                });
            }
            Ok(Class::PlanningOnly)
        }
    }
}

/// The range a pull request is judged on: the base as named and as a
/// commit, and the head.
pub(super) struct Range<'a> {
    pub base_ref: &'a str,
    pub base: &'a str,
    pub head: &'a str,
}

/// Run the classification for `codeflow ci`. Classification judges the
/// whole range against the target's own state: tracking is on when it is on
/// at the target or at the head, and the paths are the range's diff from
/// the merge-base, merge resolutions included.
pub(super) fn dispatch(
    root: &Path,
    git: &GitPolicy,
    body: &str,
    branch: &str,
    range: Option<&Range<'_>>,
    tagged: &mut Vec<super::TaggedViolation>,
    ran: &mut Vec<&str>,
) {
    let at_target = range.map(|range| durable_work_tracking_enabled_at(root, range.base));
    let enabled = match (durable_work_tracking_enabled(root), at_target) {
        (Err(error), _) => Err(error.to_string()),
        (_, Some(Err(error))) => Err(error),
        (Ok(head), target) => Ok(head || matches!(target, Some(Ok(true)))),
    };
    match enabled {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            push(
                tagged,
                "work.tracking_state",
                format!("cannot determine durable-work tracking: {error}"),
                "repair CodeFlow state or task-home access before classifying work",
            );
            ran.push("classification");
            return;
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
        return;
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
            return;
        }
    };
    let files: Vec<String> = changes.iter().map(|(_, path)| path.clone()).collect();
    let input = Input {
        body,
        branch,
        files: &files,
        branch_task: task_id_from_branch(root, branch),
        automation: None,
        epic_line: branch
            .starts_with("integration/")
            .then(|| check_epic_line(root, branch, range.base_ref, range.base, range.head)),
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
            return;
        }
    };
    match &class {
        Class::Tracked { task_id, derived } => {
            println!(
                "codeflow ci: pull request class: tracked {task_id} ({})",
                if *derived {
                    "from the branch"
                } else {
                    "from the Task: line"
                }
            );
            let own_branch = input.branch_task.as_deref() == Some(task_id.as_str());
            let added: Vec<&str> = changes
                .iter()
                .filter(|(status, _)| status == "A")
                .map(|(_, path)| path.as_str())
                .collect();
            tracked(root, task_id, own_branch, branch, &files, &added, tagged);
        }
        Class::Direct { reason } => {
            println!("codeflow ci: pull request class: direct change ({reason})");
            direct(root, git, &files, tagged);
        }
        Class::PlanningOnly => println!("codeflow ci: pull request class: planning-only"),
        Class::EpicLine(epic) => {
            println!("codeflow ci: pull request class: epic integration line of {epic}");
        }
        Class::Automation { profile } => {
            println!("codeflow ci: pull request class: automation profile {profile}");
        }
    }
}

const RULE: &str = "work.classification";
const HINT: &str = "add one line `Task: TSK-NNN` for tracked work or `Task: none: <reason>` for a \
     direct change; a pull request touching only project-management/ records and docs/plan/ is \
     planning-only";

fn push(tagged: &mut Vec<super::TaggedViolation>, rule: &str, message: String, hint: &str) {
    tagged.push(super::TaggedViolation {
        sha: None,
        violation: Violation::new(rule, PolicyLevel::Block, message, hint.to_string()),
    });
}

/// Tracked work: the record cannot authorise itself (R-78), the anchored
/// preflight holds for the named task (R-72), and a spike lands only its
/// findings (R-64). When the branch carries the same task, `codeflow ci`'s
/// own-branch preflight already reports an anchor failure (with the visible
/// workgraph check), so it is not reported twice here.
fn tracked(
    root: &Path,
    task_id: &str,
    own_branch: bool,
    branch: &str,
    files: &[String],
    added: &[&str],
    tagged: &mut Vec<super::TaggedViolation>,
) {
    if let Some(record) = added.iter().find(|path| is_record_of(path, task_id)) {
        push(
            tagged,
            RULE,
            format!("the pull request adds {record} and claims `Task: {task_id}`; a record cannot authorise itself"),
            "land the task record by its own planning pull request, then open the work pull request",
        );
    }
    let declared = declared_work_target(root, task_id);
    // The own-branch preflight prints any resolution note and reports a
    // diverged target; this check reports it only for another task's claim.
    let target = match resolve_work_target_checked(root, declared.as_deref()) {
        Ok(resolved) => resolved.map_or_else(|| "main".to_string(), |r| r.target),
        Err(_) if own_branch => return,
        Err(error) => {
            push(
                tagged,
                "work.stable_planning_anchor",
                error.to_string(),
                &format!("reconcile the target branch, then run `codeflow work start {task_id}`"),
            );
            return;
        }
    };
    match check_work_start_anchored(root, task_id, &target) {
        Ok(report) => {
            let spike = report.work_type.as_deref() == Some("spike") || branch.starts_with("spike/");
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
        Err(_) if own_branch => {}
        Err(error) => push(
            tagged,
            "work.stable_planning_anchor",
            error.to_string(),
            &format!(
                "merge the validated planning record into '{target}', then run `codeflow work start {task_id}`"
            ),
        ),
    }
}

/// A direct change is refused on the floor of R-71; a project may forbid
/// direct changes entirely.
fn direct(
    root: &Path,
    git: &GitPolicy,
    files: &[String],
    tagged: &mut Vec<super::TaggedViolation>,
) {
    if git.direct_changes == "forbid" {
        push(
            tagged,
            RULE,
            "this project forbids direct changes (git.direct_changes = forbid)".to_string(),
            "name the task with `Task: TSK-NNN`",
        );
        return;
    }
    let project = ProjectPaths::load(root);
    let sets = path_sets();
    let refused = files
        .iter()
        .filter_map(|path| {
            sets.direct_change_refusal(path, &project)
                .map(|member| format!("{path} ({member})"))
        })
        .collect::<Vec<_>>();
    if !refused.is_empty() {
        let shown = refused
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let more = refused.len().saturating_sub(5);
        push(
            tagged,
            RULE,
            format!(
                "a direct change touches protected surfaces: {shown}{}",
                if more > 0 {
                    format!(" and {more} more")
                } else {
                    String::new()
                }
            ),
            "track the work with `Task: TSK-NNN`; product code, managed instructions, policy, hooks, CI files, manifests and the record schema are never direct changes",
        );
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
fn range_changes(root: &Path, base: &str, head: &str) -> Result<Vec<(String, String)>, String> {
    let out = Command::new("git")
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
            automation: None,
            epic_line: None,
        }
    }

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn task_lines_skip_comments_and_fences_and_read_every_form() {
        let body = "<!-- Task: TSK-001 -->\n```\nTask: TSK-002\n```\n- Task: `TSK-003`\n";
        assert_eq!(task_lines(body), [TaskLine::Tracked("TSK-003".into())]);
        assert_eq!(
            task_lines("Task: none: typo in the guide"),
            [TaskLine::Direct("typo in the guide".into())]
        );
        assert_eq!(
            task_lines("Task: none:"),
            [TaskLine::Malformed("none:".into())]
        );
        assert_eq!(
            task_lines("Task: TSK-NNN | none: <reason>"),
            [TaskLine::Malformed("TSK-NNN | none: <reason>".into())]
        );
    }

    #[test]
    fn each_class_resolves() {
        let code = paths(&["src/lib.rs"]);
        let records = paths(&["project-management/tasks/TSK-009.md", "docs/plan/v3.md"]);
        assert_eq!(
            classify(&input("Task: TSK-004", "fix/typo", &code)),
            Ok(Class::Tracked {
                task_id: "TSK-004".into(),
                derived: false
            })
        );
        assert_eq!(
            classify(&input("Task: none: typo", "fix/typo", &code)),
            Ok(Class::Direct {
                reason: "typo".into()
            })
        );
        assert_eq!(
            classify(&input("", "plan/next", &records)),
            Ok(Class::PlanningOnly)
        );
        let mut line = input("", "integration/EPC-001-x", &code);
        line.epic_line = Some(Ok("EPC-001".into()));
        assert_eq!(classify(&line), Ok(Class::EpicLine("EPC-001".into())));
        let mut bot = input("", "chore/deps-bump", &code);
        bot.automation = Some("dependabot");
        assert_eq!(
            classify(&bot),
            Ok(Class::Automation {
                profile: "dependabot".into()
            })
        );
        let mut branch = input("", "task/TSK-004-x", &code);
        branch.branch_task = Some("TSK-004".into());
        assert_eq!(
            classify(&branch),
            Ok(Class::Tracked {
                task_id: "TSK-004".into(),
                derived: true
            })
        );
    }

    #[test]
    fn unclassified_mismatched_and_ambiguous_bodies_are_refused() {
        let code = paths(&["src/lib.rs"]);
        assert!(classify(&input("", "fix/typo", &code))
            .unwrap_err()
            .contains("not planning-only"));
        assert!(classify(&input("", "plan/next", &code))
            .unwrap_err()
            .contains("planning-only pull request touches a product path: src/lib.rs"));
        let mut mismatch = input("Task: TSK-005", "task/TSK-004-x", &code);
        mismatch.branch_task = Some("TSK-004".into());
        assert!(classify(&mismatch)
            .unwrap_err()
            .contains("names another task"));
        assert!(
            classify(&input("Task: TSK-004\nTask: TSK-005", "fix/x", &code))
                .unwrap_err()
                .contains("2 `Task:` lines")
        );
        let templates = paths(&["project-management/templates/task.md"]);
        assert!(classify(&input("", "plan/next", &templates)).is_err());
    }

    #[test]
    fn a_prefix_or_a_direct_line_grants_no_lighter_class() {
        let code = paths(&["src/lib.rs"]);
        // An integration/ head is an epic line only when verified.
        assert!(classify(&input("", "integration/not-an-epic", &code))
            .unwrap_err()
            .contains("not a verified epic line"));
        let mut line = input("", "integration/EPC-009-x", &code);
        line.epic_line = Some(Err("no task of EPC-009 targets it".into()));
        assert!(classify(&line).unwrap_err().contains("no task of EPC-009"));
        // `Task: none` cannot drop the task a branch carries, or a spike.
        let mut carried = input("Task: none: small edit", "spike/TSK-002-probe", &code);
        carried.branch_task = Some("TSK-002".into());
        assert!(classify(&carried).unwrap_err().contains("carries TSK-002"));
        assert!(
            classify(&input("Task: none: small edit", "spike/probe", &code))
                .unwrap_err()
                .contains("spike is tracked work")
        );
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
