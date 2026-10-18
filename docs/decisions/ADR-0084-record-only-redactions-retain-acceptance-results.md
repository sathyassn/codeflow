---
id: ADR-0084
uid: a5c3b822-40ec-4870-a185-0f71228430df
title: "Record-only redactions retain acceptance results and review provenance"
date: 2026-10-16
status: proposed          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: none # none | one line naming what in architecture.md changes
---

# ADR-0084: Record-only redactions retain acceptance results and review provenance

## Context

A closed task can contain personal or customer names in its acceptance
text. Rebinding an edited block fails when a direct product commit followed
its landing. Reopening keeps the original text in a superseded block.
ADR-0080 permits criteria wording corrections for completed standalone
tasks but does not cover that combined case (issue 134).

## Decision

A record-only redaction may replace criteria wording and descriptive text
in active or superseded acceptance blocks. It preserves status, criterion
identifiers and tags, all criterion outcomes, the journey outcome, verdict,
reviewed commit and original review provenance. It records the source
commit, a SHA-256 digest of the original record and the redaction review in
a frontmatter `redaction` mapping. CI compares the source and replacement
and refuses changes outside that record. The source already belongs to the
target at the branch point. A reviewer confirms unchanged meaning.

## Consequences

- The current record no longer carries the replaced text. Git history still
  carries it; redaction never rewrites history.
- Direct product commits already on the target do not force a new acceptance
  result for a wording-only correction.
- The exception extends ADR-0080 to acceptance and superseded text, including
  epic tasks. The original criteria-correction route remains available.
- A changed result, reviewed commit or review provenance still requires a
  reopen and review. The engine proves structure, not semantic equivalence
  or the authenticity of a review reference.

## Architecture impact

none
