# Portal page shape example (utility presentation system)

Use this as **how to think about a durable explanatory page**, not content to
copy. The source of truth remains the repository Markdown and records named in
`portal.config.json`; the portal is derived. The subject here is a review
surface, `codeflow present`, explained at three altitudes.

Use the literal depth-2 headings `## Concept`, `## Architecture` and
`## Technical`: the adapter detects the trio and renders real altitude tabs,
one layer visible at a time, selectable by keyboard and URL hash.

**Thinking:** what must be true after a glance at this layer? Put that in a
figure first, in the family whose relationship the reader must see
(`figure-grammar.md` section 3). Prose frames it: one lead sentence above, the
acting sentences below. If removing the figure leaves only essays, the page is
not done.

Every figure is a declaration file bound in configuration. The source carries
no marker; the adapter draws each figure at the head of its panel:

```json
{
  "figures": [
    { "declaration": "docs/figures/present-boundary.json", "route": "system/present", "panel": "concept" },
    { "declaration": "docs/figures/present-revisions.json", "route": "system/present", "panel": "architecture" },
    { "declaration": "docs/figures/present-limits.json", "route": "system/present", "panel": "technical" }
  ]
}
```

---

## Concept

<!-- what it is, who it is for, what it is not -->

**Lead (one sentence above the figure):** A present session is one review
surface for one agent's subject, and it stops at the review.

**Figure, structure family** (`docs/figures/present-boundary.json`):

```json
{
  "family": "structure",
  "question": "What does a present session hold, and what stays outside it?",
  "idea": "A session holds one document and its feedback; product UI and durable docs stay outside.",
  "title": "What a present session holds and what it leaves out",
  "caption": "One session holds one document and the feedback anchored to it, and nothing in it is durable documentation.",
  "states": [
    { "name": "owned", "mark": "state", "means": "Held by the session" },
    { "name": "outside", "mark": "denied", "means": "Outside the session" }
  ],
  "facts": [{
    "claim": "the utility is never product UI or durable documentation",
    "source": "AGENTS.md#entry-points",
    "derive": "the /cf-present row",
    "check": { "kind": "contains", "text": "never treat the utility as product UI or durable documentation" },
    "value": true
  }]
}
```

The declaration also carries `id`, `binding`, `wide`, `narrow` and `twin`
(`figure-grammar.md` section 6). Below the figure, two to four plain
sentences say who opens a session and when to stay in chat.

---

## Architecture

<!-- how the parts relate and where the boundaries are -->

**Lead:** Each update adds a revision, and feedback stays anchored to the
revision it was given on.

**Figure, derivation family** (`docs/figures/present-revisions.json`):

```json
{
  "family": "derivation",
  "question": "Where does each revision come from, and where does feedback go?",
  "idea": "Each revision derives from the agent's document; feedback returns to the agent, never into the source.",
  "title": "A present revision and the feedback that returns from it",
  "caption": "The agent's document becomes an immutable revision, and anchored feedback returns to the agent.",
  "states": [
    { "name": "source", "mark": "state", "means": "Agent-owned input" },
    { "name": "derives", "mark": "trans", "means": "Derives" },
    { "name": "returns", "mark": "return", "means": "Feedback returns" }
  ],
  "facts": [{
    "claim": "a revision changes document content only",
    "source": ".claude/skills/cf-present/references/document-authoring.md#feedback-and-revision-integrity",
    "derive": "the revision sentence",
    "check": { "kind": "contains", "text": "Revision updates change document content only." },
    "value": true
  }]
}
```

The adapter re-derives each fact from the section it names at build time and
fails the page if a drawn value is wrong.

Below the figure: the acting sentences, with bullets where they scan better.

- A revision is immutable; an update adds one.
- Feedback binds to a block, a revision and an exact quote or area.

---

## Technical

<!-- what exactly holds, and how far -->

**Lead:** Every document stays inside fixed limits that the runtime enforces
before rendering.

**Figure, extent family** (`docs/figures/present-limits.json`), one bar per
limit, each value a fact checked against the source that sets it:

```json
{
  "family": "extent",
  "question": "How large can one present document grow?",
  "idea": "Blocks, drawn blocks and notes each stop at a fixed ceiling.",
  "title": "The limits one present document stays within",
  "caption": "The runtime refuses a document past any of these ceilings before it renders.",
  "states": [{ "name": "used", "mark": "used", "means": "Ceiling" }],
  "facts": [
    { "claim": "at most 512 blocks", "source": "crates/codeflow-present/src/limits.rs", "derive": "MAX_BLOCKS",
      "check": { "kind": "contains", "text": "MAX_BLOCKS: usize = 512;" }, "value": true },
    { "claim": "at most 24 figures and diagrams", "source": "crates/codeflow-present/src/limits.rs", "derive": "MAX_DIAGRAM_BLOCKS",
      "check": { "kind": "contains", "text": "MAX_DIAGRAM_BLOCKS: usize = 24;" }, "value": true },
    { "claim": "at most 100 feedback notes", "source": "crates/codeflow-present/src/limits.rs", "derive": "MAX_FEEDBACK_NOTES",
      "check": { "kind": "contains", "text": "MAX_FEEDBACK_NOTES: usize = 100;" }, "value": true }
  ]
}
```

The table is the lookup and stands beside the figure, never in its place:

| Limit | Value | Set in |
|-------|-------|--------|
| Blocks per document | 512 | `crates/codeflow-present/src/limits.rs` |
| Drawn blocks (figures and diagrams) | 24 | `crates/codeflow-present/src/limits.rs` |
| Feedback notes per review | 100 | `crates/codeflow-present/src/limits.rs` |

---

## A source the guide must not edit

An instruction an agent loads, such as `AGENTS.md`, cannot take the trio. It
is declared illustrated and keeps its bytes. It needs at least one companion
figure at the page head, and may bind more to its headings:

```json
{
  "page_classes": [
    { "source": "AGENTS.md", "class": "illustrated" },
    { "prefix": "docs/decisions", "class": "pass-through", "reason": "accepted-record" }
  ],
  "figures": [
    { "declaration": "docs/figures/agent-contract.json", "route": "reference/agents" },
    { "declaration": "docs/figures/change-states.json", "route": "reference/agents", "anchor": "git-rules" }
  ]
}
```

The page says under each companion that it was declared outside the source.
An accepted decision renders as it is, with no figure.

---

## Fail closed

- Do not publish a Concept panel that is only a teaser for Architecture.
- Do not let a table stand in for a figure, or a `cf-stage` for anything but
  the flow interim.
- Do not assert portal-only facts or decisions absent from the repository
  sources. Framing text around a figure is required, not invented authority.
- Do not drop product brand packs into Starlight to make it pretty.
- Do not add present Comment chrome to the portal.
