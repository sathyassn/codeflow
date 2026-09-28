---
name: cf-ship
description: Land finished work (docs and capability updates, then a PR through the gates). Use when a change is reviewed and green and ready to merge.
---

# cf-ship: land finished work

1. Preconditions: the applicable independent review verdict is `approved` and
   every mandatory project, CodeFlow, CI, and adopted-policy gate is green.
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
   - a capability entry created or updated: status, `verified_by` test tags,
     epic and ADR links (required at full tier; keep `verified_by` non-empty so
     `validate --docs` stays clean; it does not gate epic close);
   - an ADR finalized if a Tier-3 decision was made; `docs/architecture.md`
     updated when the ADR declares architecture impact;
   - an approved spec transitioned to frozen (`status: implemented`) when its
     consuming work ships; already-frozen specs remain historical; epic and
     task statuses updated through their applicable change control.
3. Re-run `codeflow validate --docs` after the doc updates; it must pass.
   If `.codeflow/docs-portal.json` exists and this change materially affects
   authoritative docs, relationships, version context, portal configuration,
   or starter behavior, also run the adopted portal's locked check/build and
   `codeflow validate --portal <adopted-root>`; add rendered/browser checks
   matched to UX impact. Non-adopters receive no portal gate.
4. Assess release impact using the project's adopted policy and
   [references/release-policy.md](references/release-policy.md). Sweep API,
   CLI flags, config, formats, defaults and managed instructions for actual
   compatibility changes. A touched contract is not automatically breaking;
   a misleading commit type is not proof of compatibility. Mark an actual
   break with `type!:` and a `BREAKING CHANGE:` migration footer, and reconcile
   the project's authoritative release input and PR explanation. Use its one
   version calculator; `breaking_watch_paths` only warns. Where the project
   adopts same-PR preparation, include the warranted notes and coupled version
   updates now, reconciled with the current target and published baseline.
   A reviewed merge is not permission to publish or deploy.
   For a multi-platform binary or installer release, keep native Windows and
   WSL2/Linux evidence separate: the native Windows installer must select its
   Windows binary, while WSL2 uses the Linux installer and binary. Cross-build
   success proves compilation and linking only; it never replaces native
   macOS/Linux/Windows tests or installer canaries. Missing platform evidence
   blocks publication rather than becoming an inferred pass.
5. Apply `cf-editorial-review` and its copy guide to substantial changed docs,
   release notes, and the PR narrative. They refine the writing but cannot
   weaken the template, evidence, policy, or no-emoji requirements below.
6. Prepare the whole-branch PR using
   [references/pr-evidence.md](references/pr-evidence.md). Follow the project
   template and conventional-commit policy; attribute measured evidence to its
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
   locally or in completed CI jobs: a gate is the check, not the job name).
   Classify CI redness with the quality contract: assertion-red blocks;
   an infra-killed job that only restacks already-green checks does not. If
   the host merge UI still requires that unfinished job by name, the human
   waits, reruns, or overrides; that is merge authorization, not a failed
   test. Or `codeflow integrate <branch> --into <target>` when there is no
   remote. An agent never merges into a protected branch: no `gh pr merge`
   into a protected base, no by-hand merge, never `gh pr merge --delete-branch`.
   Override envs (`CODEFLOW_HUMAN_OVERRIDE`, gate tokens) are human-only.
9. Confirm the landed state with `codeflow status`; report the final epic and
   capability state.
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
