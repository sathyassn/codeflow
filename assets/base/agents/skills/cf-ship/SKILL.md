---
name: cf-ship
description: Land finished work — docs and capability updates, then a PR through the gates. Use when a change is reviewed and green and ready to merge.
---

# cf-ship — land finished work

1. Preconditions: the applicable independent review verdict is `approved` and
   every mandatory project, CodeFlow, CI, and adopted-policy gate is green.
   After review, a task PR's last commit runs `codeflow task status <id>
   complete --acceptance <file>`; its block names the reviewed code commit
   (late: the clean landing merge's second parent, or its reviewed ancestor
   followed only by that record's status and Closeout). A fix of a complete
   task may use one PR: reopen with a reason and keep the old block under
   `acceptance_superseded:`, fix the code, then complete again against a
   reviewed head inside the fix PR. Keep the anchored criteria unchanged;
   a criterion amendment still lands in a separate planning PR. After-release
   criteria stay `deferred`, never verified at build time. When a work item is
   planned, started, blocked, completed or cancelled, follow
   [the work lifecycle](../cf-method/references/project-organization.md#the-work-lifecycle).
   `codeflow test` and `codeflow validate --docs` remain required wherever the
   installed/project ship gate requires them, including for docs-only changes;
   report such a run as repository-gate evidence, not invented code coverage or
   product behavior. Skip only genuinely inapplicable optional categories with
   an explicit N/A and never turn “not run” into pass. Return only to the failed
   owner: `cf-plan` for a materially changed contract, the responsible primary/
   executor via `cf-develop` for an implementation defect, independent review
   for a review gap, or the docs/evidence owner for documentation and PR-evidence
   gaps. Never restart the whole lifecycle or force every failure through
   development.
2. Same-PR doc mutations (this is how docs stay true):
   - a capability entry created or updated — status, `verified_by` test tags,
     epic and ADR links (required at full tier; keep `verified_by` non-empty so
     `validate --docs` stays clean — it does not gate epic close);
   - an ADR finalized if a Tier-3 decision was made; `docs/architecture.md`
     updated when the ADR declares architecture impact;
   - no spec status is written at ship: `implemented` is derived once every
     consumer is complete, and already-frozen specs remain historical; epic
     and task statuses change only as the work lifecycle states.
3. Re-run `codeflow validate --docs` after the doc updates — it must pass.
   If `.codeflow/docs-portal.json` exists and this change materially affects
   authoritative docs, relationships, version context, portal configuration,
   or starter behavior, also run the adopted portal's locked check/build and
   `codeflow validate --portal <adopted-root>`; add rendered/browser checks
   matched to UX impact. Non-adopters receive no portal gate.
4. Assess release impact under the project's adopted policy and the Release
   impact rules in `references/pr-evidence.md`, which say when to read the
   release policy. Judge compatibility as the git rules' breaking-change rule
   says; a misleading commit type is not proof of compatibility. Reconcile
   the project's authoritative release input and PR explanation. Where the
   project adopts same-PR preparation, include the warranted notes and coupled
   version updates now, reconciled with the current target and published
   baseline.
   A reviewed merge is not permission to publish or deploy.
5. Apply `cf-editorial-review` to substantial changed docs, release notes, and
   the PR narrative. It refines the writing but cannot weaken the template,
   evidence, policy, or no-emoji requirements below.
6. Prepare the whole-branch PR using
   [references/pr-evidence.md](references/pr-evidence.md). Write the body and
   release notes plainly: simple, straightforward and clear, no mannered
   prose (see `.codeflow/rules/writing.md`), in short prose and bullets.
   Follow the project template and conventional-commit policy; attribute
   measured evidence to its
   revision and scope. Missing required evidence keeps the PR draft. Lint the
   body with `codeflow ci` before pushing and opening the PR. No AI attribution
   or emoji.
7. Follow the PR per
   [references/pr-evidence.md](references/pr-evidence.md): poll required
   checks at most once a minute for up to thirty minutes, fix assertion-red
   without asking, report infra-incomplete as missing evidence, never merge,
   and report readiness with the PR URL the tool printed.
8. Land via a PR **merged by a human** when required *checks* are evidenced
   green (the same `codeflow test` / `validate` / coverage / security targets,
   locally or in completed CI jobs — a gate is the check, not the job name).
   Classify CI redness with the quality contract: assertion-red blocks;
   an infra-killed job that only restacks already-green checks does not. If
   the host merge UI still requires that unfinished job by name, the human
   waits, reruns, or overrides — that is merge authorization, not a failed
   test. Or `codeflow integrate <branch> --into <target>` when there is no
   remote. An agent never merges into a protected branch — no `gh pr merge`
   into a protected base, no by-hand merge, never `gh pr merge --delete-branch`.
   Override envs (`CODEFLOW_HUMAN_OVERRIDE`, gate tokens) are human-only.
9. Confirm the landed state with `codeflow status`; report the final epic and
   capability state. After an epic-line landing, only when the project
   configures a release branch matching its release pattern, R-120, and a workflow
   integrating into it, follow [release integration](references/pr-evidence.md#release-integration-after-landing).
   Otherwise skip that step.
10. Clean up after the human merge, with proof. From outside the task worktree:
    - fetch, then use `codeflow status` as the local worktree/branch inventory;
      its removable/dirty/unproven classification is evidence, not deletion or
      ownership authorization. Confirm the task owner is inactive before
      cleaning every proven-landed resource in this closeout; do not mutate
      another active owner's worktree;
    - fetch the remote and inspect `git -C <path> status --short`; if it is
      dirty or untracked, stop and preserve or harvest the work; never use
      `git worktree remove --force`;
    - for a normal merge, require `git merge-base --is-ancestor <branch>
      origin/<target>`; for a squash merge, require `gh pr view <n> --json
      state,headRefOid` to report `MERGED` and the branch-tip SHA, or require
      `git cherry origin/<target> <branch>` to contain no unapplied `+` entry;
    - remove the clean worktree, then use `git branch -d` after ancestry proof
      or `git branch -D` only after the squash proof above.
    Retain anything unproven and record the owner plus the event that permits a
    later recheck. `git worktree prune` only removes stale administrative
    records; even after `--dry-run` it is not merge proof or a substitute for
    this closeout. Name resemblance, a closed PR, age, or a green check is not
    landing evidence.
