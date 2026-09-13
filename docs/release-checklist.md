# CodeFlow release checklist

Use this checklist for every CodeFlow release. The detailed commands and
rationale live in [the release runbook](releasing.md); this page is the compact
approval record. Record links or pasted output for every checked item. A green
job, model agreement, or peer approval is evidence, never a substitute for the
named human release decision.

## 1. Scope and version

- [ ] The generated candidate records the exact current `main` source, contains
      only allowlisted derived changes, and still matches its recorded hashes.
- [ ] Conventional commits resolve to the intended SemVer bump; breaking
      changes and migrations are explicit.
- [ ] `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, release notes, and the proposed
      `vX.Y.Z` tag agree.
- [ ] The human-approved candidate head is the exact second parent of its main
      merge, its recorded source is the first parent, and both trees agree.
- [ ] Every shipped behavior change links its capability/epic and accepted ADR;
      documentation describes current behavior, not an aspiration.

## 2. Source and security gates

- [ ] Format, workspace tests, warning-free clippy, warning-free rustdoc, the
      full CodeFlow test mode, documentation validation, and the 90% line
      coverage floor pass from a clean checkout.
- [ ] Secret scanning, dependency audit, policy validation, and the repository
      integrity/managed-drift checks pass or have a documented, human-approved
      disposition that does not weaken a non-relaxable floor.
- [ ] A fresh authenticated mirror inventories every remote branch and tag,
      hosted pull-request head and merge ref, and any other fetchable or
      servable ref. Record its ref digest; inventory forbidden runtime paths;
      run both raw and configured redacted Gitleaks scans; and reject broad
      path/directory allowlists that could hide a future secret.
- [ ] The operator-approved publication boundary is complete before visibility
      changes. For a sanitized public repository, retain the original remote as
      a sealed private archive and publish only selected clean refs—never a
      mirror push. For a history rewrite, separately purge retained hosted PR
      refs and caches. Rescan resulting public refs and regenerated exact
      fingerprints rather than reusing pre-rewrite commit IDs.
- [ ] Hosted PR/issue/review text, releases and assets, Actions logs/artifacts,
      Pages, and packages have an explicit audited or removed disposition. An
      ordinary branch scan does not prove those provider-owned surfaces safe.
- [ ] Presentation document/token/history schemas match the Rust contracts and
      adversarial fixtures; service request/auth/bootstrap/sandbox/export
      matrices, crash recovery, concurrent feedback, retention, and
      identity-scoped cleanup pass without exposing secrets or private state.
- [ ] The committed presentation web distribution rebuilds byte-identically
      from its exact lockfile and toolchain; integrity hashes, Brotli/export
      budgets, license inventory, CycloneDX SBOM, package audit, and release
      binary-delta limits pass. Consumer builds still require no Node toolchain.
- [ ] CodeFlow's repository-specific CodeQL state is honest. Before public
      launch it remains pending and no CodeQL workflow is shipped in the
      portable scaffold. After public launch, GitHub default setup for Rust uses
      `security-extended`; tool status shows the intended files analyzed with
      zero extraction/configuration errors. Treat it as advisory until five
      consecutive applicable PR runs are healthy, then decide separately
      whether branch protection should require it.
- [ ] Catastrophic-action blocked/allowed canaries pass for macOS, Linux/WSL2,
      and native Windows command forms, including supported privilege and shell
      launch wrappers.
- [ ] Claude and Codex settings validate in their current native harnesses;
      secret-store denies, public research access, permission prompts, and the
      host-specific sandbox boundary match the documented contract.

## 3. Distribution and platform assurance

- [ ] The pinned cargo-dist version regenerates the committed release workflow
      without drift.
- [ ] The dispatched plan authority job revalidates the merged candidate and
      fails closed on a wrong tag, public release, foreign draft, or any draft
      asset before cargo-dist builds. It may create or reuse only the exact empty
      candidate-bound draft with reviewed notes; generated upload/announce does
      not clobber a later host conflict.
- [ ] `cargo dist plan --output-format=json` lists the two macOS archives, the
      Linux x86-64 archive, the Windows x86-64 MSVC archive, and both shell and
      PowerShell installers on native runners.
- [ ] Optional host-agnostic `cargo-xwin` target clippy/build and
      `cargo-zigbuild` checks pass; their versions and host are recorded. These
      are static-analysis/compile/link evidence only.
- [ ] Native macOS, Linux, and Windows build/test canaries pass. Record the OS,
      architecture, Rust version, and exact command. WSL2 is recorded as Linux,
      not as native Windows.
- [ ] Each claimed presentation platform opens only a task-owned isolated
      browser/profile, passes the qualified Brotli and full review journey,
      preserves light/dark/system and accessibility behavior, exports offline,
      and proves close/crash/retention teardown. An unqualified adapter fails
      closed and `--no-launch` remains usable.
- [ ] Presentation platform evidence covers Windows Unicode known-folder and
      profile paths, creation-time ACL hardening, read-only rejection of weak
      owners/DACL inheritance/trustees, trusted system tools, exact quoted
      process identity, file URLs, and process-tree cleanup; Linux/WSL2 bounded
      no-follow `/proc` identity and process-group cleanup; and the equivalent
      macOS ownership checks. Every external child proves the shared restricted
      environment excludes provider-secret environment canaries. Cross-target compilation
      does not replace these native cases.
- [ ] Browser evidence includes the bounded dense multi-diagram corpus, records
      long-task behavior, and proves that budget exhaustion leaves escaped
      source without blocking feedback, export, close, or cleanup.
- [ ] Presentation adversarial evidence covers invalid native-path bytes,
      per-block and aggregate collection amplification, impossible and
      concurrent project quotas across create, update, runtime, and feedback
      mutations; zero-growth retries and cleanup in legacy over-quota state;
      malformed, duplicate, post-terminal, and concurrent feedback transitions;
      exact/ambiguous/missing feedback re-anchoring; stale/cross-session
      resolution and concurrent identical/conflicting terminal retries;
      client/server review-limit parity; relative and Unicode owner-private
      export creation; derived-runtime separation during live browser-profile
      writes; exact-name create/trash recovery with matching transaction proof;
      multi-eviction size recomputation; selected cleanup isolation and an
      actionable retained-session result; structured bulk partial failure;
      service crash → close → clear convergence; and leader-loss cleanup through
      one serialized, consumed record per launch attempt. PID-reuse cases never
      signal the reused PID and retain state unless bounded exact marker/process
      and native resource absence is proven. Windows resource proof walks real
      profile handles without assuming a POSIX lock file. Qualified forced
      cleanup re-proves the exact identity after its graceful-stop window.
- [ ] The shell installer selects the correct macOS/Linux artifact, the
      PowerShell installer selects `codeflow.exe` on native Windows, and WSL2
      selects the Linux artifact. Each installed binary reports the release
      version and passes `codeflow doctor` in a disposable greenfield repo.
- [ ] A brownfield update canary preserves user-owned files and intentional
      sidecars, reports conflicts, and is idempotent when repeated.
- [ ] Portal starter bytes, archive-equivalent bytes, and release-binary delta
      remain within ADR-0048. A non-adopter receives no portal workspace,
      lockfile, or baseline; an adopter passes setup/update/conflict/idempotence,
      locked install/build/audit/upgrade, `validate --portal`, source/manifest
      negative fixtures, and Chromium/Firefox/WebKit accessibility journeys.
      CodeFlow itself runs the `docs-portal` target through
      `codeflow test --mode full --strict` locally and on Ubuntu, plus the
      authority/path suite on Windows. Generic consumer CI remains portal-free
      until adoption. Generated output is not published by these checks.

## 4. Harness and model qualification

- [ ] The host-neutral Claude+Codex contract and evaluator fixtures pass with
      the currently supported model/harness bindings.
- [ ] Any material orchestration, task-graph, or verification-selection change
      updates its stable requirements, paired positive/non-ceremony cases,
      fixtures, packs, and managed mirrors. Applicable new behavioral cases
      have retained native interactive Claude and Codex canary evidence;
      deterministic corpus validation alone is not reported as model behavior.
- [ ] Any material design-contract change exercises proportional routing,
      operator-direction precedence, counterfactual evidence-grounded choice
      review, accessibility, and fidelity cases. Rendered comparisons retain
      same-environment artifacts and blinded paired judgments; taste or category
      familiarity alone is not reported as a defect.
- [ ] Whole-flow and concurrent-browser canaries exercise an affected journey
      through every applicable changed boundary, disclose controlled external
      seams, allocate isolated browser/endpoints/data/artifacts per task, and
      prove teardown. A listening MCP port is required only for a listening
      transport; headed evidence uses a test-owned browser/session and never
      the operator's existing browser or desktop.
- [ ] The mixed closeout canary inventories active, proven-landed, dirty, and
      unproven worktrees; removes only the clean proven-landed entry; and
      retains every other entry with ownership/recheck evidence. Age and
      `git worktree prune` metadata never stand in for merge proof.
- [ ] `codeflow status` classifies linked worktrees and unattached local
      branches as removable, preserve-dirty, or retain-unproven from locally
      known ancestry/patch evidence; tests cover squash-equivalent, dirty, and
      unlanded states. The report performs no mutation and does not replace the
      active-owner check.
- [ ] The current ensemble selectors and effort/worker policy match the models
      actually qualified for this release; every `capability-supported`
      harness catalog entry still satisfies the universal capability contract.
      Catalog support is not binding qualification. Any changed concrete
      binding has an approved full native result, not only a diagnostic pack.
- [ ] `codeflow doctor --check model-bindings` passes for each retained local
      promotion record, or records the exact non-probeable native canary needed;
      requested/observed identity and settings/version drift are resolved.
- [ ] Both native interactive directions complete a scoped tool/MCP canary;
      evidence records versions, effort, tool access, and graceful degradation.
- [ ] Each task records its producer and cross-lineage reviewer with verified
      routing evidence; producers first-verify, the other lineage reviews each
      unit independently, and the selected Claude judgment primary owns the
      integrated design/code judgment. Every blocking finding is resolved or
      explicitly stops the release.
- [ ] Host/peer/worker role canaries reject nested orchestration, usage state is
      observed rather than inferred, and reassignment forces fresh dual approval.

## 5. Publish, canary, and rollback

- [ ] A human approves and merges the release PR. No agent merges or tags it.
- [ ] The annotated release tag points at the reviewed merge commit and starts
      the expected release workflow.
- [ ] Release archives, installers, checksums/attestations emitted by the pinned
      distribution workflow, and release notes are complete and mutually
      consistent before the release is announced.
- [ ] Fresh public-network install canaries pass without private credentials or
      repository access.
- [ ] Rollback is ready: the prior release remains installable, the bad release
      can be marked/withdrawn without rewriting tag history, and corrective
      release ownership is named. For CodeQL, rollback removes any required
      check before disabling default setup; findings and the last healthy tool
      status remain linked in the release record.
- [ ] Downstream Agent OS work begins only from this verified public CodeFlow
      release; the portal is updated only after the matching Agent OS release.
