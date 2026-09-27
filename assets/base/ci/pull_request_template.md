<!-- Linted by `codeflow ci`: no AI attribution, no emoji, no em or en dash.
     The five sections below are always present, in this order. Conditional
     sections are listed at the end. Aim for about 65 rows wrapped at 100
     columns for a task PR, about 80 with Whole-flow evidence, and about 90
     for an epic; never drop evidence to fit. A figure here is a fenced
     ASCII block, never Mermaid. -->

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
     Missing required checks keep the PR draft. Docs only: say so and name
     the doc checks run; scripts, hook settings, shipped templates and agent
     instructions are not docs only. -->

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
     Links                last: the issue, ticket, decision or record this
                          serves. Omit it when there is nothing to link. -->
