# PR narrative and verification evidence

Read when preparing or updating a PR, including an integration-to-main PR,
and when following a PR after it opens.

Open the PR. Commits stay conventional (`type(scope): description`,
≤ 50-char description, ≤ 72-char subject, at most 3 `-` body bullets each
≤ 72 chars, optional `BREAKING CHANGE:` footer); one logical change each.
**Title** names the whole-branch outcome, not only the last commit.

## Body format

This reference owns the pull request body format; the template carries the
same shape in short comments. Five sections are always present, in order,
and the rest appear only when their condition holds.

| Section | When | Content |
|---|---|---|
| Summary | always | one to three short sentences of context (rule below) |
| Changes | always | one bullet per logical change, most important first; numbered only for a sequence; one line per task for an epic |
| Testing | always; evidence required when the range is code | tested revision and command, fenced gate summary lines, `Coverage:` when the range is code, `New tests:`, `Not tested:` |
| Reviews | always | one row per current review: reviewer, scope, verdict; `None: reason` when unreviewed |
| Release impact | always | `Impact`, `Breaking`, `Rationale`, `Migration`, then the project's own fields ([release-policy.md](release-policy.md)) |
| Screenshots | after Changes, when a rendered surface changed | the changed surface |
| Tests | after Testing, when an added or renamed test's name does not state what it pins | test and what it pins, about six rows; otherwise one `New tests:` line |
| Whole-flow evidence | after Testing, when a CLI command's behavior, flags or output; install, update or scaffold; a hook or guard; an automation handoff; or a rendered UI changed | one bullet per journey: what ran, what was observed, what was not exercised |
| Breaking change | after Release impact, when Breaking is yes and the migration needs more than one line | what breaks and the migration steps |
| Risk and follow-up | after Release impact, when Impact is the breaking level, a watched contract path changed, a hook, guard, secret scan, sandbox or permission surface changed, or landing needs a human step | what can go wrong, how to back out, steps after merge |
| Links | last, when the change serves tracked work, a decision or durable evidence | the IDs and the record that carries the detail; omitted, not `N/A`, when there is nothing to link |

- Keep the Testing heading the project's policy requires (`Testing` by
  default); a renamed heading fails an unchanged policy.
- A conditional section is a `##` sibling after its parent, never a `###`
  subsection: release parsers read a section up to the next `##` heading.
- A PR type never cancels a condition. Read the conditions from the whole
  target-to-head diff; a mixed or epic PR takes the union.
- Length target, a warning and never a reason to drop evidence: about 65
  rows at 100 columns for a task PR, about 80 for a code PR that carries
  Whole-flow evidence, and about 90 for an epic into the protected branch.
  Keep prose lines under about 160 characters and a fenced block to about
  12 lines, and link records instead of copying them.
  Migration, unresolved risk and operator actions may overflow.
- Reviews rows name the reviewer, scope and verdict only. Findings and
  dispositions live in the linked record, and authorship lines belong to the
  task closeout. A review row is the provenance the quality contract
  requires, not the AI attribution the commit and PR checks block.
- Tables carry tabular data and fenced blocks carry pasted output. A figure
  is a fenced ASCII block, a full page is a linked `cf-present` page, and
  Mermaid is never used.

**Summary** gives context only: what the PR is and why it exists, in plain
words a reader with no context understands.

- Write it as one to three short sentences.
- Keep every detail out of it: no mechanism, file name, identifier, number,
  rule list, or caveat.
- Put the details after it as bullets, one point each, in a logical order:
  problem, change, effect, limits, or the order of the flow.
- Judge it by what it carries. A short summary that already holds the
  details fails.

## Release impact and evidence

Assess the complete change under the project's adopted release policy; load
[release-policy.md](release-policy.md) for impact, authority and publication
boundaries. Carry its required release-impact explanation or justified `none`,
with evidence and migration when needed. Reconcile the authoritative commits
or change entries that will land, not only the PR title; do not add a competing
version calculator or release ledger.
Write Summary and Changes from `git log --oneline <base>..<head>` and
`git diff --stat <base>...<head>` on source-of-truth paths: every logical
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
live trials actually run. Delete unused template lines, and omit a
conditional section whose condition does not hold. After a rebase or substantive update, refresh the whole
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
