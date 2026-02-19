---
id: "INF-EPC-006"
format_id: "INF-EPC-006"
title: "Sandbox Network Settings & Templates"
summary: "Add sandbox network allowedDomains to settings templates, apply autonomous template to both settings files, update documentation"
status: complete
area_type: "INF"
work_type: "CHOR"
domain: "GENL"
is_ongoing: false
file_scope:
  - ".claude/CLAUDE.md"
  - ".claude/settings-templates/autonomous.json"
  - ".claude/settings-templates/permissive.json"
  - ".claude/settings-templates/standard.json"
  - ".claude/agents/cf-security.md"
  - ".claude/hooks/codeflow/session-start/cf-session-start-init.sh"
  - ".claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-protected-resource.sh"
  - ".claude/skills/cf-sandbox-standards/SKILL.md"
  - ".codeflow/config/enforcement/enforcement-policy.json"
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-02-19"
completed_at: "2026-02-19"
updated_at: "2026-02-19"
---

# INF-EPC-006: Sandbox Network Settings & Templates

## Summary

Add sandbox network allowedDomains to settings templates, apply autonomous template to both settings files, and update trusted domain lists and documentation.

## Scope

### In Scope

- Adding `sandbox.network.allowedDomains` to settings templates (autonomous, permissive, standard)
- Applying autonomous template to settings.json (switching from strict)
- Adding `CF_PROJECT_ROOT` environment variable to session-start hook
- Updating cf-security agent definition with dynamic path staging
- Updating cf-sandbox-standards skill with two-layer sandbox model documentation
- Updating protected-resource hook to use `CF_PROJECT_ROOT` with fallback
- Updating enforcement-policy.json managed_tmp paths with `${CF_PROJECT_ROOT}`
- Updating CLAUDE.md hook descriptions

### Out of Scope

- Changes to sandbox enforcement hooks (other than protected-resource staging fix)
- New hook scripts
- Fixing the upstream settings merge bug (GitHub #17017, #19487)

## Acceptance Criteria

- [x] Settings templates include allowedDomains
- [x] Both settings files updated with autonomous template
- [x] Documentation updated

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-006-001 | Add sandbox network allowedDomains to settings templates and update docs | complete | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Key Findings

### 1. Settings Merge Bug (GitHub #17017, #19487)

The `ask` arrays in Claude Code settings are UNIONED across all settings layers. An empty `ask: []` in the local settings file does NOT clear project-level ask rules. This means a strict project configuration combined with an autonomous local configuration does NOT work as expected -- the strict `ask` rules persist.

**Workaround:** Use autonomous template on both `settings.json` and `settings.local.json`, combined with an empty global `ask` array.

### 2. Four-Layer Settings Hierarchy

Claude Code settings are merged in this order (later layers override earlier):

1. Enterprise `managed-settings.json`
2. Global `~/.claude/settings.json`
3. Project `.claude/settings.json`
4. Project local `.claude/settings.local.json`

### 3. Three Independent Security Layers

Claude Code has three security mechanisms that operate independently:

| Layer | Mechanism | Controlled By |
|-------|-----------|---------------|
| Permissions | allow/ask/deny arrays in settings | Settings merge hierarchy |
| Sandbox | OS-level Seatbelt (macOS) / Landlock (Linux) | `sandbox.network.allowedDomains`, filesystem rules |
| Hooks | PreToolUse / PostToolUse scripts | Hook scripts in `.claude/hooks/` |

`bypassPermissions: true` does NOT bypass sandbox network checks. The sandbox is a separate OS-level enforcement layer.

### 4. sandbox.network.allowedDomains

This is a static configuration at the OS proxy level, separate from the permission system. It defines which domains the sandbox allows network access to. Added 7 standard domains across all templates:

- `github.com`, `api.github.com`, `*.githubusercontent.com`, `objects.githubusercontent.com`
- `registry.npmjs.org`, `pypi.org`, `files.pythonhosted.org`

### 5. CF_PROJECT_ROOT Environment Variable

Added `CF_PROJECT_ROOT` generation to the session-start hook. This enables hooks and enforcement config to use dynamic project root paths instead of hardcoded values. The protected-resource hook uses `CF_PROJECT_ROOT` with a fallback to `git rev-parse --show-toplevel`.

## Files Changed

| File | Change |
|------|--------|
| `.claude/settings-templates/autonomous.json` | Added `sandbox.network.allowedDomains` |
| `.claude/settings-templates/permissive.json` | Added `sandbox.network.allowedDomains` |
| `.claude/settings-templates/standard.json` | Added `sandbox.network.allowedDomains` |
| `.claude/CLAUDE.md` | Updated protected-resource hook description |
| `.claude/agents/cf-security.md` | Dynamic `$CF_PROJECT_ROOT` paths, prohibited `.state/` for staging |
| `.claude/hooks/codeflow/session-start/cf-session-start-init.sh` | Added `CF_PROJECT_ROOT` env variable generation |
| `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-protected-resource.sh` | Staging area exception uses `CF_PROJECT_ROOT` with fallback |
| `.claude/skills/cf-sandbox-standards/SKILL.md` | Added two-layer sandbox model documentation |
| `.codeflow/config/enforcement/enforcement-policy.json` | `managed_tmp` paths use `${CF_PROJECT_ROOT}` |

## Test Results

- All permission tests pass: mkdir, git fetch, cp/rm, git push all work without prompts
- Session-start hook test suite updated and passing
- Protected-resource hook test suite updated and passing

## Technical Notes

Builds on the cf-sandbox-standards skill created in PR #29 (INF-TSK-FEAT-GENL-003).

## Related

- INF-TSK-FEAT-GENL-003 (cf-sandbox-standards skill, PR #29, merged)
