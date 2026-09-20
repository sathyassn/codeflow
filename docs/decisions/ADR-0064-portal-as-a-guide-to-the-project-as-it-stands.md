---
id: ADR-0064
title: "the portal is a guide to the project as it stands"
date: 2026-09-20
status: accepted
superseded_by: null
architecture_impact: docs/architecture/utility-presentation.md page-class row and concept sentence
---

# ADR-0064: the portal is a guide to the project as it stands

## Context

The documentation portal listed every source in the repository: forty seven
tasks, fourteen epics, nine specs and sixty three decisions as pages beside the
guide, and the doctrine named records and accepted decisions as page classes
of their own. A reader looking for what CodeFlow is and how it is built met
the work tracker first. The explanatory sources were long prose, so the pages
were walls of text under a docs shell, which the composition rule already
forbids. Plan v6 tried to fix the record pages by composing them into graphs
and timelines, which made the tracker prettier without making the guide.

## Decision

The portal explains the project as it stands: what it is, what it does, how
to adopt it, how it is built and how it is operated. Every explanatory page
walks the altitude trio, Concept, Architecture and Technical, with one figure
in Concept, the structure in Architecture and the lookups in Technical, each
framed by prose. Decisions, epics, tasks and specs stay in the repository and
are pointed to from one generated pointer page that names each folder with
its purpose, its count and its repository link. The guide has no per-record
pages. The adapter keeps a records switch as a configuration affordance, off
in CodeFlow's own portal and absent from the starter default; a project that
turns it on takes the lookup form for those sources and steps outside this
guide doctrine. Pages that rely on a decision cite it by id and link to the
repository file. ADR-0048, ADR-0058 and ADR-0063 stand unchanged; SPC-010 is
withdrawn.

Correction, 2026-09-20 (TSK-051): the starter default is not free of the
switch. It ships the switch off with one pointer at `docs/decisions`, so a
scaffolded portal points at the decisions folder instead of publishing it.
The decision above stands unchanged.

## Consequences

The adapter gains a pointer page and a records switch and loses the composed
record views. The page-class gate checks the trio on every explanatory page
and the folder table on the pointer page, and fails a build whose
configuration turns records off but still emits a record route. The source
pages are authored as layered documents, which is where the guide's quality
now lives. The shared doctrine, the portal skill references and
`docs/architecture/utility-presentation.md` carry the new page classes.
