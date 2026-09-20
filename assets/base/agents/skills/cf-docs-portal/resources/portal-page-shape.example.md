# Portal page shape example (utility presentation system)

Use this as **how to think about a durable architecture page**, not content to
copy. Source of truth remains repository Markdown/ADRs named in
`portal.config.json`. Portal is derived.

Use the literal depth-2 headings `## Concept`, `## Architecture`,
`## Technical`: the adapter detects that trio and renders real altitude tabs:
exactly one layer visible at a time, selectable by keyboard and URL hash.

**Thinking:** what must be true after a glance at this layer? Put that in
structure first. Prose supports. If removing the figure leaves only essays,
the page is not architecture yet.

---

## Concept

<!-- complete for orientation -->

**Title (display role):** Two lineages meet only at settle

**Lead (one short prose block):** Independent explore lanes share no context
edge; convergence is structural, not narrative.

**Primary carrier:** full-width stage or architecture figure (nodes + named
edges). If the figure is removed, the page has failed.

---

## Architecture

<!-- complete for engineers -->

```text
  CLAUDE EXPLORE ──┐
                   ├──► SETTLE ──► CROSS REVIEW ──► GATES ──► PR
  CODEX EXPLORE  ──┘
       │
       └── no shared-context edge between explore lanes
```

Prefer a `cf-stage` fence for a labeled flow; the adapter renders it with
utility tokens:

```cf-stage
CLAUDE EXPLORE | own reading @accent
CODEX EXPLORE | own reading
->
SETTLE | rules, not preference
->
GATES | verify · seal @positive
caption: no shared-context edge between explore lanes
```

- Labeled stages, not caption chips of the same paragraph.
- State that matters (e.g. offline lineage) changes structure or labels.
- Margins and full width for the stage; nav chrome stays portal shell.
- ASCII `text` fences remain the fallback for shapes the flow grammar
  cannot express.

---

## Technical

<!-- evidence altitude -->

| Claim | State | Provenance |
|-------|--------|------------|
| Workspace tests | pass | task evidence id |
| Cross-lineage review | pass | PR / task notes |
| Native Windows canary | not_run | release qualification owns |

Code / diff / record tables here, not another wall of cards restating the stage.

---

## A second page, same grammar

The trio belongs on **every** architecture-shaped source, never one hero
page. A subsystem page repeats the shape at its own altitude:

- **Concept:** one claim plus a `cf-stage` of the subsystem's own flow.
- **Architecture:** the subsystem's labeled structure.
- **Technical:** its evidence table.

Verification exercises every tabbed page and fails an architecture-shaped
layer that has none.

---

## Fail closed

- Do not publish a concept layer that is only a teaser for architecture.
- Do not assert portal-only facts or decisions absent from the repository
  sources. Supporting text that frames a stage, table, or evidence block is
  required, not invented authority.
- Do not drop product brand packs into Starlight “to make it pretty.”
- Do not add present Comment chrome to the portal.
