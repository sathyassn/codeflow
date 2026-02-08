# PathFlow Use Case Analysis

> Full pathway traversal walkthroughs for four primary work types, showing how the Outer Shell PathFlow and Inner Pathways handle each scenario end-to-end.

**Date**: 2026-02-06
**Status**: Design
**Depends On**: 04-outer-shell-pathflow.md, 05-skill-to-teammate-model.md

---

## Overview

This document walks through four concrete use cases, showing exactly how PathFlow nodes are traversed, which sentinels are created, which teammates are involved, and how enforcement adapts. Each use case follows the same Outer Shell (OS-1 through OS-9) but demonstrates different session profiles, team compositions, inner pathways, and enforcement levels.

**Use Cases**:
1. Structured epic-driven development (the "full pipeline")
2. Unstructured ad-hoc bug fix (the "quick fix")
3. Autorun batch execution (the "CI worker")
4. Exploratory research/investigation (the "explorer")

For each use case, the walkthrough includes:
- Scenario context (who, what, why)
- Session profile configuration
- Node-by-node traversal with concrete data
- Sentinel creation timeline
- Team composition and teammate lifecycle
- What enforcement blocks and what passes through
- Edge cases and recovery paths

---

## Use Case 1: Structured Epic-Driven Development

### Scenario

A developer invokes Claude Code to implement a login form validation feature. An epic already exists from a prior planning session: `FRT-EPC-FEAT-AUTH-003 "Authentication Redesign"`. The task `FRT-TSK-FEAT-AUTH-012 "Implement login form validation"` has been pre-created. The developer runs `/cf-develop FRT-TSK-FEAT-AUTH-012`.

### Session Profile

```json
{
  "session_profile": "structured",
  "team_strategy": "standard",
  "resume_behavior": "auto-resume-matching",
  "verification_tier": "force-tier-2",
  "auto_pr": "always",
  "auto_push": "always",
  "meta_awareness": "minimal",
  "checkpoint_interaction": "interactive"
}
```

### Team Composition

```
Team Lead (PERSISTENT)
  |-- cf-developer (PERSISTENT) -- primary implementer
  |-- cf-qa (ON-DEMAND) -- spawned after implementation for testing
  |-- cf-ops (ON-DEMAND) -- spawned for PR creation
```

### Node-by-Node Traversal

#### OS-1: Session Boot

| Aspect | Detail |
|--------|--------|
| **Trigger** | User runs `claude` |
| **Hooks** | `cf-session-start-cleanup.sh` removes 3 expired sentinels from previous session. `cf-session-start-instructions.sh` displays minimal instructions (structured profile). `cf-session-start-logging.sh` creates session log. |
| **Session ID** | `session-01JKB7N3QWERTY` |
| **Sentinel** | `pathflow:shell:boot` created (TTL: 14400s) |
| **Duration** | ~1 second (automatic) |

#### OS-2: Context Load

| Aspect | Detail |
|--------|--------|
| **Trigger** | Automatic after OS-1 |
| **Active work detection** | Finds `FRT-TSK-FEAT-AUTH-012` in `active_work` table (status: `in_progress` from planning session that created it) |
| **Resume decision** | Auto-resume (structured profile + matching task) |
| **Context loaded** | Epic `FRT-EPC-FEAT-AUTH-003` details, task description with acceptance criteria, branch `feat/frt-auth-redesign` |
| **Meta-awareness** | Minimal display: "Resuming FRT-TSK-FEAT-AUTH-012: Implement login form validation" |
| **UserPromptSubmit hooks** | `cf-user-prompt-submit-context.sh` injects branch awareness for `feat/frt-auth-redesign`. `cf-user-prompt-submit-logging.sh` logs the `/cf-develop` command. |
| **Sentinel** | `pathflow:shell:context` created (TTL: 14400s) |
| **Duration** | ~2 seconds |

#### OS-3: Team Init

| Aspect | Detail |
|--------|--------|
| **Team strategy** | `standard` (from profile) |
| **Team name** | `cf-FRT-AUTH-012` (derived from task ID) |
| **Teammates spawned** | cf-developer (persistent, using Strategy A: instruct-to-read blueprint) |
| **Deferred spawns** | cf-qa and cf-ops will be spawned on-demand later |
| **Spawn prompt** | Team preamble + "Read `.claude/agents/cf-developer.md`" + Task assignment for FRT-TSK-FEAT-AUTH-012 + Branch constraint `feat/frt-auth-redesign` |
| **Verification** | Read team config to confirm cf-developer is registered |
| **Sentinel** | `pathflow:shell:team-init` created (TTL: 14400s) |
| **Duration** | ~5 seconds (teammate spawn + blueprint read) |

#### OS-4: Work Registration

