# Rework Plan: PR #64 (fix/stale-session-pid-detection)

## Status: IN PROGRESS

- PR #64: CLOSED (not merged)
- Branch: fix/stale-session-pid-detection (6 commits on origin)
- Date: 2026-02-23

---

## Root Cause Analysis

### What went wrong

1. cf-development created 3 test files with invented names that don't match any source script
2. cf-review classified naming issues as MINOR and passed them through
3. cf-quality-assurance only checked pass/fail, missed structural violations
4. Team lead (me) gave vague test instructions without specifying naming conventions
5. No agent challenged assumptions about naming, placement, or conventions

### Why it happened

- Agent definitions lack rigorous self-challenge and completion checklist sections
- Review agent allows MINOR/NOTE findings to pass
- QA agent doesn't validate test infrastructure structure
- Lead didn't research test-config.json or existing test files before assigning work

---

## Scope of Rework (2 tracks)

### Track A: Fix branch test issues

### Track B: Strengthen all agent definitions + CLAUDE.md

---

## Track A: Branch Fixes

### A1. Merge test files into existing correct files

Evidence (verified via Glob):

- `test-cf-post-tool-use-pathflow-sentinel.sh` EXISTS at `.codeflow/testing/claude-hooks/post-tool-use/`
- `test-cf-session-start-init.sh` EXISTS at `.codeflow/testing/claude-hooks/session-start/`
- `test-cf-worktree-setup.sh` EXISTS at `.codeflow/testing/scripts/worktree/`

Actions:

| Wrong File (DELETE after merge) | Merge Into (EXISTING) | Tests to Move |
|---|---|---|
| `test-cf-post-tool-use-pathflow-team.sh` | `test-cf-post-tool-use-pathflow-sentinel.sh` | 18 tests for TeamCreate/Task handlers |
| `test-cf-session-start-pid-cleanup.sh` | `test-cf-session-start-init.sh` | 16 tests for PID cleanup |
| `test-cf-worktree-selective-symlink.sh` | `test-cf-worktree-setup.sh` | 31 tests for selective symlinks |

### A2. Update test-config.json

Remove 3 wrong entries:

- Line 103: `scripts/worktree/test-cf-worktree-selective-symlink.sh`
- Line 117: `claude-hooks/post-tool-use/test-cf-post-tool-use-pathflow-team.sh`
- Line 120: `claude-hooks/session-start/test-cf-session-start-pid-cleanup.sh`

(The existing correct test files are already registered)

### A3. Verify all 65 tests pass after merge

Run each merged test file and full suite to confirm no test breakage.

### A4. Status: [ ] NOT STARTED

---

## Track B: Agent Definition & CLAUDE.md Updates

### B1. Design quality sections for ALL 5 on-demand agent definitions

Each agent gets a comprehensive new section (or expanded existing section) with these sub-sections, customized per role:

#### Common sub-sections (all 5 agents)

**1. Self-Challenge Protocol**

- Pre-work questions (what conventions exist? what files exist? what am I assuming?)
- During-work red flags (inventing names, creating when extending exists, copying patterns without verifying)
- Pre-completion verification (would I accept this in review?)

**2. Project Convention Compliance**

- File naming: test files = `test-{source-script-name}.sh`, hooks = `cf-{event}-{purpose}.sh`
- Directory placement: per CLAUDE.md Section 9 project structure
- Registration: test-config.json for tests, settings.json for hooks
- Discovery method: Glob/Grep existing files BEFORE creating new ones

**3. Assumption Identification & Verification**

- Must explicitly state every assumption
- Must verify each against evidence (file existence, config schema, naming patterns)
- Verification via tools (Grep/Glob/Read), NEVER from memory

**4. Infrastructure Wiring Checks**

- Config file registrations complete and correct
- Cross-references valid (test→source, hook→settings matcher)
- Dependencies resolve (imports, source paths, lib references)

**5. Completion Checklist**

- Role-specific, detailed, verifiable assertions
- ALL items must pass before claiming done
- BLOCKING — one failure = work is not done

#### Role-specific additions

**cf-development:**

- Technical feasibility check before implementation
- Code-to-test mapping verification (every test file maps to a source)
- Self-review step: "Would I accept this in a code review?"
- Convention research: read existing patterns before creating new files

**cf-review:**

