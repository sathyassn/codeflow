# PathFlow Design: Feasibility Findings

> Research findings for Agent Teams integration and CodeFlow design decisions.
> Researcher: feasibility-checker | Date: 2026-02-06

---

## Q1: Delegate Mode Enforcement

### What Delegate Mode Does

Delegate mode restricts the team lead to **coordination-only tools**. When enabled, the lead can only:

- **Spawn teammates** (TeammateTool)
- **Send messages** (SendMessage)
- **Shut down teammates** (SendMessage shutdown_request)
- **Manage tasks** (TaskCreate, TaskUpdate, TaskList, TaskGet)

The lead **cannot** use:

- Edit, Write (file modification)
- Bash (command execution)
- Task (subagent spawning -- this is the old subagent tool, distinct from TaskCreate)

### Can the Lead Still Read Files?

**Unclear from docs but likely yes.** The official documentation describes delegate mode as preventing the lead from "touching code" and restricting to "coordination-only tools." Read and Glob are not explicitly listed as blocked. The intent is to prevent the lead from doing _implementation work_, not from reading. However, this is not definitively documented.

**Design implication:** If the lead CAN read files in delegate mode, our read delegation hook remains partially relevant (for context conservation). If it CANNOT, the hook is irrelevant for the lead but still useful for teammates.

### How to Enable Delegate Mode

| Method | Details | Programmatic? |
|--------|---------|---------------|
| Shift+Tab | Cycles through permission modes; delegate mode appears when a team is active | No (interactive only) |
| settings.json | No documented `delegateMode` setting key found | Not available |
| Environment variable | No `CLAUDE_CODE_DELEGATE_MODE` or similar found | Not available |
| CLI flag | No `--delegate-mode` flag documented | Not available |

**Critical finding:** Delegate mode is currently **interactive-only** (Shift+Tab). There is no known way to auto-enable it via settings, env var, or programmatic API. This means CodeFlow cannot enforce delegate mode on team leads automatically.

### Related Permission Modes (Shift+Tab Cycle)

When a team is active, Shift+Tab cycles through:
1. Normal mode (default)
2. Auto-accept edits
3. Plan mode (read-only)
4. **Delegate mode** (coordination-only, team active only)

### Impact on PathFlow Design

- **Cannot auto-enforce:** PathFlow cannot guarantee the lead stays in delegate mode. The user must manually toggle it.
- **Hook-based enforcement possible:** CodeFlow hooks (PreToolUse) could detect when the lead tries to use non-coordination tools and issue warnings or blocks, effectively creating a "soft delegate mode" through hook enforcement.
- **Recommendation:** Implement a hook-based delegate enforcement that warns/blocks direct implementation by the lead, independent of the built-in delegate mode. This provides programmatic control that delegate mode lacks.

---

## Q2: Read Delegation in Agent Teams Context

### Current Hook Behavior

File: `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-read-delegation.sh`

The hook implements three checks:

1. **Allowed Paths (bypass):** Files like `.claude/CLAUDE.md`, `settings.json`, `README.md` bypass all checks.

2. **Always-Block Patterns (deny):** Files matching patterns like `.claude/memory/**`, `*.jsonl`, `*.db`, `*.log` are **blocked** with `permissionDecision: deny` (exit 2). The hook instructs the agent to use `Task(subagent_type='Explore', ...)` instead.

3. **Threshold Rules (warn):** Files exceeding line-count thresholds trigger warnings suggesting delegation. Default thresholds:
   - `.sh` files: 200 lines
   - `.py`/`.ts`/`.js` files: 300 lines
   - `.md`/`.txt` files: 500 lines
   - Default: 400 lines

Current settings.json overrides the threshold to **50 lines for all files** (`maxLines: 50` for `**/*`).

### Does Delegate Mode Supersede This Hook?

**No, they serve different purposes:**

| Concern | Delegate Mode | Read Delegation Hook |
|---------|---------------|---------------------|
| Purpose | Restrict lead to coordination | Conserve context window |
| Scope | Lead only (when toggled) | All agents (lead + teammates) |
| Mechanism | Tool-level restriction | Hook-based guidance |
| File reading | Probably still allowed | Blocks/warns large files |
| Programmatic | No | Yes |

### Recommendation for Agent Teams

The read delegation hook remains valuable **especially for teammates**:

- **Teammates have finite context windows.** Reading a 500-line file wastes context that could be used for implementation.
- **The hook should adapt its delegation target.** In Agent Teams context, instead of delegating to `Task(subagent_type='Explore')`, the hook could suggest using SendMessage to ask the lead for a summary, or suggest reading specific sections with `offset/limit`.
- **The hook should detect whether it is running in a teammate context** (check for `CLAUDE_CODE_TEAM_NAME` env var) and adjust behavior accordingly.

---

## Q3: Work Graph Task Registration

### How the V3 Spec Handles ALL Work Types

The V3 Work Graph spec (`04-knowledge-layer/01-work-graph.md`) provides comprehensive work tracking:

#### Planned Work Flow
```
Epic (planned) -> Tasks (planned) -> Active Work (runtime)
```

#### Unplanned/Informal Work Flow
The spec handles unplanned work through two mechanisms:

**1. Ongoing Epics (Catch-All)**

```
Pattern: {AREA}-EPC-{WORK}-GENL-001

Examples:
- FRT-EPC-FIX-GENL-001   "Frontend Bug Fixes (Ongoing)"
- BKD-EPC-HTFX-GENL-001  "Backend Hotfixes (Ongoing)"
- INF-EPC-CHOR-GENL-001  "Infrastructure Maintenance (Ongoing)"

Creation Strategy: lazy (created on-demand when first needed)
```

The `GENL` domain code is **reserved** specifically for general/catch-all work. Ongoing epics have `is_ongoing = TRUE` and receive unplanned tasks.

**2. Task Origin Tracking**

Each task has an `origin` field:
- `planned` -- Created during epic planning
- `informal` -- Created ad-hoc during development
- `auto` -- Created automatically by the system

**3. Active Work Registry (Task-Optional)**

The `active_work` table has `task_id` as **optional** (`REFERENCES tasks(id)` but nullable). This means:
- Work CAN be tracked without a pre-existing task
- The `topic` field provides human-readable description
- Work items link to sessions and branches independently

### Enforcement Mechanism

The spec enforces work tracking at multiple points:

| Hook Point | Enforcement |
|------------|-------------|
| `/dev` command start | Creates or links active_work entry |
| PreToolUse (Edit/Write) | Checks file against active work scope |
| PreToolUse (Bash) | Checks for branch conflicts |
| Claim validation | Pre-tool-use hook validates file claims |
| Session start | Recovers active work from previous session |

### Key Finding: No "Reject Untracked Work" Gate

The V3 spec does NOT have a hard gate that says "reject all work that isn't in the work graph." Instead:

- Work tracking is **encouraged** through hooks and workflows
- The scope policy can be `permissive` (log-only, no blocking)
- Active work entries can be created lazily
- The system tries to **assign** unplanned work to ongoing epics retroactively

**Design implication for PathFlow:** PathFlow should follow this same pattern -- encourage tracking through the task system but allow lightweight work registration. A hard enforcement gate would be too restrictive for exploratory work.

---

## Q4: Teammate Context Monitoring

### Built-in Mechanisms

**No dedicated context monitoring API exists.** Based on research:

| Mechanism | Available? | Details |
|-----------|-----------|---------|
| Context usage API | No | No env var or API exposes current token count to teammates |
| Progress reporting | Partial | Teammates can send messages with status updates |
| Automatic monitoring | No | Lead cannot query teammate context usage |
| StatusLine | Yes (lead only) | The `statusLine` in settings.json shows context usage via `context_window` JSON, but this only applies to the lead's own terminal display |

### StatusLine Context Data

The CodeFlow statusLine command already extracts context window data:

```bash
# From settings.json statusLine command:
inp=$(echo "$input" | jq -r '.context_window.current_usage.input_tokens // 0')
cr=$(echo "$input" | jq -r '.context_window.current_usage.cache_read_input_tokens // 0')
cc=$(echo "$input" | jq -r '.context_window.current_usage.cache_creation_input_tokens // 0')
cs=$(echo "$input" | jq -r '.context_window.context_window_size // 200000')
out=$(echo "$input" | jq -r '.context_window.current_usage.output_tokens // 0')
```

This data is available to hooks via StatusLine but ONLY for the agent running the hook (lead or individual teammate). A teammate's hooks can see its own context usage.

### Possible Workarounds

**1. Hook-Based Self-Reporting (Recommended)**

A PostToolUse hook on teammates could:
- Read the context window data from the hook environment
- Write it to a shared file or send a message when thresholds are reached
- The lead could then react to context warnings

