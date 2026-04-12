---
id: "task-01KP0WRNT0XMXFQQH549CCNNAR"
format_id: "INF-TSK-046-004"
epic_id: "epic-01KP0WRNT08J88E09QGQ6185BP"
epic_format_id: "INF-EPC-046"
title: "Claude artifact generalization"
description: "Rewrite all Claude artifacts (CLAUDE.md, agent defs, commands, skills excluded, hooks, templates) to eliminate Rust-specific test references. Replace cargo/nextest/llvm-cov/crate references with stack-agnostic codeflow test wrapper references. gh-pr-guard becomes target-aware. Stack-specific skills (cf-rust-standards, cf-python-standards, cf-shell-standards) remain as on-demand references but are removed from mandatory agent instructions."
status: todo
area_type: "INF"
work_type: "DOCS"
domain: "GENL"
origin: planned
file_scope:
  - ".claude/CLAUDE.md"
  - ".claude/agents/cf-development.md"
  - ".claude/agents/cf-quality-assurance.md"
  - ".claude/agents/cf-review.md"
  - ".claude/agents/cf-git-operations.md"
  - ".claude/agents/cf-knowledge-layer.md"
  - ".claude/agents/cf-planning.md"
  - ".claude/agents/cf-security.md"
  - ".claude/commands/cf-test.md"
  - ".claude/commands/cf-ship.md"
  - ".claude/commands/cf-deploy.md"
  - ".claude/commands/cf-doctor.md"
  - ".claude/commands/cf-approval-mode.md"
  - ".claude/skills/cf-sandbox-standards/SKILL.md"
  - ".claude/skills/cf-working-protocol/SKILL.md"
  - "project-management/templates/task-template.md"
  - "project-management/templates/epic-template.md"
  - "codeflow-cli/core/src/hooks/gh_pr_guard.rs"
  - ".codeflow/config/enforcement/enforcement-policy.json"
