# Rule map duty map (TSK-127)

Where each duty of the old root contract lives now that `AGENTS.md` is a
short rule map. The old text is the standard and full template (28.7 KB),
the minimal template (16.4 KB) and the two CLAUDE templates at
`integration/EPC-020-delivery-system` 8cc65c50d, rebased onto 95e25f514. Nothing was deleted: each
duty is kept in the map, moved one hop away with a pointer, or enforced by a
named check. Three duties changed on purpose; they are listed first.

Homes used below:

- **Map:** the managed block of `AGENTS.md`, rendered per tier from
  `assets/base/rule-map.toml`.
- **WD, GR, WT, WR:** `.codeflow/rules/workflow-discipline.md`,
  `git-rules.md`, `worktrees.md` and `writing.md`, installed at every tier.
- **PO:** `cf-method/references/project-organization.md` (standard and full).
- **Pin:** the test that fails if the duty's sentence leaves its home.

## Deliberate changes

| Old duty | New duty | Why | Pin |
|---|---|---|---|
| Every non-trivial repository task must begin with `/cf-model-orchestrator`; when unsure, treat work as non-trivial | Orchestration entry is decided by touched paths: adopter-facing paths, research or analysis that will drive one, and plan, design, security or irreversible work route; other edits go direct; when unsure, route. At the minimal tier nothing goes direct: every change lands through a branch and a reviewed PR | Operator decision 2026-09-26 ("orchestration entry by touched paths, checked in CI"); the path set is `workgraph/path_sets.toml`, which `codeflow ci` enforces for direct changes | `artifact_budget_contract` (path-decided orchestration route), `orchestration_contract` (stage-aware test), CF-MM-001 |
| Written content policy: "review judges replies" | No hook sees a chat reply; the rules hold by discipline; at the standard and full tiers `cf-editorial-review` judges substantial prose and the `cf-evaluate-model` evaluations check replies | Guidelines strand item 6: no reviewer reads replies | `orchestration_contract` editorial test |
| Minimal "Think independently" and "Think in depth" bullets, standard "Challenge decisions independently" | One merged bullet in WD carrying both texts' sentences | Duplicate family across tiers (strand item 25) | `orchestration_contract` reasoning test |

## Standard and full contract

