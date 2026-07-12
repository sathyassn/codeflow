<!-- PR bodies are linted by CI (`codeflow ci`): no AI attribution, no emoji.

     FORMAT RULES (they matter as much as the content):
     - Every section is BULLETS. One point per bullet, ONE line where possible.
     - No paragraph-bullets: a bullet that wraps past ~2 lines is two bullets.
     - Inline code sparingly — a reader scans this; heavy `code` mid-sentence
       makes a wall. Fenced blocks are for pasted output, not prose.
     - Evidence over claims, numbers over adjectives.

     Type and breaking-change are NOT re-declared here. They come from your
     conventional commits — `type(scope): …` and the `!` / `BREAKING CHANGE:`
     footer — which drive the version bump and the CHANGELOG.

     Delete the `## Notes` section if you have nothing for it. -->

## Summary

<!-- 2–4 bullets: what this PR does, and why now. Not a paragraph. -->

-

## Changes

<!-- One SHORT line per logical change. If the list is long, the PR is
     probably too big — consider splitting it. -->

-

## Testing

<!-- REQUIRED for any code change — a code PR without real test evidence is
     not reviewable. Paste actual output, don't summarize:

     - Results: the real summary line(s) — `X passed; 0 failed` per suite or
       workspace total — in a fenced block.
     - Coverage: the line-coverage % (and the delta vs main when you have it);
       CI's coverage job computes it — paste the number, don't just link.
     - New tests: name each new/changed test → what behavior it pins.
     - Manual / e2e: the command(s) you ran and what you observed.
     - State plainly what was NOT tested.

     Docs-only PR? Replace the bullets with one line: "Docs-only — no code
     paths changed", plus the doc checks you ran (`codeflow validate --docs`). -->

- Results:

```text
(paste the real test summary output here)
```

- Coverage:
- New tests:
- Manual / e2e:
- Not tested:

## Linked work

<!-- The IDs this PR serves: capability (CAP-###) and epic (EPC-###), plus
     any ADR whose decision shipped here. A behavior change links at least
     one — it is how the traceability spine stays intact. -->

- CAP-
- EPC-

## Notes

<!-- Optional. What the sections above don't carry: breaking-change callout
     and migration, risk and rollback, screenshots for UI changes, follow-ups
     deliberately left out. -->
