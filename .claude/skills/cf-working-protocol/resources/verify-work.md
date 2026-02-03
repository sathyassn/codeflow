# verify-work - Extended Guide (PCV Edition)

**CHECKPOINT:** Run Post-Completion Verification (PCV) BEFORE claiming work complete.
Do not claim "done" until ALL steps pass and ALL issues are resolved.

## PCV Overview

Post-Completion Verification (PCV) is a 5-step procedure that ensures work is genuinely complete, claims are verified against sources, and issues are tracked and resolved.

**Core Principle:** Show, don't just claim. Every verification must have EVIDENCE.

## The 5-Step PCV Procedure

### Step 1: Enumerate Work Artifacts

**Purpose:** Create an audit trail of everything you did.

**What to enumerate:**

| Artifact Type | Examples |
|---------------|----------|
| Files touched | `src/auth.ts` (edited), `tests/auth.test.ts` (created) |
| Claims made | "JWT tokens expire after 24 hours", "The API supports pagination" |
| References used | `Skill('cf-git-workflow')`, `docs/frameworks/auth.md:45-89` |
| Decisions made | "Used bcrypt over argon2 for password hashing" |

**Why this matters:**

- Creates explicit scope for verification
- Prevents claims from being untracked
- Makes gaps visible immediately

### Step 2: Claim-Source Verification

**Purpose:** Verify every claim against an authoritative source.

**Verification table format:**

```text
| Claim | Source | Verified | Method |
|-------|--------|----------|--------|
| JWT expires in 24h | src/config.ts:23 | Y | Read source |
| bcrypt is secure | OWASP docs | Y | External verified |
| API rate limits | Assumed | N | Not verified |
```

**Verification methods (in order of rigor):**

1. **Read source directly** - Strongest evidence
2. **Grep/search confirmed** - Pattern exists in codebase
3. **Tested behavior** - Ran code/command to verify
4. **Cited documentation** - Referenced authoritative doc

### Step 3: Adversarial Self-Interrogation

**Purpose:** Actively try to break your own work before claiming complete.

**Required questions:**

| Question | Must Provide |
|----------|--------------|
| What assumption did I make that could be wrong? | Specific assumption |
| If someone reviews this, what will they question? | Specific concern |
| What's the most likely way this work fails? | Specific failure mode |
| What did I NOT verify that I should have? | Specific gap |

**"Nothing" is NOT an acceptable answer.** Every work has gaps or assumptions.

### Step 4: Correlation Check

**Purpose:** Ensure every user requirement has corresponding work.

```text
| Requirement (from user) | Work Done (artifact) | Verified |
|-------------------------|----------------------|----------|
| Add login endpoint | src/auth/login.ts:15-45 | Y |
| Include rate limiting | NOT DONE | N |
| Write tests | tests/auth.test.ts | Y |
```

**Rules:**

- If any requirement has no corresponding artifact - INCOMPLETE
- If any artifact has no verification - UNVERIFIED
- Missing requirements must be addressed before claiming complete

### Step 5: Thoroughness Checklist

Quick validation after Steps 1-4:

1. **Requirements:** All user requirements addressed? Nothing deferred without stating?
2. **Regression:** Existing functionality preserved? No broken references?
3. **Quality:** Standards followed? Skills invoked for file types?
4. **Feasibility:** Can this actually work? Dependencies available?
5. **Coherence:** No contradictions? Parts integrate correctly?
6. **Edge Cases:** What could go wrong? Boundary conditions handled?

## Issue Tracking (MANDATORY)

### Issue Status Definitions

| Status | Meaning | Required Action |
|--------|---------|-----------------|
| **FIXED** | Agent resolved proactively | Show what was fixed, verify resolution |
| **ESCALATED** | Requires user decision | Explain WHY user input needed, provide options |
| **BLOCKED** | Cannot proceed | Explain blocker, show alternatives attempted |

### Issue Tracker Format

```text
ISSUES FOUND:
| ID | Found In | Issue | Resolution | Status |
|----|----------|-------|------------|--------|
| I-1 | Step 2 | Claim about API pagination not verified | Searched docs, found pagination spec | FIXED |
| I-2 | Step 4 | Rate limiting requirement not addressed | Needs user decision on limits | ESCALATED |
| I-3 | Step 3 | Can't test OAuth without real credentials | Mocked tests only, documented gap | BLOCKED |
```

### Completion Gate

Before claiming complete, verify:

- [ ] All issues tracked (none hidden or glossed over)
- [ ] All FIXED issues actually resolved (re-verified after fix)
- [ ] All ESCALATED issues have clear questions with options for user
- [ ] All BLOCKED issues have documented blockers and alternatives

## Tier Classification

### Tier 1: Trivial (Single Line Output)

**ONLY when ALL true:**

- Zero files touched
- Zero technical claims made
- Purely informational response

**Examples:** "The file is at src/config.ts", "Current branch is main"

### Tier 2: Standard Work (Evidence Required)

**When ANY true:**

- Files touched OR created
- Technical claims made
- Skills/operations referenced
- Decisions made

### Tier 3: Complex Work (Full PCV)

**When ANY true:**

- Multiple files across components
- Cross-references between systems
- Architectural decisions
- Security/performance critical

## Quick Reference Card

```text
PCV CHECKLIST:
Step 1: ENUMERATE - List files, claims, references, decisions
Step 2: VERIFY - Each claim has source with verification method
Step 3: ADVERSARIAL - Answer 4 questions honestly (no "nothing")
Step 4: CORRELATE - Each requirement maps to artifact
Step 5: THOROUGHNESS - 6-point quick check

ISSUE TRACKING:
All issues have status: FIXED / ESCALATED / BLOCKED
FIXED issues re-verified
ESCALATED has clear question + options
BLOCKED shows what was attempted

TIER SELECTION:
Zero files + zero claims - Tier 1
Files OR claims - Tier 2
Complex/multi-component - Tier 3
```
