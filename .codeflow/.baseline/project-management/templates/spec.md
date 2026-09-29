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
     amended in place through a reviewed planning change, with a dated note
     for each change of meaning and the superseded decision kept as history.
     That change names each consumer bound to a changed requirement and its
     disposition: unaffected, criteria amended, or reopened. Once implemented
     the spec is frozen: a changed contract is a new spec that lists
     `supersedes: [SPC-old]`; `codeflow spec status SPC-old superseded --by
     SPC-new` records the link. Keep maintained requirements and executable
     schemas current at their declared authority. -->

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

## Edge cases

<!-- The inputs and states most likely to be mishandled. -->

## Open questions

<!-- Context for the questions only. What approval reads is the
     `open_questions` frontmatter list: add each unresolved question there and
     remove it once it is settled, so the list is `[]` before building. -->