**2. Periodic Status Pings**

The lead could periodically message teammates asking for status. Teammates could include a context estimate in their reply. This is manual and adds overhead.

**3. Task-Based Context Budgeting**

Instead of monitoring, allocate work by estimated context cost:
- Small tasks (XS/S) for teammates with limited context remaining
- Larger tasks for fresh teammates
- Spawn replacement teammates rather than overloading existing ones

### Recommendation

Implement a **PostToolUse hook** for teammates that:
1. Reads context window data from the hook environment (if available)
2. When context exceeds a threshold (e.g., 70%), automatically sends a warning message to the lead
3. The lead can then decide to wind down the teammate or spawn a replacement

---

## Q5: Settings Configuration for Agent Teams

### Current Settings State

File: `.claude/settings.json`

**Agent Teams configuration: NOT present.** The settings.json does not include:
- `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` in an `env` block
- `teammateMode` setting
- Any agent-teams-specific configuration

The settings file does include:
- `statusLine` with context window monitoring
- `_read_delegation_config` with delegation settings
- `_verify_work_config`
- `_web_fetch_config`
- Comprehensive `permissions` (allow/ask/deny lists)
- Full `hooks` configuration for all lifecycle events

### Settings Templates Analysis

All three templates examined (strict, standard, autonomous) share **identical** patterns:

| Feature | strict | standard | autonomous |
|---------|--------|----------|------------|
| `_read_delegation_config.enabled` | true | true | true |
| `_read_delegation_config.delegationTarget` | cf-general-purpose | cf-general-purpose | cf-general-purpose |
| `_read_delegation_config.maxLines` | 50 | 50 | 50 |
| Agent Teams env var | Missing | Missing | Missing |
| `teammateMode` | Missing | Missing | Missing |
| Hooks | Identical | Identical | Identical |
| StatusLine | Identical | Identical | Identical |

### What Needs to Be Added for Agent Teams

To enable Agent Teams in CodeFlow, the following settings changes are needed:

```json
{
  "env": {
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1"
  },
  "teammateMode": "in-process"
}
```

Additional CodeFlow-specific configuration to consider:

```json
{
  "_agent_teams_config": {
    "enabled": true,
    "defaultTeammateMode": "in-process",
    "maxTeammates": 5,
    "contextWarningThreshold": 0.7,
    "delegateModeEnforcement": "hook-based",
    "teammateSubagentType": "cf-general-purpose"
  }
}
```

### Settings Sync Concern

All templates must stay synchronized. The hooks section is **identical** across all templates, and any Agent Teams configuration should follow the same pattern.

---

## Summary: Key Design Implications for PathFlow

| Finding | Implication |
|---------|-------------|
| Delegate mode is interactive-only | PathFlow must use hooks for enforcement, not rely on built-in delegate mode |
| Read delegation is still needed | Adapt hook for teammate context (check CLAUDE_CODE_TEAM_NAME) |
| Work graph supports unplanned work | PathFlow can use lazy registration with ongoing epics |
| No context monitoring API | Use hook-based self-reporting for teammates |
| Agent Teams settings not configured | Need to add env var and teammateMode to all templates |
| Teammates inherit lead's permissions | No per-teammate permission customization at spawn time |
| No session resumption for teammates | PathFlow must handle teammate loss gracefully |

---

## Sources

- [Claude Code Agent Teams Documentation](https://code.claude.com/docs/en/agent-teams)
- [Claude Code Settings Documentation](https://code.claude.com/docs/en/settings)
- [Claude Code Planning Modes](https://claudefa.st/blog/guide/mechanics/planning-modes)
- [Addy Osmani: Claude Code Swarms](https://addyosmani.com/blog/claude-code-agent-teams/)
- [Anthropic: Building a C Compiler with Agent Teams](https://www.anthropic.com/engineering/building-c-compiler)
- [Claude Code Agent Teams Setup Guide](https://www.marc0.dev/en/blog/claude-code-agent-teams-multiple-ai-agents-working-in-parallel-setup-guide-1770317684454)
- CodeFlow Spec V3: `04-knowledge-layer/01-work-graph.md`
- CodeFlow Spec V3: `04-knowledge-layer/03-active-work.md`
- CodeFlow Implementation: `.claude/settings.json`, `.claude/settings-templates/*.json`
- CodeFlow Implementation: `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-read-delegation.sh`
