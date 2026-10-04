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

```text
  codeflow <command>
        |
        v
  codeflow-cli        main.rs (clap) -> cmd/ handler; embedded.rs
        |
        +--> codeflow-core
        |
        +--> codeflow-present
```

The binary holds all three crates and the embedded `assets/` tree.

- `codeflow-core` owns the discipline engine and `codeflow-present` owns
  bounded local review sessions.
- `codeflow-cli` is a thin dispatcher: `main.rs` is the clap command
  surface over every subcommand, most a small handler in `cmd/` that calls
  into core, while `init` and `update` dispatch inline in `main.rs` to the
  scaffold module. The [command reference](cli.md) lists each subcommand and
  flag, generated from those clap definitions.
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
| Enforcement | `hooks/`, `security/`, `git/`, `delegate.rs`, `integrate.rs`, `remote.rs`, `root_checkout.rs` | the `git-guard`, `exec-guard` and `edit-guard` PreToolUse handlers and the git-client hook stages, the dual-mode `delegate-turn` adapter (the legacy `--result` record-and-signal mode plus the transport-neutral delegate lifecycle state machine in `delegate.rs`, architecture decision record ADR-0036), the secret scanner, git conflict detection and CI wait, the flock-guarded `integrate` primitive with its gate-context token, the GitHub remote-protect adapter, and the root checkout's root branch, the actor read from harness markers, and workspace mode (ADR-0074) |
| Records and knowledge | `models/`, `ledger/`, `workgraph/`, `validate/`, `capability.rs`, `recall.rs`, `registry.rs` | frontmatter models, the JSONL ledger, the work graph, `validate` and its `--docs` referential-integrity lint, including structural task dependency identity, reference and cycle checks, the capability registry parser, full-text search (FTS5) recall, and the cross-repo registry |
| Support | `doctor/`, `settings/`, `status.rs`, `testing/`, `file_lock.rs`, `error.rs`, `reading.rs` | the doctor check table (20 checks: hooks, claude, codex, grok, config, permissions, policy-source, network, delegates, model-bindings, delegate-roundtrip, repo-integrity, ci-perimeter, managed-drift, customization, instructions, reading, test-config, id-registry, adopter-fit), the progressive reading map (`reading.rs`: the per-task reading chain, the conditional reads and their triggers, the orphan check and the size guideline numbers, shared with `artifact_budget_contract`), including bidirectional delegate readiness (Codex auth and Model Context Protocol (MCP) servers, Claude MCP, and Herdr with tmux as the last fallback, the Codex plugin reported only as an optional fallback (ADR-0077); live interactive canaries remain outside the binary) and a sentinel-based consuming-project customization nudge, structured settings merge, generated status views, the test-gate engine, path flock, and pruned error types |

The shell plane reads a composed deletion as scoped shell.

- Each variable, positional parameter and the working directory carry every
  value they may hold through subshells, branches, loops and function calls.
- The deletion is refused when any of them reaches a protected location.
- The reader is closed-world. Its module doc lists the grammar it models, traps
  and zsh hook functions included.
- Anything else in command position or between commands makes the state
  unknown: a sourced file, a name reference, an unmodelled builtin or option,
  zsh-only syntax, or a command named by an unknown value. A deletion that
  depends on it is refused as unproven (TSK-141).

`session-orient` is also the advisory entry for `UserPromptSubmit`.

- It reads the payload's event. After a compaction, resume or fork it adds the
  kernel guidance block. For a prompt that asks for a duration, a status or a
  complex explanation it adds one rule line (`hooks/guidance.rs`, TSK-128).
- Contract 3 refuses an older binary on either event.
- The guards never pass through it. Grok Build ignores these events' output, so
  it wires only the guards.

A git-hook shim runs the `codeflow` binary whose command started git.

- That command names itself in `CODEFLOW_HOOK_BINARY` for its git children
  only. The shim fails when the named binary is missing or not executable.
- Git run outside codeflow uses the `codeflow` on PATH. The shim refuses the
  operation, prints repair instructions and exits 1 when there is none or the
  binary is too old (SPC-013 R-85).

The test gate evaluates file and aggregate coverage rules through one verdict.

