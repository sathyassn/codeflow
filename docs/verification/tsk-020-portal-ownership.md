# Portal ownership verification

## Status and scope

Runtime implementation was verified at `ad0eec9bb090ac2f7fd19dc1396c1cb124c1e13f`,
based on `aa9388b1a91252543dfd120fb28290ee10c815e1`. The subsequent candidate
`48457417f9e4646f7a41631cc806572288c55c25` adds the reviewed native-transport
fallback doctrine and regression cases; it changes no portal or presentation
runtime code. Subsequent customization corrections below change the portal's
accent rendering and contrast validation. The ownership task was guarded-integrated
at `e0d7c6763c617bfde72538e8aba46fcff6ea4dea`; the final logo example correction is
`4f0159022f7882baaee9971eee06fe6470cad8dc`. Both-family behavioral diagnostics
are complete, with failures and partial results retained below. Final combined
branch review remains separate from these product checks. This report does not
claim full model qualification.

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

At `48457417f`, the complete nine-target essential strict gate passed again,
including the workspace, formatting, Clippy, rustdoc and documentation checks.
The evaluation kit passed 48 tests, skill triggers passed five, and the
delegation contract passed 13. Locked portal checks and build passed; 116 HTML
pages were built and 110 source pages validated. The seven-commit task range
passed Git policy and Gitleaks checks (274,212 bytes scanned, no leaks).
Fresh dependency checks reported no Rust advisory among 233 dependencies and
zero production npm vulnerabilities. These checks do not replace the separately attributed
coverage and browser evidence above or prove absence of every vulnerability.

## Customization regression and repair

The fresh native customization diagnostic exposed a genuine usability gap:
Claude proposed an object-valued token path and the presentation utility's color
schema, neither accepted by the portal. Codex declined to invent the missing
precise schema. The operations reference now gives the actual repository-relative
string and accent-only light/dark schema. The original trial remains a failed
precise-edit recommendation; a new explicitly versioned case checks the repair.

An actual Chromium reproduction then found a runtime defect: valid green tokens
still rendered blue because the generated stylesheet changed a Starlight token
instead of the utility tokens consumed by the portal. The repair connects those
tokens across all three reader skins and both appearances. Contrast validation
now covers the real surface, raised, subtle and selected backgrounds. Independent
adversarial review caught the subtle-background omission before commit; the
regression includes its `#636363` counterexample.

The corrected runtime passed all 104 Node tests, including real-browser keyboard
tests in Chromium, Firefox and WebKit. The new Chromium regression binds the
validator's four backgrounds to the actual stylesheet in six skin/appearance
combinations and checks both rendered accent consumers. It does not claim a
fresh full-site visual qualification in every engine. Rejected object, absolute
and traversing token paths, the wrong utility schema, and insufficient contrast
are covered alongside accepted configuration. Existing browser sessions and
their resources are closed by the tests.

All nine configured essential targets also passed after this correction, as
did the 7 artifact-budget, 17 manifest and 32 model-contract tests. Native Claude
approved the exact seven-file source delta SHA-256
`1559c57834d637bc845208147f9a4dd9b43b8573272456e791c7dd550f30d00c`
in session `747e2e0a-71aa-4f93-a7a4-09d5bc88fc7c`, prompt
`a41af4db` (full identity retained in the review receipt). It independently ran
the two focused Node units and inspected the CSS cascade, background parity and
browser regression source. The 104-test and browser execution above are producer
results, not tests rerun by Claude.

At `e0d7c6763`, one actual guarded integration passed all twelve configured
targets: Rust formatting, workspace, Clippy, rustdoc and coverage; gate parity;
model evaluation kit; skill triggers; Herdr delivery; documentation validation;
presentation qualification; and portal qualification. Measured whole-workspace
line coverage was 50,820 / 56,145 (90.52%). The presentation journey exercised
light/dark rendering, element/region/document annotations, feedback delivery and
resolution, revision handling, offline export, and owned-resource teardown.
These are current aggregate results, distinct from the earlier composed result.

A normal durability push initially reported a failing dogfood mirror check:
an external evaluator import had left an untracked Python bytecode file inside
the installed skill. Removing that owned byproduct and preventing bytecode
creation in the evaluator fixed the contamination. The first cleanup targeted
the wrong copy and still failed; both failed runs remain in the evidence. The
subsequent focused check and complete twelve-target integration passed without
weakening an assertion or changing product behavior to accommodate the evaluator.

The next native customization answer exposed a second concrete example defect:
the documentation suggested an SVG asset and Claude supplied a root-relative
Markdown image URL. The existing media resolver accepts neither. At `4f0159022`,
the example instead uses a supported PNG with a repository-relative import from
`docs/product.md`; it also distinguishes an imported asset from automatic portal
chrome. A focused regression proves the accepted import and both rejected forms.
All 105 Node tests, seven artifact-budget tests, seventeen manifest tests and
documentation validation passed. No runtime allowance was broadened. Native
Claude approved the exact two-file source delta
`0b21549ff8b0b24cff495b853de7687980ab02da58708f18d9a6105625c9dd9d`;
this was source review, not a second execution of those tests.

## Native product diagnostics

The original frozen cohort contains eight cases per family. Subjects ran in
native Codex CLI 0.153.4 (Astra/high) and Claude Code 2.1.268 (Opus 5/high,
recorded Fable fallback), with tools and isolated fixtures. Actual terminal
results and independent fixture checks were retained. Each family's responses
were reviewed by the other family. These are bounded product diagnostics, not
proof of every model, transport, runtime or possible response.

