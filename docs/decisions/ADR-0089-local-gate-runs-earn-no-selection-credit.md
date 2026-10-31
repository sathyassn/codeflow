---
id: ADR-0089
uid: 20e233dc-ef8f-403f-b8e4-d046ce597ab6
title: "Local gate runs earn no selection credit"
date: 2026-10-30
status: proposed          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: Change-aware selection no longer skips a target on a local run record
---

# ADR-0089: Local gate runs earn no selection credit

## Context

ADR-0085 let a quick, essential or `--since` run skip a content check when
a passed, clean, complete full record of the base tree proved it. Those
records live under `~/.codeflow/gate-runs`, which every process of the
user can write: the operator's shell, an agent session through the shipped
Claude presets' sandbox grant, and the candidate's own tests, which run
inside the gate as the same user. Ownership and mode checks cannot tell
them apart, and a key any of them can read authenticates nothing (the
cross-lineage security review of TSK-337, finding 2; TSK-1044).

## Decision

No local run record earns selection credit, on any platform. Every run
executes every eligible target unless `--only` limits it; `--since` is
accepted and narrows nothing, and the task line is no longer resolved for
selection. Run records stay as reports: the gate still writes them, a full
run still copies them to `~/.codeflow/gate-runs/<repo>/<run-id>/`, and pull
requests still cite their run ids. A record written before this change is
read by nothing. Credit can return only from a recorder whose identity no
agent session and no candidate test can use, such as an attestation made
by hosted CI; that is a new decision, not a setting. This supersedes the
selection-credit part of ADR-0085; its evidence and redaction rules for
the records themselves stand.

## Consequences

- A forged, replayed or pre-upgrade record has no effect on what runs.
  The credit code and its checks (account home, record owner and mode)
  are removed rather than kept behind a switch.
- Adopters that declared `input_class: content` targets lose local skips:
  their quick, essential and `--since` runs execute those targets again,
  which costs time. The declarations stay valid and are still recorded.
- A full run with `--since` is now complete evidence, since nothing is
  skipped. `--strict` no longer fails on candidate-evidence skips.
- Windows already granted no credit; it is unchanged.
- TSK-353 (repeat-run skips), TSK-355 (result reuse) and TSK-1039
  (`--since` narrowing) need a sound recorder before they can skip.

## Architecture impact

`docs/architecture.md`, the change-aware selection and result artifact
rows: every run executes every eligible target, local records are reports
only, and the durable copy refuses links on both sides.
