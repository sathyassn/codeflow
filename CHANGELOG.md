# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Grok Build as a first-class host and catalog family (ADR-0054).** Interactive
  `grok` joins Claude Code and Codex as a CodeFlow host. The standing pair
  remains `claude-judgment-primary` and `codex-engineering-primary`;
  `grok-engineering-primary` is a catalog seat on `grok-4.6` at medium (ADR-0055). Extra-family
  review is named when a routing-policy trigger fires and the family is
  available; unavailable is an evidenced limitation, never a silent third vote.
  Claude produces design in its native session regardless of host. `grok-cli`
  is catalog-supported with a `grok-cli-version` doctor probe. Grok-hosted
  Claude/Codex lanes use Herdr and are not claimed complete without canaries.
  Headless `grok -p` is not a work-session lane.

- **Herdr-primary consult and delegate TTY overlay.** Inside Herdr, dual-lineage
  seats open in named tabs (`cf/<repo>/<work>/<kind>/<nn>`) without hijacking
  the caller pane. Resume requires the agent's cwd to match the intended
  worktree. Schema-v2 lifecycle still owns Claude turn completion; tmux remains
  the degraded host. Description-trigger checks live under `evals/skill-triggers/`
  and run in the test gate.

### Changed

- **Mid-session effort and extra-family invoke.** The medium primary stays
  the orchestrator and spawns same-family high/xhigh workers when a
  complexity trigger fires. Extra catalog families are named when
  routing-policy triggers fire and the family is available; still never a
  silent third vote.

- **Codex Computer Use QA of changed UI (ADR-0043).** Claude still produces
  `DESIGN_INTENT` and the implementer check. Codex independently QAs the
  changed surface through Computer Use on the app-server. If Codex produced
  the UI, Claude QAs with Computer Use in Claude Code. Playwright stays
  the web driver. Adaptive viewports, accessibility, i18n/LTR-RTL, and
  system layers stay proportionate intent dimensions.

- **Gate redness is classified (ADR-0017, ADR-0038).** A gate is the
  verification check, not the CI job name. An assertion-red completed check
  still cannot be overridden. An infra-killed job (runner, memory, billing)
  is missing job evidence; a completed same-check in a sibling job or local
  `codeflow test` satisfies it. Agents still never merge.

- **Medium-default effort, Astra Codex primary, contained worktrees (ADR-0055).**
  All families default to medium and escalate to high/xhigh on complexity or
  difficulty (new or existing architecture, technical, planning, or design work
  — novelty is not a trigger). Codex primary is `gpt-6-astra` via the official
  app-server, with interactive CLI as the Grok-host/Codex-host fallback (the
  Claude Code lane stays plugin-only); no third-party Grok Codex plugins.
  Production host permissions are Claude `bypassPermissions` or
  `auto`, Codex full-access/always-approve, Grok `--always-approve`. Consult
  and no-edit review stay non-bypass. Linked checkouts live under
  `.worktrees/<slug>` (gitignored). `core.hooksPath` stays project-relative.
  Herdr/tmux cwd is the project being worked; reuse the same tab for the same
  topic and close it when that work is done.

- **Plan, develop, and consult craft.** After both seats settle Plan vN, planning
  synthesizes that ground and still asks live operator-owned questions. Develop
  tests through named interfaces first. Consult findings label `axis: standards`
  or `axis: spec` (both when both apply). An operator-unattended TTY overlay
  may skip routine approval clicks when the operator asks not to babysit,
  without amending harness-boundary ADRs or granting consults write access.

## [3.0.0] - 2026-08-02

> **Breaking migrations.** Before installing v3:
>
> - replace every `changed_files` coverage scope with `per_file`, `per_package`,
>   `per_module`, or `global`;
> - rename or remove test modes outside `quick`, `essential`, and `full`;
> - set `security.dangerous_commands` to `block`, or remove the key to accept
>   the non-relaxable default; and
> - repair canonical work records reported by `codeflow validate --docs`,
>   including filename/ID mismatches, malformed or dangling relationships,
>   completed tasks with unchecked acceptance criteria, and approved or
>   implemented specs with unresolved open questions.
>
> Make those repairs on a planning branch and merge them before updating. These
> contract changes require a major bump from `v2.1.0`; feature additions do not
> reduce the release to a minor version.

### Fixed

- **Presentation qualification now exercises the real owned runtime and offline
  export boundary.** A full-only gate drives the CLI, authenticated loopback
  service, isolated headless browser profiles, feedback/update/reopen flow,
  enhanced export, accessibility, network/console state, and exact bounded
  teardown. The real journey corrected strict-cookie bootstrap handoff and
  template payload extraction defects that raw HTTP and synthetic browser tests
  had missed. An injected failure proves the primary error survives exhaustive
  exact-owned cleanup, and runner children start from an explicit functional
  environment rather than inheriting unrelated host values. Executable child
  canaries cover the complete provider-secret deny set without leaking coverage
  profiles. Windows qualification now fails closed unless an externally
  provisioned disposable profile confines the actual Local AppData Known
  Folder; the outer native lane still owns profile or VM teardown proof.
  Greenfield/brownfield initialized-baseline parity, idempotent update, narrow
  cleanup, reproducible locked assets, exact CI Node/npm tooling, binary Git
  attributes, and isolated deterministic release payload-size deltas now have
  owning checks. Interrupted presentation creation is recoverable even before
  its transaction marker is written. Documentation-portal stale stubs retain
  only a bounded unavailable identity so current records can link to the safe
  diagnostic without republishing stale content, and a parity gate keeps the
  dogfood runtime identical to the shipped starter on every supported host.
  Producer and verifier now share one strict unavailable-identity grammar,
  while the local aggregate and CI both enforce the reciprocal documentation
  graph instead of leaving that check to a separate hosted step.