scope_policy: hard
scope_root: null
estimate: XL
priority: high
assignee_id: null
autorun_eligible: false
raise_pr: true
auto_merge: false
target_branch: null
acceptance:
  - ".claude/CLAUDE.md contains zero stack-specific terms in the test-related sections (§7 Testing, §7 PR Workflow, §8 CLI, §9 Project Structure, §10 Memory): no 'cargo', 'nextest', 'llvm-cov', 'codeflow-core', 'codeflow-cli crate' appears in any guidance text for teammates. The only remaining references to Rust/cargo are in descriptive prose about CodeFlow's own implementation (e.g., §9 'Rust CLI workspace' describing the project structure) — not in instructional text."
  - ".claude/CLAUDE.md §7 Testing section rewritten to describe codeflow test as the sole authorized test execution path reading declarative test-config.json with first-match-wins per-target coverage rules"
  - ".claude/CLAUDE.md PR Workflow Test Stats block rewritten to the target-aware 5-section structure (Overall Test Pass Status, Overall Coverage, Exempted Files, Modified File Coverage, optional Test Failures + Slowest Tests); CLI crate / Core crate column headers removed"
  - ".claude/agents/cf-quality-assurance.md: every cargo/nextest/llvm-cov reference rewritten; 'Rust Quality Gate' section becomes 'Per-target Quality Gates'; codeflow test --mode full --coverage is the single authoritative invocation"
  - ".claude/agents/cf-development.md: hardcoded Rust-specific test writing guidance removed; #[cfg(test)] mod tests examples move into cf-rust-standards (on-demand skill); replacement text references standards skills as on-demand per target language"
  - ".claude/agents/cf-review.md: Rust-specific dimensions removed from main dimensional-assessment text; examples move into cf-rust-standards"
  - ".claude/agents/cf-git-operations.md: PR-body population section rewrites Test Stats to the target-aware 5-section structure; instructs teammate to source data via `codeflow test report show --format json`"
  - ".claude/agents/cf-knowledge-layer.md: cargo-test-flavored ledger event references rewritten to use the generic test_result_recorded event with target-aware fields"
  - ".claude/agents/cf-planning.md: grep returns zero cargo/nextest/llvm-cov matches"
  - ".claude/agents/cf-security.md: 'Rust memory safety' references moved into cf-rust-standards; main instruction text language-agnostic"
  - ".claude/commands/cf-test.md: invocation rewritten to `codeflow test --mode <mode> [--coverage] [--only <target>] [--skip <target>] [--report]` with a one-line description for each flag"
  - ".claude/commands/cf-ship.md: PR-body checklist uses the 5-section Test Stats format; 'cargo llvm-cov' invocation replaced with 'codeflow test --mode full --coverage'"
  - ".claude/commands/cf-deploy.md: test-related references generalized to codeflow test"
  - ".claude/commands/cf-doctor.md: documents codeflow test doctor subcommand alongside codeflow doctor"
  - ".claude/commands/cf-approval-mode.md: grep returns only non-instructional matches"
  - ".claude/skills/cf-sandbox-standards/SKILL.md: test-runner-specific sandbox exemption language generalized — codeflow test is the invocation surface and sandbox exemption follows from permission on codeflow test, not per-runner enumeration"
  - ".claude/skills/cf-working-protocol/SKILL.md: test-related examples generalized"
  - "project-management/templates/task-template.md: DEV Report and QA Report Test Stats rewritten to 5-section target-aware format; 'Rust Quality Gate' → 'Per-target Quality Gate' with placeholder; 'Suite: cargo test --workspace' label replaced with 'Suite: codeflow test --mode full --coverage'"
  - "project-management/templates/epic-template.md: grep returns zero cargo/nextest/llvm-cov matches"
  - "codeflow-cli/core/src/hooks/gh_pr_guard.rs gate logic: parses PR body markdown via pulldown-cmark (or equivalent); walks the heading tree; verifies '## Test Results' exists; verifies sub-headings '### 1. Overall Test Pass Status', '### 2. Overall Coverage', '### 3. Modified File Coverage' exist with non-empty tables, OR a top-level line matching 'No test targets configured\\.' exists beneath '## Test Results'; AI-attribution + emoji checks preserved; NOT substring-based"
  - "gh-pr-guard unit tests: (a) valid 3-target PR body passes, (b) valid fresh-project PR body passes, (c) missing '## Test Results' fails, (d) incomplete sub-sections fails, (e) AI-attribution fails (regression), (f) emoji fails (regression); per-file coverage ≥85%"
  - "Post-migration grep: `rg -S '\\bcargo\\b|\\bnextest\\b|\\bllvm-cov\\b|\\bcrate\\b|codeflow-core|codeflow-cli crate' .claude/agents/ .claude/commands/ project-management/templates/` returns zero matches (excluding cf-rust-standards skill, which is the intentional on-demand reference)"
  - "Stack-specific skills (cf-rust-standards, cf-python-standards, cf-shell-standards) NOT edited by this task; they remain as on-demand references"
  - "Agent defs' 'Standards & Practices' tables re-label trigger from 'Always' to 'Load when working on <language> targets'"
  - "CLAUDE.md §8 Capabilities: On-Demand Skills section retains cf-rust/python/shell-standards; Active Skills section remains cf-working-protocol only"
  - "Task template's Standards & Practices block documents 'Apply the standards skill for whichever language(s) the task touches'"
  - "Memory notes (`.claude/memory/`) NOT edited by this task — historical records out of scope"
  - "No task deliverable introduces an LLM code path that Edit/Writes into .state/** or .codeflow/config/testing/test-config.json directly. Every mutation has a corresponding codeflow test <subcommand>. This task edits only .claude/ and one hook source file; it does not touch .state/ or test-config.json."
  - "Validation: markdownlint .claude/CLAUDE.md clean; markdownlint on all edited agent defs and commands clean"
  - "Validation: existing agent-def schema validation passes on every edited file"
  - "Manual walkthrough: trace every cf-quality-assurance instruction with a hypothetical Node adopter; confirm zero broken instructions"
  - "Before/after diff summary: for each of the 17 edited files, produce a one-paragraph summary noting specific Rust-text removals"
  - "enforcement-policy.json git_format.pr.required_sections updated from the legacy ['## Summary', '## Testing', '## Test Stats', '### 1. Overall Test Pass Status', '### 2. Overall Coverage', '#### Exempted Files', '### 3. Modified File Coverage'] to the new canonical ['## Summary', '## Testing', '## Test Results', '### 1. Overall Test Pass Status', '### 2. Overall Coverage', '### 3. Modified File Coverage']; the nested #### Exempted Files block is no longer a top-level required section — it renders under §2 Overall Coverage only when exceptions exist; unit test verifies the new array is present and matches gh-pr-guard's validation logic"
  - "gh-pr-guard hook consumes the new enforcement-policy.json required_sections array; round-trip test: render a post-migration PR body via the engine → pass it through gh-pr-guard → assert PASS; render a legacy PR body (with ## Test Stats) → assert FAIL with a clear message 'legacy PR body format; regenerate via `codeflow test --mode full --coverage`'"
