---
id: "INF-EPC-010"
format_id: "INF-EPC-010"
title: "Phase Checkpoint Enforcement"
summary: "Replace tool-pattern-based phase sentinel creation with task-completion-driven checkpoint system using PostToolUse and TaskCompleted hooks"
status: planning
area_type: "INF"
work_type: "FEAT"
domain: "ENFC"
is_ongoing: false
file_scope:
  - ".claude/hooks/codeflow/"
  - ".codeflow/scripts/state/"
  - ".codeflow/config/pathflow/"
  - ".claude/settings.json"
  - ".claude/CLAUDE.md"
priority: high
pr_number: null
external_id: null
external_url: null
created_at: "2026-02-22"
updated_at: "2026-02-22"
---

# INF-EPC-010: Phase Checkpoint Enforcement

## Summary

Replace tool-pattern-based phase sentinel creation with a task-completion-driven checkpoint system. Currently, phase sentinels (pf-1, pf-2, pf-3, pf-6) are created when specific tool executions are detected (TeamCreate, git checkout, gh pr create), regardless of whether all PF{N}-TSK-{NN} tasks within the phase actually completed. This allows LLMs to skip required tasks and still advance phases.

The new system uses two hooks:

1. **PostToolUse on TaskCreate** (Layer 1): Registers PF task creation in a session-scoped checkpoint file
2. **TaskCompleted lifecycle hook** (Layer 2): Marks tasks complete and creates the phase sentinel only when ALL required tasks are verified done

The existing PreToolUse gate hook (Layer 3) remains unchanged -- it still reads sentinels to allow or block operations. Stage sentinels (ws-dev, ws-rev, ws-qa) also remain unchanged (message-based via STAGE-COMPLETE pattern).

## Scope

### In scope

- PostToolUse phase checkpoint hook (TaskCreate registration tracking)
- TaskCompleted phase checkpoint hook (completion verification + sentinel creation)
- Checkpoint state helper functions in cf-pathflow-state.sh
- Existing sentinel hook modification (remove phase triggers, keep stage triggers)
- pathflow-config.json updates (required_tasks per phase, PF1-TSK-02 removal, PF4-TSK-05 condition, max_qa_retries alignment)
- settings.json hook registration
- CLAUDE.md Section 7 documentation update
- Tests for all checkpoint enforcement behavior

### Out of scope

- Stage sentinel changes (ws-dev, ws-rev, ws-qa remain message-based)
- PreToolUse gate hook changes (reads sentinels, unchanged)
- Team guard hook changes (pathflow-active flag check, unchanged)
- Go CLI migration (future work)
- Cross-session checkpoint persistence (checkpoints are ephemeral per session)

## Acceptance criteria

- [ ] PostToolUse hook registers PF{N}-TSK-{NN} tasks in checkpoint file when TaskCreate fires
- [ ] TaskCompleted hook marks PF tasks complete and creates phase sentinel when all phase tasks are done
- [ ] Existing sentinel hook no longer creates phase sentinels (pf-1, pf-2, pf-3, pf-6) but still creates stage sentinels
- [ ] Conditional tasks (adhoc_only, if_pipeline_includes_qa) are handled correctly (skipped when not applicable, not blocking)
- [ ] System fails closed: missing/corrupted checkpoint blocks sentinel creation
- [ ] Checkpoint state is session-scoped and cleaned up by SessionEnd hook
- [ ] pathflow-config.json PF1-TSK-02 removed, PF4-TSK-05 has condition field, max_qa_retries aligned to 3
- [ ] CLAUDE.md Section 7 documents three-layer checkpoint enforcement model
- [ ] All tests pass for checkpoint initialization, registration, completion, sentinel creation, conditional tasks, and graceful degradation

## Tasks

| ID | Title | Status | Estimate | Priority |
|----|-------|--------|----------|----------|
| INF-TSK-010-001 | Create checkpoint hooks and state helpers | todo | L | high |
| INF-TSK-010-002 | Modify existing sentinel hook to remove phase triggers | todo | S | high |
| INF-TSK-010-003 | Update pathflow-config.json and settings.json | todo | S | normal |
| INF-TSK-010-004 | Update CLAUDE.md documentation | todo | M | normal |
| INF-TSK-010-005 | Write tests for checkpoint enforcement | todo | L | high |

## Dependencies

### Blocked by

- None (INF-EPC-008 is complete; design analysis is ready)

### Blocks

- None

## Technical notes

- Design analysis: `.codeflow/docs/analysis/phase-checkpoint-enforcement.md`
- The three-layer architecture separates concerns: Layer 1 (registration) operates inside the agentic loop, Layer 2 (completion) operates outside it via CLI lifecycle events, Layer 3 (enforcement) blocks operations based on sentinel existence
- TaskCompleted is a CLI-emitted lifecycle event (fires outside agentic loop), providing higher trust than PostToolUse events
- Checkpoint file path: `.state/checkpoints/pathflow/{session-id}/phase-tasks.json`

## Related

- Design analysis: `.codeflow/docs/analysis/phase-checkpoint-enforcement.md`
- INF-EPC-008: PathFlow PR Verification, Merge Protection & Validation Hardening (predecessor)
- INF-EPC-009: Test Enforcement Requirements and Deferred Shutdown (parallel work)
- CLAUDE.md Section 7: Enforcement & Operations
