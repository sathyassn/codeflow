---
id: ADR-0103
title: Changed-line coverage requires an explicit base
date: 2026-11-14
status: proposed
superseded_by: null
architecture_impact: A separate coverage report evaluates changed executable lines against an explicit comparison base and validates shard provenance
---

# ADR-0103: Changed-line coverage requires an explicit base

## Context

ADR-0021 refuses `changed_files` because the test gate cannot establish a
comparison base. Its existing threshold evaluator measures whole files, so it
cannot establish coverage of added lines. Hosted coverage already exports
per-line counters, and the event supplies the comparison revisions.

## Decision

Changed-line coverage is permitted when the caller supplies an explicit,
resolved base and head, their diff and the coverage of that head. This
supersedes ADR-0021's blanket rejection of changed coverage while preserving
its refusal of unavailable inputs. The dedicated report states an 80% floor
and a 90% target; the aggregate 90% gate remains independent. The standalone
test-config `changed_files` scope remains refused because its evaluator has
only file totals and no explicit comparison contract. It cannot substitute
for the changed-line report.

### Comparison inputs

| Caller | Base | Head | Missing or unresolvable input |
|---|---|---|---|
| Pull request | event `pull_request.base.sha` | event `pull_request.head.sha` | Fail with the cause |
| Push | event `before` | event `after` | Fail, including an all-zero initial-push base |
| Local | explicitly supplied `--since`, resolved by the caller | explicitly resolved head | With no base, print `unavailable: no comparison base` and enforce nothing; an explicitly supplied but unresolvable base fails |
| Schedule or manual hosted run | explicitly supplied comparison revision | explicitly resolved head | Fail when no base was supplied |

The caller resolves both revisions before producing the diff. The report
does not invoke git, infer `HEAD~1`, select a default branch, or use the
best-effort `detect_changed_files` helper. It consumes the explicit
base-to-head diff with zero context and a separate changed-path inventory.
The inventory prevents an empty diff from passing a range that adds or
changes code. Deleted files have no head lines; a rename uses its head path.
A deletion-only or pure-rename range reports no added executable lines.

TSK-1190's `CODEFLOW_BASE` and `CODEFLOW_HEAD` are compatible informational
inputs, but `unknown` is never a resolved revision. Its changed-path file
does not contain line ranges and cannot replace the explicit diff. Neither
report inputs nor results grant target-selection credit (ADR-0089).

### Coverage and paths

The report measures added head lines represented in per-line coverage.
An added code file absent from the report fails. An addition that maps to
no executable line fails with the cause rather than manufacturing a share.
LCOV `DA` records, Cobertura line records and llvm-cov regions supply line
hits; duplicate observations merge by whether any observation ran the line.
The aggregate merger retains its existing llvm-cov counting rules.

Paths are repository-relative, use `/`, and preserve case, Unicode and
spaces. A report's absolute paths must start with its recorded source root.
Leading `./` and Windows separators are normalized; parent traversal, empty
paths and absolute paths outside that root fail. Git's quoted paths are
decoded before matching. Exemptions use exact normalized paths in one JSON
list; every entry requires an owner, reason and removal condition.

### Shard inputs

Each shard record binds its report digest to revision, run, attempt, target,
shard number, shard count, format and source root. The consumer compares the
records with the expected revision, run and attempt and the configured
coverage roster. It refuses failed, stale, duplicate, missing or unexpected
shards before reading coverage. A digest binds bytes; it does not authenticate
head-produced reports. Hosted workflow results remain the run's authority.

## Consequences

- Reviewers see changed-line coverage where the workflow runs coverage.
  Pull requests into an integration line lose that report after the planned
  CI-placement sync removes coverage from that event. This task does not
  change placement or amend its acceptance criterion.
- Callers must retain explicit comparison revisions and produce both diff
  and inventory from that range. A shallow checkout cannot silently skip
  enforcement when the base is unavailable.
- A separate per-line reader avoids changing aggregate coverage semantics.
  The additional readers and shard format require malformed-input fixtures.
- Local runs without a base remain informational. Hosted runs without one
  cannot claim a passing changed-line check.

## Architecture impact

The coverage row in `docs/architecture.md` distinguishes whole-file
thresholds from the explicit-base changed-line report and its shard binding.