| Aspect | Detail |
|--------|--------|
| **Classification** | Already classified from task metadata: area=FRT, type=FEAT, domain=AUTH |
| **Task registration** | Task `FRT-TSK-FEAT-AUTH-012` already exists. Verify task_id NOT NULL: PASS |
| **begin-work** | Active work entry updated with current session ID. Work claim created on `src/components/LoginForm.tsx`, `src/utils/validation.ts` |
| **Inner pathway selected** | `development.json` (from `/cf-develop` command + `feat/*` branch pattern) |
| **Checkpoint gate** | task_id=FRT-TSK-FEAT-AUTH-012 (NOT NULL), inner pathway=development. PASS. |
| **Sentinel** | `pathflow:shell:work-reg` created (TTL: 14400s) |
| **Duration** | ~3 seconds |

#### OS-5: Inner Pathway (development.json)

The inner pathway instantiates the development graph. The team lead creates Agent Teams tasks for each inner node:

**Inner Pathway Task Graph**:

| Inner Node | Task Subject | blockedBy | Agent Role |
|-----------|-------------|-----------|------------|
| DEV-init | Initialize development context | (none) | team-lead |
| DEV-plan | Plan implementation approach | DEV-init | team-lead |
| DEV-impl | Implement login form validation | DEV-plan | cf-developer |
| DEV-test | Write and run tests | DEV-impl | cf-qa |
| DEV-final | Finalize development work | DEV-test | team-lead |

**Execution flow**:

1. **DEV-init** (team-lead): Lead verifies branch exists, checks existing code in `src/components/LoginForm.tsx`. Creates inner sentinel `pathflow:development:init`. Duration: ~30 seconds.

2. **DEV-plan** (team-lead): Lead outlines implementation approach: add validation to `LoginForm.tsx`, create `validation.ts` utility, add inline error display. Communicates plan to cf-developer via SendMessage. Creates inner sentinel `pathflow:development:plan`. Duration: ~1 minute.

3. **DEV-impl** (cf-developer): The core work node. Developer receives task assignment.
   - Reads existing `LoginForm.tsx` (PreToolUse hooks validate: work registered? YES. File claimed? YES.)
   - Creates `src/utils/validation.ts` with email regex and password length check
   - Modifies `LoginForm.tsx` to call validators and display inline errors
   - PostToolUse hooks fire after each Edit/Write (progress tracking, claim renewal)
   - Heartbeat: claim renewal at tool-call 10, 20, 30
   - Developer self-tests by reading the diff
   - Developer marks DEV-impl task complete, sends summary to lead
   - Lead creates inner sentinel `pathflow:development:impl`
   - Duration: ~15-25 minutes, ~40 tool calls

4. **DEV-test** (cf-qa): Lead spawns cf-qa on-demand.
   - Spawn prompt: team preamble + blueprint load + "Write tests for login form validation"
   - QA reads `cf-qa.md` blueprint
   - QA reads the implementation diff from cf-developer
   - QA creates `src/components/__tests__/LoginForm.test.tsx`
   - QA runs tests via Bash: `npm test -- --testPathPattern=LoginForm`
   - Tests pass (6/6)
   - QA reports: "All 6 tests pass. Coverage: LoginForm 92%, validation.ts 100%."
   - QA marks task complete
   - Lead creates inner sentinel `pathflow:development:test`
   - Lead sends shutdown_request to cf-qa. QA approves and exits.
   - Duration: ~5-10 minutes

5. **DEV-final** (team-lead): Lead reviews overall changes, verifies acceptance criteria from task description are met:
   - [x] Email format validation: YES (regex in validation.ts)
   - [x] Password minimum 8 chars: YES (length check in validation.ts)
   - [x] Inline error messages: YES (error spans in LoginForm.tsx)
   - Creates inner sentinel `pathflow:development:final`
   - Duration: ~2 minutes

**Sentinel** | `pathflow:shell:inner-complete` created (TTL: 3600s)
**Total OS-5 duration**: ~25-40 minutes

#### OS-6: Post-Work Finalize

| Aspect | Detail |
|--------|--------|
| **complete-work** | Archives work context. Creates commit sentinel (skill sentinel, TTL 600s). |
| **create-commit** | PreToolUse hook validates: complete-work sentinel exists? YES. Commit: `feat(auth): implement login form validation with inline errors` |
| **sandbox-check** | Creates network sentinel (skill sentinel, TTL 600s). |
| **sync-remote** | `git push -u origin feat/frt-auth-redesign`. Requires sandbox-check sentinel: PRESENT. |
| **create-pull-request** | Lead spawns cf-ops on-demand. Ops creates PR against `main` with description linking to task. PR URL: `https://github.com/org/repo/pull/42`. Ops marks task complete, shuts down. |
| **Record progress** | Final progress event: "Implementation complete. PR #42 created." |
| **Teammate shutdown** | cf-qa already shut down. cf-ops shuts down after PR. cf-developer remains (persistent, available for rework). |
| **Sentinel** | `pathflow:shell:finalize` created (TTL: 600s) |
| **Duration** | ~3-5 minutes |

#### OS-7: Work Verification

