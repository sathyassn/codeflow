---
id: "epic-01KSXTFFTPQS8ABK18M3YG0P1T"
format_id: "INF-EPC-051"
title: "PathFlow → Native Workflows Migration + Uniform Claude-Artifact Restructuring"
summary: "Replace PathFlow phase/sentinel/task-graph orchestration with a deterministic native Workflows layer, and uniformly restructure all .claude/ artifacts to the stateless workflow-role model."
status: draft
area_type: "INF"
work_type: "RFCT"
domain: "GENL"
is_ongoing: false
file_scope:
  - ".claude/"
  - "codeflow-cli/core/src/hooks/"
  - "codeflow-cli/core/src/pathflow/"
  - "codeflow-cli/core/src/autorun/"
  - "codeflow-cli/core/src/session/"
  - "codeflow-cli/core/src/security/"
  - "codeflow-cli/cli/src/"
  - ".codeflow/config/pathflow/"
  - ".codeflow/config/enforcement/"
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-05-30T00:00:00Z"
updated_at: "2026-05-30T00:00:00Z"
supersedes: "INF-EPC-019"
---

# INF-EPC-051: PathFlow → Native Workflows Migration + Uniform Claude-Artifact Restructuring

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

Replace the PathFlow phase/sentinel/task-graph orchestration subsystem with a deterministic native **Workflows** layer (`.claude/workflows/` stage library + composed pipelines), and uniformly restructure all `.claude/` artifacts (CLAUDE.md, 8 agent defs, commands, working-protocol skill, config) to the stateless workflow-role model. `INF-EPC-019` (Claude Artifact Token Efficiency Restructuring) is **superseded and harvested** into this epic — its 8-document analysis package at `.codeflow/docs/analysis/artifact-token-efficiency/` is carried forward, not re-derived.

## Scope

### In Scope

- Build a native Workflows layer (`.claude/workflows/` stage library + composed pipelines) replacing PathFlow orchestration
- STAGE-FINISH LIFECYCLE ANCHOR — single deterministic policy re-fire point at every stage end
- Re-home: claim lifecycle, scope/env/dirty-worktree checks, test-gate, branch-sync into the anchor
- Migrate autorun worker + `codeflow interactive` invocation to workflow runtime
- Restructure all 8 agent definitions → stateless workflow-role (drop peer-messaging, persistence, `## Communication`)
- `codeflow workflows build/run/list` CLI subcommand (new `Workflows` variant in `Command` enum)
- Remove PathFlow internals: PF1-7 graph, pathflow/{sentinel,checkpoint,gates,transitions}.rs + pipeline.rs helpers
- Remove team-guard, checkpoint-register, peer routing, deferred-shutdown, task-tracker mirroring, lead_pid disambig
- Convert pathflow-config → pipelines.config; enforcement-policy → policy-only
- working-protocol skill: drop phase framing, keep 5 cognitive ops
- Full test suite green; `codeflow doctor` clean

### Out of Scope

- WorkGraph 3-tier persistence (JSONL + SurrealDB + Markdown) — RETAINED
- cf-knowledge-layer operations — RETAINED
- 6 retained policy hooks: `gh-pr-guard:334`, `security:339`, `protection-guard:344/359`, `edit-write-guard:354`, `webfetch-guard:369`, `settings-validate:411` — RETAINED
- `codeflow interactive` worktree isolation — RETAINED
- `codeflow rescue` XDG bundle system — RETAINED
- `coordination/*` (claims/sync/merge_queue/loro) — RETAINED
- Standards skills (cf-rust-standards, cf-shell-standards, etc.) — RETAINED

## Acceptance Criteria

> **Chain-coverage requirement:** Epic acceptance criteria MUST cover the full delivery chain.

