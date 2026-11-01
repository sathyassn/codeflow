---
id: ADR-0091
uid: b46da4ae-5968-48d1-b884-991a71c44836
title: "The default branch at one commit is the only policy authority"
date: 2026-10-31
status: accepted          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: none # none | one line naming what in architecture.md changes
---

<!-- ADRs are append-only: written at the moment of decision. Never rewrite
     existing content; you may set superseded_by, or append a clearly-dated
     Note. Warranted at Tier-3 decision
     points only (new dependency, schema change, boundary change), not one
     per task. If architecture_impact is not none, update docs/architecture.md
     in the same PR. -->

# ADR-0091: The default branch at one commit is the only policy authority

## Context

Issue 279 asked for a repository content policy. CodeFlow's response, the
content policy design (revision 5, 2026-10-29), found that the local hooks
and both hosted policy jobs took their policy from places a requester can
write. A line declared by a landed task record supplied path exceptions and
levels; with remote `HEAD` unset a sole `origin/main` or `origin/master` was
guessed; an empty tracking namespace fell back to `HEAD`; pre-push and
`codeflow ci` fell back to the working copy; the hosted jobs ran the pull
request base's code; and every required check was bound only to a job name
any workflow can emit. The operator decided the design's eleven decisions on
2026-10-29. This record holds the whole model; TSK-1077 implements the local
authority, and the tasks named below implement the rest.

## Decision

**Authority.** The default branch, resolved to one commit, is the only
policy authority on every surface. Locally it is the remote's default
branch as cached in `refs/remotes/<remote>/HEAD`, which must name a tracking
branch of that remote; its commit is read once and the policy and project
settings come from that tree. Hosted, both policy jobs resolve the base
repository's `default_branch` to one SHA at job start, check it out, build
or install from it and run only its scripts, with base and head fetched as
data (TSK-336). No line, task record, `git.root_branch` or working copy
supplies a policy, an exception, a denial or a level. `git.root_branch`
keeps only its root-checkout role.

**Nothing is guessed (TSK-1077).**

| State | Local result |
|---|---|
| remote `HEAD` set to a tracking branch | that branch at one commit |
| remote `HEAD` unset | refuse, naming `git remote set-head <remote> -a` |
| remote `HEAD` naming a tag, a local branch, a short name or a commit | refuse, the same remedy |
| tracking namespace empty, remote has refs (or cannot be asked) | refuse, naming `git fetch <remote>` |
| tracking namespace empty, remote advertises no refs | labelled initialization; `HEAD` or the working copy judges |
| no remote | labelled local only; `HEAD` or the working copy judges |
| read, parse or object error at the authority, or a link at the policy path | refuse |
| the authority's tree proves `.codeflow/policy.json` absent | the shipped defaults, in the hooks, pre-push and `codeflow ci` |

Pre-push judges with the destination's advertised default branch. A
destination that advertises no branches is initialization only while this
clone holds no tracking refs for it; otherwise the push is refused with the
fetch route, never judged by the working copy. Telling a remote with refs
from one without is the only local step that asks the remote (`git
ls-remote`, bounded, non-interactive, never a URL that starts with `-`).
The guards may ask it on every command, so the question runs no command
any configuration names: it runs outside the repository with no global or
system configuration, at the remote's one configured URL, over the file,
http, https and ssh transports only, with plain `ssh` that reads no
configuration file, git's own upload command, a clean environment and an
empty home, so no stored HTTP credential such as a `.netrc` is sent; only
SSH keys, which prove possession without disclosing a secret, apply. A
remote that needs other credentials cannot be asked this way, and the
result is the fetch route.
After TSK-1057 lands, no remote is labelled local only when its
baseline holds no record for the repository; otherwise the baseline's
policy is enforced and `doctor` names the removed remote.

What TSK-1077 delivers: the guards and pre-commit's path checks read the
local authority above (pre-merge-commit runs no path check); pre-push judges with the default branch the
destination advertises; `codeflow ci` judges with the commit it is given
(`--policy-from`, else `--base`). The commit hooks' levels move to the
authority in TSK-1078, and the hosted jobs in TSK-336.

**Check identity.** Mode A, chosen for this repository (decision 3): a
dedicated GitHub App holding only Checks write creates the `authority:
commit standards`, `authority: secret scan` and `authority: release impact`
check runs from the authority job of the default branch's
`pull_request_target` workflow; the App key lives in the environment
`codeflow-authority`, whose deployment-branch rule admits only the default
branch, and only that job references it. Mode B, the floor that holds under
Mode A too and the only mode for a private GitHub Free repository: the
agent principal has no Workflows or Commit statuses write, the verdict jobs
`trusted: commit standards`, `trusted: secret scan` and `trusted: release
impact` are pinned to the Actions app id, registry-only runs get their own
job name, and the job's final step fails closed when another check run or
commit status of a trusted name exists or the API call fails. The head-run
`secret scan` and `release impact` jobs are removed, so the old names have
no producer (TSK-336).

**Host rules.** `codeflow remote protect --profile
solo-administrator|independent-review [--check-app <id>] [--lines
<pattern>]` writes three rulesets it owns by name and id (TSK-1080):

- A on `~DEFAULT_BRANCH`: `update` restricted to one repository role.
- B on `~DEFAULT_BRANCH`: pull request (zero approvals solo, one under
  `independent-review`), the required checks strict with every name pinned
  by `integration_id` (the App's for the `authority:` names in Mode A, the
  Actions app id otherwise), no force push, no deletion, no bypass.
- C on the line pattern: pull request at zero approvals, the `trusted:`
  names and the test verdicts pinned to the Actions app id, strict, no
  force push, no deletion, no bypass; line creation unrestricted. C is for
  a line's integrity, not secrecy (decision 6).