- **Claude's OS sandbox now keeps installed plugin code executable without
  exposing mutable plugin or private Claude state.** The broad `~/.claude`
  subprocess-read denial remains, while the higher-precedence filesystem
  carveout permits only `~/.claude/plugins/cache`; plugin job data, credentials,
  histories, sessions, settings, memory, and other private state remain denied.
  Updates retire superseded CodeFlow-managed sandbox array entries from the
  prior shipped baseline while preserving project-owned entries.
- **Delegate interruption coverage now waits for the process handler.** The
  SIGINT lifecycle test allows a bounded child-initialization window, avoiding
  a false failure under parallel or instrumented test startup while still
  proving exit 130 and run poisoning after prompt acceptance.
- **New durable records cannot start malformed.** Epic, spec, and task titles
  are encoded safely in YAML and must be non-empty single-line labels. Task
  allocation also proves that its declared integration target already resolves
  to a real local or remote-tracking branch instead of deferring a missing
  target to implementation time.
- **Work-start targets cannot be arbitrary Git revisions.** The stable-anchor
  preflight resolves only real local or remote-tracking non-task branches and
  record validator rejects `HEAD`, full object IDs, tags, and revision
  expressions before the ref lookup, preventing an implementation branch from
  authorizing itself through a revspec alias.
- **The stable-planning-anchor canary now tests its stated premise.** It uses a
  real declared target at the fixture's parent commit and a locally valid,
  approved workgraph added only on the planning branch, rather than sharing the
  intentionally orphaned/draft-spec fixture used by graph-rejection cases.
- **Partial epic and task updates preserve the complete planning record.**
  Status and delivery-field updates now patch the generic YAML frontmatter
  instead of re-serializing the narrower typed workgraph view. Capability,
  ADR, spec, graph, and future project-owned fields therefore remain intact,
  along with the markdown body and the template's `created` field shape.
- **Planning and implementation gates now enforce one workgraph contract.**
  Canonical task writers retain and require the stable integration target,
  blocked/closed tasks cannot start, spec linking preserves consumer comments
  and key order, and multiline template comments or explicit resolved markers
  no longer masquerade as unresolved spec questions. Duplicate identities
  across canonical and legacy layouts now fail normal validation, and anchored
  parse failures retain their specific graph error. `work start`, full-tier
  task-branch pre-commit, and full-tier task-branch CI all reject an invalid
  visible graph before checking the same anchored task snapshot; planning
  records created on a task branch cannot authorize their own implementation.
  Minimal and standard tiers retain `task/` as an ordinary documented branch
  prefix without requiring absent full-tier records. A focused lifecycle
  canary now exercises the complete brief-to-cleanup route plus material
  replanning, blocker strategy changes, cancellation honesty, and missing-seat
  degradation.

### Changed

- **Repository guide portals are now explicit, source-linked, and verifiable.**
  Standard/full projects receive the concise cross-harness `cf-docs-portal`
  workflow, while the exact-pinned Starlight/Pagefind starter remains absent
  until `codeflow portal setup --path <dir>` adopts it offline. Adopted files
  reconcile through ordinary never-clobber updates and opaque pristine
  baselines; `codeflow validate --portal <dir>` checks bounded evidence claims
  against repository/output bytes without executing project code. CodeFlow
  dogfoods the optional utility under `docs-portal/`. The pinned GFM adapter
  now requires one clean committed runtime/source/media snapshot, reads source
  claims from bounded Git blobs, detects index-masked worktree changes, rebuilds
  broken-source routes as bounded non-searchable current-source error pages
  without walking or republishing history, uses bounded batched Git reads,
  stable no-follow publication copies, raster-dimension limits, and
  locale-independent ordering, neutralizes derived metadata, and
  publishes under recoverable corpus and whole-workflow leases (ADR-0048,
  CAP-015). Canonical routes retain exact NFC case and punctuation while URL
  boundaries encode segments; shared JavaScript/Rust authority fixtures cover
  strict frontmatter, repository URLs, and SHA-1/SHA-256 object IDs. Committed
  non-reserved public files are also snapshot-authoritative. CodeFlow's full
  strict gate now blocks on locked portal tests/build/Rust validation on Ubuntu,
  with the authority/path suite repeated on Windows; generic consumer CI stays
  opt-in. Compressed release embedding keeps the offline starter within the
  ADR-0048 binary budget.

- **Multi-task epics now default to one topology-aware integration branch.**
  Planning creates the shared non-protected target from the intended protected
  branch before task allocation; independent nodes may branch concurrently
  within the host resource budget, dependent nodes branch from the updated
  integration tip only after predecessors land, and landings remain serialized.
  Aggregate verification and both-primary review run on the combined diff
  before one final human-reviewed PR. A different landing shape requires a
  recorded Plan vN rationale and dual approval rather than convenience.
- **Product language and appearance are now explicit, contextual design
  dimensions.** `cf-design` records language/voice and applicable appearance
  modes inside the proportionate `DESIGN_INTENT`, while `cf-editorial-review`
  remains the single shared authority for titles, emoji, fabricated
  personality, and language-quality judgment. New semantic cases protect
  distinct evidenced product voices, established-system collapse,
  localization honesty, preference/persistence verification, and the boundary
  that prevents CodeFlow utility defaults from becoming product design
  authority.
- **Design refinement and asset sourcing now stay bounded and attributable.**
  After a direction is selected, `cf-design` varies only a named unresolved
  material choice and stops when it is settled. An on-demand reference keeps
  inspiration distinct from user evidence, records proportionate rights,
  consent, transformation, and product-use provenance, refuses unauthorized
  private-data uploads, and binds material revision feedback to the exact
  reviewed version without adding a provider catalog or design database.
