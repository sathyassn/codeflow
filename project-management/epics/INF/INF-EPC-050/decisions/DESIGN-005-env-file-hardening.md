---
id: DESIGN-005
title: codeflow-env.sh write failure detection and recovery
status: accepted
date: 2026-05-07
deciders: [cf-planning]
consulted: []
informed: []
description: Defines how to detect codeflow-env.sh write failures early and what recovery behavior is appropriate per call site.
tags: [autorun, interactive, env, hooks, ops]
---

# DESIGN-005: codeflow-env.sh Write Failure Detection and Recovery

## Status

Accepted.

## Context

The `codeflow-env.sh` and per-PID `codeflow-env-{pid}.sh` files
(`codeflow-cli/core/src/session/env.rs`) carry the
`CODEFLOW_SESSION_ID`, `CF_PROJECT_ROOT`, and `CODEFLOW_WORKTREE_PATH`
exports needed by hooks at every tool call. They are written at
session start and read by `detect_project_dir` and the gate-check
hook. A failed write surfaces much later — usually as a vague "file
not found" or "session id missing" error from a downstream hook —
making the root cause hard to diagnose.

### Problem

Section 4.5 of the comprehensive audit
(`project-management/epics/INF/INF-EPC-050/analysis/2026-05-03-autorun-interactive-comprehensive-audit.md`)
records:

> When `.state/runtime/codeflow-env.sh` write fails (full disk,
> permission), the failure surfaces as a vague "file not found"
> later when a hook tries to `source` it. How to detect early and
> recover?

The current code paths are:

| Function | Behavior | Failure mode |
|----------|----------|-------------|
| `write_env_file` (`env.rs:58`) | Atomic tmp+rename. Returns `Result<PathBuf, SessionError::Io>`. | If callers ignore the `Err`, write failure is silent. |
| `write_env_file_with_worktree` (`env.rs:75`) | Same atomic tmp+rename, with `CODEFLOW_WORKTREE_PATH`. | Same. |
| `write_pid_env_file` (`env.rs:139`) | NON-fatal — function returns `()`, all errors discarded with `let _`. | Documented as "Non-fatal: errors are silently ignored". |

Failure modes that produce silent corruption:

1. **Disk full** (`ENOSPC`). The tmp file write fails partway; rename
   does not happen; the previous `codeflow-env.sh` is intact (good)
   but stale (still points at the previous session). Hooks read the
   STALE values and route operations to the wrong session.
2. **Permission denied** on the runtime dir (`EACCES`). The tmp file
   create fails; `codeflow-env.sh` is unchanged. Same stale-data
   failure mode as above.
3. **Read-only filesystem** (`EROFS`). Same shape as ENOSPC.
4. **Filesystem corruption** mid-rename. Rare but possible on
   network filesystems; the atomic rename's atomicity is
   filesystem-dependent.

