# TSK-009 documentation portal verification

This record binds the corrected portal implementation to exact, reproducible
evidence. The current cleanup candidate is
`25243aa0e2ebca4a5a955891394793082dab68e5`; its integration base is
`365ad969fa114cb5e7171ae0e008afad205a1cad`. It extends the independently
reviewed portal foundation at `d53fcb8ebbf2362b4222047f28c37a403b717705`.
Generated portal output and browser evidence are disposable task-owned
artifacts and were not published.

## Deterministic qualification

- At the exact cleanup candidate, `codeflow test --mode full --strict` passed
  all nine configured targets: formatting, workspace tests, Clippy, rustdoc,
  the unchanged 90% coverage floor, gate parity, model evals, cf-present
  qualification, and the real documentation-portal build and validator. No
  threshold or exclusion was changed.
- The final portal producer suite passed 81/81 tests, including a real child
  that closes its listener but remains alive until forced termination plus
  deterministic kill-failure and ambiguous-exit cases. The focused Rust portal
  validator passed 25/25, manifest consistency passed 13/13, and documentation
  validation accepted 29 records. The pinned Node 26.4.0 / npm 11.17.0
  toolchain built 96 routes from 90 authoritative source pages; Rust accepted
  all 90 pages and 318 bounded build artifacts.
- The reviewed foundation passed `cargo cross-check-windows` for
  `x86_64-pc-windows-msvc` and `cargo cross-check-linux` for the pinned GNU 2.17
  zigbuild target. Those compile-only lanes were not rerun for this JavaScript/
  workflow-only cleanup and are not native runtime claims. Native Windows and
  Linux execution remains part of TSK-007 release qualification; WSL2, when
  used, is Linux evidence.
- `cargo audit` found no advisory among 233 locked Rust dependencies, and the
  locked npm audit found zero vulnerabilities. Gitleaks scanned the exact two
  cleanup commits from the integration base through the candidate (27,359
  bytes) without a finding.
- The cleanup candidate contains 571,327 tracked starter bytes. A `git archive`
  of the exact starter tree compressed in one bounded Node `gzipSync` call at
  level 9 is 128,210 bytes (SHA-256
  `27c5e5e32f03dc5c42add00614add1185e86b6a4ca3d63a6dfd5b6d26441f4cf`),
  within ADR-0048's 2 MiB and 1 MiB limits. The exact macOS arm64 release build
  is 16,518,448 bytes with SHA-256
  `140f799fde0afbf836d47c3d1d4a5ddadccda1a4a6c9d63d915ea9775a076aea`.
  The same exact source built with only `assets/docs-portal/**` excluded is
  16,361,488 bytes (SHA-256
  `bf9c8170fd25345cc045901aefecbfa807f8eab18ab2a555d599a2a82c8a0581`),
  proving a 156,960-byte portal delta under the 1 MiB cap. The restored
  production build reproduced its exact size and digest. A non-adopter receives
  no portal workspace, lockfile, or runtime dependency.

## Browser and accessibility qualification

Run `tsk009-cleanup-25243aa0` is commit-bound to the exact cleanup candidate.
Isolated headless Chromium, Firefox, and WebKit each passed the same 16
checks: semantic landmarks and accessible names, axe WCAG 2.2 AA,
screen-reader structure,
layout, search, mode persistence before paint, the Orient -> System -> Records
journey, deep links, strict-ID hover/keyboard/touch/escape navigation, source
links, keyboard focus, both utility themes in light and dark modes, minimum
target size, responsive behavior, console cleanliness, and network isolation.
The evidence manifest hashes 18 retained screenshots/traces totaling 16,505,527
bytes and records `teardown_verified: true`; actual preview-process exit was
proven before the task-owned loopback service check. Traces retain bounded
action/network metadata while named screenshots carry the visual evidence, so
browser timelines cannot exceed the evidence envelope by duplicating images.
This is structural and rendered browser evidence, not a claim that a separate
assistive-technology session or perceptual pixel baseline ran.

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
  isolated from consuming-product and cf-present runtime/design authority;
- routes the Windows CI job through the same reviewed, shell-free
  `deps:install` boundary as other hosts; and
- proves actual child exit after bounded graceful/forced termination, rejecting
  kill failure and ambiguous exit events rather than inferring cleanup from a
  closed listener.

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
- The cleanup delta at `25243aa0e2ebca4a5a955891394793082dab68e5`
  still requires an exact independent review and a successful native Fable
  verdict before task completion. Prior review of the foundation is not
  silently carried forward to changed code.

TSK-009 remains in progress for exact-candidate independent/Fable review and
the outstanding native-platform acceptance. The green cleanup implementation
may proceed onto the EPC-005 integration branch so dependency-aware work can
continue, but its open gates must not be relabeled as complete.
