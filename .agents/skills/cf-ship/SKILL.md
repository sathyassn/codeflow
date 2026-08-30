---
name: cf-ship
description: Land finished work — docs and capability updates, then a PR through the gates. Use when a change is reviewed and green and ready to merge.
---

# cf-ship — land finished work

1. Preconditions: the independent review verdict is `approved`; `codeflow test`
   and `codeflow validate --docs` are green. Anything missing → back to
   `cf-develop`.
2. Same-PR doc mutations (this is how docs stay true):
   - a capability entry created or updated — status, `verified_by` test tags,
     epic and ADR links (required at full tier; keep `verified_by` non-empty so
     `validate --docs` stays clean — it does not gate epic close);
   - an ADR finalized if a Tier-3 decision was made; `docs/architecture.md`
     updated when the ADR declares architecture impact;
   - a spec frozen (`status: implemented`); epic and task statuses updated.
3. Re-run `codeflow validate --docs` after the doc updates — it must pass.
   If `.codeflow/docs-portal.json` exists and this change materially affects
   authoritative docs, relationships, version context, portal configuration,
   or starter behavior, also run the adopted portal's locked check/build and
   `codeflow validate --portal <adopted-root>`; add rendered/browser checks
   matched to UX impact. Non-adopters receive no portal gate.
4. Sweep the change for touched contract surfaces — API, CLI flags, config
   schema, file formats, defaults, managed-file semantics. Each one is either
   marked breaking (`type!:` on the commit + a `BREAKING CHANGE:` footer with the
   migration path, which drives the major bump) or consciously stated
   non-breaking with the reason. The `breaking_watch_paths` warn is a backstop,
   not the judgment.
   For a multi-platform binary or installer release, keep native Windows and
   WSL2/Linux evidence separate: the native Windows installer must select its
   Windows binary, while WSL2 uses the Linux installer and binary. Cross-build
   success proves compilation and linking only; it never replaces native
   macOS/Linux/Windows tests or installer canaries. Missing platform evidence
   blocks publication rather than becoming an inferred pass.
5. Apply `cf-editorial-review` to substantial changed docs, release notes, and
   the PR narrative. It refines the writing but cannot weaken the template,
   evidence, policy, or no-emoji requirements below.
6. Open the PR. Commits stay conventional (`type(scope): description`,
   ≤ 50-char description, ≤ 72-char subject, at most 3 `-` body bullets each
   ≤ 72 chars, optional `BREAKING CHANGE:` footer); one logical change each.
   **Title** names the whole-branch outcome, not only the last commit.
   **Body** follows the template (summary, changes, testing, linked IDs).
   Tables for tabular data, fenced blocks for pasted output, one-line bullets
   otherwise; Summary is plain language a zero-context reader understands.
   Write Summary and Changes from `git log --oneline <base>..<head>` and
   `git diff --stat <base>...<head>` on source-of-truth paths — every logical
   change on the branch, not the last conversation, last review, or last
   commit. For a code change, **Testing is evidence you already ran**: paste
   `codeflow test --mode essential --strict` (use `full` when the change
   touches a full-only target); run the coverage command CI uses and paste
   the TOTAL numbers — do not defer to CI; name new tests, manual/e2e, and
   what was NOT tested. Docs-only: one line plus the doc checks. Lint with
   `codeflow ci --base <base> --head HEAD --branch <name> --pr-body-file
   <file>` before `git push` and `gh pr create`. No AI attribution, no emoji.
7. Land via a PR **merged by a human** on green CI, or `codeflow integrate
   <branch> --into <target>` when there is no remote. An agent never merges into
   a protected branch — no `gh pr merge` into a protected base, no by-hand
   merge, never `gh pr merge --delete-branch`. Override envs
   (`CODEFLOW_HUMAN_OVERRIDE`, gate tokens) are human-only.
8. Confirm the landed state with `codeflow status`; report the final epic and
   capability state.
9. Clean up after the human merge, with proof. From outside the task worktree:
   - fetch, then use `codeflow status` as the local worktree/branch inventory;
     its removable/dirty/unproven classification is evidence, not deletion or
     ownership authorization. Confirm the task owner is inactive before
     cleaning every proven-landed resource in this closeout; do not mutate
     another active owner's worktree;
   - fetch the remote and inspect `git -C <path> status --short`; if it is
     dirty or untracked, stop and preserve or harvest the work — never use
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
