---
title: "Autorun Integration"
type: analysis
status: draft
author: cf-planning
created_at: "2026-03-04"
updated_at: "2026-03-05"
parent: "parallel-work/README.md"
---

# Autorun Integration

[← Back to Overview](README.md)

## Table of Contents

- [1. Current State](#1-current-state)
- [2. Worktree-Integrated Autorun](#2-worktree-integrated-autorun)
- [3. Implementation Changes](#3-implementation-changes)
- [4. Worker Lifecycle](#4-worker-lifecycle)

---

## 1. Current State

The autorun worker at `worker.go:108-116` (within `TmuxWorker.Run()` at line 84) invokes Claude Code with `WorkDir: "."` -- all workers share the project root. Each worker gets its own tmux session name (`tmuxName`) but operates on the same filesystem.

```go
result, err := w.Claude.Invoke(ctx, InvokeConfig{
    WorkDir:     ".",                                    // PROBLEM: all workers share root
    Prompt:      fmt.Sprintf("Execute autorun task %s", cfg.TaskID),
    SessionID:   cfg.SessionID,
    TaskID:      cfg.TaskID,
    AutoMerge:   cfg.AutoMerge,
    Target:      cfg.Target,
    TmuxSession: tmuxName,
})
```

---

## 2. Worktree-Integrated Autorun

Each autorun worker needs its own worktree for filesystem isolation:

```text
Autorun Orchestrator
    |
    +-- Worker 1: worktree-{SID-1}
    |   WorkDir: .git-worktrees/worktree-{SID-1}
    |   Branch: feat/task-A (created at PF3 inside worktree)
    |   Task: INF-TSK-008-001
    |
    +-- Worker 2: worktree-{SID-2}
    |   WorkDir: .git-worktrees/worktree-{SID-2}
    |   Branch: fix/task-B (created at PF3 inside worktree)
    |   Task: INF-TSK-008-002
    |
    +-- Worker 3: worktree-{SID-3}
        WorkDir: .git-worktrees/worktree-{SID-3}
        Branch: feat/task-C (created at PF3 inside worktree)
        Task: INF-TSK-008-003
```

---

## 3. Implementation Changes

| Component | Change | File |
|-----------|--------|------|
| `InvokeConfig` | Add `WorktreePath` field | `worker.go` |
| `TmuxWorker.Run()` | Before `Claude.Invoke()`, call `Manager.SetupDetached()`. Pass worktree path as `WorkDir`. | `worker.go` |
| `TmuxWorker.Run()` cleanup | After worker completes (success or failure), call `Manager.Cleanup()`. | `worker.go` |
| `RealClaudeInvoker` | Use `WorkDir` from config instead of hardcoded `"."`. | `worker_real.go` |
| Batch orchestrator | Validate batch size against `max_session_workers` (3). | `cmd/autorun/` |
| Claims coordination | Workers acquire claims on their task's file patterns before starting. Conflict = queue task for later. | `worker.go` + `claim.go` |

---

## 4. Worker Lifecycle

```text
Worker spawned by orchestrator
    |
    v
1. Generate worker session ID
2. Manager.SetupDetached("worktree-{SID}")
    -> Creates worktree with shared/local state
3. Write codeflow-env.sh in worktree
    |
    v
4. Claude.Invoke(WorkDir: worktree-path)
    -> Claude session starts inside worktree
    -> SessionStart hook detects existing worktree (skip creation)
    -> PathFlow runs PF1-PF7 inside worktree
    |
    v
5. Worker completes (success/failure/timeout)
    |
    v
6. Manager.Cleanup("worktree-{SID}")
    -> git worktree remove
    -> Deregister from worktrees.yaml
```

**Note:** Step 4 requires the SessionStart hook to detect that a worktree already exists (env file present, worktree path set) and skip worktree creation. This "pre-created worktree" mode is needed for the autorun integration where the orchestrator creates the worktree before invoking Claude.

---

[← Back to Overview](README.md)
