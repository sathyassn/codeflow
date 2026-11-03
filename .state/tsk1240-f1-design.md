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

1. **Remove the integration dispatch writer; preserve target judging.** Remove
   build_sha and the dispatched integration checkout/build. Main push builds
   only its immutable event SHA. Integration-target PR runs keep checking out
   and building their own base target as today and never save. Their token's
   cache access stays read-only. This task does not replace the target's binary
   or admission helper with main's, so ahead-of-main policy schemas remain
   judged by the target's own code.

2. **No main cache of any kind from integration output.** The only producer
   is push:main executing main code on its own runner. No dispatch or artifact
   publisher survives. Binary and dependency caches use fresh main-only
   namespaces with no fallback to old potentially contaminated entries.
   No integration artifact is downloaded, restored, copied into a build tree,
   executed or saved in main scope, even if that SHA later lands on main.
   After landing, build afresh as main code. This task disables integration
   cache writes in the policy workflow rather than retaining a second producer.
   Cost: each integration push rebuilds dependencies not already supplied by
   a matching main dependency cache; it cannot retain its own new build output
   for later runs. This trades integration cache performance for simpler saves.

3. **No new run-evidence trust boundary.** The API run/workflow-blob/tree/digest
   verifier required for part (a) is removed with that part. Nothing accepts
   integration run IDs, job text, manifests or artifact hashes as authority.
   Existing TSK-265/359 bindings remain unchanged. Part (b)'s source identity
   comes from the event's trusted main commit and clean checkout; cache keys
   are calculated locally before fetching any PR head, never from head input.

4. **Fail closed.** Wrong, absent or inexact main binary entries rebuild from
   their main target. Integration targets always build their own source and
   never use or save the main binary cache. A dirty source, failed build or
   wrong rebuilt identity refuses before PATH or save-enabling output.
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
- Reject the dispatched integration writer, artifact transfer and legacy
  namespace fallback. Confirm integration targets retain their own build and
  that neither save step can run for integration pushes or PR events.
- Sweep `.github/workflows/` and `assets/base/ci/`: no sibling workflow may
  name the policy binary or dependency namespace and become another writer.
- Assert lookup-only for push:main, ordinary restore for eligible readers,
  no fallback keys, key changes on SHA/toolchain/image changes, dirty-tree
  refusal and absence of head-derived keys.
- Retain stale/dirty/malformed/inexact binary rebuild cases and wrong rebuilt
  identity refusal. Mocked executables are consistency tests, not signatures.
- Hosted AC-6 sequence: main push saves SHA X, main-target PR base X hits,
  then moving main misses. Compare the hit job with the AC-1 PR-target miss
  baseline, not with a push job. Keep runner/toolchain conditions comparable.
- Hosted AC-3 refusal: after the change reaches the line, cite an integration
  push run showing both binary and dependency save steps skipped, with cache
  API evidence that no new main-scope entry for that SHA was created.
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

This PR restores its real policy workflow to line revision
`59c9ae96547aaf8241e0c66029c809b80d748f51`, removing the withdrawn writer from
the branch. Other workflow files already match that line. The proposed (b)
text is `operator-patches/TSK-1240-codeflow-policy.proposed.yml`, pinned by
SHA-256 in both contract suites' shared loader. Tests read that proposal and
prove the operator patch produces exactly its bytes. The real file may be
only the pinned line text or the identical operator-applied proposal.

Deliver `operator-patches/apply-TSK-1240.sh` and
`operator-patches/TSK-1240-codeflow-policy.patch`. The script checks the exact
task branch, line-workflow digest, proposal and patch digests, both test-script
digests and the existing admission helper digest. It runs git apply --check,
applies, verifies byte equality, and stops. No commit or push. Only the operator
runs it after review, so trusted main receives only reviewed identical text
through the normal landing path; the script itself refuses main. Validate
with git apply --check and sh -n without executing the script.

Non-workflow work after approval is limited to the relevant contract tests,
task/PR evidence and necessary main-owned helper changes. Planning owns
TSK-1248's follow-up re-scope. The r3 repairs are light edits and patch syntax
validation only; execution tests and hosted evidence remain owed.

TSK-1240 r3 design repairs preserve target builds, remove the withdrawn writer, and deliver the main-only change through an operator patch; no heavy run or push.
