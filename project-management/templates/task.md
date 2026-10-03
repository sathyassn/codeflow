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
     Name a non-goal when omission would otherwise be ambiguous.
     Review-relevant scope, interfaces and recovery notes go here: the
     public contracts or boundaries this task changes, and how to recover
     from an irreversible step, when a reviewer needs them. -->

## Deliverables

<!-- Each output this task produces and its home, one line each as
     `- <output>: <path>`: files or a folder, a decision record, a research
     note, evidence, a record update or a human board, at its path in the
     project's structure. Check each home against the project's structure
     authority (the document that says where things live) where one exists.
     Where a home is not known yet, give what is known, mark it provisional
     and name what decides it. `codeflow validate --docs` warns about an open
     task with no entry here and no path in its Description. -->

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

## Closeout

<!-- Fill before status becomes complete/cancelled.
     `codeflow task status` writes the status and what the transition needs:
     a `## Blocker` (reason, owner, revisit) for blocked, the lines
     `- cancelled: <reason>` and `- scope: <where it went>` for cancelled, and
     for complete the fenced `yaml` acceptance block (reviewed commit, review,
     one result per criterion, journey, not_verified, follow_ups, verdict). -->
