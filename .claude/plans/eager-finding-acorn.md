# Plan: PathFlow Enforcement + CLAUDE.md + Settings Cleanup

## Context

Phase 5 (slash commands) complete. This task addresses a critical enforcement gap:
PathFlow is entirely advisory -- all three enforcement dependencies are missing, so
Claude can (and does) bypass the entire phase lifecycle.

**Root cause (3 missing dependencies):**

1. **Flag never created:** `is_pathflow_active()` checks `.state/session/{SID}/is-pathflow-active`
   but no code ever creates this file → always returns false → pathflow-gate exits 0 (allow all).
   Source: `context-lib.sh:162-166`

2. **JSONL events never written:** pathflow-gate reads `phase_transition` events from
   `pathflow-events.jsonl` but no one calls the existing scripts (`cf-pathflow-phase-transition.sh`
   etc.) → no events found → gate exits 0 (allow all).
   Source: `cf-pre-tool-use-pathflow-gate.sh:165-186`

3. **Sentinel directory empty:** Session-start creates `.state/sentinels/pathflow/{SID}/` and
   session-end cleans it, but no code creates sentinel FILES in it.
   Source: `cf-session-start-cleanup.sh:111`, `cf-session-end-cleanup.sh:111-115`

**Result:** ALL hooks that check `is_pathflow_active()` (pathflow-gate, team-guard, stop) exit 0
immediately. PathFlow enforcement is instruction-only (CLAUDE.md text).

---

## Design Decisions

### D1: File sentinels, not JSONL for enforcement

The pathflow-gate currently reads JSONL `phase_transition` events for phase checking.
This requires:

- cf-knowledge-layer to call `cf-pathflow-phase-transition.sh` at each phase boundary
- JSONL file to exist and contain events
- jq to parse events for the current session

**Instead:** Use file-based sentinels (simple `touch` in the existing directory).

| Criterion | File Sentinels | JSONL Events |
|-----------|---------------|--------------|
| Creation | `touch file` (atomic, always works) | `jq -nc ... >> file` (needs jq) |
| Check | `[[ -f file ]]` (fast, no parsing) | `tail + jq select` (slow, fragile) |
| Dependency | None | jq, correct JSON, session filtering |
| Writer | PostToolUse hook (automatic) | cf-knowledge-layer teammate (fragile) |
| Cleanup | `rm -rf dir/` (session-end already does this) | Events persist in ledger |

**JSONL stays as-is.** The existing JSONL infrastructure (pathflow scripts, ledger) is
preserved. When cf-knowledge-layer starts writing events, the JSONL-based check in
pathflow-gate continues to work as a secondary mechanism. Both approaches coexist.

### D2: All state via hooks (not teammate instructions)

Flag and sentinel creation/removal is handled by hooks (automatic, reliable),
NOT by teammate instructions (fragile, ignorable).

| State | Created By | Removed By |
|-------|-----------|------------|
| pathflow-active flag | SessionStart hook | SessionEnd hook (session dir cleanup) |
| Sentinel files | PostToolUse hook | SessionEnd hook (sentinel dir cleanup) |

### D3: PathFlow tasks vs Claude Code tasks

- **PathFlow tasks** (`PFx-TSK-XX`): Managed by cf-knowledge-layer via JSONL/SQLite.
  Defined in `pathflow-config.json`. Session-scoped, created at phase entry.
  These are NOT related to hooks or sentinel creation.
- **Claude Code tasks** (`TaskCreate`/`TaskUpdate`): Claude's internal todo list.
  Used for team coordination. Completely separate from PathFlow tasks.
- **Sentinel hooks** interact with NEITHER task system. They observe tool events
  and create files. Simple, decoupled, reliable.

### D4: Task tool vs Teammate tool (distinct tools)

Claude Code has two SEPARATE tools for agent creation:

| Tool | Purpose | Hook Matcher | LLM Access |
|------|---------|-------------|------------|
| **Task** | Spawn sub-agents (ephemeral, read-only) | None (not hooked at PreToolUse) | Direct (in tool list) |
| **Teammate** | Spawn + manage teammates (persistent, team member) | `"Teammate"` in PreToolUse hooks | Indirect (LLM calls Task with team_name, Claude Code routes to Teammate) |