tests:
  - "codeflow-cli/core/src/hooks/gh_pr_guard.rs (inline #[cfg(test)] mod tests extended for markdown-parsing-based validation)"
  - "codeflow-cli/tests/integration_artifact_generalization.rs (new integration test: grep-based verification that .claude/ instructional text is stack-agnostic)"
branch: null
pr_number: null
external_id: null
external_url: null
dependencies:
  - "task-01KP0WRNT0BEMCW2T9GQ9V43A0"
created_at: "2026-04-11T00:00:00Z"
updated_at: "2026-04-11T00:00:00Z"
started_at: null
completed_at: null
stage: null
stage_status: null
stage_history: "[]"
---

# INF-TSK-046-004: Claude artifact generalization

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the task validation command before committing:
> `codeflow validate task <file-path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Description

Rewrite every Claude artifact (CLAUDE.md, agent definitions, slash commands, task/epic templates, and the gh-pr-guard hook source) to eliminate Rust-specific test assumptions. Every instruction to a teammate must work identically for a Node, Go, Python, or Rust adopter. The only surviving Rust-specific text allowed is in descriptive prose about CodeFlow's own implementation (e.g., §9 Project Structure mentioning "Rust CLI workspace") and in the on-demand `cf-rust-standards` skill.

After this task, a downstream Node adopter can clone CodeFlow's Claude artifacts, author a Node-appropriate `test-config.json`, and the agents behave correctly — no agent instruction assumes cargo, llvm-cov, or `codeflow-cli` crate names.

This task pairs with TSK-002's engine work (providing the `codeflow test` universal wrapper) and TSK-003's setup wizard (giving adopters a path to author their config).

## Deliverables

| Deliverable | Type | Location | Integration Point |
|-------------|------|----------|-------------------|
| Rewritten CLAUDE.md test sections | doc | `.claude/CLAUDE.md` | Read on every session by the team lead |
| Rewritten 8 agent defs | doc | `.claude/agents/cf-*.md` | Read by each teammate at spawn |
| Rewritten 5 command defs | doc | `.claude/commands/cf-*.md` | Read by teammates receiving `/cf-*` invocations |
| Rewritten 2 skill refs | doc | `.claude/skills/cf-sandbox-standards/SKILL.md`, `.claude/skills/cf-working-protocol/SKILL.md` | Generic test-related examples |
| Rewritten 2 templates | doc | `project-management/templates/{task,epic}-template.md` | Copy-target for new tasks and epics |
| gh-pr-guard markdown-parsed validation | feature | `codeflow-cli/core/src/hooks/gh_pr_guard.rs` | PreToolUse hook on `gh pr create` |

**Expected Outcome:** Every agent instruction text is stack-agnostic. `rg -S '\bcargo\b|\bnextest\b|\bllvm-cov\b|\bcrate\b' .claude/agents/ .claude/commands/ project-management/templates/` returns zero matches (cf-rust-standards skill is intentionally excluded). gh-pr-guard validates PR body structure via markdown parsing, not substring matches.

**Deployment:** PR merge on `docs/claude-artifact-generalization`. Agent def and command def edits are exempt from bypassPermissions prompts; CLAUDE.md requires staged edits per CLAUDE.md Section 5.

## Pre-Work Analysis

- [ ] Reviewed git log for recent changes to `.claude/CLAUDE.md`
- [ ] Verified TSK-002 and TSK-003 merged
- [ ] Confirmed the 5-section Test Stats format is stable in design doc §11
- [ ] Ran the catalog grep below to produce a per-file baseline

## Approach

1. **Inventory.** Run the catalog grep to produce a per-file list of Rust-specific strings.
2. **CLAUDE.md edits** via staged-edit protocol. Focus §7 (Testing), §7 (PR Workflow Test Stats), §8 (Capabilities).
3. **Agent defs** — direct edit (exempt). 8 files. Heaviest work in cf-quality-assurance and cf-development.
4. **Command defs** — direct edit (exempt). 5 files.
5. **Skill files** — edit cf-sandbox-standards and cf-working-protocol only. Do NOT edit cf-rust-standards, cf-python-standards, cf-shell-standards.
6. **Templates** — edit task-template.md and epic-template.md.
7. **gh-pr-guard.rs** — rewrite validation using pulldown-cmark.
8. **Post-migration grep** — run the invariant check; expect zero hits.
9. **Manual walkthrough** — Node-adopter mental simulation.

## Standards & Practices

| Language/Tool | Standards Skill | Key Requirements |
|--------------|----------------|-----------------|
| Markdown | cf-markdown-standards | markdownlint clean, heading structure preserved |
| Rust (gh-pr-guard) | cf-rust-standards | pulldown-cmark parser, thiserror, clippy `-D warnings` clean |

Read the applicable skill BEFORE starting implementation.

## Files

### To Modify

- `.claude/CLAUDE.md` — staged-edit protocol
- `.claude/agents/cf-development.md`
- `.claude/agents/cf-quality-assurance.md`
- `.claude/agents/cf-review.md`
- `.claude/agents/cf-git-operations.md`
- `.claude/agents/cf-knowledge-layer.md`
- `.claude/agents/cf-planning.md`
- `.claude/agents/cf-security.md`
- `.claude/commands/cf-test.md`
- `.claude/commands/cf-ship.md`
- `.claude/commands/cf-deploy.md`
- `.claude/commands/cf-doctor.md`
- `.claude/commands/cf-approval-mode.md`
- `.claude/skills/cf-sandbox-standards/SKILL.md`
- `.claude/skills/cf-working-protocol/SKILL.md`
- `project-management/templates/task-template.md`
- `project-management/templates/epic-template.md`
- `codeflow-cli/core/src/hooks/gh_pr_guard.rs`
- `.codeflow/config/enforcement/enforcement-policy.json` — `git_format.pr.required_sections` array updated to match the new canonical `## Test Results` format (per F1 reconciliation)

