# Part 8: Enforcement Model

> The three-mechanism model: Instructions (flow logic) + Tasks (visibility) + Hooks (enforcement). Sentinel system, TaskUpdate interception, verify-work team-mode bypass, team cleanup guard, and graceful degradation.

---

## Table of Contents

- [8.1 Three-Mechanism Model](#81-three-mechanism-model)
- [8.2 PathFlow Sentinels](#82-pathflow-sentinels)
- [8.3 Sentinel Types: PathFlow vs Skill](#83-sentinel-types-pathflow-vs-skill)
  - [8.3.1 Mode Detection Mechanism](#831-mode-detection-mechanism)
  - [8.3.2 Coexistence Model](#832-coexistence-model)
- [8.4 Sentinel Creation Flow](#84-sentinel-creation-flow)
- [8.5 Which Sentinels Are Needed](#85-which-sentinels-are-needed)
- [8.6 The PathFlow Gate Hook](#86-the-pathflow-gate-hook)
  - [8.6.1 Team Cleanup Guard](#861-team-cleanup-guard)
- [8.7 TaskUpdate Interception](#87-taskupdate-interception)
- [8.8 Verify-Work in Team Mode](#88-verify-work-in-team-mode)
- [8.9 Graceful Degradation](#89-graceful-degradation)
- [8.10 Defense-in-Depth Summary](#810-defense-in-depth-summary)
- [8.11 Scripts Requiring Mode-Awareness Updates](#811-scripts-requiring-mode-awareness-updates)

---

## 8.1 Three-Mechanism Model

PathFlow enforcement uses three complementary mechanisms. No single mechanism is sufficient alone; together they provide defense-in-depth.

```
+-------------------+     +-------------------+     +-------------------+
|   INSTRUCTIONS    |     |      TASKS        |     |      HOOKS        |
|   (Flow Logic)    |     |   (Visibility)    |     |   (Enforcement)   |
+-------------------+     +-------------------+     +-------------------+
|                   |     |                   |     |                   |
| Agent definitions |     | Claude Task graph |     | PreToolUse hooks  |
| Spawn prompts     |     | Phase markers     |     | PostToolUse hooks |
| CLAUDE.md rules   |     | blockedBy deps    |     | Stop hooks        |
| Lead orchestration|     | TaskList for all  |     | Sentinel checks   |
|                   |     |                   |     |                   |
+-------------------+     +-------------------+     +-------------------+
        |                         |                         |
        v                         v                         v
  "Teammates follow       "Everyone can see         "Hard guardrails
   the process because     where the session        that catch violations
   they're told to"        is and what's blocked"   even if instructions
                                                     are ignored"
```

**Why three mechanisms?**

| Mechanism | Strength | Weakness |
|-----------|----------|----------|
| Instructions | Guides agent behavior proactively | Agents can ignore or misinterpret |
| Tasks | Makes state visible to all agents | Dependencies are advisory only (not enforced) |
| Hooks | Hard enforcement at tool-call level | Cannot guide behavior, only block violations |

**Design principle**: Instructions handle the happy path. Tasks provide shared awareness. Hooks catch edge cases where the first two fail.

---

## 8.2 PathFlow Sentinels

PathFlow sentinels are file-based markers created when a phase or work stage completes. They are checked by hooks before allowing operations that require a phase to have completed.

**Properties**:

| Property | Value |
|----------|-------|
| Location | `.state/sentinels/pathflow:{phase-id}` |
| Scope | Session-scoped (valid for entire session) |
| TTL | None (no expiry) |
| Created by | PostToolUse hook on TaskUpdate of phase markers |
| Checked by | `cf-pre-tool-use-pathflow-gate.sh` |
| Cleaned up by | SessionEnd hook |
| Format | Plain text: phase ID, timestamp, session ID |

**Sentinel file example**:

```
# .state/sentinels/pathflow:pf-3
phase=pf-3
timestamp=2026-02-07T14:30:00Z
session_id=session-01JK8...
```

---

## 8.3 Sentinel Types: PathFlow vs Skill

Two types of sentinels coexist in CodeFlow. They serve different purposes and operate in different session contexts.

```
+------------------------+     +------------------------+
|   PATHFLOW SENTINELS   |     |    SKILL SENTINELS     |
+------------------------+     +------------------------+
| Session-scoped         |     | TTL-based (600s)       |
| No expiry              |     | Expire after timeout   |
| Created on phase       |     | Created on skill       |
|   marker completion    |     |   invocation           |
| Used in team sessions  |     | Used in non-team       |
| Checked by pathflow-   |     |   sessions (backward   |
|   gate hook            |     |   compatibility)       |
| Fewer needed (phases   |     | Granular per-skill     |
|   cover many skills)   |     |   markers              |
+------------------------+     +------------------------+
```

| Aspect | PathFlow Sentinels | Skill Sentinels |
|--------|-------------------|-----------------|
| Naming | `pathflow:{phase}` | `{skill-name}:{operation}` |
| Example | `pathflow:pf-3` | `cf-task-management:ensure-work-registered` |
| TTL | None (session-scoped) | 600 seconds |
| Created when | Phase marker completed | Skill operation invoked |
| Session type | Team sessions | Non-team sessions |
| Quantity needed | ~3-5 for full session | ~8-12 per workflow |

**Key simplification**: In team mode, `pathflow:pf-3` (work classified and registered) implicitly covers what multiple skill sentinels would check. If PF-3 completed, work IS registered, work IS classified. No need for separate `ensure-work-registered` and `classify-work` sentinels.

### 8.3.1 Mode Detection Mechanism

Hooks need a reliable way to determine whether the current session is running in team mode (PathFlow active) or non-team mode (skill sentinels only). The mechanism uses an explicit state flag file.

**Detection logic**:

```
if [ -f "/tmp/claude/managed/state/pathflow-active" ]; then
    # Team mode: use PathFlow sentinels
else
    # Non-team mode: use skill sentinels (backward compatible)
fi
```

**Lifecycle**:

| Event | Action |
|-------|--------|
| PF-1 (Session Initialization) | Creates `/tmp/claude/managed/state/pathflow-active` |
| During session | Hooks check for file existence to determine mode |
| PF-7 (Session End) / SessionEnd hook | Removes `/tmp/claude/managed/state/pathflow-active` |

**Flag file contents**:

```
# /tmp/claude/managed/state/pathflow-active
session_id=session-01JK8...
team_name=my-project
created_at=2026-02-07T14:00:00Z
```

**Why this approach over alternatives**:

| Alternative | Problem |
|-------------|---------|
| Check `~/.claude/teams/*/config.json` | Fragile -- team config may persist after a session ends, causing false positives in subsequent non-team sessions |
| Environment variable | Shell state does not persist between Bash tool calls in Claude Code |
| Check TaskList for phase markers | Expensive (requires API call), racy, and phase markers may not exist yet at session start |
| Explicit state flag file (chosen) | Created/destroyed by PathFlow lifecycle, accurately reflects current session state, located in `/tmp/claude/managed/state/` which is session-scoped and cleaned up automatically |

The explicit state flag is the most reliable approach because it is directly tied to PathFlow's own lifecycle. When PathFlow initializes (PF-1), it creates the flag. When PathFlow tears down (PF-7 or SessionEnd), it removes it. There is no ambiguity about whether a team session is active.

### 8.3.2 Coexistence Model

In any given session, one sentinel type is PRIMARY and the other is either absent or ignored.

```
Team Mode (pathflow-active exists):
  PathFlow sentinels = PRIMARY (created and checked)
  Skill sentinels    = CREATED (backward compat) but NOT checked
  Gate hook uses     = pathflow_active() -> PathFlow sentinel checks

Non-Team Mode (pathflow-active absent):
  PathFlow sentinels = DO NOT EXIST
  Skill sentinels    = PRIMARY (created and checked)
  Gate hook uses     = skill sentinel checks (existing behavior)
```

**Rules**:

1. **Team mode**: PathFlow sentinels are the authority. Skill sentinels are still created by existing hooks (backward compatibility) but the gate hook does NOT check them. This avoids TTL-expiry false negatives during long team sessions.
2. **Non-team mode**: Skill sentinels are the authority. PathFlow sentinels are never created because no PathFlow lifecycle runs. Existing hook behavior is completely unchanged.
3. **Storage**: Both types stored in `.state/sentinels/` with different prefixes (`pathflow:` vs skill name).
4. **Cleanup**: SessionEnd cleanup removes BOTH types regardless of mode, ensuring a clean slate.

---

## 8.4 Sentinel Creation Flow

Sentinels are created automatically via the PostToolUse hook when a phase marker is completed.

```
                                                    +------------------+
Team Lead calls:                                    |  PostToolUse     |
  TaskUpdate(task_id=#7,                            |  Hook            |
             status="completed")                    +------------------+
        |                                                  |
        v                                                  |
  Claude Task #7                                           |
  subject: "[PF-3] Work Classification"                    |
  status: completed                                        |
        |                                                  |
        +----------------> Hook inspects subject ------->  |
                           Matches "[PF-" pattern?         |
                                  |                        |
                                 YES                       |
                                  |                        |
                                  v                        |
                           Extract phase: "pf-3"           |
                           Create file:                    |
                             .state/sentinels/pathflow:pf-3|
                                  |                        |
                                  v                        |
                           Log: "Sentinel created:         |
                                 pathflow:pf-3"            |
                                                           |
                           Work stage markers too:         |
                           "[WS-DEV-DONE]"  -> pathflow:ws-dev-done
                           "[WS-REV-DONE]"  -> pathflow:ws-rev-done
                           "[WS-QA-DONE]"   -> pathflow:ws-qa-done
                           "[WS-WORK-DONE]" -> pathflow:ws-work-done
```

**Implementation note**: The hook parses the TaskUpdate tool_use result to find the task subject. If the subject matches `[PF-N]` or `[WS-*-DONE]` patterns and the new status is `completed`, the sentinel is created.

---

## 8.5 Which Sentinels Are Needed

In team mode, PathFlow sentinels replace many individual skill sentinels. Here are the sentinels that actually matter:

### PathFlow Sentinels (Team Mode)

| Sentinel | Created When | Checked Before | Purpose |
|----------|-------------|----------------|---------|
| `pathflow:pf-3` | PF-3 completed | Edit, Write operations | Prevent coding before work is classified and registered |
| `pathflow:ws-dev-done` | Dev stage done | Review stage operations | Ensure dev is complete before review starts |
| `pathflow:ws-rev-done` | Review stage done | QA stage operations | Ensure review passed before QA starts |
| `pathflow:ws-work-done` | Work stage done (non-code) | Review stage (non-code pipeline) | Ensure WORK stage is complete before REVIEW in non-code workflows |

### Existing Sentinels (Still Needed)

| Sentinel | Purpose | Still Needed? | Notes |
|----------|---------|:------------:|-------|
| `complete-work` before commit | Prevent committing unfinished work | Yes | cf-gitops checks before committing |
| `sandbox-check` before push | Prevent unauthorized network access | Yes | Hook enforcement for network ops |

### Sentinels Made Redundant by PathFlow

| Old Sentinel | Replaced By | Reasoning |
|-------------|-------------|-----------|
| `cf-task-management:ensure-work-registered` | `pathflow:pf-3` | PF-3 includes work registration |
| `cf-task-management:classify-work` | `pathflow:pf-3` | PF-3 includes work classification |
| `cf-memory-management:begin-work` | `pathflow:pf-3` | PF-3 includes active_work creation |
| `cf-working-protocol:verify-work` | WS-REV stage | Review stage replaces PCV |

---

## 8.6 The PathFlow Gate Hook

A new PreToolUse hook (`cf-pre-tool-use-pathflow-gate.sh`) enforces that certain operations cannot proceed without the required PathFlow sentinel.

**Gate rules**:

| Tool | Condition | Required Sentinel |
|------|-----------|-------------------|
| Edit | Always | `pathflow:pf-3` |
| Write | Always | `pathflow:pf-3` |
| Bash (git commit) | Command matches `git commit` | `pathflow:pf-3` + `complete-work` |

**Hook logic (pseudocode)**:

```
on PreToolUse(tool, input):
  if tool in [Edit, Write]:
    if pathflow_active():
      # Team mode: check PathFlow sentinel
      if not sentinel_exists("pathflow:pf-3"):
        BLOCK: "Work must be classified (PF-3) before editing files."
    else:
      # Non-team mode: fall back to skill sentinel check
      check_skill_sentinel("cf-task-management:ensure-work-registered")

function pathflow_active():
  return file_exists("/tmp/claude/managed/state/pathflow-active")
```

**Mode detection**: The hook calls `pathflow_active()` which checks for the existence of `/tmp/claude/managed/state/pathflow-active` (see [Section 8.3.1](#831-mode-detection-mechanism)). This flag file is created during PF-1 and removed during PF-7/SessionEnd, so it accurately reflects whether a PathFlow team session is currently active. If the flag does not exist, the hook falls back to skill sentinel checks for backward compatibility.

### 8.6.1 Team Cleanup Guard

A PreToolUse hook (`cf-pre-tool-use-team-guard.sh`) prevents accidental team dissolution during an active PathFlow session.

| Aspect | Detail |
|--------|--------|
| Hook | `cf-pre-tool-use-team-guard.sh` |
| Matcher | `Teammate` tool |
| Logic | Blocks `Teammate(operation="cleanup")` when `pathflow-active` flag exists |
| Purpose | Prevents accidental team dissolution, which destroys the entire Claude Task list and all phase/work-stage tracking |

**Why this matters**: The Claude Task list is the shared coordination backbone for all PathFlow phases and work stages. If a teammate (or the lead) accidentally calls `Teammate(operation="cleanup")`, the team is destroyed along with all tasks. This guard ensures cleanup can only happen when PathFlow is not active (i.e., after PF-7 tears down the session and removes the `pathflow-active` flag).

See [10-session-lifecycle.md](10-session-lifecycle.md), Section 10.11 for the team lifecycle invariant that this guard enforces.

---

## 8.7 TaskUpdate Interception

Beyond sentinel checks, the enforcement model can intercept `TaskUpdate` calls to validate workflow ordering. This uses Claude Code's settings.json matcher configuration.

**Settings matcher for TaskUpdate**:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "TaskUpdate",
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-task.sh"
          }
        ]
      }
    ]
  }
}
```

**What the hook validates**:

1. If the task being updated has unresolved `blockedBy` dependencies, warn (advisory)
2. If the task is a phase marker being completed out of order, warn (advisory)
3. Log the TaskUpdate for audit purposes

**Advisory, not blocking**: Task dependency enforcement in the hook is advisory because the Claude Task system itself does not enforce dependencies. Hard blocking could deadlock the workflow if the dependency graph has an error. See [Section 8.9](#89-graceful-degradation) for the reasoning.

---

## 8.8 Verify-Work in Team Mode

In PathFlow v3 team mode, the `cf-stop-verify-work.sh` Stop hook's PCV (Post-Completion Verification) behavior is **bypassed**. The hook file itself remains, but when the `pathflow-active` flag exists, it skips PCV checking entirely -- the WS-REV (Review) work stage replaces that function. In non-team mode (no `pathflow-active` flag), the hook continues to check for PCV markers as before.

### What verify-work Did

The old hook (`cf-stop-verify-work.sh`) fired at stop time and checked whether the agent's last message contained a PCV (Post-Completion Verification) marker. It was advisory-only (Stop hooks always exit 0) and checked for:

- A verification emoji marker
- The text "verify-work" or "VERIFY-WORK"
- Tier-specific sections (ARTIFACTS, VERIFICATION, ADVERSARIAL)

### Why It Is Removed

| Problem | Detail |
|---------|--------|
| Stop-time is too late | By the time the agent stops, the work is "done." Catching issues at stop time means they go unaddressed. |
| Advisory only | Stop hooks cannot block. The check was purely informational. |
| Self-verification is weak | The same agent that wrote the code was verifying it. This is like grading your own homework. |
| Autorun incompatible | In autorun mode, there is no human to see the advisory. The check was invisible. |

### What Replaces It

The WS-REV (Review) stage provides:

| Aspect | Old (verify-work) | New (WS-REV) |
|--------|-------------------|---------------|
| When | Stop time (after all work) | After dev stage (before QA) |
| Who | Same agent (self-review) | Separate cf-reviewer agent |
| Enforcement | Advisory only | Blocks QA stage until approved |
| Autorun | Invisible | Automated reviewer with rework routing |
| Rework | Cannot trigger rework | Routes back to WS-DEV on failure |
| Auditability | Last-message text matching | Full stage_history in WorkGraph |

### The Autorun Check

In autorun mode, the Review stage can use a fast model (Haiku) for initial automated review:

1. cf-reviewer spawns with instructions to check acceptance criteria
2. Uses Haiku-class model for fast, cost-effective review
3. If acceptance criteria are met: verdict = approved
4. If criteria not met: verdict = changes_requested, routes to rework
5. Rework limits prevent infinite loops (see [07-work-stages.md](07-work-stages.md), Section 7.7)

---

## 8.9 Graceful Degradation

If the enforcement system becomes unreliable (sentinel files cannot be created, hooks fail, etc.), PathFlow degrades gracefully rather than breaking the session.

**Degradation path**:

```
Full enforcement (nominal):
  Instructions + Tasks + Hooks all active
  Sentinels created and checked
  TaskUpdate interception active
         |
         | (sentinel creation fails)
         v
Partial enforcement:
  Instructions + Tasks active
  Hooks fire but sentinel checks skip (missing file)
  TaskUpdate interception logs warnings
  Advisory messages to lead: "Sentinel creation failed for PF-3"
         |
         | (hooks fail entirely)
         v
Advisory only:
  Instructions + Tasks active
  No hook enforcement
  Lead orchestration + task graph provide ordering
  Session proceeds but without guard rails
```

**Design philosophy**: A development session should never be BLOCKED by an enforcement system failure. The enforcement system exists to catch mistakes, not to be a gating prerequisite for work. If enforcement degrades:

1. Log the degradation for post-session analysis
2. Continue the session with available mechanisms
3. The lead's instructions and task graph still provide ordering

---

## 8.10 Defense-in-Depth Summary

For a typical operation like "cf-developer edits a file during WS-DEV":

```
Layer 1: INSTRUCTIONS
  cf-developer's agent definition says:
  "Only modify files relevant to your assigned task"
  "You must be in a DEV stage to write code"
  -> Agent follows instructions (happy path)

Layer 2: TASKS
  The Edit task is blocked by [WS-DEV] marker
  cf-developer can see in TaskList that WS-DEV is active
  -> Agent is aware of correct ordering

Layer 3: HOOKS
  cf-pre-tool-use-pathflow-gate.sh checks:
    Does pathflow:pf-3 sentinel exist? YES -> allow
  cf-pre-tool-use-edit-write.sh checks:
    Is the file within task scope? YES -> allow
  -> System enforces even if agent ignores instructions

All three layers agree: ALLOW the edit.
```

For a violation like "cf-developer tries to edit before PF-3 completes":

```
Layer 1: INSTRUCTIONS
  Agent definition says wait for assignment
  -> Agent should not try this (but might)

Layer 2: TASKS
  The Edit task is blocked by [PF-3] marker
  TaskList shows PF-3 is still pending
  -> Agent should see it's blocked (but might ignore)

Layer 3: HOOKS
  cf-pre-tool-use-pathflow-gate.sh checks:
    Does pathflow:pf-3 sentinel exist? NO -> BLOCK
  -> "Work must be classified (PF-3) before editing files."
  -> Hard stop. Edit prevented.
```

---

## 8.11 Scripts Requiring Mode-Awareness Updates

The following existing scripts must be made mode-aware: they need to check the `pathflow-active` flag (see [Section 8.3.1](#831-mode-detection-mechanism)) and behave differently in team vs non-team mode.

### Hooks Needing Updates

| Hook | Path | Current Behavior | Team Mode Change |
|------|------|-----------------|------------------|
| Post-tool-use skill sentinel | `.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-skill.sh` | Creates sentinels after skill invocation | In team mode: also create PathFlow sentinels when phase markers complete |
| Pre-tool-use bash sentinel | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-bash-sentinel.sh` | Checks sentinels before bash commands | In team mode: check PathFlow sentinels instead of/in addition to skill sentinels |
| Pre-tool-use file sentinel | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-file-sentinel.sh` | Checks sentinels for file operations | In team mode: check PathFlow sentinels |
| Pre-tool-use task sentinel | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-task-sentinel.sh` | Checks task context | In team mode: check PathFlow phase markers |
| Pre-tool-use grep sentinel | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-grep-sentinel.sh` | Grep sentinel checks | In team mode: may need PathFlow awareness |
| Session start cleanup | `.claude/hooks/codeflow/session-start/cf-session-start-cleanup.sh` | Sentinel cleanup at session start | Must clean both PathFlow and skill sentinels |
| Session end cleanup | `.claude/hooks/codeflow/session-end/cf-session-end-cleanup.sh` | Sentinel cleanup at session end | Must clean both sentinel types AND remove the `pathflow-active` flag |

### Security Scripts Needing Updates

| Script | Path | Team Mode Change |
|--------|------|------------------|
| Core sentinel library | `.codeflow/scripts/security/sentinel/cf-sentinel.sh` | Needs to support both PathFlow and skill sentinel types (create, check, remove) |
| Security helper library | `.codeflow/scripts/security/lib/security-lib.sh` | Sentinel helper functions need mode-aware sentinel checking via `pathflow_active()` |

### Post-Tool-Use Hooks

| Hook | Path | Team Mode Change |
|------|------|------------------|
| Memory/progress hook | `.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-memory-progress.sh` | References sentinels; may need PathFlow sentinel awareness for progress tracking |

### Implementation Pattern

All scripts listed above should follow the same mode-detection pattern:

```bash
# Source the shared mode detection function
source "${CODEFLOW_ROOT}/.codeflow/scripts/security/lib/security-lib.sh"

if pathflow_active; then
    # Team mode: use PathFlow sentinels
    check_pathflow_sentinel "pathflow:pf-3"
else
    # Non-team mode: use skill sentinels (existing behavior)
    check_skill_sentinel "cf-task-management:ensure-work-registered"
fi
```

The `pathflow_active` function should be defined once in `security-lib.sh` and sourced by all scripts that need it, avoiding duplication.

---

## Related Documents

- [06-progressive-orchestration.md](06-progressive-orchestration.md) -- Phase markers that create sentinels
- [07-work-stages.md](07-work-stages.md) -- Work stages that sentinels enforce
- [09-knowledge-layer-integration.md](09-knowledge-layer-integration.md) -- Data tier for stage tracking
- [10-session-lifecycle.md](10-session-lifecycle.md) -- Session boundary and cleanup
- [12-changes-and-claude-components.md](12-changes-and-claude-components.md) -- Hook changes and verify-work removal