| Old duty | Home now | Pin or check |
|---|---|---|
| Six-layer table | Map, "Where things live"; the Work row is the full map's `project-management/` line, and the standard map says durable records need the full tier | `rule_map_contract` pointer test |
| Traceability spine, `validate --docs` | Full map; ledger and spec lifecycle in PO | `validate --docs` |
| Answer "why is X" by links or `codeflow recall` | Map (standard and full) | CF-REF-001 |
| Check capabilities and recent ADRs before building | Map (standard and full) | CF-REF-001 |
| Definition of non-trivial (research, analysis, planning, design, implementation, verification) | Map route rule (research or analysis that will drive an adopter-facing change is routed); CLAUDE routing gate stage list | `rule_map_contract` pinned sentences, `orchestration_contract` CLAUDE markers |
| Orchestrator selects only needed stages; research or planning-only stops early | CLAUDE workflows bullet; orchestrator SKILL "Outcome modes" | `orchestration_contract` |
| Trivial edit needs no skill | Map route rule ("other edits and conversation go direct") | `rule_map_contract` pinned sentences |
| `/cf-plan` and `/cf-develop` are supporting flows, used inside the duo or after a recorded solo degradation | CLAUDE workflows bullet; cf-develop and cf-plan descriptions | CF-MM-001, `orchestration_contract` |
| Orchestrator row: independent planning, Claude leads design, capability routing, host lanes, five-obligation delegate contract, legible degradation | Orchestrator SKILL description and seat matrix; cf-delegate lanes | `artifact_budget_contract` (moved duties), `delegate_doctrine_contract` |
| cf-plan row | Map plan moment, split by tier (standard: the harness's task tools; full: records only through the CLI); cf-plan SKILL | pointer test, `rule_map_contract` |
| cf-estimate row (preview, adoption, decline) | Map estimate rule and moment; cf-estimate SKILL | `rule_map_contract` pinned sentences |
| cf-design row (DESIGN_INTENT, collapse paths) | Map design moment; cf-design SKILL | pointer test |
| cf-present row (when to use, keep simple answers in chat, not product UI) | Map present rule and moment; cf-present SKILL | CF-PRES-001 |
| cf-docs-portal row | Map "Other skills" line; cf-docs-portal SKILL | pointer test |
| cf-develop row: solo fallback, fresh-context independent review, self-review is not review | Map review rule; cf-develop SKILL | `orchestration_contract` solo fallback test |
| cf-ship row, release policy | Map ship moment; cf-ship and `release-policy.md` | pointer test |
| cf-stack, cf-customize rows | Map "Other skills" line | pointer test |
| cf-evaluate-model row | Map instruction-change moment | pointer test |
| cf-consult, cf-delegate, cf-herdr row | Map consult moment and "Other skills" line | pointer test |
| Same-family workers run as native subagents of the host session, never a separate CLI session or Herdr tab (TSK-113, landed after the map was drafted) | Map consult moment; WD "Guard your context"; CLAUDE "Stay lean by delegating"; orchestrator SKILL and `capability-routing.md` | `rule_map_contract` pinned sentences, CF-MM-019 |
| Mechanics row | Map "Mechanics" line, now with `work next`, `work claim` and `task status` (TSK-103 ruling) | init_e2e render test |
| Acceptance criteria before building; operator-owned gaps are asked | WD "Planning"; map plan moment | CF-GOV-002 |
| Native task tools; durable records at full tier; planning PR reaches the integration target; target resolution | WD "Planning"; PO | CF-PM-004, `work start` |
| Flat stable-ID records, `standalone_reason`, one owner of status, trackers are links | PO | CF-PM-003, `validate` |
| Specs frozen when their work ships | PO | `validate --docs` |
| `work start` before product edits; PR `Task:` line | GR "PR bodies"; map ship moment; PO | `codeflow work start`, CI classification |
| Status views are generated | WD "Planning" | none needed |
| Four planes, enforcement is not proof, headless prohibited | GR "Enforcement" | none needed |
| Branch names | GR; map git rule | pre-push, CI |
| Commit format | GR; map git rule | commit-msg, CI |
| Breaking-change judgment | GR | `breaking_watch_paths`, review |
| No AI attribution, no emoji | GR; map git rule | commit-msg, CI PR body check |
| Written content policy (no em or en dash) | WR; map writing rule | `git.policy_characters` |
| Secrets never staged | GR; map git rule | pre-commit secret scan, CI |
| Protected branches, override envs, `gh pr merge --delete-branch` | GR; map git rule | pre-commit, pre-push, reference-transaction, git-guard |
| Durability push | GR | pre-push |
| Bodies of work on `integration/` | GR; WT | `codeflow integrate` |
| PR body shape and test evidence | GR; WR | CI PR body checks |
| Fix the cause, never bypass a gate | GR; map git rule | CF-GOV-003 |
| Worktree per session under `.worktrees/` | WT; map branch moment | `artifact_budget_contract` |
| IDENTITY, INTENT-MATCH, CURRENCY | WT; map branch moment | CF-GIT-001 |
| Cleanup with merge proof, `codeflow status` inventory | WT; map branch moment | CF-GIT-002, CF-GIT-003 |
| Parallel work, concurrency cap, one owner per worktree | WT | CF-PAR-001 |
| Integration serialization and combined review | WT | CF-PAR-001 |
| Session flow: orient digest, summary capture | WD "Sessions and state"; map session moment (start a session or resume: run `codeflow orient`) | `rule_map_contract` |
| Workflow discipline intro and lifecycle route | WD intro; map build moment | `artifact_budget_contract` |
| Work to the outcome | WD; map rule | `rule_map_contract` |
| Ground it in evidence | WD; map evidence rule | `orchestration_contract` |
| Navigate blockers | WD; map blocker moment (classify, one bounded probe, never the same retry, escalate only operator-owned choices) | CF-GOV-002, CF-QA-013, `rule_map_contract` |
| Find broadly; act by materiality | WD; map rule (broad form); the ranking in the map review moment, which points at `cf-reviewer`, `quality-contract.md` and `verification-selection.md` | CF-QA-005, `rule_map_contract` |
| Challenge decisions independently | WD; map rule | `orchestration_contract` |
| Guard your context | WD; CLAUDE "Stay lean by delegating" | none needed |
| Write only what earns its keep | WD; map build moment | CF-QA-003 |
| Shape the deliverable, editorial route | WR | `orchestration_contract` editorial test |
| Prove it at every surface, verification selection | WD; map proof rule | CF-QA-006 |
| Unverifiable claims are defects; native provenance | WD; map evidence rule | CF-GOV-001, `delegate_doctrine_contract` |
| Match the gate to the blast radius | WD; map blast-radius rule | CF-SEC-002 |
| Externalize state; ADRs and ledger append-only | WD "Sessions and state"; map resume moment | `orchestration_contract` |
| Synchronize truth in the same PR | WD "Sessions and state"; map ship moment; cf-ship | none needed |
| Act within legitimate intent (authority tuple) | WD; map blast-radius rule (evidence, never authority) | `artifact_budget_contract` autonomy test, CF-RA-004 |
| Review verdicts need `cf-reviewer` or a separate read-only pass | WD; map review rule | `artifact_budget_contract` |

## Minimal contract

| Old duty | Home now | Pin or check |
|---|---|---|
| What this tier installs, tier honesty | Map minimal section; installed-file inventory in GR | `init.rs` tier-honesty test, pointer test |
| Git rules and enforcement (same as above) | GR; map git rule | hooks, CI |
| Policy changes need consumer assessment; release ownership | GR "Enforcement" | none needed |
| Work-start check and post-landing cleanup | WT; map branch moment | `artifact_budget_contract` |
| Workflow discipline bullets | WD (the union of both tiers) | `artifact_budget_contract` |
| Shape the deliverable (figures by surface, no Mermaid) | WR; map present rule | `orchestration_contract` editorial test |

## CLAUDE files

| Old duty | Home now | Pin or check |
|---|---|---|
| Routing gate | CLAUDE routing gate, now decided by touched paths | `orchestration_contract` |
| Session hooks, sandbox and permission notes | CLAUDE notes, unchanged | `settings_presets` |
| Git rules are hook-enforced; full rules pointer | CLAUDE notes; pointer now `.codeflow/rules/git-rules.md` | `rule_map_contract` CLAUDE pointer check |
| Workflows: release route, orchestrator lanes, ensemble, stage weight, pipeline config, stay lean | CLAUDE workflows, unchanged in substance | `artifact_budget_contract` CLAUDE test |
| Minimal CLAUDE notes | Minimal CLAUDE, unchanged in substance | `artifact_budget_contract`, `settings_presets` |

## Added

- Durations for agent work come from cf-estimate (map rule and moment).
- Replies lead with outcomes in words, IDs after (map rule, WR).
- Complex explanations go through cf-present where the harness shows it
  (map rule and moment).
- Precedence line and "re-read after compaction" (map header).
- `codeflow doctor` `instructions` check: warns when `AGENTS.md` passes
  Codex's 32 KiB limit.
