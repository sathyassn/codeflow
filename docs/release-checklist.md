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
- [ ] Both native interactive directions complete a scoped tool/MCP canary;
      evidence records versions, effort, tool access, and graceful degradation.
- [ ] Each task records its producer and cross-lineage reviewer with verified
      routing evidence; producers first-verify, the other lineage reviews each
      unit independently, and Fable owns the integrated design/code judgment.
      Every blocking finding is resolved or explicitly stops the release.
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
      release ownership is named.
- [ ] Downstream Agent OS work begins only from this verified public CodeFlow
      release; the portal is updated only after the matching Agent OS release.
