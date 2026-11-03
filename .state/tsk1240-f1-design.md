# TSK-1240: main-built policy caches only

Part (b)-only proposal for Fable's short re-read. The Director dropped part
(a) after F2 found no eligible additional reuse sequence. No new gate-evidence
reuse, provenance producer, verifier or artifact transfer remains. Existing
TSK-265/359 reuse is unchanged. No implementation, heavy run, merge or push
is authorized before the design re-read.

## Outcome and criteria

Main's own push at SHA X builds and saves its policy binary. A later PR-target
event with main base X restores that exact binary, reducing its build time.
A new main SHA misses and rebuilds. Integration builds never populate main's
cache scope. There is no integration-target speedup claim.

| Criterion | Design obligation |
|---|---|
| AC-1 | Retain historical measurements and input/scope inventory |
| AC-2 | Exact main SHA and environment/toolchain key, no fallback, main rebuild on miss |
| AC-3 | Only push on refs/heads/main saves main binary or dependency caches |
| AC-4 | Verify restored source/clean identity, rebuild wrong entries, refuse wrong rebuilt identity |
| AC-5 | Cross-lineage cache-security review, including integration poisoning |
| AC-6 | Main saves X; PR-target base X hits faster than a PR-target miss; changed main SHA misses |

AC-7/AC-8 are removed with part (a); their IDs are not reused. AC-5 no longer
asks for evidence-reuse attacks. AC-1/2/3/4/6 remain unchanged by this ruling.

## Four binding rules

1. **No integration execution under main's ref.** Remove the main dispatch
   writer, build_sha input and integration checkout/containment path. No main
   job builds, installs toolchains from, tests or executes an integration tree
   or artifact. Main push builds only its own immutable event SHA. Main-target
   PR policy checks use their main base SHA. For integration-target PR checks,
   keep target commits as Git data and use the trusted main-built policy binary
   rather than build the integration target under pull_request_target's main
   ref. Preserve target-policy/record inspection and head-as-data behavior;
   the main binary must never load executable helpers from the integration
   tree. The existing admission script must likewise come from trusted main.
   This path needs review of compatibility with target policy before building;
   no unverified target-code execution is retained as a fallback.

2. **No main cache of any kind from integration output.** The only producer
   is push:main executing main code on its own runner. No dispatch or artifact
   publisher survives. Binary and dependency caches use fresh main-only
   namespaces with no fallback to old potentially contaminated entries.
   No integration artifact is downloaded, restored, copied into a build tree,
   executed or saved in main scope, even if that SHA later lands on main.
   After landing, build afresh as main code. This task disables integration
   cache writes in the policy workflow rather than retaining a second producer.

3. **No new run-evidence trust boundary.** The API run/workflow-blob/tree/digest
   verifier required for part (a) is removed with that part. Nothing accepts
   integration run IDs, job text, manifests or artifact hashes as authority.
   Existing TSK-265/359 bindings remain unchanged. Part (b)'s source identity
   comes from the event's trusted main commit and clean checkout; cache keys
   are calculated locally before fetching any PR head, never from head input.

4. **Fail closed.** Wrong, absent or inexact binary entries rebuild only from
   trusted main source. A dirty source, failed build or wrong rebuilt identity
   refuses before PATH or save-enabling output. No integration build fallback.
   The PR body explains runtime-token arbitrary-key cache poisoning and why
   self-reported binary identity cannot authenticate an adversarial writer.
   Main-owned production plus read-only PR consumers is the standard boundary;
   an identity string detects stale entries, not malicious trusted producers.

## Workflow behavior

Only `.github/workflows/codeflow-policy.yml` changes. Remove workflow_dispatch
and its inputs, the warm job, actions: write and dispatched-SHA selection.
Remove master consistently from triggers/allowed in this main-only repository
copy; the shipped template is untouched. Top-level permissions stay exactly
contents: read. Preserve the existing policy job's necessary read permissions.

