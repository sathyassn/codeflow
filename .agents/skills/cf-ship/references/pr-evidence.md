# PR narrative and verification evidence

Read when preparing or updating a PR, including an integration-to-main PR,
and when following a PR after it opens.

Open the PR. Commits stay conventional (`type(scope): description`,
≤ 50-char description, ≤ 72-char subject, at most 3 `-` body bullets each
≤ 72 chars, optional `BREAKING CHANGE:` footer); one logical change each.
**Title** names the whole-branch outcome, not only the last commit.
**Body** follows the template (summary, changes, testing, linked IDs).
Tables for tabular data, fenced blocks for pasted output, and bullets for
enumerable points; do not split necessary explanations to meet a line count.

**Summary** gives context only: what the PR is and why it exists, in plain
words a reader with no context understands.

- Write it as one to three short sentences.
- Keep every detail out of it: no mechanism, file name, identifier, number,
  rule list, or caveat.
- Put the details after it as bullets, one point each, in a logical order:
  problem, change, effect, limits, or the order of the flow.
- Judge it by what it carries. A short summary that already holds the
  details fails.

Assess the complete change under the project's adopted release policy; load
[release-policy.md](release-policy.md) for impact, authority and publication
boundaries. Carry its required release-impact explanation or justified `none`,
with evidence and migration when needed. Reconcile the authoritative commits
or change entries that will land, not only the PR title; do not add a competing
version calculator or release ledger.
Write Summary and Changes from `git log --oneline <base>..<head>` and
`git diff --stat <base>...<head>` on source-of-truth paths — every logical
change on the branch, not the last conversation, last review, or last
commit. Inspect the actual diff as well: filenames and commit subjects
alone cannot establish behavior, risk, or completeness. For a code change,
**Testing is evidence you already ran**: identify the tested revision and
commands, paste their real summaries, and state their scope. Use
`codeflow test --mode essential --strict` (`full` when the change touches a
full-only target). Report measured coverage TOTALs, metric, scope, and
governing floor from the project's coverage command, locally or from a
completed attributable CI run; a job's `PASS` is not a coverage number.
Do not relabel subset coverage as workspace coverage. Unsupported coverage
is `N/A` with a technical reason; unavailable or stale evidence is a gap,
never zero, an invented percentage, or an inferred pass. Missing required
evidence keeps the PR draft. Name new tests and what was NOT tested.
Whole-flow evidence includes changed operator/CLI journeys, even without
product UI. Docs-only means no executable behavior changed: scripts, hook
settings, generated runtime assets, and executable examples do not qualify
merely because they live under docs. Instruction-only changes name the doc
checks and relevant behavioral evaluations; distinguish added cases from
live trials actually run. Remove unused template tables or replace them
with a reasoned N/A. After a rebase or substantive update, refresh the whole
PR narrative and affected evidence before marking ready; preserve a prior
review only with a reasoned unchanged-scope link. Lint with
`codeflow ci --base <base> --head HEAD --branch <name> --pr-body-file
<file>` before `git push` and `gh pr create`. No AI attribution, no emoji.

## After opening

Follow the PR until its required checks finish or the budget ends. The
required checks are the project's gates as the skill defines them (test,
validation, coverage and security), whether or not the host marks them
required. Run `gh pr checks <url> --required` at most once a minute, for at
most thirty minutes. Exit code 8 means checks are still pending.

Remote protection may mark no check required. Then `--required` fails with
`no required checks reported`, and an empty list is not readiness. Read all
check runs with `gh pr checks <url>` on the same cadence and match each
project gate to the run that carries it. A gate with no matching run, or
whose run you cannot read, is missing evidence.

Do not use `--watch` without a ceiling, and do not poll without an end.
`--watch` refreshes every ten seconds by default and has no timeout of its
own. Run `timeout 30m gh pr checks <url> --watch --interval 60`, or poll by
hand.

Classify each red or stuck check with the quality contract's redness classes:

- **Assertion-red, caused by this change.** Return it to its owner (skill
  step 1), fix it, run the check locally where you can, push, and restart the
  poll. Do not wait for the operator to name the job. A fix that changes the
  accepted contract goes back to `cf-plan`.
- **Red, and you cannot run it locally.** Read its log with
  `gh run view <run-id> --log-failed`. Fix it if the cause is in the change;
  otherwise report it as red with the failing line. It still blocks the PR.
- **Infrastructure-incomplete.** The job was queued past the budget,
  cancelled, lost its runner, or never started, for example because GitHub
  Actions was refused for billing or a spending limit. Report it as missing
  evidence with the reason the tool gave, not as a product defect. A
  completed green run of the same check still counts, as the quality contract
  says. Do not rerun the same job without a new reason.

Never merge. When every required check is green, or the thirty minutes end,
send one readiness report without being asked. It gives the PR URL exactly as
`gh pr create` or `gh pr view --json url` printed it, never one built from a
number or guessed. It lists each required check with its state, any missing
evidence with its reason, and the next action. For a green PR the next action
is a human merge.
