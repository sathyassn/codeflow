<!-- PR bodies are linted by CI (`codeflow ci`): no AI attribution, no emoji.

     FORMAT RULES — match the presentation to the SHAPE of the data:
     - TABLES for tabular data: coverage metrics, test→what-it-pins lists,
       exit-code or before/after matrices. Never force these into sentences.
     - Fenced blocks for pasted output. Numbered lists for sequences.
     - Bullets for genuinely enumerable points; keep one coherent point per
       bullet. Do not split a necessary explanation just to meet a line count.
     - PLAIN language in Summary: a reader with zero context must understand
       it — no jargon, no internal shorthand; say what it means for the user.
     - Evidence over claims, numbers over adjectives.

     Follow the project's release policy for impact declarations and notes.
     Conventional commits — `type(scope): …` and the `!` / `BREAKING CHANGE:`
     footer — must agree with its authoritative release input. They calculate
     versions only if that project uses a commit-driven release tool; do not
     introduce a second calculator alongside fragments or another policy.

     Delete unused tables and `## Notes` when empty. Keep evidence gaps explicit;
     blank placeholders and invented numbers are not completed evidence. -->

## Summary

<!-- Context only: one to three short sentences on what this is and why, in
     words someone who has never seen this repo understands. Then every
     detail as bullets, one point each, in a logical order. cf-ship's PR
     evidence reference owns this shape. Derive from
     `git log --oneline <base>..<head>` and `git diff --stat <base>...<head>`.
     Cover every logical change on the branch. Do not write from the last
     conversation turn, last review round, or latest commit subject. Inspect
     the full diff too; refresh the title/body after substantive branch updates. -->

## Changes

<!-- One SHORT line per logical change. If the list is long, the PR is
     probably too big — consider splitting it. -->

-

## Testing

<!-- REQUIRED for any code change — a code PR without real test evidence is
     not reviewable. Paste actual output; put tabular data in the tables.

     Identify the tested revision and exact commands. Attribute prior or CI
     evidence to its revision and scope; do not imply it covers later changes.
     Missing required checks keep the PR draft.

     Docs-only PR? State "Docs-only — no code paths changed" plus doc checks
     and relevant instruction evaluations actually run. Scripts, hook settings,
     generated runtime assets, and executable examples require behavioral checks
     even when stored under docs. Shipped templates and agent instructions also
     need behavioral evidence even when written in Markdown. Added eval cases
     are not completed trials. -->

- Results:

```text
(paste the real test summary output here)
```

- Coverage (measured TOTAL from the project's command; name revision, command, metric, and scope; CI PASS alone is insufficient):

| Metric and scope | Measured result | Required floor | Evidence revision / command |
|---|---|---|---|
| Line coverage |  |  |  |

<!-- Unsupported coverage: replace with N/A and the technical reason. Unrun,
     stale, or unavailable required coverage is a gap, not N/A or a pass.
     Never present focused-target coverage as whole-project coverage. -->

- New / changed tests:

| Test | What it pins |
|---|---|
|  |  |

- Manual / e2e verification:

| What was run | Observed result |
|---|---|
|  |  |

- Whole-flow evidence:

<!-- For every materially changed user or operator journey (including CLI,
     install/update, hooks, and harness delegation), show the affected
     boundaries exercised together. Name controlled doubles and any boundary not
     exercised. If no journey changed, replace the table with one reasoned
     `N/A — ...` line. -->

| Changed journey | Boundaries exercised | Exact run and observed result | Controlled or unverified boundaries |
|---|---|---|---|
|  |  |  |  |

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
