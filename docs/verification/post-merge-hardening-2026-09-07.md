# Post-merge hardening work and evidence

Status: implementation and verification in progress. This record does not certify
native model behavior or visual quality before the corresponding trials finish.

Base: CodeFlow `cf0f124c544eb13a08b71483aa33a2b9f03e3b8b`; companion Agent OS
alignment starts at `919bdbae410b7225d7470ed008236a4eff9743f2`.

## Required outcomes

- Isolate authored presentation CSS from review controls without breaking semantic
  text, element, or area annotations. Exercise hostile selectors in a real browser.
- Anchor selected text to the actual occurrence, including repeated text, nested
  markup, whitespace normalization, and UTF-16 offsets; fail closed on mismatch.
- Make the advertised utility font choices real, locally available fonts in both
  the presentation runtime and documentation portal, including offline export.
- Preserve purposeful explanatory motion and reduced-motion accessibility; do not
  constrain motion to decoration or impose product-design choices on utilities.
- Exercise presentation creation, annotations, submission, retrieval, and native
  harness consumption. Distinguish portable feedback retrieval from automatic
  insertion into a chat UI; do not claim unsupported integrations.
- Build and browser-test the documentation portal and its reusable starter.
- Reconcile Agent OS permissions, transport selection, and effort guidance without
  relaxing human authority, destructive-action, or secrets boundaries.
- Align PR guidance with complete branch changes, attributable verification,
  independent review, honest missing evidence, and safe git closeout. Executable
  files under documentation paths must not bypass testing requirements.
- Verify medium-default orchestration with deliberate strongest-qualified
  same-family high/xhigh delegation for demanding architecture, technical planning,
  design, and other complex work. A direct xhigh trigger need not first fail at
  high. Preserve cross-family review and avoid wasteful routine escalation.
- Add adversarial behavioral cases for these decisions, run applicable native
  trials, and report unobserved behavior separately from deterministic contracts.
- Synchronize canonical assets, installed mirrors, baselines, and manifests; run
  proportionate full checks and independent Claude review before final PRs.

## Delivery boundary

Use isolated fix branches. No main merges, visibility changes, or public release
are part of this work. Historical evidence remains historical; new results must
identify their tested revision and limitations rather than rewriting old claims.

## Local evidence

Implementation commits: `08a610962`, `2e989dc1d`, and `54b1f6f97`.
Checks ran on macOS arm64; these results do not qualify Linux or Windows runtime
behavior or a new model binding.

- Rust workspace tests and coverage passed after the documentation-contract and
  binary-portal update fixes: 90.26% line coverage (53,775 lines, 5,240 missed).
  This is Rust coverage, not browser-code coverage. Formatting, workspace Clippy
  with warnings denied, rustdoc with warnings denied, and dependency audit passed.
- Portal checks passed 89 tests. The build produced 102 routes from 96 source
  pages in four layers. All 19 browser checks passed in each of Chromium, Firefox,
  and WebKit at `2e989dc1d`, including actual local font loading, search,
  navigation, responsive behavior, themes, and accessibility. Desktop and mobile
  screenshots were inspected. The later SVG excerpt correction does not change
  portal sources.
- Presentation browser regressions cover repeated-text offsets, nested SVG text,
  genuine repeated labels, CSS containment, and complete document excerpts. The
  final end-to-end run after the SVG correction passed creation, annotations,
  submission, CLI feedback retrieval, and isolated teardown; evidence is retained
  locally under `/tmp/cf-post-merge-browser-v5`. Earlier cleanup fault-injection
  checks also passed. Synthetic comments test the transport, not human approval.
- Suite validation and deterministic orchestration/review contracts passed.
  The new effort scenarios test demanding planning, direct xhigh escalation, and
  routine restraint. Agent OS additionally tests unavailable demanding routes.

## Remaining qualification

Fresh native Claude review and model-subject behavioral trials have not run.
The installed official Codex plugin has a sibling Stop hook; the required
`cf-delegate` preflight needs operator confirmation that its effective
`stopReviewGate` is off through the plugin surface. This is a lifecycle-safety
precondition, not an authentication, usage-credit, or consultation-approval issue.
No plugin-private state was inspected or guard bypassed.

Update, September 8: at the operator's request to check the plugin, its public
`codex-companion.mjs setup --json` command reported `reviewGateEnabled: false`
and `actionsTaken: []`. No setting was changed. The interactive Fable run
`hardening-review-0908` then reached ready and exact-prompt acceptance, but
returned native `StopFailure` / `rate_limit` (HTTP 429). The authorized Opus
fallback is reviewing the same scope in a fresh native session
`5835a785-c743-4654-b592-a611b2d4a911`; its diagnostic terminal identifies
Opus 5 at medium. Review results remain pending until the lifecycle returns.

Portable CLI feedback retrieval is verified. Automatic insertion into each
harness chat UI and native Hermes consumption are not established by that test.
Do not promote these results to universal harness compatibility or behavioral
qualification. The full aggregate result is recorded separately when complete.

## Aggregate result and retry

The full strict aggregate passed 11 of 12 targets. Presentation qualification
failed on Chromium `ERR_NETWORK_IO_SUSPENDED` while polling the local service;
the network failure was not suppressed. The exact configured presentation target
was rerun unchanged at `54b1f6f97` and passed, including the live browser journey
and both cleanup fault-injection checks. Thus every configured target has a
passing local result, but the initial aggregate invocation did not pass as one
uninterrupted run.

The retry also verified the release binary's repeatable hash and its unchanged
size budgets: service delta 1,190,688 bytes against 1,250,000; combined service
and export delta 2,412,880 against 2,600,000. The final web asset tree is
`7c173f857a857699b4879f89543f46e237f1b2519825be6206515ff4272855d2`.

## Native effort diagnostic: failed dispatch boundary

One fresh Opus/medium Claude Code trial of
`complex-planning-delegates-qualified-reasoning` completed through schema-v2
session `437780a8-46ef-4ba0-b011-ef8ffc00768a`, prompt
`c0935fef-e0ab-427c-8e94-139242b76fee`. Fixture digest:
`sha256:75410d397476d81df7facc19356521d9f34f8e5a19acb4af4d42dba048e58d58`.
The subject received only `Read TASK.md and complete the task.`; graders and
expected signals were excluded by the materializer. The retained native result
is `/tmp/cf-effort-planning-0908/turns/trial-1/result.json`.

The trial correctly selected stronger reasoning for cross-cutting architecture
and security, rejected a cheaper collection worker as a substitute, and left
the plan unapproved. Nevertheless, its concrete proposed Codex command used
`--effort xhigh` and `ROLE: worker` directly from Claude. That bypasses the
receiving default-effort primary and its internal routing ownership. Treat this
as a failed behavioral result, not a pass based on surrounding medium-primary
prose. The case asks for assignments and prompts, so it proves neither actual
worker launch nor observed worker effort. Correcting symmetric family-entry
guidance and the graders, then running fresh trials, is required before
claiming this boundary works.
