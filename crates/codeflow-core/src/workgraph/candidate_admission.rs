//! Each member of a landing candidate bound to an admitted pull request
//! head (TSK-1351 AC-1).
//!
//! A member merge's second parent must be the exact head commit of an open
//! pull request into the candidate's line, and every `trusted:` check the
//! policy workflow declares at the default branch's tip must be green at
//! that commit by a run of the policy workflow itself. Both facts are read
//! from the forge at pre-push. The pull request's head and base are the
//! forge's record, and the names the checks must carry come from the
//! default branch's tip, never from the candidate.
//!
//! A check run's name and app do not say which workflow wrote it: any
//! workflow a pull request adds runs as GitHub Actions and can name its job
//! `trusted: ...`. So the newest run of each name must belong to a check
//! suite whose one workflow run is [`POLICY_WORKFLOW`] on
//! `pull_request_target`, the event GitHub runs from the default branch's
//! own copy of that file, at the member's head, for the member's pull
//! request into the line. A newer green run from any other workflow or
//! event refuses rather than outranking the policy run, and so does a
//! suite or workflow run the forge does not show. This binds the verdict
//! to the workflow's path and event as the forge records them; which
//! revision of the file ran rests on what `pull_request_target` itself
//! guarantees, the default branch's copy.
//!
//! This proves admission at the head, the hosted verdicts and record checks
//! the trusted workflow ran there. It does not prove that a person reviewed
//! the head: a completion block in a member's commits is recheckable, not
//! authenticated, and is no input here.
//!
//! Every read that fails, and a project whose default branch declares no
//! `trusted:` check, refuses the candidate with the cause: landing the pull
//! requests one at a time stays the path.

use serde_json::Value;

use crate::remote::{GITHUB_ACTIONS_APP_ID, MAX_LIST_PAGES, POLICY_WORKFLOW, TRUSTED_PREFIX};

/// Entries read per page of a forge list.
const PAGE_SIZE: usize = 100;

/// A `gh` call: its arguments, answering stdout or the failure's text.
pub type Gh<'a> = &'a dyn Fn(&[&str]) -> Result<String, String>;

/// One member merge of a candidate: the merge and its second parent, the
/// member's head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The member merge on the candidate's first-parent chain.
    pub merge: String,
    /// The merge's second parent.
    pub head: String,
}

/// A member bound to the pull request whose head it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admitted {
    /// The member's head commit.
    pub head: String,
    /// The open pull request into the line whose head it is.
    pub pull: u64,
}

/// The `trusted:` check names the policy workflow declares, read from its
/// text at the default branch's tip (`policy` is `None` when the file is
/// not there). The caller reads the text from that tip, never from the
/// candidate.
///
/// # Errors
///
/// Returns why the project has no forge admission check to read.
pub fn admission_checks(policy: Option<&str>) -> Result<Vec<String>, String> {
    let none = |why: &str| {
        format!(
            "the project has no forge admission check: {why}, so no member head can be shown admitted; land the pull requests one at a time"
        )
    };
    let text =
        policy.ok_or_else(|| none(&format!("{POLICY_WORKFLOW} is not on the default branch")))?;
    let names: Vec<String> = crate::remote::workflow_job_names(text)
        .map_err(|why| {
            none(&format!(
                "{POLICY_WORKFLOW} at the default branch cannot be read ({why})"
            ))
        })?
        .into_iter()
        .filter(|name| name.starts_with(TRUSTED_PREFIX))
        .collect();
    if names.is_empty() {
        return Err(none(&format!(
            "{POLICY_WORKFLOW} at the default branch declares no `{TRUSTED_PREFIX}` check"
        )));
    }
    Ok(names)
}

/// Bind every member to an open pull request into `line` whose head is the
/// member's head, with every name in `checks` green there. `nwo` is the
/// forge repository, `owner/name`.
///
/// # Errors
///
/// Returns the cause for the first member that fails, or for a forge read
/// that fails.
pub fn admit(
    gh: Gh<'_>,
    nwo: &str,
    line: &str,
    checks: &[String],
    members: &[Member],
) -> Result<Vec<Admitted>, String> {
    let shown = crate::git::display_text(line);
    let pulls = open_pulls(gh, nwo, line)?;
    let mut admitted = Vec::new();
    for member in members {
        let head = &member.head;
        let short = |sha: &str| sha.get(..9).unwrap_or(sha).to_string();
        let Some(pull) = pulls
            .iter()
            .find(|(_, at)| at.eq_ignore_ascii_case(head))
            .map(|(number, _)| *number)
        else {
            return Err(format!(
                "member head {} (merged by {}) is not the head of an open pull request into '{shown}': a member is merged at its pull request's exact current head, and a closed, retargeted or rewritten pull request is not one",
                short(head),
                short(&member.merge)
            ));
        };
        let runs = check_runs(gh, nwo, head)?;
        for name in checks {
            // The newest run of the name that GitHub Actions produced
            // decides; a run any other app reported proves nothing. A green
            // one counts only when the policy workflow wrote it for this
            // pull request at this head.
            let newest = runs
                .iter()
                .filter(|run| run.name == *name && run.app == Some(GITHUB_ACTIONS_APP_ID))
                .max_by_key(|run| run.id);
            let verdict = match newest {
                None => Some("has no run".to_string()),
                Some(run) if run.status != "completed" => Some(format!("is {}", run.status)),
                Some(run) if run.conclusion.as_deref() != Some("success") => Some(format!(
                    "concluded {}",
                    run.conclusion.as_deref().unwrap_or("with no conclusion")
                )),
                Some(run) => producer(gh, nwo, line, pull, head, run)?,
            };
            if let Some(verdict) = verdict {
                return Err(format!(
                    "member head {} of pull request #{pull} is not admitted: its trusted admission check '{name}' {verdict} at that exact commit (a green run on an older head does not count)",
                    short(head)
                ));
            }
        }
        admitted.push(Admitted {
            head: head.clone(),
            pull,
        });
    }
    Ok(admitted)
}

