---
title: "Unified Session Lifecycle: SessionStart, SessionEnd, and PathFlow Hooks"
type: analysis
status: proposed
date: 2026-03-15
area: infrastructure
supersedes:
  - session-start-redesign.md
  - session-end-cleanup-redesign.md
  - session-lifecycle-interaction.md
scope:
  - codeflow-cli/core/src/hooks/session_start.rs
  - codeflow-cli/core/src/hooks/session_end.rs
  - codeflow-cli/core/src/hooks/post_tool_use.rs
  - codeflow-cli/core/src/hooks/mod.rs
  - codeflow-cli/core/src/session/mod.rs
  - codeflow-cli/core/src/session/env.rs
  - codeflow-cli/cli/src/cmd/hooks/session_start.rs
  - codeflow-cli/cli/src/cmd/hooks/session_end.rs
  - codeflow-cli/cli/src/helpers.rs
---

# Unified Session Lifecycle

This document is the authoritative reference for the SessionStart, SessionEnd,
and PathFlow hook lifecycle. It supersedes the three predecessor analysis
documents (`session-start-redesign.md`, `session-end-cleanup-redesign.md`,
`session-lifecycle-interaction.md`) and incorporates empirical findings from
2026-03-15 testing of tmux vs in-process teammate behavior.

## Table of Contents

