# Part 11: Use Cases

> Six detailed walkthroughs with diagrams: tracked dev session, non-code work, untracked Q&A, autorun session, rework loop, and ad-hoc teammate.

---

## Table of Contents

- [11.1 Use Case A: Tracked Development Session](#111-use-case-a-tracked-development-session)
- [11.2 Use Case B: Non-Code Work (Documentation)](#112-use-case-b-non-code-work-documentation)
- [11.3 Use Case C: Untracked Q&A Session](#113-use-case-c-untracked-qa-session)
- [11.4 Use Case D: Autorun Session](#114-use-case-d-autorun-session)
- [11.5 Use Case E: Rework Loop](#115-use-case-e-rework-loop)
- [11.6 Use Case F: Ad-Hoc Teammate (Security Audit Mid-Session)](#116-use-case-f-ad-hoc-teammate-security-audit-mid-session)

---

## 11.1 Use Case A: Tracked Development Session

**Scenario**: User asks to implement login form validation for task FRT-TSK-FEAT-AUTH-042. Full DEV -> REVIEW -> QA pipeline.

### Timeline

```
User: "Work on FRT-TSK-FEAT-AUTH-042"

== PF-1: SESSION START =========================================================

Lead:     Creates team "feat-auth-42"
Lead:     Spawns cf-knowledge-layer (persistent)
Lead:     Creates [PF-1] marker, init tasks, [PF-2] marker
cf-knowledge-layer: Creates session record in Knowledge Layer
Lead:     Marks [PF-1] complete -> sentinel pathflow:pf-1 created

== PF-2: CONTEXT AWARENESS =====================================================

cf-knowledge-layer: Queries active_work -> none found
cf-knowledge-layer: Loads task FRT-TSK-042 from WorkGraph
              -> title: "Implement login form validation"
              -> acceptance_criteria: [4 criteria listed]
              -> work_type: FEAT, estimate: M
cf-knowledge-layer: Reports context to lead
Lead:     Marks [PF-2] complete -> sentinel pathflow:pf-2 created

== PF-3: WORK CLASSIFICATION ===================================================

Lead:     Analyzes: FEAT type, medium size -> needs DEV + REV + QA
cf-knowledge-layer: Registers active_work entry with task_id
Lead:     Spawns cf-gitops (persistent)
cf-gitops: Creates branch feat/frt-auth-validation
Lead:     Marks [PF-3] complete -> sentinel pathflow:pf-3 created

== PF-4: WORK EXECUTION ========================================================

--- WS-DEV ---
Lead:     Spawns cf-developer (on-demand)
Lead:     Assigns: "Implement login form validation per acceptance criteria"
cf-developer: Reads task requirements and relevant code
cf-developer: Implements validation logic (3 files modified)
cf-developer: Self-tests basic functionality
cf-developer -> cf-gitops: "Please commit: feat(auth): add login form validation"
cf-gitops: Commits as abc1234
cf-developer -> cf-knowledge-layer: "Dev stage complete for FRT-TSK-042. Verdict: pass."
cf-knowledge-layer: Updates WorkGraph (stage=dev, stage_status=complete)
cf-developer -> Lead: "Task complete. Implementation done."
Lead:     Shuts down cf-developer
Lead:     Marks [WS-DEV-DONE] -> sentinel pathflow:ws-dev-done

--- WS-REV ---
Lead:     Spawns cf-reviewer (on-demand)
Lead:     Assigns: "Review login form implementation. Check acceptance criteria."
cf-reviewer: Reviews committed code
cf-reviewer -> cf-knowledge-layer: "Review complete. Verdict: approved."
cf-knowledge-layer: Updates WorkGraph (stage=review, stage_status=complete)
cf-reviewer -> Lead: "Code approved. No issues found."
Lead:     Shuts down cf-reviewer
Lead:     Marks [WS-REV-DONE] -> sentinel pathflow:ws-rev-done

--- WS-QA ---
Lead:     Spawns cf-qa (on-demand)
Lead:     Assigns: "Write and run tests for login form validation."
cf-qa:    Writes 8 test cases based on acceptance criteria
cf-qa:    Runs tests: 8/8 pass, coverage 94%
cf-qa -> cf-gitops: "Please commit: test(auth): add login validation tests"
cf-gitops: Commits as def5678
cf-qa -> cf-knowledge-layer: "QA stage complete. 8/8 tests pass. Coverage 94%."
cf-knowledge-layer: Updates WorkGraph (stage=qa, stage_status=complete)
cf-qa -> Lead: "All tests pass."
Lead:     Shuts down cf-qa
Lead:     Marks [WS-QA-DONE] -> sentinel pathflow:ws-qa-done

== PF-5: WORK VERIFICATION =====================================================

Lead:     Verifies: all 4 acceptance criteria met, tests pass, code reviewed
Lead:     Marks [PF-5] complete

== PF-6: WORK COMPLETION =======================================================

cf-knowledge-layer: Marks FRT-TSK-042 as complete in WorkGraph (stage=done)
cf-gitops: Creates PR #42 against main
cf-gitops -> Lead: "PR #42 created"
Lead:     Marks [PF-6] complete

== PF-7: SESSION END ============================================================

Lead:     Shuts down cf-gitops, cf-knowledge-layer
Lead:     Cleans up team
Lead:     Reports to user: "PR #42 created for FRT-TSK-FEAT-AUTH-042."
```

### Communication Diagram

```
Lead           cf-knowledge-layer    cf-gitops     cf-developer   cf-reviewer   cf-qa
  |                 |              |              |              |           |
  |--- spawn ------>|              |              |              |           |
  |--- spawn -------|------------->|              |              |           |
  |                 |              |              |              |           |
  |--- spawn -------|--------------|------------>|              |           |
  |--- assign ------|--------------|------------>|              |           |
  |                 |              |              |              |           |
  |                 |              |<-- commit ---|              |           |
  |                 |<-- stage ----|              |              |           |
  |<-- complete ----|--------------|--------------|              |           |
  |--- shutdown ----|--------------|------------>|              |           |
  |                 |              |              |              |           |
  |--- spawn -------|--------------|--------------|------------>|           |
  |--- assign ------|--------------|--------------|------------>|           |
  |                 |<-- stage ----|--------------|-------------|           |
  |<-- complete ----|--------------|--------------|-------------|           |
  |--- shutdown ----|--------------|--------------|------------>|           |
  |                 |              |              |              |           |
  |--- spawn -------|--------------|--------------|--------------|---------->|
  |--- assign ------|--------------|--------------|--------------|---------->|
  |                 |              |<-- commit ---|--------------|-----------|
  |                 |<-- stage ----|--------------|--------------|-----------|
  |<-- complete ----|--------------|--------------|--------------|-----------|
  |--- shutdown ----|--------------|--------------|--------------|---------->|
  |                 |              |              |              |           |
  |--- shutdown --->|              |              |              |           |
  |--- shutdown ----|------------->|              |              |           |
  |                 |              |              |              |           |
```

---

## 11.2 Use Case B: Non-Code Work (Documentation)

**Scenario**: User asks to write API reference documentation for the Knowledge Layer. Work type DOCS, stages WORK -> REVIEW.

### Timeline

```
User: "Write API reference docs for the Knowledge Layer"

== PF-1: SESSION START =========================================================

Lead:     Creates team "docs-knowledge"
Lead:     Spawns cf-knowledge-layer (persistent)

== PF-2: CONTEXT AWARENESS =====================================================

cf-knowledge-layer: No active work found
Lead:     No existing task for this -- new work

== PF-3: WORK CLASSIFICATION ===================================================

Lead:     Analyzes: documentation work, no existing task
cf-knowledge-layer: Creates task DOC-TSK-DOCS-GENL-015 in ongoing docs epic
              work_type=DOCS, estimate=S
cf-knowledge-layer: Registers active_work entry
Lead:     Determines stages: WORK + REVIEW (no QA for docs)
Lead:     Spawns cf-gitops
cf-gitops: Creates branch docs/doc-knowledge-api-ref

== PF-4: WORK EXECUTION ========================================================

--- WS-WORK ---
Lead:     Spawns cf-documenter (on-demand)
Lead:     Assigns: "Write API reference for Knowledge Layer"
cf-documenter: Reads existing schema and operations docs
cf-documenter: Writes API reference document
cf-documenter -> cf-gitops: "Please commit: docs(knowledge): add API reference"
cf-gitops: Commits
cf-documenter -> cf-knowledge-layer: "Work stage complete for DOC-TSK-015"
cf-documenter -> Lead: "Documentation written."
Lead:     Shuts down cf-documenter

--- WS-REV ---
Lead:     Spawns cf-reviewer (on-demand)
Lead:     Assigns: "Review documentation for accuracy and completeness"
cf-reviewer: Reviews against actual schema and code
cf-reviewer -> cf-knowledge-layer: "Review complete. Verdict: approved."
cf-reviewer -> Lead: "Documentation approved."
Lead:     Shuts down cf-reviewer

== PF-5 through PF-7 ===========================================================

(Same as Use Case A: verify, complete, create PR, end session)
```

### Key Differences from Use Case A

| Aspect | Feature Dev (Case A) | Documentation (Case B) |
|--------|---------------------|----------------------|
| Work stages | DEV -> REV -> QA | WORK -> REV |
| Primary agent | cf-developer | cf-documenter |
| QA stage | Yes (tests) | No (not applicable) |
| Review focus | Code correctness | Content accuracy |
| Task creation | Pre-existing task | Created on the fly |

---

## 11.3 Use Case C: Untracked Q&A Session

**Scenario**: User asks "How does the authentication module work?" No work to track, no files to modify.

### Timeline

```
User: "How does the authentication module work?"

== PF-1: SESSION START (lightweight) ==========================================

Lead:     Creates session record (minimal)
          No team creation
          No cf-knowledge-layer spawned

== PF-2: CONTEXT AWARENESS ====================================================

Lead:     Reads user prompt: question about auth module
          No active work to detect
          Classification: UNTRACKED (exploration/question)

== NO FURTHER PATHFLOW PHASES ==================================================

Lead:     Uses Explore sub-agents to read auth module code
Lead:     Synthesizes findings
Lead:     Responds to user with explanation

          No PF-3 through PF-7.
          No WorkGraph updates.
          No sentinels.
          No team.
          Session ends when user is satisfied.
```

### Diagram

```
+-----+     +------+     +----------+
| PF-1|---->| PF-2 |---->| UNTRACKED|
+-----+     +------+     +----------+
                               |
                          Direct Q&A
                          (no PathFlow)
                               |
                          Session ends
```

**What makes this untracked**: No files modified, no commits, no work products. The lead detects this from the user's prompt (a question, not a work request) and skips PathFlow phases beyond PF-2.

**Transition to tracked**: If the user then says "Actually, there's a bug in the auth module. Fix it." the lead transitions:

```
+-----+     +------+     +----------+     +------+     +------+
| PF-1|---->| PF-2 |---->| UNTRACKED|---->| PF-3 |---->| PF-4 |---> ...
+-----+     +------+     +----------+     +------+     +------+
                               |               ^
                               +--- user says --+
                                   "fix the bug"
```

---

## 11.4 Use Case D: Autorun Session

**Scenario**: Autorun batch runner picks up task FRT-TSK-FIX-AUTH-087 (bug fix). Stages: DEV + QA (review optional for small fixes). No human interaction.

### Timeline

```
Autorun worker starts with task_id=FRT-TSK-FIX-AUTH-087

== PF-1: SESSION START (automatic) ============================================

Lead:     Creates team "autorun-fix-087"
Lead:     Spawns cf-knowledge-layer
cf-knowledge-layer: Creates session record (autorun=true)

== PF-2: CONTEXT AWARENESS (automatic) ========================================

cf-knowledge-layer: Loads task FRT-TSK-FIX-AUTH-087
              -> title: "Fix race condition in token refresh"
              -> work_type: FIX, estimate: S
              -> acceptance_criteria: [2 criteria]
              -> autorun_eligible: true

== PF-3: WORK CLASSIFICATION (automatic) ======================================

Lead:     FIX type, small -> stages: DEV + QA (no review)
cf-knowledge-layer: Registers active_work
Lead:     Spawns cf-gitops
cf-gitops: Creates branch fix/frt-auth-token-race

== PF-4: WORK EXECUTION (automatic) ===========================================

--- WS-DEV ---
Lead:     Spawns cf-developer
cf-developer: Reads task description and acceptance criteria
cf-developer: Identifies the race condition
cf-developer: Implements fix (1 file changed)
cf-developer -> cf-gitops: "Please commit: fix(auth): resolve token refresh race"
cf-developer -> cf-knowledge-layer: "Dev complete. Verdict: pass."
Lead:     Shuts down cf-developer

--- WS-QA ---
Lead:     Spawns cf-qa
cf-qa:    Writes regression test for the race condition
cf-qa:    Runs full test suite: 42/42 pass
cf-qa -> cf-gitops: "Please commit: test(auth): add token race regression test"
cf-qa -> cf-knowledge-layer: "QA complete. 42/42 pass. Coverage 91%."
Lead:     Shuts down cf-qa

== PF-5: WORK VERIFICATION (automatic) ========================================

Lead:     Programmatically checks acceptance criteria:
          [x] "Token refresh no longer races under concurrent requests"
              -> Regression test passes
          [x] "Existing auth tests still pass"
              -> Full suite passes
          Verdict: PASS

== PF-6: WORK COMPLETION (automatic) ==========================================

cf-knowledge-layer: Marks task complete (stage=done)
cf-gitops: Creates PR #87
Lead:     Records result in autorun_task_runs table

== PF-7: SESSION END (automatic) ==============================================

Lead:     Shuts down all teammates
Lead:     Reports: { status: "completed", pr_number: 87, duration: 420s }

Autorun worker: Picks up next task from batch queue.
```

### Autorun Flow Diagram

```
Batch Queue          Autorun Worker           PathFlow
+---------+          +-------------+          +---------+
| Task 087|--------->| Start       |--------->| PF-1    |
| Task 088|          | session     |          | PF-2    |
| Task 089|          |             |          | PF-3    |
| ...     |          |             |          | PF-4    |
+---------+          |             |          |  WS-DEV |
                     |             |          |  WS-QA  |
                     |             |          | PF-5    |
                     |             |          | PF-6    |
                     |             |<---------| PF-7    |
                     |             |          +---------+
                     | Pick next   |
                     | task        |--------->| PF-1    |
                     | ...         |          | ...     |
```

---

## 11.5 Use Case E: Rework Loop

**Scenario**: During a feature development session, the reviewer finds issues. Work routes back to DEV, then re-enters REVIEW.

### Timeline

```
(PF-1 through PF-3 same as Use Case A -- skipped for brevity)

== PF-4: WORK EXECUTION ========================================================

--- WS-DEV (iteration 1) ---
cf-developer: Implements login form validation
cf-developer -> cf-gitops: "Commit: feat(auth): add login form validation"
cf-developer -> cf-knowledge-layer: "Dev complete. Verdict: pass."
Lead:     Marks [WS-DEV-DONE]

--- WS-REV (iteration 1) ---
cf-reviewer: Reviews implementation
cf-reviewer: FINDING: "Missing error handling for network timeout in validateToken()"
cf-reviewer -> cf-developer: "Changes requested: add network timeout handling"
cf-reviewer -> cf-knowledge-layer: "Review complete. Verdict: changes_requested."
cf-reviewer -> Lead: "Review failed. Routing to rework."

Lead:     Notes: iteration 1 review failed.
Lead:     Creates rework tasks:

  TASK                                              ASSIGNED         BLOCKED BY
  #N+1  Rework: add network timeout handling        cf-developer     #16 (review)
  #N+2  Commit rework                               cf-gitops        #N+1
  #N+3  [WS-REV] Re-review                          (marker)         #N+2
  #N+4  Re-review rework changes                    cf-reviewer      #N+3

--- WS-DEV (iteration 2 -- rework) ---
cf-developer: Reads review findings
cf-developer: Adds try/catch for network timeout in validateToken()
cf-developer: Self-tests the error path
cf-developer -> cf-gitops: "Commit: fix(auth): add network timeout handling"
cf-developer -> cf-knowledge-layer: "Dev rework complete. Iteration 2."

--- WS-REV (iteration 2) ---
cf-reviewer: Reviews the fix
cf-reviewer: Original finding addressed. No new issues.
cf-reviewer -> cf-knowledge-layer: "Review complete. Verdict: approved. Iteration 2."
cf-reviewer -> Lead: "Code approved on second review."
Lead:     Marks [WS-REV-DONE]

--- WS-QA ---
(Proceeds normally -- cf-qa writes tests, all pass)

== PF-5 through PF-7 (same as Use Case A) =====================================
```

### Rework Loop Diagram

```
                            Iteration 1                    Iteration 2
                    +---------------------------+  +---------------------------+
                    |                           |  |                           |
  [WS-DEV] ------> | cf-developer implements   |  | cf-developer fixes        |
                    | Commits via cf-gitops     |  | Commits via cf-gitops     |
                    +---------------------------+  +---------------------------+
                                |                              |
                                v                              v
  [WS-REV] ------> +---------------------------+  +---------------------------+
                    | cf-reviewer reviews       |  | cf-reviewer re-reviews    |
                    | Verdict: CHANGES_REQUESTED|  | Verdict: APPROVED         |
                    +---------------------------+  +---------------------------+
                                |                              |
                                |                              v
                                +--- Route back to DEV    [WS-REV-DONE]
                                                               |
                                                               v
                                                          [WS-QA] ...
```

### Stage History After Rework

The WorkGraph task's `stage_history` after this session:

```json
[
  { "stage": "dev",    "verdict": "pass",              "iteration": 1 },
  { "stage": "review", "verdict": "changes_requested", "iteration": 1 },
  { "stage": "dev",    "verdict": "pass",              "iteration": 2, "rework": true },
  { "stage": "review", "verdict": "approved",          "iteration": 2 },
  { "stage": "qa",     "verdict": "pass",              "iteration": 1 }
]
```

### Rework Limit Reached (Autorun Variant)

If this were an autorun session with `max_rework_iterations=3`:

```
Iteration 1: DEV -> REV (changes_requested) -> back to DEV
Iteration 2: DEV -> REV (changes_requested) -> back to DEV
Iteration 3: DEV -> REV (changes_requested) -> LIMIT REACHED

Lead:     Marks task as BLOCKED
          Reason: "Rework limit exceeded after 3 iterations"
          Records in WorkGraph
          Skips to PF-7 (Session End)
          Task remains in WorkGraph for human review
```

---

## 11.6 Use Case F: Ad-Hoc Teammate (Security Audit Mid-Session)

**Scenario**: During a feature development session, the lead discovers that the implementation touches authentication logic. No predefined role covers security auditing, so the lead spawns an ad-hoc teammate.

### Timeline

```
(PF-1 through PF-3 complete as in Use Case A)

== PF-4: WORK EXECUTION ========================================================

--- WS-DEV ---
Lead:     Spawns cf-developer
cf-developer: Implements OAuth token refresh logic
cf-developer -> Lead: "Implementation touches auth token storage and refresh.
                       This is security-sensitive code."

Lead:     Decides: this needs a security review before standard code review.
          The predefined roster has no security specialist.

Lead:     Spawns ad-hoc teammate:
          name: "security-auditor"
          type: general-purpose
          prompt: "You are a security auditor. Review the OAuth token
                   implementation for OWASP vulnerabilities, token leakage,
                   insecure storage, and timing attacks. Focus on files
                   modified in the current branch."

security-auditor: Reads the modified files
security-auditor: Identifies: token stored in localStorage (XSS risk)
security-auditor -> cf-developer: "Finding: token in localStorage is
                    vulnerable to XSS. Use httpOnly cookies or secure
                    session storage instead."
security-auditor -> Lead: "Security audit complete. 1 critical finding:
                    insecure token storage. Recommendation: switch to
                    httpOnly cookies."

Lead:     Shuts down security-auditor (ad-hoc, one-time use)
Lead:     Routes finding to cf-developer for rework before proceeding to REV

cf-developer: Refactors token storage to httpOnly cookies
cf-developer -> cf-gitops: "Commit: fix(auth): use httpOnly cookies for tokens"

--- WS-REV ---
(Proceeds normally -- reviewer now sees secure implementation)

--- WS-QA ---
(Proceeds normally)
```

### Key Points

This use case demonstrates that the predefined teammate roster is a set of optimized defaults, NOT a constraint:

| Aspect | Predefined Roles | Ad-Hoc Teammate |
|--------|-----------------|-----------------|
| Definition file | `.claude/agents/cf-{role}.md` | None needed (instructions in spawn prompt) |
| Persistence | Function=persistent, Role=on-demand | On-demand (one-time use) |
| Lifecycle | Managed by PathFlow stages | Managed by lead's judgment |
| When spawned | At predefined points (PF-1, PF-3, PF-4) | Whenever the lead identifies a need |

**Other ad-hoc teammate examples**:

- **Explore sub-agent** during PF-2: Spawned to quickly search a large codebase for related modules before work classification
- **Performance analyst** during WS-QA: Spawned to benchmark the implementation when cf-qa finds performance concerns
- **Migration specialist**: Spawned for a one-time database migration task that doesn't fit any standard role
- **Parallel developers**: Two cf-developer instances working on frontend and backend modules simultaneously

---

## Related Documents

- [06-progressive-orchestration.md](06-progressive-orchestration.md) -- Phase progression mechanics
- [07-work-stages.md](07-work-stages.md) -- Stage definitions and routing
- [08-enforcement-model.md](08-enforcement-model.md) -- How enforcement applies in each case
- [09-knowledge-layer-integration.md](09-knowledge-layer-integration.md) -- Data recorded during each use case
- [10-session-lifecycle.md](10-session-lifecycle.md) -- Session types (tracked, untracked, autorun)