/// [`admit`] against the forge itself: the checks the policy workflow
/// declares at `default_tip`, the forge repository `origin` names, which
/// the push's `destination` must also name, and `gh` pinned to that host.
///
/// # Errors
///
/// Returns why the project has no admission check, why the forge cannot be
/// read, or the first member that fails.
pub fn admit_at_forge(
    root: &std::path::Path,
    destination: &str,
    line: &str,
    default_tip: &str,
    members: &[Member],
) -> Result<Vec<Admitted>, String> {
    let policy = super::acceptance::text_at(root, default_tip, POLICY_WORKFLOW)?;
    let checks = admission_checks(policy.as_deref())?;
    let (host, nwo) = crate::remote::origin_host_repository(root)
        .map_err(|why| unreadable("the forge repository", &why))?
        .ok_or_else(|| {
            unreadable(
                "the forge repository",
                "`origin` names no forge repository as host/owner/name",
            )
        })?;
    let pushed = crate::remote::parse_repo_url(destination);
    if pushed.as_ref() != Some(&(host.clone(), nwo.clone())) {
        return Err(format!(
            "the push goes to {}, not to {nwo} on {host}, the forge repository `origin` names, so the member pull requests read there do not bind this push",
            crate::git::remote_query::display_urls(destination)
        ));
    }
    let gh_path = which::which("gh")
        .map_err(|why| unreadable("the GitHub CLI", &format!("`gh` is not found: {why}")))?;
    let gh = |args: &[&str]| crate::remote::run_gh(&gh_path, root, Some(&host), args, None);
    admit(&gh, &nwo, line, &checks, members)
}

/// Every open pull request into `line`: its number and head commit. A pull
/// request the forge returns with another base is dropped, so the filter
/// is never trusted alone.
fn open_pulls(gh: Gh<'_>, nwo: &str, line: &str) -> Result<Vec<(u64, String)>, String> {
    let base = query_value(line);
    let mut pulls = Vec::new();
    for page in 1..=MAX_LIST_PAGES {
        let answer = gh(&[
            "api",
            &format!("repos/{nwo}/pulls?state=open&base={base}&per_page={PAGE_SIZE}&page={page}"),
        ])
        .map_err(|error| unreadable("the open pull requests", &error))?;
        let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(&answer) else {
            return Err(unreadable(
                "the open pull requests",
                &format!("page {page} is not a JSON list"),
            ));
        };
        let short = entries.len() < PAGE_SIZE;
        for entry in &entries {
            let open = entry.get("state").and_then(Value::as_str) == Some("open");
            let into = entry.pointer("/base/ref").and_then(Value::as_str) == Some(line);
            let number = entry.get("number").and_then(Value::as_u64);
            let head = entry.pointer("/head/sha").and_then(Value::as_str);
            match (open && into, number, head) {
                (false, _, _) => {}
                (true, Some(number), Some(head)) => pulls.push((number, head.to_string())),
                _ => {
                    return Err(unreadable(
                        "the open pull requests",
                        "an entry has no number or head commit",
                    ))
                }
            }
        }
        if short {
            return Ok(pulls);
        }
    }
    Err(unreadable(
        "the open pull requests",
        &format!("the list did not end within {MAX_LIST_PAGES} pages"),
    ))
}

/// One check run at a commit.
struct Run {
    id: u64,
    name: String,
    status: String,
    conclusion: Option<String>,
    app: Option<u64>,
    /// The check suite the run belongs to, which names its workflow run.
    suite: Option<u64>,
}

/// The event GitHub runs from the default branch's own copy of the
/// workflow file, whatever the pull request changes.
const TRUSTED_EVENT: &str = "pull_request_target";

