---
id: TSK-{{NNN}}
epic_id: {{EPIC_ID}}              # EPC-NNN, or null for a justified standalone task
standalone_reason: {{STANDALONE_REASON}} # required exactly when epic_id is null
title: {{TITLE_YAML}}
status: todo             # todo | blocked | in_progress | complete | cancelled
work_type: feat          # feat | fix | docs | refactor | test | chore | ci | hotfix | plan | spike | experiment
specs: []                # task-specific SPC-### inputs; epic specs are inherited
depends_on: []           # structural predecessors; Plan guards own branch readiness
integration_target: {{TARGET_BRANCH}} # main/master or integration/EPC-NNN-<slug>
external_refs: []        # opaque links/ids only; never mirror external status
created: {{DATE}}
---

# TSK-{{NNN}} — {{TITLE}}

## Description

<!-- One paragraph: the outcome/change, why it is needed, and its bounded scope.
     Name a non-goal when omission would otherwise be ambiguous. -->

## Affected surfaces and interfaces

<!-- Paths/areas, capabilities, public contracts, data or state boundaries, and
     integration points this task may change. Delete when genuinely obvious. -->

## Acceptance Criteria

<!-- The single home for this task's acceptance — testable statements,
     preferably in EARS ("When <trigger>, the system shall <response>") or
     Given/When/Then form; a subset scoped from the epic's when there is one.
     Each criterion names concrete, claim-matched evidence: automate where
     meaningful, otherwise name a bounded observable or review. Do not invent
     a hard-coded or meaningless test merely to satisfy the record. The
     reviewer checks the named evidence, and unsupported claims are defects. -->

- [ ]

## Execution contract

<!-- For non-trivial orchestrated work: approved Plan vN, producer, independent
     cross-lineage reviewer, integration target, and task-specific
     risk/recovery requirement. Shared engineering/security/testing doctrine
     stays in AGENTS.md and the skills; do not paste a generic checklist here.
     Delete this section for a trivial direct task. -->

## Closeout

<!-- Fill before status becomes complete/cancelled; delete inapplicable bullets.
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