| Test-gate rule | Behavior |
|---|---|
| `changed_files` rules | rejected at config load, because standalone test runs have no explicit comparison base (ADR-0021); silently evaluating an empty set is not a supported degradation |
| child output | stdout and stderr are drained into bounded tail buffers, with truncation recorded on each target result |
| setup | an adapter over the same typed config boundary: root-only detection, release-embedded templates, and explicit target append all use the deterministic writer and colocated schema |
| automatic setup | may fill an absent or empty config but never replaces populated or malformed project intent |
| target relations | a target may declare the targets it `requires`, the `outputs` it produces, the `narrow` inputs that alone select it, and `exclusive` when it needs the machine to itself; the runner schedules producers and prerequisites first and runs targets with no producer and consumer relation in parallel up to `max_parallel`; a dependent of a red target reports "not run: prerequisite failed", and a missing or cyclic `requires` fails the run before any target starts |
| candidate tree | named only after the producers ran; the run fails when generation changed tracked bytes |
| change-aware selection | skips a target only when its declared inputs are unchanged against a base that has a recorded green run for the same config digest; a path no set matches, a change to the run-everything set (Rust, assets, build, config, workflows, gate scripts), a rename or deletion, an unknown base, and an epic close (`--all`) run every target; the run prints what it selected and skipped and why |
| result artifact | bound to its revision, tree hash and config digest; a full run copies it to `~/.codeflow/gate-runs/<repo>/<run-id>/`, outside any worktree, where PR bodies cite it |
| CodeFlow's own full gate | runs the Rust suite once, instrumented (`cargo llvm-cov nextest` with a 90% aggregate line floor), and its journey check reads that run's results by exact test name; the doctest target still runs, and on GitHub the Windows jobs' raw steps re-run the suite independently of the runner, in six nextest partitions whose results one journey job reads together; one `windows` verdict passes only when the other Windows checks, every partition and the journey job passed. On GitHub the Linux full gate runs as four parallel `codeflow test --only` parts behind one `codeflow gates` verdict (TSK-203) |

Delegation has two engine surfaces: the legacy `delegate-turn --result`
adapter, and the transport-neutral delegate lifecycle in `delegate.rs`, surfaced
as `codeflow delegate init|arm|wait` and `hook delegate-turn`. Its states and
invariants are on the [delegation page](delegation.md); the rationale is
ADR-0036 and ADR-0037.

A Claude Code task notice for work the turn backgrounded is admitted as a
continuation of that turn when the session transcript shows the turn launched
the task.

- Its record keeps the notice's `prompt_id` and byte digest.
- The Stop that closes it writes a result only after the transcript proves the
  prompt was a native task notice with those bytes. A typed copy poisons the
  run.
- The model may still act on a forged notice within that turn. The check keeps
  it from being recorded as a clean result.

Records follow the Markdown-truth design.

| Records contract | Detail |
|---|---|
| authority | markdown plus YAML frontmatter is the source of truth, the JSONL ledger is the append-only event log, and a SQLite full-text search cache is rebuildable, with no database-as-authority and no embeddings |
| store | core reads through a `RecordStore` trait with a `MarkdownStore` implementation |
| layout | the flat, independently allocated stable-ID layout `project-management/{epics/EPC-NNN.md,specs/SPC-NNN.md,tasks/TSK-NNN.md}` |
| layout reader | one shared layout enumerator keeps the store, docs validator, and recall index on that contract; specs are indexed as project-management recall sources |
| relationships | frontmatter owns them: tasks point to an epic or carry a standalone rationale, tasks name direct predecessors, and consuming epics or tasks link specs. Task records carry an `integration_target` and `depends_on` metadata |
| recall | walks source trees without following directory symlinks and applies depth and count budgets; encoded path bytes are index identity while lossy paths are display-only |
| id registry | ids come from a shared registry (ADR-0072, SPC-013): the data branch `codeflow/registry` on the authority remote holds one file per issued id, `ids/<KIND>/<N>.toml`, binding the number to the record's hidden `uid`. `epic new`, `spec new` and `task new` fetch the branch without force, take one more than the highest id ever added in its history or present on any ref, push the new file without force (the host's tip check is the compare-and-swap; a moved tip is retried at most five times), then create the record with that `uid`. Offline, the reservation is a pending local commit that `ids sync` publishes |
| id registry module | `codeflow_core::ids` owns the protocol, the ledger of the registry's history (append-only rule, current and repaired damage, typed restore), the record inventory on every ref, seeding with provenance, and the read-only judgements: `ids check`, the merge rule that binds every added record to its `uid`, and a uniqueness scan over all refs that holds even without a registry |
| id registry enforcement | pre-push and git-guard refuse deletion, force and non-additive ranges on the registry; the enforcing CI job runs on `pull_request_target` (the registry step of the `codeflow-policy` workflow, from the default branch) and checks out the pull request's base commit; `remote protect` applies the branch's data profile; `doctor` reports damage, unplaced ids and host assurance. Claims stay advisory (ADR-0072) |
| ledger compaction | syncs the directory after installing the merged base and again after deleting fragments, so crash ordering preserves the base |
| documentation validation | checks the non-executable structural graph for well-formed IDs, filenames, references, duplicates, parent/standalone exclusivity, spec readiness, self-edges, and cycles |
| `work start` preflight | read-only; it proves the task and its applicable graph at the merge-base with the declared target, or at HEAD for a standalone task whose record arrives in its own PR (ADR-0076); a code predecessor that is reviewed but not complete is accepted only through a reviewed pin (`--on TSK-NNN@<sha>`), and CI still requires it complete at the merge-base when the task lands. The CLI and detached CI share the structural core of that check; only `work start` applies the start gate (status, Blocker, awaiting selection), so CI admits a standalone record that arrives complete with a valid acceptance block |
| out of scope | scheduling and status mutation remain Plan and native-harness concerns (ADR-0040, ADR-0046) |