- [ ] `.claude/workflows/` exists with stage library + composed `cf-*.js` pipelines for all 10 work types; `codeflow workflows build/run/list` runs them deterministically (no agent messaging). Verify: `codeflow workflows list` enumerates ≥10 pipelines; `codeflow workflows run cf-develop --dry-run` shows FEAT stage sequence.
- [ ] STAGE-FINISH LIFECYCLE ANCHOR fires at end of every stage and re-homes: claim-release, scope check, env-file check, dirty-worktree check, test-validation gate. Verify: anchor unit tests cover all 7 stage types; claim-leak regression test passes.
- [ ] WP-4 parity validation passes: workflow layer reproduces every retained policy behavior before any PathFlow removal. Verify: WP-4 parity matrix all-PASS.
- [ ] PathFlow removal complete: PF1-7 graph, pathflow/{sentinel,checkpoint,gates,transitions}.rs + pipeline.rs cumulative helpers, team-guard, checkpoint-register, peer routing, deferred-shutdown, task-tracker mirroring, lead_pid disambiguation all removed; settings.json unwired (379/389/401/421/464). Verify: `grep -rn 'pathflow-pf-\|checkpoint-register\|team-guard' codeflow-cli/core/src/hooks` returns only residue tests; `codeflow doctor` clean.
- [ ] Retained surface intact: 6 policy hooks still present in settings.json at cited lines (334/339/344/354/359/369/411); `codeflow interactive status` works.
- [ ] All artifacts restructured: CLAUDE.md → thin router; 8 agent defs → stateless workflow-role (no peer-messaging/persistence, no `## Working Protocol`/`## Communication`, added `## Workflow Role`); commands split workflow-backed vs direct; working-protocol skill drops phase framing; pathflow-config → pipelines.config; enforcement-policy → policy-only.
- [ ] Full test suite green (`codeflow test --mode full`, ≥2 consecutive clean runs, per-file ≥85% on modified files); `codeflow doctor` healthy.

### PII Handling Review

- [x] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-051-001 | Formalize epic: supersede+harvest INF-EPC-019, validation-branch plan, WorkGraph register | complete | normal |
| INF-TSK-051-002 | Create + verify the validation branch; confirm gate-check unlock behavior | complete | normal |
| INF-TSK-051-003 | Fix 3 verified doc/citation errors (TeamGuard:157, hook count 21→22, acquire_claim_or_block:977) | complete | normal |
| INF-TSK-051-004 | SPIKE: prove one-level workflow() expresses the FEAT rework loop (verify-first) | todo | normal |
| INF-TSK-051-005 | Design STAGE-FINISH LIFECYCLE ANCHOR contract + re-home map | todo | normal |
| INF-TSK-051-006 | Build bridge agent() stage runtime (WorkGraph / git / claim-lifecycle) | todo | normal |
| INF-TSK-051-007 | Stage-library module: WS-DEV + WS-REV stages | todo | normal |
| INF-TSK-051-008 | Stage-library module: WS-QA + WS-TEST stages | todo | normal |
| INF-TSK-051-009 | Stage-library module: WS-PLAN + WS-DOCS stages | todo | normal |
| INF-TSK-051-010 | Stage-library module: WS-SEC stage | todo | normal |
| INF-TSK-051-011 | pipelines.config codegen from pathflow-config pipelines key (10 pipelines) | todo | normal |
| INF-TSK-051-012 | codeflow workflows build/run/list CLI subcommand (Workflows variant in Command enum) | todo | normal |
| INF-TSK-051-013 | Compose cf-*.js pipelines + CLAUDE.md router section | todo | normal |
| INF-TSK-051-014 | Re-home autorun worker invocation to workflow runtime (verify-first NV: worker.rs swap) | todo | normal |
| INF-TSK-051-015 | Re-home codeflow interactive invocation (verify-first NV: interactive.rs:821/914-922/941-950) | todo | normal |
| INF-TSK-051-016 | Restructure 8 agent defs → stateless workflow-role stages (2 parallel batches) | todo | normal |
| INF-TSK-051-017 | SPLIT gate-check: drop ordering, re-home claim/env/dirty/test policy into anchor wiring | todo | high |
| INF-TSK-051-018 | SPLIT sentinel-write + checkpoint-complete: drop stage/team/graph, re-home claim-release/pr_pushed/branch-sync | todo | normal |
| INF-TSK-051-019 | Trim session_start.rs + session_end.rs PathFlow halves; drop security is_pathflow_active branch | todo | normal |
| INF-TSK-051-020 | WP-4 PARITY GATE: validate workflow layer reproduces all retained policy behavior | todo | high |
| INF-TSK-051-021 | REMOVE PathFlow internals: PF1-7 graph, pathflow/{sentinel,checkpoint,gates,transitions}.rs + pipeline.rs | todo | normal |
| INF-TSK-051-022 | REMOVE team-guard + checkpoint-register + peer routing + deferred-shutdown + task-tracker mirroring; unwire settings.json | todo | normal |
| INF-TSK-051-023 | Convert pathflow-config→pipelines.config (final); enforcement-policy→policy-only; commands split | todo | normal |
| INF-TSK-051-024 | working-protocol skill drop phase framing; final docs/router reconcile; full-suite green + doctor clean | todo | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None (supersedes INF-EPC-019)

