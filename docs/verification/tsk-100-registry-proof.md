# Shared id registry proof on GitHub

This records the TSK-100 host proof: the id registry of SPC-013 run end to
end on a real GitHub repository before TSK-101 builds it. It is the gate for
EPC-020. On 2026-09-26 AC-1 to AC-6 passed and nothing failed, so decision 1
(the protected data branch `codeflow/registry`) is not returned. AC-8 passed
on 2026-09-27 when GitHub started the first scheduled run of the check. The
proof also found one gap in the R-109
arrangement that the design must close before TSK-101 ships, and two smaller
facts that TSK-101 must handle. They are listed under Findings.

- Host: `https://github.com/sathyassn/codeflow-registry-proof` (public, owned
  by a personal account, disposable, authorized by the operator).
- Script: `scripts/proofs/registry-proof/run.sh`, with `ids.py` (a stand-in
  for `codeflow ids check|next|bound`) and `registry-check.yml` (the
  code-branch workflow).
- Transcripts and host listings: `docs/verification/evidence/tsk-100/`.

```text
 code branch: main                          data branch: codeflow/registry
 ruleset proof-main-code-profile            ruleset codeflow-registry-data-profile
 (deletion, non_fast_forward,               (deletion, non_fast_forward;
  pull_request)                              no pull_request, no bypass)
 +---------------------------------+        +------------------------------+
 | project-management/tasks/*.md   |        | orphan root: seed: TSK-001   |
 | ci/ids.py         (check code)  | fetch  | issue: TSK-002 ... TSK-014   |
 | .github/workflows/              | <----- | admit: TSK-008               |
 |   registry-check.yml            | read   | ids/TSK/<N>.toml only        |
 |   pull_request, push (main,     | only   | no workflow, no code         |
 |   integration/**), schedule     |        +------------------------------+
 |   permissions: contents: read   |              ^ plain push only
 +---------------------------------+              | issuers A, B, maintainer
```

## How to rerun

```text
export REPO=<owner>/<disposable repo> WORK=<scratch dir> OUT=<transcript dir>
scripts/proofs/registry-proof/run.sh setup      # then, in order:
protect  issue  race  lost-ack  fork  delete  pushrule  pr-edit  evidence
```

Each step writes `OUT/<step>.txt` and exits non-zero on a failed assertion.
The steps are ordered: `delete` damages the registry on purpose, and
`pr-edit` relies on that damage.

## Results

| Criterion | Result | Evidence |
|---|---|---|
| AC-1 data rules through the API; deletion and force push refused; no PR requirement; `main` unchanged | pass | `protect.txt`, `rules-before-*.json`, `rules-after-*.json` |
| AC-2 plain push of a reservation accepted; plain push onto the moved tip rejected | pass | `issue.txt`, `race.txt` |
| AC-3 lost acknowledgement resolved by read back of the `uid` | pass | `lost-ack.txt` |
| AC-4 deleted top reservation not reissued; code-branch CI reports it | pass | `delete.txt`, runs 36276880984 and 36276894678 |
| AC-5 fork-style record red until admitted, green after | pass | `fork.txt`, run 36276831759 attempts 1 and 2 |
| AC-6 file-path push rule on the host | recorded: not offered for this repository, and unsuitable where offered | `pushrule.txt` |
| AC-7 failure hand-off | not triggered: no criterion failed | this record |
| AC-8 R-109 arrangement, one run of each job kind, read-only permissions | pass | `evidence.txt`, `workflow-main.yml`, `runs.json`, run 36283958457 |

### AC-1: data-branch rules

The script created ruleset `codeflow-registry-data-profile` (id 24054251)
through `POST /repos/{owner}/{repo}/rulesets`. The effective rules for
`codeflow/registry` went from none to `deletion` and `non_fast_forward`, with
no `pull_request` rule. The `main` ruleset (id 24054247) was byte-identical
before and after, and `main`'s effective rules did not change.

The host then refused, with the owner's admin credentials and no bypass:

- `git push --delete`: `GH013 ... Cannot delete this branch`.
- `git push --force` of a rewritten root: `GH013 ... Cannot force-push to this branch`.
- `PATCH git/refs/heads/codeflow/registry` with `force=true`: HTTP 422, the same rule.
- `DELETE git/refs/heads/codeflow/registry`: HTTP 422, the same rule.

