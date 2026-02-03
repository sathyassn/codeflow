# respond-organized - Extended Guide

## Response Calibration Matrix

| Request Type | Response Length | Structure |
|--------------|----------------|-----------|
| Simple confirmation | 1-2 sentences | Direct answer only |
| Simple question | 1 paragraph | Answer + brief context |
| Medium task | 2-3 paragraphs | Summary + key details |
| Complex implementation | Multiple sections | Full structure (summary/details/rationale) |
| Analysis/planning | Structured sections | Progressive disclosure with headings |

## Length Matching - Execution

**Simple request:**

"Fixed. The typo in README.md:15 is corrected."

**Medium request:**

Summary: I'll implement feature X by doing A, B, C.

I'll create ComponentA for behavior 1, ComponentB for behavior 2, and integrate them in ModuleZ. This provides [key benefit] while maintaining [important constraint].

**Complex request:**

Summary: I'll refactor the authentication system across 5 files.

Details: [Organized explanation with logical flow]

Rationale: [Essential reasoning for approach]

## Verbosity Management

**Avoid:**

- Exhaustive lists when subset suffices
- Repeating information already stated
- Over-explaining standard concepts
- Apologetic padding ("I apologize...", "Let me...", "Here's what I'll do...")
- Meta-commentary about your thinking process

**Prefer:**

- Direct answers
- Essential information only
- Appropriate depth for complexity
- Confidence without over-explanation

## Think-Then-Respond Pattern

1. Read user request fully
2. Understand scope and complexity
3. Determine appropriate response length
4. Structure response (summary/details/rationale as needed)
5. Write response (not stream-of-consciousness)
6. Review: Is this appropriate length? Too verbose?
7. Deliver

**Anti-pattern:** Blurting thoughts as they come (stream-of-consciousness)

**Correct pattern:** Organized, measured response after thinking

## Response Format Selection

**Threshold:** Based on structure, not lines

**When to write files vs inline:**

| Content Type | Format | Trigger |
|--------------|--------|---------|
| Simple answer | Inline | No formal structure, < 50 lines |
| Quick analysis | Inline | Conversational, no headings |
| Structured analysis | File + summary | Has headings, ToC, or sections |
| New documentation | File + summary | User requested docs or formal structure |
| Complex reports | File + summary | Multiple sections, tables, diagrams |

**Pattern for file-based responses:**

```markdown
[2-3 sentence summary of what was created/analyzed]

**Details:** [file-path] - [brief description]

**Key findings:**
- Point 1
- Point 2
- Point 3
```

**File location decision:**

| Content Type | Location | Example |
|--------------|----------|---------|
| Work analysis | .claude/memory/{domain}/{topic}/ | agent-comparison-analysis.md |
| Formal proposals | docs/proposals/ | feature-proposal.md |
| Architecture docs | docs/architecture/ | system-design.md |
| User-requested docs | docs/{user-specified}/ | Follow user intent |
| Exploratory work | .claude/memory/general-work/{topic}/ | exploration-notes.md |

**Decision heuristic:** When uncertain - default to memory location. User can request relocation.

## Reference Clarity

**Purpose:** Enable user navigation through precise file:line references

### Hyperlink Format Patterns

**Files:** `path/to/file.md:123` (always include line number when referencing specific content)

**Functions:** `functionName in path/to/file.ts:456`

**Sections:** `docs/guide.md#section-name`

**External:** `[Source Name](full-url)`

**Example:** "The validateInput function in src/utils/validation.ts:45 handles this case."

### Line Number Determination

**When you know exact line:** Use it

**When uncertain:** Use approximate range (e.g., `:100-150`)

**When editing file:** Update line references in same response

**When file not yet read:** Omit line number, note will add after reading

### Anti-Patterns

**Missing line numbers:**

"The function is in src/utils/validation.ts"

Problem: User must search file manually (could be 1000+ lines)

**Vague file references:**

"It's in the utils folder"

Problem: Which file? Which line?

**No context for functions:**

"The validateInput function handles this"

Problem: Which file contains validateInput?

**Correct patterns:**

- Files: `src/utils/validation.ts:45` (specific line)
- Functions: `validateInput in src/utils/validation.ts:45`
- External: `[Claude Code Hooks](https://docs.claude.com/hooks)` (verified URL)
- Sections: `docs/guide.md#installation` (with anchor)
