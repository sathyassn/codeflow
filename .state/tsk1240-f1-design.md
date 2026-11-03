# TSK-1240: exact-tree gate evidence reuse and main-built caches

Design for Fable review under the Director's re-scope. Exactly two parts:
(a) reuse verified integration-push gate evidence for the exact judged tree;
(b) build and save main's policy-binary cache from main's own push code.
The integration-to-main cache promise is removed. No implementation, task-record
edit, line merge, build or push is authorized by this document.

## Design map

| Section | Part | Binding rules |
|---|---|---|
| Evidence verification and reuse | (a) | 1, 2, 3, 4 |
| Main-built cache | (b) | 1, 2, 3, 4; rule 3 binds any reused gate evidence, not a transferred binary |
| Permissions and isolation | (a), (b) | 1, 2, 4 |
| Negative tests and hosted proof | (a), (b) | 1–4 |
| Workflow delivery and planning handoff | (a), (b) | 1–4 preserved through review and operator application |

## Four numbered binding requirements

1. **Main-ref execution never executes integration code.** The evidence verifier
   uses a pinned main-owned helper and workflow only. It reads integration Git
   objects through the API as data, never checking out, building, testing,
   installing a toolchain from, sourcing or executing that tree. It does not
   run an artifact's `--version`. Part (b) builds only the main push's own
   immutable event SHA, never a dispatch-selected SHA. If an integration commit
   later lands on main, it is built afresh as main code; no integration output
   is reused as an executable or build input.

2. **Main never saves any cache from anything integration produced.** Part (a)
   has no cache action, cache API call, dependency-cache restore/save or save
   post-hook. Its sole output is a main-generated verified evidence record
   and a decision consumed by trusted verdict jobs. There is no binary-transfer
   publisher, including under a separate namespace. Integration archives are
   downloaded only for digest verification, never extracted or executed.
   Part (b) uses a separate fresh job, workspace and main-only cache namespace;
   dependencies also use a new main-only namespace with no legacy/integration
   fallback. It never reads part (a)'s downloaded artifacts. Main's push always
   populates its own binary cache as needed even when (a) reuses the gate.

