# TSK-009 documentation portal verification

> Status: in progress. All evidence and review claims below are historical,
> provisional records from superseded candidates. Material integration findings
> invalidated promotion of those candidates. Nothing below is a current pass or
> approval claim; the corrected implementation SHA will replace this record
> after fresh deterministic, browser, security, platform, size, and independent
> Claude/Codex reviews.

This record binds the portal task to observable repository, build, browser,
security, and size evidence. Generated portal output is disposable and was not
published.

## EPC-005 integration requalification — 2026-08-02

The portal task was rebased onto exact integration base
`afd0898e2b24c97b06814b3a3194c6799068c896`, after TSK-011. Shared CLI module
registration, manifests, capability records, and evaluation registries were
resolved as semantic unions: neither `cf-present` nor `cf-docs-portal` lost an
asset, route, case, requirement, or release-smoke obligation. The composed
release pack retains its incumbent orchestration-first order and appends the
portal diagnostic rather than silently changing the release entry point.

The combined workspace exposed two real integration gaps. A public CLI suite
now proves opt-in portal adoption, repeat reconciliation, project-owned config
preservation through `update`, unsafe/uninitialized rejection, and read-only
validation that does not execute project scripts. A complete one-page public
validator fixture proves the adoption → authoritative Git config/source →
generated Markdown/twin/`llms.txt` → Pagefind-marked artifact chain, then
rejects bounded tampering with identity, provenance, route, state, output, and
artifact claims. These are claim-matched acceptance cases, not production
branches or copied test oracles. Combined line coverage is 90.03%; no threshold
or exclusion changed.

Local integrated evidence:

- the eight-target full strict gate passed after formatting, workspace tests,
  Clippy and rustdoc with warnings denied, 90.03% coverage, gate parity, model
  evaluation, and the real portal build/validator;
- 51/51 portal adapter tests passed under pinned Node 24.18.0; Astro checked
  the project, Starlight built 96 routes from 90 source pages, Pagefind indexed
  them, and Rust accepted 90 pages plus 318 built artifacts;
- `cargo cross-check-windows` passed for `x86_64-pc-windows-msvc`, and
  `cargo cross-check-linux` passed for the pinned GNU 2.17 zigbuild target;
  these are cross-target compile checks, not native runtime claims;
- `cargo audit` found no advisory among 233 locked Rust dependencies; npm found
  zero vulnerabilities among 539 dependencies; gitleaks scanned 2,154 commits
  and about 87.65 MB with no leak;
- the integrated macOS arm64 release binary is 17,136,656 bytes versus
  16,475,216 bytes at the exact integration base: a 661,440-byte portal delta,
  below the 1 MiB limit. The starter is 459,722 unpacked bytes and 103,376
  deterministic gzip archive-equivalent bytes, below its 2 MiB/1 MiB limits.

The post-rebase browser rerun retained isolated headless Chromium, Firefox, and
WebKit contexts. Axe found a genuine light-mode Starlight sidebar-label contrast
regression; the shared light token was narrowed from `#74817e` to `#62716e`
without altering product authority or dark mode. All three engines then passed
the full journey, search, mode/theme persistence, keyboard preview, responsive,
console/network, and axe checks. Lighthouse on `/system/` scored 100 in
performance, accessibility, best practices, SEO, and agentic browsing. The
task-owned loopback server and browser resources were stopped after use.

## Original isolated-task qualification

- Local host: macOS 26.5.2 arm64; Rust 1.94.0.
- Portal runtime: the pinned Node 24.18.0 from `.node-version`.
- Repository test gate: `codeflow test --mode full --strict` passed all eight
  configured targets: format, workspace tests, clippy, rustdoc, coverage,
  gate-parity, model-eval kit, and docs portal.
- Rust: 60 CLI and 1,490 core unit tests passed, together with the integration,
  manifest, scaffold, hook, delegate, orchestration, release, and tier suites.
  `cargo llvm-cov` enforced the configured 90% line floor.