| Aspect | Detail |
|--------|--------|
| **PCV tier** | Tier 2 (forced by structured profile) |
| **Verification output** | ARTIFACTS: `src/utils/validation.ts` (new), `src/components/LoginForm.tsx` (modified), `src/components/__tests__/LoginForm.test.tsx` (new). VERIFICATION: Tests 6/6 pass, PR #42 created, acceptance criteria met. |
| **Stop hook** | `cf-stop-verify-work.sh` validates: Tier 2 output present? YES. ARTIFACTS section? YES. VERIFICATION section? YES. Decision: ALLOW. |
| **Sentinel** | `pathflow:shell:verified` created (TTL: 300s) |
| **Duration** | ~1 minute |

#### OS-8: Session Close

| Aspect | Detail |
|--------|--------|
| **Teammate shutdown** | Send shutdown_request to cf-developer (last persistent teammate). Developer approves. |
| **Team cleanup** | `Teammate.cleanup()` removes team `cf-FRT-AUTH-012` and task directories. |
| **SessionEnd hooks** | `cf-session-end-cleanup.sh` archives session state. `cf-session-end-logging.sh` finalizes log. |
| **Sentinel** | `pathflow:shell:close` created (TTL: 60s) |
| **Duration** | ~5 seconds |

#### OS-9: Session End

| Aspect | Detail |
|--------|--------|
| **Session record** | Status: `completed`. Duration: ~35 minutes. |
| **Context summary** | "Implemented login form validation. PR #42. Tests passing." |
| **Cleanup** | All `pathflow:shell:*` sentinels for this session will expire or be cleaned next session. |

### Sentinel Timeline

```
T+0s     pathflow:shell:boot           (OS-1)
T+1s     pathflow:shell:context        (OS-2)
T+6s     pathflow:shell:team-init      (OS-3)
T+9s     pathflow:shell:work-reg       (OS-4)
T+40s    pathflow:development:init     (OS-5, inner)
T+100s   pathflow:development:plan     (OS-5, inner)
T+1500s  pathflow:development:impl     (OS-5, inner)
T+2100s  pathflow:development:test     (OS-5, inner)
T+2220s  pathflow:development:final    (OS-5, inner)
T+2220s  pathflow:shell:inner-complete (OS-5)
T+2280s  pathflow:shell:finalize       (OS-6)
T+2340s  pathflow:shell:verified       (OS-7)
T+2345s  pathflow:shell:close          (OS-8)
T+2346s  [session ends]               (OS-9)
```

### What Enforcement Blocked

1. **PreToolUse sentinel check**: If cf-developer tried to `git commit` before `complete-work`, the bash-sentinel hook would BLOCK (exit 2).
2. **PreToolUse sentinel check**: If cf-ops tried to `git push` before `sandbox-check`, the bash-sentinel hook would BLOCK.
3. **Pathway gate**: If any teammate tried to execute inner pathway node work before its predecessor sentinel existed, the pathflow-gate hook would BLOCK.
4. **Stop hook**: If the lead tried to stop the session before PCV Tier 2 output was produced, the stop hook would BLOCK (JSON decision: block).

---

## Use Case 2: Unstructured Ad-Hoc Bug Fix

### Scenario

A developer opens Claude Code to fix a quick bug: "The submit button on the settings page is not disabled while saving." No epic exists. No task exists. This is an ad-hoc fix.

### Session Profile

```json
{
  "session_profile": "unstructured",
  "team_strategy": "solo",
  "resume_behavior": "never-resume",
  "verification_tier": "force-tier-1",
  "auto_pr": "never",
  "auto_push": "prompt",
  "meta_awareness": "minimal",
  "checkpoint_interaction": "auto-evaluate"
}
```

### Team Composition

```
Team Lead (PERSISTENT) -- operates solo, no teammates
```

### Node-by-Node Traversal

#### OS-1: Session Boot

| Aspect | Detail |
|--------|--------|
| **Hooks** | Standard cleanup, minimal instructions, logging. |
| **Session ID** | `session-01JKC2P8ABCDEF` |
| **Sentinel** | `pathflow:shell:boot` created |
| **Duration** | ~1 second |

#### OS-2: Context Load

| Aspect | Detail |
|--------|--------|
| **Active work detection** | Finds nothing (no in-progress work). |
| **Resume** | Skipped (never-resume profile). |
| **Meta-awareness** | Minimal: "Ready for new work." |
| **UserPromptSubmit** | User types: "Fix the submit button on settings page -- it should be disabled while saving." Hooks inject context. |
| **Sentinel** | `pathflow:shell:context` created |
| **Duration** | ~1 second |

#### OS-3: Team Init (SKIPPED)

| Aspect | Detail |
|--------|--------|
| **Decision** | `team_strategy: solo` -- skip team init. |
| **Sentinel** | `pathflow:shell:team-init` created with `skipped: true` metadata |
| **Audit** | "Team init skipped: solo session profile" logged to JSONL |
| **Duration** | ~0 seconds (no team operations) |

#### OS-4: Work Registration

