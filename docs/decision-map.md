# Decision map

<!-- Moved from docs/decisions/README.md, which keeps a pointer here. A map of
     the architecture decision records, built from each decision's frontmatter
     (id, title, status, superseded_by); the area column is authored. It
     summarises no decision: the records stay append-only and authoritative.
     Add a row when a decision lands. -->

## Concept

**An architecture decision record (ADR) says why the project is the way it is,
and this map only helps you find the right one.**

Each record is written once, when the decision is made, and never rewritten
afterwards. The map groups the records by the area they bind and shows which
ones replaced others. It summarises no decision, so read the record itself
before relying on it.

## Architecture

A later decision can replace an earlier one. The earlier record stays in
place with its `superseded_by` field naming its successor, so a chain always
ends at the decision that binds today.

- ADR-0002, then ADR-0006
- ADR-0010, then ADR-0012, then ADR-0061, then ADR-0062
- ADR-0015, then ADR-0023
- ADR-0018, then ADR-0023
- ADR-0045, then ADR-0046
- ADR-0048, then ADR-0058

The release chain is the longest. Some records in these chains still read
`accepted`, because a record is not rewritten when a later one narrows it; the
`superseded_by` link is what counts.

## Technical

67 decisions, 3 with `status: superseded`. A
decision is proposed, then accepted, and may later be superseded; its
content is never rewritten, and only `superseded_by` or a dated note is added.
[Architecture](architecture.md) links each area to its consequence, and the
[capability registry](capabilities.md) links each capability to the decisions
that shaped it.