### To Create

- `codeflow-cli/tests/integration_artifact_generalization.rs`

### To Read

- `.codeflow/docs/analysis/generic-testing-subsystem.md` §§11, 13
- `.claude/skills/cf-rust-standards/SKILL.md` — where Rust content moves (not edited)
- `.claude/skills/cf-python-standards/SKILL.md` — unchanged
- `.claude/skills/cf-shell-standards/SKILL.md` — unchanged
- Current `codeflow-cli/core/src/hooks/gh_pr_guard.rs` — understand current substring validation before rewriting

### Do NOT edit

- `.claude/skills/cf-rust-standards/SKILL.md`
- `.claude/skills/cf-python-standards/SKILL.md`
- `.claude/skills/cf-shell-standards/SKILL.md`
- `.claude/memory/**` (historical)
- `.claude/settings.json`, `.claude/settings-templates/*.json` (not instructional prose)

## Concurrency Considerations

| Shared Resource | Access Pattern | Safety Mechanism |
|----------------|---------------|-----------------|
| `.claude/CLAUDE.md` | staged-edit | /tmp staging then cp back |
| `.claude/agents/*.md`, `.claude/commands/*.md` | direct edit | exempt path |
| `codeflow-cli/core/src/hooks/gh_pr_guard.rs` | direct edit + build | cargo build after edit |

## Acceptance Criteria

