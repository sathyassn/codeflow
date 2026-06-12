# v2 Execution Status

Live tracker for the charter's §12 execution plan. Updated as waves complete.

## Day 0 — transition

- [x] `v1-final` tag + `archive/v1` branch created and pushed to origin
- [x] Stale session worktrees deregistered and removed
- [x] v1 local git hooks (symlinks into `.codeflow/scripts/git-hooks/`) superseded: `core.hooksPath` → `.git/hooks-v2/` with transition hooks (protected-branch commit/push block via `CODEFLOW_TRANSITION` escape, staged-.env block, conventional-commit + no-AI-attribution check). v1 hook content preserved in `archive/v1` and `.git/hooks-v1-backup/`
- [x] Tree reset: all v1 content removed from working tree (preserved in archive)
- [x] v2 skeleton: README, .gitignore, `docs/plan/v2/`
- [ ] Cargo workspace skeleton (`crates/codeflow-core`, `crates/codeflow-cli`, `assets/`) compiling
- [ ] Push main
- [x] Probe: native workflow rework-loop (charter §12 Day 0) — verdict Door A (native scripts suffice, no bespoke runtime); analysis + draft script in `docs/plan/v2/probe/`

## Day 1 — parallel workstreams

- [ ] A — testing engine import (green)
- [ ] B — records import: workgraph, ledger, store, validate, models subset (green)
- [ ] C — guards import: security scanner, git/conflict + ci_wait, file_lock, error, doctor core, settings (green)
- [ ] D — scaffold engine: init/update, manifest, ownership classes, 3-way merge, rust-embed (new)
- [ ] E — corpus authoring: AGENTS.md, CLAUDE.md shim, cf-reviewer, cf-method, develop workflow, 3 commands, settings presets, policy.json, git-hook shims, CI + docs templates (new)

## Day 2 — new builds + integration

- [ ] git-guard, integrate, recall + orient + session-summary, remote protect (GitHub), status
- [ ] init end-to-end; full suite green; doctor clean

## Day 3 — dogfood + release

- [ ] `codeflow init` on this repo (consumer #1); EPC-001 recorded
- [ ] `codeflow init` on first real user project
- [ ] v2.0.0 tag + cargo-dist release (PAUSE for user go)

## Notes / deviations

- Sandbox guardrails required explicit bypasses for: `.git/config` write (hooksPath), worktree dir deletion, residual `.claude/` deletion. All sanctioned transition steps; logged here for the record.
- Remote protection unavailable on this repo (private + GitHub Free, API 403). Perimeter = CI + discipline until public/Pro. `codeflow remote protect` must report this exact degradation (charter AC #4 caveat verified empirically).
