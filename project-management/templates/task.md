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

## Outcome

<!-- One line saying what someone can do once this lands, in the form
     "after this merges, <who> can <do what>". `codeflow validate --docs`
     refuses an open record created after the project adopted this section
     when the line is missing (`outcome_present_uids` in
     `.codeflow/project.toml`), and warns for an older open record. -->

## Deliverables

<!-- Each output this task produces and its home, one line each as
     `- <output>: <path>`: files or a folder, a decision record, a research
     note, evidence, a record update or a human board, at its path in the
     project's structure. Check each home against the project's structure
     authority (the document that says where things live) where one exists.
     Where a home is not known yet, give what is known, mark it provisional
     and name what decides it. `codeflow validate --docs` warns about an open
     task with no entry here and no path in its Description. -->

<!-- Optional, only where the project adopted the estimation time-box
     profile (`cf-estimate`, `.codeflow/estimate.json`); otherwise leave it
     out. `cf-plan` writes it at breakdown, from the rubric, and it is then
     frozen with the profile revision. Copy it out of this comment as a
     `## Estimate` section here:

     ## Estimate
     - Drivers: S2 1, S3 3 = 4, Medium
     - Band: Medium (2 to 4 h), profile v1; moved: none
     - Split: not needed
     - H: none; B: none

     A move names one step and its reason (proof volume, record volume or a
     key review set). Grade tasks by judgement: most are Medium or Hard,
     an Easy task usually belongs with a sibling that shares its cause, code
     path or proof, and a Very Hard task is split before claim. Every task
     carries its own outcome and proof.
     During the
     work a Hard task adds `- Half-top: <time> <one line>`, and a task past
     its top adds `- Cause: <blocker | wrong estimate | scope | review wait
     | host>; <finish | split | re-plan>`. Class stays in frontmatter. -->

## Acceptance Criteria

<!-- The single home for this task's acceptance: testable statements,
     preferably in EARS ("When <trigger>, the system shall <response>") or
     Given/When/Then form; a subset scoped from the epic's when there is one.
     Each criterion names concrete, claim-matched evidence: automate where
     meaningful, otherwise name a bounded observable or review. Do not invent
     a hard-coded or meaningless test merely to satisfy the record. The
     reviewer checks the named evidence, and unsupported claims are defects.
     List each criterion as `- AC-n <criterion>` with no checkbox; ids are
     unique and never renumbered. This task's own PR may amend them, with a
     reason in its PR body for each one it removes. End a criterion with
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
