# CodeFlow release checklist

## Concept

**This checklist is the approval record for every CodeFlow release.**

Tick a box only when its evidence already exists, and record a link or pasted
output for it. Work through the five sections in order. A named human makes the
release decision; a green job, model agreement or peer approval is evidence for
a box and never a substitute for that decision. Commands, rationale and policy
live in [the release runbook](releasing.md).

## Architecture

Each section is bound to the runbook section that owns its procedure.

| Section | What it must establish | Runbook |
|---|---|---|
| 1. Scope and version | One cumulative target that the notes, stamps, tag and proposed merge tree all agree on, with labelled pending entries, notes read before the tag and the ceremony report recorded | [codeflow's own releases](releasing.md#codeflows-own-releases), [pending entries, local checks and repairs](releasing.md#pending-entries-local-checks-and-repairs), [ceremony check before a release](releasing.md#ceremony-check-before-a-release) |
| 2. Source and security gates | The code gates pass from a clean checkout: format, workspace tests, clippy, rustdoc, the full CodeFlow test mode, documentation validation and the coverage floor. Scanning, audit, policy, integrity and provider-surface findings either pass or carry a documented human-approved disposition that never weakens a non-relaxable floor | [same-PR preparation and deliberate publication](releasing.md#same-pr-preparation-and-deliberate-publication), [re-verification before tagging](releasing.md#re-verification-before-tagging) |
| 3. Distribution and platform assurance | Every claimed archive, installer and platform has native evidence, and the publication guards fail closed | [cross-build toolchain](releasing.md#cross-build-toolchain), [presentation renderer assets](releasing.md#presentation-renderer-assets), [portal ownership migration](releasing.md#portal-ownership-migration), [when publication stops](releasing.md#when-publication-stops) |
| 4. Harness and model qualification | The bindings and both interactive lanes are qualified for this release, with routing and review evidence per task | [re-verification before tagging](releasing.md#re-verification-before-tagging) |
| 5. Publish, canary and rollback | A human merges, a human dispatches, the public artifacts agree, and a rollback path is named before the announcement | [same-PR preparation and deliberate publication](releasing.md#same-pr-preparation-and-deliberate-publication), [public version baseline](releasing.md#public-version-baseline) |

## Technical

Each box says what must be true and what counts as evidence. Longer
explanations sit in the detail table after each section, keyed by box number.

### 1. Scope and version

Runbook: [codeflow's own releases](releasing.md#codeflows-own-releases), [pending entries, local checks and repairs](releasing.md#pending-entries-local-checks-and-repairs). Details: [scope and version details](#scope-and-version-details).

- [ ] 1.1 The normal work PR holds reviewed pending notes, one labelled entry with an adjacent impact annotation per item, and every warranted coupled stamp change. Evidence: the PR link and `release.py check-pr` output.
- [ ] 1.2 The cumulative version is the verified public baseline bumped once by the highest remaining pending impact, with breaking changes and migrations explicit. Evidence: `release.py sync` output.
- [ ] 1.3 `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, the release notes and the proposed `vX.Y.Z` tag agree. Evidence: `release.py check-pr` output.
- [ ] 1.4 PR validation used the current target and the actual proposed merge tree, rechecked just before the human merge. Evidence: the fresh PR CI run.
- [ ] 1.5 Every shipped behavior change links its capability or epic and its accepted architecture decision record (ADR), and the docs describe current behavior. Evidence: those links.
- [ ] 1.6 The notes rendered from the final assembled source were read twice before the tag, as a new user and as a user upgrading from the last release. Evidence: both reads and their fixes.
- [ ] 1.7 No published section changed; any correction is a dated `## Errata` entry. Evidence: the `CHANGELOG.md` diff.
- [ ] 1.8 The ceremony report over the release's window is pasted with a one-line comparison against the recorded baseline. Evidence: the report output.

#### Scope and version details

| Box | Detail |
|---|---|
| 1.1 | Every label is unique, and a removed or lowered entry carries its `Withdrawal` |
| 1.2 | Conventional markers do not understate the version |
| 1.4 | A clean result is not a fresh one, so recheck immediately before the human merge |
| 1.5 | Documentation describes current behavior, never an aspiration |
| 1.6 | The new-user read checks what the release does; the upgrade read checks what to do, in order. Rendering: step 3 of [same-PR preparation and deliberate publication](releasing.md#same-pr-preparation-and-deliberate-publication) |
| 1.8 | The report informs the release decision and never blocks it. Procedure: [ceremony check before a release](releasing.md#ceremony-check-before-a-release) |

### 2. Source and security gates

Runbook: [same-PR preparation and deliberate publication](releasing.md#same-pr-preparation-and-deliberate-publication). Details: [source and security details](#source-and-security-details).

- [ ] 2.1 Format, workspace tests, warning-free clippy and rustdoc, the full CodeFlow test mode, documentation validation and the 90% line coverage floor pass from a clean checkout. Evidence: pasted output.
- [ ] 2.2 Secret scanning, dependency audit, policy validation and the repository integrity and managed-drift checks pass, or carry a documented human-approved disposition that keeps every non-relaxable floor. Evidence: output or disposition.
- [ ] 2.3 A fresh authenticated mirror of every fetchable ref passes raw and redacted Gitleaks scans with no broad allowlist. Evidence: the ref digest and both scan outputs.
- [ ] 2.4 The operator-approved publication boundary is complete before visibility changes. Evidence: the boundary record and a rescan of the resulting public refs.
- [ ] 2.5 Hosted PR, issue and review text, releases and assets, Actions logs and artifacts, Pages and packages each have an audited or removed disposition. Evidence: the disposition record.
- [ ] 2.6 Private vulnerability reporting is enabled before `docs/SECURITY.md`'s channel or a public release is advertised. Evidence: **Report a vulnerability** visible to a logged-out visitor or non-maintainer.
- [ ] 2.7 Presentation schemas match the Rust contracts and adversarial fixtures, and the service matrices pass without exposing secrets or private state. Evidence: test output.
- [ ] 2.8 The committed presentation web distribution rebuilds byte-identically and passes its integrity, budget, license, audit and binary-delta limits. Evidence: `npm run supply-chain` and `npm run check` output.
- [ ] 2.9 CodeQL state is honest for the repository's visibility, as the [CodeQL row](releasing.md#re-verification-before-tagging) requires. Evidence: the tool status page.
- [ ] 2.10 Catastrophic-action blocked and allowed canaries pass for macOS, Linux and WSL2, and native Windows command forms, including supported privilege and shell launch wrappers. Evidence: canary output.
- [ ] 2.11 Claude and Codex settings validate in their current native harnesses, and their secret-store denies, research access, prompts and sandbox match the documented contract. Evidence: validation output.

#### Source and security details

| Box | Detail |
|---|---|
| 2.3 | The mirror inventories every remote branch and tag, hosted pull-request head and merge ref, and any other fetchable or servable ref. Record its ref digest, inventory forbidden runtime paths, run both raw and configured redacted Gitleaks scans, and reject broad path or directory allowlists that could hide a future secret |
| 2.4 | For a sanitized public repository, keep the original remote as a sealed private archive and publish only selected clean refs, never a mirror push. For a history rewrite, separately purge retained hosted PR refs and caches. Rescan the resulting public refs and regenerated exact fingerprints instead of reusing pre-rewrite commit IDs |
| 2.5 | An ordinary branch scan does not prove these provider-owned surfaces safe |
| 2.6 | Open the repository's Security tab while logged out or as a non-maintainer. An API 404 leaves the feature state unverified; it is evidence of neither enabled nor disabled. `docs/SECURITY.md` never points reporters at a channel the repository has not enabled |
| 2.7 | Document, token and history schemas match. Service request, auth, bootstrap, sandbox and export matrices, crash recovery, concurrent feedback, retention and identity-scoped cleanup pass |
| 2.8 | The rebuild uses the exact lockfile and toolchain. The checks cover integrity hashes, Brotli and export budgets, the license inventory, the CycloneDX software bill of materials (SBOM), the package audit and release binary-delta limits. Consumer builds still need no Node toolchain. Commands: [presentation renderer assets](releasing.md#presentation-renderer-assets) |
| 2.11 | The checks cover secret-store denies, public research access, permission prompts and the host-specific sandbox boundary |

### 3. Distribution and platform assurance

Runbook: [cross-build toolchain](releasing.md#cross-build-toolchain), [presentation renderer assets](releasing.md#presentation-renderer-assets), [portal ownership migration](releasing.md#portal-ownership-migration). Details: [distribution and platform details](#distribution-and-platform-details).

- [ ] 3.1 The pinned cargo-dist version regenerates the committed release workflow without drift. Evidence: an empty regeneration diff.
- [ ] 3.2 The dispatched authority job enforces every check in the runbook's [Dispatch and authority rows](releasing.md#architecture) and fails closed. Evidence: the authority job log.
- [ ] 3.3 Generated host dependencies reject a failed or cancelled authority or main-recheck job, so publishing cannot skip either. Evidence: the generated workflow and a run log.
- [ ] 3.4 The post-announce verifier binds the public tag and release to the selected main source and matches every asset exactly. Evidence: the verifier log.
- [ ] 3.5 A stopped attempt is retried or recovered only as [when publication stops](releasing.md#when-publication-stops) allows. Evidence: the attempt record.
- [ ] 3.6 `cargo dist plan --output-format=json` lists both macOS archives, the Linux x86-64 archive, the Windows x86-64 MSVC archive and both installers on native runners. Evidence: the pasted plan.
- [ ] 3.7 Optional `cargo-xwin` and `cargo-zigbuild` target checks pass. Evidence: their output, with tool versions and host recorded.
- [ ] 3.8 Native macOS, Linux and Windows build and test canaries pass. Evidence: OS, architecture, Rust version and exact command, with WSL2 recorded as Linux.
- [ ] 3.9 Each claimed presentation platform passes the isolated browser review journey and teardown, and an unqualified adapter fails closed. Evidence: journey output per platform.
- [ ] 3.10 Presentation platform evidence covers every [native case](releasing.md#presentation-renderer-assets) for Windows, Linux and WSL2, and macOS. Evidence: native run output per platform.
- [ ] 3.11 Browser evidence covers long-task behavior, including budget exhaustion. Evidence: browser run output.
- [ ] 3.12 Presentation adversarial evidence covers every case in the adversarial table below. Evidence: test output per case.
- [ ] 3.13 Each installer selects the right artifact, and each installed binary reports the release version and passes `codeflow doctor` in a disposable greenfield repo. Evidence: installer canary output.
- [ ] 3.14 A brownfield update canary preserves user-owned files and intentional sidecars, reports conflicts, and is idempotent when repeated. Evidence: output of two runs.
- [ ] 3.15 Portal size limits hold and the non-adopter, adopter and CodeFlow portal checks pass. Evidence: size measurements and test output.

#### Distribution and platform details

| Box | Detail |
|---|---|
| 3.2 | The latest exact-source main-push results come from `codeflow-ci` and `codeflow-release`; the release-state check runs in `codeflow-release` |
| 3.3 | A dry run creates no draft and performs no hosting. Recheck timing and the one-publication-at-a-time rule are in [the Architecture bullets](releasing.md#architecture) |
| 3.4 | Every asset name, size and SHA-256 digest matches the same-run staged artifact set, with no missing, duplicate or extra asset |
| 3.7 | These are static-analysis, compile and link evidence only ([cross-build toolchain](releasing.md#cross-build-toolchain)) |
| 3.9 | The platform opens only a task-owned isolated browser and profile, passes the qualified Brotli and full review journey, preserves light, dark and system themes and accessibility behavior, exports offline, and proves close, crash and retention teardown. `--no-launch` remains usable |
| 3.11 | Budget exhaustion leaves escaped source and does not block feedback, export, close or cleanup |
| 3.13 | The shell installer selects the correct macOS or Linux artifact, the PowerShell installer selects `codeflow.exe` on native Windows, and WSL2 selects the Linux artifact |
| 3.15 | Portal starter bytes, archive-equivalent bytes and release-binary delta stay within ADR-0048. A non-adopter receives no portal workspace, lockfile or baseline. An adopter passes setup, update, conflict and idempotence; locked install, build, audit and upgrade; `validate --portal`; source and manifest negative fixtures; and Chromium, Firefox and WebKit accessibility journeys. CodeFlow itself runs the `docs-portal` target through `codeflow test --mode full --strict` locally and on Ubuntu, selected when its inputs change and always under `--all` at epic close, plus the authority and path suite on Windows. Generic consumer CI stays portal-free until adoption. These checks publish no generated output |

Presentation adversarial cases for box 3.12:

| Case | What the evidence shows |
|---|---|
| Native paths | Invalid native-path bytes are handled |
| Collection amplification | Per-block and aggregate amplification are bounded |
| Project quotas | Impossible and concurrent quotas across create, update, runtime and feedback mutations; zero-growth retries and cleanup in a project already over its quota |
| Feedback transitions | Malformed, duplicate, post-terminal and concurrent transitions |
| Feedback re-anchoring | Exact, ambiguous and missing re-anchoring |
| Resolution | Stale and cross-session resolution, and concurrent identical and conflicting terminal retries |
| Review limits | Client and server review-limit parity |
| Export | Relative and Unicode owner-private export creation |
| Runtime separation | Derived-runtime separation during live browser-profile writes |
| Create and trash recovery | Exact-name create and trash recovery with matching transaction proof |
| Eviction | Size recomputation across multiple evictions |
| Cleanup | Selected cleanup isolation with an actionable retained-session result, and structured bulk partial failure |
| Crash convergence | A service crash, then close, then clear, converge |
| Leader loss | Cleanup through one serialized, consumed record per launch attempt |
| PID reuse | The reused PID is never signalled, and state is retained unless bounded exact marker, process and native resource absence is proven |
| Windows resources | Resource proof walks real profile handles without assuming a POSIX lock file |
| Forced cleanup | Qualified forced cleanup re-proves the exact identity after its graceful-stop window |

### 4. Harness and model qualification

Runbook: [re-verification before tagging](releasing.md#re-verification-before-tagging). Details: [harness and model details](#harness-and-model-details).

- [ ] 4.1 The host-neutral Claude and Codex contract and evaluator fixtures pass with the currently supported model and harness bindings. Evidence: fixture test output.
- [ ] 4.2 A material orchestration, task-graph or verification-selection change updates its requirements, cases, fixtures, packs and mirrors. Evidence: the PR and retained native canaries.
- [ ] 4.3 A material design-contract change exercises the required design cases. Evidence: retained same-environment artifacts and blinded paired judgments.
- [ ] 4.4 Whole-flow and concurrent-browser canaries cover every applicable changed boundary with isolated resources and proven teardown. Evidence: the canary records.
- [ ] 4.5 The mixed closeout canary removes only the clean proven-landed worktree and retains every other entry. Evidence: canary output with ownership and recheck evidence.
- [ ] 4.6 `codeflow status` classifies linked worktrees and unattached local branches as removable, preserve-dirty or retain-unproven without mutating anything. Evidence: its test output.
- [ ] 4.7 The ensemble selectors, effort and worker policy and catalog entries match the [ensemble row](releasing.md#re-verification-before-tagging). Evidence: the ensemble record and native results.
- [ ] 4.8 `codeflow doctor --check model-bindings` passes for each retained local promotion record, or records the native canary still needed. Evidence: doctor output.
- [ ] 4.9 Both native interactive directions complete a scoped tool and Model Context Protocol (MCP) canary. Evidence: versions, effort, tool access and graceful degradation.
- [ ] 4.10 Each task records its producer and cross-lineage reviewer, and every blocking finding is resolved or stops the release. Evidence: verified routing evidence per task.
- [ ] 4.11 Host, peer and worker role canaries reject nested orchestration, observe usage state, and record a reassignment where the assignment lives, reviewed by one other-lineage seat and never carrying an old approval (ADR-0076). Evidence: canary output.

#### Harness and model details

| Box | Detail |
|---|---|
| 4.2 | The change updates its stable requirements, paired positive and non-ceremony cases, fixtures, packs and managed mirrors. Applicable new behavioral cases have retained native interactive Claude and Codex canary evidence. Deterministic corpus validation alone is never reported as model behavior |
| 4.3 | The cases are proportional routing, operator-direction precedence, counterfactual evidence-grounded choice review, accessibility and fidelity. Rendered comparisons retain same-environment artifacts and blinded paired judgments. Taste or category familiarity alone is never reported as a defect |
| 4.4 | The canaries exercise an affected journey through every applicable changed boundary, disclose controlled external seams, allocate an isolated browser, endpoints, data and artifacts per task, and prove teardown. A listening MCP port is required only for a listening transport. Headed evidence uses a test-owned browser and session, never the operator's existing browser or desktop |
| 4.5 | The canary inventories active, proven-landed, dirty and unproven worktrees. Every retained entry carries ownership and recheck evidence. Age and `git worktree prune` metadata never stand in for merge proof |
| 4.6 | Classification uses locally known ancestry and patch evidence. Tests cover squash-equivalent, dirty and unlanded states. The report does not replace the active-owner check |
| 4.10 | Producers verify first, the other lineage reviews each unit independently, and the selected Claude judgment primary owns the integrated design and code judgment |
| 4.11 | Usage state is observed, never inferred |

### 5. Publish, canary and rollback

Runbook: [same-PR preparation and deliberate publication](releasing.md#same-pr-preparation-and-deliberate-publication), [public version baseline](releasing.md#public-version-baseline). Details: [publish and rollback details](#publish-and-rollback-details).

- [ ] 5.1 A human approves and merges the normal work PR after its release-state check is fresh, and no agent merges or tags it. Evidence: the merged PR and fresh check.
- [ ] 5.2 A human dispatches the generated workflow for current `main` and the pending `vX.Y.Z`, and cargo-dist tags only after the guarded build succeeds. Evidence: the run link.
- [ ] 5.3 Archives, installers, checksums and attestations from the pinned workflow, and the release notes, are complete and consistent before announcement. Evidence: the release page.
- [ ] 5.4 Fresh public-network install canaries pass without private credentials or repository access. Evidence: canary output.
- [ ] 5.5 Rollback is ready, with the prior release still installable, the bad release withdrawable without rewriting tags, and a corrective release owner named. Evidence: the rollback record.
- [ ] 5.6 Downstream Agent OS work starts only from this verified public release, and the Agent OS portal updates only after the matching Agent OS release. Evidence: the release link.

#### Publish and rollback details

| Box | Detail |
|---|---|
| 5.5 | The bad release can be marked or withdrawn without rewriting tag history. For CodeQL, rollback removes any required check before disabling default setup, and the findings and last healthy tool status stay linked in the release record |
| 5.6 | Agent OS is the companion repository that consumes CodeFlow |