- **Durable work now has one explicit, adaptable authority and a stable start
  boundary (ADR-0046).** Full-tier scaffolds write independent `EPC-NNN`,
  `SPC-NNN`, and `TSK-NNN` records in one flat Git Markdown workgraph. Tasks
  declare an epic or standalone rationale, direct dependencies, consumed specs,
  and a non-task integration target; specs are allocated and linked by the CLI.
  Historical dual-identity, nested, and `TSK-NNN-NNN` records remain readable.
  `codeflow work start` rejects task-branch targets and proves from the
  merge-base that planning reached the declared target, the anchored record
  declares that same target, and parents, specs, and predecessors are ready; the
  full-tier pre-commit hook and detached CI reuse the same read-only core check,
  and a full-tier task branch with no visible durable record fails locally
  instead of falling through to ordinary commit checks. The
  planning method now makes the discussion → independent duo settlement →
  planning PR → anchored task → implementation/review/ship path and its
  failure routes explicit. Monorepo decomposition, serialized allocation,
  loose-link external trackers, bounded-deviation closeout, spec recall, and
  rebuildable local caches remain part of the authority contract.
- **End-to-end and concurrent UI verification now own their real boundaries
  (ADR-0044).** Material changed journeys map the affected frontend, service,
  state, external-seam, infrastructure, runtime, outcome, and recovery path,
  then prove one faithful vertical run instead of promoting disconnected unit
  and mocked layer tests to E2E. Concurrent Playwright work receives
  task-owned isolated browser state, non-overlapping endpoints where
  applicable, namespaced test data, run-scoped artifacts, and verified
  teardown; headed or Computer Use work cannot commandeer the operator's
  browser or active desktop. Consuming projects choose their allocator/ranges
  and cleanup commands during customization. Worktree closeout now inventories
  and promptly removes proven-landed entries while preserving active, dirty,
  and unproven work with ownership evidence; `codeflow status` classifies
  linked worktrees and unattached local branches from ancestry, squash-safe
  patch equivalence, and dirtiness without deleting them. Three behavioral
  canaries and deterministic contract tests pin the contract without adding a
  browser, port broker, or deletion command.
- **Product and interface design direction is now an explicit proportionate
  contract (ADR-0043).** Standard/full scaffolds gain a mirrored `cf-design`
  skill. Material new surfaces settle evidence-grounded `DESIGN_INTENT` inside
  Plan vN; cosmetic and established-system work can collapse without ceremony.
  The stable Claude judgment role leads intent, Codex challenges feasibility
  and fidelity, both approve the plan, and review distinguishes evidenced drift
  from taste. Behavioral and rendered cases exercise direction selection,
  operator precedence, evidence-grounded design-choice review, accessibility, and
  fidelity without tying the doctrine to a concrete model release.
- **Coverage now requires test integrity, not only a percentage.** The shared
  quality contract and independent reviewer reject tautologies, production
  logic copied into test oracles, mock-only wiring checks, weakened assertions,
  and production branches added only to manufacture coverage. `CF-QA-009` and a
  behavioral canary keep the 80/90 coverage policy from being gamed while
  preserving explicit contract fixtures and stable named invariants.
- **Performance, scale, and concurrency review now has an operating-shape
  canary.** Review checks complexity/N+1 access, bounded work and memory,
  backpressure/cancellation, async blocking, state synchronization and races,
  idempotency/retry amplification, and resource cleanup. Measurement,
  load/stress, or concurrency evidence is required when material risk earns it,
  without imposing ceremonial benchmarks on unaffected paths. A targeted
  native Fable/Sol
  [diagnostic](docs/verification/model-role-quality-diagnostic-2026-07-26.md)
  exercised this case together with managed-primary resolution and
  test-integrity review; it is retained as diagnostic evidence, not binding
  qualification.
- **Stable model roles now resolve through qualified project bindings
  (ADR-0041), and verification is explicitly layered (ADR-0042).** Durable
  orchestration names the Claude judgment and Codex engineering roles rather
  than a model release. The managed ensemble still selects the current Fable
  and Sol bindings, while standard/full projects may atomically reference
  locally approved binding IDs from `.codeflow/model-selection.json`.
  `codeflow doctor --check model-bindings` fails closed on invalid, drifted, or
  pseudo-duo selections without partial fallback. Role-tagged evals, a
  seat-collapse regression, deterministic-red verification, and bounded-history
  craftsmanship cases protect the boundary. CodeFlow will enable GitHub CodeQL
  default Rust analysis after public launch; consuming projects select SAST
  from stack/hosting evidence during customization, so no CodeQL workflow is
  imposed by the portable scaffold.
- **Model and harness evolution now has qualified bindings (ADR-0039).**
  Durable independent planning, Claude-led design, capability-routed
  production, cross-lineage review, integrated Claude judgment, and graceful
  degradation remain unchanged. One managed ensemble record now owns concrete
  model selectors, effort policy, worker classes, and escalation triggers; a
  universal native-harness capability catalog replaces the evaluator's
  hard-coded harness enum. Catalog entries mean capability support, not
  concrete model-binding qualification. Composable diagnostic packs select existing cases
  without weakening full promotion. Approved full-suite results can emit
  compact non-secret local binding records, and a fourteenth doctor check
  reports requested/observed contradictions plus observable harness-version and
  settings drift without launching, scraping, or automatically routing a
  model. Catalogs may name only code-allowlisted version probes; they cannot
  supply commands or arguments.
- **Multi-task plans now settle a durable, non-executable task graph
  (ADR-0040).** Both primary seats approve explicit nodes, dependencies, and
  genuine evidence-guarded decisions. Task records use canonical `depends_on`
  with a read-compatible `dependencies` alias, while `validate --docs` rejects
  malformed IDs, conflicting fields, dangling/self/duplicate edges, and cycles
  without becoming a scheduler. Material graph and cross-task contract changes
  require Plan vN+1; ordinary in-node detail does not. Verification planning
  may earn property tests, targeted mutation testing, or a project-owned
  architecture fitness check from explicit risk and oracle evidence, without
  imposing optional tools on consuming projects.
