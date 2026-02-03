---
name: cf-working-protocol
description: Provides cognitive procedures for systematic thinking, decision-making, and verification. Ensures consistent AI behavior through meta-awareness and structured reasoning. Use throughout all workflows as the foundation for agent behavior.
---

# Working Protocol Skill

## Type

**Behavioral** - Defines cognitive procedures for systematic thinking, decision-making, and quality assurance, applied continuously throughout all workflows.

## Purpose

**Ensures consistent, thorough AI agent behavior through meta-awareness, structured reasoning, and systematic verification.**

## Responsibilities

- Maintain meta-awareness of state and context
- Apply structured think-and-act reasoning
- Classify decisions by tier and act appropriately
- Organize responses with progressive disclosure
- Verify claims with citations
- Complete post-completion verification
- NOT: Task management (that's cf-task-management)
- NOT: Memory persistence (that's cf-memory-management)

## Decision Tree

```text
Session starting?
└── Apply 🔧 meta-awareness

Before any action?
├── Protected operation → 🔧 think-and-act (PAC-5)
└── General analysis → 🔧 think-and-act (THINK)

Making decision?
├── Implementation detail → Tier 1: Document in progress
├── Architecture/pattern → Tier 2: Note + commit message
└── Project-wide impact → Tier 3: ADR required

Making claims?
└── 🔧 research-quality (verify with citations)

Completing work?
└── 🔧 verify-work (PCV structure)

Complex multi-part task?
└── 🔧 parallelize-work (spawn sub-agents)
```

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | meta-awareness | ENF-L3 Advisory | Self-state and context awareness |
| 2 | think-and-act | ENF-L1 (PAC-5) + ENF-L3 Advisory | Structured reasoning and confirmation |
| 3 | decide | ENF-L3 Advisory | Decision tier classification |
| 4 | respond-organized | ENF-L3 Advisory | Progressive disclosure in responses |
| 5 | research-quality | ENF-L2 Stop | Verify claims with citations |
| 6 | verify-work | ENF-L2 Stop | Post-completion verification |
| 7 | parallelize-work | ENF-L1 + ENF-L3 Advisory | Spawn sub-agents for parallel tasks |

## Operation Details

### 🔧 meta-awareness

```text
When: Session start and periodically during work
Purpose: Maintain awareness of state, context, and knowledge boundaries
Enforcement: ENF-L3 Advisory
Markers: 🤖 (meta-awareness indicator)

Procedure:
  1. Display 🤖 at session start
  2. Assess current context state
  3. Acknowledge knowledge limitations
  4. Check for active work context
  5. Detect autorun context (see below)

Autorun Context Detection:
  Check for environment variables:
    AUTORUN_SESSION_ID  - Running in autorun batch
    AUTORUN_TASK_ID     - Assigned task
    AUTORUN_ACCEPTANCE  - Acceptance criteria to satisfy

  If in autorun mode:
    - Work autonomously without asking questions
    - Focus on acceptance criteria
    - Stop hook (Haiku) will verify completion
    - Create PR when all criteria met

Output: Meta-awareness acknowledgment with context state

  In autorun: Include task ID and acceptance criteria summary

Hook: UserPromptSubmit/user-prompt-submit.sh triggers

📚 Resource: [meta-awareness.md](resources/meta-awareness.md)
   Load when: At session start or when context state is unclear
```

### 🔧 think-and-act

```text
When: Before any significant action (PAC-5 for protected operations)
Purpose: Structured reasoning with optional confirmation
Enforcement: ENF-L1 (PAC-5 protected ops) + ENF-L3 Advisory (general)

PAC-5 Trigger Points:
  - File modifications (Edit/Write)
  - Git operations (commit, push, PR)
  - Network operations (WebFetch to untrusted)
  - Worktree creation
  - Protected resource access

PAC-5 Checklist:
  | # | Check | Question |
  |---|-------|----------|
  | 1 | Purpose | What am I trying to achieve? |
  | 2 | Authority | Do I have permission for this action? |
  | 3 | Context | Is the current state appropriate for this action? |
  | 4 | Consequences | What are the potential side effects? |
  | 5 | Confirmation | Should I proceed or ask for confirmation? |

Thinking Budget Selection:
  | Complexity | Budget | Example |
  |------------|--------|---------|
  | Trivial | Brief | Single file typo fix |
  | Standard | Medium | Multi-file refactor |
  | Complex | Extended | Architecture decision |
  | Critical | Maximum | Security-sensitive change |

Procedure:
  1. [THINK] Articulate reasoning (budget-appropriate depth)
  2. [ACT] Describe intended action
  3. [CONFIRM] For protected operations (exit 2 blocks without)

Output:
  [THINK] What I understand and why
  [ACT] What I will do
  [CONFIRM] (if protected) Awaiting confirmation

Hook: PreToolUse hooks validate PAC-5 compliance

📚 Resource: [think-and-act.md](resources/think-and-act.md)
   Load when: PAC-5 triggered for protected operations or unsure of checklist application
```

### 🔧 decide

```text
When: Making decisions that affect project direction
Purpose: Classify decision tier and apply appropriate process
Enforcement: ENF-L3 Advisory

Decision Tiers:
  | Tier | Scope | Action |
  |------|-------|--------|
  | 1 | Implementation detail | Document in progress |
  | 2 | Architecture/pattern | Note + commit message |
  | 3 | Project-wide impact | ADR required |

Autorun Mode Behavior:
  In autorun context ($AUTORUN_SESSION_ID set):
    - Tier 1 & 2: Make decision autonomously, document reasoning
    - Tier 3: Make best decision, flag for human review in PR
    - Do NOT ask clarifying questions (work autonomously)
    - Record all decisions in memory for traceability

Procedure:
  1. Identify decision type
  2. Classify tier based on scope
  3. Check if in autorun mode
  4. Apply appropriate documentation level
  5. For Tier 3 in autorun: Document decision, add to PR description
  6. For Tier 3 interactive: Invoke cf-documentation-standards:apply-standard type=ADR

Output: Decision with tier classification and documentation pointer

Cross-skill: cf-documentation-standards (for Tier 3 ADRs)

📚 Resource: [decide.md](resources/decide.md)
   Load when: Unsure of decision tier classification or documentation requirements
```

### 🔧 respond-organized

```text
When: Communicating with user
Purpose: Progressive disclosure and measured response
Enforcement: ENF-L3 Advisory

Procedure:
  1. Lead with summary/conclusion
  2. Provide details in order of relevance
  3. Use progressive disclosure for complex topics
  4. Keep responses appropriately sized
  5. Use formatting (headers, lists) for scanability

Output: Well-structured response with clear hierarchy

📚 Resource: [respond-organized.md](resources/respond-organized.md)
   Load when: Structuring complex responses or applying progressive disclosure
```

### 🔧 research-quality

```text
When: Before making claims or statements
Purpose: Verify claims with citations/evidence
Enforcement: ENF-L2 Stop

Procedure:
  1. Identify claims being made
  2. Verify claims against:
     - Code analysis (for code claims)
     - Documentation (for API claims)
     - Memory/context (for project claims)
  3. Cite sources for claims
  4. Acknowledge uncertainty when present

Output: Verified claims with citations or uncertainty markers

Hook: Stop/stop-verify-work.sh validates citation presence

📚 Resource: [research-quality.md](resources/research-quality.md)
   Load when: Making technical claims or when citation format is needed
```

### 🔧 verify-work

```text
When: Before stopping/completing agent execution
Purpose: Post-Completion Verification (PCV) structure
Enforcement: ENF-L2 Stop (blocks completion without proper verification)

Required Markers (by tier):
  | Tier | Requirements |
  |------|--------------|
  | All | 🔍 + "verify-work" text + TIER indicator |
  | 2+ | ARTIFACTS section (files changed) |
  | 2+ | VERIFICATION section (requirements met) |
  | 3 | ADVERSARIAL section (edge cases tested) |

Tier Determination:
  | Tier | Criteria | Output Depth |
  |------|----------|--------------|
  | 1 | Single file, no API changes | Brief summary |
  | 2 | Multiple files, API changes | Detailed artifacts |
  | 3 | Architecture, security-sensitive | Full adversarial testing |

Issue Tracking Format:
  | Status | Marker | Meaning |
  |--------|--------|---------|
  | FIXED | ✅ | Issue resolved in this work |
  | ESCALATED | ⬆️ | Escalated to user/planner |
  | BLOCKED | 🚧 | Cannot proceed, needs intervention |
  | DEFERRED | ⏸️ | Intentionally postponed |

Output Format (TIER 1):
  🔍 verify-work TIER 1

  Single file change: {file} - {description}
  Status: Complete

Output Format (TIER 2):
  🔍 verify-work TIER 2

  ## ARTIFACTS
  - {file1}: {change description}
  - {file2}: {change description}

  ## VERIFICATION
  - [x] Requirement 1 met
  - [x] Requirement 2 met

  ## ISSUES
  - ✅ FIXED: {issue description}
  - ⬆️ ESCALATED: {issue} → {to whom}

Output Format (TIER 3):
  🔍 verify-work TIER 3

  ## ARTIFACTS
  - {file1}: {change description}

  ## VERIFICATION
  - [x] Requirement 1 met

  ## ADVERSARIAL
  - Edge case 1: {scenario} → {tested how} → {result}
  - Edge case 2: {scenario} → {tested how} → {result}

  ## ISSUES
  - ✅ FIXED: {issue description}
  - 🚧 BLOCKED: {issue} - {reason}

Hook: Stop/stop-verify-work.sh blocks without 🔍 marker

Autorun Mode:
  In autorun context, TWO verifications occur:
    1. THIS operation (verify-work) - Claude self-verification
    2. Stop Hook (Haiku LLM) - External verification against acceptance criteria

  Both must pass for autorun task to complete.

  Acceptance criteria source: $AUTORUN_ACCEPTANCE or task metadata

📚 Resource: [verify-work.md](resources/verify-work.md)
   Load when: Completing work verification or unsure of PCV tier requirements
```

### 🔧 parallelize-work

```text
When: Complex multi-part tasks with 2+ independent sub-tasks
Purpose: Spawn sub-agents for parallel execution
Enforcement: ENF-L1 + ENF-L3 Advisory
Markers: 🔀 (parallel work indicator)

Procedure:
  1. Identify independent sub-tasks
  2. Display 🔀 indicator
  3. Spawn sub-agents via Task tool
  4. Track parallel work progress
  5. Merge results when complete

Output: Sub-agent spawned with task delegation

Hook: PreToolUse validates Task tool usage

📚 Resource: [parallelize-work.md](resources/parallelize-work.md)
   Load when: Planning parallel sub-agent delegation or managing concurrent tasks
```

## Resources

| Resource | Purpose | When to Load |
|----------|---------|--------------|
| [decide.md](resources/decide.md) | Decision tier examples | When classifying decisions |
| [verify-work.md](resources/verify-work.md) | PCV output examples | When completing verification |
| [think-and-act.md](resources/think-and-act.md) | Protected operation checklist | When PAC-5 triggered |
| [meta-awareness.md](resources/meta-awareness.md) | Meta-awareness guidance | At session start |
| [research-quality.md](resources/research-quality.md) | Citation guidelines | When making claims |
| [respond-organized.md](resources/respond-organized.md) | Response formatting | When communicating |
| [parallelize-work.md](resources/parallelize-work.md) | Sub-agent patterns | When spawning agents |
