# TSK-008 documentation portal prototypes

These isolated prototypes test three materially different ways to explain
CodeFlow. They are evidence for design settlement, not production portal code
or an approved visual direction.

## Run locally

From the repository root, serve the repository over HTTP:

```sh
python3 -m http.server 43188 --bind 127.0.0.1
```

Then open one of these pages:

- `http://127.0.0.1:43188/docs/verification/tsk-008-portal-prototypes/guided-path/`
- `http://127.0.0.1:43188/docs/verification/tsk-008-portal-prototypes/system-atlas/`
- `http://127.0.0.1:43188/docs/verification/tsk-008-portal-prototypes/evidence-notebook/`

The prototypes share only small test utilities for theme, search, mobile
navigation, and drill-down controls. They use no external fonts, images, or
runtime dependencies. Source links resolve against the repository served from
the root.

## Directions under test

| Direction | Primary question | Distinguishing structure |
|---|---|---|
| Guided path | Can a newcomer move from purpose to use and then implementation without losing context? | Reader journey, responsibility boundary, traceability spine, progressive technical drill-down |
| System atlas | Can an experienced contributor interrogate architecture and source relationships quickly? | C4-like level controls, interactive component inspector, evidence chain |
| Evidence notebook | Can durable long-form explanation stay readable while keeping claims near their sources? | Editorial chapters, task graph, repository anatomy, layered verification comparison |

All three use representative CodeFlow content and include concept,
architecture, and technical detail. The full comparison and provisional Codex
recommendation are in
[`../documentation-portal-research-2026-08-01.md`](../documentation-portal-research-2026-08-01.md).
The later Claude-led cross-model decision is retained in
[`../documentation-portal-settlement-2026-08-01.md`](../documentation-portal-settlement-2026-08-01.md).

## Retained evidence

The `rendered/` directory contains desktop and mobile captures in light and
dark modes. Additional captures preserve the Guided Path mobile navigation and
implementation state and the System Atlas code-level state. `SHA256SUMS`
identifies the exact 14 retained captures. Browser checks exercised pre-paint
theme selection and persistence, mobile navigation, labeled search and result
status, architecture-level switching, and the component inspector.

Run the source-integrity checks from the repository root:

```sh
node docs/verification/tsk-008-portal-prototypes/verify.mjs
```

The verifier checks the real delegate excerpt and state filenames, capability
count, stable prototype base, pre-paint theme order, search landmarks, and all
repository source-link targets.

The captures demonstrate the exact prototype state only. They do not establish
production accessibility, cross-browser support, generated-content accuracy,
or framework suitability.