- **Critical-path focus now preserves bounded quality work (ADR-0038).**
  Materiality and the dependency presently controlling the accepted outcome are
  treated as related but distinct. Models protect all quality and safety gates,
  fix clear safe in-scope improvements when focused validation is bounded, and
  consolidate only genuinely uncertain secondary observations for one natural
  Claude+Codex checkpoint. Retained work is tracked once at an existing
  planning altitude with evidence and a deterministic revisit event; vague
  time-only deferral and issue-per-nit churn are rejected. Paired `CF-QA-005`
  fixtures guard both material-first execution and the no-reflexive-deferral
  case. Blocked work now advances only through changed evidence or a safe
  outcome-preserving strategy; the operator is asked only for a real external
  dependency or owner decision, while deterministic and safety gates remain
  non-bypassable.
- **Duo execution is now capability-routed per task (ADR-0035).** Independent
  Claude+Codex discovery, Claude-led design, versioned joint approval, native
  interactive transport, evidence gates, and integrated Claude judgment remain
  mandatory. The host now records each task's producer and cross-lineage
  reviewer from verified task fit, tools/context, independence, routing,
  resources, and observed native usage signals; a seat/lineage reassignment
  invalidates approval. Explicit host/peer/worker roles prevent nested duos,
  unverified workers are unavailable rather than guessed, and units authored
  by the Claude judgment primary receive independent Codex review without
  calling the primary's integrated judgment self-independent.
- **Reviews and proactive discovery now act by materiality (ADR-0034).**
  Models still search broadly, but substantiate candidates and lead with
  consequential strategic, architectural, structural, correctness, security,
  robustness, operability, and meaningful edge/error concerns. Severity stays
  separate from confidence and remediation effort; repeated small symptoms may
  form one systemic finding, while cosmetics remain minor and non-blocking.
  Evidenced out-of-scope material risk is escalated or routed to one tracked
  item without silent scope expansion or external mutation. Paired `CF-QA-005`
  cases guard ordering, approval with nits only, systemic diagnosis,
  effort-neutral severity, CVSS-aligned security vocabulary, and proactive
  routing without issue farming.
- **Web UI verification now routes browser mode and evidence by claim.**
  Routine deterministic Playwright E2E may run its browser headless while the
  Claude/Codex seats remain native interactive sessions. Behavioral assertions,
  same-environment visual comparisons, console/network evidence, and bounded
  failure traces are kept distinct; screenshots and automated accessibility
  checks are not treated as complete proof. Headed mode is reserved for claims
  that materially need live rendering, browser chrome, or debugging, and
  Computer Use remains a fallback outside the controlled page. A `CF-QA-002`
  canary guards this distinction across future model bindings.
- **Catastrophic-action protection is cross-platform and non-relaxable
  (ADR-0033).** The deterministic shell guard now covers Linux/WSL2, macOS, and
  native Windows Bash/PowerShell events; blocks protected-root deletion plus
  destructive disk, recovery, and system-permission operations; and cannot be
  downgraded through project policy. Common privilege, shell-command, and
  environment wrappers cannot bypass the classifier, while equivalent
  task-scoped project paths remain autonomous. Model agreement and automatic review are
  explicitly not human authorization. Secret-store denies now reach Claude's
  OS-sandboxed subprocesses. CodeFlow adds a native Windows x64 artifact,
  PowerShell installer, elevated Codex sandbox default, Windows build/test
  lane, and configurable test-command shell with dependency-free native
  defaults; WSL2 remains the required route
  for Claude work that needs OS-enforced containment. A release checklist now
  makes source/security gates, native platform and installer canaries,
  harness/model qualification, public-network installation, and rollback
  evidence explicit before downstream Agent OS work begins.
- **Adaptive test setup is safe and release-complete (ADR-0031).** Existing
  populated or malformed configs are preserved unless a reviewed template is
  explicitly applied with `--replace`; malformed configured pre-push gates now
  violate instead of skipping. `codeflow test setup` can list/apply templates
  embedded in the binary and append monorepo targets, while automatic detection
  remains root-only. Setup no longer emits dormant structural rules, doctor
  warns on legacy blocks without enabling the parked validator, and the schema
  now states the actual `CI` and quick/essential/full contracts; configs with
  empty mode maps or non-public mode names now fail validation.
- **Substantial prose now has a contextual editorial gate (ADR-0032).**
  Standard/full scaffolds gain a mirrored on-demand `cf-editorial-review` skill
  for documentation, ADRs, proposals, release notes, PR narratives, operator
  communications, and user-facing copy. It preserves technical meaning and the
  consuming project's documented voice, removes unsupported certainty,
  sycophancy, inflation, and obstructive formatting, and never invents a
  persona. The always-loaded contract gains only the voice hierarchy; detailed
  smells remain progressive disclosure. Six `CF-OUT-002` cases protect
  technical prose, operator updates, voice, PR formatting, contextual emoji,
  and legitimate punctuation/terms/lists. No Vale, AI detector, or lexical
  blacklist is added.
- **Right-sized design and code are now a blocking duo gate (ADR-0030).**
  Both seats must approve design proportionality; every routed producer
  first-verifies the smallest coherent, idiomatic change, the other lineage
  reviews it independently, and the directly invoked Claude judgment primary
  owns the integrated quality verdict. Speculative features, abstractions,
  configuration, dependencies, compatibility paths, dead code, and other
  complexity without a current requirement or evidenced risk yield
  `changes_requested` even when tests pass. The same gate rejects brittle
  under-design: duplicated business rules, unexplained hard-coding, swallowed
  errors, missing accepted edge cases, or one-off UI that bypasses an existing
  design system. Paired regression fixtures test both directions without
  treating raw line count or a framework pattern as a quality target. A context
  canary distinguishes disposable experiments from durable multi-team systems,
  requires clarification when that distinction is material, and uses safe
  reversible defaults rather than project size as an architecture rule.
