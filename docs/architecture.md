# CodeFlow architecture

<!-- HOW layer. Updated only inside the ship flow, in the same PR as the code,
     when an ADR declares architecture impact. Link to decisions by ADR id;
     never duplicate their content here.
     When an area outgrows this file, graduate it to docs/architecture/<area>.md
     and leave a one-line pointer behind. -->

## Concept

This repository has three areas, and two of them ship inside the one
`codeflow` binary.

The engine is the Rust code, the scaffold is the `assets/` tree a consumer
receives, and the docs describe both. Because the repository is its own first
consumer, the scaffold it ships is the scaffold it runs under. Each surface,
from scaffold to present, is a command of the same binary, never a separate
service.

## Architecture

A three-crate Cargo workspace builds one binary with the scaffold and the
presentation renderer embedded.

- `codeflow-core` owns the discipline engine and `codeflow-present` owns
  bounded local review sessions.
- `codeflow-cli` is a thin dispatcher: `main.rs` is a clap command surface
  over 22 subcommands (`init`, `update`, `hook`, `git-hook`, `orient`, `test`,
  `validate`, `ci`, `status`, `integrate`, `doctor`, `policy`, `recall`,
  `remote`, `epic`, `task`, `spec`, `work`, `delegate`, `estimate`, `present`,
  `portal`), most a small handler in `cmd/` that calls into core, while
  `init` and `update` dispatch inline in `main.rs` to the scaffold module.
- `embedded.rs` embeds `assets/` via rust-embed; debug builds read `assets/`
  from disk for instant scaffold iteration.

Enforcement is structural rather than a module of that dispatcher: four planes
read one policy source, `.codeflow/policy.json`. That area has its own page,
[enforcement planes](architecture/enforcement-planes.md).

## Technical

Each command passes through the crates in the same order; the tables below are
the per-area depth.

### engine: `crates/codeflow-core` + `crates/codeflow-cli` + `crates/codeflow-present`

Core modules grouped by responsibility.

| Area | Modules | What it owns |
|---|---|---|
| Scaffold | `scaffold/` | `init`, `update`, the manifest, 3-way merge, and the ownership classes below, sourced from the rust-embed asset provider |
| Enforcement | `hooks/`, `security/`, `git/`, `delegate.rs`, `integrate.rs`, `remote.rs` | the `git-guard` and `exec-guard` PreToolUse handlers and the git-client hook stages, the `delegate-turn` adapter and the transport-neutral delegate lifecycle state machine in `delegate.rs` (architecture decision record ADR-0036), the secret scanner, git conflict detection and CI wait, the flock-guarded `integrate` primitive with its gate-context token, and the GitHub remote-protect adapter |
| Records and knowledge | `models/`, `ledger/`, `workgraph/`, `validate/`, `capability.rs`, `recall.rs`, `registry.rs` | frontmatter models, the JSONL ledger, the work graph, `validate` and its `--docs` referential-integrity lint, including structural task dependency identity, reference and cycle checks, the capability registry parser, full-text search (FTS5) recall, and the cross-repo registry |
| Support | `doctor/`, `settings/`, `status.rs`, `testing/`, `file_lock.rs`, `error.rs` | the doctor check table (15 checks: hooks, claude, codex, grok, config, permissions, network, delegates, model-bindings, delegate-roundtrip, repo-integrity, ci-perimeter, managed-drift, customization, test-config), including bidirectional delegate readiness (Codex auth and Model Context Protocol (MCP) servers, the Claude plugin, and tmux prerequisites; live interactive canaries remain outside the binary) and a sentinel-based consuming-project customization nudge, structured settings merge, generated status views, the test-gate engine, path flock, and pruned error types |

The test gate evaluates file and aggregate coverage rules through one verdict.

| Test-gate rule | Behavior |
|---|---|
| `changed_files` rules | rejected at config load, because standalone test runs have no explicit comparison base (ADR-0021); silently evaluating an empty set is not a supported degradation |
| child output | stdout and stderr are drained into bounded tail buffers, with truncation recorded on each target result |
| setup | an adapter over the same typed config boundary: root-only detection, release-embedded templates, and explicit target append all use the deterministic writer and colocated schema |
| automatic setup | may fill an absent or empty config but never replaces populated or malformed project intent |
| CodeFlow's own full local gate | additionally runs `cargo llvm-cov` with a 90% aggregate line floor, matching the independent CI coverage job |

