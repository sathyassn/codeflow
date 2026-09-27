<!-- Linted by `codeflow ci`: no AI attribution, no emoji, no em or en dash.
     The sections a PR needs follow what its range touches. A range with
     code, config, scripts, shipped templates or agent instructions needs
     all five sections below, in this order. A range of only Markdown under
     docs/ or project-management/, outside product, watched contract and
     template paths, needs Summary and Changes; Testing and Reviews are
     optional there, and a missing Release impact reads as no impact.
     Conditional sections are listed at the end. Aim for about 65 rows
     wrapped at 100 columns for a task PR, about 80 with Whole-flow
     evidence, and about 90 for an epic; never drop evidence to fit. A
     figure here is a fenced ASCII block, never Mermaid. -->

<!-- With durable work tracking: the task this delivers, or why it is a direct change. -->
Task: `TSK-NNN | none: <reason>`

## Summary

<!-- One to three short sentences of context: what this is, why, and the
     outcome, for a reader with no context. No file names, identifiers,
     numbers or caveats; details go in Changes. Cover the whole branch:
     derive from `git log --oneline <base>..<head>` and
     `git diff --stat <base>...<head>`, and inspect the full diff. Do not
     write from the last conversation turn, review round or commit. -->

## Changes

<!-- One bullet per logical change, most important first; number them only
     for a sequence. About eight for a task PR; one line per task for an
     epic. -->

-

## Testing

<!-- Evidence already run: the tested revision and command, then the gate's
     summary lines in a fenced block (about twelve lines, never a full log).
     Coverage when the range is code: the measured TOTAL from the project's
     command; name revision, command, metric, and scope; CI PASS alone is
     insufficient; unmeasured is a stated gap. New tests: count and suites.
     Missing required checks keep the PR draft. Docs or planning only: the
     section is optional; when kept, say so and name the doc checks run.
     Scripts, hook settings, shipped templates and agent instructions are
     not docs only. -->

- Revision and command:

```text
(paste the real test summary output here)
```

- Coverage:
- New tests:
- Not tested:

## Reviews

<!-- One row per current review: reviewer (human or tool), scope (commit
     range), verdict. Findings live in the linked record. A row is review
     provenance, not authorship attribution. "None: reason" if unreviewed. -->

| Reviewer | Scope | Verdict |
|---|---|---|
|  |  |  |

## Release impact

<!-- Impact is the change level a consumer sees; the project's release
     policy maps it to a version and names the level a break takes.
     Breaking states compatibility. Choose each value; never leave the
     alternatives. Migration is normally `none` for nonbreaking work; it
     names steps when Breaking is yes, and a refinement of a pending breaking
     entry keeps that entry's migration reference. Project fields go after
     Migration. -->

- Impact: `none | patch | minor | major`
- Breaking: `yes | no`
- Rationale:
- Migration: `none`, steps, or "see Breaking change"
- Unit: `codeflow`
- Evidence:

<!-- Read by `scripts/release.py` with the fields above, on every PR,
     docs and planning included: keep this section here. CodeFlow has one
     release unit, and Breaking is yes if and only if Impact is major. The
     adjacent changelog annotations are the one version input: put a
     codeflow:release-impact patch|minor|major HTML marker directly before
     each new pending changelog entry, and Impact must equal the highest one
     added. Declared impact cannot be below conventional commit markers. A
     watched contract path needs Breaking stated; a path match does not prove
     a break. When this PR removes pending content or lowers the target, add
     a Withdrawal field that says what was removed and why the remaining net
     contract permits it; do not add Withdrawal to ordinary PRs. -->

<!-- Conditional sections: add each as a `##` sibling after its parent only
     when its condition holds. A PR type never cancels a condition; a mixed
     or epic PR takes the union.

     Screenshots          after Changes: a rendered surface changed.
     Tests                after Testing: an added or renamed test's name
                          does not state what it pins. Test and Pins table,
                          about six rows.
     Whole-flow evidence  after Testing: a CLI command's behavior, flags or
                          output; install, update or scaffold; a hook or
                          guard; an automation handoff; or a rendered UI
                          changed. One bullet per journey: what ran, what
                          was observed, what was not exercised.
     Breaking change      after Release impact: Breaking is yes and the
                          migration needs more than one line.
     Risk and follow-up   after Release impact: Impact is the breaking level,
                          a watched contract path changed, a hook, guard,
                          secret scan, sandbox or permission surface changed,
                          or landing needs a human step. Bullets: what can go
                          wrong, how to back out, steps after merge.
     Links                last: the task, epic, ADR and capability IDs this
                          serves, and the task closeout or verification
                          record. Omit it when there is nothing to link. -->