- **Primary model seats now start at high and escalate by evidence (ADR-0028).**
  Cross-harness calls still target Fable and GPT-5.6 Sol (or their strongest
  supported successors) as the two independent reasoning seats. High is the
  default; xhigh is reserved for defined complexity, ambiguity, security,
  long-horizon, disagreement, or failed/stalled-high triggers. Fable owns
  Opus medium/high tool-operation routing, and Codex owns any verified native
  Sol-class medium/high worker routing without transferring either primary
  seat's judgment, approval, implementation, or verification responsibility.
  Codex project settings pin high as the fallback, while Claude-hosted plugin
  turns pass high/xhigh explicitly so a user-level default cannot silently
  change the observed peer binding.
- **Claude can use trusted native tools that require host state (ADR-0029).**
  The fail-closed sandbox remains the default, but Auto may classify one
  unsandboxed retry after a sandbox-boundary failure for a trusted installed
  tool such as the official Codex plugin. This avoids granting arbitrary Bash
  write access to `~/.codex`; destructive, privileged, secret, credential, and
  private-network controls remain in force, and arbitrary unsandboxed commands
  remain outside the workflow contract.
- **Model-effort comparisons now prove the binding and isolate the variable.**
  The evaluator can record harness-observed model/effort evidence and fixed peer
  seats, reject configuration drift outside the declared experiment variable,
  recompute raw results safely, report latency/token/cost distributions, and
  block promotion on any case regression, incomplete run, validity flag, or
  missing identified human approval. Existing protocol-conformant schema-version
  1 results remain valid when they are not used for strict promotion comparisons.
- **Coverage scope must have a measurable denominator.** Configuration now
  rejects the former `changed_files` scope because an invalid base ref could
  make it measure an empty file set and pass vacuously. Migrate affected rules
  to `per_file`, `per_package`, `per_module`, or `global`.
- **Harness settings now close the remaining credential and destructive-action
  gaps (ADR-0026).** Codex explicitly selects full public subprocess networking,
  denies high-confidence workspace key/certificate files plus its raw auth
  store to sandboxed subprocesses, and pins the built-in secret-bearing
  environment filter. Claude removes raw Anthropic, OpenAI, and AWS credentials
  from sandboxed Bash, protects transcripts, memory, session state, settings,
  and auth caches with narrow home-directory denies that leave the official
  plugin runtime readable, and asks before restore, common checkout-discard,
  force-push, flag-based remote-delete, prune/mirror, forced local-branch
  reset/move, and delete forms.
  Protected-branch hooks remain the backstop for deletion-refspec syntax that
  Claude's permission grammar cannot safely express. Public research, brokered
  tools/MCPs, Auto-at-CLI scope, and loopback UI testing remain available.
  Codex's automatic reviewer is a reviewer subagent, so consumers that require
  a human for every sandbox escalation must select `user` in the project or
  launch override and enforce it through managed requirements where available.
- **Linked worktrees no longer rewrite write-once document baselines.** Existing
  non-JSON user-owned files now keep their original shipped baseline and
  manifest hash, so a different worktree directory name cannot create unrelated
  scaffold drift. A missing baseline still self-heals from the current scaffold
  without touching the live file; newly adopted files and schema-versioned JSON
  keep their existing update behavior.
- **The Claude+Codex duo is host-neutral and evidence-gated (ADR-0023).**
  Claude Code hosts through the official Codex plugin; Codex App/interactive
  CLI hosts through a task-scoped interactive Claude CLI in tmux. Both models
  independently research/analyze/plan and Claude leads design. ADR-0035 now
  routes production and cross-lineage review per task while preserving the
  selected Claude primary's integrated judgment. A versioned
  dual-approved plan, reproducible evidence ledger, scenario-first tests, an
  80% coverage floor/90% target where measurable, UI-driven validation, and
  independent security review now form one shared quality contract.
- **Reverse-lane completion no longer relies on terminal stability.**
  Task-scoped Claude Stop/StopFailure hooks and `last_assistant_message` are
  the protocol signal. The new `codeflow hook delegate-turn` handler writes an
  owner-only terminal result exactly once, permits an exact retry to re-signal,
  rejects conflicting evidence, and releases only the matching tmux waiter;
  pane capture is limited to the dedicated task session after completion or
  bounded diagnosis. Doctor now checks both directions' inspectable
  prerequisites while requiring retained interactive canaries.
- **Batch automation no longer implies cross-vendor assurance.** The seeded
  Claude workflow replaces its misleading `duo` preset with an explicitly
  `single-vendor-assurance` preset and rejects old duo/plan-align semantics.
- **Codeflow's full local gate now enforces 90% aggregate line coverage.**
  `cargo llvm-cov --workspace --summary-only --fail-under-lines 90` runs as a
  full-mode target, matching the independent CI coverage threshold.
- **Security: four enforcement-bypass fixes from the pre-release review.**
  The destructive-command guard now tokenizes `rm` instead of pattern-matching,
  so `rm -r -f /`, `rm --recursive --force /`, `rm -rf -- /`, and `rm -rf $HOME`
  are blocked, not just the exact `rm -rf` spelling. The commit-standard merge
  exemption keys off a real merge (`MERGE_HEAD`), not the subject text, so a
  one-parent commit named `Merge ...` is fully checked. `codeflow update` rejects
  a manifest `dest` that is absolute or escapes the repo with `..`, closing an
  arbitrary out-of-repo file deletion via a tampered manifest. The pre-commit
  secret scan classifies each assignment by its own value, so a placeholder word
  in a comment no longer suppresses the first live quoted credential later on the
  line. Subsequent pre-release slices closed the additional destructive-command
  spellings and shell wrappers, scaffold symlink traversal, and vacuous
  coverage-exception handling found by the second pass. The shipped scaffold's
  accepted policy remains configurable and defaults dependency/security review
  to `warn` (ADR-0016); this repository now hardens both keys to `block` after
  triage. Only `secret_scan` is never relaxable in the shared product policy.