The binary key includes exact main source SHA, rustc/Cargo and compiler identity,
runner image and architecture, and build flags. A versioned main-only namespace
fixes release/default-feature/native-target choices. No restore-keys. Separate
binary cache storage from Cargo's build tree; roll dependency namespace too.

Use lookup-only on push:main restore. An exact existing entry avoids redundant
cache population without downloading/executing that entry; any binary needed
for the push's own policy work must still be obtained through the trusted build
path. PR-target readers restore bytes, then perform the identity check. Remove
any separate pre-check version execution; the identity predicate is the single
version probe. Require executable regular file, exact source SHA and clean
state. Recheck newly built identity before exposing the binary.

Binary save-if requires verified fresh build, cache miss, and exactly
`github.event_name == 'push' && github.ref == 'refs/heads/main'`.
Dependency save-if also requires exactly that event/ref conjunction. No other
event is allowed to save. workflow_dispatch and master never enter allowed.
Use strict shell mode. A missing cache never weakens these rules.

## Tests and hosted proof

- Both contract suites assert exact top-level contents: read and no write
  permission anywhere after removing the dispatcher. Reject any top-level or
  job-level contents/pull-requests/actions write addition.
- Mutation tests reject every non-main save: PR head, integration push,
  arbitrary-ref dispatch, workflow_dispatch in allowed, master in allowed,
  or either save-if missing the push/main-ref conjunction.
- Reject integration checkout/build/toolchain/helper execution on main-ref
  events, artifact transfer and old/integration namespace fallback. Confirm
  integration-target policy checks read target Git data with trusted tooling.
- Assert lookup-only for push:main, ordinary restore for eligible readers,
  no fallback keys, key changes on SHA/toolchain/image changes, dirty-tree
  refusal and absence of head-derived keys.
- Retain stale/dirty/malformed/inexact binary rebuild cases and wrong rebuilt
  identity refusal. Mocked executables are consistency tests, not signatures.
- Hosted AC-6 sequence: main push saves SHA X, main-target PR base X hits,
  then moving main misses. Compare the hit job with the AC-1 PR-target miss
  baseline, not with a push job. Keep runner/toolchain conditions comparable.
- AC-5 requires the new cross-lineage review; old reviews and the old quick
  gate do not approve this design's implementation. Hosted evidence is still
  unavailable until trusted main workflow content carries the change.

## Scope decision and residuals

F2's bounded search found no additional eligible sequence. Closest:
[integration run 41069570670](https://github.com/sathyassn/codeflow/actions/runs/41069570670)
and main run 41117114880 share a tree, but changed cache inputs and missing
release-gate coverage prohibit reuse. Retaining (a) would add an unused trust
surface; the Director removed it. No change to scripts/ci-reuse.py,
codeflow-ci.yml or gate verdict jobs remains in this task.

Existing verdict reuse still inherits its source run's caches. That is an
existing residual, not a new TSK-1240 evidence path or permission to transfer
integration caches into main. Main's own code/cache writers remain trusted;
this task cannot authenticate against malicious control of main itself.

## Operator delivery and remaining review

Prepare the policy workflow patch against a copy and preserve
`target/task-evidence/codeflow-policy.yml.patch`. Deliver the untracked script
`coordination/scripts/operator/apply-tsk1240-codeflow-policy.sh` with a plain
header, exact task-head and line-base pins, drift refusal, apply-check,
apply/commit/push and printed head/ref/stat. The operator reviews and runs it;
the builder does not edit the real workflow or run the script.

Non-workflow work after approval is limited to the relevant contract tests,
task/PR evidence and necessary main-owned helper changes. Planning owns
TSK-1248's follow-up re-scope. Do not build before Fable re-reads this b-only
design, including the main-ref integration-target reader boundary in rule 1.

TSK-1240 design is main-built caches only; AC-5 narrowed, AC-7/AC-8 removed; awaiting Fable re-read, no implementation or push.
