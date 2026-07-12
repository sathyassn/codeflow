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
4. Push and open the PR. Body: summary, changes, test results, linked epic and
   capability IDs. No AI attribution, no emoji.
5. Land via a PR **merged by a human** on green CI, or `codeflow integrate
   <branch> --into <target>` when there is no remote. An agent never merges into
   a protected branch — no `gh pr merge` into a protected base, no by-hand
   merge, never `gh pr merge --delete-branch`. Override envs
   (`CODEFLOW_HUMAN_OVERRIDE`, gate tokens) are human-only.
6. Confirm the landed state with `codeflow status`; report the final epic and
   capability state.