**Evidence:** settings.json line 361 has `"matcher": "Teammate"` for the team-guard
PreToolUse hook. The team-guard checks `tool_name == "Teammate"` (line 58) and
`tool_name == "TeamDelete"` (line 53) — two distinct tools from "Task".

### D6: PostToolUse hook visibility (EMPIRICALLY VALIDATED)

**Test methodology:** Created a dedicated PostToolUse hook script and wired it into
settings.local.json with explicit matcher `TeamCreate|TeamDelete|Teammate|SendMessage|Task`.
Then executed: TeamCreate → spawn teammate (Task with team_name) → SendMessage →
shutdown_request → TeamDelete. Compared results against the `.*` matcher.

**Finding:** The `.*` regex matcher does NOT match team/communication tools.
But **explicit matchers DO work**. Team tools fire PostToolUse — they just need
to be named explicitly in the matcher.

| Operation | tool_name in PostToolUse | `.*` catches it? | Explicit matcher catches it? |
|-----------|-------------------------|-------------------|------------------------------|
| Create team | `TeamCreate` | NO | **YES** |
| Spawn teammate | `Task` | NO | **YES** |
| Send message | `SendMessage` | NO | **YES** |
| Shutdown request | `SendMessage` | NO | **YES** |
| Shutdown response | `SendMessage` | NO | **YES** |
| Delete team | `TeamDelete` | NO | **YES** |

**Key details:**

- Teammate spawn appears as `tool_name=Task` (NOT "Teammate")
- The `"Teammate"` tool name is only seen by PreToolUse hooks (team-guard)
- Shutdown request/response both appear as `SendMessage`
- All team tools require explicit naming in matcher — `.*` is insufficient

**Impact on sentinel design — ALL sentinels are now hook-automatic:**

| Sentinel | Detection | Matcher Needed | Confidence |
|----------|-----------|---------------|------------|
| pf-1 | tool_name=`TeamCreate` | `TeamCreate` | **HIGH** ✓ |
| pf-2 | tool_name=`Task`, input has cf-knowledge-layer | `Task` | **HIGH** ✓ |
| pf-3 | tool_name=`Bash`, command has git checkout -b | `Bash` (already in `.*`) | **HIGH** ✓ |
| ws-* | tool_name=`SendMessage`, content has STAGE-COMPLETE | `SendMessage` | **HIGH** ✓ |
| pf-6 | tool_name=`Bash`, command has gh pr create | `Bash` (already in `.*`) | **HIGH** ✓ |

**Sentinel hook matcher:** `TeamCreate|Task|SendMessage|Bash`
(Bash is already caught by `.*`, but including it for clarity. TeamDelete not
needed — no sentinel on team deletion.)

**Full documentation:** `.codeflow/docs/research/agent-teams-hook-findings.md`

### D5: Tracked → Untracked transition

From CLAUDE.md: "An untracked session can become tracked. The reverse does not happen."

**Edge case:** User changes mind mid-session ("actually, never mind, I don't want to fix that").

**Current handling (sufficient):**

- Session stays tracked → user can say "let's end this session"
- Lead triggers PF7-END → SessionEnd hook removes flag + sentinels → clean state
- No code change needed — `/cf-cleanup` or natural session end handles this
- The pathflow-active flag is per-session, so a new session starts clean

**Deferred:** A `/cf-abandon` command to fast-track from any phase to PF7-END
without creating a PR. This is a nice-to-have for the Go CLI, not needed now.

---

## Part A: PathFlow Enforcement Hardening

### The Flag

- **Path:** `.state/session/{SESSION_ID}/is-pathflow-active`
- **Type:** File existence check (`touch` to create, `rm` to remove)
- **Scope:** Per-session (each session gets own directory)
- **Created by:** SessionStart pathflow-init hook (automatic, every session)
- **Removed by:** SessionEnd cleanup hook (removes session directory)
- **Consumers:** pathflow-gate, team-guard, stop hooks (via `is_pathflow_active()`)

### A1: NEW SessionStart hook creates flag

**File:** `.claude/hooks/codeflow/session-start/cf-session-start-pathflow-init.sh` (NEW)