| Area | Decision | Title | Status | Superseded by |
|---|---|---|---|---|
| Stack and CLI boundary | [ADR-0001](decisions/ADR-0001-stack-choice.md) | stack choice: Rust | accepted | none |
| Stack and CLI boundary | [ADR-0003](decisions/ADR-0003-cli-vs-claude-boundary.md) | CLI vs Claude command boundary | accepted | none |
| Stack and CLI boundary | [ADR-0004](decisions/ADR-0004-composable-pipeline.md) | Composable pipeline workflow, shipped user-owned | accepted | none |
| Stack and CLI boundary | [ADR-0022](decisions/ADR-0022-replace-unmaintained-yaml-parser.md) | replace the unmaintained YAML parser | accepted | none |
| Git, merge, and CI policy | [ADR-0002](decisions/ADR-0002-sync-push-policy.md) | sync push policy: push_to_protected warns on this repo | superseded | ADR-0006 |
| Git, merge, and CI policy | [ADR-0006](decisions/ADR-0006-pr-based-landings.md) | PR-based landings: supersede the integrate path on this repo | accepted | none |
| Git, merge, and CI policy | [ADR-0007](decisions/ADR-0007-agent-human-merge-boundary.md) | agent and human merge boundary: protected branch merge and ref controls | accepted | none |
| Git, merge, and CI policy | [ADR-0009](decisions/ADR-0009-human-authorization-out-of-band.md) | human authorization is out-of-band; local guards are the honest-agent floor | accepted | none |
| Git, merge, and CI policy | [ADR-0017](decisions/ADR-0017-codeflow-ci-portable-verification.md) | codeflow ci: CI portable, binary sourced verification | accepted | none |
| Git, merge, and CI policy | [ADR-0020](decisions/ADR-0020-restore-original-commit-standard.md) | restore the original commit standard: 50/72 subject, bullet only body, block enforced | accepted | none |
| Scaffold lifecycle and tiers | [ADR-0011](decisions/ADR-0011-update-orphan-reconciliation.md) | update reconciles orphaned managed files (prune on upstream rename/removal) | accepted | none |
| Scaffold lifecycle and tiers | [ADR-0019](decisions/ADR-0019-enforcement-is-the-floor.md) | enforcement is the floor: tiers scale project management, not discipline | accepted | none |
| Release | [ADR-0010](decisions/ADR-0010-automated-release.md) | automated release: release-plz owns version, changelog and tag, cargo-dist owns artifacts | superseded | ADR-0012 |
| Release | [ADR-0012](decisions/ADR-0012-git-cliff-releases.md) | version + changelog via git-cliff (replacing release-plz); cargo-dist releases | accepted | ADR-0061 |
| Release | [ADR-0061](decisions/ADR-0061-human-authorized-release-candidates.md) | automate release candidates while keeping publication human-authorized | accepted | ADR-0062 |
| Release | [ADR-0062](decisions/ADR-0062-same-pr-release-state.md) | keep release state in the normal work PR | accepted | none |
| Security and sandbox boundaries | [ADR-0008](decisions/ADR-0008-harness-parity-and-exec-guard.md) | harness parity and the exec-guard security stage | accepted | none |
| Security and sandbox boundaries | [ADR-0014](decisions/ADR-0014-codex-credential-read-guard.md) | Codex credential read-guard via a named permission profile (cf-guard) | accepted | none |
| Security and sandbox boundaries | [ADR-0016](decisions/ADR-0016-security-redteam-review.md) | security / red-team review: dual-vendor adversarial stage plus deterministic scanner floor | accepted | none |
| Security and sandbox boundaries | [ADR-0025](decisions/ADR-0025-effective-harness-autonomy-and-tool-access.md) | effective harness autonomy with broad tools and guarded side effects | accepted | none |
| Security and sandbox boundaries | [ADR-0026](decisions/ADR-0026-harness-boundary-corrections.md) | correct harness credential and destructive-action boundaries | accepted | none |
| Security and sandbox boundaries | [ADR-0029](decisions/ADR-0029-classified-trusted-tool-sandbox-retry.md) | permit classified sandbox retry for trusted installed tools | accepted | none |
| Security and sandbox boundaries | [ADR-0033](decisions/ADR-0033-cross-platform-catastrophic-boundary.md) | make the catastrophic-action boundary cross-platform | accepted | none |
| Security and sandbox boundaries | [ADR-0047](decisions/ADR-0047-narrow-plugin-code-sandbox-carveout.md) | narrow plugin-code sandbox carveout | accepted | none |
| Security and sandbox boundaries | [ADR-0066](decisions/ADR-0066-permission-preset-leaves-mode-and-ordinary-deletes-to-the-operator.md) | Permission preset leaves the mode and ordinary deletes to the operator | accepted | none |
| Cross-vendor transport and delegation | [ADR-0005](decisions/ADR-0005-cross-vendor-delegation.md) | cross-vendor delegation via harness-boundary composition | accepted | none |
| Cross-vendor transport and delegation | [ADR-0018](decisions/ADR-0018-interactive-only-cross-model-transport.md) | interactive only cross model transport: one lane per direction | accepted | ADR-0023 |
| Cross-vendor transport and delegation | [ADR-0023](decisions/ADR-0023-host-neutral-duo.md) | host-neutral Claude+Codex duo with fixed roles and evidence gates | accepted | none |
| Cross-vendor transport and delegation | [ADR-0036](decisions/ADR-0036-transport-neutral-delegate-lifecycle.md) | make delegate turns a durable transport-neutral lifecycle | accepted | none |
| Cross-vendor transport and delegation | [ADR-0037](decisions/ADR-0037-canonical-delegate-prompt-boundary.md) | require a canonical delegate prompt boundary | accepted | none |
| Cross-vendor transport and delegation | [ADR-0059](decisions/ADR-0059-qualified-native-transport-fallback.md) | qualify native transport by capabilities instead of plugin exclusivity | accepted | none |
| Duo orchestration and quality | [ADR-0015](decisions/ADR-0015-duo-model-orchestration.md) | duo-model orchestration: the Claude and Codex develop flow (cf-model-orchestrator) | accepted | ADR-0023 |
| Duo orchestration and quality | [ADR-0024](decisions/ADR-0024-stage-aware-duo-and-bounded-parallelism.md) | stage-aware duo default with bounded parallel worktrees | accepted | none |
| Duo orchestration and quality | [ADR-0028](decisions/ADR-0028-evidence-routed-model-effort.md) | route model effort by evidence and task demand | accepted | none |
| Duo orchestration and quality | [ADR-0030](decisions/ADR-0030-proportionate-design-and-code-quality.md) | make proportionate design and code quality a duo gate | accepted | none |
| Duo orchestration and quality | [ADR-0032](decisions/ADR-0032-contextual-editorial-quality.md) | make editorial quality contextual and on demand | accepted | none |
| Duo orchestration and quality | [ADR-0034](decisions/ADR-0034-materiality-and-proactive-stewardship.md) | prioritize substantiated material findings without issue farming | accepted | none |
| Duo orchestration and quality | [ADR-0035](decisions/ADR-0035-capability-routed-duo-execution.md) | route duo execution by verified per-task capability | accepted | none |
| Duo orchestration and quality | [ADR-0038](decisions/ADR-0038-critical-path-deferral-stewardship.md) | preserve critical-path focus without reflexive deferral or escalation | accepted | none |
| Duo orchestration and quality | [ADR-0056](decisions/ADR-0056-high-primary-effort-with-bounded-workers.md) | High primary effort with bounded workers | accepted | none |
| Duo orchestration and quality | [ADR-0060](decisions/ADR-0060-accountable-worker-delegation.md) | separate primary accountability from worker execution | accepted | none |
| Model and harness qualification | [ADR-0027](decisions/ADR-0027-native-interactive-model-evaluation.md) | qualify model and harness bindings with native-interactive evaluations | accepted | none |
| Model and harness qualification | [ADR-0039](decisions/ADR-0039-qualified-model-harness-bindings.md) | separate durable orchestration doctrine from qualified model and harness bindings | accepted | none |
| Model and harness qualification | [ADR-0041](decisions/ADR-0041-role-based-project-model-selection.md) | select qualified model bindings by stable project roles | accepted | none |
| Model and harness qualification | [ADR-0054](decisions/ADR-0054-grok-first-class-host-and-catalog.md) | Grok as first-class host and catalog family; standing pair remains the quality floor | accepted | none |
| Model and harness qualification | [ADR-0055](decisions/ADR-0055-medium-default-astra-contained-worktrees.md) | Medium-default effort, Astra Codex primary, contained worktrees, Herdr project cwd | accepted | none |
| Verification and evidence | [ADR-0021](decisions/ADR-0021-reject-baseless-changed-coverage.md) | reject changed-file coverage without a comparison base | accepted | none |
| Verification and evidence | [ADR-0031](decisions/ADR-0031-safe-adaptive-test-setup.md) | safe adaptive test setup and fail-closed configured gates | accepted | none |
| Verification and evidence | [ADR-0040](decisions/ADR-0040-task-graph-and-verification-strength.md) | settle task graphs and select verification strength by evidence | accepted | none |
| Verification and evidence | [ADR-0042](decisions/ADR-0042-layered-code-verification.md) | combine selected deterministic analyzers with contextual agentic verification | accepted | none |
| Verification and evidence | [ADR-0044](decisions/ADR-0044-whole-flow-and-ui-resource-isolation.md) | require whole-flow evidence and isolated UI resources | accepted | none |
| Durable work records | [ADR-0045](decisions/ADR-0045-durable-work-authority-and-layout.md) | define durable work authority and canonical record layout | superseded | ADR-0046 |
| Durable work records | [ADR-0046](decisions/ADR-0046-independent-work-ids-and-stable-planning-anchor.md) | use independent work ids and a stable planning anchor | accepted | none |
| Session orientation and memory | [ADR-0013](decisions/ADR-0013-codex-orient-and-compaction-resilience.md) | Codex gets the orient digest (SessionStart); compaction resilience is a disposition, not a snapshot | accepted | none |
| Design direction | [ADR-0043](decisions/ADR-0043-design-direction-contract.md) | make product and interface design direction an explicit proportionate contract | accepted | none |
| Design direction | [ADR-0051](decisions/ADR-0051-bounded-design-sourcing-and-revision.md) | extend product design with bounded variation, trustworthy sourcing, and revision provenance | accepted | none |
| Presentation and documentation portal | [ADR-0048](decisions/ADR-0048-opt-in-documentation-portal-boundary.md) | isolate portal adoption and verify derived evidence | accepted | ADR-0058 |
| Presentation and documentation portal | [ADR-0049](decisions/ADR-0049-cf-present-runtime-boundary.md) | bounded cf-present runtime and renderer boundary | accepted | none |
| Presentation and documentation portal | [ADR-0050](decisions/ADR-0050-cf-present-document-session-contract.md) | versioned cf-present document and session contract | accepted | none |
| Presentation and documentation portal | [ADR-0052](decisions/ADR-0052-separate-present-runtime-and-durable-state.md) | separate cf-present ephemeral runtime from durable state | accepted | none |
| Presentation and documentation portal | [ADR-0053](decisions/ADR-0053-utility-presentation-stack-and-design-system.md) | utility presentation design system with Preact present chrome and Starlight portal | accepted | none |
| Presentation and documentation portal | [ADR-0058](decisions/ADR-0058-explicit-portal-runtime-ownership.md) | replace portal source merging with explicit runtime ownership | accepted | none |
| Presentation and documentation portal | [ADR-0063](decisions/ADR-0063-utility-presentation-doctrine-home.md) | one shared utility presentation doctrine with a durable architecture home | accepted | none |
| Presentation and documentation portal | [ADR-0064](decisions/ADR-0064-portal-as-a-guide-to-the-project-as-it-stands.md) | the portal is a guide to the project as it stands | accepted | none |
| Presentation and documentation portal | [ADR-0068](decisions/ADR-0068-figure-grammar-as-default-form.md) | Figure grammar is the default form of a utility figure | accepted | none |
| Agentic estimation | [ADR-0057](decisions/ADR-0057-optional-agentic-estimation.md) | Optional agentic operating and estimation method | accepted | none |
| Written content | [ADR-0067](decisions/ADR-0067-written-content-policy.md) | Written content policy for new text | accepted | none |
