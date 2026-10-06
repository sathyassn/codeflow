<!-- Linted by `codeflow ci`: no AI attribution, no emoji, no em or en dash.
     Write it plainly: simple, straightforward and clear, no mannered prose
     (see `.codeflow/rules/writing.md`).
     The sections a PR needs follow what its range touches. A range with
     code, config, scripts, shipped templates or agent instructions needs
     the sections below, in this order. A range of only documentation files
     leaves out Testing. A range of only Markdown under
     docs/ or project-management/, outside product, watched contract and
     template paths, needs Summary and Changes; Testing and Reviews are
     optional there. Conditional sections are listed at the end.
     `codeflow ci` warns on unclosed HTML, on a Testing section with no
     `Not tested:` line and on a body over 1,000 words. Link records
     instead of copying them; never drop evidence to shorten the body. A figure here is a fenced ASCII block,
     never Mermaid. -->

<!-- Every PR names its work. Where durable tracking is active: TSK-NNN, or
     EPC-NNN for the breakdown PR and the PR to main. A planning amendment
     names every epic it changes: Task: EPC-001, EPC-002. Where it is not: the
     name of the harness's tracked unit. A missing or empty line is refused. -->
Task: `TSK-NNN | EPC-NNN | EPC-001, EPC-002 | <unit name>`

## Summary

<!-- One paragraph of plain prose to anchor a reader with no context: the
     result this gives its consumer, why it matters and where it stands.
     A key file name or number belongs here when it is part of that context.
     Then the details as `-` bullets or a table, and at most one closing
     paragraph after them; `codeflow ci` checks this shape. Other details go
     in Changes. Cover the whole branch:
     derive from `git log --oneline <base>..<head>` and
     `git diff --stat <base>...<head>`, and inspect the full diff. Do not
     write from the last conversation turn, review round or commit. Where
     `cf-editorial-review` is installed, its copy guide has the full rules;
     otherwise this comment is the whole rule. -->

## Changes

<!-- One bullet per logical change, most important first; number them only
     for a sequence; one line per task for an epic. -->

-

## Testing

<!-- Evidence already run: the tested revision and command, then the
     summary lines in a fenced block, never a full log. A task PR pastes its
     targeted tests and its quick run; the full gate on the landing
     candidate is cited by run id and revision when the batch lands, and a
     standalone PR runs it as its own candidate.
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

<!-- Required on a PR into a protected branch or whose range carries a
     breaking commit; elsewhere optional, and checked whenever present.
     Impact is the change level a consumer sees; the project's release
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
