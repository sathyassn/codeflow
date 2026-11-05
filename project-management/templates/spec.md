---
id: SPC-{{NNN}}
uid: {{UID}}              # hidden record identity, written once by `new`; never edit
title: {{TITLE_YAML}}
status: draft            # draft | approved | superseded; change it with `codeflow spec status`
open_questions: []       # each question still open, one string per item; approval needs this line, empty
created: {{DATE}}
---

# SPC-{{NNN}}: {{TITLE}}

<!-- Specs are optional settled work inputs, frozen once implemented, not living
     requirements. `codeflow spec new --for EPC-NNN|TSK-NNN` allocates this
     file and links it from the consuming work item. Write one only when
     interfaces, formats, or behavior need pinning down before building; many
     work items need no spec. `approved` requires an empty `open_questions`
     list. `implemented` is derived, never written: every consumer is terminal
     and at least one is complete. Until then, a change to an approved spec is
     Plan vN+1, amended in place through a reviewed planning change: each
     change of meaning gets a dated note naming its resolution (an editorial
     change needs none), and the superseded decision stays visible as
     history. That change names each consumer bound to a changed requirement
     and its disposition: unaffected, criteria amended in the same change, or
     reopened. Once implemented the spec is frozen, so a later change gets a
     new spec that lists `supersedes: [SPC-old]`, linked by `codeflow spec
     status SPC-old superseded --by SPC-new`, or an explicit superseding
     record. Keep maintained requirements and executable schemas current at
     their declared authority. -->

## Summary

<!-- What is being pinned down and why. Link the authoritative living source
     and revision when applicable; do not duplicate it here or invent an
     external_refs frontmatter field. -->

## Delta against current capability

<!-- Optional, for a brownfield change: express the spec as a delta against the
     named capability (docs/capabilities.md), so the change is traceable at the
     spec level. Drop any heading that does not apply; skip the whole section
     for greenfield work. -->

<!-- ADDED: new behavior or surface this introduces. -->

<!-- MODIFIED: existing behavior whose meaning changes (old -> new). -->

<!-- REMOVED: behavior or surface this retires. -->

## Behavior

<!-- What the thing does, observable from outside. -->

## Interfaces and formats

<!-- Signatures, schemas, file formats, CLI surfaces: only what the builder
     needs pinned down. -->

<!-- Optional, only where the project adopted the estimation time-box
     profile (`cf-estimate`); otherwise leave it out. `cf-plan` writes it
     while the spec is brought to readiness, so the review of the pinned
     commit covers it; until then it is provisional and no milestone uses
     it. A spec with a scope-changing gap open is UNSIZED and carries no
     hours. A feature gets a task mix, never one grade or band. Copy it out
     of this comment as a `## Delivery size` section here:

     ## Delivery size
     Frozen at the pin commit.
     - Epics: 1 (value line: <one sentence>); split trigger: <named, or none>
     - Task mix: 7 to 9 tasks: 1 to 2 Easy, 4 to 6 Medium, 1 to 2 Hard
     - Dependencies: <prerequisites, and which tasks wait on them>
     - Budget envelope: 13 to 40 task-hours (fewest tasks at band floors to
       most at band tops; a cross-check, never a date)
     - Serial resources: <maintainer decisions, approvals, key reviews,
       merges to the default branch>
     - H: <hard human gates>; B: <start blockers>
     - Basis: analogue epics <ids>

     With two or more epics, split the mix and the envelope by epic. -->

## Edge cases

<!-- The inputs and states most likely to be mishandled. -->

## Open questions

<!-- Context for the questions only. What approval reads is the
     `open_questions` frontmatter list: add each unresolved question there and
     remove it once it is settled, so the list is `[]` before building. -->
