# Design Principles for Restructured Artifacts

**MANDATORY:** Every executor working on INF-EPC-019 tasks MUST read this file before starting any work. These 6 principles and the SPINE mandate govern ALL restructuring decisions.

## Principle 0: Restore, Review, Revamp -- Do Not Blindly Restore

This restructuring restores the skill-based architecture that existed before V4 Phase 4 flattened it. The archived skills have the correct structure: functional domain scoping, declaratively named operations, decision trees, and When/Purpose/Procedure/Example operation format. The implementation tasks start from archived skill versions, but archived content may be STALE -- procedures, file paths, and conventions may have evolved since archival.

### No Blind Restore

Archived skills are a STARTING POINT, not the final product. Every implementer MUST:

1. **Read the archived skill** -- understand its structure and content
2. **Read the CURRENT agent definition(s)** that will consume this skill -- identify improvements made since archival
3. **Read the CURRENT CLAUDE.md sections** relevant to this skill's domain (if applicable) -- identify any awareness content that informs procedures
4. **Read the CURRENT command files** that reference this skill's domain (if applicable) -- identify conventions the skill must align with
5. **Reconcile all sources** -- the final skill must reflect the CURRENT state of the project, not the archived state. If the archived version and current artifacts disagree, the current artifact is authoritative for behavior; the archived version is authoritative for structure.

This applies to ALL skill tasks: RESTORE + UPDATE (003, 015, 016, 006, 007), RESTORE + EXTRACT (002, 004), and NEW skills that reference archived patterns (001, 005).

## Principle 1: Awareness vs Procedure Separation

| In CLAUDE.md (Awareness) | In Skills (Procedures) |
|--------------------------|----------------------|
| "8 teammates exist, here is the roster" | "How to spawn each teammate, with detailed prompts" |
| "PathFlow has 7 phases, PF1 through PF7" | "Step-by-step execution for each phase with task tracker mirroring" |
| "Enforcement uses 3 layers" | "Sentinel creation details, checkpoint system internals" |
| "Recovery commands: /cf-resume, /cf-doctor" | "Full recovery procedure for context overflow, stuck sessions" |

## Principle 2: Skill Reference over Inline Duplication

Agent definitions should reference shared skills instead of duplicating content:

```text
BEFORE (in every agent def, 12 lines each x 8 agents = 96 lines):
    ## Working Protocol
    Apply cf-working-protocol skill throughout all work:
    | Operation | When | Purpose |
    ... (full table)

AFTER (in every agent def, 2 lines each x 8 agents = 16 lines):
    ## Skills
    - cf-working-protocol: Apply throughout all work
```

## Principle 3: ASCII Art Only (No Mermaid)

All workflow diagrams across all artifacts (CLAUDE.md, agent defs, commands, skills) use ASCII art exclusively. Mermaid diagrams are prohibited -- they consume more tokens for equivalent information and require rendering support. Commands and skills should use ASCII art diagrams as quick-reference entry points:

```text
BEFORE (verbose text description):
    "When starting a development session, first the SessionStart hook
    fires automatically. Then the team lead creates the team via
    TeamCreate..."

AFTER (ASCII diagram + concise steps):
    SESSION START --> SessionStart hook (auto)
        |
        v
    PF1-INIT --> TeamCreate + spawn cf-security
        |
        v
    PF2-CONTEXT --> spawn cf-knowledge-layer + query active work
        ...
```

## Principle 4: Minimal Cross-References

Each artifact should contain only what its consumer needs. Cross-references use file paths, not inline copies:

```text
BEFORE (inline copy of communication patterns in each agent):
    | Recipient | When | Format |
    | cf-git-operations | Code ready | "Please commit: ..." |
    | cf-knowledge-layer | Progress update | "DEV-UPDATE: ..." |
    ... (30 lines repeated per agent)

AFTER (reference to shared skill):
    ## Communication
    For standard patterns (commit requests, progress updates, escalations):
    Load cf-team-communication skill.
    This agent's specific peers: cf-git-operations, cf-knowledge-layer.
```

## Principle 5: Progressive Disclosure in Commands

Commands should show the workflow overview first, then link to details:

```text
/cf-develop workflow:
    USER REQUEST --> classify work type
        |
        v
    PF3-CLASSIFY --> create branch
        |
        v
    PF4-EXECUTE --> WS-DEV (cf-development)
        |           |
        |           v
        |       WS-REV (cf-review)
        |           |
        |           v
        |       WS-QA (cf-quality-assurance)
        v
    PF5-VERIFY --> PF6-COMPLETE --> PF7-END

For detailed phase procedures: .claude/skills/cf-pathflow-protocol/SKILL.md
For teammate spawn patterns: .claude/skills/cf-pathflow-protocol/SKILL.md
```

## Principle 6: Workflow Diagram as Structural SPINE

The workflow diagram is NOT decoration -- it IS the primary structural backbone of every artifact. Every artifact's organization revolves around its workflow diagram. Each step node in the diagram tells the reader exactly what happens and where to find the detailed procedure. No dead ends. No disconnected sections.

**Pattern: Diagram --> Steps --> Supporting Sections**

