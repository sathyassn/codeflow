# codeflow — decision map

<!-- A map of the ADR record, authored from each decision's frontmatter (id,
     title, status, superseded_by). It summarises no ADR: the decisions remain
     append-only and authoritative. Add a row when an ADR lands. -->

**Decisions are grouped by the area they bind.**

| Area | Decision | Title | Superseded by |
|---|---|---|---|
| Stack and CLI boundary | [ADR-0001](ADR-0001-stack-choice.md) | stack choice — rust | — |
| · | [ADR-0003](ADR-0003-cli-vs-claude-boundary.md) | CLI vs Claude command boundary | — |
| · | [ADR-0004](ADR-0004-composable-pipeline.md) | Composable pipeline workflow, shipped user-owned | — |
| · | [ADR-0022](ADR-0022-replace-unmaintained-yaml-parser.md) | replace the unmaintained YAML parser | — |
| Git, merge, and CI policy | [ADR-0002](ADR-0002-sync-push-policy.md) | sync push policy — push_to_protected warn on this repo | ADR-0006 |
| · | [ADR-0006](ADR-0006-pr-based-landings.md) | pr-based landings — supersede the integrate path on this repo | — |
| · | [ADR-0007](ADR-0007-agent-human-merge-boundary.md) | agent/human merge boundary — protected-branch merge and ref controls | — |
| · | [ADR-0009](ADR-0009-human-authorization-out-of-band.md) | human authorization is out-of-band; local guards are the honest-agent floor | — |
| · | [ADR-0017](ADR-0017-codeflow-ci-portable-verification.md) | codeflow ci — CI-portable, binary-sourced verification | — |
| · | [ADR-0020](ADR-0020-restore-original-commit-standard.md) | restore the original commit standard — 50/72 subject, bullet-only body, block-enforced | — |
| Scaffold lifecycle and tiers | [ADR-0011](ADR-0011-update-orphan-reconciliation.md) | update reconciles orphaned managed files (prune on upstream rename/removal) | — |
| · | [ADR-0019](ADR-0019-enforcement-is-the-floor.md) | enforcement is the floor — tiers scale project-management, not discipline | — |
| Release | [ADR-0010](ADR-0010-automated-release.md) | automated release — release-plz owns version/changelog/tag, cargo-dist owns artifacts | ADR-0012 |
| · | [ADR-0012](ADR-0012-git-cliff-releases.md) | version + changelog via git-cliff (replacing release-plz); cargo-dist releases | ADR-0061 |
| · | [ADR-0061](ADR-0061-human-authorized-release-candidates.md) | automate release candidates while keeping publication human-authorized | ADR-0062 |
| · | [ADR-0062](ADR-0062-same-pr-release-state.md) | keep release state in the normal work PR | — |
| Security and sandbox boundaries | [ADR-0008](ADR-0008-harness-parity-and-exec-guard.md) | harness parity and the exec-guard security stage | — |
| · | [ADR-0014](ADR-0014-codex-credential-read-guard.md) | Codex credential read-guard via a named permission profile (cf-guard) | — |
| · | [ADR-0016](ADR-0016-security-redteam-review.md) | security / red-team review — dual-vendor adversarial stage plus deterministic scanner floor | — |
| · | [ADR-0025](ADR-0025-effective-harness-autonomy-and-tool-access.md) | effective harness autonomy with broad tools and guarded side effects | — |
| · | [ADR-0026](ADR-0026-harness-boundary-corrections.md) | correct harness credential and destructive-action boundaries | — |
| · | [ADR-0029](ADR-0029-classified-trusted-tool-sandbox-retry.md) | permit classified sandbox retry for trusted installed tools | — |
| · | [ADR-0033](ADR-0033-cross-platform-catastrophic-boundary.md) | make the catastrophic-action boundary cross-platform | — |
| · | [ADR-0047](ADR-0047-narrow-plugin-code-sandbox-carveout.md) | narrow plugin-code sandbox carveout | — |
| Cross-vendor transport and delegation | [ADR-0005](ADR-0005-cross-vendor-delegation.md) | cross-vendor delegation via harness-boundary composition | — |
| · | [ADR-0018](ADR-0018-interactive-only-cross-model-transport.md) | interactive-only cross-model transport — one lane per direction | ADR-0023 |
| · | [ADR-0023](ADR-0023-host-neutral-duo.md) | host-neutral Claude+Codex duo with fixed roles and evidence gates | — |
| · | [ADR-0036](ADR-0036-transport-neutral-delegate-lifecycle.md) | make delegate turns a durable transport-neutral lifecycle | — |
| · | [ADR-0037](ADR-0037-canonical-delegate-prompt-boundary.md) | require a canonical delegate prompt boundary | — |
| · | [ADR-0059](ADR-0059-qualified-native-transport-fallback.md) | qualify native transport by capabilities instead of plugin exclusivity | — |
| Duo orchestration and quality | [ADR-0015](ADR-0015-duo-model-orchestration.md) | duo-model orchestration — the Claude+codex develop flow (cf-model-orchestrator) | ADR-0023 |
| · | [ADR-0024](ADR-0024-stage-aware-duo-and-bounded-parallelism.md) | stage-aware duo default with bounded parallel worktrees | — |
| · | [ADR-0028](ADR-0028-evidence-routed-model-effort.md) | route model effort by evidence and task demand | — |
| · | [ADR-0030](ADR-0030-proportionate-design-and-code-quality.md) | make proportionate design and code quality a duo gate | — |
| · | [ADR-0032](ADR-0032-contextual-editorial-quality.md) | make editorial quality contextual and on demand | — |
| · | [ADR-0034](ADR-0034-materiality-and-proactive-stewardship.md) | prioritize substantiated material findings without issue farming | — |
| · | [ADR-0035](ADR-0035-capability-routed-duo-execution.md) | route duo execution by verified per-task capability | — |
| · | [ADR-0038](ADR-0038-critical-path-deferral-stewardship.md) | preserve critical-path focus without reflexive deferral or escalation | — |
| · | [ADR-0056](ADR-0056-high-primary-effort-with-bounded-workers.md) | High primary effort with bounded workers | — |
| · | [ADR-0060](ADR-0060-accountable-worker-delegation.md) | separate primary accountability from worker execution | — |
| Model and harness qualification | [ADR-0027](ADR-0027-native-interactive-model-evaluation.md) | qualify model and harness bindings with native-interactive evaluations | — |
| · | [ADR-0039](ADR-0039-qualified-model-harness-bindings.md) | separate durable orchestration doctrine from qualified model and harness bindings | — |
| · | [ADR-0041](ADR-0041-role-based-project-model-selection.md) | select qualified model bindings by stable project roles | — |
| · | [ADR-0054](ADR-0054-grok-first-class-host-and-catalog.md) | Grok as first-class host and catalog family; standing pair remains the quality floor | — |
| · | [ADR-0055](ADR-0055-medium-default-astra-contained-worktrees.md) | Medium-default effort, Astra Codex primary, contained worktrees, Herdr project cwd | — |
| Verification and evidence | [ADR-0021](ADR-0021-reject-baseless-changed-coverage.md) | reject changed-file coverage without a comparison base | — |
| · | [ADR-0031](ADR-0031-safe-adaptive-test-setup.md) | safe adaptive test setup and fail-closed configured gates | — |
| · | [ADR-0040](ADR-0040-task-graph-and-verification-strength.md) | settle task graphs and select verification strength by evidence | — |
| · | [ADR-0042](ADR-0042-layered-code-verification.md) | combine selected deterministic analyzers with contextual agentic verification | — |
| · | [ADR-0044](ADR-0044-whole-flow-and-ui-resource-isolation.md) | require whole-flow evidence and isolated UI resources | — |
| Durable work records | [ADR-0045](ADR-0045-durable-work-authority-and-layout.md) | define durable work authority and canonical record layout | ADR-0046 |
| · | [ADR-0046](ADR-0046-independent-work-ids-and-stable-planning-anchor.md) | use independent work ids and a stable planning anchor | — |
| Session orientation and memory | [ADR-0013](ADR-0013-codex-orient-and-compaction-resilience.md) | Codex gets the orient digest (SessionStart); compaction resilience is a disposition, not a snapshot | — |
| Design direction | [ADR-0043](ADR-0043-design-direction-contract.md) | make product and interface design direction an explicit proportionate contract | — |
| · | [ADR-0051](ADR-0051-bounded-design-sourcing-and-revision.md) | extend product design with bounded variation, trustworthy sourcing, and revision provenance | — |
| Presentation and documentation portal | [ADR-0048](ADR-0048-opt-in-documentation-portal-boundary.md) | isolate portal adoption and verify derived evidence | ADR-0058 |
| · | [ADR-0049](ADR-0049-cf-present-runtime-boundary.md) | bounded cf-present runtime and renderer boundary | — |
| · | [ADR-0050](ADR-0050-cf-present-document-session-contract.md) | versioned cf-present document and session contract | — |
| · | [ADR-0052](ADR-0052-separate-present-runtime-and-durable-state.md) | separate cf-present ephemeral runtime from durable state | — |
| · | [ADR-0053](ADR-0053-utility-presentation-stack-and-design-system.md) | utility presentation design system with Preact present chrome and Starlight portal | — |
| · | [ADR-0058](ADR-0058-explicit-portal-runtime-ownership.md) | replace portal source merging with explicit runtime ownership | — |
| · | [ADR-0063](ADR-0063-utility-presentation-doctrine-home.md) | one shared utility presentation doctrine with a durable architecture home | — |
| Agentic estimation | [ADR-0057](ADR-0057-optional-agentic-estimation.md) | Optional agentic operating and estimation method | — |

63 decisions. Three carry `status: superseded`; the chains below record every
`superseded_by` link declared in frontmatter, including the ones whose source
still reads `accepted`. A superseded ADR stays in place and is never rewritten
— the chain names the decision that now binds.

- ADR-0002 → ADR-0006
- ADR-0010 → ADR-0012 → ADR-0061 → ADR-0062
- ADR-0015 → ADR-0023
- ADR-0018 → ADR-0023
- ADR-0045 → ADR-0046
- ADR-0048 → ADR-0058

`docs/architecture.md` links each area to its consequence, and
`docs/capabilities.md` links each capability to the ADRs that shaped it.
