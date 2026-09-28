---
id: TSK-{{NNN}}
uid: {{UID}}              # hidden record identity, written once by `new`; never edit
epic_id: {{EPIC_ID}}              # EPC-NNN, or null for a justified standalone task
standalone_reason: {{STANDALONE_REASON}} # required exactly when epic_id is null
title: {{TITLE_YAML}}
status: todo             # todo | blocked | complete | cancelled; change it with `codeflow task status`
work_type: feat          # feat | fix | docs | refactor | test | chore | ci | hotfix | plan | spike | experiment
specs: []                # task-specific SPC-### inputs; epic specs are inherited
depends_on: []           # every direct structural predecessor; [] only for a true root; a research or decision input is {id: TSK-NNN, kind: research, pin: "<commit sha>"}, the pin quoted
integration_target: {{TARGET_BRANCH}} # main/master or integration/EPC-NNN-<slug>
external_refs: []        # opaque links/ids only; never mirror external status
created: {{DATE}}
---

# TSK-{{NNN}}: {{TITLE}}

## Description

<!-- One paragraph: the outcome/change, why it is needed, and its bounded scope.
     Name a non-goal when omission would otherwise be ambiguous. -->

## Affected surfaces and interfaces

<!-- Paths/areas, capabilities, public contracts, and material native build,
     runtime, release, data/trust, or integration boundaries this task may
     change. Name the actual owner; a folder alone does not enforce a boundary.
     Delete when genuinely obvious. -->

## Acceptance Criteria

<!-- The single home for this task's acceptance: testable statements,
     preferably in EARS ("When <trigger>, the system shall <response>") or
     Given/When/Then form; a subset scoped from the epic's when there is one.
     Each criterion names concrete, claim-matched evidence: automate where
     meaningful, otherwise name a bounded observable or review. Do not invent
     a hard-coded or meaningless test merely to satisfy the record. The
     reviewer checks the named evidence, and unsupported claims are defects.
     List each criterion as `- AC-n <criterion>` with no checkbox; ids are
     unique and never renumbered. End a criterion with
     `(serves EPC-NNN AC-m)` when it serves an epic criterion, or with
     `(journey)` when it names the path exercised. -->

- AC-1

## Execution contract

<!-- For non-trivial orchestrated work: approved Plan vN, responsible primary,
     actual executor (if different), independent cross-lineage reviewer,
     integration target, and task-specific risk/recovery requirement. The
     primary remains accountable for acceptance when execution is delegated.
     Shared engineering/security/testing doctrine stays in AGENTS.md, its
     .codeflow/rules/ references and the skills; do not paste it here.
     Delete this section for a trivial direct task. -->

## Closeout

<!-- Fill before status becomes complete/cancelled; delete inapplicable bullets.
     `codeflow task status` writes the status and what the transition needs:
     a `## Blocker` (reason, owner, revisit) for blocked, the lines
     `- cancelled: <reason>` and `- scope: <where it went>` for cancelled, and
     for complete the fenced `yaml` acceptance block (reviewed commit, review,
     one result per criterion, journey, not_verified, follow_ups, verdict).
     - Delivered outcome.
     - Plan conformance: as approved under Plan vN, or the later dual-approved
       Plan vN+1 that authorized a material change.
     - Bounded discoveries/deviations: only review-relevant facts that stayed
       inside the approved node's outcome, scope, interfaces, ownership, and
       safety boundary, with a compact evidence pointer.
     - Verification evidence and anything not verified.
     - Recovery evidence, only for an irreversible/high-blast-radius surface.
     - Follow-ups, each routed to its single real tracked home.

     A material node, edge, guard, owner, acceptance/interface, scope, or safety
     change is never legalized here after implementation: stop, settle Plan
     vN+1 with both primary seats, then continue. -->
