---
description: "Set Claude Code approval mode"
argument-hint: "[mode]"
---

# /cf-approval-mode Command

## Working Protocol

**Skill:** `.claude/skills/cf-working-protocol/SKILL.md`

Apply cognitive operations throughout execution:

- 🔧 think-and-act: Before applying settings changes (PAC-5)
- 🔧 decide: Mode selection is Tier 1 (reversible, user-driven)
- 🔧 respond-organized: Clear confirmation of applied mode

**Note:** cf-working-protocol loaded at SessionStart, applies to all execution.

---

## 1. Purpose & Usage

**Purpose:** View or change the Claude Code approval mode by applying a settings template to `.claude/settings.local.json`.

**Usage:**

```text
/cf-approval-mode [mode]
```

**Use When:**

- Want to adjust how much confirmation Claude Code requests before tool use
- Switching between strict (maximum confirmation) and autonomous (minimal confirmation) modes
- Setting up a session for attended (strict/standard) or unattended (autonomous/permissive) work

**Do Not Use When:**

- Modifying hook behavior (hooks are configured separately in `.claude/settings.json`)
- Changing model or other CLI settings (use Claude Code CLI flags)

### Pipeline Position

```text
Phase: Any (always available) | Type: Information/Settings
No pipeline dependencies — can be invoked at any point during a session.
```

---

## 2. Arguments & Flags

**Arguments:**

| Argument | Required | Description |
|----------|----------|-------------|
| `mode` | No | Approval mode to apply: `strict`, `standard`, `autonomous`, `permissive` |

**Modes:**

| Mode | Description | Best For |
|------|-------------|----------|
| `strict` | Confirms every tool call | Learning, auditing, sensitive work |
| `standard` | Confirms destructive and external operations | Normal interactive development |
| `autonomous` | Confirms only high-risk operations | Experienced users, routine tasks |
| `permissive` | Minimal confirmations | Autorun sessions, trusted pipelines |

If no mode is provided, the command displays the current mode and shows a selection menu.

**Examples:**

```bash
# Show current mode and menu
/cf-approval-mode

# Set to autonomous mode
/cf-approval-mode autonomous

# Set to strict mode for sensitive work
/cf-approval-mode strict
```

---

## 3. Prerequisites

**Required State:**

- [ ] Settings templates exist at `.claude/settings-templates/`
- [ ] Write access to `.claude/settings.local.json`

**Required Files:**

| File | Purpose |
|------|---------|
| `.claude/settings-templates/strict.json` | Strict mode template |
| `.claude/settings-templates/standard.json` | Standard mode template |
| `.claude/settings-templates/autonomous.json` | Autonomous mode template |
| `.claude/settings-templates/permissive.json` | Permissive mode template |

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
Phase: Any | Type: Information/Settings

/cf-approval-mode invoked
    |
    v
Parse mode argument
    |
    v
Mode provided? ---NO---> Show current mode
    |                        |
    YES                      v
    |                    Display selection menu
    |                        |
    |                        v
    |                    User selects mode
    |                        |
    +----------+-------------+
               |
               v
Valid mode? ---NO---> ERROR: "Invalid mode.
    |                 Valid: strict, standard, autonomous, permissive"
    YES
    |
    v
Read template file
    |
    v
Template exists? ---NO---> ERROR: "Template not found"
    |
    YES
    |
    v
Validate mode with cf-security                [cf-security]
    |
    v
Copy template to settings.local.json
    |
    v
Confirm: Mode applied successfully
    |
    v