Every write sends the full body with an explicit `bypass_actors`; it never
writes a foreign ruleset, lowers an approval count, widens a bypass list or
removes a rule. `doctor` fails while B's or C's bypass list is non-empty.

**Agent principal (decision 1).** An isolated non-administrator principal
with Contents and Pull requests write and Actions read; no Workflows,
Commit statuses, Checks, Administration, Environments or secrets access.
Changes under `.github/workflows/` are pushed and merged by the operator.

**Report lines.** The hosted job and `doctor` report the content verdict
and the authority line separately: "judged by `<default branch>@<sha>`;
check identity: App `<id>` or Actions; host rules read from
`rules/branches/<default>`: A and B present or absent, approvals, strict,
pins; bypass actors not visible to this token". Locally, `codeflow doctor
--check policy-source` prints the authority ref and its commit, or the
labelled state. No attestation record is kept (decision 2).

**Bootstrap and recovery.** A remote advertising no refs is initialization;
creating the default branch is a creation, never an authority claim. First
adoption: the operator reads the effective rules (`doctor --check
remote-perimeter`), merges the pull request that adds the workflow under
the existing rules with a local `codeflow ci` run in its body (reduced
assurance, stated), sets up Mode A, then runs `remote protect`. Recovery,
when the required check fails on every pull request including the repair:
the administrator adds the administrator role to B's (or C's) bypass list
for the window, merges the one repair pull request bound to its head, then
runs `remote protect`, which writes the empty list back; `doctor` is red
while the window is open.

**Enablement order.** TSK-336 reaches the default branch through the line's
reviewed pull request under the old rules. From that merge the old check
names have no producer, so every pull request into the default branch and
the lines is unmergeable, fail closed, until the operator runs `remote
protect` to write A, B and C and retires the foreign rulesets that require
the old names (here 24379074 and 25435656). `remote protect` refuses B and
C before the default branch's workflow declares their names.

**Content scope (4.2.0).** Junk classes use net scope: a path in the head
tree matching a class and absent from the base tree. Secret names and the
gitleaks content scan keep history scope over every introduced commit, a
file added and later deleted included.

**Departures from issue 279's reference policy.**

| Reference | CodeFlow |
|---|---|
| a digest-pinned JSON catalogue as the engine's input | classes in code beside the shipped defaults, the binary pinned at the authority as trust anchor |
| five marker-fenced instruction lines and a mirror check | the duties ship in managed rules and skills, not that form |
| a per-repository page mirroring the data file | `doctor --check policy-source` |
| a public fixture format | the 43 cases as test rows (decision 9: neither format nor catalogue) |
| designated integration lines as exception sources | no line is a source |
| per-ref activation boundaries and history admission rows | the range rule, net scope for junk classes |
| NFKC matching | NFC plus ASCII case folding |
| a generic review-record block class | none (decision 10) |
| superseded marker at block | warn (decision 11) |

## Consequences

- One source on every surface once TSK-1078 and TSK-336 land: pre-commit,
  pre-push, the guards and `codeflow ci` agree with the hosted job. Records on a
  line can no longer refuse every command, as two disagreeing
  `integration_target` values did on 2026-10-30, because records are not
  read for policy.
- Public behaviour change for 4.0.0 (decision 5): a project that keeps
  real exceptions or levels only on a line loses them; none exist in this
  repository. Pull requests into lines are judged by the default branch's
  policy, locally and hosted. A repository whose remote `HEAD` is unset
  now refuses until `git remote set-head <remote> -a` or a fetch on git
  2.48 or later records it. The adoption change itself is judged by the
  shipped defaults.
- A fresh clone of an empty remote asks the remote on each local
  authority read until its first branch exists.

Residuals, stated here and in every report:

- Check identity: under Mode A whoever lands code on the default branch
  controls the key; under Mode B anyone with Workflows write can forge a
  required name, and GitHub's choice among same-named runs is
  undocumented.
- Public lines: a task-branch push publishes its content before any
  hosted check; secrecy rests on the pre-commit and pre-push scans and the
  hosted scan. A line's checks can be forged by a workflow writer, the
  operator alone.
- The recovery window lets every administrator merge any pull request
  and delete a line while open; the enablement window leaves every pull
  request unmergeable for one operator session.
- The App key is readable by any job running default-branch code on the
  default branch's ref.
- Exempt executables: a hostile regular file at an exempt executable path
  is admitted by design; review and the hook-integrity digest are the
  controls.
- Local authority before TSK-1057: `refs/remotes/<remote>/HEAD` and the
  tracking refs are a local cache. In session git-guard refuses
  tracking-ref writes and deletes; outside a session, or after `git remote
  remove`, nothing local binds. A URL rewrite to an empty repository with
  the tracking refs removed yields the initialization label.
- The shared credential: until decision 1's principal exists, the seats
  hold the operator's account and the host cannot see the separation.
- Self-authorization: under `solo-administrator` the administrator merges
  their own pull request at zero approvals once B passes.
- Rule editing: administrators can edit or disable A, B and C; classic
  `enforce_admins: false` is live on `main`.
- `update` semantics: that A refuses a write-access App's merge is read
  from the REST wording and proved only by the hosted fixture.
- Tests of head code run head code: `codeflow gates`, `windows` and
  `security review` stay head verdicts.
- The content scan's limits: lookalike names beyond NFC and ASCII
  folding, gitlink payloads, and a force-pushed blob still reachable at
  its old SHA.
- Merge queues are not covered until `merge_group` is wired.

## Architecture impact

None.