The tip was unchanged afterwards. A plain push with no pull request is
accepted, shown in AC-2.

### AC-2: issue by compare-and-swap push

Issuers A and B fetched the same tip. A computed TSK-002 from history, and its
plain push was accepted. B, still on the old tip, pushed its own TSK-002 and
was rejected (`[rejected] (fetch first)`). B then followed R-13: fetch,
recompute, retry, and reserved TSK-003. A read back TSK-002 bound to its
`uid`.

That rejection comes from the client reading the host's ref advertisement.
`race.txt` covers the server side: in three trials A and B pushed the same
number at the same moment. Each time exactly one push was accepted, and the
host refused the other with `[remote rejected] (cannot lock ref ... is at
<new> but expected <old>)`. The script classifies that as a moved tip.

### AC-3: lost acknowledgement

A pushed its TSK-004 reservation to the repository URL with all output and the
exit status discarded, so A had no acknowledgement and its remote-tracking ref
did not move. B then issued TSK-005 on top. A fetched and read back: TSK-004
is bound to A's `uid`, A's commit is an ancestor of the tip and not the tip,
so A treats TSK-004 as reserved.

Fault control: A prepared TSK-006 and never sent it, and B took TSK-006. A's
read back found TSK-006 bound to another `uid`, so A did not treat it as
reserved, and A's retry reserved TSK-007.

The lost acknowledgement is simulated at the client; the host side is real.

### AC-4: deleted top reservation

A plain commit deleting `ids/TSK/008.toml`, the top reservation, was pushed
and the host accepted it; branch rules do not look at file contents (AC-6).
Afterwards:

- Allocation from the tip alone would have issued TSK-008 again. Allocation
  from history gave TSK-009 (R-7).
- Issue was refused because the registry check failed (R-9).
- The PR job on the code branch (run 36276880984) and a run on `main` (run
  36276894678, `workflow_dispatch`) were red and reported
  `deletes ids/TSK/008.toml (R-8)` and the damaged tip (R-9).

The registry is left damaged on purpose; typed restore (R-108) is out of
scope for this proof.

### AC-5: fork-style admission

A contributor wrote TSK-008 by hand with its own `uid` and no reservation and
opened pull request #1. Its check (run 36276831759, attempt 1) was red:
`TSK-008: not reserved in the registry; a maintainer must admit uid ...`. A
maintainer pushed `admit: TSK-008` binding that `uid`. The same check, rerun
(attempt 2), was green and reported `bound: TSK-008 -> <uid>`.

The contributor pushed a branch in the same repository. A cross-account fork
was not possible, because agents may not create repositories. The check needs
no secret and no write scope, so it would behave the same on a fork.

### AC-6: file-path push rule

GitHub offers `file_path_restriction` only in push rulesets. The host refused
both forms on this repository:

- Push ruleset: HTTP 422, `Source public repos cannot have push rules` and
  `Source only org-owned repos can have push rules`.
- Branch ruleset with the same rule: HTTP 422, `Invalid rule
  'file_path_restriction'`.

Where push rulesets are available (private or internal repositories owned by
an organization), GitHub's own description of the rule is "Prevent commits
that include changes in specified file paths from being pushed" (GitHub Docs,
"Available rules for rulesets"). It would refuse additions under `ids/` as
well, so it would block issue itself. It cannot add append-only prevention
for free. R-8's pre-push and git-guard checks and the CI check stay the
controls.

### AC-8: the R-109 arrangement

- The registry branch is an orphan with one root commit, `seed: TSK-001`,
  which adds only `ids/TSK/001.toml`. Every path on the tip is an
  `ids/<KIND>/<N>.toml` file; there is no workflow or code on it.
- The workflow lives on `main` only. It fetches `codeflow/registry` explicitly
  and read-only, reads the check code from the target branch
  (`origin/target:ci/ids.py`), and runs on `pull_request`, on `push` to `main`
  and `integration/**`, and on a schedule.
- Its permission block is `contents: read`. The push, pull request and
  schedule run logs show `GITHUB_TOKEN Permissions: Contents: read, Metadata:
  read`, and the job uses no token to fetch.