A dedicated hook for PathFlow initialization, separate from cleanup concerns.
The cleanup hook (`cf-session-start-cleanup.sh`) handles session directory creation
and stale state removal. This new hook handles PathFlow-specific initialization.

**What it does:**

1. Reads session_id from stdin (same pattern as cleanup hook)
2. Sources cf-pathflow-state.sh library
3. Creates the flag file with JSON metadata

**Flag file format** (per pathflow-v3 spec Section 8.3.1, updated for V4 JSON):

```json
{
  "session_id": "ses-17711717382560a1b2c",
  "team_name": "",
  "created_at": "2026-02-15T10:30:00.000Z",
  "tracking_level": "pending"
}
```

- `session_id`: From stdin JSON (Claude Code provides it)
- `team_name`: Empty at init, updated when TeamCreate is called (or left empty)
- `created_at`: ISO 8601 timestamp
- `tracking_level`: Starts as "pending" (matches revamp proposal v2 Section 3.2)

**Why `is_pathflow_active()` still works:** The function at context-lib.sh:164 checks
`[[ -f "$flag_file" ]]` — file existence only. JSON content is metadata for debugging
and audit, not for the gate check itself.

**Hook registration:** Add to SessionStart hooks in all 4 templates + project settings.
Runs AFTER cleanup hook (cleanup creates the directories, this creates the flag).

**SessionEnd cleanup:** The existing session-end hook removes the entire session dir
(`.state/session/{SID}/`), which removes the flag file automatically.

### A2: Pathflow state library (NEW)

**File:** `.codeflow/scripts/state/cf-pathflow-state.sh`

Functions for hooks to use (source guard, like cf-work-state.sh pattern):

```bash
# Flag operations
create_pathflow_flag()   # touch .state/session/{SID}/is-pathflow-active
remove_pathflow_flag()   # rm -f .state/session/{SID}/is-pathflow-active

# Sentinel operations
create_sentinel(name)    # touch .state/sentinels/pathflow/{SID}/pathflow-{name}
has_sentinel(name)       # [[ -f .state/sentinels/pathflow/{SID}/pathflow-{name} ]]
list_sentinels()         # ls .state/sentinels/pathflow/{SID}/pathflow-*
```

All functions use `CODEFLOW_SESSION_ID` and `REPO_ROOT` from environment.
Follows cf-work-state.sh patterns: source guard, readonly constants, jq-free.

### A3: PostToolUse sentinel hook (NEW)

**File:** `.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-pathflow-sentinel.sh`
**Matcher:** `TeamCreate|Task|SendMessage|Bash` (explicit — `.*` does NOT catch team tools)

Reads PostToolUse stdin (tool_name, tool_input) and detects phase-transition events:

| Tool Event | Sentinel Created | Detection Logic | Confidence |
|------------|-----------------|-----------------|------------|
| `TeamCreate` called | `pathflow-pf-1` | `tool_name == "TeamCreate"` | **HIGH** (empirically verified) |
| Teammate spawns cf-knowledge-layer | `pathflow-pf-2` | `tool_name == "Task"` AND input.name contains "cf-knowledge-layer" | **HIGH** (empirically verified: spawns show as `Task`) |
| `Bash`: git branch creation | `pathflow-pf-3` | `tool_name == "Bash"` AND command matches `git (checkout -b\|switch -c)` | **HIGH** (CRITICAL gate) |
| `SendMessage`: STAGE-COMPLETE | `pathflow-ws-{stage}` | `tool_name == "SendMessage"` AND input.content matches `STAGE-COMPLETE: WS-*` | **HIGH** (empirically verified) |
| `Bash`: gh pr create | `pathflow-pf-6` | `tool_name == "Bash"` AND command matches `gh pr create` | **HIGH** |

**Sentinel path:** `.state/sentinels/pathflow/{SESSION_ID}/pathflow-{name}`
**Idempotent:** `touch` on existing file is a no-op.
**Early exit:** If `is_pathflow_active()` returns false, exit 0 (no sentinels needed).

**Empirically confirmed tool_name values (tested 2026-02-15):**

