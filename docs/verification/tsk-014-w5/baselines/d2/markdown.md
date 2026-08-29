# SPC-004 — interactive presentation utility contract

Ordinary repository Markdown: how a record page reads today, with its decision links and
evidence written out. Retained as the second plain baseline; it is not a design.

- **Status** `approved` · **Created** 2026-07-31 · `project-management/specs/SPC-004.md`

## Claims and their decisions

### CLM-1 — § Behavior 1 · ADR-0050 governs

> An older renderer opening a newer schema falls back to an explicit read-only/raw view,
> never a silent partial render.

Evidence: `codeflow test --mode full --strict` passed all nine targets at candidate
`82671551`, macOS arm64 only.

### CLM-2 — § Behavior 10 · ADR-0049 governs

> never attaches to the operator's browser profile or active view

Evidence: the real headless browser journey and the injected-failure cleanup journey both
ran at the exact candidate. The journey used `--no-launch` with a directly owned Playwright
browser, so it is not native product-launcher evidence.

### CLM-3 — § Behavior 8 · ADR-0049 governs, ADR-0052 supersedes one named mechanism

> Owner-private state uses the platform-appropriate OS state directory keyed by canonical
> project identity, so worktrees share state without writing runtime data into `.git` or
> the worktree.

Evidence: a deterministic confinement guard runs, and the record states exactly what it
does not prove — start-time confinement only.

### CLM-4 — § Interfaces and formats · ADR-0052 governs

> each supported runtime needs native or explicitly scoped runtime evidence, and an
> unqualified platform fails closed rather than selecting an insecure fallback.

Evidence: Linux arm64 evidence exists at revision `74feaf04`, older than the exact
candidate, and is explicitly not promoted.

### CLM-5 — § Behavior 3 · ADR-0051 governs the process

> The selected default must be modern, elegant, aesthetically coherent, pleasing,
> functional, responsive, and suited to focused explanation/review rather than resembling a
> generic model artifact or dashboard template.

Evidence: none recorded. The operator's real trial invalidated the prior design acceptance,
and the replacement rendered board's blind gates are not run.

## The partial supersession

> ADR-0049 and ADR-0050 remain historical decisions; this ADR supersedes only their
> same-tree runtime assumption and post-create Windows DACL mechanism where those details
> conflict.