## Technical Notes

### The 4 LOCKED DECISIONS (binding constraints on every task)

| # | Locked Decision | Constraint imposed on tasks |
|---|-----------------|------------------------------|
| **LD-1** | **ONE UNIFIED EPIC.** Workflow migration **and** uniform Claude-artifact restructuring are a single epic. `INF-EPC-019` is **SUPERSEDED-and-HARVESTED** (status→`superseded` + forward pointer; its 8-doc analysis package is harvested — see Harvest Manifest). | No task may re-derive harvested analysis; cite the harvest doc instead. |
| **LD-2** | **CLEAN BREAK.** Drop the planned `cf-pathflow-protocol` skill **entirely**. **DELETE peer-messaging** — no `cf-team-communication` skill created. Deterministic workflow stage hand-off **replaces** agent SendMessage routing. | No task may introduce a messaging skill or preserve `SendMessage` peer routing. Agent defs drop `## Communication` + persistence. |
| **LD-3** | **TOKEN-EFFICIENCY = SECONDARY.** Primary goal is the migration; token reduction is a soft, re-baselined benefit vs the 1790-line CLAUDE.md. | No task carries a hard token AC. Any token metric is *reported*, not *gated*. |
| **LD-4** | **BOOTSTRAP-SAFE ON A VALIDATION BRANCH.** Build + validate the workflow layer **first** on a dedicated branch (live `gate-check` @ `settings.json:379` currently BLOCKS workflow edits). Remove PathFlow ordering/sentinels **only after WP-4 validation proves parity.** | **No removal task (WP-5/WP-7 trims) may run before its WP-3 re-home AND the WP-4 validation GATE.** |

### Harvest Manifest (INF-EPC-019 → INF-EPC-051)

Source dir (verified, 8 files): `.codeflow/docs/analysis/artifact-token-efficiency/`

| # | Harvested Asset | Carried into |
|---|---|---|
| 1 | `README.md` (package index) | Epic Technical Notes / harvest pointer |
| 2 | `01-design-principles.md` (P0–P6 + SPINE) | Anchor contract (T05) + router design (T13) |
| 3 | `02-skill-taxonomy.md` | working-protocol skill rework (T24); confirms **drop** cf-pathflow-protocol/cf-team-communication (LD-2) |
| 4 | `03-claude-md.md` (target thin-router shape) | CLAUDE.md router (T13, T24) |
| 5 | `04-agent-definitions.md` (stateless workflow-role template) | Agent-def restructure (T16) |
| 6 | `05-commands.md` (workflow-backed vs direct split) | Command split (T23) |
| 7 | `06-skills-detail.md` (archived-skill restore plan) | Skill reconcile (T24) |
| 8 | `07-implementation-order.md` | Composition/build order (T13) + this epic's sequencing |

### `/goal` Command Note

Claude Code's native `/goal` command (v2.1.139+) is an optional completion-driver for single-session/headless task runs — a transcript-judged Stop-hook loop; it is NOT a replacement for WS-QA/test gates (the evaluator does not run tools).

### Validation-Branch Stance (LD-4)

All build/spec/migration work lands on a dedicated validation branch (e.g. `refactor/inf-epc-051-native-workflows`). The legacy PathFlow `gate-check` (`settings.json:379`) still gates Edit/Write on this branch via the normal pf-3 unlock; tasks operate within that. **PathFlow ordering + sentinels are removed only after WP-4 proves parity** — never on the same branch state that still relies on them for the gate that proved parity.

### Gate-check behavior for `.claude/workflows/` (LD-4, T002)

Firsthand analysis (settings.json hook wiring + `pre_tool_use.rs` enforcement) of what blocks Edit/Write to `.claude/workflows/**` pre-migration, so migration sequencing relies on facts, not assumptions. The directory does not exist yet — this is forward-looking.

**Hook wiring (`settings.json`).** Three PreToolUse hooks fire for an Edit/Write, in this order:

| Hook | Matcher line | Command line | Edit/Write role |
|------|-------------:|-------------:|-----------------|
| `edit-write-guard` | `350` (`Edit\|Write`) | `354` | blocked-directory + project-containment check |
| `protection-guard` | `350` (`Edit\|Write`) | `359` | tiered protected-resource check (critical/high/moderate + worktree expansion) |
| `gate-check` | `375` (`Edit\|Write\|Bash\|Task`) | `379` | PathFlow pf-3 sentinel gate + `scope_policy` claim |

**Enumerated blocking scenarios for `.claude/workflows/**`:**

| # | Scenario | Blocked? | By which hook + condition | Citation |
|---|----------|----------|---------------------------|----------|
| 1 | Edit/Write **before** pf-3 sentinel exists | **BLOCKED** (path-agnostic) | `gate-check` — `GateType::EditWrite` requires `pathflow-pf-3`; the check never inspects the file path | `pre_tool_use.rs:1293-1305`; classify @ `:386` |
| 2 | Edit/Write **after** pf-3, in-scope under `scope_policy=soft` | Allowed | `gate-check` auto-acquires the CRDT claim | `pre_tool_use.rs:1313-1321`, `:847-848` |
| 3 | Edit/Write **after** pf-3, out-of-scope under `scope_policy=soft` | Allowed unless another session holds the claim | `gate-check` → `acquire_claim_or_block` (ScopeExpansion on success, ClaimConflict block on conflict) | `pre_tool_use.rs:849-852`, `acquire_claim_or_block` @ `:977` |
| 4 | Edit/Write **after** pf-3, out-of-scope under `scope_policy=hard` | **BLOCKED** | `gate-check` — `is_in_scope` false ⇒ exit 2, no claim attempt | `pre_tool_use.rs:805-830` |
| 5 | protection-guard tiered match on the path | **NOT blocked** | No critical/high/moderate pattern matches `.claude/workflows/**`. The only `.claude/`-prefixed protected patterns are `.claude/settings.json`, `.claude/settings.local.json`, `.claude/CLAUDE.md` (critical) and `.claude/hooks/codeflow/**`, `.claude/settings-templates/**` (high). No blanket `.claude/**`. `matches_glob_pattern` does a `starts_with(prefix-before-**)`, so `.claude/hooks/codeflow/` never matches `.claude/workflows/`. | `enforcement-policy.json:70-95`; `check_tier` @ `pre_tool_use.rs:1891-1921`; matcher @ `:2572-2600` |
| 6 | worktree-protection expansion match | **NOT blocked** | `worktree_protection.patterns` carries the same `.claude/` subset (no `.claude/workflows/`); expansion prefix `.git-worktrees/*/` only widens those same patterns | `enforcement-policy.json:97-127`; `matches_worktree_pattern` @ `pre_tool_use.rs:1924-1936` |
| 7 | edit-write-guard blocked-directory match | **NOT blocked** | `blocked_directories` = `.git`, `.state`, `node_modules`, `__pycache__`, `.venv`, `venv`, `.tox`, `.nox`, `dist`, `build`, `.eggs`. `.claude` is not in the list; `match_blocked_dir` is a path-component name match | `enforcement-policy.json:6-18`; `match_blocked_dir` @ `pre_tool_use.rs:1651-1655` |
| 8 | bypassPermissions prompt for `.claude/` non-exempt subdir | Prompts (interactive only) | CLAUDE.md §5: in bypassPermissions, `.claude/agents/*.md`, `.claude/commands/*.md`, `.claude/skills/**` are EXEMPT; other `.claude/` paths prompt. `.claude/workflows/` is not in the exempt list, so a write there would prompt under bypassPermissions — a harness behavior, not a hook block. Autorun has no human; protection-guard + edit-write-guard early-exit on `is_autorun_session()`. | CLAUDE.md §5 (Teammate Permissions); autorun early-exit @ `pre_tool_use.rs:2228`, `:1712` |

**Implication for migration sequencing:** the **only** hook-enforced block on `.claude/workflows/` edits is the universal pf-3 gate (scenario 1) — there is no path-specific protected-resource block. WP-1+ tasks that create/edit `.claude/workflows/**` need only the normal pf-3 unlock on the validation branch; no protection-tier carve-out or staging workflow is required for that directory (contrast `.claude/settings.json`, `.claude/CLAUDE.md`, `.claude/hooks/**`, which DO require staging). If a future task sets `scope_policy=hard`, `.claude/workflows/**` must be added to that task's `file_scope` (scenario 4). The LD-4 phrase "gate-check … currently BLOCKS workflow edits" is precise only in the path-agnostic pre-pf-3 sense (scenario 1), not a `.claude/workflows/`-specific rule.