```text
ARTIFACT STRUCTURE:

    1. WORKFLOW DIAGRAM (the SPINE)
       Every node annotated with:
       - What happens at this step
       - Which skill/operation handles the HOW
       - Which section below contains details
       |
       v
    2. EXECUTION STEPS (matching diagram nodes 1:1)
       Each step heading corresponds to a diagram node.
       Contains the WHAT/WHEN for this agent.
       References a specific skill operation for the HOW.
       |
       v
    3. SUPPORTING SECTIONS (hang off the diagram)
       Quality Checklist, Error Handling, Constraints --
       these exist to serve the workflow, not independently.
```

### Diagrams MUST Have Accompanying Execution Steps

A workflow diagram alone is NOT sufficient. Every diagram MUST be followed by execution steps that elaborate each node 1:1. Diagrams without steps are decorative. Steps without diagrams lack structure. Both are required.

### Diagram Decomposition for LLM Consumption

Complex workflows MUST be decomposed into multiple levels:

```text
LEVEL 1: HIGH-LEVEL OVERVIEW DIAGRAM
    Shows the happy path from start to finish.
    Each node is a phase or major step.
    Annotations point to Level 2 sub-diagrams.

LEVEL 2: DETAILED SUB-DIAGRAMS (one per branch/scenario)
    Shows decision points, error paths, edge cases.
    Each sub-diagram covers ONE branch of the Level 1 diagram.
    Keeps individual diagrams small and focused.
```

LLMs process sequential text -- a single monolithic ASCII tree with 20+ nodes degrades comprehension. Decomposed diagrams (high-level overview + focused sub-diagrams per scenario) allow the LLM to navigate top-down: understand the overall flow first, then drill into the relevant branch. This is the same progressive disclosure principle applied to visual structure.

**Annotated workflow node format:**

```text
RECEIVE assignment                              --> Step 1, this file
    |
    v
EXPLORE codebase                                --> Step 2, this file
    |                                               Load cf-code-exploration
    v
IMPLEMENT changes                               --> Step 3, this file
    |                                               Domain-specific SOPs inline
    v
COMMIT via cf-git-operations                    --> Step 4, this file
    |                                               cf-team-communication: commit-request
    v
REPORT completion                               --> Step 5, this file
    |                                               cf-team-communication: stage-complete
    v
DONE
```

Every step node has two annotations: (1) which execution step in THIS file elaborates the WHAT/WHEN, and (2) which skill operation provides the HOW. Decision points show branching paths. Terminal nodes show outcomes. No node is a dead end -- every path leads somewhere concrete.

**Why this matters for restructuring:** When procedural content moves from agent definitions to skills, the workflow diagram becomes the connective tissue. Without it, an agent definition is a disconnected list of references. With it, the agent definition is a navigable map -- the reader follows the diagram top-to-bottom and knows exactly where to look for any detail.

## Anti-Patterns

| Anti-Pattern | Why It Fails | Correct Approach |
|-------------|-------------|------------------|
| Inline duplication of shared patterns | Maintenance burden, token waste across agents | Extract to shared skill, reference by name |
| Mermaid diagrams in any artifact | Higher token cost, rendering dependency | ASCII art only |
| Agent def without workflow SPINE | Disconnected sections, no navigation | Diagram as structural backbone |
| CLAUDE.md containing step-by-step procedures | Always-loaded content bloat | Move HOW to on-demand skills |
| Consolidating distinct functional domains into one skill | Violates one-skill-per-domain principle | Separate skills per domain (e.g., 3 knowledge skills, not 1) |
| Working Protocol table in every agent def | 96 lines of pure duplication | Remove entirely; skill loaded by SessionStart hook |
| Stub references ("see skill for details") without context | Reader has no idea what to expect | Anchored references: What it is, When to use, What to do |

## Artifact Interaction Flow

```text
SESSION START
    |
    v
Claude Code auto-loads CLAUDE.md (~400 lines, ~3K tokens)
    |
    v
SessionStart hook loads cf-working-protocol (410 lines, ~3K tokens)
    |
    v
Total always-loaded: ~6K tokens  (54% reduction from ~13K)
    |
    v
Team lead needs phase procedures?
    |
    +-- YES --> Load cf-pathflow-protocol (~500 lines, ~4K tokens)
    +-- NO  --> Skip (tokens saved)
    |
    v
Team lead spawns function teammate (cf-git-operations, cf-knowledge-layer, cf-security)
    |
    +-- Teammate reads slim agent def (~140-200 lines, ~1-1.5K tokens)
    |       |
    |       +-- No Working Protocol section (removed)
    |       +-- No Execution Steps (moved to per-agent skill)
    |       +-- Loads domain skill(s) on-demand
    |       +-- Loads cf-team-communication on-demand for messaging
    |
    v
Team lead spawns role teammate (cf-development, cf-planning, etc.)
    |
    +-- Teammate reads agent def (~380-625 lines, ~2.5-4.5K tokens)
    |       |
    |       +-- No Working Protocol section (removed)
    |       +-- Trimmed Communication (common patterns in cf-team-communication)
    |       +-- SOPs remain inline (role-specific)
    |       +-- Loads shared skills on-demand as needed
    |
    v
Command invoked
    |
    +-- Command file loaded (~300 lines, ~2K tokens)
            |
            +-- Single-line working protocol reference
            +-- ASCII diagram (no Mermaid)
```