- [1. Empirical Findings](#1-empirical-findings)
  - [1.1 Hook events by backend mode](#11-hook-events-by-backend-mode)
  - [1.2 PID process chain for hooks](#12-pid-process-chain-for-hooks)
  - [1.3 teammateMode setting behavior](#13-teammatemode-setting-behavior)
  - [1.4 Env var propagation](#14-env-var-propagation)
  - [1.5 Serde field mapping fix](#15-serde-field-mapping-fix)
- [2. State Files](#2-state-files)
  - [2.1 pathflow-session-status.json](#21-pathflow-session-statusjson)
  - [2.2 pathflow-team.json](#22-pathflow-teamjson)
  - [2.3 codeflow-env.sh](#23-codeflow-envsh)
  - [2.4 session-meta.json (eliminated)](#24-session-metajson-eliminated)
- [3. SessionStart Unified Flow](#3-sessionstart-unified-flow)
- [4. PostToolUse Team Lifecycle](#4-posttooluse-team-lifecycle)
- [5. SessionEnd Unified Flow](#5-sessionend-unified-flow)
- [6. Status File Lifecycle](#6-status-file-lifecycle)
- [7. Team File Lifecycle](#7-team-file-lifecycle)
- [8. Scenario Matrix](#8-scenario-matrix)
- [9. Stale Sweep Enhancement](#9-stale-sweep-enhancement)
- [10. Worktree Future Integration](#10-worktree-future-integration)
- [11. Code Changes](#11-code-changes)
- [12. Related Files](#12-related-files)

---

## 1. Empirical Findings

All findings verified empirically on 2026-03-15 using Claude Code v2.1.76.

[↑ Back to top](#unified-session-lifecycle)

### 1.1 Hook events by backend mode

Claude Code has two teammate backends: **in-process** (teammates run inside the
lead's terminal) and **tmux** (each teammate gets its own tmux pane as a
separate `claude` process). The backend determines which hook events fire.

**In-process mode** (plain terminal, no tmux):

```text
Agent Type               Spawn Event                 Stop Event
─────────────────────────────────────────────────────────────────
Sub-agent (no team)      SubagentStart               SubagentStop
                         agent_type=general-purpose
Named agent (no team)    SubagentStart               SubagentStop
                         agent_type=general-purpose
Teammate (name+team)     SubagentStart               SubagentStop
                         agent_type=teammate_name
```

SessionStart/SessionEnd fire ONLY for the lead. No teammate detection needed.

**Tmux mode** (terminal inside tmux):

```text
Agent Type               Spawn Event                 Stop Event
─────────────────────────────────────────────────────────────────
Sub-agent (no team)      SubagentStart               SubagentStop
                         agent_type=general-purpose
Teammate (name+team)     SessionStart                SessionEnd
                         source=startup, new UUID
                         NO agent_id, NO agent_type
                         Identical payload to lead
```

SessionStart/SessionEnd fire for BOTH lead AND teammates. Teammate detection
IS needed — the hook payload is indistinguishable from a new lead.

[↑ Back to top](#unified-session-lifecycle)

### 1.2 PID process chain for hooks

Claude Code executes hook commands via **direct exec** (no intermediate shell):

```text
Claude Code (PID A, long-lived) → codeflow hooks ... (PID B, ephemeral)
```

Verified by comparing hook `parent_pid` with Bash `$PPID`:

```text
Hook parent_id()  = 62077  (Claude Code PID)
Bash $PPID        = 62077  (same)
```

This means `std::os::unix::process::parent_id()` in the hook process gives the
Claude Code process PID. This PID is:

- **Stable** across compact/resume/clear (same process, context management only)
- **Alive** for the entire session duration
- **Different** between lead and tmux teammate (separate `claude` processes)

Verified with teammate spawn:

```text
Lead hook parent_id()      = 62077  (Lead's Claude Code)
Teammate hook parent_id()  = 67946  (Teammate's Claude Code, different process)
```

**Current bug:** The CLI dispatch uses `std::process::id()` (the hook's own
ephemeral PID, dead after hook exits). Must change to `parent_id()`.

[↑ Back to top](#unified-session-lifecycle)

### 1.3 teammateMode setting behavior

`teammateMode: "in-process"` in settings.json is **ignored** when the terminal
is inside tmux. Claude Code auto-detects tmux and forces tmux backend.

```text
Setting          Terminal        Actual backend
────────────────────────────────────────────────
"in-process"     plain terminal  in-process (correct)
"in-process"     inside tmux     tmux (setting ignored)
"inprocess"      inside tmux     tmux (setting ignored)
```

Env var suppression (`TMUX=""`, `TMUX_PANE=""`, `TERM_PROGRAM=""` in
settings.json `env` section) also fails — Claude Code uses multiple detection
signals. This is a Claude Code bug (docs say the setting should override).

**Implication:** Cannot force in-process mode inside tmux. The hook code must
handle tmux teammates via PID-based detection.

[↑ Back to top](#unified-session-lifecycle)

### 1.4 Env var propagation

Tested whether `CODEFLOW_SESSION_ID` set by the lead propagates to tmux
teammates:

```text
Mechanism                    Process-level env?  Inherited by tmux panes?
──────────────────────────────────────────────────────────────────────────
settings.json "env" section  YES                 YES (but static, not per-session)
CLAUDE_ENV_FILE              NO (Bash shell only) NO (per-session, not inherited)
Hook stdout {"env":{...}}    NO                  NO
```

`CLAUDE_ENV_FILE` is per-session — each tmux teammate gets its own with its own
values. The lead's values are NOT inherited by teammates.

**Implication:** Cannot use env vars for teammate detection. PID-based detection
is the only reliable mechanism.

[↑ Back to top](#unified-session-lifecycle)

### 1.5 Serde field mapping fix

**Critical bug found and fixed:** Claude Code sends stdin JSON with field names
that did not match the Rust `HookInput` struct. ALL hooks were silently failing
(exit 0, graceful degradation) and doing nothing.

```text
Our struct field    Claude Code sends     Fix applied
──────────────────────────────────────────────────────────────────
event               hook_event_name       #[serde(alias = "hook_event_name")]
HookEvent values    PascalCase            #[serde(alias = "SessionStart")] etc.
project_dir         cwd                   #[serde(alias = "cwd")]
```

Impact: All PathFlow enforcement (gate-check, sentinel-write, checkpoint,
team-guard) was completely inactive since the Rust CLI was deployed. Fixed in
`hooks/mod.rs` with serde aliases that accept both formats.

[↑ Back to top](#unified-session-lifecycle)

---

## 2. State Files

[↑ Back to top](#unified-session-lifecycle)

### 2.1 pathflow-session-status.json

**Location:** `.state/session/{SID}/pathflow/pathflow-session-status.json`

Canonical source of truth for session lifecycle state. Sole authority for
`should_skip_cleanup` decisions.

```json
{
  "session_id": "ses-01kp...",
  "status": "pf-in-progress",
  "lead_pid": 62077,
  "team_name": "my-team",
  "last_completed_phase": "pf-4",
  "last_completed_stage": "ws-dev",
  "source_at_start": "startup",
  "latest_source": "compact",
  "latest_source_at": "2026-03-15T12:00:00Z",
  "created_at": "2026-03-15T10:00:00Z",
  "updated_at": "2026-03-15T12:00:00Z"
}
```

| Field | Set by | Purpose |
|-------|--------|---------|
| `session_id` | SessionStart (Section 7) | Session identifier |
| `status` | SessionStart, PostToolUse | Lifecycle state (see [Section 6](#6-status-file-lifecycle)) |
| `lead_pid` | SessionStart (Section 7) | Claude Code PID of lead (`parent_id()`) |
| `team_name` | PostToolUse (TeamCreate) | Team name for config lookup |
| `last_completed_phase` | PostToolUse (checkpoint-complete) | Latest pf-N sentinel |
| `last_completed_stage` | PostToolUse (sentinel-write) | Latest ws-* sentinel |
| `source_at_start` | SessionStart (Section 7) | Original source (startup/resume) |
| `latest_source` | SessionStart (compact/resume/clear path) | Most recent source event |
| `latest_source_at` | SessionStart (compact/resume/clear path) | When latest source occurred |
| `created_at` | SessionStart (Section 7) | File creation time |
| `updated_at` | All writers | Last modification time |

**Status values:** `created` → `pf-started` → `pf-in-progress` → `pf-complete`

[↑ Back to top](#unified-session-lifecycle)

### 2.2 pathflow-team.json

**Location:** `.state/session/{SID}/pathflow/pathflow-team.json`

Team composition and process tracking. Created at TeamCreate, deleted at
TeamDelete.

```json
{
  "team_name": "my-team",
  "codeflow_session_id": "ses-01kp...",
  "lead_pid": 62077,
  "teammate_spawned": true,
  "created_at": "2026-03-15T10:05:00Z",
  "last_spawn_name": "cf-development",
  "teammates": [
    {
      "name": "cf-development",
      "pid": 67946,
      "spawned_at": "2026-03-15T10:10:00Z"
    },
    {
      "name": "cf-review",
      "pid": 68100,
      "spawned_at": "2026-03-15T10:30:00Z"
    }
  ]
}
```

| Field | Set by | Purpose |
|-------|--------|---------|
| `team_name` | handle_team_create | Team identifier |
| `codeflow_session_id` | handle_team_create | Associated session |
| `lead_pid` | handle_team_create | Claude Code PID (copy from status file) |
| `teammate_spawned` | handle_teammate_spawn | Whether any teammate was spawned |
| `created_at` | handle_team_create | Team creation time |
| `last_spawn_name` | handle_teammate_spawn | Most recent teammate name |
| `teammates` | handle_teammate_spawn + teammate SessionStart | Name, PID, spawn time per teammate |

**Teammate PID population:** The lead's PostToolUse (teammate spawn) records
`{ name, pid: 0, spawned_at }`. The teammate's own SessionStart (tmux path,
TEAMMATE detection) updates its entry with `pid: parent_id()`. For in-process
teammates (no SessionStart), `pid` remains 0.

[↑ Back to top](#unified-session-lifecycle)

### 2.3 codeflow-env.sh

**Location:** `.state/runtime/codeflow-env.sh`

Single source of truth for current session ID. Shell-sourceable format.

```bash
export CODEFLOW_SESSION_ID='ses-01kp...'
export CF_PROJECT_ROOT='codeflow'
```

Written atomically (tmp + rename) by SessionStart. Read by SessionEnd
(`current_session_id`), `handle_stale_cleanup`, and external hooks. Race-safe
removal in SessionEnd (SID check before delete).

[↑ Back to top](#unified-session-lifecycle)

### 2.4 session-meta.json (eliminated)

Previously stored `session_id`, `created_at`, `source`, `ppid`, `version`,
`permission_mode`. All useful fields merged into `pathflow-session-status.json`:

- `source` → `source_at_start` + `latest_source` + `latest_source_at`
- `ppid` → `lead_pid` (corrected to Claude Code PID via `parent_id()`)
- `version`, `permission_mode` → not needed (available from process context)

`session-meta.json` creation removed from SessionStart. No backward
compatibility concern — the file was never read by any hook logic.

[↑ Back to top](#unified-session-lifecycle)

---

## 3. SessionStart Unified Flow

This flow handles all scenarios: in-process, tmux, lead, teammate,
compact/resume/clear, and future worktree creation.

```text
SessionStart hook fires
│
│  WHO fires this?
│  ├─ In-process mode: ONLY the lead (teammates fire SubagentStart)
│  └─ Tmux mode: Lead AND each teammate (separate claude processes)
│
▼
Section 0: Acquire session lock (flock on session.lock)
│
▼
Section 1: Read codeflow-env.sh
│
├─ No env file ─────────────────────────────────────► NEW LEAD (A)
│                                                     (no prior session)
│
▼ Has env file → existing_sid
│
├─ source = compact/resume/clear ───────────────────► MINIMAL PATH (B)
│   │
│   │  Reuse existing_sid. No destructive ops.
│   │  Update pathflow-session-status.json:
│   │    → latest_source = {source}
│   │    → latest_source_at = now()
│   │  Read status file → output recovery context
│   │    (last_completed_phase, last_completed_stage, team_name)
│   │  Set CODEFLOW_SESSION_ID = existing_sid
│   │  Release lock
│   │  RETURN
│   │
│   │  Applies to: lead compact, lead resume, lead clear,
│   │              tmux teammate compact (all identical)
│   └───────────────────────────────────────────────────────► EXIT
│
▼ source = startup/unknown
│
Section 1b: Read pathflow-session-status.json for existing_sid
│
├─ File missing ────────────────────────────────────► NEW LEAD (A)
├─ Parse error ─────────────────────────────────────► NEW LEAD (A)
├─ status = "" ─────────────────────────────────────► NEW LEAD (A)
├─ status = "pf-complete" ──────────────────────────► NEW LEAD (A)
│
▼ status = "created" / "pf-started" / "pf-in-progress"
│
├─ team_name empty ─────────────────────────────────► NEW LEAD (A)
│
▼ team_name set
│
Section 1c: Check ~/.claude/teams/{team_name}/config.json
│
├─ Config missing ──────────────────────────────────► NEW LEAD (A)
│                                                     (team dissolved)
│
▼ Config exists → active session detected
│
Section 1d: Read lead_pid from pathflow-session-status.json
│
├─ No lead_pid or lead_pid = 0 ─────────────────────► NEW LEAD (A)
│                                                     (pre-PID session)
│
▼ lead_pid found
│
Section 1e: kill(lead_pid, 0) — is lead process alive?
│
├─ DEAD (ESRCH) ────────────────────────────────────► NEW LEAD (A)
│                                                     (crash recovery)
│
▼ ALIVE — lead is running, caller is a TEAMMATE
│
┌────────────────────────────────────────────────────────────────────┐
│                     TEAMMATE PATH (C)                              │
│                                                                    │
│  is_teammate = true                                                │
│  session_id = existing_sid (lead's SID)                            │
│  Update pathflow-team.json:                                        │
│    → Find teammate entry by name, set pid = parent_id()            │
│  Set CODEFLOW_SESSION_ID = existing_sid                            │
│  Release lock                                                      │
│  Output env JSON                                                   │
│  RETURN                                                            │
│                                                                    │
│  ⛔ Does NOT: generate SID, create dirs, write status file,        │
│     create worktree, sweep stale sessions, init checkpoint         │
└───────────────────────────────────────────────────────────────►EXIT


┌────────────────────────────────────────────────────────────────────┐
│                     NEW LEAD PATH (A)                              │
│                                                                    │
│  Section 2: Generate new SID (ULID-based)                          │
│                                                                    │
│  Section 2b [FUTURE — INF-EPC-023]:                                │
│    Create worktree: Manager.setup_detached("worktree-{SID}")       │
│    → .state/ with shared symlinks (db, ledger, coordination)       │
│    → .state/ with local dirs (runtime, session, sentinels)         │
│    → Set CODEFLOW_WORKTREE_PATH                                    │
│    NOTE: Only reached by NEW LEAD — teammates returned at 1e       │
│                                                                    │
│  Section 2c: Write codeflow-env.sh                                 │
│    → CODEFLOW_SESSION_ID = new SID                                 │
│    → CF_PROJECT_ROOT = project name                                │
│    → [FUTURE: CODEFLOW_WORKTREE_PATH]                              │
│                                                                    │
│  Release session lock                                              │
│                                                                    │
│  Section 3: Create directories                                     │
│    → .state/session/{SID}/pathflow/                                │
│    → .state/sentinels/pathflow/{SID}/                              │
│    → .state/runtime/, .state/ledger/, .state/logs/                 │
│                                                                    │
│  Sections 4+5: Sweep stale sessions                                │
│    For each old ses-* (skip current):                              │
│    ├─ No status file → CLEAN                                      │
│    ├─ pf-complete → CLEAN                                         │
│    ├─ created, no config → CLEAN                                  │
│    ├─ active, no team_name → CLEAN                                │
│    ├─ active, config missing → CLEAN                              │
│    ├─ active, config exists, lead_pid dead → CLEAN                │
│    └─ active, config exists, lead_pid alive → KEEP                │
│    Also: sweep orphan sentinel dirs                                │
│                                                                    │
│  Section 6: Active task cleanup                                    │
│                                                                    │
│  Section 7: Create pathflow-session-status.json                    │
│    → { session_id, status: "created", lead_pid: parent_id(),       │
│        team_name: "", source_at_start: source,                     │
│        latest_source: source, latest_source_at: now(),             │
│        phases/stages empty, timestamps }                           │
│                                                                    │
│  Section 7c: Init checkpoint (pathflow-phase-tasks.json)           │
│                                                                    │
│  Section 9: Detect stale teams (startup only)                      │
│  Section 10: Compact recovery detection                            │
│  Section 11: Create project temp directory                         │
│                                                                    │
│  Output env JSON                                                   │
│  RETURN                                                            │
└───────────────────────────────────────────────────────────────►EXIT
```

[↑ Back to top](#unified-session-lifecycle)

---

## 4. PostToolUse Team Lifecycle

```text
TeamCreate PostToolUse
│
▼
handle_team_create(team_name, session_dir, session_id):
  Create pathflow-team.json:
    { team_name, codeflow_session_id, lead_pid: parent_id(),
      teammate_spawned: false, teammates: [],
      created_at: now(), last_spawn_name: null }
  Update pathflow-session-status.json:
    → status = "pf-started"
    → team_name = {team_name}
    → Clear last_completed_phase/stage


Task/Agent PostToolUse (teammate spawn, fires on LEAD side):
│
▼
handle_teammate_spawn(agent_name, session_dir):
  Update pathflow-team.json:
    → teammate_spawned = true
    → last_spawn_name = agent_name
    → Append to teammates: { name, pid: 0, spawned_at: now() }
      (pid=0 placeholder — tmux teammate updates own PID
       via TEAMMATE PATH when its SessionStart fires;
       in-process teammates never fire SessionStart, pid stays 0)


TaskCompleted → checkpoint-complete:
│
▼
  Update pathflow-session-status.json:
    → status = "pf-in-progress" (on first phase completion)
    → last_completed_phase = "pf-{N}"


SendMessage "STAGE-COMPLETE: WS-{X}" → sentinel-write:
│
▼
  Create sentinel file: pathflow-ws-{stage}
  Update pathflow-session-status.json:
    → last_completed_stage = "ws-{stage}"


TeamDelete PostToolUse:
│
▼
handle_team_delete(session_dir, project_dir, session_id):
  1. Update pathflow-session-status.json:
       → status = "pf-complete"              ← MUST BE FIRST
  2. Remove .state/sentinels/pathflow/{SID}/
  3. Remove pathflow-team.json
  4. Reset pathflow-phase-tasks.json

  Ordering is critical: SessionEnd reads status to decide if
  cleanup is safe. Status must be "pf-complete" BEFORE sentinels
  are removed, or a concurrent SessionEnd could see
  "pf-in-progress" with no sentinels and skip cleanup incorrectly.
```

[↑ Back to top](#unified-session-lifecycle)

---

## 5. SessionEnd Unified Flow

```text
SessionEnd hook fires
│
│  WHO fires this?
│  ├─ In-process mode: ONLY the lead
│  └─ Tmux mode: Lead AND each teammate (separate processes)
│
▼
Section 2: Resolve session ID
│  → session::current_session_id(project_dir)
│  → Reads codeflow-env.sh → lead's SID
│  (Tmux teammate reads SAME env file → gets lead's SID)
│
├─ No SID found → "no session ID, skipping" ────────────► EXIT
│
▼
Section 3: should_skip_cleanup(session_state_dir)
│
│  Read pathflow-session-status.json (sole authority)
│
│  ├─ File missing → ALLOW cleanup
│  ├─ Parse error → ALLOW cleanup
│  ├─ status = "created" → ALLOW (no team, nothing to protect)
│  ├─ status = "pf-complete" → ALLOW (session done)
│  ├─ status = "" → ALLOW
│  ├─ status = "pf-started" / "pf-in-progress":
│  │   ├─ team_name empty → ALLOW
│  │   ├─ config.json missing → ALLOW (team dissolved)
│  │   ├─ last_completed_phase = "pf-7" → ALLOW (stuck status)
│  │   └─ All pass → SKIP (session active, protect it)
│  └─ unknown status → ALLOW
│
├─ SKIP ──► "session active, skipping cleanup" ─────────► EXIT
│           (Teammate exit during active session — correct)
│           (Lead crash-exit — safe, stale sweep next startup)
│
▼ ALLOW
│
Section 4:  PF7 diagnostic (check pf-7 sentinel)
Section 5:  Clean sentinels (.state/sentinels/pathflow/{SID}/)
Section 6:  Active task preservation (keep in_progress)
Section 8:  Read team name from pathflow-team.json
Section 9:  Clean team artifacts (~/.claude/teams/, ~/.claude/tasks/)
              → Re-checks status before deleting (race guard)
Section 10: Clean session state (.state/session/{SID}/)
Section 11: Clean runtime files (env file — race-safe SID check)
Section 12: Clean project temp dir
[FUTURE Section 13: Clean worktree — git worktree remove]
│
▼
EXIT
```

**No changes needed to `should_skip_cleanup`.** The status-based approach works
for both in-process and tmux modes. Teammate exits see active status and skip.
Lead crash-exits also skip (safe — stale sweep handles cleanup on next startup).

[↑ Back to top](#unified-session-lifecycle)

---

## 6. Status File Lifecycle

```text
SessionStart(startup, lead)
  → CREATE { status:"created", lead_pid:PID, source_at_start:"startup" }

TeamCreate PostToolUse
  → UPDATE status="pf-started", team_name="{name}"

Checkpoint-complete (PF1 tasks done)
  → UPDATE status="pf-in-progress", last_completed_phase="pf-1"

Checkpoint-complete (PF{N} tasks done)
  → UPDATE last_completed_phase="pf-{N}"

Sentinel-write (STAGE-COMPLETE)
  → UPDATE last_completed_stage="ws-{stage}"

SessionStart(compact/resume/clear)
  → UPDATE latest_source="{source}", latest_source_at=now()

TeamDelete PostToolUse
  → UPDATE status="pf-complete"

SessionEnd (cleanup allowed)
  → DELETE (removed as part of .state/session/{SID}/ removal)
```

[↑ Back to top](#unified-session-lifecycle)

---

## 7. Team File Lifecycle

```text
TeamCreate PostToolUse
  → CREATE { team_name, lead_pid:PID, teammates:[], ... }

Teammate spawn PostToolUse (lead side)
  → UPDATE teammates += { name, pid:0, spawned_at }

Teammate SessionStart (tmux, teammate side — TEAMMATE PATH)
  → UPDATE teammates[name].pid = parent_id()

TeamDelete PostToolUse
  → DELETE
```

[↑ Back to top](#unified-session-lifecycle)

---

## 8. Scenario Matrix

### SessionStart scenarios

```text
Scenario                          Source    Path       Result
──────────────────────────────────────────────────────────────────────
Fresh lead, no prior session      startup  A (new)    Gen SID, full init
Lead after clean PF7              startup  A (new)    Gen SID, sweep old
Lead after crash                  startup  A (new)    lead_pid dead → new lead
Tmux teammate joins               startup  C (team)   lead_pid alive → teammate
In-process teammate joins         (n/a)    (n/a)      SubagentStart, not SessionStart
Lead compact                      compact  B (min)    Reuse SID, update latest_source
Lead resume (same process)        resume   B (min)    Reuse SID, update latest_source
Lead clear                        clear    B (min)    Reuse SID, update latest_source
Tmux teammate compact             compact  B (min)    Reuse SID
[FUTURE] Lead in worktree         startup  A (new)    Gen SID + create worktree
[FUTURE] Tmux teammate in wt      startup  C (team)   lead_pid alive → no worktree
```

### SessionEnd scenarios

```text
Scenario                          Status        Config  Decision
──────────────────────────────────────────────────────────────────────
Lead after PF7                    pf-complete   gone    ALLOW
Teammate exits (tmux)             active        exists  SKIP
Lead crash-exit                   active        exists  SKIP *
Teammate after TeamDelete         pf-complete   gone    ALLOW
No PathFlow session               (no file)     n/a     ALLOW
[FUTURE] Lead in worktree         pf-complete   gone    ALLOW + worktree cleanup

* Cleaned by stale sweep on next startup (lead_pid dead → CLEAN)
```

[↑ Back to top](#unified-session-lifecycle)

---

## 9. Stale Sweep Enhancement

The stale sweep in `sweep_all_stale_sessions` gains a PID-based check for
sessions where status is active and team config still exists:

```text
For each old ses-* directory (skip current SID):
│
├─ No status file → CLEAN
├─ status = "pf-complete" → CLEAN
├─ status = "created", no config → CLEAN
├─ status = "pf-started" / "pf-in-progress":
│   ├─ team_name empty → CLEAN
│   ├─ config missing → CLEAN
│   ├─ config exists:
│   │   ├─ lead_pid present and alive → KEEP (session genuinely active)
│   │   ├─ lead_pid present and dead → CLEAN (crashed, stale config)
│   │   └─ no lead_pid field → KEEP (pre-PID session, safe default)
```

This fixes the "orphaned session with config" problem — crashed sessions are
cleaned on the first subsequent startup instead of waiting for multiple sweeps
and `detect_stale_teams` to remove the config.

[↑ Back to top](#unified-session-lifecycle)

---

## 10. Worktree Future Integration

From INF-EPC-023 (Parallel Execution Core), each PathFlow session runs in its
own git worktree. The `.state/` directory is split:

```text
Shared (symlinked to main .state/):
  db/            SurrealDB database (WAL mode for concurrent access)
  ledger/        Append-only JSONL (flock for I/O safety)
  coordination/  Loro CRDT state (sole coordination mechanism)
  logs/          Event logs (flock for append safety)
  registry/      Worktree tracking
  backups/       Timestamped DB backups

Per-worktree (local, not shared):
  runtime/       codeflow-env.sh, active-task.json
  session/       pathflow-session-status.json, pathflow-team.json, checkpoint
  sentinels/     Phase and stage sentinels
```

**Worktree creation ordering:** Section 2b (future) runs AFTER teammate
detection (Section 1) and BEFORE directory creation (Section 3). Teammates
return early at Section 1e and never reach Section 2b — preventing accidental
worktree creation for tmux teammates.

**Within a worktree:** The lead and its teammates all work in the SAME worktree.
The lead creates the worktree at SessionStart. Tmux teammates detect as
TEAMMATE (lead_pid alive) and join the existing worktree. In-process teammates
don't fire SessionStart at all.

**Parallel sessions:** Each session has its own worktree with its own
`codeflow-env.sh`, `pathflow-session-status.json`, and sentinels. No cross-
session interference — the critical singletons are eliminated by per-worktree
scoping.

[↑ Back to top](#unified-session-lifecycle)

---

## 11. Code Changes

### Files to modify

```text
File                                        Change
──────────────────────────────────────────────────────────────────────────
cli/src/cmd/hooks/session_start.rs          ppid: process::id() → parent_id()
core/src/hooks/mod.rs                       Extend PathflowTeamInfo + add TeammateEntry
                                            (serde aliases already applied)
core/src/hooks/session_start.rs             PID check in handle_stale_cleanup
                                            lead_pid + source fields in status file
                                            Remove SAFEGUARD block
                                            PID check in sweep_all_stale_sessions
                                            Eliminate session-meta.json creation
                                            Update compact/resume/clear path
                                              (latest_source, latest_source_at)
core/src/hooks/post_tool_use.rs             lead_pid + teammates in handle_team_create
                                            Teammate entry in handle_teammate_spawn
core/src/hooks/session_end.rs               No changes to should_skip_cleanup
```

### Files to delete/clean

```text
File                                        Action
──────────────────────────────────────────────────────────────────────────
.claude/hooks/project/hook-pid-logger.sh    Delete (temporary test hook)
.claude/hooks/project/hook-env-setter.sh    Delete (temporary test hook)
.claude/hooks/project/hook-event-logger.sh  Delete or keep for diagnostics
settings.json                               Remove pid-logger hook entry
                                            Remove TMUX/TMUX_PANE/TERM_PROGRAM env overrides
```

### SAFEGUARD block removal

Lines 324-360 of `resolve_or_generate_session_id` — the SAFEGUARD block that
re-checks for active sessions before generating a new SID. This duplicates
`handle_stale_cleanup` logic and has the same false-positive bug (concludes
"teammate" for a new lead after crash when status is active + config exists).
With PID-based detection in `handle_stale_cleanup`, the SAFEGUARD is
unnecessary and harmful. Remove entirely.

[↑ Back to top](#unified-session-lifecycle)

---

## 12. Related Files

### Predecessor analysis documents (superseded)

| File | Relevance |
|------|-----------|
| `session-start-redesign.md` | SessionStart unified flow (superseded by Section 3) |
| `session-end-cleanup-redesign.md` | SessionEnd shouldSkipCleanup redesign (superseded by Section 5) |
| `session-lifecycle-interaction.md` | Full start-to-end lifecycle timeline (superseded by Sections 3-7) |
| `session-startup-cleanup-redesign.md` | Earlier Go CLI startup bug fixes (historical reference) |

### Parallel work analysis

| File | Relevance |
|------|-----------|
| `parallel-work/worktree-architecture.md` | Worktree state split, singleton scoping |
| `parallel-work/decisions.md` | 23 architectural decisions for parallel execution |
| `parallel-work/autorun-integration.md` | Autorun worker worktree model |

### Implementation files

| File | Relevance |
|------|-----------|
| `codeflow-cli/core/src/hooks/session_start.rs` | SessionStart handler implementation |
| `codeflow-cli/core/src/hooks/session_end.rs` | SessionEnd handler implementation |
| `codeflow-cli/core/src/hooks/post_tool_use.rs` | Team lifecycle handlers |
| `codeflow-cli/core/src/hooks/mod.rs` | HookInput, HookEvent, PathflowTeamInfo types |
| `codeflow-cli/core/src/session/mod.rs` | Session ID generation, env file operations |
| `codeflow-cli/core/src/session/env.rs` | Env file read/write/remove |
| `codeflow-cli/cli/src/cmd/hooks/session_start.rs` | CLI dispatch (ppid assignment) |
| `codeflow-cli/cli/src/cmd/hooks/session_end.rs` | CLI dispatch |
| `codeflow-cli/cli/src/helpers.rs` | Hook handler runner, stdin parsing |

[↑ Back to top](#unified-session-lifecycle)
