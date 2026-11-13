//! TSK-1351 AC-1: a member is admitted only at the exact head of an open
//! pull request into the line, with every declared `trusted:` check green
//! there, written by the policy workflow's own run for that pull request
//! at that head. The forge is a recorded host: a `gh` stand-in answering
//! each API path from a table.

use std::collections::BTreeMap;

use super::*;

const NWO: &str = "acme/widgets";
const LINE: &str = "integration/EPC-001-line";
const CHECK: &str = "trusted: commit standards";
/// The reviewed commit a completion block names.
const REVIEWED: &str = "1111111111111111111111111111111111111111";
/// The completion commit on top of it: the pull request's final head.
const COMPLETED: &str = "2222222222222222222222222222222222222222";
/// A planning pull request's head.
const PLANNING: &str = "3333333333333333333333333333333333333333";
/// A head no pull request has.
const STRAY: &str = "4444444444444444444444444444444444444444";
const MERGE: &str = "5555555555555555555555555555555555555555";

const POLICY: &str = "on: pull_request_target\njobs:\n  commit-lint:\n    name: \"trusted: commit standards\"\n    runs-on: ubuntu-24.04\n    steps: []\n  build:\n    name: codeflow gates\n    runs-on: ubuntu-24.04\n    steps: []\n";

/// A recorded forge: the answer for each API path, and paths that fail.
#[derive(Default)]
struct Forge {
    answers: BTreeMap<String, String>,
    failing: Vec<String>,
    /// The recorded pull requests' numbers and heads.
    heads: Vec<(u64, String)>,
}

/// A workflow run as the forge records it: `(path, event, head, pulls)`,
/// each pull `(number, head, base)`.
type WorkflowRun<'a> = (&'a str, &'a str, &'a str, &'a [(u64, &'a str, &'a str)]);

/// A check run id, and its check suite's id, distinct per commit.
fn run_id(sha: &str) -> u64 {
    u64::from_str_radix(&sha[..8], 16).unwrap()
}

impl Forge {
    /// Open pull requests into the line: `(number, head, state, base)`.
    fn pulls(mut self, pulls: &[(u64, &str, &str, &str)]) -> Self {
        self.heads = pulls
            .iter()
            .map(|(number, head, _, _)| (*number, (*head).to_string()))
            .collect();
        let list: Vec<Value> = pulls
            .iter()
            .map(|(number, head, state, base)| {
                serde_json::json!({
                    "number": number,
                    "state": state,
                    "head": {"sha": head},
                    "base": {"ref": base},
                })
            })
            .collect();
        self.answers.insert(
            format!("repos/{NWO}/pulls?state=open&base={LINE}&per_page=100&page=1"),
            Value::Array(list).to_string(),
        );
        self
    }

    /// The check runs at `sha`: `(id, name, status, conclusion, app)`. Each
    /// run's check suite has the run's id.
    fn runs(mut self, sha: &str, runs: &[(u64, &str, &str, Option<&str>, u64)]) -> Self {
        let list: Vec<Value> = runs
            .iter()
            .map(|(id, name, status, conclusion, app)| {
                serde_json::json!({
                    "id": id,
                    "name": name,
                    "status": status,
                    "conclusion": conclusion,
                    "app": {"id": app},
                    "check_suite": {"id": id},
                })
            })
            .collect();
        self.answers.insert(
            format!("repos/{NWO}/commits/{sha}/check-runs?per_page=100&page=1"),
            serde_json::json!({"total_count": list.len(), "check_runs": list}).to_string(),
        );
        self
    }