- Portal producer: 51/51 JavaScript tests passed. They include a real
  Starlight build for mixed-case, space, `+`, and NFC Unicode routes; strict
  frontmatter/config parity; SHA-1/SHA-256 Git object IDs; source/public/runtime
  snapshot authority; bounded Git operations; crash recovery; symlink/race
  rejection; monorepo roots; and stale/error-page behavior.
- Portal output: a locked install reported no npm vulnerabilities; Starlight
  built 90 routes and Pagefind indexed them; the Rust verifier accepted 84
  source pages and 299 bounded built artifacts. The integrated counts above
  supersede these isolated-branch counts.
- Supply chain: `cargo audit` and `npm audit --audit-level=high` reported no
  known vulnerabilities. The locked npm install reports the existing
  `allow-scripts` review reminders for esbuild and the macOS-only fsevents
  package; it does not silently approve new install scripts.

## Browser and accessibility evidence

An isolated, headless, task-owned server on loopback port 43199 and separate
Playwright contexts exercised Chromium, Firefox, and WebKit. Each engine passed
the Orient → System → Records journey, strict-ID preview keyboard behavior,
search, persisted light/dark mode, both portal utility themes, mobile overflow,
24 px primary targets, request/console cleanliness, and WCAG 2.2 AA axe checks.
The test did not open or reuse the operator's browser profile or window.

Lighthouse then exposed a light-mode journey-count contrast defect that axe's
dark mobile sample did not. The portal token was corrected and the candidate
was rebuilt and revalidated. The focused rerun scored performance 99,
accessibility 100, best practices 100, and SEO 100. This is why browser engines,
axe, and Lighthouse remain complementary rather than interchangeable evidence.

The browser service was stopped after the run. Successful screenshots and
traces remained task-local during review; no failure artifact required durable
retention.

## Authority and portability findings

- Route identity is the exact NFC semantic-layer/source-root-relative text.
  Percent encoding occurs only at URL boundaries; case folding detects
  collisions and never rewrites identity.
- Committed non-reserved public files are read from Git blobs and compared with
  the worktree. Untracked, symlinked, masked, or changing public/runtime/source
  inputs fail closed.
- A shared fixture keeps JavaScript and Rust aligned on repository URLs,
  bounded metadata, empty and invalid relationships, strict IDs, and strict
  frontmatter. The whole-repository run caught the previously missing
  `null`-means-absent relationship case before closeout.
- Native separators are converted component-by-component. POSIX literal
  backslashes and non-UTF-8 paths remain rejected; the same test exercises
  Windows separators on the Windows CI runner.
- Local native Windows and Linux execution was unavailable. Their release and
  test claims therefore remain assigned to the native CI/release matrix rather
  than being inferred from macOS or WSL.

## Footprint

The macOS arm64 release baseline at
`88215cc977039ed405b8875a38ae00cd61f614c5` is 10,473,952 bytes. The task release
binary is 11,161,104 bytes: a 687,152-byte increase, below the 1 MiB limit. The
starter is 459,633 tracked bytes and 103,182 gzip archive-equivalent bytes,
below the 2 MiB and 1 MiB limits. Its mirrored skill is 134 lines, below the
300-line limit. Upstream `rust-embed` compression preserves the offline binary
contract without embedding the starter at unpacked size; its locked dependency
set passed the supply-chain audit. These are the isolated-task measurements;
the integrated measurements above are the current release evidence.

## Multi-model review

Fable/high reviewed the task at material authority and portability checkpoints.
The first review rejected worktree-derived config authority, incomplete public
boundary tests, and red clippy state. The second rejected a native-Windows path
normalization failure. The third passed the corrected exact candidate. A final
exact-SHA review covers the completed task record, shared authority additions,
compressed embedding, repository gate ownership, and accessibility correction.
The post-TSK-011 semantic reconciliation received a fresh exact-candidate Fable
5/high review and an independent Codex review after the integrated deterministic
and rendered gates. Their verdicts apply only to that exact candidate; a later
material conflict resolution requires focused requalification and new reviews.