| Aspect | Detail |
|--------|--------|
| **Classification** | Auto-detect: area=FRT (frontend, from "settings page"), type=FIX, domain=GENL (no specific domain) |
| **Epic** | No existing epic. Create ongoing epic: `FRT-EPC-FIX-GENL-001 "Frontend Bug Fixes (Ongoing)"` (lazy creation, `is_ongoing = TRUE`) |
| **Task** | Create task: `FRT-TSK-FIX-GENL-015 "Fix settings submit button disabled state during save"` (origin: `informal`) |
| **begin-work** | Register active_work with task_id. Create claim on `src/pages/SettingsPage.tsx`. |
| **Inner pathway** | `bugfix.json` selected (type=FIX) |
| **Sentinel** | `pathflow:shell:work-reg` created |
| **Duration** | ~3 seconds |

#### OS-5: Inner Pathway (bugfix.json)

The bugfix inner pathway is streamlined -- no checkpoints, 4 nodes:

**Inner Pathway Task Graph** (solo -- lead executes all):

| Inner Node | Action | Duration |
|-----------|--------|----------|
| BUG-investigate | Read SettingsPage.tsx, find the submit handler, identify missing `disabled` prop | ~2 minutes |
| BUG-fix | Add `isSubmitting` state, wire to button `disabled` prop | ~3 minutes |
| BUG-test | Run existing tests, verify no regressions | ~1 minute |
| BUG-finalize | Review diff, confirm fix | ~30 seconds |

**Execution flow**:

Since this is solo mode, the team lead executes all nodes directly (no teammates):

1. **BUG-investigate**: Lead reads `src/pages/SettingsPage.tsx`. Finds the `handleSave` function sets `isSubmitting` to `true` but the submit button does not use it. The button JSX lacks `disabled={isSubmitting}`.

2. **BUG-fix**: Lead edits `src/pages/SettingsPage.tsx`:
   - Line 47: Change `<button type="submit">` to `<button type="submit" disabled={isSubmitting}>`
   - One Edit tool call. PreToolUse validates: work registered? YES. File claimed? YES.

3. **BUG-test**: Lead runs `npm test -- --testPathPattern=Settings`. 12/12 pass. No new test needed (existing test covers the rendering, just not the disabled state -- but Tier 1 verification does not require new test coverage).

4. **BUG-finalize**: Lead reviews `git diff`. Single line change. Correct.

**Sentinel** | `pathflow:shell:inner-complete` created (TTL: 3600s)
**Total OS-5 duration**: ~7 minutes, ~8 tool calls

#### OS-6: Post-Work Finalize

| Aspect | Detail |
|--------|--------|
| **complete-work** | Archives context. Creates commit sentinel. |
| **create-commit** | `fix(settings): disable submit button during save operation` |
| **push** | User prompted (auto_push: prompt). User says "yes." `git push -u origin fix/frt-settings-submit-disabled` |
| **PR** | Not created (auto_pr: never). User can create manually if desired. |
| **Sentinel** | `pathflow:shell:finalize` created |
| **Duration** | ~1 minute |

#### OS-7: Work Verification

| Aspect | Detail |
|--------|--------|
| **PCV tier** | Tier 1 (forced by unstructured profile) |
| **Verification output** | `verify-work` marker + TIER 1 indicator. Files: `src/pages/SettingsPage.tsx` (1 line modified). Tests: 12/12 pass. Committed and pushed. |
| **Stop hook** | Validates Tier 1: marker present? YES. Decision: ALLOW. |
| **Sentinel** | `pathflow:shell:verified` created |
| **Duration** | ~15 seconds |

#### OS-8: Session Close

| Aspect | Detail |
|--------|--------|
| **Teammates** | None to shut down (solo). |
| **Cleanup** | Full cleanup (unstructured profile). |
| **Sentinel** | `pathflow:shell:close` created |

#### OS-9: Session End

| Aspect | Detail |
|--------|--------|
| **Duration** | Total session: ~10 minutes |
| **Status** | `completed` |

### Key Differences from Use Case 1

| Aspect | Structured (UC1) | Unstructured (UC2) |
|--------|:-----------------:|:------------------:|
| Team | Lead + developer + qa + ops | Solo (lead only) |
| Epic | Pre-existing | Lazy-created ongoing epic |
| Task | Pre-existing | Created on-the-fly |
| Inner pathway | development.json (8 nodes, checkpoints) | bugfix.json (4 nodes, no checkpoints) |
| Verification | Tier 2 (artifacts + verification) | Tier 1 (marker only) |
| PR | Always created | Not created |
| Total duration | ~35 minutes | ~10 minutes |
| Tool calls | ~60+ | ~8 |

---

## Use Case 3: Autorun Batch Execution

### Scenario

A CI pipeline triggers Claude Code in autorun mode to implement a pre-defined task. The batch configuration specifies:

```json
{
  "task_id": "BKD-TSK-FEAT-API-007",
  "description": "Add rate limiting middleware to the /api/v1/users endpoint. Limit: 100 requests per minute per IP. Return 429 status with Retry-After header when exceeded.",
  "inner_pathway": "development",
  "team_strategy": "standard",
  "acceptance_criteria": [
    "Rate limiting middleware exists at src/middleware/rateLimit.ts",
    "Middleware is applied to /api/v1/users route",
    "Tests verify 429 response after 100 requests",
    "No linting errors",
    "All existing tests pass",
    "PR created against main branch"
  ],
  "max_duration_seconds": 1800,
  "auto_pr_base": "main"
}
```

### Session Profile

```json
{
  "session_profile": "autorun",
  "team_strategy": "standard",
  "resume_behavior": "auto-resume-matching",
  "verification_tier": "auto-detect",
  "auto_pr": "always",
  "auto_push": "always",
  "meta_awareness": "silent",
  "checkpoint_interaction": "auto-evaluate"
}
```

### Team Composition

```
Team Lead (PERSISTENT)
  |-- cf-developer (PERSISTENT) -- implementation
  |-- cf-qa (ON-DEMAND) -- testing
  |-- cf-ops (ON-DEMAND) -- PR creation
```

### Node-by-Node Traversal

#### OS-1: Session Boot

| Aspect | Detail |
|--------|--------|
| **Trigger** | CLI invocation: `claude --autorun batch.json` |
| **Instructions** | Silent (autorun profile). Batch context injected. |
| **Session ID** | `session-01JKD5R9MNOPQR` |
| **Sentinel** | `pathflow:shell:boot` created |

#### OS-2: Context Load

| Aspect | Detail |
|--------|--------|
| **Active work** | Checks for `BKD-TSK-FEAT-API-007`. Found with status `pending`. |
| **Resume** | Auto-resume matching task ID from batch config. |
| **Meta-awareness** | Silent (no display). |
| **UserPromptSubmit** | Batch description injected as initial prompt. No interactive hooks. |
| **Sentinel** | `pathflow:shell:context` created |

#### OS-3: Team Init

| Aspect | Detail |
|--------|--------|
| **Strategy** | `standard` (from batch config) |
| **Team name** | `cf-BKD-API-007` |
| **Spawned** | cf-developer (persistent) |
| **Sentinel** | `pathflow:shell:team-init` created |

#### OS-4: Work Registration

| Aspect | Detail |
|--------|--------|
| **Classification** | Batch-defined: area=BKD, type=FEAT, domain=API. No auto-detection needed. |
| **Task** | `BKD-TSK-FEAT-API-007` exists. task_id confirmed NOT NULL. |
| **begin-work** | Register active_work. Claims on `src/middleware/`, `src/routes/users.ts`. |
| **Inner pathway** | `development.json` (from batch config). |
| **Sentinel** | `pathflow:shell:work-reg` created |

#### OS-5: Inner Pathway (development.json, autorun variant)

In autorun mode, inner pathway checkpoints are auto-evaluated against acceptance criteria instead of requiring interactive approval.

**Execution flow**:

1. **DEV-init** (lead): Load batch context, verify branch. Create `feat/bkd-api-rate-limiting`. Sentinel created.

2. **DEV-plan** (lead): Plan derived from batch description. No interactive planning needed. Implementation plan: create middleware, apply to route, write tests. Sentinel created.

3. **DEV-impl** (cf-developer):
   - Creates `src/middleware/rateLimit.ts` with sliding window rate limiter
   - Modifies `src/routes/users.ts` to apply middleware
   - Modifies `src/routes/index.ts` to import middleware
   - Self-tests: `npm test -- --testPathPattern=rateLimit` -- no existing tests yet, expected
   - Duration: ~10-15 minutes
   - Sentinel created.

4. **DEV-test** (cf-qa, spawned on-demand):
   - Creates `src/middleware/__tests__/rateLimit.test.ts`
   - Tests cover: normal requests pass, 101st request returns 429, Retry-After header present, different IPs have independent limits
   - Runs full test suite: 47/47 pass (including 4 new)
   - Reports coverage
   - Sentinel created. QA shuts down.

5. **DEV-final** (lead): Auto-evaluates against acceptance criteria:
   - [x] `src/middleware/rateLimit.ts` exists? YES
   - [x] Applied to `/api/v1/users`? YES (grep confirms import in routes)
   - [x] Tests verify 429? YES (test `should return 429 after 100 requests`)
   - [x] No linting errors? Runs `npm run lint` -- PASS
   - [x] All existing tests pass? 47/47 -- PASS
   - [x] PR created? Not yet (happens in OS-6)
   - Partial pass (5/6 criteria met, PR deferred to OS-6)
   - Sentinel created.

**Sentinel** | `pathflow:shell:inner-complete` created
**Timeout tracking**: Timer started at OS-5 entry. Current elapsed: ~20 minutes. Max: 30 minutes. Within budget.

#### OS-6: Post-Work Finalize

| Aspect | Detail |
|--------|--------|
| **complete-work** | Archives context. Commit sentinel created. |
| **create-commit** | `feat(api): add rate limiting middleware to /api/v1/users endpoint` |
| **push** | Auto-push (autorun profile). `git push -u origin feat/bkd-api-rate-limiting`. |
| **PR** | Auto-PR (autorun profile). Base: `main` (from batch config). PR #43 created. |
| **Final acceptance check** | [x] PR created against main? YES. All 6/6 criteria now met. |
| **Sentinel** | `pathflow:shell:finalize` created |