3. **Reuse binds only independently verified run and object identities.** Run
   IDs and commit inputs are lookup hints, not evidence. For part (a), API and
   Git-object checks below bind run ID, exact workflow path and base-branch
   workflow blob SHA, event push, exact source head_sha, integration/* ref,
   success, artifact API digest and exact judged tree. Never trust printed
   run text, self-reported version, an artifact manifest or caller-supplied
   checksum. Part (b)'s key is computed only from its checked-out main event
   SHA and actual build environment; no source-run key or binary enters it.

4. **Fail closed and explain the threat.** Any missing, ambiguous, stale or
   mismatching provenance denies reuse and emits no verified success. The
   trusted consumer runs the ordinary required gates instead; if that cannot
   happen, its verdict fails. A main cache miss rebuilds main rather than
   falling back to integration data. The PR body will name arbitrary-key
   runtime-token cache poisoning and forgeable binary identity, and explain
   why separating lower-trust execution, trusted evidence verification and
   main-owned cache production is the standard design. No unknown evidence
   becomes a passing check.

## (a) Exact-tree gate evidence verification and reuse — AC-7/AC-8, rules 1–4

Extend the existing hosted reuse mechanism in `scripts/ci-reuse.py`, rather
than create another independent admission authority. A main-ref check verifies
an actual completed `codeflow-ci` integration-push run. The current policy-cache
warm dispatch is removed: a successful policy-only run cannot stand in for the
full gate. The main check emits a record naming both the source run and the
commit/tree being judged. Required gate verdict jobs cite that record when
reusing the result; they never treat an unverified skip as success.

Source discovery is restricted to the pushed main commit's Git topology:
for a merge, use only its second parent; for a fast-forward, use the exact
pushed tip carried forward from the source line. The first parent is the
pre-push baseline, not a license to scan its ancestors. Reject ambiguous
multi-parent merges. Never accept a candidate SHA/run ID from workflow input,
PR text or a broad equal-tree search over integration runs. Resolve each
eligible commit through `actions/runs?head_sha=<exact-sha>&event=push`, then
require API head_branch `integration/*` and exact path
`.github/workflows/codeflow-ci.yml`. A fast-forward tip qualifies only when
that exact tip has the required API integration-push record. The verifier
obtains the judged commit from its trusted main event and independently checks:

- Fixed repository endpoints; run ID and attempt are positive API identities;
  repository and head-repository IDs equal this repository. Never follow an
  input-supplied repository, API URL or artifact URL.
- Run event is push, API head_branch is an integration/* branch, API head_sha
  equals the exact selected source commit, and status/conclusion are completed
  and success. Check every required gate job in that attempt, not only the
  workflow's overall conclusion. Reject a draft-skipped, partial, cancelled,
  failed or missing gate; initially require executed source jobs rather than
  recursive reuse credit.
- Run workflow ID/path resolve to `.github/workflows/codeflow-ci.yml`. Read
  that file's blob at the source push SHA and require equality with the blob
  on the pinned main/base revision whose check is being executed. Confirm the
  push's executed workflow revision association; if unavailable, deny reuse.
  A display name, same filename or a matching artifact field is insufficient.
  Pin/check the decider helper and executable workflow dependencies as well.
- Obtain source and judged commits' tree SHAs from Git objects/API and require
  exact equality. Source head_sha remains bound to its exact source commit:
  a later main merge may have a different commit SHA but must have the same
  tree. No changed-record, same-diff or partial-path exception. The record
  states both commit SHAs, both tree SHAs and the pinned base/workflow blob.
- Validate the exact check: required job membership, full-mode commands and
  config, dependencies and tool/environment identity. Follow ADR-0085's
  exact-check principle and preserve existing stronger reuse checks. Whole-tree
  equality alone is not sufficient. Evidence generated by repository code
  cannot prove its own environment; corroborate with platform-owned metadata.
  If the relevant environment cannot be independently established, run the
  gate. Do not use text printed inside the run as authority.
- Select the expected evidence artifact by immutable ID within that run and
  attempt; require one unexpired match and consistent API repository/run/SHA
  fields. Compare the downloaded archive hash to the authenticated API digest.
  Missing digest, ambiguous attempt, API failure or mismatch denies reuse.
  Hash opaque archive bytes without extraction. The artifact does not supply
  authoritative verdict/identity fields: the verifier builds those from API
  and Git data. No binary artifact is needed for the rescoped design.
- Recheck run attempt/status before issuing the record. A rerun race invalidates
  the decision. No earlier green attempt can override a newer failed attempt.

Extend `provenance` to run on integration push runs as well as PR runs. Its
push record names the exact head commit, tree, workflow blob and runner image,
using the existing `read_provenance` shape adapted for the push event. Read
the record as data only, never executable input. Bind its archive digest to
the API and corroborate each authority-bearing field with API/Git/platform
evidence; the manifest alone does not prove its claims. A missing integration
push provenance artifact denies reuse, never drops the digest obligation.

The benefit is avoiding the whole duplicate gate run for an eligible identical
main tree, not avoiding main's own cache build. Main's other independent checks
still run: policy/admission, release, and any check whose evidence does not
match. This is hosted API-backed evidence reuse, not a claim that a local
ADR-0085 run file is authenticated landing evidence. The checked-out ADR is
marked proposed and distinguishes local selection from landing; the Director's
explicit ruling authorizes this hosted extension, subject to design review.

### F2: observed eligibility and AC-7 hosted case

No additional eligible real sequence was found in the bounded investigation
on 2026-11-03. AC-7's hosted accepted case remains unproven; planning must
decide the value/scope of (a) before implementation. This design retains (a).
Do not substitute a fabricated direct push or bypass the repository PR flow
to manufacture an example.

Checked the latest ten main push-run summaries, the latest forty push-run
summaries across branches, and the commits, parents, source-head push-run
queries and job lists for the three most recent main pushes:

- [November 1 main run 41311187540](https://github.com/sathyassn/codeflow/actions/runs/41311187540),
  commit `b2bb61af`: merge of planning PR 462. Its second parent
  `fdce7f10fb0942bb18ae443872b9a4e548537796` has zero push runs in the API.
- [October 31 main run 41230613447](https://github.com/sathyassn/codeflow/actions/runs/41230613447),
  commit `941bc470`: merge of planning PR 439. Its second parent
  `ab3af8de0b07cee80de7827f5ff86b23ee8971ca` also has zero push runs.
- [October 30 main run 41117114880](https://github.com/sathyassn/codeflow/actions/runs/41117114880),
  commit `997c60b0`, merged integration PR 396. Its second parent `461827479`
  has successful [integration run 41069570670](https://github.com/sathyassn/codeflow/actions/runs/41069570670).
  Both commits have tree `12404e5cabc5adfda819fe751215277f852b1af8`.
  However the push changes Cargo.lock, Cargo.toml, codeflow-ci.yml and
  scripts/ci-reuse.py relative to first parent `4226a940`. The existing
  cache-input guard must still run the gate; (a) cannot claim this saving.
  The source also skips the release gate parts that the main run executes,
  so it does not cover the exact main check membership.

Also checked recent [integration run 41637539351](https://github.com/sathyassn/codeflow/actions/runs/41637539351):
its provenance job is skipped and release parts are skipped. This confirms
the provenance producer change is needed, not a ready accepted case.
No claim is made about all historical runs, a hypothetical future landing,
or independently matching runner environments for these rejected candidates.
Preserve the current cache-input and exact-check-membership guards: a required
main gate/cache producer without source evidence still executes. Main policy
binary cache production under (b) is independent of these existing safeguards.

Accepted residual for the PR body and AC-5 review: a reused verdict inherits
the source run's dependency/build caches, including its integration/main
prefix restores, as existing PR-run reuse does. This authenticates which
run passed, not pristine dependencies. No cache bytes cross into main through
this evidence path; a stronger source-cache trust policy is not claimed.

## (b) Main's own push-built cache — rules 1–4

AC mapping: AC-2 covers the exact key and miss; AC-3 covers main-only saves;
AC-4 covers restored identity; AC-6 covers the hosted hit/miss journey.
For AC-6, main's push saves at SHA X, a PR event with main base X restores
that entry with a shorter job time, and a push moving main to a new SHA
misses and rebuilds. Two pushes with different SHAs cannot demonstrate a hit.

In `codeflow-policy.yml`, only push:main builds and saves a main-scope binary
from the exact main event SHA. Verify clean source identity after the fresh
build. Key exact SHA plus compiler/Cargo, image/architecture and build inputs;
use a versioned main-only namespace without fallback. Main-target readers may
restore that cache and rebuild from their main target on a miss or stale
identity. Identity is a consistency check, not proof against a malicious writer.

On push:main, restore is lookup-only; an exact existing entry skips redundant
cache population without downloading or executing it. PR-target readers use
ordinary restore and identity/rebuild checks. Binary and dependency save-if
both require exactly `github.event_name == 'push' && github.ref == 'refs/heads/main'`
(plus the verified-build/miss predicate where applicable). Remove the
workflow_dispatch trigger and build_sha input from this policy workflow;
dispatch is never in `allowed`. Remove master from `allowed`. If any separate
integration-scope caching remains, its namespace must be distinct from main's
and must not be restored by a main-target consumer. No integration save uses
the main-only namespace. The shipped workflow template stays unchanged.

Remove integration-to-main warming and all dispatch-selected source builds.
Integration-target PRs keep their required target builds; there is no promise
that integration push caches are visible to them. Integration builds must not
populate main policy or dependency caches. New namespaces exclude potentially
contaminated entries left by the previous design, without deleting caches.
Remove master consistently from this repository's main-only policy path.

## Permissions and isolation — (a), (b), rules 1, 2, 4

- Evidence verifier: contents: read and actions: read, main-owned code; no
  secrets, no cache action or artifact execution. Ordinary workflow outputs
  and a small evidence JSON record carry the verified verdict.
- Integration gate producer: retain existing least-privilege permissions; its
  artifact remains integration trust. No actions: write dispatcher is needed
  for API discovery by the main push check.
- Main cache producer: contents: read, main code and fresh workspace; cache
  runtime authority is exercised only for its own output.
- Policy reader: existing contents/pull-requests/actions read permissions;
  preserve platform read-only cache access for pull_request_target.

Top-level permissions remain contents: read. Remove the old warm job and its
write grant; both contract suites reject any added write permission. Do not
claim GITHUB_TOKEN permissions restrict the separate runtime cache token.
Use strict shell mode wherever shell remains; no pre-check artifact execution.

## Tests and proof — (a), (b), rules 1–4

For (a), mutate run/repository/workflow path/blob, source head SHA, judged tree,
event/ref, attempt, job coverage, conclusion, environment and artifact digest.
Every mismatch must deny reuse and select the real gate, never produce a green
skip. Cover same tree/different commit (eligible with all other bindings),
same commit claim/wrong API head, same path/different workflow blob, changed
completion record, stale run, partial gate, rerun race and correct digest from
the wrong run. A sentinel executable archive must never execute. Mutations
adding checkout/build/toolchain/test/artifact execution or any cache operation
to the verifier fail the contract tests.

Add discovery mutations that accept a workflow input, unrelated equal-tree
commit, first-parent ancestor or broad run scan; each must fail. Test both
allowed topology cases and API filters. A mutation that removes integration
push from `provenance`, omits any required push field, or trusts artifact text
without corroboration must fail; missing provenance gets a named refusal.

For (b), mutation tests reject integration/dispatch saves, source-run keys,
legacy dependency fallback and integration artifacts as main build inputs.
Retain exact-hit/miss, stale identity and wrong-rebuild refusal tests. Verify
main cache production still occurs when the independent gate is reused.

Contract tests explicitly reject dispatch in `allowed`, retained policy
build_sha dispatch inputs, master in `allowed`, either main-cache save-if
without the push/main-ref conjunction, an integration save in main's namespace,
and non-lookup push:main restore. F2's rejected examples remain refusal cases,
not AC-7 hosted success evidence. AC-8 has a named denial test for every
mismatch kind; AC-5 covers the cross-lineage security review and residuals.

Hosted proof must show a qualifying exact-tree reuse naming its source run,
a changed-tree/workflow rejection that actually runs the gate, a main-owned
cache save and later main-target hit/miss. No mocked fixture proves hosted
API metadata or runner-environment binding; unqualified cases run the gate.

## Files, operator delivery and planning handoff — (a), (b), rules 1–4

Workflow changes: `.github/workflows/codeflow-ci.yml` for (a)'s trusted reuse
and verdict wiring, and `.github/workflows/codeflow-policy.yml` for (b) and
removal of the unsafe integration warmer. No release workflow or shipped
workflow template change is proposed. Extend scripts/ci-reuse.py and its tests,
plus the policy-cache and gate-contract suites and agreed documentation.

Prepare both workflow diffs against copies only. Preserve patches under
`target/task-evidence/` and deliver them through the untracked operator script
`coordination/scripts/operator/apply-tsk1240-codeflow-ci.sh`, explicitly naming
both workflow paths in its header. Pin exact task head and line base; refuse
movement; apply-check both patches before any apply, then apply, commit, push,
and print head/ref/stat. Only the operator runs it. No real workflow writes
or script execution by this builder.

Planning's record is committed at `6c3ea9f6c`, with corrected AC-6 at
`9b5e5f0dc`. AC-7/AC-8 map to (a); AC-2/AC-3/AC-4/AC-6 to (b); AC-5
reviews both. AC-1 retains the historical baseline. F3's record path and the
AC-6 comparison-pair wording remain planning-owned. This update changes only
the design, not the task record. No criterion is marked complete.

TSK-1240 design updated for Fable r2; no additional eligible F2 sequence found in the checked runs; planning decision remains owed, no implementation or push.