    /// The workflow runs of check suite `suite`, as the forge answers
    /// `actions/runs?check_suite_id=`.
    fn suite(mut self, suite: u64, runs: &[WorkflowRun<'_>]) -> Self {
        let list: Vec<Value> = runs
            .iter()
            .map(|(path, event, head, pulls)| {
                let pulls: Vec<Value> = pulls
                    .iter()
                    .map(|(number, head, base)| {
                        serde_json::json!({
                            "number": number,
                            "head": {"sha": head},
                            "base": {"ref": base},
                        })
                    })
                    .collect();
                serde_json::json!({
                    "check_suite_id": suite,
                    "path": path,
                    "event": event,
                    "head_sha": head,
                    "pull_requests": pulls,
                })
            })
            .collect();
        self.answers.insert(
            format!("repos/{NWO}/actions/runs?check_suite_id={suite}"),
            serde_json::json!({"total_count": list.len(), "workflow_runs": list}).to_string(),
        );
        self
    }

    /// The pull request recorded with head `sha`, else 7.
    fn pull_at(&self, sha: &str) -> u64 {
        self.heads
            .iter()
            .find(|(_, head)| head == sha)
            .map_or(7, |(number, _)| *number)
    }

    /// The admission check green at `sha`, written by the policy workflow
    /// on `pull_request_target` for the pull request whose head it is.
    fn green(self, sha: &str) -> Self {
        let id = run_id(sha);
        let pull = self.pull_at(sha);
        self.runs(
            sha,
            &[(
                id,
                CHECK,
                "completed",
                Some("success"),
                GITHUB_ACTIONS_APP_ID,
            )],
        )
        .suite(
            id,
            &[(
                POLICY_WORKFLOW,
                "pull_request_target",
                sha,
                &[(pull, sha, LINE)],
            )],
        )
    }

    fn admit(&self, heads: &[&str]) -> Result<Vec<Admitted>, String> {
        let gh = |args: &[&str]| -> Result<String, String> {
            let path = args.last().copied().unwrap_or_default();
            if self.failing.iter().any(|failing| failing == path) {
                return Err("HTTP 502: Bad Gateway".to_string());
            }
            self.answers
                .get(path)
                .cloned()
                .ok_or_else(|| format!("HTTP 404: Not Found ({path})"))
        };
        let members: Vec<Member> = heads
            .iter()
            .map(|head| Member {
                merge: MERGE.to_string(),
                head: (*head).to_string(),
            })
            .collect();
        let checks = admission_checks(Some(POLICY)).unwrap();
        admit(&gh, NWO, LINE, &checks, &members)
    }
}

fn refused(result: Result<Vec<Admitted>, String>, needle: &str) {
    match result {
        Ok(admitted) => panic!("admitted {admitted:?}, expected a refusal naming {needle:?}"),
        Err(cause) => assert!(cause.contains(needle), "{needle:?} not in: {cause}"),
    }
}

/// Positive: a member whose head ends in its completion commit is admitted
/// at that head, the pull request's final head, with no special case for
/// the completion.
#[test]
fn a_member_whose_head_ends_in_its_completion_commit_is_admitted() {
    let forge = Forge::default()
        .pulls(&[(7, COMPLETED, "open", LINE)])
        .green(COMPLETED);
    assert_eq!(
        forge.admit(&[COMPLETED]).unwrap(),
        vec![Admitted {
            head: COMPLETED.to_string(),
            pull: 7
        }]
    );
}

/// Positive: a planning member, with no completion at all, is admitted at
/// its pull request's head beside a task member.
#[test]
fn a_planning_member_is_admitted() {
    let forge = Forge::default()
        .pulls(&[(7, COMPLETED, "open", LINE), (8, PLANNING, "open", LINE)])
        .green(COMPLETED)
        .green(PLANNING);
    let admitted = forge.admit(&[COMPLETED, PLANNING]).unwrap();
    assert_eq!(admitted[1].pull, 8);
}

/// Refused: a member head no open pull request into the line has.
#[test]
fn a_member_head_with_no_admitted_pull_request_is_refused() {
    let forge = Forge::default()
        .pulls(&[(7, COMPLETED, "open", LINE)])
        .green(COMPLETED)
        .green(STRAY);
    refused(
        forge.admit(&[COMPLETED, STRAY]),
        "member head 444444444 (merged by 555555555) is not the head of an open pull request into",
    );
}

/// Refused: a rewritten member head, one that differs from the head its
/// pull request has now, even with the same content.
#[test]
fn a_rewritten_member_head_is_refused() {
    let forge = Forge::default()
        .pulls(&[(7, COMPLETED, "open", LINE)])
        .green(COMPLETED);
    refused(
        forge.admit(&[STRAY]),
        "is not the head of an open pull request",
    );
}

/// Refused: a completion block or review reference in the member's own
/// commits is no input. A member merged at the reviewed commit its
/// completion names, short of the pull request's head, is refused, and so
/// is a head whose fabricated completion names any commit at all.
#[test]
fn a_fabricated_completion_block_or_review_reference_is_refused() {
    let forge = Forge::default()
        .pulls(&[(7, COMPLETED, "open", LINE)])
        .green(COMPLETED)
        .green(REVIEWED);
    refused(
        forge.admit(&[REVIEWED]),
        "member head 111111111 (merged by 555555555) is not the head of an open pull request",
    );
}

/// Refused: a closed pull request, and one retargeted to another branch,
/// even when the forge's filter returns them.
#[test]
fn a_closed_or_retargeted_member_pull_request_is_refused() {
    let closed = Forge::default()
        .pulls(&[(7, COMPLETED, "closed", LINE)])
        .green(COMPLETED);
    refused(
        closed.admit(&[COMPLETED]),
        "is not the head of an open pull request",
    );
    let retargeted = Forge::default()
        .pulls(&[(7, COMPLETED, "open", "main")])
        .green(COMPLETED);
    refused(
        retargeted.admit(&[COMPLETED]),
        "is not the head of an open pull request",
    );
}

/// Refused: the admission check green only on an older head. At the member
/// head it failed, is still running, has no run, or is green only from an
/// app other than GitHub Actions; the newest run decides.
#[test]
fn an_admission_check_green_only_on_an_older_head_is_refused() {
    let pulls = [(7, COMPLETED, "open", LINE)];
    // The forge answers a commit with no check runs with an empty list.
    let older = Forge::default()
        .pulls(&pulls)
        .green(REVIEWED)
        .runs(COMPLETED, &[]);
    refused(
        older.admit(&[COMPLETED]),
        &format!("its trusted admission check '{CHECK}' has no run at that exact commit"),
    );
    let failed = Forge::default().pulls(&pulls).runs(
        COMPLETED,
        &[
            (
                10,
                CHECK,
                "completed",
                Some("success"),
                GITHUB_ACTIONS_APP_ID,
            ),
            (
                11,
                CHECK,
                "completed",
                Some("failure"),
                GITHUB_ACTIONS_APP_ID,
            ),
        ],
    );
    refused(failed.admit(&[COMPLETED]), "concluded failure");
    let pending = Forge::default().pulls(&pulls).runs(
        COMPLETED,
        &[(12, CHECK, "in_progress", None, GITHUB_ACTIONS_APP_ID)],
    );
    refused(pending.admit(&[COMPLETED]), "is in_progress");
    let foreign = Forge::default()
        .pulls(&pulls)
        .runs(COMPLETED, &[(13, CHECK, "completed", Some("success"), 99)]);
    refused(foreign.admit(&[COMPLETED]), "has no run");
}

/// One green admission check at `COMPLETED`, pull request 7's head, from
/// GitHub Actions in check suite 20, whose workflow runs are `runs`.
fn green_from(runs: &[WorkflowRun<'_>]) -> Forge {
    Forge::default()
        .pulls(&[(7, COMPLETED, "open", LINE)])
        .runs(
            COMPLETED,
            &[(
                20,
                CHECK,
                "completed",
                Some("success"),
                GITHUB_ACTIONS_APP_ID,
            )],
        )
        .suite(20, runs)
}

/// Refused (CWE-345): a green check of the trusted name from GitHub Actions
/// that another workflow wrote. A workflow the pull request adds runs as
/// GitHub Actions and can name its job `trusted: ...`; its newer green run
/// does not outrank the policy workflow's failed one.
#[test]
fn a_same_app_check_from_another_workflow_is_refused() {
    let lookalike = Forge::default()
        .pulls(&[(7, COMPLETED, "open", LINE)])
        .runs(
            COMPLETED,
            &[
                (
                    20,
                    CHECK,
                    "completed",
                    Some("failure"),
                    GITHUB_ACTIONS_APP_ID,
                ),
                (
                    21,
                    CHECK,
                    "completed",
                    Some("success"),
                    GITHUB_ACTIONS_APP_ID,
                ),
            ],
        )
        .suite(
            20,
            &[(
                POLICY_WORKFLOW,
                "pull_request_target",
                COMPLETED,
                &[(7, COMPLETED, LINE)],
            )],
        )
        .suite(
            21,
            &[(
                ".github/workflows/lookalike.yml",
                "pull_request_target",
                COMPLETED,
                &[(7, COMPLETED, LINE)],
            )],
        );
    refused(
        lookalike.admit(&[COMPLETED]),
        &format!("its trusted admission check '{CHECK}' comes from .github/workflows/lookalike.yml on pull_request_target, not from {POLICY_WORKFLOW} on pull_request_target"),
    );
    // The policy file's own path on `pull_request` runs the pull request's
    // copy of it, which the pull request can rewrite.
    let head_copy = green_from(&[(
        POLICY_WORKFLOW,
        "pull_request",
        COMPLETED,
        &[(7, COMPLETED, LINE)],
    )]);
    refused(
        head_copy.admit(&[COMPLETED]),
        &format!("comes from {POLICY_WORKFLOW} on pull_request, not from"),
    );
}

/// Refused: the policy workflow's green run judged another pull request,
/// or this one into another base.
#[test]
fn a_policy_check_for_another_pull_request_is_refused() {
    let other_pull = green_from(&[(
        POLICY_WORKFLOW,
        "pull_request_target",
        COMPLETED,
        &[(9, COMPLETED, LINE)],
    )]);
    refused(
        other_pull.admit(&[COMPLETED]),
        &format!("the forge does not show judging pull request #7 at this head into '{LINE}'"),
    );
    let other_base = green_from(&[(
        POLICY_WORKFLOW,
        "pull_request_target",
        COMPLETED,
        &[(7, COMPLETED, "main")],
    )]);
    refused(
        other_base.admit(&[COMPLETED]),
        "does not show judging pull request #7",
    );
    let no_pull = green_from(&[(POLICY_WORKFLOW, "pull_request_target", COMPLETED, &[])]);
    refused(
        no_pull.admit(&[COMPLETED]),
        "does not show judging pull request #7",
    );
}

/// Refused: the policy workflow's run behind the check ran at another
/// commit, or names the member's pull request with another head.
#[test]
fn a_policy_check_for_another_head_is_refused() {
    let other_commit = green_from(&[(
        POLICY_WORKFLOW,
        "pull_request_target",
        REVIEWED,
        &[(7, REVIEWED, LINE)],
    )]);
    refused(
        other_commit.admit(&[COMPLETED]),
        &format!("comes from a {POLICY_WORKFLOW} run at 111111111, another commit"),
    );
    let other_pull_head = green_from(&[(
        POLICY_WORKFLOW,
        "pull_request_target",
        COMPLETED,
        &[(7, REVIEWED, LINE)],
    )]);
    refused(
        other_pull_head.admit(&[COMPLETED]),
        "does not show judging pull request #7 at this head",
    );
}

/// Refused: a check suite whose workflow run the forge does not show, shows
/// twice, or cannot answer.
#[test]
fn an_unshown_policy_run_is_refused() {
    let none = green_from(&[]);
    refused(none.admit(&[COMPLETED]), "with 0 workflow runs");
    let run = (
        POLICY_WORKFLOW,
        "pull_request_target",
        COMPLETED,
        &[(7, COMPLETED, LINE)][..],
    );
    let two = green_from(&[run, run]);
    refused(two.admit(&[COMPLETED]), "with 2 workflow runs");
    let mut silent = green_from(&[run]);
    silent
        .failing
        .push(format!("repos/{NWO}/actions/runs?check_suite_id=20"));
    refused(
        silent.admit(&[COMPLETED]),
        "the forge cannot be read (the workflow run of check run 20",
    );
}

/// Refused: a forge that cannot be read, for the pull requests or for the
/// check runs, refuses rather than admitting.
#[test]
fn an_unreadable_forge_is_refused() {
    let mut forge = Forge::default()
        .pulls(&[(7, COMPLETED, "open", LINE)])
        .green(COMPLETED);
    forge.failing.push(format!(
        "repos/{NWO}/commits/{COMPLETED}/check-runs?per_page=100&page=1"
    ));
    refused(forge.admit(&[COMPLETED]), "the forge cannot be read");
    let silent = Forge::default();
    refused(silent.admit(&[COMPLETED]), "the forge cannot be read");
}

/// Refused: a project whose default branch has no policy workflow, or one
/// that declares no `trusted:` check, has no forge admission check.
#[test]
fn a_project_with_no_forge_admission_check_is_refused() {
    let none = admission_checks(None).unwrap_err();
    assert!(
        none.contains("the project has no forge admission check"),
        "{none}"
    );
    assert!(none.contains("one at a time"), "{none}");
    let untrusted = admission_checks(Some(
        "on: pull_request\njobs:\n  build:\n    name: codeflow gates\n    runs-on: x\n    steps: []\n",
    ))
    .unwrap_err();
    assert!(
        untrusted.contains("declares no `trusted: ` check"),
        "{untrusted}"
    );
    assert_eq!(
        admission_checks(Some(POLICY)).unwrap(),
        vec![CHECK.to_string()]
    );
}
