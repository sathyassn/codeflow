# Holistic-fix doctrine and copy guide: classification and duty map

TSK-131 carries the EPC-015 holistic-fix doctrine (D19 to D23) and the
working method of 2026-09-25 into the shipped skills, and puts TSK-073's copy
guide in the writing reference. The per-task reading chain stays under its
cap: new doctrine lands in a conditional section, and every-task text changed
only where weaker or duplicated wording made room.

## Reading chain

| Measure | Bytes |
|---|---|
| Cap (`READING_CHAIN_CAP_BYTES`, 148 KiB, unchanged) | 151,552 |
| Chain on the base `2d3d37176` | 151,486 |
| Chain on this branch | 151,476 |
| Headroom left | 76 |

The chain files that changed:

| File | Before | After |
|---|---|---|
| `cf-model-orchestrator/SKILL.md` | 21,759 | 21,664 |
| `cf-model-orchestrator/resources/quality-contract.md` | 1,864 | 2,035 |
| `cf-model-orchestrator/resources/quality/design-implementation.md` | 3,407 | 3,401 |
| `cf-develop/SKILL.md` | 5,091 | 5,011 |

`quality/findings.md` (4,820 bytes) is a conditional read, recorded in
`CONDITIONAL_READS` from each place that links it, with its trigger: the
quality index row ("when a defect is fixed, or review findings are briefed,
written or acted on"), the orchestrator's loop invariant ("When review findings
are acted on") and step 5 ("Any confirmed issue returns to its responsible
primary"), and cf-develop from step 5a ("When the change fixes a defect") and
step 5c ("On `changes_requested`"). The blocker navigation row's trigger
widened to "when a step is blocked or would depart from what was approved",
because that section now holds the D21 departure rule; its `CONDITIONAL_READS`
entry was changed to the new text.

Files outside the chain keep their own budgets: `cf-reviewer.md` 9,801 to
9,820 of 9,856; `cf-consult/SKILL.md` 6,603 to 6,644 of 6,656. The rules
references have no byte cap: `writing.md` 4,036 to 8,151 and
`workflow-discipline.md` 13,108 to 13,517.

## Duty map

Every sentence this task removed or rewrote in an existing section, and where
each of its duties is stated after the change.

| Section | Removed or rewritten text | Where each duty lives now |
|---|---|---|
| Orchestrator invariant "Bounded, evidence-moving loops" | "Plan reconciliation and post-review rework are each bounded to at most two rounds." | Plan reconciliation: the same bullet ("bounded to at most two rounds") and step 2 ("If both do not explicitly approve the same version, stop for the human"). Rework: `quality/findings.md`, Review rounds, bounded by change class. |
| Same | "At the bound, diagnose the persistent constraint and either take an approved-outcome-preserving strategic route with fresh evidence or surface a genuine external/owner block." | Rework: `quality/findings.md` last paragraph. Any tactical cycle: `quality/blockers-and-gates.md` ("When a bounded tactical cycle fails, move up a level"). All tiers: `workflow-discipline.md`, "Navigate blockers". |
| Same | "A repeated attempt without a new hypothesis or changed evidence is not another round." | Kept in the bullet as "A repeat without ..."; also in `quality/findings.md`. |
| Orchestrator step 5 | "Rework is bounded to two rounds and requires fresh evidence." | "... within the [findings](resources/quality/findings.md) round bounds." (linked directly after Fable's review of d60ce13f0). The fresh-evidence duty is the invariant's "A repeat without a new hypothesis or changed evidence is not another round". |
| Orchestrator intro | "Staged routes keep startup concise." | No duty: it described the paragraph. |
| Design and implementation quality | "Coherence includes justified structure, not merely less structure." | The same paragraph's rule that under-design is `changes_requested` and that variants and evidenced cases may require abstraction; `workflow-discipline.md` "Preserve justified structure". |
| Same | "and review follows [review and degradation](../routing/review.md)" | `routing/review.md` stays a required read from the capability-routing index. |
| Same | "The `claude-judgment-primary` owns the final quality verdict; helpers may collect evidence but cannot replace that judgment." | Orchestrator invariant "The Claude judgment primary owns integrated Claude judgment" ("owns the final quality verdict") and "workers replace no primary or named reviewer"; `routing/review.md` ("owns the final Claude quality judgment"). |
| cf-develop step 2 | "note what the change touches" | Strengthened to "name the bounded impact set (quality contract)". |
| cf-develop step 5c | "with options and a recommendation" and "genuine" at the bound | Blocker navigation ("present verified state, attempts, options with consequences, and a recommendation") and `workflow-discipline.md` at every tier ("include evidence, attempts, real options, consequences, and a recommendation"); "operator-owned decision" (CF-GOV-002 marker) stays. |
| cf-develop step 5c | "address blocker and major findings, re-review" | `quality/findings.md`, Review rounds (one batch, one apply-and-verify cycle, the finder confirms), named in the step. |
| cf-develop step 5c | "For a defect that resists a first glance, require one already-run command ... closest executable check." | Moved to step 5a Build, stated for every defect fix with the mechanism and the regression test (Fable review of d60ce13f0). |
| cf-develop step 5a | "preserve useful types, justify material unchecked/broad bypasses, validate untrusted external values at the boundary", "Do not add redundant wrappers or validators", "existing-stack" | `quality/design-implementation.md`, the rule 5a names ("Preserve useful type information; justify unchecked casts, broad escape types, suppressed checks, or equivalent bypasses at the affected boundary", "parse and validate untrusted inputs at trust boundaries", "Do not add wrapper layers, duplicate domain models, validation everywhere"). |
| Same | "Maximum 3 evidence-moving cycles." | "Maximum 2 evidence-moving cycles for code; docs and records follow that section's bound." This fixes the disagreement with the orchestrator's two-round bound. |
| Same | "Never repeat the same repair without a new hypothesis or changed evidence." | Orchestrator invariant and `quality/findings.md`. |
| Same | "At the bound ... take a safe approved-outcome-preserving route when one remains, or surface the genuine external dependency or operator-owned decision with attempts, options, consequences, and a recommendation." | Kept as "take a strategic route that keeps the approved outcome or surface the external dependency or operator-owned decision"; the attempts, options, consequences and recommendation: `quality/blockers-and-gates.md` ("present verified state, attempts, options with consequences, and a recommendation") and `workflow-discipline.md` at every tier ("include evidence, attempts, real options, consequences, and a recommendation"). |
| Same | "require one already-run command that fails on the exact reported symptom before hypothesising (quality-contract blocker navigation). Prefer a cheap local failing test when one exists; otherwise name the closest executable check." | Kept: "first run one command that fails on the exact reported symptom (a cheap local failing test, else the closest executable check)"; the full probe stays in `quality/blockers-and-gates.md`. |
| cf-reviewer step 4 | The `--docs` tier-graceful parenthetical | Kept in shorter words: "wherever `docs/` is installed (it skips an absent layer with a note); plain `codeflow validate` at minimal tier". |
| cf-reviewer step 7 | The performance checklist (complexity and N+1 access through resource cleanup) | `quality/performance.md` holds the same list, now named in the step; "require measured or stress/race evidence only when the claim or risk is material" stays. |
| cf-reviewer step 8 | "rather than reporting a pile of isolated nits" | The duty is the sentence it closed ("Investigate repeated small symptoms as a possible systemic major"), kept; also `quality/materiality.md`. |
| cf-consult preamble | "Same-vendor scrutiny is useful, but never counts ..." | "Same-vendor scrutiny never counts as independent cross-lineage review." |
| cf-consult step 1 | "Do your own analysis first, the consult sharpens it, it does not replace it." (with a dash) and "explicitly prohibit" | "Do your own analysis first; the consult sharpens it." and "prohibit"; the duty is unchanged. |
| cf-consult step 4 | "(same vocabulary as the quality contract and `cf-reviewer`)" | No duty: the disposition words are listed in the same sentence. |
| Eval requirement CF-GOV-002 | Marker "Maximum 3 evidence-moving cycles" | Marker "Maximum 2 evidence-moving cycles for code", the new cf-develop text. Statement and level unchanged. |

No duty was deleted and no rule was softened. The review-round bound for
code changed from three cycles (cf-develop) and two rounds (orchestrator) to
one bound by change class, as AC-2 requires. At d60ce13f0 that claim was
wrong for one duty: cf-develop stated the defect-fix duty only after a review
bounced the change; the D19 develop row records the fix.

## Classification of the EPC-015 pins

Source: `ENGINEERING_BAR_PINS` in `crates/codeflow-cli/tests/orchestration_contract.rs`
on `origin/integration/EPC-015-engineering-bar` at `db55fc01c`, 63 rows for D19
to D23. Present means the duty already stood on this line; carried means this
task added it, adapted where the note says; dropped rows give the reason.
Carried and present rows are pinned in `HOLISTIC_FIX_PINS`.

| Pin | Class | Home on this line and note |
|---|---|---|
| D19 mechanism | carried | `quality/findings.md` Repair |
| D19 regression | carried, adapted | Repair; states the test fails before the fix and passes after |
| D19 probe | present | `quality/blockers-and-gates.md` ("that probe is one command already run ...") |
| D19 sufficiency | carried | Repair |
| D19 reproduction | carried | Repair |
| D19 no masking | carried, adapted | Repair; drops "a repair refinement of the swallowed-error rule above", since that rule is in another section here |
| D19 probe citation | carried, adapted | Repair links the probe in blocker navigation |
| D19 develop | carried, adapted | cf-develop step 5a Build, for every defect fix: apply Repair in `findings.md`, state the evidenced mechanism, add a regression test that fails before the fix and passes after, and run the symptom probe first when needed. At d60ce13f0 this sat only in 5c under "On `changes_requested`" and omitted the regression test, a narrower trigger this row did not disclose; Fable's review found it and it is fixed |
| D19 reviewer | carried | cf-reviewer step 7 |
| D20 candidate set | carried | `quality/findings.md` Change impact; every-task short form in `quality/design-implementation.md` |
| D20 actual set | carried | Change impact |
| D20 classification | carried | Change impact |
| D20 siblings | carried | Change impact |
| D20 discovery | carried, adapted | Change impact; routes "under materiality" in place of ADR ids an adopter does not have |
| D20 stacking | carried | Change impact |
| D20 adjacent | carried, adapted | `quality/design-implementation.md`, every task; "as well as the changed path" |
| D20 selection | dropped | A heading with no duty; its two clauses are carried below |
| D20 selection body | carried, adapted | Change impact ("Run the tests of the change's dependents and consumers and of the journey the change sits in") |
| D20 selection scope | carried, adapted | Change impact ("verify each same-mechanism sibling repaired with it") |
| D20 checklist | dropped | Its host, the quality-bar checklist from D0 to D18, is not on this line; the defect-repair duty is in Repair and cf-reviewer |
| D20 checklist evidence | dropped | Same host; the evidence it listed is in Repair, Change impact and cf-reviewer step 7 |
| D20 template | dropped | Its host, the PR template's Quality report from D10, is not on this line |
| D20 agents pointer | present | The rule map's "Prove it where it runs" line ("what it touches upstream and downstream") |
| D20 develop | carried, adapted | cf-develop step 2 "name the bounded impact set (quality contract)" |
| D20 develop verify | dropped | cf-develop 5d already applies the quality contract, whose every-task impact rule states it; cf-develop is at 5,011 of 5,120 bytes and in the chain |
| D20 reviewer | carried, adapted | cf-reviewer step 7 "Require the named impact set" |
| D21 pre-apply | carried | `quality/blockers-and-gates.md` |
| D21 form | carried, adapted | Same; "departure form" |
| D21 withheld | carried, adapted | Same; "The dependent action waits for the answer ..." |
| D21 reuse | carried, adapted | Same; "reused and not asked again" |
| D21 compatibility | carried, adapted | Same; the git rules' breaking-change rule replaces D7's break rule, which is not on this line |
| D21 minimal | carried, adapted | `workflow-discipline.md`, installed at every tier; the minimal map is now rendered from the TSK-127 kernel |
| D21 develop | dropped | The quality index trigger ("would depart from what was approved") routes the moment to its one home; cf-develop has no room in the chain |
| D22 remedy | carried | `quality/findings.md` Findings and remedies |
| D22 uncertain | carried | Same |
| D22 options | carried | Same |
| D22 read-only | carried, adapted | Same; one sentence |
| D22 minor | carried | Same |
| D22 no second flag | carried | Same |
| D22 security | carried | Same |
| D22 reviewer field | carried, adapted | cf-reviewer verdict format `remedy:`; adds "or the options" |
| D22 routing | dropped | A heading with no duty; the brief now sits with the remedy rule |
| D22 routing brief | carried, adapted | Findings and remedies, moved from capability-routing to the section that owns the remedy duty |
| D22 routing return | carried | Same |
| D22 routing scope | carried, adapted | Same; drops "not a second schema", since the brief and the remedy rule are one section |
| D22 consult | carried | cf-consult step 1 |
| D23 batch | carried, adapted | `quality/findings.md` Review rounds; "the round's findings", as one round runs all reviewers |
| D23 evaluation | carried | Same |
| D23 disposition | carried | Same |
| D23 rejection | carried, adapted | Same; a disputed finding returns to the reviewer who raised it (the finder confirms) |
| D23 conflict | carried | Same |
| D23 escalation | carried, adapted | Same; "departure form under blocker navigation"; the ADR-0038 id is dropped for adopters |
| D23 one cycle | carried | Same |
| D23 re-review | carried, adapted | Same; the finder confirms on the affected scope |
| D23 failed round | carried | Same |
| D23 gates unchanged | carried, adapted | Same; names the completion gate, since D2's approval-freshness rule is not on this line |
| D23 cycle definition | carried | Same |
| D23 bound | carried, adapted | Same; bounded by change class (code two cycles, docs and records two rounds per version, reset once) |
| D23 strategic | carried | Same |
| D23 lifecycle | dropped | `workflow-lifecycle.md` is in the chain with no room; its review-concern route stands and the re-review scope is in Review rounds |
| D23 develop | carried, adapted | cf-develop 5c "Maximum 2 evidence-moving cycles for code" |
| D23 develop owner | carried, adapted | cf-develop 5c names `quality/findings.md` |
| D23 eval kit | carried, adapted | CF-GOV-002 marker "Maximum 2 evidence-moving cycles for code" |

Totals: 2 present, 53 carried (28 of them adapted), 8 dropped with a reason.
`HOLISTIC_FIX_PINS` holds the 55 present and carried rows plus three pins with
no EPC-015 row: `D19 develop probe` (the symptom probe in cf-develop Build),
`D20 every task` (the every-task impact rule) and `D21 trigger` (the widened
blocker navigation trigger).

## Working method (AC-2)

| Rule | Where it is stated |
|---|---|
| One review round runs every reviewer in parallel | `quality/findings.md`; orchestrator invariant; `workflow-discipline.md` |
| The finder confirms its own finding | `quality/findings.md`; cf-develop 5c; `workflow-discipline.md` |
| No round for a minor fix | `quality/findings.md`; `workflow-discipline.md` |
| Rounds bounded by change class | `quality/findings.md`; cf-develop 5c; orchestrator invariant |
| Orchestration entry by touched paths | orchestrator opening (already present, pinned by CF-MM-001) and the rule map |

## Copy guide (AC-3)

The ten sections AC-3 names are in `.codeflow/rules/writing.md`, each with
one example that `writing_reference_carries_the_copy_guide_with_sourced_examples`
resolves in its named source.

| Section | Example source |
|---|---|
| Voice | `cf-editorial-review/references/editorial-smells.md` |
| Sentences | `cf-editorial-review/references/editorial-smells.md` |
| Words | `rules/workflow-discipline.md` |
| Titles and headings | `docs/adoption.md` |
| Leads | `AGENTS.md.tmpl` (the rule map) |
| Captions | `docs/verification/evidence/tsk-006/prototype.html` (a full-sentence caption) |
| Summaries | ADR-0067 |
| Bullets and tables | SPC-013 R-118 |
| Microcopy | `crates/codeflow-present/web/src/chrome.tsx` |
| Replies | `docs/verification/tsk-014-w3/baselines/p3/chat.md` (a recorded chat reply) |

Reconciled with TSK-073's table: its summary rule said two to four
sentences, and this line ships "one to three short sentences" (TSK-127), so
the guide follows the shipped rule. TSK-073's legend keys, skill prose, and
ADR and PR shape sections, its pointer sweep and its evaluation cases stay
with TSK-073 on EPC-016.