1. `.claude/CLAUDE.md` contains zero stack-specific terms in the test-related sections (§7 Testing, §7 PR Workflow, §8 CLI, §9 Project Structure, §10 Memory): no 'cargo', 'nextest', 'llvm-cov', 'codeflow-core', 'codeflow-cli crate' appears in any guidance text for teammates. The only remaining references to Rust/cargo are in descriptive prose about CodeFlow's own implementation (e.g., §9 'Rust CLI workspace' describing the project structure) — not in instructional text.
2. `.claude/CLAUDE.md` §7 Testing section rewritten to describe `codeflow test` as the sole authorized test execution path reading declarative `test-config.json` with first-match-wins per-target coverage rules
3. `.claude/CLAUDE.md` PR Workflow Test Stats block rewritten to the target-aware 5-section structure (Overall Test Pass Status, Overall Coverage, Exempted Files, Modified File Coverage, optional Test Failures + Slowest Tests); "CLI crate" / "Core crate" column headers removed
4. `.claude/agents/cf-quality-assurance.md`: every cargo/nextest/llvm-cov reference rewritten; 'Rust Quality Gate' section becomes 'Per-target Quality Gates'; `codeflow test --mode full --coverage` is the single authoritative invocation
5. `.claude/agents/cf-development.md`: hardcoded Rust-specific test writing guidance removed; `#[cfg(test)] mod tests` examples move into cf-rust-standards (on-demand skill); replacement text references standards skills as on-demand per target language
6. `.claude/agents/cf-review.md`: Rust-specific dimensions removed from main dimensional-assessment text; examples move into cf-rust-standards
7. `.claude/agents/cf-git-operations.md`: PR-body population section rewrites Test Stats to the target-aware 5-section structure; instructs teammate to source data via `codeflow test report show --format json`
8. `.claude/agents/cf-knowledge-layer.md`: cargo-test-flavored ledger event references rewritten to use the generic `test_result_recorded` event with target-aware fields
9. `.claude/agents/cf-planning.md`: grep returns zero cargo/nextest/llvm-cov matches
10. `.claude/agents/cf-security.md`: 'Rust memory safety' references moved into cf-rust-standards; main instruction text language-agnostic
11. `.claude/commands/cf-test.md`: invocation rewritten to `codeflow test --mode <mode> [--coverage] [--only <target>] [--skip <target>] [--report]` with a one-line description for each flag
12. `.claude/commands/cf-ship.md`: PR-body checklist uses the 5-section Test Stats format; 'cargo llvm-cov' invocation replaced with `codeflow test --mode full --coverage`
13. `.claude/commands/cf-deploy.md`: test-related references generalized to `codeflow test`
14. `.claude/commands/cf-doctor.md`: documents `codeflow test doctor` subcommand alongside `codeflow doctor`
15. `.claude/commands/cf-approval-mode.md`: grep returns only non-instructional matches
16. `.claude/skills/cf-sandbox-standards/SKILL.md`: test-runner-specific sandbox exemption language generalized — `codeflow test` is the invocation surface and sandbox exemption follows from permission on `codeflow test`, not per-runner enumeration
17. `.claude/skills/cf-working-protocol/SKILL.md`: test-related examples generalized
18. `project-management/templates/task-template.md`: DEV Report and QA Report Test Stats rewritten to 5-section target-aware format; 'Rust Quality Gate' → 'Per-target Quality Gate' with placeholder; 'Suite: cargo test --workspace' label replaced with 'Suite: codeflow test --mode full --coverage'
19. `project-management/templates/epic-template.md`: grep returns zero cargo/nextest/llvm-cov matches
20. `codeflow-cli/core/src/hooks/gh_pr_guard.rs` gate logic: parses PR body markdown via pulldown-cmark (or equivalent); walks the heading tree; verifies '## Test Results' exists; verifies sub-headings '### 1. Overall Test Pass Status', '### 2. Overall Coverage', '### 3. Modified File Coverage' exist with non-empty tables, OR a top-level line matching 'No test targets configured\.' exists beneath '## Test Results'; AI-attribution + emoji checks preserved; NOT substring-based
21. gh-pr-guard unit tests: (a) valid 3-target PR body passes, (b) valid fresh-project PR body passes, (c) missing '## Test Results' fails, (d) incomplete sub-sections fails, (e) AI-attribution fails (regression), (f) emoji fails (regression); per-file coverage ≥85%
22. Post-migration grep: `rg -S '\bcargo\b|\bnextest\b|\bllvm-cov\b|\bcrate\b|codeflow-core|codeflow-cli crate' .claude/agents/ .claude/commands/ project-management/templates/` returns zero matches (excluding cf-rust-standards skill, which is the intentional on-demand reference)
23. Stack-specific skills (cf-rust-standards, cf-python-standards, cf-shell-standards) NOT edited by this task; they remain as on-demand references
24. Agent defs' 'Standards & Practices' tables re-label trigger from 'Always' to 'Load when working on <language> targets'
25. CLAUDE.md §8 Capabilities: On-Demand Skills section retains cf-rust/python/shell-standards; Active Skills section remains cf-working-protocol only
26. Task template's Standards & Practices block documents 'Apply the standards skill for whichever language(s) the task touches'
27. Memory notes (`.claude/memory/`) NOT edited by this task — historical records out of scope
28. No task deliverable introduces an LLM code path that Edit/Writes into `.state/**` or `.codeflow/config/testing/test-config.json` directly. Every mutation has a corresponding `codeflow test <subcommand>`. This task edits only `.claude/` and one hook source file; it does not touch `.state/` or `test-config.json`.
29. Validation: markdownlint .claude/CLAUDE.md clean; markdownlint on all edited agent defs and commands clean
30. Validation: existing agent-def schema validation passes on every edited file
31. Manual walkthrough: trace every cf-quality-assurance instruction with a hypothetical Node adopter; confirm zero broken instructions
32. Before/after diff summary: for each of the 17 edited files, produce a one-paragraph summary noting specific Rust-text removals
33. `enforcement-policy.json` `git_format.pr.required_sections` updated from the legacy `["## Summary", "## Testing", "## Test Stats", "### 1. Overall Test Pass Status", "### 2. Overall Coverage", "#### Exempted Files", "### 3. Modified File Coverage"]` to the new canonical `["## Summary", "## Testing", "## Test Results", "### 1. Overall Test Pass Status", "### 2. Overall Coverage", "### 3. Modified File Coverage"]`; the nested `#### Exempted Files` block is no longer a top-level required section — it renders under §2 Overall Coverage only when exceptions exist; unit test verifies the new array is present and matches gh-pr-guard's validation logic
34. gh-pr-guard hook consumes the new `enforcement-policy.json` `required_sections` array; round-trip test: render a post-migration PR body via the engine → pass it through gh-pr-guard → assert PASS; render a legacy PR body (with `## Test Stats`) → assert FAIL with a clear message "legacy PR body format; regenerate via `codeflow test --mode full --coverage`"

