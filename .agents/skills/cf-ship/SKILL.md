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
4. Sweep the change for touched contract surfaces — API, CLI flags, config
   schema, file formats, defaults, managed-file semantics. Each one is either
   marked breaking (`type!:` on the commit + a `BREAKING CHANGE:` footer with the
   migration path, which drives the major bump) or consciously stated
   non-breaking with the reason. The `breaking_watch_paths` warn is a backstop,
   not the judgment.
5. Push and open the PR. Every commit conforms to the standard — `type(scope):
   description` (≤ 50-char description, ≤ 72-char subject line), a body of only
   `-` bullets (at most 3, each a single line ≤ 72 chars) with an optional
   trailing `BREAKING CHANGE:` footer, one logical change each; reword or squash
   any that drifted before pushing. PR body: follow the PR template — summary,
   changes, verification (name the surface, paste evidence), linked epic and
   capability IDs. Bullets for the enumerable sections, prose only where a
   sentence earns its place. No AI attribution, no emoji.
6. Land via a PR **merged by a human** on green CI, or `codeflow integrate
   <branch> --into <target>` when there is no remote. An agent never merges into
   a protected branch — no `gh pr merge` into a protected base, no by-hand
   merge, never `gh pr merge --delete-branch`. Override envs
   (`CODEFLOW_HUMAN_OVERRIDE`, gate tokens) are human-only.
7. Confirm the landed state with `codeflow status`; report the final epic and
   capability state.