The per-PID variant's `let _` documentation says the function is
non-fatal. This is correct for some call sites (best-effort cleanup)
but wrong for others (the early-write at SessionStart that the lead
PID's hooks depend on). The ambiguity is the bug.

### Constraints

- Atomic tmp+rename is already correct for the common-case write
  path. The fix is only about detection and recovery, not write
  semantics.
- Hooks that consume the env file run inside Claude's tool call
  pipeline; an OS-level error must not crash Claude itself.
- The runtime dir lives under `.state/runtime/`, which is shared
  across worktrees by symlink (and per-worktree under
  `worktree/.state/runtime/local/` in worktree mode). Either path
  must be writable, but neither has special rights at OS level.
- An autorun worker that cannot write its env file is unrecoverable;
  hooks will not function. The right answer is to refuse to start.
- An interactive session that cannot write its env file is also
  broken, but the user is present and can intervene; refuse to
  start with a clear error.

### Assumptions

- `fs::create_dir_all` failure means no directory exists and none
  can be created; the underlying error preserves the OS reason.
- A failed write that DOES NOT replace the target leaves the target
  in its prior state (stale or absent). The tmp file may or may not
  exist after a failed write.
- `EROFS` and `ENOSPC` are persistent across retries within the
  current process lifetime — retrying does not help.
- The current `let _` discards in `write_pid_env_file` represent a
  conscious "best effort" choice but were never audited for which
  call sites need fail-loud semantics.

## Design

### Detection

Replace `write_pid_env_file`'s silent error discard with explicit
return:

```rust
pub fn write_pid_env_file(runtime_dir: &Path, pid: u32, worktree_path: &str)
    -> Result<PathBuf, SessionError> {
    let shared = runtime_dir.join("shared");
    fs::create_dir_all(&shared)?;
    let path = shared.join(format!("{PID_ENV_PREFIX}{pid}{PID_ENV_SUFFIX}"));
    let content = format!("export CODEFLOW_WORKTREE_PATH='{worktree_path}'\n");
    let tmp = shared.join(format!(".{}.tmp", path.file_name().unwrap().to_string_lossy()));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, &path)?;
    Ok(path)
}
```

Add a separate `try_write_pid_env_file(...) -> Result<...>` that
existing best-effort callers (cleanup paths) can wrap with `let _`.
Call sites that load-bear on the env file get the new fail-loud
function.

Add a verification helper:

```rust
pub fn verify_env_file_readable(state_dir: &Path) -> Result<EnvFile, SessionError> {
    read_env_file(state_dir)?.ok_or(SessionError::EnvFileMissing)
}
```

Call it immediately after every load-bearing write to confirm
readback succeeds. This catches the rare case where the rename
appears to succeed but the file is empty / partial / not visible
(network filesystem caching, container layer issues).

### Recovery

| Failure context | Behavior |
|-----------------|----------|
| Autorun worker SessionStart write fails | Refuse to start. Surface a structured error. Do not spawn the worker. The orchestrator marks the worker `failed_env_write` and continues with the other workers. The batch is not aborted. |
| Interactive `codeflow -i` SessionStart write fails | Refuse to start. Print `error: cannot write {path}: {reason}; aborting session start`. Exit non-zero. The user fixes the underlying problem (disk full, permission) and retries. |
| Hook calls `read_env_file` and gets `Ok(None)` | Hook MUST fail closed: log the missing file, BLOCK the tool call (exit 2 with `EnvFileMissing` reason), prompt the operator. This applies to PreToolUse hooks; hooks that fire on Stop / SessionEnd already handle env-file-missing gracefully (cleanup is best-effort there). |
| Best-effort cleanup write fails | Caller uses `try_write_pid_env_file` and continues with `let _`. No new behavior. |

A new `SessionError::EnvFileWriteFailed { path, reason }` variant
carries the OS error string. A `SessionError::EnvFileMissing { path }`
variant covers the readback case. Both are surfaced via the existing
`anyhow::Result` plumbing.

### Numeric tuning

| Knob | Value | Rationale |
|------|-------|-----------|
| Retry count on write | **0** (no retry) | ENOSPC / EACCES / EROFS are not retriable; retrying delays the error without helping. |
| Readback verification | **1 attempt** | Same reasoning. A failed readback is a structural problem. |
| Readback timeout | **None** (synchronous) | The file is small; the OS read is sub-ms. A timeout would mask filesystem issues. |
| `try_write_*` retry | **0** (best-effort means best-effort) | If a best-effort write fails, fail silently and move on. |

## Alternatives Considered

### Option 1: Retry-on-failure with backoff

Retry the write 3 times with exponential backoff before giving up.

**Pros:**

- Could mask transient filesystem issues (NFS lag, cache flush
  timing).

**Cons:**

- The actual failure modes (ENOSPC, EACCES, EROFS) are not
  transient.
- Backoff delays the error without changing the outcome.
- Adds complexity and timing dependencies to a code path that
  fires on every session start.

**Why rejected:** the failure modes the audit describes are not
shaped like the ones retry helps with.

### Option 2: Fall back to env-vars-only mode

If the env file cannot be written, set the values as exported env
vars in the current process and require all hooks to consult both
the env file AND the env vars.

**Pros:**

- The current process can continue without the env file.

**Cons:**

- Doubles the variable-resolution logic in every hook.
- Breaks the per-PID visibility model: child processes inherit env
  vars only at spawn, not on parent's later writes.
- Hooks fire from Claude's process, which inherits Claude's env at
  Claude-start time; we cannot mutate Claude's env after start.
- Effectively requires the env file anyway for child-hook
  visibility — fallback gives nothing.

**Why rejected:** doesn't actually provide a fallback.

### Option 3: Centralized env-file health check at SessionStart

Add a `health_check_env_file(state_dir)` that verifies the file is
present and readable, called from a single place at SessionStart.

**Pros:**

- One canonical place to catch env-file problems.

**Cons:**

- Doesn't address the per-PID variant's silent-failure issue.
- Doesn't address writes from later in the session (e.g.,
  worktree-mode-on-the-fly).