### File inventory with Rust-reference counts (pre-edit baseline, per Grep evidence)

| File | Rust-ref count | Disposition |
|------|---------------:|-------------|
| `.claude/CLAUDE.md` | 26 | Rewrite §7, §8 instructional text; retain descriptive prose in §9 Project Structure |
| `.claude/agents/cf-quality-assurance.md` | 20 | Rewrite; heaviest edit; §Rust Quality Gate becomes §Per-target Quality Gates |
| `.claude/agents/cf-development.md` | 14 | Rewrite; `#[cfg(test)] mod tests` examples move to cf-rust-standards |
| `.claude/agents/cf-review.md` | 9 | Rewrite; Rust-specific review focus moves to cf-rust-standards |
| `.claude/agents/cf-security.md` | 9 | Rewrite; memory-safety content moves to cf-rust-standards |
| `.claude/agents/cf-git-operations.md` | 3 | Rewrite PR-body population section |
| `.claude/agents/cf-knowledge-layer.md` | 2 | Rewrite ledger event naming |
| `.claude/agents/cf-planning.md` | 3 | Grep-clean |
| `.claude/commands/cf-doctor.md` | 1 | Add `codeflow test doctor` reference |
| `.claude/commands/cf-approval-mode.md` | 1 | Grep-clean |
| `.claude/commands/cf-test.md` | (edit) | Full rewrite of invocation text |
| `.claude/commands/cf-ship.md` | (edit) | Full rewrite of PR-body checklist |
| `.claude/commands/cf-deploy.md` | (edit) | Test references generalized |
| `.claude/skills/cf-sandbox-standards/SKILL.md` | 2 | Generalize test-runner exemption |
| `.claude/skills/cf-working-protocol/SKILL.md` | 2 | Generalize test examples |
| `project-management/templates/task-template.md` | 7 | Full rewrite of DEV Report and QA Report Test Stats |
| `project-management/templates/epic-template.md` | 0 | Grep-clean (already generic) |
| `codeflow-cli/core/src/hooks/gh_pr_guard.rs` | (code) | Rewrite validation to markdown-parsed |

