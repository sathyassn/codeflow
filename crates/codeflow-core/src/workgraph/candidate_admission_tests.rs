//! TSK-1351 AC-1: a member is admitted only at the exact head of an open
//! pull request into the line, with every declared `trusted:` check green
//! there and produced by GitHub Actions. The forge is a recorded host: a
//! `gh` stand-in answering each API path from a table.

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
}

impl Forge {
    /// Open pull requests into the line: `(number, head, state, base)`.
    fn pulls(mut self, pulls: &[(u64, &str, &str, &str)]) -> Self {
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
            format!(
                "repos/{NWO}/pulls?state=open&base={LINE}&per_page=100&page=1"
            ),
            Value::Array(list).to_string(),
        );
        self
    }

    /// The check runs at `sha`: `(id, name, status, conclusion, app)`.
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
                })
            })
            .collect();
        self.answers.insert(
            format!("repos/{NWO}/commits/{sha}/check-runs?per_page=100&page=1"),
            serde_json::json!({"total_count": list.len(), "check_runs": list}).to_string(),
        );
        self
    }

    fn green(self, sha: &str) -> Self {
        self.runs(
            sha,
            &[(10, CHECK, "completed", Some("success"), GITHUB_ACTIONS_APP_ID)],
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
    let older = Forge::default().pulls(&pulls).green(REVIEWED);
    refused(
        older.admit(&[COMPLETED]),
        &format!("its trusted admission check '{CHECK}' has no run at that exact commit"),
    );
    let failed = Forge::default().pulls(&pulls).runs(
        COMPLETED,
        &[
            (10, CHECK, "completed", Some("success"), GITHUB_ACTIONS_APP_ID),
            (11, CHECK, "completed", Some("failure"), GITHUB_ACTIONS_APP_ID),
        ],
    );
    refused(failed.admit(&[COMPLETED]), "concluded failure");
    let pending = Forge::default().pulls(&pulls).runs(
        COMPLETED,
        &[(12, CHECK, "in_progress", None, GITHUB_ACTIONS_APP_ID)],
    );
    refused(pending.admit(&[COMPLETED]), "is in_progress");
    let foreign = Forge::default().pulls(&pulls).runs(
        COMPLETED,
        &[(13, CHECK, "completed", Some("success"), 99)],
    );
    refused(foreign.admit(&[COMPLETED]), "has no run");
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
    assert!(none.contains("the project has no forge admission check"), "{none}");
    assert!(none.contains("one at a time"), "{none}");
    let untrusted = admission_checks(Some(
        "on: pull_request\njobs:\n  build:\n    name: codeflow gates\n    runs-on: x\n    steps: []\n",
    ))
    .unwrap_err();
    assert!(untrusted.contains("declares no `trusted: ` check"), "{untrusted}");
    assert_eq!(admission_checks(Some(POLICY)).unwrap(), vec![CHECK.to_string()]);
}
