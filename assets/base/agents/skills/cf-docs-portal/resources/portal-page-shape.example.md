# Portal page shape example (utility presentation system)

Use this as **how to think about a durable architecture page**—not content to
copy. Source of truth remains repository Markdown/ADRs named in
`portal.config.json`. Portal is derived.

**Thinking:** what must be true after a glance at this layer? Put that in
structure first. Prose supports. If removing the figure leaves only essays,
the page is not architecture yet.

---

## Concept (complete for orientation)

**Title (display role):** Two lineages meet only at settle

**Lead (one short prose block):** Independent explore lanes share no context
edge; convergence is structural, not narrative.

**Primary carrier:** full-width stage or architecture figure (nodes + named
edges). If the figure is removed, the page has failed.

---

## Architecture (complete for engineers)

```text
  CLAUDE EXPLORE ──┐
                   ├──► SETTLE ──► CROSS REVIEW ──► GATES ──► PR
  CODEX EXPLORE  ──┘
       │
       └── no shared-context edge between explore lanes
```

- Labeled stages, not caption chips of the same paragraph.
- State that matters (e.g. offline lineage) changes structure or labels.
- Margins and full width for the stage; nav chrome stays portal shell.

---

## Technical (evidence altitude)

| Claim | State | Provenance |
|-------|--------|------------|
| Workspace tests | pass | task evidence id |
| Cross-lineage review | pass | PR / task notes |
| Native Windows canary | not_run | release qualification owns |

Code / diff / record tables here—not another wall of cards restating the stage.

---

## Fail closed

- Do not publish a concept layer that is only a teaser for architecture.
- Do not invent portal-only prose that is not in the repository sources.
- Do not drop product brand packs into Starlight “to make it pretty.”
- Do not add present Comment chrome to the portal.