#### OS-7: Work Verification

| Aspect | Detail |
|--------|--------|
| **PCV tier** | Auto-detect: standard feature with tests -> Tier 2. |
| **Autorun evaluation** | Programmatic check against all 6 acceptance criteria. All pass. |
| **Verification output** | ARTIFACTS: 1 new file, 2 modified files, 1 new test file. VERIFICATION: 47 tests pass, lint clean, PR #43. |
| **Stop hook** | Autorun mode: evaluates criteria programmatically. Decision: ALLOW. |
| **Sentinel** | `pathflow:shell:verified` created |

#### OS-8: Session Close

| Aspect | Detail |
|--------|--------|
| **Shutdown** | Immediate termination of cf-developer (no confirmation wait in autorun). |
| **Cleanup** | Team removed. |
| **Sentinel** | `pathflow:shell:close` created |

#### OS-9: Session End

| Aspect | Detail |
|--------|--------|
| **Duration** | ~25 minutes (within 30-minute budget) |
| **Status** | `completed` |
| **Output** | PR #43 URL written to batch output file |

### Autorun-Specific Behaviors

| Behavior | How It Differs from Interactive |
|----------|-------------------------------|
| **No prompts** | Every decision derived from batch config or acceptance criteria |
| **Timeout enforcement** | `max_duration_seconds: 1800` triggers forced finalization if exceeded |
| **Auto-PR** | PR created without asking. Base branch from config. |
| **Acceptance evaluation** | Programmatic string matching / command execution instead of user confirmation |
| **Teammate shutdown** | Immediate (no graceful confirmation wait) |
| **Failure handling** | On failure: session status = `failed`, error report in batch output. No retry within session. |

### Autorun Timeout Scenario

If the session approaches the 30-minute timeout:

```
T+1500s (25 min): Timeout warning. Lead checks inner pathway progress.
T+1680s (28 min): If inner pathway not complete:
  - Lead sends "wind down" to cf-developer
  - Developer commits current state
  - Inner pathway marked as partial completion
T+1740s (29 min): Force-enter OS-6 with partial work
  - Commit whatever is ready
  - PR created with "WIP:" prefix
  - Acceptance criteria: 4/6 met
T+1800s (30 min): Session forcefully ends
  - Status: `partial`
  - Output: PR URL + unmet criteria list
```

---

## Use Case 4: Exploratory Research/Investigation

### Scenario

A developer wants to understand how the current authentication system works before planning a redesign. They type: "Explore the auth system -- how does token refresh work? What are the failure modes? Document what you find."

### Session Profile

```json
{
  "session_profile": "unstructured",
  "team_strategy": "solo",
  "resume_behavior": "never-resume",
  "verification_tier": "force-tier-1",
  "auto_pr": "never",
  "auto_push": "never",
  "meta_awareness": "minimal",
  "checkpoint_interaction": "auto-evaluate"
}
```

### Team Composition

```
Team Lead (PERSISTENT) -- solo, uses sub-agents for parallel research
```

This use case demonstrates the sub-agent pattern (Task tool with Explore type) rather than full teammates.

### Node-by-Node Traversal

#### OS-1: Session Boot

Standard. Session ID: `session-01JKE8V2RSTUVW`. Sentinel created.

#### OS-2: Context Load

| Aspect | Detail |
|--------|--------|
| **Active work** | None found. |
| **Resume** | Skipped (never-resume). |
| **Meta-awareness** | Minimal. |
| **First prompt** | "Explore the auth system -- how does token refresh work?" |
| **Sentinel** | `pathflow:shell:context` created |

#### OS-3: Team Init (SKIPPED)

| Aspect | Detail |
|--------|--------|
| **Decision** | Solo profile. No team spawned. |
| **Sentinel** | `pathflow:shell:team-init` created with `skipped: true` |

#### OS-4: Work Registration

| Aspect | Detail |
|--------|--------|
| **Classification** | area=BKD (backend, auth is backend), type=RSCH (research), domain=AUTH |
| **Epic** | Create ongoing: `BKD-EPC-RSCH-GENL-001 "Backend Research (Ongoing)"` |
| **Task** | Create: `BKD-TSK-RSCH-AUTH-001 "Investigate auth token refresh mechanism"` (origin: `informal`) |
| **Inner pathway** | `research.json` selected (type=RSCH) |
| **Sentinel** | `pathflow:shell:work-reg` created |

#### OS-5: Inner Pathway (research.json)

The research inner pathway is exploration-focused:

**Inner Pathway Nodes** (solo execution, with sub-agent delegation):

| Inner Node | Action |
|-----------|--------|
| RSH-scope | Define research questions |
| RSH-search | Search codebase for relevant code |
| RSH-analyze | Deep analysis of findings |
| RSH-synthesize | Write findings document |

