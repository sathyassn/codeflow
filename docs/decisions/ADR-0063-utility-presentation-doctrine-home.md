---
id: ADR-0063
title: one shared utility presentation doctrine with a durable architecture home
date: 2026-09-19
status: accepted
superseded_by: null
architecture_impact: docs/architecture/utility-presentation.md — new graduated area page records the utility presentation system
---

# ADR-0063: one shared utility presentation doctrine with a durable architecture home

## Context

ADR-0053 settled the utility presentation design system and named a dated
verification note as its normative intent, while the two skills that apply it
each carried a self-described "canonical" copy of the doctrine that had
drifted from one another and from the shipped tokens. Three claimants and no
architecture page meant authors could not tell what a compliant page was, and
the portal doctrine could be read as "render the Markdown" rather than
"compose the sources".

## Decision

The normative doctrine is one shared resource file, byte-identical in
`cf-present` and `cf-docs-portal`, with profile-specific rules confined to each
skill's `references/visual-craft.md`. It states the composition rule (a utility
surface is a composed visual presentation with supporting text, never the
subject's prose re-rendered), the page-class rule, and only the carriers the
runtimes render. `docs/architecture/utility-presentation.md` is the durable
architecture home; ADR-0053 remains accepted and unedited, and its pointer to
the 2026-08-07 design-intent note is read as historical evidence, not as the
current normative text.

## Consequences

Authors and verifiers judge against one text; drift between the two skills is
a defect the scaffold parity tests catch. Unsupported carriers cannot be
promised, so a portal source that needs a tree or free diagram must use a
supported stage or a justified `text` figure until an ADR adds the carrier. The
verification notes under `docs/verification/` keep their evidentiary role but
no longer compete for authority.

## Architecture impact

`docs/architecture/utility-presentation.md` is added as the graduated area
page for the utility presentation system. `docs/architecture.md` is owned by
the orient-sources task in this epic, which adds the graduation pointer before
the epic body reaches `main`.
