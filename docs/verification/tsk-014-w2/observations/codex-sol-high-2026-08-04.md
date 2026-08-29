# Blind rendered observation — Codex GPT-5.6 Sol/high

## Qualification

- Harness: native interactive Codex CLI in dedicated tmux session
- Model shown by the harness: `gpt-5.6-sol`, high effort
- Working directory: this study root
- Input boundary: the exact baseline and candidate PNGs plus the questions and
  G1–G8 rubric only
- Explicitly excluded: `answer-key.md`, content/source files, HTML, repository
  records, checks and prior reviews
- Writes and delegation: forbidden
- Motion: not observable from still images, so no motion claim is made

The observer fixed each G1 first-glance statement after its first bounded view
of the light desktop candidate, then inspected the baseline and responsive/mode
variants. The coordinating session compared the reported answers with the
pre-registered answer key only after the observer had returned.

## Results

### P1 — EPC-005 task paths

| Candidate | Exact first-glance statement | Material result |
|---|---|---|
| P1-A wave lanes | “EPC-005’s unfinished work is organized into readiness waves showing what can start today, the longest chain to TSK-010, and what can wait.” | G1–G5, G7 and still-image G8 pass. It recovered TSK-013/TSK-014 as startable, TSK-014 → TSK-011 → TSK-007 → TSK-010 as the longest chain, and TSK-013 as waitable. |
| P1-B blocking matrix | “EPC-005’s dependency matrix shows who blocks whom, which tasks are free to start, and which tasks set the finish.” | G1, G2, G4, G5 and G7 pass. G3 fails because the baseline answers faster. G8 fails because mobile clips most blocker columns and requires horizontal scrolling. |
| P1-C critical ribbon | “EPC-005 has a four-task critical chain that sets the finish, while two other tasks have scheduling room.” | G1–G5, G7 and still-image G8 pass; the complete answer was immediate and the mobile sequence preserved chain and room. |

G6 passes: the wave graph, blocker matrix and critical/slack ribbon cannot
exchange content without rebuilding their encodings. Observer recommendation:
**P1-C**.

### P2 — exact-candidate evidence

| Candidate | Exact first-glance statement | Material result |
|---|---|---|
| P2-A evidence grid | “TSK-007 proves the exact candidate only on macOS arm64, with most other platform evidence missing and the Claude judgment blocked.” | G1–G5 and G7 pass. It correctly found macOS arm64 as the only exact-candidate journey and `oauth_org_not_allowed` as an authorization failure. G8 fails because native Windows, WSL2 and independent-review cells are off-screen on mobile. |
| P2-B provenance rail | “TSK-007’s claims travel through provenance rails, and four of six stop before acceptance at different evidence stages.” | G1–G5, G7 and still-image G8 pass; the mobile form preserves stages and reasons. |

G6 passes: a claim-by-lane grid and staged provenance rails require different
information models. Observer recommendation: **P2-B**.

### P3 — review convergence

| Candidate | Exact first-glance statement | Material result |
|---|---|---|
| P3-A finding-anchored delta | “The qualification harness is broken down by code region to show how many rounds each finding took.” | G1–G5 and G7 pass. It correctly found F1/F5/F6 as multi-round and F6 as externally reopened. G8 fails because dense code and annotations are not comfortably readable on mobile. |
| P3-B convergence ledger | “Seven findings are tracked across eight rounds to show which later rounds reopened earlier fixes and which cause came from outside the harness.” | G1–G5, G7 and still-image G8 pass; it recovered F1/F5/F6 and the off-axis F6 cause after scanning. |

G6 passes: code-region deltas and a finding-by-round ledger use different primary
units. Observer recommendation: **P3-B**.

### D1 — preconditions for starting a task

| Candidate | Exact first-glance statement | Material result |
|---|---|---|
| D1-A concept dependency path | “Before starting a task, eight repository concepts must be understood in dependency order, ending at the gate that enforces nine prerequisites.” | G1, G2, G4, G5, G7 and still-image G8 pass. G3 fails because the baseline's numbered preconditions and gate table answer the exact question faster. |
| D1-B task-first entry | “A task moves from worktree creation through work-start, editing, testing, push, and human merge, with refusal gates branching off at each station.” | G1, G2, G4, G7 and still-image G8 pass. G3 fails because the full lifecycle exceeds the question; G5 fails because a sizeable explanatory prose tail follows the diagram. |
| D1-C contract map | “One policy document is enforced across five planes, but only CodeFlow CI plus human merge forms the repository’s real boundary.” | G1, G4, G5, G7 and still-image G8 pass. G2 fails because the candidate does not expose the content of all nine prerequisites; G3 therefore fails too. |

G6 passes: the concept path, lifecycle and rule-by-plane map are distinct
encodings. Observer recommendation: **none; retain or improve the baseline for
the exact entry question**.

### D2 — SPC-004 authority and evidence

| Candidate | Exact first-glance statement | Material result |
|---|---|---|
| D2-A chain in place | “SPC-004’s claims are mapped to the decisions that govern them and to the evidence supporting each claim.” | G1, G2, G4, G5, G7 and still-image G8 pass. It correctly found ADR-0052 as partial authority and CLM-5 as unbacked. G3 fails because the baseline states both more directly. |
| D2-B evidence-adjacent margin | “SPC-004 places each claim beside its governing decision and its supporting artifact, exposing one central claim with no evidence.” | G1–G5, G7 and still-image G8 pass; authority, evidence and absence remain adjacent on mobile. |

G6 fails: both siblings use the same claim/decision/evidence content units and
mainly change bracketed reach into adjacent margins. Observer recommendation:
**D2-B**, while recording that the sibling set did not meet the distinctness
gate.

## Boundaries of this record

This is a blind observation, not an operator decision and not a production
approval. G2 records whether the answer was immediate or required scanning; no
wall-clock stopwatch result is claimed. G8 covers responsive and light/dark
still-image behavior only. Claude design judgment and operator direction remain
separate required gates.
