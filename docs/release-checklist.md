# CodeFlow release checklist

Use this checklist for every CodeFlow release. The detailed commands and
rationale live in [the release runbook](releasing.md); this page is the compact
approval record. Record links or pasted output for every checked item. A green
job, model agreement, or peer approval is evidence, never a substitute for the
named human release decision.

## 1. Scope and version

- [ ] The release branch starts at the latest protected `main` and contains
      only the intended release changes.
- [ ] Conventional commits resolve to the intended SemVer bump; breaking
      changes and migrations are explicit.
- [ ] `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, release notes, and the proposed
      `vX.Y.Z` tag agree.
- [ ] Every shipped behavior change links its capability/epic and accepted ADR;
      documentation describes current behavior, not an aspiration.

## 2. Source and security gates

- [ ] Format, workspace tests, warning-free clippy, warning-free rustdoc, the
      full CodeFlow test mode, documentation validation, and the 90% line
      coverage floor pass from a clean checkout.
- [ ] Secret scanning, dependency audit, policy validation, and the repository
      integrity/managed-drift checks pass or have a documented, human-approved
      disposition that does not weaken a non-relaxable floor.
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
- [ ] `cargo dist plan --output-format=json` lists the two macOS archives, the
      Linux x86-64 archive, the Windows x86-64 MSVC archive, and both shell and
      PowerShell installers on native runners.
- [ ] Optional host-agnostic `cargo-xwin` and `cargo-zigbuild` checks pass; their
      versions and host are recorded. These are compile/link evidence only.
- [ ] Native macOS, Linux, and Windows build/test canaries pass. Record the OS,
      architecture, Rust version, and exact command. WSL2 is recorded as Linux,
      not as native Windows.
- [ ] The shell installer selects the correct macOS/Linux artifact, the
      PowerShell installer selects `codeflow.exe` on native Windows, and WSL2
      selects the Linux artifact. Each installed binary reports the release
      version and passes `codeflow doctor` in a disposable greenfield repo.
- [ ] A brownfield update canary preserves user-owned files and intentional
      sidecars, reports conflicts, and is idempotent when repeated.

## 4. Harness and model qualification

- [ ] The host-neutral Claude+Codex contract and evaluator fixtures pass with
      the currently supported model/harness bindings.
- [ ] Any material orchestration, task-graph, or verification-selection change
      updates its stable requirements, paired positive/non-ceremony cases,
      fixtures, packs, and managed mirrors. Applicable new behavioral cases
      have retained native interactive Claude and Codex canary evidence;
      deterministic corpus validation alone is not reported as model behavior.
- [ ] Any material design-contract change exercises proportional routing,
      operator-direction precedence, generic-default challenge, accessibility,
      and fidelity cases. Rendered comparisons retain same-environment artifacts
      and blinded paired judgments; taste alone is not reported as a defect.
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