- Zero tolerance: NO MINOR/NOTE tier. Everything blocks.
- Structural review: naming, placement, registration, concern separation
- Assumption challenging: identify and verify 5+ assumptions per review
- Convention audit: every new file checked against project patterns
- Acceptance criteria: each criterion gets PASS/FAIL with file:line evidence

**cf-quality-assurance:**

- Pre-test structural validation (before running any tests)
- Test-to-source mapping: every test file name matches a source script
- Config completeness: test-config.json entries verified
- Structural fail = QA FAIL even if all tests pass functionally

**cf-planning:**

- Convention research requirement before proposing new files
- Design feasibility assessment against project structure
- Assumption documentation in all design output

**cf-documentation:**

- Technical accuracy: every path, script name, command verified to exist
- Cross-reference validation against actual codebase state

### B2. Update CLAUDE.md

**Section 5 (Task Specification Quality):**
Add to mandatory template:

```text
Conventions: {what naming/location conventions apply}
Existing files: {list existing files that may be extended}
Registration: {what config files need updating}
```

Add rule: Lead MUST research existing test files before assigning test creation.

**Section 7 (Enforcement):**
Add zero tolerance review policy statement.

### B3. Status: [ ] NOT STARTED

---

## Execution Sequence

| Step | Track | Action | Teammate | Status |
|------|-------|--------|----------|--------|
| 1 | B | Design quality sections for all 5 agents | agent-def-designer (Opus) | [ ] |
| 2 | A | Merge 3 test files + delete wrong ones + update test-config.json | cf-development (Opus) | [ ] |
| 3 | B | Implement agent definition updates (all 5 .md files) | cf-development (Opus) | [ ] |
| 4 | B | Update CLAUDE.md (staged to /tmp, user applies) | cf-development (Opus) | [ ] |
| 5 | A+B | Commit all changes | cf-git-operations-2 | [ ] |
| 6 | — | Re-review with NEW strict rules | cf-review (Opus, fresh) | [ ] |
| 7 | — | Re-QA with structural checks | cf-quality-assurance (Sonnet, fresh) | [ ] |
| 8 | — | Squash, push, create new PR | cf-git-operations-2 | [ ] |

Note: Steps 1-4 can partially overlap. Step 1 (design) must complete before Step 3 (implement defs). Steps 2 and 1 are independent and can run in parallel.

---

## Quality Gate (PR will NOT be created until ALL pass)

- [ ] Every test file name matches its source script name exactly
- [ ] No orphan test files (wrong-named files deleted)
- [ ] test-config.json has only correct entries
- [ ] All 65 tests pass in their new locations
- [ ] Full test suite passes (no regressions)
- [ ] All 5 agent definitions have comprehensive quality sections
- [ ] CLAUDE.md has test specification and zero-tolerance updates
- [ ] cf-review re-reviews with strict rules and finds ZERO issues
- [ ] cf-quality-assurance passes both structural AND functional checks
- [ ] No MINOR or NOTE findings — everything resolved

---

## Files to Modify

### Track A (branch fixes)

- `.codeflow/testing/claude-hooks/post-tool-use/test-cf-post-tool-use-pathflow-sentinel.sh` (EXTEND)
- `.codeflow/testing/claude-hooks/session-start/test-cf-session-start-init.sh` (EXTEND)
- `.codeflow/testing/scripts/worktree/test-cf-worktree-setup.sh` (EXTEND)
- `.codeflow/testing/claude-hooks/post-tool-use/test-cf-post-tool-use-pathflow-team.sh` (DELETE)
- `.codeflow/testing/claude-hooks/session-start/test-cf-session-start-pid-cleanup.sh` (DELETE)
- `.codeflow/testing/scripts/worktree/test-cf-worktree-selective-symlink.sh` (DELETE)
- `.codeflow/testing/test-config.json` (UPDATE — remove 3 entries)

### Track B (agent defs + CLAUDE.md)

- `.claude/agents/cf-development.md` (UPDATE)
- `.claude/agents/cf-review.md` (UPDATE)
- `.claude/agents/cf-quality-assurance.md` (UPDATE)
- `.claude/agents/cf-planning.md` (UPDATE)
- `.claude/agents/cf-documentation.md` (UPDATE)
- `.claude/CLAUDE.md` (UPDATE — staged to /tmp)