- **Codeflow's own Rust verification now includes format and documentation
  gates.** Local `codeflow test` and the independent CI Rust job both enforce
  `cargo fmt --check` and warning-free rustdoc alongside tests and clippy; the
  parity guard compares the complete Rust command set.
- **Dependency security is blocking in this repository.** The project-owned
  `git.security_review` and `git.dep_audit` levels are `block`, while the
  consumer scaffold keeps ADR-0016's configurable `warn` default.
- **Coverage thresholds now fail the test gate.** A configured per-file
  coverage threshold that a measured file misses fails `codeflow test --mode
  full` and the integrate gate, instead of being collected and silently
  ignored. A run with no coverage data recorded stays informational.
- **PR bodies now demand test evidence and digestible bullets.** The shipped
  PR template's `Verification` section becomes `## Testing` — required for any
  code change, carrying pasted test-summary output, the coverage number, the
  new tests added, manual/e2e evidence, and a plain "not tested" statement
  ("tests pass" as prose is a claim, not evidence). Every section is short
  one-line bullets — no paragraph-walls. The rule ships in the template, the
  AGENTS contracts (full and minimal), and `cf-ship`.
- **Enforcement is the floor; the tiers scale project-management (ADR-0019).**
  The `--minimal` tier now installs the complete four-plane enforcement floor,
  not just the pre-commit secret scan: the `commit-msg`, `pre-push`,
  `pre-merge-commit`, and `reference-transaction` git hooks, the scaffolded CI
  workflow (`codeflow-ci.yml`), the in-session `git-guard`/`exec-guard` +
  orient/summary hooks (`.claude/settings.json` and the `.codex/` starter), and a
  new lean `CLAUDE.md` all moved into the minimal tier alongside the armed policy
  it already shipped. `--standard` and `--full` are unchanged; they still add the
  method (cf-* skills, reviewer agents, the pipeline), the traceability spine, and
  project-management on top. The change is additive — nothing is removed from any
  tier.
- **Existing `--minimal` installs gain the enforcement floor automatically on
  their next `codeflow update`.** The update reconciliation installs manifest
  entries that are in-tier but missing on disk, so an old-minimal repo's next
  update adds the moved hooks, CI, settings, and Codex starter and records them —
  no re-init required.
- **The original commit standard is restored and block-enforced (ADR-0020).**
  The subject description is capped at 50 chars and the whole subject line at 72,
  and a commit body is again only `-` bullets (at most 3, each ≤ 72 chars) plus
  an optional `BREAKING CHANGE:` footer — a prose "story" body is now a blocked
  mistake. The rules ship armed at every tier via five new `git` policy keys
  (`commit_desc_max_len`, `commit_subject_max_len`, `commit_body`,
  `commit_body_max_bullets`, `commit_body_bullet_max_len`), added to an existing
  `policy.json` with their defaults on the next `codeflow update`.
- **A contract-surface tripwire nudges breaking-change discipline (ADR-0020).**
  A new `git.breaking_watch_paths` key (path globs, default empty) makes the
  commit-msg check — and `codeflow ci`, which reuses it — emit a WARN (never a
  block) when a commit touches a declared contract surface without a `type!:`
  subject marker or a `BREAKING CHANGE:` footer. Detection of a break stays a
  judgment call; the glob only prompts a confirm.

### Added

- **Bounded interactive review documents (ADR-0049, ADR-0050, ADR-0052).** Standard/full
  scaffolds gain the cross-harness `cf-present` skill and managed public
  document, primitive-token, and history schemas. The new `codeflow present`
  surface opens, updates, lists, resumes, exports, closes, and clears immutable
  local review sessions and delivers stable feedback envelopes at least once.
  One loopback-only authenticated service and a CodeFlow-owned isolated browser
  profile render a closed accessible block catalog with light/dark utility
  modes, inert HTML sandboxing, strict optional project primitive tokens, and
  self-contained read-only export. Versioned state is owner-private, project-
  keyed, and quota bounded; browser-owned profile/cache and small runtime
  controls use a separate derived owner-private root. Cleanup is identity-
  scoped; event recovery uses one bounded
  opened handle, diagram count/source/enhancement are capped, and native
  adapters use trusted platform paths, exact process identity, a shared
  allowlist-only child environment, creation-only Windows ACL hardening, and
  read-only owner/DACL verification. No daemon, remote viewer, product UI
  framework, or documentation portal is introduced. Native-path project keys,
  serialized creation quota enforcement, collection cardinality, versioned feedback
  resolution, exact re-anchoring/visible orphan states, and fail-closed orphan
  process recovery keep the bounded contract explicit. Native platform and
  browser qualification remains the explicit CAP-016/TSK-007 release boundary.
  All durable growth now reserves capacity before publication under one
  project-to-session-to-runtime-control lock order, while exact retries and
  cleanup remain usable for legacy over-quota state. Exact accepted and terminal
  feedback retries converge without growth; retention recomputes after each
  eviction; selected cleanup reports a retained named session while remaining
  isolated from unrelated state; interrupted creates, atomic temporaries, and
  trash recover only from exact names plus matching transaction proof; and relative Unicode export remains
  create-new and owner-private. One strict feedback ledger rejects impossible
  transitions; browser cleanup re-qualifies identity before forced escalation;
  exports are owner-private from creation; and browser limits mirror the
  server's note, text, selector, and payload bounds. Browser launch is serialized
  around one consumed record per attempt, reused PIDs take bounded exact-marker
  recovery, Windows proves real profile-resource release without assuming a
  POSIX lock file, every Windows append/lease file is private at creation, and
  Unix state-root environment paths cannot resolve into the worktree.

