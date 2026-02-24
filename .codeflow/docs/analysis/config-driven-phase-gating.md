---
id: BRIEF-007
title: "Config-Driven Phase Gating for PathFlow"
status: proposed
author: team-lead
created: "2026-02-24"
updated: "2026-02-24"
epic_id: null
---

# Config-Driven phase gating for PathFlow

## Summary

PathFlow's gate enforcement logic is currently hardcoded across three separate shell scripts.
This brief proposes moving all gate definitions into `pathflow-config.json` as declarative
configuration, with a single unified PreToolUse hook reading that config to enforce all gates.
This is a future work proposal — no implementation is planned until after the Go CLI lands.

## Problem statement

### Current state

Gate enforcement is scattered across multiple hooks with hardcoded sentinel names and command
patterns:

| Hook | Hardcoded logic |
|------|----------------|
| `cf-pre-tool-use-pathflow-gate.sh` | `pf-3` for Edit/Write/commit; `pf-5` + `ws-rev` for push/PR; hardcoded `ROLE_TEAMMATES` list |
| `cf-pre-tool-use-team-guard.sh` | `pf-6` for TeamDelete; pathflow-active flag check |
| `cf-post-tool-use-pathflow-sentinel.sh` | Stage ordering: which sentinel unlocks which stage |

Each new gate requires editing shell scripts, understanding the internal sentinel naming
convention, and manually testing all affected command patterns. There is no single place
to read and understand what gates exist or why.

### Desired state

All gate definitions live in `pathflow-config.json` under a `gates` section per phase and
stage. A single unified PreToolUse hook reads the config and enforces all gates. Adding a
new gate is a config change, not a code change. Agents can read the config to understand
what constraints exist and why.

---

## Scope

### In scope

- `pathflow-config.json` schema extension: `gates` sections per phase and stage
- New unified hook `cf-pre-tool-use-pathflow-unified-gate.sh`
- Migration of logic from `cf-pre-tool-use-pathflow-gate.sh` (edit/write, commit, push/PR, role teammate gates)
- Migration of logic from `cf-pre-tool-use-team-guard.sh` (TeamDelete gate)
- Migration of stage ordering checks from `cf-post-tool-use-pathflow-sentinel.sh`
- Config schema validation at session start (via SessionStart hook)
- Dual-write parallel validation period before removing old hooks

### Out of scope

- `cf-post-tool-use-phase-checkpoint.sh` (task registration — different concern)
- `cf-task-completed-phase-checkpoint.sh` (phase sentinel creation — different concern)
- `cf-pre-tool-use-edit-write.sh` (scope enforcement — different concern)
- `cf-pre-tool-use-protected-resource.sh` (resource protection — different concern)
- `cf-pre-tool-use-security.sh` (security checks — different concern)
- `cf-pre-tool-use-webfetch.sh` (URL validation — different concern)
- Go CLI integration (config-driven gating can ship before or after CLI)

---

## Requirements

### Functional requirements

1. **FR-1**: All gate definitions (sentinel names, blocked tools, blocked commands, blocked
   teammates) must be expressible in `pathflow-config.json` without shell script changes.

2. **FR-2**: The unified gate hook must enforce dual-sentinel gates (e.g., `pf-5` AND `ws-rev`
   both required) and `any-of` sentinel gates (e.g., at least one of `ws-dev`, `ws-docs`,
   `ws-plan`, `ws-test`).

3. **FR-3**: Config validation must run at session start and emit a clear error if the config
   is malformed, rather than silently misenforcing gates.

4. **FR-4**: Graceful degradation must be preserved: critical gates (push/PR, role teammate
   spawn, TeamDelete) default to BLOCK when config or sentinel library is unavailable;
   non-critical gates (edit/write, commit) default to ALLOW.

5. **FR-5**: The unified hook must be backward compatible — existing sessions with existing
   sentinels must enforce correctly without re-initialization.

### Non-functional requirements

1. **NFR-1**: Hook invocation overhead must not exceed 50ms per call (jq parsing is the
   primary risk — config should be cached per session where possible).

2. **NFR-2**: The config schema must be machine-readable and self-documenting (field names
   and descriptions in the schema make the gate semantics clear without reading shell code).

3. **NFR-3**: Migration must be zero-downtime: old and new hooks run in parallel during
   validation, with identical block/allow decisions. Any divergence is logged and treated
   as a bug in the new hook.

---

## Proposed config schema

Each phase and stage in `pathflow-config.json` gains a `gates` section:

```json
{
  "phases": {
    "PF3": {
      "gates": {
        "required_sentinel": "pf-2",
        "blocked_tools": ["Edit", "Write"],
        "blocked_commands": ["git commit"],
        "blocked_teammates": [
          "cf-development",
          "cf-planning",
          "cf-documentation",
          "cf-review",
          "cf-quality-assurance"
        ]
      }
    },
    "PF5": {
      "gates": {
        "required_sentinel": "pf-4"
      }
    },
    "PF6": {
      "gates": {
        "required_sentinel": "pf-5",
        "blocked_commands": ["git push", "gh pr create", "gh pr merge"]
      }
    },
    "PF7": {
      "gates": {
        "required_sentinel": "pf-6",
        "blocked_tools": ["TeamDelete"]
      }
    }
  },
  "stages": {
    "WS-REV": {
      "gates": {
        "required_sentinels_any": ["ws-dev", "ws-docs", "ws-plan", "ws-test"],
        "description": "At least one primary stage must complete before WS-REV can gate push/PR"
      }
    },
    "WS-QA": {
      "gates": {
        "required_sentinels": ["ws-rev"]
      }
    }
  }
}
```

### Schema field reference

| Field | Type | Semantics |
|-------|------|-----------|
| `required_sentinel` | string | Single sentinel that must exist (ALL gate) |
| `required_sentinels` | string[] | All sentinels must exist (AND gate) |
| `required_sentinels_any` | string[] | At least one sentinel must exist (OR gate) |
| `blocked_tools` | string[] | Tool names blocked without the required sentinel |
| `blocked_commands` | string[] | Bash command patterns blocked without the required sentinel |
| `blocked_teammates` | string[] | Teammate names blocked from spawning without the required sentinel |
| `description` | string | Human-readable explanation of the gate's purpose |

---

## Technical approach

### Key components

- **Config loader**: jq-based parser in the unified hook; reads `pathflow-config.json` once
  per hook invocation (or from a cached temp file per session)
- **Unified gate hook** (`cf-pre-tool-use-pathflow-unified-gate.sh`): replaces three existing
  PreToolUse hooks; uses the sentinel library (`has_sentinel`) unchanged
- **Schema validator**: runs at SessionStart (via `cf-session-start-init.sh`); validates all
  `gates` sections have valid field types and sentinel name formats
- **Dual-write harness**: during migration, both old and new hooks run; a comparison function
  logs any block/allow divergence to `.state/logs/gate-validation.jsonl`

### Hook execution flow (proposed)

```text
PreToolUse fires
    |
    v
cf-pre-tool-use-pathflow-unified-gate.sh
    |
    +-- Classify tool call (edit_write / git_commit / git_push_pr / role_spawn / team_delete / ungated)
    |
    +-- Load gates from pathflow-config.json (jq)
    |
    +-- Find matching gate rule for this tool call type
    |
    +-- Check required sentinel(s) via has_sentinel()
    |
    +-- Allow or block (with same error messages as current hooks)
```

### Dependencies

- `pathflow-config.json` schema extension (backward compatible — existing parsers ignore unknown fields)
- `has_sentinel()` function from sentinel library (unchanged)
- `jq` (already required throughout the hook system)
- SessionStart hook for config validation step

---

## Migration path

| Step | Action | Validation |
|------|--------|-----------|
| 1 | Add `gates` sections to `pathflow-config.json` | Schema validator passes at session start |
| 2 | Create `cf-pre-tool-use-pathflow-unified-gate.sh` | Unit tests cover all gate scenarios |
| 3 | Run old and new hooks in parallel (dual-write) | Zero divergence in gate-validation.jsonl over N sessions |
| 4 | Remove old hooks from `settings.json` hook config | Existing tests still pass |
| 5 | Delete old hook scripts | Clean up |

---

## Risks

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| jq parsing adds latency per PreToolUse call | Medium | Medium | Cache parsed config to session temp file; measure actual overhead before shipping |
| Config schema malformed by mistake | High | Low | Schema validation at SessionStart blocks session before any work begins |
| New hook misses an edge case present in old hooks | High | Medium | Dual-write parallel validation period with divergence logging before removing old hooks |
| Migration breaks existing in-flight sessions | Medium | Low | Backward-compatible config schema; old sentinels remain valid; no re-initialization needed |
| `required_sentinels_any` semantics unclear | Low | Medium | Explicit `description` field in config; schema docs clarify OR vs AND gates |

---

## Success metrics

- All gate scenarios covered by config (zero hardcoded sentinel names in shell scripts)
- Hook invocation overhead under 50ms measured in test suite
- Zero gate divergence during dual-write validation period (minimum 5 tracked sessions)
- Existing 1,555+ test suite passes without modification after migration

---

## Related

- `pathflow-config.json` -- current phase and stage definitions (to be extended with `gates`)
- `cf-pre-tool-use-pathflow-gate.sh` -- primary hook to be replaced
- `cf-pre-tool-use-team-guard.sh` -- team-guard hook to be merged in
- `cf-post-tool-use-pathflow-sentinel.sh` -- stage ordering logic to be migrated
- BRIEF-006: PathFlow lifecycle reference -- documents current gate enforcement (Section: Sentinel system)
