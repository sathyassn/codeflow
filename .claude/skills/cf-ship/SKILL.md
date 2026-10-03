---
name: cf-ship
description: Land finished work (docs and capability updates, then a PR through the gates). Use when a change is reviewed and green and ready to merge.
---

# cf-ship: land finished work

1. Before choosing a landing route, read `PLAN.md` and the landing policy
   in the project README when present, alongside the project operating
   contract. Resolve any conflict with the approved plan before changing
   its target.
   Preconditions: the applicable independent review verdict is `approved` and
   every mandatory project, CodeFlow, CI, and adopted-policy gate is green.
   After review, a task PR's last commit runs `codeflow task status <id>
   complete --acceptance <file>`; its block names the reviewed code commit
   (late: the clean landing merge's second parent, or its reviewed ancestor
   followed only by that record's status and Closeout), and CI binds it. A
   one-PR fix of a complete task reopens it (old block kept as superseded)
   and completes it again in that PR; once landed, its criteria change only
   through a separate planning PR.
   After-release criteria stay `deferred`, never verified at build time.
   When a work item is planned, started, blocked, completed or cancelled,
   follow
   [the work lifecycle](../cf-method/references/project-organization.md#the-work-lifecycle).
   Select the checks from what the change affects and the adopted policy,
   never from the PR's label: a check the project gate requires still runs,
   including for docs-only changes. Report a run as the evidence it is, not
   invented code coverage or product behavior; skip only genuinely
   inapplicable optional categories with an explicit N/A and never turn
   “not run” into pass. Return only to the failed owner: `cf-plan` for a
   materially changed contract, the responsible primary/executor via
   `cf-develop` for an implementation defect, independent review for a
   review gap, or the docs/evidence owner for documentation and PR-evidence
   gaps. Never restart the whole lifecycle or force every failure through
   development.
2. Same-PR doc mutations (this is how docs stay true):
   - a capability entry created or updated: status, `verified_by` test tags,
     epic and ADR links (required at full tier; keep `verified_by` non-empty so
     `validate --docs` stays clean; it does not gate epic close);
   - an ADR finalized if a Tier-3 decision was made; `docs/architecture.md`
     updated when the ADR declares architecture impact;
   - no spec status is written at ship: `implemented` is derived once every
     consumer is complete, and already-frozen specs remain historical; epic
     and task statuses change only as the work lifecycle states.
3. Re-run `codeflow validate --docs` after the doc updates; it must pass.
   If `.codeflow/docs-portal.json` exists and this change materially affects
   authoritative docs, relationships, version context, portal configuration,
   or starter behavior, also run the adopted portal's locked check/build and
   `codeflow validate --portal <adopted-root>`; add rendered/browser checks
   matched to UX impact. Non-adopters receive no portal gate.
4. Assess release impact under the project's adopted policy and the Release
   impact rules in `references/pr-evidence.md`. Judge compatibility as the
   git rules' breaking-change rule says; a misleading commit type is not
   proof of compatibility. Reconcile the project's authoritative release
   input and PR explanation. A reviewed merge is not permission to publish
   or deploy.
5. Apply `cf-editorial-review` and its copy guide where its description
   triggers it (by consequence). They refine the writing but cannot weaken
   the template, evidence, policy, or no-emoji requirements below.
6. Prepare the whole-branch PR using
   [references/pr-evidence.md](references/pr-evidence.md). Write the body and
   release notes plainly: simple, straightforward and clear, no mannered
   prose (see `.codeflow/rules/writing.md`), in short prose and bullets.
   Follow the project template and conventional-commit policy; attribute
   measured evidence to its revision and scope. Missing required evidence
   keeps the PR draft. Lint the body with `codeflow ci` before pushing and
   opening the PR. No AI attribution or emoji.
7. After opening, follow
   [references/pr-evidence.md](references/pr-evidence.md), "After opening":
   no polling by default. Only where the adopted policy requires hosted
   checks green before landing, wait for them with a bounded poll (once a
   minute, at most thirty minutes). Then fix assertion-red without asking,
   report infra-incomplete as missing evidence, and report readiness with
   the PR URL the tool printed.
8. Merge by target once required *checks* are evidenced green, locally or in
   completed CI jobs (a gate is the check, not the job name; the quality
   contract classifies redness). For red or unfinished CI jobs, follow
   [PR checks](references/pr-checks.md#redness-classes). The primary merges a
   green, reviewed PR into an `integration/` branch no protected-branch policy
   covers (`main`, `master`, `.codeflow/policy.json` globs) without fast
   forward or with `codeflow integrate <branch> --into <target>`, once the
   full gate is green on that exact candidate (a batch, or a standalone PR
   as its own). A protected target, including a protected `integration/`
   glob, is reported ready (on local evidence only as `pr-evidence.md`
   allows) and **merged by a human**, through the PR or
   `codeflow integrate`. An agent never merges into a protected target: no
   `gh pr merge` or by-hand merge there, never `gh pr merge --delete-branch`
   (step 10 deletes the pushed branch after proof), and override envs
   (`CODEFLOW_HUMAN_OVERRIDE`, gate tokens) are human-only.
9. Confirm the landed state with `codeflow status`; report the final epic and
   capability state. After an epic-line landing, only when the project
   configures a release branch matching its release pattern, R-120, and a
   workflow integrating into it, follow
   [release integration](references/pr-evidence.md#release-integration-after-landing).
   Otherwise skip that step.
10. Clean up after every landing, with proof, in the same step: whoever
    landed it runs cleanup, the primary after an integration-line landing
    and the session confirming a human merge into `main`, from outside the
    task worktree, as the worktree rules' "Cleanup" section sets out. The
    proof commands: `git merge-base --is-ancestor <branch> origin/<target>`
    for a normal merge; for a squash merge, require `gh pr view <n> --json
    state,headRefOid` to report `MERGED` and the branch-tip SHA, or
    `git cherry origin/<target> <branch>` to contain no unapplied `+` entry.
    Then remove the clean worktree (never use `git worktree remove --force`)
    and delete the branch with `git branch -d`, or `-D` only after the squash
    proof. Delete the pushed branch and remove the task's build output as
    those rules set out. Retain anything unproven.