- TeamCreate → `tool_name="TeamCreate"`
- Teammate spawn via Task(team_name=...) → `tool_name="Task"` (NOT "Teammate")
- SendMessage (including shutdown) → `tool_name="SendMessage"`
- TeamDelete → `tool_name="TeamDelete"`
- `"Teammate"` tool_name only appears in PreToolUse hooks (team-guard matcher)

**STAGE-COMPLETE extraction:** When tool_input.content matches
`STAGE-COMPLETE: WS-(DEV|REV|QA|TEST|PLAN|DOCS)`, create sentinel
`pathflow-ws-{stage-lowercase}` (e.g., `pathflow-ws-dev`, `pathflow-ws-rev`).

**How teammate tool calls are detected:** Teammates share the same session as the lead.
PostToolUse hooks fire for ALL tool calls in the session, including teammate calls.
When cf-development calls `SendMessage(content="STAGE-COMPLETE: WS-DEV")`, this hook
fires and creates the sentinel.

### A4: Pathflow-gate tightening

**File:** `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh`

**Current flow (broken):**

```text
is_pathflow_active()? → false → exit 0 (allow all)  ← ALWAYS EXITS HERE
... JSONL phase lookup (never reached) ...
```

**New flow (with sentinel-first check):**

```text
1. Tool is Edit/Write/Bash?  No → exit 0 (allow)
2. Bash but not git commit/push/PR?  → exit 0 (allow)
3. is_pathflow_active()?  No → exit 0 (not in PathFlow, allow all)
4. Source cf-pathflow-state.sh
5. Gate check:
   - Edit/Write or git commit → has_sentinel("pf-3")? Yes → ALLOW. No → BLOCK (exit 2)
   - git push/PR → has_sentinel("ws-rev")? Yes → ALLOW. No → BLOCK (exit 2)
6. Fallback: existing JSONL phase lookup (kept for when cf-knowledge-layer writes events)
```

**Key change:** Replace the `current-session-id` + JSONL lookup section (lines 152-213)
with sentinel file checks. Keep JSONL lookup as commented fallback.

**Block message (stderr, exit 2):**

```text
BLOCKED: PathFlow gate - prerequisite not met
Reason: Edit/Write requires PF3-CLASSIFY (branch creation). No pathflow-pf-3 sentinel.
Complete earlier phases before this operation.
Current sentinels: pathflow-pf-1, pathflow-pf-2
```

### A5: Agent definition updates (STAGE-COMPLETE protocol)

Add to Communication section of 5 on-demand agent definitions:

| Agent | Message on Completion |
|-------|----------------------|
| cf-development | `"STAGE-COMPLETE: WS-DEV"` in final SendMessage to lead |
| cf-review | `"STAGE-COMPLETE: WS-REV"` in final SendMessage to lead |
| cf-quality-assurance | `"STAGE-COMPLETE: WS-QA"` or `"STAGE-COMPLETE: WS-TEST"` |
| cf-documentation | `"STAGE-COMPLETE: WS-DOCS"` in final SendMessage to lead |
| cf-planning | `"STAGE-COMPLETE: WS-PLAN"` in final SendMessage to lead |

**Files:** `.claude/agents/cf-{development,review,quality-assurance,documentation,planning}.md`

Format: Add to existing Communication/Messaging section:

```markdown
### Stage Completion Protocol
When your work stage is complete, include `STAGE-COMPLETE: WS-{STAGE}` in your
final message to the team lead. This triggers automatic sentinel creation for
PathFlow enforcement.
```

### A6: Register new hooks in settings and templates

**Two new hooks to register** in all 4 templates + 2 project settings (6 files):

1. **SessionStart** — PathFlow init hook (runs AFTER cleanup hook):

```json
{
  "type": "command",
  "command": "bash .claude/hooks/codeflow/session-start/cf-session-start-pathflow-init.sh"
}
```

2. **PostToolUse** — Sentinel creation hook:

```json
{
  "matcher": "TeamCreate|Task|SendMessage|Bash",
  "hooks": [{
    "type": "command",
    "command": "bash .claude/hooks/codeflow/post-tool-use/cf-post-tool-use-pathflow-sentinel.sh"
  }]
}
```

**Why not `.*`:** Empirically verified that `.*` does NOT match team/communication
tools (TeamCreate, Task, SendMessage, TeamDelete). Explicit naming required.

