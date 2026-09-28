---
id: ADR-0071
uid: ad18a9a6-035c-4a42-b59a-f849c1a4418b
title: "Outcome-first working and reporting"
status: accepted
date: 2026-09-25
supersedes: []
superseded_by: []
architecture_impact: none
---

# ADR-0071: Outcome-first working and reporting

## Context

CodeFlow already asks agents to know the intent of a task and what done
means, to keep the dependency that controls the outcome in view, and to
report with evidence. Two independent design reviews on 2026-09-25 (Fable
and Astra) found that this does not carry through. A worker sees a scoped
task, its criteria and a test list; the reason the task matters is easier to
lose than the mechanics. Green gates and met criteria get reported as done
while the result they were meant to show is still missing, and reports open
with plan versions, check states and counts before the reader learns what
they can now do.

The operator asked on 2026-09-25 that agents keep the real result, and who
it is for, in view while they work and when they report, and that the
operator can see at a glance what needs them. The same day the operator
clarified the summary rule: a summary anchors the reader, as judgment, not a
sentence count or a list of banned items.

## Decision

Eight rules, each amending a passage that already exists. No section, skill,
policy key or machine-readable format is added.

1. **Outcome while working.** Before acting, the agent names the result the
   work exists to produce, who uses it in their terms, and the evidence that
   would establish it. It keeps the larger result in view only where that
   changes scope, priority or completion, and uses the result at each
   choice: what next, what to skip, when a step is done, when to stop. A gate
   or criterion is evidence toward the result, never the result. What is done
   here stays separate from what depends on other work. The agent rechecks
   when evidence or the plan changes and stays within scope and authority.
   Owner: "Work to the outcome" in both contract templates.
2. **Criteria met, result missed.** A task whose criteria pass while its
   named result is not reached is a finding that returns to planning, not a
   pass. Owners: the completion gate in `quality-contract.md` and step 7 of
   `cf-reviewer`, whose verdict format is unchanged.
3. **Frame by real parts.** A non-trivial subject is framed by its own parts
   as its consumer meets them (for a platform: surfaces, services,
   contracts, data, infrastructure, deployment and consumers). Files and
   steps are the means. This is judgment, not a checklist. Owner: "Establish
   the route" in `workflow-lifecycle.md`.
4. **Report order.** A reply or report opens with the result it serves and
   where the work stands, then what would change that and who resolves it,
   then what the reader must decide or do; steps, gates, counts and tooling
   come last. This is an order, not headings. A one-line answer stays one
   line; labels belong only in status, readiness or closeout reports the same
   reader compares, and labels forced onto a short answer are a defect.
   Owners: the lifecycle reply rule, with a compact form in both templates.
5. **Report owners.** The orchestrator's joint closeout, `cf-develop` step 6,
   the readiness report and Summary in `pr-evidence.md`, the pull request
   template's Summary comment and the running report in `autonomy.md` each
   gain one sentence that opens with the result. Their existing evidence
   lists stay as the detail.
6. **One attention marker.** In a reply to the operator, the items the
   operator must act on go under a single heading, NEED YOUR ATTENTION, at
   most once per reply, after the opening and before the detail. Each item
   starts with what is needed (Decide, Do, Confirm, Clarify or Note) and
   stands on its own: the subject, the options and a recommendation. With
   nothing owed there is no heading. It never appears in a pull request
   body, document, commit message, outbound draft or machine payload. The
   items are the ladder's ask and notify rungs, including a hard gate that
   waits on the operator. A consuming project may rename or drop the heading
   in its own instructions. Owners: the lifecycle reply rule and
   `autonomy.md`, with a compact form in both templates.
7. **Summaries anchor the reader.** A summary gives just enough context for
   the reader to get their bearings: what this is, why it matters, where it
   stands, in a few lines. A key number, file name, data point or caveat
   belongs there when it is part of that context; detail that does not help
   the reader orient comes after. This replaces the "two to four sentences"
   count of ADR-0067 and the context-only rule of its 2026-09-24 note on
   `main`; see the ADR-0067 note of 2026-09-25.
8. **Dashes are a prose guideline.** Em and en dashes are avoided in prose;
   a dash stays only where it is really needed, such as a quoted title or a
   numeric range in data. Review and evaluation judge it; the mechanical
   check and its default level are a separate decision. Owners: both
   contract templates, the lifecycle reply rule and `editorial-smells.md`.

Evaluation: requirement CF-OUT-007 and an amended CF-OUT-002 carry cases
with paired controls graded on substance, never on the presence of a label.

## Consequences

- An agent that meets its criteria but misses the result reports a finding
  instead of done, and a reviewer has a named rule to cite.
- Reports lead with what the reader can now do; the evidence that proves it
  is still required and still follows.
- The operator finds every decision owed in one place per reply. A project
  that prefers another label renames it in its own instructions.
- The always-loaded contracts grow. The standard template and the dogfood
  contract move their reviewed ratchets within the charter caps (the
  standard template 28 KiB to 29 KiB, the dogfood root 31 KiB to
  31 KiB + 640 B, still below the 32 KiB harness limit); the minimal contract
  stays at its 16 KiB cap by condensing text that repeated its own bullets.
- On `main`, the 2026-09-24 summary wording and the pull request Summary
  warnings for a code span or a path conflict with rule 7. TSK-086
  reconciles them when it merges `main` forward.

## Alternatives considered

### Four fixed headings in every reply

Rejected. Outcome, position, gap and ask are a useful order but forced
headings pad a short answer and reward labels over substance. The rule is an
order; labels stay optional, and a faulty control fails a polished labelled
report that claims done from green tests alone.

### Four inline markers

Rejected. Separate decision, clarification, action and attention markers
scatter what the operator owes across a reply. One heading, once, with a
verb on each item, is easier to scan and keeps the category.

### A new outcome section or skill

Rejected. The duties already have owners. A second place would drift from
the first, which is the problem ADR-0070 solved for the operator-owned list.

## Architecture impact

None. Managed instruction text and evaluation data change; no runtime
behavior changes.

## Carried to the 3.0.0 source

TSK-108 carried this decision to the 3.0.0 source on 2026-09-28 with the
SPC-013 R-117 adaptation: the running report and the attention heading that
rules 5 and 6 give to `autonomy.md` live in the lifecycle reply rule and the
orchestrator's joint closeout, and rule 7 is reconciled here by the
ADR-0067 note of 2026-09-25 and the removal of the pull request Summary
warnings for a code span, a path or more than three sentences.

The requirement is CF-OUT-007 here, where the source branch named it
CF-OUT-006: TSK-130 had already landed a different CF-OUT-006, on naming
each item by its outcome, and evaluation surfaces are append-only (R-118).
