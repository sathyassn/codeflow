# v2 Execution Status

Live tracker for the charter's §12 execution plan. Updated as waves complete.

## Day 0 — transition

- [x] `v1-final` tag + `archive/v1` branch created and pushed to origin
- [x] Stale session worktrees deregistered and removed
- [x] v1 local git hooks (symlinks into `.codeflow/scripts/git-hooks/`) superseded: `core.hooksPath` → `.git/hooks-v2/` with transition hooks (protected-branch commit/push block via `CODEFLOW_TRANSITION` escape, staged-.env block, conventional-commit + no-AI-attribution check). v1 hook content preserved in `archive/v1` and `.git/hooks-v1-backup/`
- [x] Tree reset: all v1 content removed from working tree (preserved in archive)
- [x] v2 skeleton: README, .gitignore, `docs/plan/v2/`
- [x] Cargo workspace skeleton (`crates/codeflow-core`, `crates/codeflow-cli`, `assets/`) compiling
- [x] Push main
- [x] Probe: native workflow rework-loop (charter §12 Day 0) — verdict Door A (native scripts suffice, no bespoke runtime); analysis + draft script in `docs/plan/v2/probe/`

## Day 1 — parallel workstreams

- [x] A — testing engine import: 327 tests green, clippy clean (23 files; config path moved to `.codeflow/test-config.json`; schema+9 templates shipped in `assets/base/testing/`)
- [x] B — records import: 180 tests green, clippy clean (models/ledger/workgraph/validate; SurrealDB NOT imported — `RecordStore` trait + `MarkdownStore` over markdown+frontmatter per D17; format ids simplified to `EPC-NNN`/`TSK-NNN-NNN`)
- [x] C — guards import: 376 tests green, clippy clean (security scanner ×10 modules, git/conflict + ci, file_lock, error pruned 1178→476 lines, doctor v2 check table, settings structured-merge per §4.3 class 2)
- [x] D — scaffold engine: 43 tests green, clippy clean (init w/ bootstrap grace + tier system + husky/hooksPath detection; update w/ 3-way merge via .baseline + `.new` conflicts + additive policy key sync; managed-region + structured settings merge; scaffold-manifest.toml spec; rust-embed CLI with disk-loading in debug; version-skew warning). Integrated: **926 tests green**, clippy 0; asset gaps (gitignore, develop.workflow.js) filled at integration
- [x] E — corpus authoring, all §4.4 caps respected: AGENTS.md.tmpl 116 lines, CLAUDE.md.tmpl 14, cf-reviewer 64, cf-method 131, 3 commands ≤26, policy.json (§6.1 exact), git-hook shims, 3 settings presets, CI template, docs+pm templates; repo's own AGENTS.md/CLAUDE.md instantiated (dogfood)
- [x] Integration: probe+E+A+B+C merged serially into main; **883 tests green** (exact workstream sum), **clippy 0 warnings workspace-wide**

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