Total Rust references in instructional text: 99. Expected post-edit count: 0 (excluding explicit descriptive prose in CLAUDE.md §9 and the untouched `cf-rust-standards` skill).

### Scenarios covered

| # | Scenario | Covered by acceptance # |
|---|----------|------------------------|
| 1 | Node adopter reads cf-quality-assurance.md; all instructions work | 4, 31 |
| 2 | Go adopter reads cf-development.md; test-writing guidance works | 5, 31 |
| 3 | Python adopter reads cf-git-operations.md; PR body emits correctly | 7 |
| 4 | Rust adopter loads cf-rust-standards on demand; gets Rust specifics | 23, 24 |
| 5 | CodeFlow self-host still works after artifact rewrite | All (preserves functionality) |
| 6 | gh-pr-guard validates a multi-target PR body | 20, 21 |
| 7 | gh-pr-guard validates a fresh-project PR body | 21 |
| 8 | gh-pr-guard catches missing Test Results section | 21 |
| 9 | gh-pr-guard catches AI attribution (regression) | 21 |
| 10 | Grep invariant passes post-migration | 22 |

### PII Handling Review

- [ ] Does this task involve code that handles PII? (N)

### Criteria Status

> Legend: -- Not evaluated | DONE Implemented | PASS Verified passing | FAIL Verified failing | PARTIAL Partially met | N/A Not applicable
> Pipeline: WS-DOCS -> WS-REV

| # | Criterion | DOCS | REV | Notes |
|---|-----------|------|-----|-------|
| 1 | CLAUDE.md test-related sections stack-agnostic | -- | -- | |
| 2 | CLAUDE.md §7 Testing rewritten | -- | -- | |
| 3 | CLAUDE.md PR Workflow uses 5-section target-aware format | -- | -- | |
| 4 | cf-quality-assurance.md: all Rust refs rewritten; Per-target Quality Gates | -- | -- | |
| 5 | cf-development.md: Rust specifics moved to cf-rust-standards | -- | -- | |
| 6 | cf-review.md: dimensional-assessment generalized | -- | -- | |
| 7 | cf-git-operations.md: PR body section rewritten | -- | -- | |
| 8 | cf-knowledge-layer.md: ledger event naming generic | -- | -- | |
| 9 | cf-planning.md: grep-clean | -- | -- | |
| 10 | cf-security.md: memory-safety content moved | -- | -- | |
| 11 | cf-test.md: flag summary | -- | -- | |
| 12 | cf-ship.md: checklist uses 5-section format | -- | -- | |
| 13 | cf-deploy.md: test references generalized | -- | -- | |
| 14 | cf-doctor.md: references codeflow test doctor | -- | -- | |
| 15 | cf-approval-mode.md: grep-clean | -- | -- | |
| 16 | cf-sandbox-standards: test-runner exemption generalized | -- | -- | |
| 17 | cf-working-protocol: test examples generalized | -- | -- | |
| 18 | task-template.md: 5-section format + Per-target Quality Gate | -- | -- | |
| 19 | epic-template.md: grep-clean | -- | -- | |
| 20 | gh-pr-guard: markdown-parsed validation | -- | -- | |
| 21 | gh-pr-guard: 6 unit-test scenarios pass | -- | -- | |
| 22 | Post-migration grep returns zero hits in instructional text | -- | -- | |
| 23 | Stack-specific skills untouched | -- | -- | |
| 24 | Agent Standards tables re-labeled "Load when working on <lang> targets" | -- | -- | |
| 25 | CLAUDE.md §8 capability table: skills on-demand | -- | -- | |
| 26 | Task template Standards block: load-per-language docstring | -- | -- | |
| 27 | Memory notes NOT edited | -- | -- | |
| 28 | Task preserves `.state/`+test-config.json invariant | -- | -- | |
| 29 | markdownlint clean on CLAUDE.md + all edited artifacts | -- | -- | |
| 30 | Agent def schema validation passes | -- | -- | |
| 31 | Manual walkthrough: Node adopter compatibility | -- | -- | |
| 32 | Before/after diff summaries produced for each of 17 edited files | -- | -- | |
| 33 | enforcement-policy.json required_sections updated to canonical ## Test Results array | -- | -- | |
| 34 | gh-pr-guard round-trip: new format passes, legacy format fails with clear error | -- | -- | |