- **Transport-neutral durable delegate lifecycle (ADR-0036).** New
  `codeflow delegate init|arm|wait` commands and a schema-v2
  `hook delegate-turn --state-dir` mode drive a delegated harness turn through
  durable owner-only records — ready, armed, accepted, terminal — with SHA-256
  prompt binding, one outstanding turn, deterministic terminal correlation,
  and durable poisoning for session restarts, interrupts after acceptance,
  and ambiguous retries; a duplicate or digest-mismatched prompt submission
  is instead blocked (hook exit 2) with run state preserved. The binary
  never launches a harness or delivers a prompt; the host keeps transport,
  and the current event adapter is Claude hooks. Schema-v2 waiting is
  file-polled and tmux-free, while the legacy `--result` mode is
  byte-compatible and unchanged. State lives outside Git
  worktrees, carries digests instead of prompt text, caps raw hook input before
  parsing, and fails closed on native Windows (use WSL2). ADR-0037 narrows the
  transportable prompt boundary to non-empty canonical UTF-8 text with
  internal LF, no terminal line break, and no other control characters, rejected
  before turn creation, and
  pins a bounded paste-to-Enter settle with only one diagnosis-proven retry.
  `codeflow doctor`
  gains a thirteenth,
  Fail-severity `delegate-roundtrip` check that runs the installed binary
  through the full synthetic lifecycle — rebuild and reinstall the CLI
  (`cargo install --path crates/codeflow-cli`) before it can pass.
  The dated PR1 canary record covers the active Stop-hook set, a Unicode
  normalization case, live `prompt_id` binding, AskUserQuestion, and
  permission-response routing on the available macOS arm64 host. The reusable
  sibling-hook rejection procedure, full fake-TUI stress matrix, and broader
  native-platform evidence remain PR2/release gates.
- **Native-interactive model/harness qualification (ADR-0027).** Standard/full
  scaffolds gain `/cf-evaluate-model`: stable requirement-to-source-to-case
  traceability, balanced regression/capability cases, exact disposable fixture
  materialization, expected-versus-observed scoring, repeated full trials,
  baseline comparison, and marker+run-ID-gated cleanup. Subject models remain in
  supervised native Codex or Claude sessions with their actual tools; no
  headless model runner, CLI subcommand, CI model call, hard token-deletion
  gate, or generic cleanup surface is introduced.
- **A lazy PR body now fails CI mechanically.** When `codeflow ci` is given a
  PR/MR body, it checks the body's structure against three new `git` policy
  keys: `pr_sections` (level, default `block`) governs the check;
  `pr_required_sections` (default `["Summary", "Changes"]`) are headings every
  PR body must carry with real content — a section holding only template
  comments and bare `-` bullets counts as missing; `pr_code_sections` (default
  `["Testing"]`) are required only when the commit range touches non-docs
  files (docs-only = every changed path is `*.md`, `*.txt`, `LICENSE*`,
  `docs/**`, or a `.github` template — anything else, or a range whose files
  cannot be listed, counts as code). Leftover template placeholders — the
  paste-your-output stub, table rows of empty cells, bare `- CAP-`/`- EPC-`
  bullets — draw a warning naming their line, never a block. A run without a
  PR body skips the check, so local `codeflow ci` is unchanged; an existing
  `policy.json` gains the three keys with their defaults on the next
  `codeflow update`.
- **`codeflow policy explain` / `policy show` — the policy file is fully
  discoverable from the binary.** `explain` renders the complete
  `.codeflow/policy.json` key schema — every key's type, default (rendered live
  from the built-in defaults), valid values, purpose, and sharp edges (e.g.
  `allow` and `off` are both inactive levels) — grouped top-level/git/security;
  `show` prints the EFFECTIVE policy: each key's current value, whether it comes
  from the project file or the built-in default, and a loud flag on invalid
  values. Consumers get only the binary, so both need no source access; the
  schema registry is pinned to the policy struct's serde fields by a
  drift-guard test, so a new key cannot ship undocumented.
- **Strict `policy.json` validation — invalid config fails loudly, never a
  silent default-revert.** Malformed JSON, an unknown key, a wrong-typed value,
  an invalid enum value, or an unparseable `commit_ticket_pattern` regex is now
  a hard error naming every offending key, its value, and the valid set (e.g.
  `invalid value 'worn' for git.commit_ticket_required; expected one of: off,
  warn, allow, block`) — surfaced at the commit-msg git hook (exit 1),
  `codeflow ci` (exit 2, nothing verified), and `codeflow validate` (exit 1).
  Previously one invalid value made the whole file fail-parse and silently
  reverted EVERY key to the built-in defaults — including keys a consumer had
  hardened past them. The enforcement loaders keep their fail-safe fallback;
  the loud check is an explicit pre-check at those three surfaces.
- **Opt-in footer trailers and required footers, strict by default (ADR-0020).**
  The commit body stays bullets + a `BREAKING CHANGE:` footer only — every other
  trailer blocks — but a project can now open specific escape hatches via
  `policy.json`, all empty by default: `git.commit_footer_tokens` *allows* named
  trailers (e.g. `Signed-off-by`), and `git.commit_required_footers` *requires*
  them on every commit (e.g. DCO sign-off). Deliberately no populated default —
  in an agent-driven repo every default-allowed trailer is a slot an agent fills.
  Even when `Co-authored-by` is opted in, the `ai_attribution` rule still blocks
  an AI value; a human co-author passes only when the token is opted in.
- **Opt-in ticket references with an allow-vs-require split (ADR-0020).**
  `git.commit_ticket_keys` (default empty) *allows* ticket trailers (e.g. `Refs`,
  `Closes`); `git.commit_ticket_required` (default `off`; `warn`/`block`) makes a
  matching ticket *required*; `git.commit_ticket_pattern` (e.g. `^PROJ-\d+$`)
  constrains the value — a present-but-malformed reference blocks even when
  optional. Merge/revert/fixup commits are exempt; the git hook and `codeflow ci`
  enforce it identically.

