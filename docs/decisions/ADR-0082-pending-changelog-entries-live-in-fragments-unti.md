---
id: ADR-0082
uid: c118b68a-f86e-4b57-9786-890035b30799
title: "Pending changelog entries live in fragments until a reviewed assemble"
date: 2026-10-09
status: accepted          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: none # none | one line naming what in architecture.md changes
---

# ADR-0082: Pending changelog entries live in fragments until a reviewed assemble

## Context

ADR-0062 keeps release state in each work pull request, and every such
pull request wrote its entry into the one undated pending section of
`CHANGELOG.md`. With several pull requests open and strict required checks
on `main`, each landing forces every other open pull request to merge the
new `main`, and their entries meet in the same lines of that section. The
merge needs a hand resolution, so the acceptance binding sees a changed
merge instead of a clean re-merge, and every other pull request pays a
fresh review (issue 119).

## Decision

Pending entries live in one carrier: fragments under `changelog.d/`, one
file per pull request named `<name>.md` (by convention the task id), each
holding labelled entries with their impact markers under the kind headings
`### Added`, `### Changed`, `### Deprecated`, `### Removed`, `### Fixed` and
`### Security`; or, after a reviewed `scripts/release.py assemble`, one
written pending section. `release.py` reads both through one reader that
composes the fragments into the section every check judges: kinds in that
fixed order, then fragments by file name in byte order, then entries in
file order, under the published baseline bumped once by the highest impact.
A tree that holds fragments and a written pending section fails. `sync`
stamps the composed target and never writes the section; `assemble` is the
only writer, its pull request declares `Impact: none`, and `release-notes`
refuses a source that still holds a fragment. This amends ADR-0062's
carrier, not its rule that release state travels with the work.

## Consequences

- Two pull requests that each add a fragment touch different paths, so the
  main merge each owes after a landing is a clean re-merge and its review
  carries. A pull request that edits a carried entry edits that entry's
  fragment, and two such edits of one entry still conflict.
- Identity, comparison, withdrawal and typed repair keep their rules: an
  entry is its label, entries compare byte for byte, a removed fragment is
  a withdrawal of its labels, and a repair may remove a whole fragment but
  never edit one. Moving an entry between carriers with its bytes unchanged
  is not an edit.
- Publication gains one reviewed step. A pull request that adds an entry
  after `assemble` lands and before the dispatch runs `assemble` itself, and
  two such pull requests meet in the section again; assemble last.
- Two open pull requests with one label are each green until one lands, as
  with the shared section; the refreshed check refuses the second. Two that
  pick one fragment name conflict in git; the task-id name makes that rare.
- A range that reopened its own task still reviews its own work: no layout
  changes the reopen rule.
- The release-policy guidance CodeFlow ships to other projects is not
  changed: it does not install this repository's calculator, and a project
  whose release tool never reads `changelog.d/` must not be told to write
  there.

## Architecture impact

None. `docs/releasing.md` carries the runbook.