### Verified Source Locations (firsthand, apply to all tasks)

- `try_acquire_claim` @ `pre_tool_use.rs:738`
- `check_env_file_present` @ `pre_tool_use.rs:558`
- dirty-worktree guard block @ `pre_tool_use.rs:1178` (return Block @ `:1185`)
- `check_test_validation_gate` @ `pre_tool_use.rs:1209`
- `release_claims_on_stage_complete` @ `post_tool_use.rs:152` (#1 claim-leak risk)
- `pr_pushed` write @ `post_tool_use.rs:418-434`
- `update_branch_from_current` @ `post_tool_use.rs:129`
- `update_worktree_branch` @ `task_completed.rs:120`
- `TeamGuard` struct @ `pre_tool_use.rs:1396`; `impl HookHandler` @ `:1464`; `BlockCategory::TeamGuard` @ `mod.rs:157` (NOT 158)
- `is_pathflow_active` @ `security/mod.rs:221` (field @ `:78`)
- `Command` enum @ `cli/src/main.rs:17-163` — **30 variants** pre-add
- `fn acquire_claim_or_block` @ `pre_tool_use.rs:977`
- `gate-check` @ `settings.json:379`
- 6 retained policy hooks: settings.json lines 334/339/344/354/359/369/411
- Settings.json unwire targets (PathFlow only): 379/389/401/421/464

### Parallel Execution Map

```
START
  ├─ pg-bootstrap:  T01 → T02             (epic formalize → validation branch)
  └─ pg-wp0-docs:   T03                   (∥ anytime, markdown-only, zero file collision)
        │
        ▼ (T02 done)
  PG-SPIKE:         T04                    (gates WP-1 design — YES/NO must resolve)
        │
        ▼
  serial core:      T05 → T06
        │
        ▼
  PG-STAGES (∥):    T07 ∥ T08 ∥ T09 ∥ T10   (4 disjoint stage modules)
        │
        ▼
  serial:           T11 → T12 → T13       (pipelines.config → CLI → compose+router)
        │
        ├─ pg-wp2 (∥):       T14 ∥ T15   (autorun + interactive re-home, both NV)
        └─ PG-AGENTDEFS (∥): T16 (2 batches)
        │
        ▼
  WP-3 serial:      T17 → T18 → T19       (gate-check → sentinel/checkpoint → session/security)
        │
        ▼
══════ WP-4 PARITY GATE: T20 ══════       ◀── HARD SERIAL BARRIER (LD-4)
        │
        ▼
  WP-5 serial:      T21 → T22             (remove internals → remove guards + unwire settings)
        │
        ▼
  serial tail:      T23 → T24             (config promote → skill/docs/full-suite)
END
```

### Autorun Batching

**Execution Order:** Bootstrap (T01→T02) ∥ T03 → T04 → T05 → T06 → T07∥T08∥T09∥T10 → T11 → T12 → T13 → T14∥T15∥T16 → T17 → T18 → T19 → T20 → T21 → T22 → T23 → T24

**Batch Groups:**

| Batch | Tasks | max_workers | Notes |
|-------|-------|-------------|-------|
| 0a | T01, T03 | 2 | Bootstrap + doc fixes, disjoint scopes |
| 0b | T02 | 1 | Validation branch (after T01) |
| 1 | T04 | 1 | SPIKE — serial gate for WP-1 design |
| 2 | T05, T06 | 1 | Serial core design → runtime |
| 3 | T07, T08, T09, T10 | 4 | PG-STAGES parallel — disjoint files |
| 4 | T11, T12, T13 | 1 | Serial: codegen → CLI → compose |
| 5 | T14, T15, T16 | 3 | WP-2 + agent defs parallel |
| 6 | T17, T18, T19 | 1 | WP-3 serial (same-file deps) |
| 7 | T20 | 1 | **HARD BARRIER** — parity gate |
| 8 | T21, T22 | 1 | WP-5 serial removal |
| 9 | T23, T24 | 1 | Config + final reconcile |

**Estimated Duration:** 12-16 sessions (XL tasks T17/T20 are the long poles)

## Related

- INF-EPC-019: Superseded — "Claude Artifact Token Efficiency Restructuring" (status: superseded, superseded_by: INF-EPC-051)
- Harvest source: `.codeflow/docs/analysis/artifact-token-efficiency/` (8 files)
