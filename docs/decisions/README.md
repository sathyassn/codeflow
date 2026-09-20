# codeflow: decision map

<!-- A map of the ADR record, authored from each decision's frontmatter (id,
     title, status, superseded_by). It summarises no ADR: the decisions remain
     append-only and authoritative. Add a row when an ADR lands. -->

**Decisions are grouped by the area they bind.**

| Area | Decision | Title | Superseded by |
|---|---|---|---|
| Stack and CLI boundary | [ADR-0001](ADR-0001-stack-choice.md) | stack choice — rust | none |
| · | [ADR-0003](ADR-0003-cli-vs-claude-boundary.md) | CLI vs Claude command boundary | none |
| · | [ADR-0004](ADR-0004-composable-pipeline.md) | Composable pipeline workflow, shipped user-owned | none |
| · | [ADR-0022](ADR-0022-replace-unmaintained-yaml-parser.md) | replace the unmaintained YAML parser | none |
| Git, merge, and CI policy | [ADR-0002](ADR-0002-sync-push-policy.md) | sync push policy — push_to_protected warn on this repo | ADR-0006 |
| · | [ADR-0006](ADR-0006-pr-based-landings.md) | pr-based landings — supersede the integrate path on this repo | none |
| · | [ADR-0007](ADR-0007-agent-human-merge-boundary.md) | agent/human merge boundary — protected-branch merge and ref controls | none |
| · | [ADR-0009](ADR-0009-human-authorization-out-of-band.md) | human authorization is out-of-band; local guards are the honest-agent floor | none |
| · | [ADR-0017](ADR-0017-codeflow-ci-portable-verification.md) | codeflow ci — CI-portable, binary-sourced verification | none |
| · | [ADR-0020](ADR-0020-restore-original-commit-standard.md) | restore the original commit standard — 50/72 subject, bullet-only body, block-enforced | none |
| Scaffold lifecycle and tiers | [ADR-0011](ADR-0011-update-orphan-reconciliation.md) | update reconciles orphaned managed files (prune on upstream rename/removal) | none |
| · | [ADR-0019](ADR-0019-enforcement-is-the-floor.md) | enforcement is the floor — tiers scale project-management, not discipline | none |
| Release | [ADR-0010](ADR-0010-automated-release.md) | automated release — release-plz owns version/changelog/tag, cargo-dist owns artifacts | ADR-0012 |
| · | [ADR-0012](ADR-0012-git-cliff-releases.md) | version + changelog via git-cliff (replacing release-plz); cargo-dist releases | ADR-0061 |
| · | [ADR-0061](ADR-0061-human-authorized-release-candidates.md) | automate release candidates while keeping publication human-authorized | ADR-0062 |
| · | [ADR-0062](ADR-0062-same-pr-release-state.md) | keep release state in the normal work PR | none |
| Security and sandbox boundaries | [ADR-0008](ADR-0008-harness-parity-and-exec-guard.md) | harness parity and the exec-guard security stage | none |
| · | [ADR-0014](ADR-0014-codex-credential-read-guard.md) | Codex credential read-guard via a named permission profile (cf-guard) | none |
| · | [ADR-0016](ADR-0016-security-redteam-review.md) | security / red-team review — dual-vendor adversarial stage plus deterministic scanner floor | none |
| · | [ADR-0025](ADR-0025-effective-harness-autonomy-and-tool-access.md) | effective harness autonomy with broad tools and guarded side effects | none |
| · | [ADR-0026](ADR-0026-harness-boundary-corrections.md) | correct harness credential and destructive-action boundaries | none |
| · | [ADR-0029](ADR-0029-classified-trusted-tool-sandbox-retry.md) | permit classified sandbox retry for trusted installed tools | none |
| · | [ADR-0033](ADR-0033-cross-platform-catastrophic-boundary.md) | make the catastrophic-action boundary cross-platform | none |
| · | [ADR-0047](ADR-0047-narrow-plugin-code-sandbox-carveout.md) | narrow plugin-code sandbox carveout | none |
| Cross-vendor transport and delegation | [ADR-0005](ADR-0005-cross-vendor-delegation.md) | cross-vendor delegation via harness-boundary composition | none |
| · | [ADR-0018](ADR-0018-interactive-only-cross-model-transport.md) | interactive-only cross-model transport — one lane per direction | ADR-0023 |
| · | [ADR-0023](ADR-0023-host-neutral-duo.md) | host-neutral Claude+Codex duo with fixed roles and evidence gates | none |
| · | [ADR-0036](ADR-0036-transport-neutral-delegate-lifecycle.md) | make delegate turns a durable transport-neutral lifecycle | none |
| · | [ADR-0037](ADR-0037-canonical-delegate-prompt-boundary.md) | require a canonical delegate prompt boundary | none |
| · | [ADR-0059](ADR-0059-qualified-native-transport-fallback.md) | qualify native transport by capabilities instead of plugin exclusivity | none |
| Duo orchestration and quality | [ADR-0015](ADR-0015-duo-model-orchestration.md) | duo-model orchestration — the Claude+codex develop flow (cf-model-orchestrator) | ADR-0023 |
| · | [ADR-0024](ADR-0024-stage-aware-duo-and-bounded-parallelism.md) | stage-aware duo default with bounded parallel worktrees | none |
| · | [ADR-0028](ADR-0028-evidence-routed-model-effort.md) | route model effort by evidence and task demand | none |
| · | [ADR-0030](ADR-0030-proportionate-design-and-code-quality.md) | make proportionate design and code quality a duo gate | none |
| · | [ADR-0032](ADR-0032-contextual-editorial-quality.md) | make editorial quality contextual and on demand | none |
| · | [ADR-0034](ADR-0034-materiality-and-proactive-stewardship.md) | prioritize substantiated material findings without issue farming | none |
| · | [ADR-0035](ADR-0035-capability-routed-duo-execution.md) | route duo execution by verified per-task capability | none |
| · | [ADR-0038](ADR-0038-critical-path-deferral-stewardship.md) | preserve critical-path focus without reflexive deferral or escalation | none |
| · | [ADR-0056](ADR-0056-high-primary-effort-with-bounded-workers.md) | High primary effort with bounded workers | none |
| · | [ADR-0060](ADR-0060-accountable-worker-delegation.md) | separate primary accountability from worker execution | none |
| Model and harness qualification | [ADR-0027](ADR-0027-native-interactive-model-evaluation.md) | qualify model and harness bindings with native-interactive evaluations | none |
| · | [ADR-0039](ADR-0039-qualified-model-harness-bindings.md) | separate durable orchestration doctrine from qualified model and harness bindings | none |
| · | [ADR-0041](ADR-0041-role-based-project-model-selection.md) | select qualified model bindings by stable project roles | none |
| · | [ADR-0054](ADR-0054-grok-first-class-host-and-catalog.md) | Grok as first-class host and catalog family; standing pair remains the quality floor | none |
| · | [ADR-0055](ADR-0055-medium-default-astra-contained-worktrees.md) | Medium-default effort, Astra Codex primary, contained worktrees, Herdr project cwd | none |
| Verification and evidence | [ADR-0021](ADR-0021-reject-baseless-changed-coverage.md) | reject changed-file coverage without a comparison base | none |
| · | [ADR-0031](ADR-0031-safe-adaptive-test-setup.md) | safe adaptive test setup and fail-closed configured gates | none |
| · | [ADR-0040](ADR-0040-task-graph-and-verification-strength.md) | settle task graphs and select verification strength by evidence | none |
| · | [ADR-0042](ADR-0042-layered-code-verification.md) | combine selected deterministic analyzers with contextual agentic verification | none |
| · | [ADR-0044](ADR-0044-whole-flow-and-ui-resource-isolation.md) | require whole-flow evidence and isolated UI resources | none |
| Durable work records | [ADR-0045](ADR-0045-durable-work-authority-and-layout.md) | define durable work authority and canonical record layout | ADR-0046 |
| · | [ADR-0046](ADR-0046-independent-work-ids-and-stable-planning-anchor.md) | use independent work ids and a stable planning anchor | none |
| Session orientation and memory | [ADR-0013](ADR-0013-codex-orient-and-compaction-resilience.md) | Codex gets the orient digest (SessionStart); compaction resilience is a disposition, not a snapshot | none |
| Design direction | [ADR-0043](ADR-0043-design-direction-contract.md) | make product and interface design direction an explicit proportionate contract | none |
| · | [ADR-0051](ADR-0051-bounded-design-sourcing-and-revision.md) | extend product design with bounded variation, trustworthy sourcing, and revision provenance | none |
| Presentation and documentation portal | [ADR-0048](ADR-0048-opt-in-documentation-portal-boundary.md) | isolate portal adoption and verify derived evidence | ADR-0058 |
| · | [ADR-0049](ADR-0049-cf-present-runtime-boundary.md) | bounded cf-present runtime and renderer boundary | none |
| · | [ADR-0050](ADR-0050-cf-present-document-session-contract.md) | versioned cf-present document and session contract | none |
| · | [ADR-0052](ADR-0052-separate-present-runtime-and-durable-state.md) | separate cf-present ephemeral runtime from durable state | none |
| · | [ADR-0053](ADR-0053-utility-presentation-stack-and-design-system.md) | utility presentation design system with Preact present chrome and Starlight portal | none |
| · | [ADR-0058](ADR-0058-explicit-portal-runtime-ownership.md) | replace portal source merging with explicit runtime ownership | none |
| · | [ADR-0063](ADR-0063-utility-presentation-doctrine-home.md) | one shared utility presentation doctrine with a durable architecture home | none |
| Agentic estimation | [ADR-0057](ADR-0057-optional-agentic-estimation.md) | Optional agentic operating and estimation method | none |

63 decisions. Three carry `status: superseded`; the chains below record every
`superseded_by` link declared in frontmatter, including the ones whose source
still reads `accepted`. A superseded ADR stays in place and is never rewritten;
the chain names the decision that now binds.

- ADR-0002 → ADR-0006
- ADR-0010 → ADR-0012 → ADR-0061 → ADR-0062
- ADR-0015 → ADR-0023
- ADR-0018 → ADR-0023
- ADR-0045 → ADR-0046
- ADR-0048 → ADR-0058

`docs/architecture.md` links each area to its consequence, and
`docs/capabilities.md` links each capability to the ADRs that shaped it.