**Why rejected:** narrower than the chosen design. The chosen design
includes this check at SessionStart and at every load-bearing
write.

## Consequences

### Positive

- Disk-full / permission failures fail at session start with a
  clear OS-error message, not three calls deep in a hook.
- The `try_write_pid_env_file` / `write_pid_env_file` split makes
  call-site intent explicit; reviewers can tell at a glance which
  call sites are load-bearing.
- The hook-side fail-closed behavior (`EnvFileMissing` blocks the
  tool call) prevents the historic "wrong session id leaks into
  another session" failure mode.

### Negative

- A new `SessionError` variant requires call-site updates.
- The fail-loud mode for the per-PID env file means that a
  partially-broken filesystem can now block worker startup that
  previously would have started in degraded mode. This is
  intentional — degraded mode was always wrong — but is
  user-visible.

### Risks

- **Hook-blocking false positives.** If a hook fires before
  SessionStart's env-file write completes (very fast race),
  fail-closed blocks the tool call. Mitigation: SessionStart
  writes the env file SYNCHRONOUSLY before any tool call can
  fire (the hook bus is in-process and ordered).
- **Operators reading old documentation** that says "errors are
  silently ignored" will find the new fail-loud behavior surprising.
  Mitigation: comment in `env.rs` and the implementation ticket's
  PR description explicitly call out the change.

## Follow-up

The follow-up implementation ticket
[INF-TSK-050-013](../tasks/INF-TSK-050-013.md) (newly filed in this
session) will:

- [ ] Split `write_pid_env_file` into fail-loud (`write_pid_env_file
  -> Result`) and best-effort (`try_write_pid_env_file -> ()`).
- [ ] Update all call sites: SessionStart load-bearing writes use the
  fail-loud version; cleanup uses best-effort.
- [ ] Add `verify_env_file_readable` and call it after every
  SessionStart write.
- [ ] Add `SessionError::EnvFileWriteFailed` and
  `SessionError::EnvFileMissing` variants.
- [ ] Update the gate-check / detect-project-dir hook to fail-closed
  on missing env file.
- [ ] Add unit tests for the load-bearing failure modes (mock
  filesystem returning ENOSPC / EACCES).
- [ ] Add an integration test: write to a read-only directory,
  assert SessionStart returns the structured error.

## References

- [Comprehensive Audit (2026-05-03)](../analysis/2026-05-03-autorun-interactive-comprehensive-audit.md), section 4.5.
- [`codeflow-cli/core/src/session/env.rs`](../../../../../codeflow-cli/core/src/session/env.rs).
- [`codeflow-cli/core/src/hooks/session_start.rs`](../../../../../codeflow-cli/core/src/hooks/session_start.rs).
- Memory: `feedback_worktree_path_resolution.md`, `codeflow_env_audit.md` (codeflow-env audit notes).