### A7: Comprehensive update of revamp proposal v2

**File:** `.codeflow/docs/analysis/codeflow-revamp-proposal-v2.md`

This is NOT a simple section addition. The document needs a full review and holistic
update to resolve inconsistencies introduced by the enforcement hardening decisions.

**Inconsistencies to fix:**

1. **Line 440:** "No mode detection needed, no pathflow-active flag file" → INCORRECT.
   Flag IS needed. Created by SessionStart hook. `is_pathflow_active()` checks it.

2. **Section 3.3 (Sentinel comparison):** Says PathFlow sentinels are "JSONL markers
   checked by pathflow-gate." Now file sentinels are PRIMARY, JSONL is SECONDARY.

3. **Section 6.2 (PathFlow-Gate):** Describes JSONL-only enforcement. Needs update
   to show sentinel-first checking with JSONL fallback.

4. **Section 3.7 (Config simplification):** References agent_teams config that we're
   removing in Part D. Needs consistency.

5. **Hook disposition table (Section 5.4):** Missing the new PostToolUse sentinel hook.

**New content to add:**

6. **NEW section:** "PathFlow Enforcement Mechanism" — comprehensive flow showing the
   full session lifecycle from session start to session end:
   - SessionStart hook creates pathflow-active flag (JSON metadata)
   - PostToolUse sentinel hook detects phase-transition events and creates sentinels
   - PreToolUse pathflow-gate blocks Edit/Write before PF3 sentinel exists
   - PreToolUse team-guard blocks TeamDelete while flag exists
   - Stage completion messages (STAGE-COMPLETE) create work stage sentinels
   - PR gate checks ws-rev sentinel before allowing gh pr create
   - SessionEnd hook removes flag + sentinel directory (clean state)
   Written as a permanent design reference with ASCII flow diagram (similar to
   the walkthrough at the end of this plan, lines 504-571).

7. **STAGE-COMPLETE protocol:** Document the message format and its role in automatic
   sentinel creation via PostToolUse hook.

8. **File sentinels vs JSONL sentinels:** Design decision rationale (why file sentinels
   as primary enforcement, with JSONL as secondary for when cf-knowledge-layer matures).

9. **Tracked → Untracked considerations:** Note that once tracked, a session stays
   tracked. User can abandon via PF7-END. `/cf-abandon` deferred to Go CLI.

---

## Part B: CLAUDE.md Improvements (8 changes)

CLAUDE.md is CRITICAL-protected (cf-pre-tool-use-protected-resource.sh blocks
Edit/Write). Must use Python via Bash to modify.

B1: Quick-Reference Phase Map table (Section 4, after Phase Progression)
B2: Token-Aware Delegation subsection (Section 4, under Team Lead Role)
B3: Exploration/Research Routing (Section 6, after Command routing)
B4: Ad-Hoc Teammate Spawn Examples (Section 5, expand existing table)
B5: Scenario Navigator Breadcrumbs (Section 3, after initialization sequence)
B6: Inline Breadcrumbs (Section 4 subsections → "see Section 5 for spawn patterns")
B7: PF1-INIT Flag Creation docs (Section 4 Phase table, add flag to PF1 outputs)
B8: Section 10 Constraints Table (add token-heavy delegation row)

---

## Part C: Instruction Files

### C1: New delegation-only.txt

**File:** `.codeflow/config/instructions/delegation-only.txt`

```text
🔒 DELEGATION-ONLY: You are an orchestrator. ALL work goes through teammates.
⛔ FORBIDDEN: Edit/Write files, git commit/push, run tests, create docs.
⛔ FORBIDDEN: Reading large files (>50 lines) — delegate to Explore sub-agent or teammate.
Permitted: Read files for quick verification, TeamCreate, SendMessage, TaskCreate.
```

### C2: Update workflow-adherence.txt

**File:** `.codeflow/config/instructions/workflow-adherence.txt`
**Change:** Add token-aware delegation clause.

### C3: Update session-start.txt

**File:** `.codeflow/config/instructions/session-start.txt`
**Change:** Add breadcrumbs to CLAUDE.md sections + note about enforcement gate.

