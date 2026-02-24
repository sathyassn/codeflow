---
id: "epic-01KJ7PE4G3NZDZ159KE8RQHQZK"
format_id: "INF-EPC-016"
title: "PathFlow Cleanup Logic Fixes"
summary: "Fix session-end cleanup logic, team-guard hook, and sentinel creation hooks for proper PathFlow session lifecycle management"
status: complete
area_type: INF
work_type: FIX
domain: ENFC
is_ongoing: false
file_scope: []
priority: high
pr_number: null
external_id: null
external_url: null
created_at: "2026-02-24T11:25:00Z"
updated_at: "2026-02-24T11:25:00Z"
---

# INF-EPC-016: PathFlow Cleanup Logic Fixes

## Summary

Fix the PathFlow session cleanup system: session-end hook logic for proper pathflow-active flag removal, team-guard hook to correctly gate TeamDelete during active sessions, and sentinel creation hooks to ensure sentinels are created at the right phase boundaries.

## Scope

### In Scope

- Session-end hook cleanup logic (`is-pathflow-active` flag removal)
- Team-guard hook (`cf-pre-tool-use-team-guard.sh`) gate enforcement
- Sentinel creation hooks (phase-checkpoint, pathflow-sentinel hooks)

### Out of Scope

- New PathFlow features or phase additions
- Go CLI implementation (tracked in INF-EPC-015)

## Acceptance Criteria

- [ ] Session-end hook correctly removes `is-pathflow-active` flag at session end
- [ ] Team-guard hook correctly blocks TeamDelete while session is active
- [ ] Sentinel creation hooks create sentinels at correct phase boundaries
- [ ] Existing tests pass (1,555+ test suite)

## Tasks

| Task ID | Title | Status |
|---------|-------|--------|
| INF-TSK-016-001 | Fix PathFlow cleanup logic: session-end, team-guard, sentinel hooks | in_progress |
