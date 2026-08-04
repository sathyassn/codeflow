# SPC-004 — authority and evidence

Ordinary Markdown treatment of the same content. `baseline.html` is a faithful
hand-authored HTML rendering of this file.

## The record

| Field | Value |
|---|---|
| id | `SPC-004` |
| title | interactive presentation utility contract |
| status | `approved` |
| created | 2026-07-31 |

Not `implemented`: its consuming work `TSK-011` is `blocked` on `TSK-006` and
`TSK-014`.

## Decisions

| ADR | Title | Date | Status | superseded_by |
|---|---|---|---|---|
| ADR-0049 | bounded cf-present runtime and renderer boundary | 2026-08-01 | accepted | `null` |
| ADR-0050 | versioned cf-present document and session contract | 2026-08-01 | accepted | `null` |
| ADR-0052 | separate cf-present ephemeral runtime from durable state | 2026-08-01 | accepted | `null` |

ADR-0052 is the one that governs the record now, partially:

> ADR-0049 and ADR-0050 remain historical decisions; this ADR supersedes only
> their same-tree runtime assumption and post-create Windows DACL mechanism where
> those details conflict.

and

> This decision changes no product outcome, task owner, graph, or integration
> target.

## The central claim

SPC-004 § Behavior 3:

> The selected default must be modern, elegant, aesthetically coherent, pleasing,
> functional, responsive, and suited to focused explanation/review rather than
> resembling a generic model artifact or dashboard template.

Evidence: **none current.** The operator's real `cf-present` trial invalidated the
prior design acceptance (`TSK-014` closeout), and the replacement rendered board's
gates G1–G8 are not run (`observer-state.md`).

## What evidence does exist

| Claim area | Evidence | Bound |
|---|---|---|
| nine strict test targets | `codeflow test --mode full --strict`, macOS arm64, at `82671551` | macOS arm64 only |
| native Linux support | headless Ubuntu 24.04 arm64 guest at revision `74feaf04` | older than the exact candidate; retained as bounded diagnostic, not promoted |
| Windows profile safety | deterministic confinement guard | start-time confinement only, not account/profile/VM teardown |
| native Windows / WSL2 run | none | Parallels licence expired |
| cross-lineage review | two GPT-5.6 Sol/high reviews, no material findings | at `82671551` |
| Claude judgment verdict | absent | accepted turn ended `oauth_org_not_allowed` |