- One run of each kind: push 36276768160 and pull request 36276831759 are
  green. Schedule run 36283958457 (event `schedule`, branch `main`, created
  2026-09-27T00:55:26Z, job `registry-check`) ran from the code branch with
  the same read-only permissions and reported `registry check: FAIL (2
  findings)`: `deletes ids/TSK/008.toml (R-8)` and `damaged:
  ids/TSK/008.toml ... absent from the tip (R-9)`.

That FAIL is the check working, not a proof failure. It is the damage the
AC-4 fault case left in the registry on purpose, and the earlier
`workflow_dispatch` run 36276894678 on `main` reported the same two
findings. The scheduled job therefore shows the third job kind running from
the code branch with read-only scope and detecting damage that persists
between pushes. Log: `run-36283958457-schedule-latest.log`.

The workflow reached `main` at 22:35 UTC with a `*/15` cron, chosen so that a
scheduled run would happen during the proof; the product cadence is daily.
GitHub started the first scheduled run about 2 hours 20 minutes later, and
by 04:04 UTC it had started no other. `gh run view --log` came back empty
for that run, so `run.sh evidence` now falls back to the job log API.

## Findings

1. **A pull request chooses the workflow that checks it (material, R-109,
   R-113, R-14).** Under `pull_request`, GitHub runs the workflow file from the
   pull request's merge commit. Pull request #2 replaced the check step with an
   `echo`. Its run (36276985397) was green while the registry was damaged and
   every honest run was red. Reading the check code from the target branch
   does not help, because the workflow that reads it is itself replaceable. So
   R-109's "a PR cannot select the binary that checks it" does not hold as
   arranged, and the merge rule is adjustable by the pull request. Options for
   the design owner: run the check under `pull_request_target`, which uses the
   base branch's workflow (safe here only because the check reads pull
   request content as data and never runs it); an organization-level required
   workflow (needs an organization); or a protected-path review rule on
   `.github/`. This proof did not test any of them. This changes R-109, so it
   goes back to the primaries rather than into TSK-101 silently.
2. **The host reports a lost compare-and-swap as a remote rejection (R-13).**
   Two simultaneous pushes produce `[remote rejected] (cannot lock ref ... but
   expected ...)` for the loser, not `[rejected] (fetch first)`. TSK-101's
   classifier must map it to "the tip moved, retry", not to a refusal or a
   permission error. The first run of this proof's own race step classified
   it as a refusal; the script was corrected and the step rerun.
3. **Binding lookup reads the tip (R-14, R-17).** After the TSK-008
   reservation was deleted, the PR check told the maintainer to admit TSK-008
   again. Doing so would re-add a path, which R-8 forbids; the right repair is
   a typed restore. The blocking result is still correct, because R-9 already
   fails, but TSK-101 should take the binding from the first addition in
   history and name `ids restore` when the tip is damaged.

## What was not tested

- A real cross-account fork (see AC-5).
- CodeFlow's own git hooks and git-guard on the registry branch: the scratch
  clones have no CodeFlow hooks, and the product rules for the data profile
  arrive in TSK-101.
- Typed restore (R-108), offline issue and `ids sync` (R-15), retarget (R-16),
  resume (R-18) and hosts without branch rules (R-10).
- A shallow `actions/checkout` (R-20). The workflow fetches three refs into an
  empty repository instead.
- A required status check on `main`. The baseline `main` ruleset requires a
  pull request but no check, because Finding 1 makes a required check of this
  workflow unsound.

## Proof setup choices

- `main` of the disposable repository received three commits through the
  contents API before its ruleset existed: the record TSK-001, `ci/ids.py`
  and the workflow. The operator authorized adding workflows there, and a
  scheduled workflow must be on the default branch.
- The `main` ruleset `proof-main-code-profile` is a fixture created by the
  proof, so that "unchanged" compares a real rule set rather than an empty one.
- Proof identities use `.invalid` email addresses.

## Host state left for cleanup

Rulesets 24054247 and 24054251, branches `codeflow/registry`,
`proof/rewrite-candidate`, `task/TSK-008-fork-record` and
`task/TSK-010-workflow-edit`, open pull requests #1 and #2, and the workflow
with its `*/15` schedule, which keeps running until it is disabled or the
repository is removed.