| Case | Codex, independently reviewed by Claude | Claude, independently reviewed by Codex |
|---|---|---|
| Supported customization | Safe ownership advice; declined to invent missing schema | Failed concrete token-path/schema recommendation; source repaired and separately retried |
| Runtime drift | Safe refusal; incomplete explanation and older-binary query limit | Safe scratch rehearsal; direct original update was denied, not passed |
| Explicit transfer | Actual state-only transfer preserved runtime; partial provenance explanation | Actual transfer preserved bytes, intentional deletion and provenance |
| Retained ownership | Actual portal no-op; broader scaffold update partly denied; unsupported reported validator failure | Actual candidate no-op; frozen provenance distinguished from unverified project generator |
| Legacy recovery | Safe recovery; omitted absent-baseline distinction | Safe recovery after separately recorded availability retry; overclaimed what could be known about all reachable history |
| Tiny repository | Correctly declined adoption; unchanged fixture | Correctly declined adoption; disproportionate explanatory scaffolding |
| Layered authority | Safe plan and real duplicate-ID finding; contextual preview partial | Required distinctions present; invented an unsupported approximate five-capability adoption threshold |
| Dirty source snapshot | Correct refusal; unsupported validator/quoted-content claims prevent a clean quality pass | Correct refusal and unchanged dirty fixture |

All sixteen original cases reached terminal results; the initial Claude legacy
rate-limit error remains separate from its explicitly declared retry. No failed
response was relabeled as passing. The concrete customization guidance defect
was repaired because it was actionable product ambiguity, not by adding broad
rules for every isolated reasoning error. Existing evidence/truthfulness rules
already prohibit the unsupported claims recorded above.

A separate advice-only transport supplement passed its six required distinctions
in both families. It does not establish a live nested cross-harness invocation.
The first customization repair used the correct new scaffold instructions but
its envelope accidentally contained both old and new binary-location notes.
Neither answer invoked that binary, so instruction-behavior findings remain
useful; new-binary execution is not qualified by those runs. Codex's repaired
token answer passed five signals; Claude's additional logo advice exposed the
second example correction described above.

The final two-family follow-up uses `4f0159022` instructions, fresh fixtures
and exactly one candidate binary location per envelope (release SHA-256
`bdeed6923982ab0c6d6d4002c448804e48b0ed221cb02714fb3e8efda374078e`).
Both actual answers use the correct token shape and document-relative PNG
import, preserve managed ownership and disclose the absence of rendered proof.
Codex independently passes Claude's five required signals. Claude's answer
still has narrower reporting defects: its validator command needs repository-root
execution, transfer is not literally permanent because reviewed Git restoration
is possible, and tracked-file listing alone does not prove no untracked public
directory exists. The source already describes the correct paths and restoration;
these isolated reasoning limits are retained, not treated as a further source
defect or a reason to rerun until perfect. The final Codex answer is included in
the integrated Claude review packet; its independent grade is recorded at closeout.

The optional Claude read observer emitted capture errors and was removed from
later launches without changing the core native lifecycle. A complete command
trace is therefore not claimed. Some subjects read candidate source outside the
frozen fixture; those reads are disclosed. Public tool outputs were bounded,
and a later supported public re-read resolved one omitted version string but
did not establish the missing validator invocations. No private session stores
were inspected. All original subjects and independent grading sessions were
closed, with task-owned fixture and process checks retained.

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

Native Opus/high subsequently approved ADR-0059's practical transport plan,
the exact CodeFlow and Agent OS source changes, and the final CodeFlow wording
delta (diff SHA-256 `37dfd084a9083b2dddd4849836b6cb3e46d965adc1628e8b615106532d5f363a`).
That review independently ran the 48-test evaluation kit and documentation
validation. It did not claim a new Rust or browser execution. The optional
fallback cannot bypass a safety rejection; qualification remains specific to
the native client, version, direction and launching host. Product diagnostics
may run under independently hosted native subjects without pretending each
subject has qualified a nested delegation route.

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

The later `48457417f` release candidate built successfully and is 17,947,776
bytes, SHA-256
`642cfbb52609396af00aa8492df187a0dc28df2f9a7db030e1a89209f24a10f2`.
This is the binary used for the fresh product trials; it does not replace or
relabel the earlier isolated comparison.

## Retained failures and remaining evidence

Failed preparation runs, the real hard-coded generator-version defect, the
keyboard navigation helper defect and failed initializer cleanup are retained
alongside their reviewed repairs. The original release comparison was invalid
because it reused the wrong release fingerprints; it supplies no size claim.
Fourteen leaked test trees were preserved with matching before/after inventories,
not silently deleted or excluded from asset checks.

Earlier native diagnostics have three distinct dispositions; they must not be collapsed
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

The later completed cohort is reported separately above; it does not erase
these earlier failures. No universal score, model promotion or predictive
calibration is claimed. Final combined integration still requires its own
exact-head review, rather than treating individual source approvals as that review.

Detailed producer receipts are retained in the task worktree's `.state/`
(`tsk-020-browser-qualification-run5`, `tsk-020-full-strict-v2.log`, affected
rerun logs and `tsk-020-release-comparison-run3`). Native returns and the
prospective pilot are retained in the external session evidence collection.
These local evidence locations are not distributed portal runtime dependencies.