Next: No pipeline progression — standalone settings command.
```

### 4.2 Execution Steps

**Step 1: Parse Mode Argument**

- Check for optional mode argument
- If omitted, proceed to display current mode and menu

**Step 2: Show Current Mode (if no argument)**

- Read `.claude/settings.local.json` if it exists
- Detect current mode by comparing against templates
- Display current mode with description
- Present selection menu:

  ```text
  Current approval mode: standard

  Available modes:
  1. strict      -- Confirms every tool call
  2. standard    -- Confirms destructive and external operations
  3. autonomous  -- Confirms only high-risk operations
  4. permissive  -- Minimal confirmations

  Select mode (1-4):
  ```

**Step 3: Validate Mode**

- Check mode is one of: `strict`, `standard`, `autonomous`, `permissive`
- If invalid, display error with valid options

**Step 4: Read Template**

- Read template from `.claude/settings-templates/{mode}.json`
- Validate template is well-formed JSON

**Step 5: Apply Template**

- SendMessage to cf-security: `"validate-config mode={mode}"`
- Copy template content to `.claude/settings.local.json`
- The `settings.local.json` file is automatically merged with `settings.json` by Claude Code

**Step 6: Confirm**

- Display confirmation:

  ```text
  Approval mode set to: {mode}
  Settings applied to: .claude/settings.local.json
  ```

- Note: Changes take effect on next tool call (no session restart needed)

---

## 5. Skills Integration

| Teammate | Operation | Purpose |
|----------|-----------|---------|
| cf-working-protocol | think-and-act, decide | Reasoning before settings change |
| cf-security | validate-config | Validate mode is safe for current context |

**Note:** cf-security is consulted before applying the template to ensure the mode is appropriate. For example, cf-security may warn if switching to `permissive` mode while working on protected resources.

---

## 6. Hooks Integration

| Hook | When | Purpose |
|------|------|---------|
| SessionStart | Session start | Load cf-working-protocol |
| UserPromptSubmit | `/cf-approval-mode` invoked | Validate invocation |
| PostToolUse | After Write to settings.local.json | `cf-post-tool-use-settings-templates.sh` may sync |
| Stop | Command completes | Standard session logging |

**PostToolUse detail:** The `cf-post-tool-use-settings-templates.sh` hook monitors writes to settings files and may perform additional synchronization.

---

## 7. Memory Integration

**Memory usage:** No work registration required.

This command modifies a local settings file only. It does not interact with the WorkGraph, JSONL events, or memory files.

### File Modified

| File | Action |
|------|--------|
| `.claude/settings.local.json` | Overwritten with selected template |

**Note:** `.claude/settings.local.json` is gitignored (local to each developer). The templates in `.claude/settings-templates/` are version-controlled and shared.

---

## 8. Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| Invalid mode | Argument not in valid set | Show valid modes: strict, standard, autonomous, permissive |
| Template not found | Missing file in `.claude/settings-templates/` | Check template directory exists; reinstall if needed |
| Write permission denied | Cannot write to `.claude/settings.local.json` | Check file permissions and directory ownership |
| Malformed template | Template JSON is invalid | Validate template syntax; restore from git |

**Recovery Procedures:**

```text
ON "Template not found":
  1. Verify .claude/settings-templates/ directory exists
  2. Check for all 4 template files: strict.json, standard.json, autonomous.json, permissive.json
  3. If missing, restore from git: git checkout main -- .claude/settings-templates/

ON "Write permission denied":
  1. Check ownership: ls -la .claude/settings.local.json
  2. Fix permissions if needed
  3. Retry command
```

---

## 9. Examples

**Example 1: View current mode**

```bash
/cf-approval-mode
```

Output:

```text
Current approval mode: standard

Available modes:
1. strict      -- Confirms every tool call
2. standard    -- Confirms destructive and external operations
3. autonomous  -- Confirms only high-risk operations
4. permissive  -- Minimal confirmations

Select mode (1-4):
```

**Example 2: Set autonomous mode**

```bash
/cf-approval-mode autonomous
```

Output:

```text
Approval mode set to: autonomous
Settings applied to: .claude/settings.local.json

Autonomous mode confirms only high-risk operations (force push, branch delete, protected resource access).
```

**Example 3: Set strict mode for sensitive work**

```bash
/cf-approval-mode strict
```

Output:

```text
Approval mode set to: strict
Settings applied to: .claude/settings.local.json

Strict mode confirms every tool call. Best for auditing and sensitive operations.
```

**Example 4: Invalid mode**

```bash
/cf-approval-mode yolo
```

Output:

```text
ERROR: Invalid mode "yolo"

Valid modes: strict, standard, autonomous, permissive

Usage: /cf-approval-mode [mode]
```

---

## 10. References

- [Settings templates](../settings-templates/) -- Mode template files
- [cf-security agent](../agents/cf-security.md) -- Security validation
- [CLAUDE.md](../CLAUDE.md) -- Full command reference
- [cf-help command](./cf-help.md) -- Help and navigation
