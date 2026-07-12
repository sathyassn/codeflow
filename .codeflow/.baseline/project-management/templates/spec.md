---
id: SPC-{{NNN}}
format_id: SPC-{{NNN}}   # SPC-NNN — must match the filename
epic_id: EPC-{{NNN}}
title: {{TITLE}}
status: draft            # draft | implemented — set implemented (frozen) when the epic ships
created: {{DATE}}
---

# SPC-{{NNN}} — {{TITLE}}

<!-- Specs are hand-authored input docs, not living documents and not managed
     by any CLI tooling — no allocator (pick SPC-{{NNN}} by hand to match this
     file's epic), no model, no `validate` support. Write one only when
     interfaces, formats, or behavior need pinning down before building —
     many epics need no spec. Frozen at ship: after that, truth lives in
     architecture + capabilities + tests, findable via `codeflow recall`.
     Never update a frozen spec to match later reality. -->

## Summary

<!-- What is being pinned down, and for which epic. -->

## Delta against current capability

<!-- Optional, for a brownfield change: express the spec as a delta against the
     named capability (docs/capabilities.md), so the change is traceable at the
     spec level. Drop any heading that does not apply; skip the whole section
     for greenfield work. -->

<!-- ADDED — new behavior or surface this introduces. -->

<!-- MODIFIED — existing behavior whose meaning changes (old -> new). -->

<!-- REMOVED — behavior or surface this retires. -->

## Behavior

<!-- What the thing does, observable from outside. -->

## Interfaces and formats

<!-- Signatures, schemas, file formats, CLI surfaces — only what the builder
     needs pinned down. -->

## Edge cases

<!-- The inputs and states most likely to be mishandled. -->

## Open questions

<!-- Must be empty before building starts. -->