**Execution flow**:

1. **RSH-scope**: Lead formulates three research questions:
   - How does token refresh work (flow)?
   - What are the failure modes?
   - What are the current limitations?

2. **RSH-search**: Lead uses sub-agents for parallel search (Task tool with Explore type):

   **Sub-agent 1**: "Search for token refresh implementation. Look in `src/auth/`, `src/middleware/`, `src/utils/`. Find the refresh token flow."

   **Sub-agent 2**: "Search for error handling in auth. Look for catch blocks, error responses, and failure logging in auth-related files."

   **Sub-agent 3**: "Search for auth configuration. Find token TTL values, refresh settings, and any hardcoded limits."

   Each sub-agent returns findings and terminates. No persistent teammates needed.

   Results:
   - Token refresh flow: `src/auth/tokenService.ts:refreshToken()` -> validates refresh token -> issues new access token -> rotates refresh token
   - Failure modes: expired refresh token (401), invalid token format (400), revoked token (403), database connection failure (500)
   - Configuration: access token TTL 15min, refresh token TTL 7d, max refresh chain 10

3. **RSH-analyze**: Lead synthesizes sub-agent findings. Reads key files directly for deeper understanding:
   - `src/auth/tokenService.ts` (the core service)
   - `src/middleware/authMiddleware.ts` (where tokens are validated)
   - `src/auth/tokenStore.ts` (persistence layer)

   Key insight discovered: the refresh token rotation does NOT invalidate the old refresh token immediately -- there is a 60-second grace period. This means a stolen refresh token could be used within the grace window.

4. **RSH-synthesize**: Lead writes findings document. In this case, since the user asked to "document what you find," the lead creates a research findings file (or provides it as a response).

   The lead does NOT write to `project/` docs (that would require cf-documenter). Instead, the findings are presented directly in the response to the user.

**Sentinel** | `pathflow:shell:inner-complete` created
**Total duration**: ~10-15 minutes

#### OS-6: Post-Work Finalize

| Aspect | Detail |
|--------|--------|
| **complete-work** | Archives research context. |
| **create-commit** | No code changes made. No commit needed. Skip. |
| **push** | Nothing to push. Skip. |
| **PR** | Not applicable. Skip. |
| **Record progress** | "Research complete: auth token refresh mechanism documented." |
| **Sentinel** | `pathflow:shell:finalize` created |

**Note**: For research sessions with no code changes, OS-6 is lightweight. The `complete-work` sentinel still gates the flow, but commit/push/PR operations are conditionally skipped.

#### OS-7: Work Verification

| Aspect | Detail |
|--------|--------|
| **PCV tier** | Tier 1 (no code changes, research only) |
| **Verification** | Research findings presented to user. No artifacts to verify beyond the response. |
| **Stop hook** | Tier 1: marker present? YES. Decision: ALLOW. |
| **Sentinel** | `pathflow:shell:verified` created |

#### OS-8 & OS-9: Session Close & End

Standard. No teammates to shut down. Full cleanup. Duration: ~12 minutes total.

### Research-Specific Behaviors

| Behavior | Detail |
|----------|--------|
| **Sub-agent pattern** | Task tool with `subagent_type='Explore'` for parallel codebase searches |
| **No git operations** | No branch, commit, push, or PR (read-only session) |
| **Lightweight finalize** | OS-6 skips most operations (no code changes) |
| **Tier 1 verification** | Research quality is in the response, not in artifacts |
| **No teammates** | Solo mode with sub-agent delegation for parallelism |

---

## Use Case Comparison Matrix

| Dimension | UC1: Structured Dev | UC2: Ad-Hoc Fix | UC3: Autorun | UC4: Research |
|-----------|:------------------:|:---------------:|:------------:|:-------------:|
| **Profile** | structured | unstructured | autorun | unstructured |
| **Team** | Lead + dev + qa + ops | Solo | Lead + dev + qa + ops | Solo + sub-agents |
| **Epic** | Pre-existing | Lazy-created ongoing | Pre-existing | Lazy-created ongoing |
| **Task** | Pre-existing | Created on-the-fly | Pre-existing (batch) | Created on-the-fly |
| **Inner pathway** | development.json | bugfix.json | development.json | research.json |
| **OS-3 (Team Init)** | Standard team | Skipped | Standard team | Skipped |
| **Checkpoints** | Interactive | Auto-evaluate | Auto-evaluate | Auto-evaluate |
| **Git operations** | Branch + commit + push + PR | Branch + commit + push | Branch + commit + push + PR | None |
| **Verification tier** | Tier 2 | Tier 1 | Tier 2 (auto) | Tier 1 |
| **Duration** | ~35 min | ~10 min | ~25 min (budget: 30) | ~12 min |
| **Tool calls** | ~60+ | ~8 | ~50+ | ~20 |
| **Sentinels created** | 8 shell + 5 inner + skill | 8 shell + 4 inner + skill | 8 shell + 5 inner + skill | 8 shell + 4 inner |
| **Teammates spawned** | 3 (dev persistent, qa+ops on-demand) | 0 | 3 (dev persistent, qa+ops on-demand) | 0 (sub-agents only) |

