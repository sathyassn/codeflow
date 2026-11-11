//! Each member of a landing candidate bound to an admitted pull request
//! head (TSK-1351 AC-1).
//!
//! A member merge's second parent must be the exact head commit of an open
//! pull request into the candidate's line, and every `trusted:` check the
//! policy workflow declares at the default branch's tip must be green at
//! that commit, produced by GitHub Actions. Both facts are read from the
//! forge at pre-push. The pusher writes neither: the pull request's head
//! and base are the forge's record, the check runs at a commit are written
//! by the workflow the default branch holds, and the names they must carry
//! come from the default branch's tip, never from the candidate.
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
            // decides; a run any other app reported proves nothing.
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
                Some(_) => None,
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