Delegation is one engine surface: the transport-neutral delegate lifecycle in
`delegate.rs`, surfaced as `codeflow delegate init|arm|wait` and
`hook delegate-turn`. Its states and invariants are on the
[delegation page](delegation.md); the rationale is ADR-0036 and ADR-0037.

Records follow the Markdown-truth design.

| Records contract | Detail |
|---|---|
| authority | markdown plus YAML frontmatter is the source of truth, the JSONL ledger is the append-only event log, and a SQLite full-text search cache is rebuildable, with no database-as-authority and no embeddings |
| store | core reads through a `RecordStore` trait with a `MarkdownStore` implementation |
| layout | the flat, independently allocated stable-ID layout `project-management/{epics/EPC-NNN.md,specs/SPC-NNN.md,tasks/TSK-NNN.md}` |
| layout reader | one shared layout enumerator keeps the store, docs validator, and recall index on that contract; specs are indexed as project-management recall sources |
| relationships | frontmatter owns them: tasks point to an epic or carry a standalone rationale, tasks name direct predecessors, and consuming epics or tasks link specs. Task records carry an `integration_target` and `depends_on` metadata |
| recall | walks source trees without following directory symlinks and applies depth and count budgets; encoded path bytes are index identity while lossy paths are display-only |
| ledger compaction | syncs the directory after installing the merged base and again after deleting fragments, so crash ordering preserves the base |
| documentation validation | checks the non-executable structural graph for well-formed IDs, filenames, references, duplicates, parent/standalone exclusivity, spec readiness, self-edges, and cycles |
| `work start` preflight | read-only; it proves the task and its applicable graph at the merge-base with the declared target, and the CLI, pre-commit hook, and detached CI share that core check |
| out of scope | scheduling and status mutation remain Plan and native-harness concerns (ADR-0040, ADR-0046) |

Three areas outgrew this file and are graduated, with the pointer left behind:
interactive presentation (`codeflow-present`, ADR-0049, ADR-0050, ADR-0052) to
[architecture/present.md](architecture/present.md), the four enforcement planes
to [architecture/enforcement-planes.md](architecture/enforcement-planes.md),
and the shared design system to
[architecture/utility-presentation.md](architecture/utility-presentation.md).
The runtime posture each harness receives, including the Codex permission
profile and Claude's credential mask, is in
[harness posture](harness-posture.md).

### scaffold: `assets/`

`assets/base/` holds the shipped scaffold (AGENTS.md/CLAUDE.md templates, the
`claude/` artifacts, policy.json, git-hook shims, docs and pm templates), and
`assets/docs-portal/` holds the optional portal starter. Each area below is
owned by an accepted decision; the mirrored skills and the capability registry
carry the operating detail.