## Dependencies

### Blocked By

- INF-TSK-046-002 (engine must exist so agent docs can reference `codeflow test` surface)

### Blocks

- None (TSK-005 runs in parallel with TSK-004)

## Verification

### Automated

- [ ] `rg -S '\bcargo\b|\bnextest\b|\bllvm-cov\b|\bcrate\b|codeflow-core|codeflow-cli crate' .claude/agents/ .claude/commands/ project-management/templates/` returns zero results
- [ ] `markdownlint .claude/CLAUDE.md .claude/agents/*.md .claude/commands/*.md project-management/templates/*.md` clean
- [ ] `cargo test --package codeflow-core hooks::gh_pr_guard` passes
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] `codeflow test --mode full --coverage` full pipeline passes post-edit (regression check)

### Manual

- [ ] Walk through cf-quality-assurance.md imagining a Node adopter; confirm every instruction is followable
- [ ] Walk through cf-git-operations.md imagining a Go adopter; confirm PR body instructions work
- [ ] Test gh-pr-guard locally: create a branch with a multi-target test-config.json, generate a PR body, run the guard

## Stage Reports

### DOCS Report

> Populated by cf-documentation before STAGE-COMPLETE: WS-DOCS

**Documentation Summary:**
{TBD}

**Files Updated:**

| File | Action | Lines changed | Description |
|------|--------|--------------|-------------|

**Deviations from Approach:** None

### REV Report

> Populated by cf-review before STAGE-COMPLETE: WS-REV

**Review Mode:** DOCUMENTATION_REVIEW
**Verdict:** {APPROVED | CHANGES_REQUESTED}

### Confidence Score

| Stage | Agent | Score (0-100) | Rationale |
|-------|-------|--------------|-----------|
| WS-DOCS | cf-documentation | {n} | {brief rationale} |
| WS-REV | cf-review | {n} | {brief rationale} |

## Notes

Design doc: `.codeflow/docs/analysis/generic-testing-subsystem.md` §§11, 13.
ADR: `.codeflow/docs/adr/ADR-001-generic-testing-subsystem.md`.

**Important context:**

- Primary deliverable is documentation (agents, commands, templates); one code file (gh-pr-guard.rs) is a secondary deliverable. If scope expands materially, consider a sibling FEAT task.
- CLAUDE.md edits require the **staged-edit protocol** (CLAUDE.md Section 5).
- Agent def edits are **exempt** from bypassPermissions prompts — direct Edit works.
- Command def edits are **exempt** — direct Edit works.
- Template edits are not protected — direct Edit works.
- Do NOT edit cf-rust-standards, cf-python-standards, cf-shell-standards. They are intentional stack-specific on-demand references.

**Wrong approaches (do NOT):**

- Delete cf-rust-standards.
- Rename agent defs.
- Change `.codeflow/config/testing/test-config.json` path in templates — that's TSK-005.
- Touch `.state/` references in agent defs beyond test-related refs — session/pathflow/coordination refs are out of scope.