### C4: Register delegation-only in instructions-config.json

**File:** `.codeflow/config/instructions/instructions-config.json`
**Change:** Add `delegation-only` entry to `hooks.UserPromptSubmit`.

---

## Part D: Settings Cleanup

### D1: Remove `_verify_work_config`

Stale config block. No hook reads it. Present in all 4 templates + 2 project settings.

### D2: Remove `_read_delegation_config`

Stale config block. References non-existent agent name. Same 6 files.

### D3: Remove `_codeflow.agent_teams` / `pathflow_mode` config

Dead code: `is_pathflow_active()` only checks flag file, never reads settings.json.
`get_pathflow_setting()` in context-lib.sh reads it but nothing calls that function.
**Keep:** `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` env var (Claude Code platform toggle).

### D4-D5: Update templates and sync

Update all 4 templates (strict, standard, permissive, autonomous) with:

- Stale config removal (D1-D3)
- New sentinel PostToolUse hook (A6)

Sync project settings:

- `strict.json → settings.json`
- `autonomous.json → settings.local.json`

---

## Files to Modify (~22 files)

**Part A — PathFlow Enforcement (4 new/modified hooks + 1 library):**

```text
CREATE  .codeflow/scripts/state/cf-pathflow-state.sh
CREATE  .claude/hooks/codeflow/session-start/cf-session-start-pathflow-init.sh
CREATE  .claude/hooks/codeflow/post-tool-use/cf-post-tool-use-pathflow-sentinel.sh
MODIFY  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh
MODIFY  .codeflow/docs/analysis/codeflow-revamp-proposal-v2.md  (comprehensive review)
```

**Part A — Agent Definitions (5 files):**

```text
MODIFY  .claude/agents/cf-development.md
MODIFY  .claude/agents/cf-review.md
MODIFY  .claude/agents/cf-quality-assurance.md
MODIFY  .claude/agents/cf-documentation.md
MODIFY  .claude/agents/cf-planning.md
```

**Part C — Instructions (4 files):**

```text
CREATE  .codeflow/config/instructions/delegation-only.txt
MODIFY  .codeflow/config/instructions/workflow-adherence.txt
MODIFY  .codeflow/config/instructions/session-start.txt
MODIFY  .codeflow/config/instructions/instructions-config.json
```

**Part D — Settings (6 files):**

```text
MODIFY  .claude/settings-templates/strict.json
MODIFY  .claude/settings-templates/standard.json
MODIFY  .claude/settings-templates/permissive.json
MODIFY  .claude/settings-templates/autonomous.json
MODIFY  .claude/settings.json
MODIFY  .claude/settings.local.json
```

**Part B — CLAUDE.md (1 file, via Python/Bash):**

```text
MODIFY  .claude/CLAUDE.md
```

---

## Implementation Order

1. `cf-pathflow-state.sh` — state library (flag + sentinel functions)
2. `cf-session-start-pathflow-init.sh` — NEW hook for flag creation at session start
3. `cf-post-tool-use-pathflow-sentinel.sh` — automatic sentinel creation
4. `cf-pre-tool-use-pathflow-gate.sh` — sentinel-based enforcement
5. Agent definitions — STAGE-COMPLETE message protocol (5 files)
6. Instruction files + config — delegation-only, breadcrumbs (4 files)
7. Settings templates + sync — stale removal + new hook (6 files)
8. CLAUDE.md — 8 improvements (via Python/Bash)
9. `codeflow-revamp-proposal-v2.md` — record design decisions
10. Verification

---

## Enforcement Flow Walkthrough (Session Lifecycle)

