<!-- PR bodies are linted by CI (`codeflow ci`): no AI attribution, no emoji.

     FORMAT RULES — match the presentation to the SHAPE of the data:
     - TABLES for tabular data: coverage metrics, test→what-it-pins lists,
       exit-code or before/after matrices. Never force these into sentences.
     - Fenced blocks for pasted output. Numbered lists for sequences.
     - Bullets only for genuinely enumerable points — one point per bullet,
       ONE line where possible; a bullet wrapping past ~2 lines is two bullets.
     - PLAIN language in Summary: a reader with zero context must understand
       it — no jargon, no internal shorthand; say what it means for the user.
     - Evidence over claims, numbers over adjectives.

     Type and breaking-change are NOT re-declared here. They come from your
     conventional commits — `type(scope): …` and the `!` / `BREAKING CHANGE:`
     footer — which drive the version bump and the CHANGELOG.

     Delete the `## Notes` section if you have nothing for it. -->

## Summary

<!-- 2–4 bullets, plain words: what this does and why now. The test: someone
     who has never seen this repo understands every bullet. -->

-

## Changes

<!-- One SHORT line per logical change. If the list is long, the PR is
     probably too big — consider splitting it. -->

-

## Testing

<!-- REQUIRED for any code change — a code PR without real test evidence is
     not reviewable. Paste actual output; put tabular data in the tables.

     Docs-only PR? Replace this section's content with one line — "Docs-only —
     no code paths changed" — plus the doc checks you ran. -->

- Results:

```text
(paste the real test summary output here)
```

- Coverage (from the CI coverage job — paste numbers, don't link):

| Metric | This PR | Floor / main |
|---|---|---|
| Line coverage |  |  |

- New / changed tests:

| Test | What it pins |
|---|---|
|  |  |

- Manual / e2e verification:

| What was run | Observed result |
|---|---|
|  |  |

- Not tested: <!-- plainly state the gaps -->

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
