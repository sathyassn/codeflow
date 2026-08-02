# TSK-009 documentation portal verification

This record binds the corrected portal implementation to exact, reproducible
evidence. The implementation candidate is
`d53fcb8ebbf2362b4222047f28c37a403b717705`; its integration base is
`afd0898e2b24c97b06814b3a3194c6799068c896`. Generated portal output and
browser evidence are disposable task-owned artifacts and were not published.

## Deterministic qualification

- `codeflow test --mode full --strict` passed all eight configured targets:
  formatting, workspace tests, Clippy, rustdoc, the 90% coverage floor,
  gate-parity, model-eval-kit, and the real documentation-portal build and
  validator. Aggregate line coverage was 90.03%; no threshold or exclusion was
  changed.
- The final portal producer suite passed 76/76 tests, the Rust portal suite
  passed 50/50, and manifest consistency passed 13/13. The exact-pinned
  Node 24.18.0 toolchain built 96 routes from 90 authoritative source pages;
  Rust accepted all 90 pages and 318 bounded build artifacts. Documentation
  validation accepted 29 records.
- `cargo cross-check-windows` passed for `x86_64-pc-windows-msvc`, and
  `cargo cross-check-linux` passed for the pinned GNU 2.17 zigbuild target.
  These are cross-target compile checks, not native runtime claims. Native
  Windows and Linux execution remains part of TSK-007 release qualification;
  WSL2, when used, is Linux evidence.
- `cargo audit` found no advisory among 233 locked Rust dependencies. The
  locked npm audit found zero vulnerabilities. Gitleaks scanned the exact
  52-commit range from the integration base through the candidate, about
  3.83 MiB, without a finding.
- The starter contains 557,655 tracked bytes and produces a 124,157-byte
  deterministic gzip archive-equivalent. The macOS arm64 release binary is
  17,387,072 bytes against the 16,475,216-byte integration baseline: a
  911,856-byte increase. All three measurements remain within ADR-0048's
  2 MiB, 1 MiB, and 1 MiB limits respectively. A non-adopter receives no
  portal workspace, lockfile, or runtime dependency.

## Browser and accessibility qualification

Run `tsk009-d53fcb8` is commit-bound to the exact candidate. Isolated headless
Chromium, Firefox, and WebKit each passed the same 16 checks: semantic
landmarks and accessible names, axe WCAG 2.2 AA, screen-reader structure,
layout, search, mode persistence before paint, the Orient -> System -> Records
journey, deep links, strict-ID hover/keyboard/touch/escape navigation, source
links, keyboard focus, both utility themes in light and dark modes, minimum
target size, responsive behavior, console cleanliness, and network isolation.
The evidence manifest hashes every retained screenshot and trace and records
`teardown_verified: true`; the task-owned loopback service was no longer
listening after the run. This is structural and rendered browser evidence, not
a claim that a separate assistive-technology session or perceptual pixel
baseline ran.

## Authority, recovery, and code-quality findings

Exact review corrected material gaps rather than accepting the first green
candidate. The final implementation now:

- reads configured source, runtime, and public inputs from one clean committed
  Git snapshot; rejects masked, untracked, symlinked, remote, changing, or
  out-of-bound inputs; and keeps semantic route identity aligned between the
  Node producer and Rust verifier;
- publishes transactionally, preserves bounded user-owned files, refuses
  unsafe output roots, and prevents corrupt, swapped, symlinked, or otherwise
  unverifiable recovery claims from regaining authority;
- serializes recovery/lease transitions without resurrecting a retired owner,
  preserves valid replacement ownership, bounds non-authoritative quarantine
  diagnostics, and proves cleanup across interleavings and crash boundaries;
- bounds titles, paths, reads, child processes, artifacts, and evidence, while
  neutralizing base paths, rendered content, and repository links at their
  trust boundaries; and
- keeps the opt-in Node toolchain, portal components, and utility themes
  isolated from consuming-product and cf-present runtime/design authority.

## Independent review state

- Codex independently inspected and tested exact implementation SHA
  `d53fcb8ebbf2362b4222047f28c37a403b717705` after the recovery corrections
  and approved it with no material findings. Its focused recovery suite passed
  11/11 alongside formatting, Clippy, mirror parity, diff, and clean-worktree
  checks.
- A native interactive Fable 5/high review in Claude Code session
  `0314a6cb-be4d-4ed8-8c05-6b6172e09501` independently rechecked the exact
  candidate, reran the 76 producer, 50 Rust portal, and three CLI tests, and
  reported positive design, doctrine, security, and evidence findings. It
  stopped before four Claude-side specialists returned, so it did **not** issue
  the required terminal verdict and is not recorded as approval.
- Fresh schema-v2 retries were correctly correlated to native sessions
  `22d1bb34-bce6-4152-af95-e34b3e19c0a4` and
  `72eaf197-37ec-472c-923c-7703949b6671`, but the service returned explicit
  `rate_limit` and `oauth_org_not_allowed` failures before review. No fallback
  model was silently substituted.

TSK-009 therefore remains open only for a successful exact-candidate Fable
verdict and metadata closeout. The green implementation may proceed onto the
EPC-005 integration branch so dependency-aware work can continue, but TSK-010
release approval remains withheld until that cross-lineage gate is satisfied.