```text
SESSION START
│
├─ SessionStart hooks fire (in order):
│  ├─ cf-session-start-cleanup.sh (existing):
│  │  ├─ Creates .state/session/{SID}/
│  │  ├─ Creates .state/sentinels/pathflow/{SID}/
│  │  └─ Cleans stale sessions (>24h)
│  │
│  └─ cf-session-start-pathflow-init.sh (NEW):
│     └─ Creates .state/session/{SID}/is-pathflow-active  ← FLAG (JSON metadata)
│        └─ is_pathflow_active() now returns TRUE
│
├─ Lead reads CLAUDE.md → TeamCreate
│  └─ PostToolUse fires (tool_name=TeamCreate)
│     └─ Sentinel hook creates: pathflow-pf-1              ← SENTINEL
│
├─ PF1-INIT: Lead spawns cf-security (Teammate tool internally)
│  └─ PostToolUse fires (tool_name=Teammate or Task)
│     └─ name != "cf-knowledge-layer" → no new sentinel
│
├─ PF2-CONTEXT: Lead spawns cf-knowledge-layer (Teammate tool internally)
│  └─ PostToolUse fires (tool_name=Teammate or Task)
│     └─ name matches "cf-knowledge-layer" → creates: pathflow-pf-2
│
├─ GATE TEST: Lead tries Edit/Write (BEFORE PF3)
│  └─ PreToolUse pathflow-gate fires
│     ├─ is_pathflow_active()? → TRUE ✓ (flag exists)
│     ├─ has_sentinel("pf-3")? → FALSE ✗
│     └─ exit 2 → BLOCKED! ✓ (enforcement works)
│
├─ PF3-CLASSIFY: cf-git-operations runs Bash(git checkout -b feat/...)
│  └─ PostToolUse fires (tool_name=Bash)
│     └─ Command matches git branch creation → creates: pathflow-pf-3
│
├─ GATE TEST: Edit/Write now attempted
│  └─ PreToolUse pathflow-gate fires
│     ├─ is_pathflow_active()? → TRUE
│     ├─ has_sentinel("pf-3")? → TRUE ✓
│     └─ exit 0 → ALLOWED! ✓
│
├─ PF4-EXECUTE: Work pipeline runs
│  ├─ WS-DEV: cf-development works, sends "STAGE-COMPLETE: WS-DEV"
│  │  └─ PostToolUse → creates: pathflow-ws-dev
│  ├─ WS-REV: cf-review works, sends "STAGE-COMPLETE: WS-REV"
│  │  └─ PostToolUse → creates: pathflow-ws-rev
│  └─ WS-QA: cf-quality-assurance works, sends "STAGE-COMPLETE: WS-QA"
│     └─ PostToolUse → creates: pathflow-ws-qa
│
├─ PF5-VERIFY: Lead verifies (no hookable event, instruction-driven)
│
├─ GATE TEST: Bash(gh pr create) attempted
│  └─ PreToolUse pathflow-gate fires
│     ├─ has_sentinel("ws-rev")? → TRUE ✓ (review completed)
│     └─ exit 0 → ALLOWED! ✓
│
├─ PF6-COMPLETE: cf-git-operations creates PR
│  └─ PostToolUse → creates: pathflow-pf-6
│
├─ PF7-END: Shutdown + cleanup
│  └─ SessionEnd hook fires
│     ├─ Removes .state/sentinels/pathflow/{SID}/ (all sentinels)
│     └─ Removes .state/session/{SID}/ (flag + state)
│
SESSION END (clean state)
```

---

## Verification

1. **Existing test suite passes** — `./codeflow test` (1,555+ tests)
2. **JSON validation** — all settings/config files parse cleanly
3. **Flag creation** — SessionStart creates `is-pathflow-active` in session dir
4. **Sentinel creation** — PostToolUse creates `pathflow-pf-3` on git branch
5. **Gate enforcement** — pathflow-gate blocks Edit when pf-3 missing, allows when present
6. **PR gate** — pathflow-gate blocks `gh pr create` when ws-rev missing
7. **CLAUDE.md** — has all 8 new additions
8. **Settings consistency** — hooks identical across all 4 templates
9. **Agent defs** — all 5 on-demand agents have STAGE-COMPLETE protocol
10. **Session-end cleanup** — flag and sentinels removed at session end

---

## Deferred to Go CLI

- Session ID auto-generation (currently from hook stdin)
- JSONL event logging automation (currently via cf-knowledge-layer)
- Phase transition state machine (currently instruction + hook driven)
- Rework loop automation (currently instruction-driven)
- codeflow CLI commands: session create, sentinel create, next-phase, status
- Full sentinel lifecycle management (Go CLI replaces shell state library)
- JSONL-primary enforcement (currently file sentinels are primary, JSONL secondary)
