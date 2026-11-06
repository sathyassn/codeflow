//! The workflow-change finding (TSK-336 AC-11, ADR-0091): a range that
//! changes a managed workflow, `codeflow-release.yml` or any other file
//! under `.github/workflows/` or `.github/actions/` (TSK-1349) gets a warning
//! naming the triggers, the permissions and the jobs it changes. The
//! default branch's copy of a workflow judges every pull request, so a
//! head's edit takes effect only once it lands there; the warning puts
//! what it would change in front of the reviewer, who is the boundary for
//! that landing. It never blocks: the operator's own update of the policy
//! workflow passes through it.
use codeflow_core::hooks::{PolicyLevel, Violation};
use codeflow_core::scaffold::ScaffoldManifest;
use serde_yaml::{Mapping, Value};
use std::collections::BTreeSet;
use std::path::Path;

use super::TaggedViolation;
use crate::embedded::EmbeddedAssets;

/// The rule this finding prints under. It has no policy key and no level
/// to configure.
pub(super) const RULE: &str = "ci.workflow_change";

/// This repository's own release workflow, which no manifest entry
/// installs but which carries a verdict job.
const RELEASE_WORKFLOW: &str = ".github/workflows/codeflow-release.yml";

/// The workflow paths the finding watches: every managed workflow the
/// shipped manifest installs, and the release workflow. A manifest that
/// will not load leaves the known policy and CI workflows, so the watch
/// never empties.
fn watched() -> BTreeSet<String> {
    let mut paths: BTreeSet<String> = [
        ".github/workflows/codeflow-ci.yml",
        ".github/workflows/codeflow-policy.yml",
        RELEASE_WORKFLOW,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    if let Ok(manifest) = ScaffoldManifest::load(&EmbeddedAssets) {
        paths.extend(
            manifest
                .entries
                .into_iter()
                .map(|entry| entry.dest)
                .filter(|dest| dest.starts_with(".github/workflows/")),
        );
    }
    paths
}

/// Add one warning per watched workflow the range `base..head` changes.
pub(super) fn dispatch(
    root: &Path,
    base: Option<&str>,
    head: &str,
    tagged: &mut Vec<TaggedViolation>,
    ran: &mut Vec<&str>,
) {
    let Some(base) = base else {
        return;
    };
    let mut paths = watched();
    // Every workflow or local action the range adds, edits or removes,
    // whatever its name: a new file there runs once it lands (TSK-1349).
    match changed_workflows(root, base, head) {
        Ok(changed) => paths.extend(changed),
        Err(error) => tagged.push(TaggedViolation {
            sha: None,
            violation: Violation::new(
                RULE,
                PolicyLevel::Warn,
                format!(
                    "which workflows this change edits cannot be told ({error}); review every file under .github/workflows/ and .github/actions/ as a workflow change"
                ),
                codeflow_core::remedy::CI_WORKFLOW_CHANGED.with(&[("path", WORKFLOW_DIRS[0])]),
            ),
        }),
    }
    for path in paths {
        if let Some(message) = describe(root, base, head, &path) {
            tagged.push(TaggedViolation {
                sha: None,
                violation: Violation::new(
                    RULE,
                    PolicyLevel::Warn,
                    message,
                    codeflow_core::remedy::CI_WORKFLOW_CHANGED.with(&[("path", &path)]),
                ),
            });
        }
    }
    ran.push("workflow-change");
}

/// The finding as `codeflow ci` prints it: a warning with no policy key,
/// whose authority is the default branch's workflow.
pub(super) fn render(violation: &Violation, plane: &str) -> String {
    violation.render_with_authority(
        plane,
        "the default branch's copy of each workflow, which judges every pull request (ADR-0091)",
    )
}

/// The directories whose files run as CI code: the workflows GitHub reads,
/// and the local actions a workflow runs with `uses: ./.github/actions/<x>`.
const WORKFLOW_DIRS: [&str; 2] = [".github/workflows/", ".github/actions/"];

/// The paths under [`WORKFLOW_DIRS`] that differ between `base` and `head`,
/// compared without case so a spelling a case-insensitive checkout folds
/// into the directory is listed too.
fn changed_workflows(root: &Path, base: &str, head: &str) -> Result<Vec<String>, String> {
    let out = codeflow_core::git::judging_command()
        .args(["diff", "--name-only", "-z", "--no-renames", base, head])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run git diff: {error}"))?;
    if !out.status.success() {
        return Err(format!(
            "git diff failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out
        .stdout
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .filter(|name| {
            let lower = name.to_ascii_lowercase();
            WORKFLOW_DIRS.iter().any(|dir| lower.starts_with(dir))
        })
        .collect())
}

/// The bytes of `path` at `rev`: `Ok(None)` when absent. They stay bytes,
/// so a lossy decode can never make two different versions compare equal.
fn bytes_at(root: &Path, rev: &str, path: &str) -> Result<Option<Vec<u8>>, String> {
    let Some(id) = codeflow_core::git::blob_id_at(root, rev, path)? else {
        return Ok(None);
    };
    let out = codeflow_core::git::judging_command()
        .args(["cat-file", "blob", &id])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run git to read {path} at {rev}: {error}"))?;
    if !out.status.success() {
        return Err(format!(
            "cannot read {path} at {rev}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(Some(out.stdout))
}

/// What a version that is not valid UTF-8 lets the warning say.
const NOT_UTF8: &str = "a file that is not valid UTF-8 on one side, so its triggers, permissions and jobs cannot be listed";

/// The warning for one path, or `None` when the range leaves it unchanged.
/// The raw bytes decide whether it changed; the text only describes how.
pub(super) fn describe(root: &Path, base: &str, head: &str, path: &str) -> Option<String> {
    let (was, now) = match (bytes_at(root, base, path), bytes_at(root, head, path)) {
        (Ok(was), Ok(now)) => (was, now),
        (Err(error), _) | (_, Err(error)) => {
            return Some(format!(
                "whether this change edits {path} cannot be told ({error}); review it as a workflow change"
            ))
        }
    };
    if was == now {
        return None;
    }
    let text = |bytes: &[u8]| std::str::from_utf8(bytes).ok().map(str::to_owned);
    let changes = match (&was, &now) {
        (None, Some(bytes)) => format!(
            "adds it: {}",
            text(bytes).map_or_else(|| NOT_UTF8.to_owned(), |text| whole(&text))
        ),
        (Some(bytes), None) => format!(
            "removes it: {}",
            text(bytes).map_or_else(|| NOT_UTF8.to_owned(), |text| whole(&text))
        ),
        (Some(was), Some(now)) => match (text(was), text(now)) {
            (Some(was), Some(now)) => compare(&was, &now),
            _ => format!("changes {NOT_UTF8}"),
        },
        (None, None) => return None,
    };
    Some(format!(
        "this change edits {path}, which runs with the repository's token and decides required checks; it {changes}; the default branch's copy keeps judging until this lands there"
    ))
}

fn parse(text: &str) -> Option<Mapping> {
    match serde_yaml::from_str::<Value>(text) {
        Ok(Value::Mapping(map)) => Some(map),
        _ => None,
    }
}

/// The triggers of a workflow by event name, whatever form `on:` takes.
fn triggers(workflow: &Mapping) -> Mapping {
    // YAML 1.1 readers take a bare `on` as `true`; serde_yaml keeps the key.
    let on = workflow
        .get(Value::from("on"))
        .or_else(|| workflow.get(Value::Bool(true)));
    match on {
        Some(Value::Mapping(map)) => map.clone(),
        Some(Value::String(event)) => [(Value::from(event.as_str()), Value::Null)]
            .into_iter()
            .collect(),
        Some(Value::Sequence(events)) => events
            .iter()
            .map(|event| (event.clone(), Value::Null))
            .collect(),
        _ => Mapping::new(),
    }
}

fn jobs(workflow: &Mapping) -> Mapping {
    match workflow.get(Value::from("jobs")) {
        Some(Value::Mapping(map)) => map.clone(),
        _ => Mapping::new(),
    }
}

fn key(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim()
            .to_owned(),
    }
}

/// A job named by its id and, when it has one, its check name.
fn job_label(id: &Value, job: &Value) -> String {
    match job.get("name").and_then(Value::as_str) {
        Some(name) => format!("{} ('{name}')", key(id)),
        None => key(id),
    }
}

/// A one-line account of a whole added or removed workflow.
fn whole(text: &str) -> String {
    let Some(workflow) = parse(text) else {
        return "a file that is not a YAML mapping".to_owned();
    };
    let events: Vec<String> = triggers(&workflow).keys().map(key).collect();
    let jobs: Vec<String> = jobs(&workflow)
        .iter()
        .map(|(id, job)| job_label(id, job))
        .collect();
    format!("triggers {}; jobs {}", list(&events), list(&jobs))
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_owned()
    } else {
        items.join(", ")
    }
}

/// The named differences between two versions of one workflow.
fn compare(was: &str, now: &str) -> String {
    let (Some(was), Some(now)) = (parse(was), parse(now)) else {
        return "changes a file that is not a YAML mapping on one side, so its triggers, permissions and jobs cannot be listed".to_owned();
    };
    let mut parts = Vec::new();
    diff_keys(
        "triggers",
        &triggers(&was),
        &triggers(&now),
        |id, _| key(id),
        &mut parts,
    );
    let perms = Value::from("permissions");
    if was.get(&perms) != now.get(&perms) {
        parts.push("changes the workflow's permissions".to_owned());
    }
    let (old_jobs, new_jobs) = (jobs(&was), jobs(&now));
    diff_keys("jobs", &old_jobs, &new_jobs, job_label, &mut parts);
    let repermitted: Vec<String> = new_jobs
        .iter()
        .filter(|(id, job)| {
            old_jobs
                .get(*id)
                .is_some_and(|old| old.get("permissions") != job.get("permissions"))
        })
        .map(|(id, job)| job_label(id, job))
        .collect();
    if !repermitted.is_empty() {
        parts.push(format!(
            "changes the permissions of jobs {}",
            repermitted.join(", ")
        ));
    }
    let rest_changed = {
        let strip = |map: &Mapping| {
            let mut map = map.clone();
            for name in ["on", "jobs", "permissions"] {
                map.remove(Value::from(name));
            }
            map.remove(Value::Bool(true));
            map
        };
        strip(&was) != strip(&now)
    };
    if rest_changed {
        parts.push("changes its other top-level settings".to_owned());
    }
    if parts.is_empty() {
        "changes only its comments or layout".to_owned()
    } else {
        parts.join("; ")
    }
}

/// Added, removed and changed entries of one mapping, by key.
fn diff_keys(
    what: &str,
    was: &Mapping,
    now: &Mapping,
    label: impl Fn(&Value, &Value) -> String,
    parts: &mut Vec<String>,
) {
    let added: Vec<String> = now
        .iter()
        .filter(|(id, _)| !was.contains_key(*id))
        .map(|(id, value)| label(id, value))
        .collect();
    let removed: Vec<String> = was
        .iter()
        .filter(|(id, _)| !now.contains_key(*id))
        .map(|(id, value)| label(id, value))
        .collect();
    let changed: Vec<String> = now
        .iter()
        .filter(|(id, value)| was.get(*id).is_some_and(|old| old != *value))
        .map(|(id, value)| label(id, value))
        .collect();
    for (verb, items) in [("adds", added), ("removes", removed), ("changes", changed)] {
        if !items.is_empty() {
            parts.push(format!("{verb} {what} {}", items.join(", ")));
        }
    }
}
