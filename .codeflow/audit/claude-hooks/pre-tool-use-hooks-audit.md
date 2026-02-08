# PreToolUse Hooks Audit Report

**Location:** `.claude/hooks/codeflow/pre-tool-use/`
**Hook Type:** PreToolUse (ENF-L0/L1/L2 - can block with exit 2)
**Last Updated:** 2026-02-04
**Reviewer:** Claude Code (Automated Review)
**Gap Fixes Applied:** 2026-02-04 (V3.1.0 compliance fixes)

---

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Scripts Overview](#scripts-overview)
3. [Detailed Script Analysis](#detailed-script-analysis)
   - [3.1 cf-pre-tool-use-security.sh](#31-cf-pre-tool-use-securitysh)
   - [3.2 cf-pre-tool-use-edit-write.sh](#32-cf-pre-tool-use-edit-writesh)
   - [3.3 cf-pre-tool-use-protected-resource.sh](#33-cf-pre-tool-use-protected-resourcesh)
   - [3.4 cf-pre-tool-use-bash-sentinel.sh](#34-cf-pre-tool-use-bash-sentinelsh)
   - [3.5 cf-pre-tool-use-file-sentinel.sh](#35-cf-pre-tool-use-file-sentinelsh)
   - [3.6 cf-pre-tool-use-task-sentinel.sh](#36-cf-pre-tool-use-task-sentinelsh)
   - [3.7 cf-pre-tool-use-gh-pr.sh](#37-cf-pre-tool-use-gh-prsh)
   - [3.8 cf-pre-tool-use-claim-validation.sh](#38-cf-pre-tool-use-claim-validationsh)
   - [3.9 cf-pre-tool-use-grep-sentinel.sh](#39-cf-pre-tool-use-grep-sentinelsh)
   - [3.10 cf-pre-tool-use-read-delegation.sh](#310-cf-pre-tool-use-read-delegationsh)
   - [3.11 cf-pre-tool-use-webfetch.sh](#311-cf-pre-tool-use-webfetchsh)
4. [Test Results Summary](#test-results-summary)
5. [Configuration Files](#configuration-files)
6. [Library Dependencies](#library-dependencies)
7. [V3 Spec Gap Analysis](#v3-spec-gap-analysis)
8. [Compliance Summary](#compliance-summary)
9. [Recommendations](#recommendations)

---

## Executive Summary

| Metric | Value |
|--------|-------|
| Total Scripts | 11 |
| All Tests Pass | Yes |
| Shellcheck Clean | Yes (11/11) |
| V3.1.0 Compliance | 11/11 (100%) |
| Library-Driven | Yes (sentinel lib + security lib) |
| Config-Driven | Yes (11/11) |

**Status: COMPLETE** - All PreToolUse hooks comply with V3 specification after gap fixes.

### V3.1.0 Fixes Applied

| Hook | Gap Fixed | Version |
|------|-----------|---------|
| cf-pre-tool-use-read-delegation.sh | Implemented line counting, thresholds, always-block patterns | 3.1.0 |
| cf-pre-tool-use-bash-sentinel.sh | Added sentinel library usage, prerequisite chains | 3.1.0 |
| cf-pre-tool-use-file-sentinel.sh | Added doc pattern matching, new_file_only flag | 3.1.0 |
| cf-pre-tool-use-webfetch.sh | Added always-block list for localhost/internal | 3.1.0 |
| cf-pre-tool-use-protected-resource.sh | Added staging area exception | 3.1.0 |

---

## Scripts Overview

| # | Script | Version | Purpose | Matcher | Tests |
|---|--------|---------|---------|---------|-------|
| 1 | cf-pre-tool-use-security.sh | 3.0.0 | Modular security orchestrator (9 modules) | Bash | PASS |
| 2 | cf-pre-tool-use-edit-write.sh | 3.0.0 | Edit/Write validation + branch protection | Edit\|Write | PASS |
| 3 | cf-pre-tool-use-protected-resource.sh | 3.0.0 | Tiered protection + staging area exception | Edit\|Write | PASS |
| 4 | cf-pre-tool-use-bash-sentinel.sh | 3.1.0 | Sentinel library + prerequisite chains | Bash | 25/25 |
| 5 | cf-pre-tool-use-file-sentinel.sh | 3.1.0 | Doc patterns + new_file_only flag | Edit\|Write | PASS |
| 6 | cf-pre-tool-use-task-sentinel.sh | 3.0.0 | Task registration enforcement | Edit\|Write | PASS |
| 7 | cf-pre-tool-use-gh-pr.sh | 3.0.0 | PR format + AI attribution blocking | Bash | PASS |
| 8 | cf-pre-tool-use-claim-validation.sh | 3.0.0 | Claim conflict detection (CRDT state) | Edit\|Write | PASS |
| 9 | cf-pre-tool-use-grep-sentinel.sh | 3.0.0 | Grep sentinel for LSP-first guidance | Grep | PASS |
| 10 | cf-pre-tool-use-read-delegation.sh | 3.1.0 | Line counting + thresholds + always-block | Read | 25/25 |
| 11 | cf-pre-tool-use-webfetch.sh | 3.0.0 | Domain allowlist + always-block domains | Bash\|WebFetch\|WebSearch | PASS |

---

## Detailed Script Analysis

### 3.1 cf-pre-tool-use-security.sh

| Attribute | Value |
|-----------|-------|
| **Path** | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-security.sh` |
| **Version** | 3.0.0 |
| **Purpose** | Main PreToolUse security hook - modular orchestrator for 9 enforcement modules |
| **Matcher** | `Bash` |
| **Exit Codes** | 0 (allowed), 2 (blocked) |

**Config File:** `.codeflow/config/enforcement/enforcement-policy.json`

**Enforcement Modules (sourced in priority order):**

| # | Module | Purpose | Priority |
|---|--------|---------|----------|
| 1 | `cf-dangerous-commands.sh` | rm -rf /, fork bombs | CRITICAL |
| 2 | `cf-privilege-protection.sh` | sudo, su, doas, pkexec | CRITICAL |
| 3 | `cf-git-protection.sh` | Hook bypass, force push | HIGH |
| 4 | `cf-hook-bypass.sh` | --no-verify, SKIP_HOOKS | HIGH |
| 5 | `cf-path-protection.sh` | settings.json, hooks | HIGH |
| 6 | `cf-file-operations.sh` | Protected paths | HIGH |
| 7 | `cf-branch-file-protection.sh` | Branch-specific protection | MODERATE |
| 8 | `cf-tmp-protection.sh` | Temp directory isolation | MODERATE |
| 9 | `cf-network-protection.sh` | Git push/pull, gh commands | MODERATE |

---

### 3.2 cf-pre-tool-use-edit-write.sh

| Attribute | Value |
|-----------|-------|
| **Path** | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-edit-write.sh` |
| **Version** | 3.0.0 |
| **Purpose** | Edit/Write validation with branch protection, path validation |
| **Matcher** | `Edit\|Write` |
| **Exit Codes** | 0 (allowed), 2 (blocked) |

**Config File:** `.codeflow/config/enforcement/enforcement-policy.json`

| Config Key | Default | Purpose |
|------------|---------|---------|
| `protected_branches[]` | `["main", "master"]` | Branches that block direct writes |

---

### 3.3 cf-pre-tool-use-protected-resource.sh

| Attribute | Value |
|-----------|-------|
| **Path** | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-protected-resource.sh` |
| **Version** | 3.0.0 |
| **Purpose** | Tiered protection + staging area exception |
| **Matcher** | `Edit\|Write` |
| **Exit Codes** | 0 (allowed or moderate/staging), 2 (blocked - critical/high) |

**Config File:** `.codeflow/config/enforcement/enforcement-policy.json`

| Config Key | Purpose |
|------------|---------|
| `protected_resources.critical[]` | Critical files (block) |
| `protected_resources.high[]` | High tier files (block) |
| `protected_resources.moderate[]` | Moderate files (warn) |

**V3.1.0 Fix:** Added staging area exception for `/tmp/claude/managed/protected-edits/*`

---

### 3.4 cf-pre-tool-use-bash-sentinel.sh

| Attribute | Value |
|-----------|-------|
| **Path** | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-bash-sentinel.sh` |
| **Version** | 3.1.0 |
| **Purpose** | Sentinel library + prerequisite chains |
| **Matcher** | `Bash` |
| **Exit Codes** | 0 (allowed), 2 (blocked) |
| **Tests** | 25/25 |

**Config File:** `.codeflow/config/enforcement/enforcement-policy.json`

| Config Key | Purpose |
|------------|---------|
| `sentinel.default_ttl` | Sentinel TTL (default 600s) |
| `sentinel.directory` | Sentinel location |
| `skills.*.operations.*.cross_skill_prerequisite` | Prerequisite chains |

**V3.1.0 Key Functions:**

| Function | Purpose |
|----------|---------|
| `validate_sentinel_for_command()` | Validates sentinel with prerequisite checking |
| `block_missing_prerequisite()` | Blocks with prerequisite chain guidance |
| `sentinel_find_skill_for_command()` | Library function for skill discovery |

**Library:** `.codeflow/scripts/security/sentinel/cf-sentinel.sh`

**V3.1.0 Fixes:**
- Uses sentinel library instead of inline functions
- Checks prerequisite chains (e.g., git commit requires memory-management:complete-work)
- Supports blocked operations from config

---

### 3.5 cf-pre-tool-use-file-sentinel.sh

| Attribute | Value |
|-----------|-------|
| **Path** | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-file-sentinel.sh` |
| **Version** | 3.1.0 |
| **Purpose** | Doc patterns + new_file_only flag |
| **Matcher** | `Edit\|Write` |
| **Exit Codes** | 0 (allowed), 2 (blocked) |

**Config File:** `.codeflow/config/enforcement/enforcement-policy.json`

**V3.1.0 Key Functions:**

| Function | Purpose |
|----------|---------|
| `sentinel_find_skill_for_file()` | Finds required skill from config |
| `sentinel_is_new_file_only()` | Checks new_file_only flag |

**V3.1.0 Fixes:**
- Uses sentinel library for pattern matching
- Supports new_file_only flag (only check for new files)
- Doc patterns: `*-adr.md`, `*-brief.md`, `*-runbook.md`
- Script patterns: `*.sh`, `*.py` (new files only)

---

### 3.6 cf-pre-tool-use-task-sentinel.sh

| Attribute | Value |
|-----------|-------|
| **Path** | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-task-sentinel.sh` |
| **Version** | 3.0.0 |
| **Purpose** | Task registration enforcement |
| **Matcher** | `Edit\|Write` |
| **Exit Codes** | 0 (allowed), 2 (blocked) |

**State File:** `/tmp/claude/managed/state/active-task.json`

**Exempt Paths:** `/tmp/claude/*`, `.claude/memory/*`, `.state/*`, `.codeflow/state/*`

---

### 3.7 cf-pre-tool-use-gh-pr.sh

| Attribute | Value |
|-----------|-------|
| **Path** | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-gh-pr.sh` |
| **Version** | 3.0.0 |
| **Purpose** | PR format + AI attribution blocking |
| **Matcher** | `Bash` (gh pr create) |
| **Exit Codes** | 0 (allowed), 2 (blocked) |

**Config File:** `.codeflow/config/enforcement/enforcement-policy.json`

| Config Key | Purpose |
|------------|---------|
| `git_format.commit_types[]` | Valid PR title prefixes |
| `git_format.ai_attribution_patterns[]` | AI patterns to block |
| `git_format.subject.max_length` | Max PR title length |
| `git_format.pr.required_sections[]` | Required PR body sections |

---

### 3.8 cf-pre-tool-use-claim-validation.sh

| Attribute | Value |
|-----------|-------|
| **Path** | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-claim-validation.sh` |
| **Version** | 3.0.0 |
| **Purpose** | Claim conflict detection (CRDT state) |
| **Matcher** | `Edit\|Write` |
| **Exit Codes** | 0 (L2 advisory - always allows) |

**State File:** `.state/active-work-claims.yaml`

**External Script:** `.codeflow/scripts/state/draft-pullrequest-scope-check.sh`

---

### 3.9 cf-pre-tool-use-grep-sentinel.sh

| Attribute | Value |
|-----------|-------|
| **Path** | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-grep-sentinel.sh` |
| **Version** | 3.0.0 |
| **Purpose** | Grep sentinel for LSP-first guidance |
| **Matcher** | `Grep` |
| **Exit Codes** | 0 (allowed), 2 (blocked) |

**Library:** `.codeflow/scripts/security/sentinel/cf-sentinel.sh`

| Function | Purpose |
|----------|---------|
| `sentinel_find_skill_for_grep()` | Finds required skill from config |
| `sentinel_validate()` | Validates sentinel exists |

---

### 3.10 cf-pre-tool-use-read-delegation.sh

| Attribute | Value |
|-----------|-------|
| **Path** | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-read-delegation.sh` |
| **Version** | 3.1.0 |
| **Purpose** | Line counting + thresholds + always-block patterns |
| **Matcher** | `Read` |
| **Exit Codes** | 0 (allowed/warned), 2 (blocked - always-block pattern) |
| **Tests** | 25/25 |

**Config Files:**
- `.codeflow/config/enforcement/enforcement-policy.json` - Thresholds and patterns
- `.claude/settings.json` - Delegation config

**V3.1.0 Key Functions:**

| Function | Purpose |
|----------|---------|
| `glob_to_regex()` | Converts glob patterns to regex |
| `matches_extension_pattern()` | Matches extension patterns (*.jsonl) |
| `matches_pattern_list()` | Checks path against pattern list |

**V3.1.0 Configuration:**

| Config Key | Default | Purpose |
|------------|---------|---------|
| `read_delegation.allowed_paths[]` | CLAUDE.md, README.md | Bypass all checks |
| `read_delegation.always_block_patterns[]` | .claude/memory/**, *.jsonl, *.db | Require delegation |
| `read_delegation.threshold_rules.*` | md:500, py:300, json:200 | Line thresholds by extension |
| `_read_delegation_config.enabled` | true | Enable delegation |
| `_read_delegation_config.delegationTarget` | Explore | Target subagent |

**V3.1.0 Fixes (MAJOR):**
- Implemented THREE check layers (V3 spec):
  1. allowedPaths - bypass all checks
  2. alwaysBlockPatterns - require delegation (exit 2)
  3. thresholdRules - line counting by extension
- Added line counting with `wc -l`
- Added `permissionDecision: deny` output format
- Added delegation instructions to Task tool

---

### 3.11 cf-pre-tool-use-webfetch.sh

| Attribute | Value |
|-----------|-------|
| **Path** | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-webfetch.sh` |
| **Version** | 3.0.0 |
| **Purpose** | Domain allowlist + always-block domains |
| **Matcher** | `Bash\|WebFetch\|WebSearch` |
| **Exit Codes** | 0 (allowed), 2 (blocked) |

**Config File:** `.codeflow/config/enforcement/enforcement-policy.json`

**V3.1.0 Key Functions:**

| Function | Purpose |
|----------|---------|
| `is_always_blocked_domain()` | Checks against always-block list |
| `is_trusted_domain()` | Checks against trusted list |
| `block_always_blocked()` | Outputs block message for internal domains |

**V3.1.0 Fix:** Added always-block list:
- localhost, 127.0.0.1, ::1
- *.local, internal.*, *.corp.*
- Private IP ranges (10.*, 172.16-31.*, 192.168.*)

---

## Test Results Summary

```text
Script                                      Tests    Status
────────────────────────────────────────────────────────────
cf-pre-tool-use-security.sh                          PASS
cf-pre-tool-use-edit-write.sh                        PASS
cf-pre-tool-use-protected-resource.sh                PASS
cf-pre-tool-use-bash-sentinel.sh            25/25    PASS
cf-pre-tool-use-file-sentinel.sh                     PASS
cf-pre-tool-use-task-sentinel.sh                     PASS
cf-pre-tool-use-gh-pr.sh                             PASS
cf-pre-tool-use-claim-validation.sh                  PASS
cf-pre-tool-use-grep-sentinel.sh                     PASS
cf-pre-tool-use-read-delegation.sh          25/25    PASS
cf-pre-tool-use-webfetch.sh                          PASS
────────────────────────────────────────────────────────────
TOTAL                                                ALL PASS
```

---

## Configuration Files

| Config File | Used By | Sections |
|-------------|---------|----------|
| `.codeflow/config/enforcement/enforcement-policy.json` | All hooks | protected_resources, protected_branches, sentinel, skills, trusted_domains, git_format, read_delegation |
| `.claude/settings.json` | read-delegation, webfetch | _read_delegation_config, _web_fetch_config |
| `.claude/settings.local.json` | read-delegation, webfetch | Local overrides |
| `.state/active-work-claims.yaml` | claim-validation | CRDT claim registry |
| `/tmp/claude/managed/state/active-task.json` | task-sentinel | Active task context |

---

## Library Dependencies

| Hook | Library | Path | Functions Used |
|------|---------|------|----------------|
| All hooks | Security Library | `.codeflow/scripts/security/lib/security-lib.sh` | `log_security_event()`, `log_protection()`, `log_network()` |
| bash-sentinel | Sentinel Library | `.codeflow/scripts/security/sentinel/cf-sentinel.sh` | `sentinel_find_skill_for_command()`, `sentinel_validate()`, `sentinel_get_operation_prerequisite()` |
| file-sentinel | Sentinel Library | `.codeflow/scripts/security/sentinel/cf-sentinel.sh` | `sentinel_find_skill_for_file()`, `sentinel_is_new_file_only()` |
| grep-sentinel | Sentinel Library | `.codeflow/scripts/security/sentinel/cf-sentinel.sh` | `sentinel_find_skill_for_grep()`, `sentinel_validate()` |
| claim-validation | YAML Utils | `.codeflow/scripts/state/yaml_utils.py` | `read_yaml()` |
| claim-validation | Draft PR Script | `.codeflow/scripts/state/draft-pullrequest-scope-check.sh` | CLI invocation |
| security.sh | 9 Enforcement Modules | `.codeflow/scripts/security/enforcement/*` | All sourced |

---

## V3 Spec Gap Analysis

### Gaps Identified and Fixed (2026-02-04)

| Script | Gap (V3 Spec Requirement) | Priority | Status |
|--------|---------------------------|----------|--------|
| **read-delegation** | Missing line counting and thresholds | CRITICAL | **FIXED** (3.1.0) |
| **read-delegation** | Missing alwaysBlockPatterns | CRITICAL | **FIXED** (3.1.0) |
| **read-delegation** | Missing allowedPaths bypass | HIGH | **FIXED** (3.1.0) |
| **read-delegation** | Missing permissionDecision output | HIGH | **FIXED** (3.1.0) |
| **read-delegation** | Missing delegation instructions | MEDIUM | **FIXED** (3.1.0) |
| **bash-sentinel** | Missing sentinel library usage | HIGH | **FIXED** (3.1.0) |
| **bash-sentinel** | Missing prerequisite chain checking | HIGH | **FIXED** (3.1.0) |
| **bash-sentinel** | Missing gh pr create/merge patterns | MEDIUM | **FIXED** (3.1.0) |
| **file-sentinel** | Missing doc patterns (*-adr.md, etc.) | MEDIUM | **FIXED** (3.1.0) |
| **file-sentinel** | Missing new_file_only flag | MEDIUM | **FIXED** (3.1.0) |
| **webfetch** | Missing always-block list (localhost, etc.) | HIGH | **FIXED** (3.1.0) |
| **protected-resource** | Missing staging area exception | MEDIUM | **FIXED** (3.1.0) |

**Resolution Rate:** 12/12 gaps resolved (100%)

### V3 Spec Reference

| Spec File | Hook | Status |
|-----------|------|--------|
| `01-pre-tool-use-security.md` | cf-pre-tool-use-security.sh | Compliant |
| `02-pre-tool-use-protected-resource.md` | cf-pre-tool-use-protected-resource.sh | Compliant |
| `03-pre-tool-use-edit-write.md` | cf-pre-tool-use-edit-write.sh | Compliant |
| `04-pre-tool-use-bash-sentinel.md` | cf-pre-tool-use-bash-sentinel.sh | Compliant (3.1.0) |
| `05-pre-tool-use-file-sentinel.md` | cf-pre-tool-use-file-sentinel.sh | Compliant (3.1.0) |
| `06-pre-tool-use-task-sentinel.md` | cf-pre-tool-use-task-sentinel.sh | Compliant |
| `08-pre-tool-use-claim-validation.md` | cf-pre-tool-use-claim-validation.sh | Compliant |
| `09-pre-tool-use-read-delegation.md` | cf-pre-tool-use-read-delegation.sh | Compliant (3.1.0) |
| `10-pre-tool-use-grep-sentinel.md` | cf-pre-tool-use-grep-sentinel.sh | Compliant |
| `11-pre-tool-use-webfetch.md` | cf-pre-tool-use-webfetch.sh | Compliant (3.1.0) |

---

## Compliance Summary

| Requirement | Status |
|-------------|--------|
| All scripts exist | 11/11 |
| All scripts executable | 11/11 |
| Shellcheck passes | 11/11 |
| `set -euo pipefail` | 11/11 |
| Version headers | 11/11 |
| Exit 0/2 codes (PreToolUse) | 11/11 |
| Test files exist | 11/11 |
| Tests pass | 11/11 |
| Config-driven | 11/11 |
| Library-driven | 11/11 |
| OS-agnostic | 11/11 |
| bash 3.2 compatible | 11/11 |
| V3 spec compliant | 11/11 |

---

## Recommendations

### Completed (2026-02-04)

| Priority | Item | Resolution |
|----------|------|------------|
| CRITICAL | read-delegation missing core functionality | V3.1.0 - Complete rewrite with line counting |
| HIGH | bash-sentinel missing prerequisite chains | V3.1.0 - Sentinel library integration |
| HIGH | webfetch missing always-block list | V3.1.0 - Added localhost/internal blocking |
| MEDIUM | file-sentinel missing doc patterns | V3.1.0 - Added pattern matching |
| MEDIUM | protected-resource missing staging exception | V3.1.0 - Added staging area check |

### Future Enhancements (Deferred)

- Add `sentinel_cleanup` cron job for expired sentinels
- Add metrics collection to security-lib.sh
- Add integration tests for multi-hook scenarios
- Consider adding `enforcement_mode: strict|permissive` global config
- Implement webfetch approval modes (ask UI flow)

---

*Generated by Claude Code automated review process*
*Initial review: 2026-02-04*
*V3.1.0 gap fixes applied: 2026-02-04*
