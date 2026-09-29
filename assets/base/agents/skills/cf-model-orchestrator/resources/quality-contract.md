# Duo quality contract

This resource is the portable contract shared by Claude Code, Codex, and any
other capable host. Project rules may strengthen it but must not weaken it
silently.

It loads by section. The sections marked "every task" hold rules that do not
change from unit to unit: read them once per session and reuse them for each
unit, rereading only after compaction or when a section changed. Per unit,
the plan fields and the completion record are what change. Read each other
section when its trigger applies. Each section is the one home for its
duties; a trigger that fires later in the run loads its section then.

| Section | Read |
|---|---|
| [Plan fields](quality/plan.md) | every task |
| [Design and implementation quality](quality/design-implementation.md) | every task |
| [Materiality and prioritization](quality/materiality.md) | every task |
| [Evidence ledger](quality/evidence.md) | every task |
| [Responsible authority and data](quality/authority.md) | every task |
| [Required verification](quality/verification.md) | every task |
| [Coverage](quality/coverage.md) | every task |
| [Independent review](quality/review.md) | every task |
| [Completion: the acceptance block and PR body](quality/completion.md) | every task |
| [Blocker navigation and gate redness](quality/blockers-and-gates.md) | when a step is blocked or would depart from what was approved, or a check or CI job is red or did not finish |
| [Review findings and repair](quality/findings.md) | when a defect is fixed, or review findings are briefed, written or acted on |
| [Parallel execution contract](quality/parallel.md) | when work fans out into parallel tasks |
| [Editorial quality](quality/editorial.md) | when substantial prose is written, or its presentation is reviewed |
| [UI and design verification](quality/ui-design.md) | when a user-facing surface or its design intent changes |
| [Catastrophic and irreversible actions](quality/irreversible.md) | when an action is catastrophic or irreversible |
| [Performance, scale and concurrency](quality/performance.md) | when a changed path is performance-, scale-, or concurrency-sensitive |
| [Research, analysis and planning runs](quality/research-planning.md) | when the run is research, analysis or planning only |
