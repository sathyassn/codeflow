---
id: ADR-0085
uid: 697c88bd-f38b-4c99-a574-4c80feede079
title: "Gate selection requires evidence for the exact check"
date: 2026-10-19
status: proposed          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: Test gate selection binds each skipped check to candidate evidence
---

# ADR-0085: Gate selection requires evidence for the exact check

## Context

A landed merge can have the same tree as the candidate whose full gate passed,
while its commit ID differs. That equality alone cannot establish that an
unchanged check still has the same tools, environment, membership or producer
outputs. Quick and full commands can also differ for one target.

## Decision

The default comparison is the merge base of HEAD and the current task's declared
integration target. Only a content target with complete matching dependency
evidence can be skipped. A qualifying record is a passed, clean, complete full
run with the base's Git tree ID and the current config digest. Its credited
execution must have the same command and mode, or an explicitly declared mode
relation whose commands are identical. Evidence records inputs and dependencies
before and after each target. Sensitive and fingerprint environment inputs use
keyed fingerprints under a private key outside every repository; their values
are redacted from gate-owned artifacts.
Unknown evidence selects the target and names the reason.

Skip credit is a local convenience, never landing evidence. Run records are
mutable by their owner; ownership checks do not authenticate a successful run.
Selection refuses records not owned by the effective user or writable by a
group or everyone, and grants no credit when `CODEFLOW_HOME` is overridden.
Where ownership protection is unavailable, records receive no selection credit.
A record MAC under a sandbox `denyRead` key was considered and is deferred in
this repair. It would require a separate protected writer and key boundary.
The backstop is a non-selective landing gate: default full runs execute every
target, and both integration modes request all targets and refuse incomplete
runs, including runs with no targets.

## Consequences

- A candidate's evidence survives a clean merge with the same tree. A completion
  record changes the tree and selects every check that reads that record.
- Config authors must audit input classes, tools, relevant environment, test
  membership and producer outputs. An absent audit continues to run the check.
- This implementation proves mode coverage only for identical commands of the
  same target. Different-command and cross-target coverage are not credited.
- Environment fingerprints use an owner-only key on Unix. When that protection
  is unavailable, including native Windows, sensitive dependencies always run.
  Audited non-secret dependencies use value digests on every platform.
- Strict runs fail when no target executes or candidate evidence skips a
  requested target. Caller exclusions through `--only` or mode membership are
  not skips. `--strict --all --only X` can pass while remaining incomplete
  full-gate evidence. Summaries show executed and skipped counts and identify
  incomplete runs.
- Skipped targets are recorded separately from executed results. Any skip makes
  a run incomplete. Landing candidates use `--all` and execute every target.
- Pre-push uses the same selector. Its ref, authority, range, release and
  checked-out-tree checks remain independent and still run.
- TSK-353 owns this repository's target declarations. Until those declarations
  exist, the new selector does not reduce this repository's gate membership.

## Architecture impact

The test-gate selection and result-artifact rows in `docs/architecture.md`
now describe the default task-line base and per-target evidence. The field
reference lives in `docs/test-gate.md`; CAP-004 links the behavior and tests.