| Area | Owning ADR(s) | Consequence |
|---|---|---|
| Ownership classes | ADR-0011, ADR-0019 | Fully-managed files (agents, skills, hook shims, CI) refresh by hash and 3-way merge from `.codeflow/.baseline/`; managed-region files (AGENTS.md markers, settings.json codeflow-prefixed keys) touch only their region; schema-versioned user config additively gains new keys with their defaults and never mutates a value you set, while the write-once `docs/` seeds are laid once at init and never touched again. `scaffold-manifest.toml` is the update contract |
| Duo orchestration (`cf-model-orchestrator`) | ADR-0030, ADR-0035, ADR-0041, ADR-0056, ADR-0060 | The stage-aware, harness-neutral default for every non-trivial task in standard/full; primaries default to high effort and obtain xhigh on trigger without restarting; linked checkouts live under `.worktrees/`; each actual executor first-verifies its unit, the responsible primary accepts it, and a different lineage reviews it independently |
| Optional estimation (`cf-estimate`) | ADR-0057 | Shipped to standard/full as progressive disclosure (capability CAP-017); adoption is project-owned `.codeflow/estimate.json`, forecasts and actuals live under the project's existing planning authority, and none of it is scaffold-managed. Minimal receives no method files |
| Design direction (`cf-design`) | ADR-0043, ADR-0051 | Material product, UX, interaction or visual work records a proportionate `DESIGN_INTENT` inside Plan vN; the qualified Claude judgment role leads intent and owns design implementation and fidelity; refinement after selection stays bounded to one named unresolved choice, and no second design database appears |
| Editorial quality (`cf-editorial-review`) | ADR-0032 | Substantial prose loads the mirrored skill rather than expanding the always-loaded contract; truth and policy outrank philosophy, voice and requested tone, and the Claude judgment primary owns the final contextual verdict |
| Task graphs and verification strength | ADR-0040 | A multi-task plan settles one acyclic graph whose guards represent genuine decisions; property tests, targeted mutation testing and architecture fitness checks are earned from risk and oracle evidence, and CodeFlow never becomes a task scheduler |
| Review materiality | ADR-0034 | Substantiated material and systemic findings precede cosmetics, evidence confidence stays distinct from severity, security keeps its Common Vulnerability Scoring System (CVSS) vocabulary, and out-of-scope material risk is routed without silent scope expansion |
| Critical-path stewardship | ADR-0038, ADR-0017 | The current critical path never licenses weaker quality, testing, security, review, documentation or recovery; escalation is reserved for a true external dependency or an intent-level choice; a gate is the check, not the CI job name, and a completed same-check in a sibling job or local run satisfies it |
| Runtime autonomy and settings | ADR-0025, ADR-0026, ADR-0028, ADR-0039, ADR-0060 | Claude's project settings are a fail-closed sandbox with file secret denies and exact raw model/cloud environment denies; a failed sandboxed command may request an auto-classified unsandboxed retry only for a trusted installed tool that needs host state, and arbitrary bypass stays outside the contract. The plan records the responsible primary separately from the actual executor. Detail: [harness posture](harness-posture.md) |
| Model and harness qualification (`cf-evaluate-model`) | ADR-0027 | A new model, harness, permission profile or material instruction revision is qualified over fresh one-commit disposable repositories in supervised native interactive sessions; no engine model router, headless peer runner, CI model call or general cleanup command is added. Detail: [model and harness upgrades](model-upgrades.md) |
| Binding facts versus durable doctrine | ADR-0039, ADR-0041, ADR-0054 | `current-ensemble.json`, `routing-policy.json` and `harnesses.json` own the fast-changing selectors, triggers and capability catalog while the orchestrator keeps stable role duties; a consuming project maps a stable role to an approved binding ID in `.codeflow/model-selection.json`, and doctor fails closed on a malformed, missing, ineligible, unsupported, drifted or lineage-collapsing override. Interactive Grok Build is a first-class host; Hermes (an outer coordinator, not a native CodeFlow host) normally delegates the whole repository task |
| Layered code verification | ADR-0042 | Project-owned deterministic lanes cover the applicable syntax/style, software composition analysis (SCA), source and data flow, taint, secret and architecture rules while the two primary lineages review intent and semantics; a deterministic red result blocks regardless of model agreement, and consuming projects choose their own analyzer or record the residual risk |
| Whole-flow and UI isolation | ADR-0044 | Each material changed journey records one faithful vertical run across its applicable changed boundaries; concurrent UI tasks receive isolated browser state, non-overlapping endpoints, namespaced data, run-scoped artifacts and verified teardown; `codeflow status` reports removable, dirty and unproven resources and never deletes |
| Session review CLI (`codeflow present`) | ADR-0049, ADR-0050, ADR-0052 | Agents author this session's catalog document and the runtime owns chrome and the Comment tool; it is neither a documentation portal nor a clone of the design-exploration board |
| Documentation portal bundle | ADR-0048, ADR-0058 | A separate managed bundle, absent until `codeflow portal setup --path <dir>` adopts it and records its root, release, hashes and ownership in `.codeflow/docs-portal.json`; the exact-pinned Node adapter is the sole author of disposable pages, twins, `llms.txt` and evidence from one clean committed snapshot; drift, collisions and edited retirement stop every portal write; `codeflow validate --portal` re-derives the byte claims without executing project code |
| Utility presentation system | ADR-0053, ADR-0063 | One shared design system of tokens, type roles, altitude grammar, and stage grammar backs both `cf-present` and the portal, with a contract test failing the build on token drift |

### docs: `docs/`

The six-layer knowledge model this file belongs to. The decision records live
in `docs/decisions/` and are listed on the [decision map](decision-map.md).
