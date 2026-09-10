# Portal ownership verification

## Status and scope

The runtime ownership implementation and producer verification are complete at
`ad0eec9bb090ac2f7fd19dc1396c1cb124c1e13f`, based on
`aa9388b1a91252543dfd120fb28290ee10c815e1`. TSK-020 remains in progress:
native behavioral diagnostics and final integrated review are not yet complete.
This report does not authorize a merge or claim full model qualification.

The delivered contract replaces the portal-specific pristine-source cache with
explicit managed or project-owned runtime status. Managed updates preflight
drift; explicit transfer preserves the entire runtime, including intentional
deletions, and later updates do not reclaim it. Configuration, tokens and
additional project assets retain their separate ownership. The ordinary
CodeFlow scaffold baseline is unaffected. See [ADR-0058](../decisions/ADR-0058-explicit-portal-runtime-ownership.md)
and [SPC-008](../../project-management/specs/SPC-008.md) for authority and recovery.

## End-to-end evidence

An independent no-hardlinks clone exercised the actual install, build,
validation, transfer and general update commands. It had no remote and retained
ordinary Git hooks. The browser runs used task-owned profiles, loopback servers,
output directories and verified teardown; they did not use the operator's browser.

| Phase | Runtime identity | Build and validation | Browser result |
|---|---|---|---|
| Managed, `1ef66ac917` | `@codeflow/docs-portal` 2.0.0 | 115 HTML pages; 109 source pages validated | Chromium, Firefox and WebKit; 19 named checks per engine |
| Transferred fork, `0fcccbf29c` | `@example/project-guide` 7.0.0 | 115 HTML pages; 109 source pages validated | Same three engines and checks; teardown verified |

The journey edited configuration and a runtime file, deleted a runtime asset,
transferred ownership, repeated setup, changed the real package/lock/generator
identity coherently, committed the fork and ran general `codeflow update`.
Runtime bytes, intended absence, configuration and adoption-state bytes survived.
The fork's full Node suite passed 93 tests. Later changes through `ad0eec9bb`
repair test setup and a case-list assertion, not rendered runtime behavior.

The retained run-5 receipt contains exact commits, commands, file hashes,
screenshots, traces and teardown results. A source-level test additionally proves
that a failed fixture initialization removes only its own temporary root and
preserves the original failure; a sibling sentinel stays unchanged.

## Deterministic checks

The strict aggregate at `547026ca` passed nine targets and failed three. It remains
a failed run. The following result combines those unaffected passing checks with
the repaired targets and affected checks rerun at `ad0eec9bb`; it is not a claim
that one aggregate invocation passed all twelve targets.

| Check | Result |
|---|---|
| Rust workspace | 2,080 tests passed; zero failed |
| Line coverage | 50,820 / 56,145 lines; 90.52%, above unchanged 90% gate |
| Formatting, all-target Clippy and rustdoc | Passed again at the final source checkpoint |
| Gate parity, evaluation kit, skill triggers, Herdr and docs validation | Unaffected strict-run targets passed; evaluation kit: 47 tests |
| Presentation qualification | Exact configured target passed, including browser isolation, teardown and cleanup fault injection |
| Portal qualification | Build, validation and runtime checks passed |
| Changed-range Git discipline | Five commits passed CI/work-start checks |
| Changed-range secret scan | Gitleaks 8.30.1; zero leaks |
| Normal durability push | All nine quick targets passed |

The stale assertion expected eight portal cases after five approved ownership
cases had been added. Its repair preserves the original eight and exact ordered
comparison; the focused contract suite passes 32 tests. The presentation failure
came from selecting a different compression-library build of Node 26.4.0. The
passing rerun uses the verified official runtime and unchanged asset budgets,
not relaxed assertions or regenerated limits.

## Independent Claude review

Native Opus 5 was used after the recorded Fable availability failure; high
effort was requested, not independently observed. Exact source reviews cover
the ownership distribution, generator provenance, keyboard verification, failed
fixture cleanup and final case-list repair. Combined source coherence was
reviewed at `547026ca`, followed by the exact final test-only delta.

A separate native test review actually passed 17 ownership, one provenance and
seven CLI test cases. Its Node run passed 86 tests and failed seven fixture setups
inside the sandbox. Its browser attempt had zero passes, four failures and one
cancellation. Chromium reported a sandbox IPC denial; the Firefox and WebKit
causes were not established. These are not native browser passes. Claude reviewed
five retained screenshots from the successful producer journey for rendered
conformance. Source review, screenshot review and native execution remain
distinct evidence categories.

## Release measurement

Fresh isolated release builds on macOS arm64, with the same Rust/Cargo 1.94.0
toolchain, produced the following whole-binary comparison:

| Source | Bytes | SHA-256 |
|---|---:|---|
| `aa9388b1` | 17,930,720 | `1eb890d85562d4854b51714bd9edc41b8ff0e6b6d248f211f91632e9df6bda87` |
| `ad0eec9bb` | 17,949,584 | `5917fd903083934b099e6c46299bec426ef4e3770db3b16362e56aa5aa7760c1` |

The increase is 18,864 bytes, not an isolated portal-payload measurement. Actual
installation proves embedded starter/package 2.0.0. The starter is 804,882 bytes
unpacked and 283,753 bytes as a gzip archive-equivalent, within the existing
2 MiB / 1 MiB budgets. Windows and Linux runtime behavior is not inferred from
this macOS measurement.

## Retained failures and remaining evidence

Failed preparation runs, the real hard-coded generator-version defect, the
keyboard navigation helper defect and failed initializer cleanup are retained
alongside their reviewed repairs. The original release comparison was invalid
because it reused the wrong release fingerprints; it supplies no size claim.
Fourteen leaked test trees were preserved with matching before/after inventories,
not silently deleted or excluded from asset checks.

Native diagnostics have three distinct dispositions; they must not be collapsed
into a single unavailable-lane claim:

- Four fresh Agent OS diagnostics used its current `c7de06bf` candidate.
  `documentation-portal-delegation-029` and
  `model-qualification-release-readiness-003` failed their strict JSON envelope
  because the response was fenced. `estimation-missing-capability-005` and
  `estimation-context-preview-001` passed the supported envelope, correlation,
  fixture-integrity and response-semantic checks, but remain inconclusive
  overall because a supported tool trace was unavailable. These are Agent OS
  observations, not TSK-020 ownership-case passes.
- One fresh CodeFlow TSK-018 decline trial was accepted and then ended with a
  native `StopFailure` HTTP 429. Its fixture remained unchanged and it was not
  retried. This is a started trial that errored, not a preflight denial; the
  error does not establish which internal model exhausted its limit.
- The isolated official-plugin canary completed, but subsequent native
  permission-review denials prevented the intended Codex batch from launching.
  Earlier sandbox startup failures remain infrastructure observations. Neither
  a successful canary nor an accepted operator prompt proves a subject ran.

Both-family subjective testing remains incomplete. No score, model promotion or
completed behavioral cohort is claimed. Final integration must add the remaining
ownership-case dispositions and exact-head independent review before completion.

Detailed producer receipts are retained in the task worktree's `.state/`
(`tsk-020-browser-qualification-run5`, `tsk-020-full-strict-v2.log`, affected
rerun logs and `tsk-020-release-comparison-run3`). Native returns and the
prospective pilot are retained in the external session evidence collection.
These local evidence locations are not distributed portal runtime dependencies.