/// Why the green `run` at `head` does not come from the policy workflow
/// judging pull request `pull` into `line`, or `None` when it does: its
/// check suite's one workflow run must be [`POLICY_WORKFLOW`] on
/// [`TRUSTED_EVENT`] at `head`, for `pull` with that head and base `line`.
///
/// # Errors
///
/// Returns why the check suite's workflow run cannot be read.
fn producer(
    gh: Gh<'_>,
    nwo: &str,
    line: &str,
    pull: u64,
    head: &str,
    run: &Run,
) -> Result<Option<String>, String> {
    let what = format!("the workflow run of check run {}", run.id);
    let Some(suite) = run.suite else {
        return Err(unreadable(&what, "the check run names no check suite"));
    };
    let answer = gh(&[
        "api",
        &format!("repos/{nwo}/actions/runs?check_suite_id={suite}"),
    ])
    .map_err(|error| unreadable(&what, &error))?;
    let workflow_runs = serde_json::from_str::<Value>(&answer)
        .ok()
        .and_then(|value| {
            value
                .get("workflow_runs")
                .and_then(Value::as_array)
                .cloned()
        })
        .ok_or_else(|| unreadable(&what, "the answer holds no workflow_runs list"))?;
    let [workflow_run] = workflow_runs.as_slice() else {
        return Ok(Some(format!(
            "comes from check suite {suite}, which the forge shows with {} workflow runs, not one {POLICY_WORKFLOW} run",
            workflow_runs.len()
        )));
    };
    let field = |name: &str| workflow_run.get(name).and_then(Value::as_str);
    let path = field("path").unwrap_or("(no path)");
    let event = field("event").unwrap_or("(no event)");
    if path != POLICY_WORKFLOW || event != TRUSTED_EVENT {
        return Ok(Some(format!(
            "comes from {} on {}, not from {POLICY_WORKFLOW} on {TRUSTED_EVENT}; a check of the same name from another workflow or event proves nothing",
            crate::git::display_text(path),
            crate::git::display_text(event)
        )));
    }
    let ran_at = field("head_sha").unwrap_or("(no commit)");
    if !ran_at.eq_ignore_ascii_case(head) {
        return Ok(Some(format!(
            "comes from a {POLICY_WORKFLOW} run at {}, another commit",
            crate::git::display_text(ran_at.get(..9).unwrap_or(ran_at))
        )));
    }
    let judged = workflow_run
        .get("pull_requests")
        .and_then(Value::as_array)
        .is_some_and(|pulls| {
            pulls.iter().any(|entry| {
                entry.get("number").and_then(Value::as_u64) == Some(pull)
                    && entry
                        .pointer("/head/sha")
                        .and_then(Value::as_str)
                        .is_some_and(|sha| sha.eq_ignore_ascii_case(head))
                    && entry.pointer("/base/ref").and_then(Value::as_str) == Some(line)
            })
        });
    if !judged {
        return Ok(Some(format!(
            "comes from a {POLICY_WORKFLOW} run the forge does not show judging pull request #{pull} at this head into '{}'",
            crate::git::display_text(line)
        )));
    }
    Ok(None)
}

/// Every check run at `sha`.
fn check_runs(gh: Gh<'_>, nwo: &str, sha: &str) -> Result<Vec<Run>, String> {
    let what = format!("the check runs at {}", sha.get(..9).unwrap_or(sha));
    let mut runs = Vec::new();
    for page in 1..=MAX_LIST_PAGES {
        let answer = gh(&[
            "api",
            &format!("repos/{nwo}/commits/{sha}/check-runs?per_page={PAGE_SIZE}&page={page}"),
        ])
        .map_err(|error| unreadable(&what, &error))?;
        let listed = serde_json::from_str::<Value>(&answer)
            .ok()
            .and_then(|value| value.get("check_runs").and_then(Value::as_array).cloned())
            .ok_or_else(|| unreadable(&what, &format!("page {page} holds no check_runs list")))?;
        let short = listed.len() < PAGE_SIZE;
        for run in &listed {
            let (Some(id), Some(name), Some(status)) = (
                run.get("id").and_then(Value::as_u64),
                run.get("name").and_then(Value::as_str),
                run.get("status").and_then(Value::as_str),
            ) else {
                return Err(unreadable(&what, "a run has no id, name or status"));
            };
            runs.push(Run {
                id,
                name: name.to_string(),
                status: status.to_string(),
                conclusion: run
                    .get("conclusion")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                app: run.pointer("/app/id").and_then(Value::as_u64),
                suite: run.pointer("/check_suite/id").and_then(Value::as_u64),
            });
        }
        if short {
            return Ok(runs);
        }
    }
    Err(unreadable(
        &what,
        &format!("the list did not end within {MAX_LIST_PAGES} pages"),
    ))
}

fn unreadable(what: &str, why: &str) -> String {
    format!(
        "the forge cannot be read ({what}: {}), so no member head can be shown admitted",
        why.lines().next().unwrap_or(why).trim()
    )
}

/// A text for a URL query value: unreserved characters and `/` stay, every
/// other byte is percent-encoded.
fn query_value(text: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
#[path = "candidate_admission_tests.rs"]
mod tests;