---

## Edge Cases and Recovery Paths

### Edge Case 1: Verification Fails (UC1 Structured)

At OS-7, the stop hook finds PCV Tier 2 output is missing the VERIFICATION section.

**Recovery**:
1. Stop hook returns `{"decision": "block", "reason": "VERIFICATION section missing"}`
2. Session continues. Lead re-enters OS-7.
3. Lead runs tests again, adds verification section.
4. Stop hook re-evaluates: PASS.
5. Reopen count: 1. Max: 3. No force-allow needed.

### Edge Case 2: Teammate Context Exhaustion (UC1 Structured)

At inner node DEV-impl, cf-developer hits 85% context usage after 45 tool calls.

**Recovery**:
1. PostToolUse hook detects 85% threshold, writes warning.
2. Developer sends handoff summary to lead: files modified, decisions made, remaining work.
3. Lead sends shutdown_request to developer. Developer approves.
4. Lead spawns fresh cf-developer with handoff summary in spawn prompt.
5. Replacement continues implementation from where predecessor left off.
6. Inner pathway node DEV-impl continues with new developer.

### Edge Case 3: Autorun Timeout (UC3)

At T+28 minutes, implementation is 80% done but tests have not been written yet.

**Recovery**:
1. Timeout warning triggers at T+25 min.
2. Lead sends "wind down" to cf-developer: "Commit current progress."
3. Developer commits partial implementation.
4. Lead skips DEV-test node (creates sentinel with `skipped: true, reason: "timeout"`).
5. Lead enters OS-6: commits, pushes, creates PR with "WIP: " prefix.
6. Acceptance criteria: 3/6 met.
7. Session status: `partial`. Output includes unmet criteria.
8. Next autorun invocation can resume the task.

### Edge Case 4: No Code Changes Needed (UC4 Research)

Research finds the auth system is well-documented and no investigation was actually needed.

**Recovery**:
1. OS-5 inner pathway completes quickly (RSH-search finds existing documentation).
2. OS-6: No commit, push, or PR. All skipped (no changes).
3. OS-7: Tier 1 verification with response-only output. PASS.
4. Session completes normally in ~5 minutes. This is a valid short session.

### Edge Case 5: Wrong Inner Pathway Selected (Any UC)

OS-4 selects `bugfix.json` but the user actually wants to add a feature.

**Recovery**:
1. During OS-5, lead realizes the pathway is too minimal for the work scope.
2. OS-4 is a checkpoint with `reopenable: true`.
3. Lead re-opens OS-4: deletes `pathflow:shell:work-reg` sentinel.
4. OS-5 progress is lost (inner pathway sentinels deleted).
5. Lead re-classifies work, selects `development.json`.
6. OS-5 restarts with the correct inner pathway.
7. Reopen count: 1. Audit log: "OS-4 reopened: incorrect pathway selection."

### Edge Case 6: Multi-Work Session (Hypothetical)

User completes a bug fix and then says "Now also add a loading spinner to the same page."

**Current behavior** (single traversal):
1. OS-7 verification passes for the bug fix.
2. Session proceeds to OS-8/OS-9 and ends.
3. User must start a new session for the loading spinner.

**Future extension** (multi-traversal):
1. OS-7 verification passes for the bug fix.
2. Instead of proceeding to OS-8, lead detects new work request.
3. Lead loops back to OS-4 with new classification.
4. New inner pathway instantiated for the second piece of work.
5. OS-6/OS-7 run again for the second piece.
6. Then OS-8/OS-9 for final session end.

This is noted as a future extension in the outer shell design document (Open Question Q3).

---

## Insights for PathFlow Design Refinement

### Insight 1: Solo Mode is the Common Case

Use cases 2 and 4 (unstructured and research) run in solo mode. These are likely the most frequent session types -- quick fixes and exploration. The outer shell must be lightweight for solo mode. The OS-3 skip mechanism is critical.

### Insight 2: Inner Pathway Diversity Matters

The four use cases use three different inner pathways (`development.json`, `bugfix.json`, `research.json`). The plug-in mechanism must handle this cleanly. The outer shell should not assume any particular inner pathway structure.

### Insight 3: Autorun Needs Acceptance Criteria Evaluation

The autorun variant requires programmatic evaluation of acceptance criteria. This is a distinct capability from interactive verification. The outer shell should provide a standard interface for criteria evaluation at OS-7 that both modes can use.

### Insight 4: Research Sessions Have No Git Operations

When no code changes are made, OS-6 should gracefully skip git operations. The finalize node should detect "no changes" and create its sentinel without requiring commit/push/PR.

### Insight 5: Teammate Lifecycle is Highly Variable

UC1 spawns 3 teammates over 35 minutes. UC2 spawns none. UC3 spawns 3 under time pressure. UC4 uses sub-agents instead of teammates. The team management layer must handle all these patterns without overhead in the simple cases.