Three areas outgrew this file and are graduated, with the pointer left behind:

- Interactive presentation (`codeflow-present`, ADR-0049, ADR-0050,
  ADR-0052): [architecture/present.md](architecture/present.md).
- The four enforcement planes:
  [architecture/enforcement-planes.md](architecture/enforcement-planes.md).
- The shared design system:
  [architecture/utility-presentation.md](architecture/utility-presentation.md).

The runtime posture each harness receives, including the Codex permission
profile and Claude's credential mask, is in
[harness posture](harness-posture.md).

#### Text from the operating system and git

Names, paths and process text that the operating system or git supplies are
bytes, and valid UTF-8 is not promised (issue 79). One rule covers the engine:

- **Read it as bytes or lossily where the value is only compared or shown.** A
  process argument, an index path, a listing line or a config value is
  compared as bytes or read with `String::from_utf8_lossy`, and never turns a
  legal input into a failure.
- **Never decode a value that is an identity.** A lossy spelling can equal a
  different valid name (an invalid byte and a real U+FFFD both read as
  U+FFFD), so a process argument, a config key or a snapshot key is compared
  as bytes, or keyed so that no valid name can equal it, or dropped from the
  comparison. Where a decode cannot be exact, the read refuses.
- **Fail where a wrong value would change a security or identity decision**,
  and say why in a comment at that site, starting "OS text rule" or "Kept
  strict". Refuse and do not skip: a name the check cannot read is work or
  authority it cannot prove (a policy source ref, a remote name, a worktree
  name, a state directory written into a hook command).
- **A name that can only match a valid pattern is skipped.** A directory entry
  tested against a UUID or a `.tmp` suffix cannot match when it is not valid
  UTF-8, so the scan moves on.
- **File content is not covered.** JSON, TOML, Markdown and blobs are a format
  contract, and a decode failure there names the file.

The sites that stay strict are the ones with a comment. The pre-push and
reference-transaction stdin reads are strict by an earlier decision that the
`remedy_clearing` tests pin.

### scaffold: `assets/`

- `assets/base/` holds the shipped scaffold: the AGENTS.md and CLAUDE.md
  templates rendered from the `rule-map.toml` kernel by `scaffold::rule_map`,
  the `.codeflow/rules/` references, the `claude/` artifacts, `policy.json`,
  git-hook shims, docs and pm templates.
- `assets/docs-portal/` holds the optional portal starter.

Each area below is owned by an accepted decision. The mirrored skills and the
capability registry carry the operating detail.

