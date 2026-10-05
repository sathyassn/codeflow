# PR narrative and verification evidence

Read when preparing or updating a PR, including an integration-to-main PR,
and when following a PR after it opens. Commits follow the git rules' commit
format, one logical change each. The **title** names the whole-branch
outcome, not only the last commit.

## Body format

This reference owns the pull request body format; the template carries the
same shape in short comments. The sections a body needs follow the change
class `codeflow ci` reads from the whole target-to-head range: a code range
carries every required section below; a docs-only range (documentation files
only) leaves out Testing; a light range (only Markdown under `docs/` or
`project-management/`, outside every contract surface) needs only Summary
and Changes. The rest appear only when their condition holds.

Every PR names its work on a `Task:` line: `TSK-NNN` or `EPC-NNN` where
durable tracking is active (judged from the tracking state, not the
installed tier), and a non-empty unit name where it is not. It names its
task, or its epic for the breakdown PR and the PR to main. A missing, empty,
malformed, repeated or mismatched `Task:` line is refused.

| Section | When | Content |
|---|---|---|
| Summary | always | a few lines that anchor the reader (rule below) |
| Changes | always | one bullet per logical change, most important first; numbered only for a sequence; one line per task for an epic |
| Testing | unless the range is docs-only or light; evidence required when the range is code | tested revision and command, fenced summary lines, `Coverage:` when the range is code, `New tests:`, `Not tested:` (rules below) |
| Reviews | unless the range is light | one row per current review: reviewer, scope, verdict; `None: reason` when unreviewed |
| Release impact | on a PR into a protected branch or whose range carries a breaking commit; checked whenever present | `Impact`, `Breaking`, `Rationale`, `Migration`, then the project's own fields (rules below) |
| Screenshots | after Changes, when a rendered surface changed | the changed surface |
| Tests | after Testing, when an added or renamed test's name does not state what it pins | test and what it pins, about six rows; otherwise one `New tests:` line |
| Whole-flow evidence | after Testing, when a CLI command's behavior, flags or output; install, update or scaffold; a hook or guard; an automation handoff; or a rendered UI changed | one bullet per journey: what ran, what was observed, what was not exercised |
| Breaking change | after Release impact, when Breaking is yes and the migration needs more than one line | what breaks and the migration steps |
| Risk and follow-up | after Release impact, when Impact is the breaking level, a watched contract path changed, a hook, guard, secret scan, sandbox or permission surface changed, or landing needs a human step | what can go wrong, how to back out, steps after merge |
| Links | last, when it serves tracked work, a decision or durable evidence | the IDs, any FB ids of feedback it closes, and the record with the detail; omitted, not `N/A`, if none |

- Keep the Testing heading the project's policy requires (`Testing` by
  default); a renamed heading fails an unchanged policy. A conditional
  section is a `##` sibling after its parent, never a `###` subsection:
  release parsers read a section up to the next `##` heading.
- A PR type never cancels a condition. Read the conditions from the whole
  target-to-head diff; a mixed or epic PR takes the union.
- `codeflow ci` warns on unclosed HTML and on a Testing section with no
  `Not tested:` line. Keep the body short by linking records instead of
  copying them; never drop evidence to shorten it.
- `codeflow ci` also warns, and never blocks, when the body passes 1,000
  words as a reader sees it: HTML comments are left out, fenced blocks and
  tables count. The warning names the count and the three largest `##`
  sections. A body grows when each review round is appended. Write it to its
  final state instead: replace it on each update, link records instead of
  copying them, keep one results block at the head and one review row per
  reviewer.
- Reviews rows name the reviewer with the model that produced the verdict,
  the scope and the verdict, nothing more; the verdict and its native
  provenance live on the PR and findings live in the linked record. A review
  row is the provenance the quality contract requires, not the AI
  attribution the commit and PR checks block.
- Tables carry tabular data and fenced blocks carry pasted output. A figure
  is a fenced ASCII block, a full page is a linked `cf-present` page, and
  Mermaid is never used.

**Summary** anchors a zero-context reader in a few lines of plain language:
the result the change gives its consumer, why it matters and where it
stands. It is judgment, not a sentence count or a list of banned items. A
key file name or number belongs there when it is part of that context. The
details follow as bullets, one point each, in a logical order (problem,
change, effect, limits, or the order of the flow); a summary that buries the
anchor in detail fails, however short it is. The NEED YOUR ATTENTION heading
of operator replies never appears in a PR body.

`codeflow ci` checks the Summary's shape (`git.pr_summary`): one prose
paragraph, then a list or table, then at most one closing paragraph. HTML
comments count for nothing; whether the lead anchors stays with review.

## Release impact and evidence

Assess the complete change under the project's adopted release policy
wherever it ships behavior, and record the result in the project's release
input. The Release impact section is required on a PR into a protected
branch or whose range carries a breaking commit, and is checked whenever it
is present. Its four fields are defined here once; the release policy points
here:

- `Impact`: the level a consumer sees. In stable SemVer, major is an
  incompatible change to an accepted contract, minor is compatible added
  behavior, patch is a compatible fix or clarification, and `none` is no
  shipped impact under the project's policy, with a reason. Other schemes
  follow the project's rules.
- `Breaking`: `yes` or `no`; in stable SemVer yes exactly when Impact is
  major. Never prefill it on a watched contract path.
- `Rationale`: the consumer-visible effect and the evidence for the level.
- `Migration`: always present; normally `none` when nonbreaking, otherwise
  steps or a pointer to Breaking change. A nonbreaking PR that refines or
  reconciles a pending breaking entry, such as a wording-only edit declared
  `none`, still carries that entry's migration reference; a checker that
  assesses edits at the entry's impact, as CodeFlow's does, also requires
  the break to be declared.
- Each value is chosen, never left as the template's alternatives. Pre-1.0
  and other schemes name the level a break takes in their adopted policy.

Declare what this PR's own entries add, not the cumulative pending version:
an additive task declares minor and Breaking no even when earlier work
already made the pending release major.
Read [release-policy.md](release-policy.md) when the impact may be minor or
major or is disputed, when the PR carries version or release-note updates,
when the project has no adopted release process, and before publication.
Reconcile the authoritative commits or change entries that will land, not
only the PR title; do not add a competing version calculator or release
ledger.

Write Summary and Changes from `git log --oneline <base>..<head>` and
`git diff --stat <base>...<head>` on source-of-truth paths: every logical
change on the branch, not the last conversation, last review, or last
commit. Inspect the actual diff as well: filenames and commit subjects
alone cannot establish behavior, risk, or completeness. For a code change,
**Testing is evidence you already ran**: identify the tested revision and
commands, paste their real summaries, and state their scope. A builder
pastes its targeted tests and its `codeflow test --mode quick` run, each
with revision and command; the pre-push hook's quick run counts when it
covered the same tree. The full gate runs once on the landing candidate:
the primary links it in each member's PR when the batch lands, and a
standalone PR runs it as its own candidate. Cite a gate run by its run id
and revision from its durable home (the CodeFlow home's `gate-runs`
directory, outside any worktree), so the citation still resolves after the
worktree and its `target/` are removed. Report measured coverage TOTALs,
metric, scope, and governing floor from the run that measured them, locally
or from a completed attributable CI run; a job's `PASS` is not a coverage
number. Do not relabel subset coverage as workspace coverage. Unsupported
coverage is `N/A` with a technical reason; unavailable or stale evidence is
a gap, never zero, an invented percentage, or an inferred pass. Missing
required evidence keeps the PR draft. Name new tests and what was
NOT tested.
Whole-flow evidence includes changed operator/CLI journeys, even without
product UI. Docs-only means no executable behavior changed: scripts, hook
settings, generated runtime assets, and executable examples do not qualify
merely because they live under docs. Instruction-only changes name the doc
checks and relevant behavioral evaluations; distinguish added cases from
live trials actually run. Delete unused template lines, and omit a
conditional section whose condition does not hold. After a rebase or
substantive update, refresh the whole PR narrative and affected evidence
before marking ready; preserve a prior review only with a reasoned
unchanged-scope link. Lint with
`codeflow ci --base <base> --head HEAD --branch <name> --pr-body-file
<file>` before `git push` and `gh pr create`. No AI attribution, no emoji.

## After opening

No polling by default. The PR already carries its cited evidence; the
primary reads the hosted results once when it assembles the batch, and a
standalone PR's author reads them once before asking for the merge. The
required checks are the project's gates as the skill defines them (test,
validation, coverage and security), whether or not the host marks them
required.

When a required check is red or stuck, or the adopted policy requires
hosted checks green before landing, follow [PR checks](pr-checks.md): the
bounded wait and the redness classes (assertion-red, red you cannot run
locally, infrastructure-incomplete).

A PR reported ready goes back to draft (`gh pr ready <n> --undo`) before
any further change to its branch, and is reported ready again only once the
new head has its review and checks.

When the evidence is complete, send one readiness report without being
asked. It opens with the result the change gives its consumer and where it
stands. It gives the PR URL exactly as `gh pr create` or
`gh pr view --json url` printed it, never one built from a number or
guessed. It lists each required check with its state, any missing evidence
with its reason, every review finding in the record the quality contract's
completion gate defines, and the next action. Who merges a green PR depends on its
target, as `cf-ship` step 8 says: the primary merges into an `integration/`
branch no protected-branch policy covers and says so in the report; for
every protected target, including a protected `integration/` glob, the next
action is a human merge.

Say "ready for your merge on local evidence" only when every owed required
check has a completed green result of the same check at the PR head, locally
or in a completed hosted job. Paste the local full gate summary with the head
SHA, and name each hosted job that never ran with the reason the tool gave. A
required check with no completed result anywhere and no local equivalent is a
missing gate: name it as the blocker, keep the PR draft where required
evidence is missing, and continue other authorized work.
