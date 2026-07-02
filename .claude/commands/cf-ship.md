---
description: Land finished work — docs and capability updates, then PR through the gates
argument-hint: [branch or epic ID to ship]
---

Ship this work: $ARGUMENTS

1. Preconditions: `cf-reviewer` verdict is `approved`; `codeflow test` and
   `codeflow validate --docs` are green. Anything missing → back to
   `/cf-develop`.
2. Same-PR doc mutations (this is how docs stay true):
   - capability entry created or updated — status, `verified_by` test tags,
     epic and ADR links (required discipline at full tier; keep `verified_by`
     non-empty so `validate --docs` stays clean — it does not gate epic close);
   - ADR finalized if a Tier-3 decision was made; `docs/architecture.md`
     updated when the ADR declares architecture impact;
   - spec frozen (`status: implemented`); epic and task statuses updated.
3. Re-run `codeflow validate --docs` after the doc updates — it must pass.
4. Push and open the PR. Body: summary, changes, test results, linked epic and
   capability IDs. No AI attribution, no emoji.
5. Land via PR + green CI, or `codeflow integrate <branch> --into <target>`
   when there is no remote. Never merge into a protected branch by hand.
6. Confirm the landed state with `codeflow status`; report the final epic and
   capability state.