| Area | Owning ADR(s) | Consequence |
|---|---|---|
| Ownership classes | ADR-0011, ADR-0019 | Fully-managed files (agents, skills, hook shims, CI) refresh by hash and 3-way merge from `.codeflow/.baseline/`; managed-region files (AGENTS.md markers, settings.json codeflow-prefixed keys) touch only their region; schema-versioned user config additively gains new keys with their defaults and never mutates a value you set, while the write-once `docs/` seeds are laid once at init and never touched again. `scaffold-manifest.toml` is the update contract |
| Duo orchestration (`cf-model-orchestrator`) | ADR-0030, ADR-0035, ADR-0041, ADR-0056, ADR-0060 | The stage-aware, harness-neutral entry for routed work in standard/full, decided by touched paths as the root map states; primaries default to high effort and obtain xhigh on trigger without restarting; linked checkouts live under `.worktrees/`; each actual executor first-verifies its unit, the responsible primary accepts it, and a different lineage reviews it independently |
| Optional estimation (`cf-estimate`) | ADR-0057 | Shipped to standard/full as progressive disclosure (capability CAP-017); adoption is project-owned `.codeflow/estimate.json`, forecasts and actuals live under the project's existing planning authority, and none of it is scaffold-managed. Minimal receives no method files |
| Design direction (`cf-design`) | ADR-0043, ADR-0051 | Material product, UX, interaction or visual work records a proportionate `DESIGN_INTENT` inside the epic's planning change; the qualified Claude judgment role leads intent and owns design implementation and fidelity; refinement after selection stays bounded to one named unresolved choice, and no second design database appears |
| Editorial quality (`cf-editorial-review`) | ADR-0032 | Substantial prose loads the mirrored skill rather than expanding the always-loaded contract; truth and policy outrank philosophy, voice and requested tone, and the Claude judgment primary owns the final contextual verdict |
| Task graphs and verification strength | ADR-0040 | A multi-task plan settles one acyclic graph whose guards represent genuine decisions; property tests, targeted mutation testing and architecture fitness checks are earned from risk and oracle evidence, and CodeFlow never becomes a task scheduler |
| Review materiality | ADR-0034 | Substantiated material and systemic findings precede cosmetics, evidence confidence stays distinct from severity, security keeps its Common Vulnerability Scoring System (CVSS) vocabulary, and out-of-scope material risk is routed without silent scope expansion |
| Critical-path stewardship | ADR-0038, ADR-0017 | The current critical path never licenses weaker quality, testing, security, review, documentation or recovery; escalation is reserved for a true external dependency or an intent-level choice; a gate is the check, not the CI job name, and a completed same-check in a sibling job or local run satisfies it |
| Runtime autonomy and settings | ADR-0025, ADR-0026, ADR-0028, ADR-0039, ADR-0060 | Claude's project settings are a fail-closed sandbox with file secret denies and exact raw model/cloud environment denies; a failed sandboxed command may request an auto-classified unsandboxed retry only for a trusted installed tool that needs host state, and arbitrary bypass stays outside the contract. The plan records the responsible primary separately from the actual executor. Detail: [harness posture](harness-posture.md) |
| Model and harness qualification (`cf-evaluate-model`) | ADR-0027 | A new model, harness, permission profile or material instruction revision is qualified over fresh one-commit disposable repositories in supervised native interactive sessions; no engine model router, headless peer runner, CI model call or general cleanup command is added. Detail: [model and harness upgrades](model-upgrades.md) |
| Binding facts versus durable doctrine | ADR-0039, ADR-0041, ADR-0054, ADR-0069 | `current-ensemble.json` is the schema 5 model catalog: families, product lines with ordered versions and pinned ids, seats with their designations, duties with their participants, effort floors and triggers. `codeflow models resolve` is the one read-only command that turns it into a duty's participants, pinned ids, efforts and obligations, or names the open participant; it launches nothing. `routing-policy.json` and `harnesses.json` own the fast-changing triggers and capability catalog while the orchestrator keeps stable role duties; a consuming project maps a stable role to an approved binding ID in `.codeflow/model-selection.json`, an absent or empty file keeps the managed catalog, and doctor fails closed on a malformed, missing, ineligible, unsupported, drifted or lineage-collapsing override. Interactive Grok Build is a first-class host; Hermes (an outer coordinator, not a native CodeFlow host) normally delegates the whole repository task |
| Layered code verification | ADR-0042 | Project-owned deterministic lanes cover the applicable syntax/style, software composition analysis (SCA), source and data flow, taint, secret and architecture rules while the two primary lineages review intent and semantics; a deterministic red result blocks regardless of model agreement, and consuming projects choose their own analyzer or record the residual risk |
| Whole-flow and UI isolation | ADR-0044 | Each material changed journey records one faithful vertical run across its applicable changed boundaries; concurrent UI tasks receive isolated browser state, non-overlapping endpoints, namespaced data, run-scoped artifacts and verified teardown; `codeflow status` reports removable, dirty and unproven resources and never deletes |
| Session review CLI (`codeflow present`) | ADR-0049, ADR-0050, ADR-0052 | Agents author this session's catalog document and the runtime owns chrome and the Comment tool; it is neither a documentation portal nor a clone of the design-exploration board |
| Documentation portal bundle | ADR-0048, ADR-0058 | A separate managed bundle, absent until `codeflow portal setup --path <dir>` adopts it and records its root, release, hashes and ownership in `.codeflow/docs-portal.json`; the exact-pinned Node adapter is the sole author of disposable pages, twins, `llms.txt` and evidence from one clean committed snapshot; drift, collisions and edited retirement stop every portal write; `codeflow validate --portal` re-derives the byte claims without executing project code |
| Utility presentation system | ADR-0053, ADR-0063 | One shared design system of tokens, type roles, altitude grammar, and stage grammar backs both `cf-present` and the portal, with a contract test failing the build on token drift |

### docs: `docs/`

The six-layer knowledge model this file belongs to. The decision records live
in `docs/decisions/` and are listed on the [decision map](decision-map.md).