- **Two working principles in both the minimal and full agent contracts.**
  *"Think independently — not a yes-man"*: a request, opinion, claim, or proposed
  approach — the operator's included — is owed analysis and evidence, not
  agreement; the operator still makes the final call, but agreement without
  examination is a failure mode, not deference. *"Think in depth, not at the
  surface"*: chase the implication chain ("and therefore? …") to the fundamental
  that decides the matter, and trace how each order ripples across the related
  domains, not just the immediate area.

### Fixed

- **Brownfield setup preserves the consuming project's decision history.**
  Init and update no longer add the starter stack `ADR-0001` when an existing
  repository already has ADRs. Test setup now installs the JSON schema beside
  generated `.codeflow/test-config.json` files, uses a correctly relative
  `$schema` reference, and repairs a missing schema without replacing a
  populated project configuration. Repositories initialized by an affected
  prerelease build should delete its duplicate starter ADR once; subsequent
  updates leave it deleted. Previously populated test configs are intentionally
  not rewritten; regenerate one to adopt the corrected `$schema` reference.
- **Public contribution and security guidance is release-neutral.** The
  contributor path now names the complete repository gate, including coverage
  and model-evaluation contracts, and the security policy supports the latest
  released major line without going stale at the v3 cut.
- **CI verifies security-tool downloads before executing them.** The shipped
  workflow and CodeFlow's own perimeter pin the official SHA-256 digests for
  Gitleaks and OSV-Scanner, fail closed on a mismatch, and retain the exact
  release versions already validated by the repository.
- **Auto-detected test configs now run with each stack's declared baseline
  tools.** Rust no longer assumes a project-defined nextest `full` profile;
  Node, Go, and Python no longer silently require optional report or coverage
  plugins that detection did not prove were installed. Generated targets use
  conservative native commands and leave richer reports and coverage to
  project customization. The empty-stack message also stops recommending an
  unimplemented `--add-target` flag, and coverage output now states honestly
  that configured thresholds affect the gate verdict.
- **Dependency advisories and maintenance debt.** `anyhow`, `git2`, and
  `quick-xml` move to versions that clear the active RustSec advisories; the
  unmaintained `serde_yaml` parser is replaced by the maintained
  `serde_yaml_ng` continuation behind the same source alias (ADR-0022).
- **Test code no longer mutates process-global environment variables from
  parallel unit tests.** CI detection is injected into the runner tests, and
  child Git-environment removal is verified through `Command` construction,
  eliminating the Rust-2024-unsafe `set_var` / `remove_var` calls and the race
  they represented.
- **Public API documentation builds warning-free.** Broken intra-doc links in
  the hook and policy modules and one redundant CLI link are corrected; the new
  rustdoc gate prevents regression.
- **Live reviewer definitions match the shipped scaffold sources.** The
  repository's `cf-reviewer` tier guidance and `cf-security-reviewer` CI posture
  now carry the same corrected doctrine as newly initialized projects.
- **The full-history secret scan ignores one reviewed synthetic fixture by its
  exact fingerprint.** The historical line demonstrated scanner behavior and
  contained no credential; `.gitleaksignore` suppresses only that immutable
  finding rather than weakening a rule.

## [2.1.0] - 2026-07-12

### Added

- Cross-harness skills: the `cf-*` skills install to `.agents/skills/` (read by
  Codex) alongside `.claude/skills/`, so one skill set serves multiple coding
  CLIs. Claude Code merged custom commands into skills (v2.1.101); codeflow now
  ships skills only.
- The commit-msg gate flags a mis-cased `BREAKING CHANGE:` / `BREAKING-CHANGE:`
  footer, so a breaking change is never silently downgraded to a minor bump.

### Fixed

- `codeflow update` no longer silently discards merged-in user edits to a
  managed file on the *next* update. After a clean 3-way merge the manifest now
  records the pristine shipped hash (restoring the invariant `recorded ==
  hash(baseline)`), so a merged file stays classified user-modified and is
  re-merged rather than overwritten with the shipped version.
- `codeflow update` reconciles orphaned managed files: an artifact removed or
  renamed upstream (e.g. a command that became a skill), and its baseline and
  manifest record, is pruned when unmodified instead of lingering and colliding
  with its renamed replacement. User-modified and user-owned files are never
  deleted (ADR-0011).

### Changed

- Shipped scaffold content and documentation corrected for the commands→skills
  rename and for install/enforcement accuracy ahead of the public release.

## [2.0.0] - 2026-07-03

2.0.0 is a complete Rust rewrite of codeflow. The 1.x line (a shell/Node tooling
set) shares no code with it and is preserved at the `v1-final` tag.

### Added

- Single `codeflow` binary (crates `codeflow-core` + `codeflow-cli`) with an
  embedded scaffold, installed into any repo via `codeflow init`.
- Policy-driven git enforcement across four planes — git client hooks, an
  in-session PreToolUse guard, CI, and remote branch protection — all reading a
  single `.codeflow/policy.json`.
- Scaffolding with three ownership classes (managed, managed-region,
  user-owned), 3-way-merge `codeflow update`, and version-skew detection.
- Knowledge model: capabilities registry, ADRs, epics/specs, an automatic
  ledger, and `codeflow recall` full-text search.
- Composable pipeline workflow (build → independent review → verify) and
  cross-vendor delegation (`cf-delegate` / `cf-consult`).
- CLI surface: `init`, `update`, `test`, `validate [--docs]`, `status`,
  `recall`, `orient`, `doctor`, `integrate`, `remote`.
- cargo-dist release pipeline with prebuilt binaries for macOS (arm64/x64) and
  Linux (x64) and a shell installer.

[Unreleased]: https://github.com/sathyassn/codeflow/compare/v3.0.0...HEAD
[3.0.0]: https://github.com/sathyassn/codeflow/compare/v2.1.0...v3.0.0
[2.1.0]: https://github.com/sathyassn/codeflow/compare/v2.0.0...v2.1.0
[2.0.0]: https://github.com/sathyassn/codeflow/releases/tag/v2.0.0
