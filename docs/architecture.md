# codeflow: architecture

<!-- HOW layer. Updated only inside the ship flow, in the same PR as the code,
     when an ADR declares architecture impact. Link to decisions by ADR id;
     never duplicate their content here.
     When an area outgrows this file, graduate it to docs/architecture/<area>.md
     and leave a one-line pointer behind. -->

## Concept

One Rust binary installs the AI-development discipline layer into any
repository: it scaffolds the rules, enforces them while agents work, verifies
the result, and remembers why, while the harness does the developing.

```cf-stage
SCAFFOLD | init and update seed the rules @accent
->
ENFORCE | git hooks · guards · ci from one policy source
->
VERIFY | codeflow test · validate
->
REMEMBER | ledger · records · recall @positive
caption: the harness (Claude Code, Codex, Grok Build, …) does the developing
```

The consuming repo is its own first consumer, so `assets/` is as much the
product as the code: the scaffold content this repository ships is the same
content it runs under.

## Architecture

A three-crate Cargo workspace builds one binary with the scaffold and
presentation renderer embedded; enforcement is structural: four planes read
one policy source, so no single harness is a required trust anchor.

```cf-stage
crates/codeflow-core | scaffold · enforcement · records/knowledge · support @accent
crates/codeflow-present | bounded local review sessions (ADR-0049)
assets/ | base scaffold · docs-portal starter
->
crates/codeflow-cli | thin clap dispatcher over 22 subcommands · rust-embed folds assets/ in
->
codeflow | one binary with the scaffold and presentation renderer embedded @positive
caption: the cli calls core and present; the scaffold this repository ships is the scaffold it runs under
```

```cf-stage
.codeflow/policy.json | one source of truth @accent
->
git client hooks | pre-commit · commit-msg · pre-merge-commit · reference-transaction · pre-push
PreToolUse guards | git-guard · exec-guard, in-session
codeflow ci | the same git standards, server-side
remote protection | where the host arms it
->
protected branches | human-merged PRs on evidenced-green checks @positive
caption: local planes are fast feedback, CI and remote protection are the authoritative perimeter
```

`codeflow-core` owns the discipline engine, `codeflow-present` owns bounded
local review sessions, and `codeflow-cli` is a thin dispatcher: `main.rs` is a
clap command surface over 22 subcommands
(`init`, `update`, `hook`, `git-hook`, `orient`, `test`, `validate`, `ci`,
`status`, `integrate`, `doctor`, `policy`, `recall`, `remote`, `epic`, `task`,
`spec`, `work`, `delegate`, `estimate`, `present`, `portal`), most a small handler in `cmd/` that
calls into core, while `init`/`update` dispatch inline in `main.rs` to the
scaffold module; `embedded.rs` embeds `assets/` via rust-embed (debug builds
read `assets/` from disk for instant scaffold iteration).

## Technical

Per-area depth: engine internals, the shipped scaffold, and the docs layer.

### engine: `crates/codeflow-core` + `crates/codeflow-cli` + `crates/codeflow-present`

Core modules grouped by responsibility:

- **Scaffold** (`scaffold/`): `init`, `update`, manifest, 3-way merge, and the
  ownership classes below; sourced from the rust-embed asset provider.
- **Enforcement** (`hooks/`, `security/`, `git/`, `delegate.rs`,
  `integrate.rs`, `remote.rs`): the `git-guard` and `exec-guard` PreToolUse
  handlers and git-client hook stages, the dual-mode `delegate-turn` adapter
  (legacy `--result` record-and-signal plus the schema-v2 lifecycle backed by
  the transport-neutral `delegate.rs` state machine, ADR-0036), the secret
  scanner, git conflict detection + CI wait, the flock-guarded `integrate`
  primitive with its gate-context token, and the GitHub remote-protect
  adapter.
- **Records / knowledge** (`models/`, `ledger/`, `workgraph/`, `validate/`,
  `capability.rs`, `recall.rs`, `registry.rs`): frontmatter models, the JSONL
  ledger, the work graph, `validate` (+ the `--docs` referential-integrity
  lint, including structural task dependency identity/reference/cycle checks),
  the capability registry parser, FTS5 recall, and the cross-repo registry.
- **Support** (`doctor/`, `settings/`, `status.rs`, `testing/`, `file_lock.rs`,
  `error.rs`): the doctor check table (15 checks: hooks, claude, codex, grok, config,
  permissions, network, delegates, qualified model bindings, delegate-roundtrip, repo-integrity,
  ci-perimeter, managed-drift,
  customization, test-config), including bidirectional delegate readiness
  (Codex auth/MCP, Claude plugin/MCP, and tmux prerequisites; live interactive
  canaries remain outside the binary) and a sentinel-based consuming-project
  customization nudge, structured settings merge,
  generated status views, the test-gate engine, path flock, and pruned error
  types.

The test gate evaluates file and aggregate coverage rules through one verdict.
`changed_files` rules are rejected at config load because standalone test runs
have no explicit comparison base (ADR-0021); silently evaluating an empty set is
not a supported degradation. Child stdout and stderr are drained into bounded
tail buffers, with truncation recorded on each target result.
Setup is an adapter over the same typed config boundary: root-only detection,
release-embedded templates, and explicit target append all use the deterministic
writer and colocated schema. Automatic setup may fill an absent/empty config but
never replaces populated or malformed project intent. The schema retains legacy
`structural` blocks for compatibility while doctor labels them unenforced; the
prescriptive structural validator remains outside the gate (ADR-0031).
Codeflow's own full local gate additionally runs `cargo llvm-cov` with a 90%
aggregate line floor, matching the independent CI coverage job.

Enforcement is spread across four planes: git client hooks, the in-session
PreToolUse (Bash) guards, and remote branch protection read one config
(`.codeflow/policy.json`); the scaffolded CI runs the same git standards through
the `codeflow ci` binary (commit format, the 50/72 subject-length budget, the
bullet-only body shape with opt-in footer trailers and ticket references, and the
warn-only contract-surface tripwire of ADR-0020, attribution, emoji,
breaking-footer, branch naming). It is one source of truth, with no
inline drift, and portable across CI
hosts via thin per-platform wrappers (ADR-0017). This four-plane floor is the
**minimal** tier: it installs from `--minimal` up, before any of the method or
project-management scaffolding; the tiers scale project-management, not
enforcement (ADR-0019). CI also carries the
**security-review** plane (ADR-0016): a `security-review` job whose deterministic
floor is `osv-scanner`, stack-agnostic SCA across every lockfile ecosystem, the
universal floor today (per-stack scanners such as `cargo audit` / `pip-audit` /
`govulncheck` / `semgrep` are an optional future extension), with the
`cf-security-reviewer` dual-vendor red-team layered on top. It is gated by the
`security_review` (whole-job umbrella) and `dep_audit` (SCA sub-gate) policy keys
beside `secret_scan`; the advisory blocks when either is `block`. Local planes are
fast feedback; CI + remote protection are the authoritative perimeter
(the v2 charter, `docs/plan/v2/00-charter.md`, §6.5). The
git client plane carries five shims: `pre-commit`, `commit-msg`,
`pre-merge-commit` (non-fast-forward merge commits onto protected),
`reference-transaction` (the harness-agnostic backstop: fast-forward merges,
`reset --hard`, and `branch -D` on protected, git ≥ 2.28), and `pre-push`
(ADR-0007). The in-session guard plane is two handlers, `git-guard` (git
policy) and `exec-guard` (the `security` section: destructive commands block,
privilege escalation warns), wired for Claude in `.claude/settings.json` and,
through a byte-compatible PreToolUse payload, for an interactive Codex session in
`.codex/hooks.json` and for Grok Build in `.grok/hooks/codeflow.json` (ADR-0008
analog; Grok project hooks need `/hooks-trust` or `--trust`. Headless
`codex exec` / `grok -p` do not run project PreToolUse hooks, so those
invocations are not work-session lanes and rely on the git-hook plane). Codex credential
*reads* are guarded too, not only the Bash guards: a `cf-guard` permission profile
in `.codex/config.toml` (selected via `default_permissions`, extending `:workspace`)
denies the home-dir secret stores and high-confidence workspace key material
(`~/.ssh`, `~/.aws`, `.env`, `*.key`, `*.p12`, …) at the OS-sandbox
layer, so unlike the PreToolUse guards it holds even in headless `codex exec`
(ADR-0014); the `gh`/`docker` tool-token stores are deliberately left readable so
those tools can read their own tokens. The profile is the only sandbox
configuration, because legacy `sandbox_mode` would shadow it, and it also enables broad
public egress, exact loopback for local UI tests, and live search. When a
session is launched without `--sandbox danger-full-access`, private destinations
and arbitrary Unix sockets stay closed. Production Codex (ADR-0055) uses
`approval_policy = "never"` plus `--sandbox danger-full-access`, so that OS
sandbox is off for the process; git-guard, exec-guard, git hooks, and CI remain
the floor. The shell keeps Codex's default
`KEY`/`SECRET`/`TOKEN` environment scrub (ADR-0025, ADR-0026). Claude's
sandbox removes the raw Anthropic, OpenAI, and AWS credentials named in
ADR-0026 from arbitrary Bash while leaving brokered tools and MCP processes
available. The deterministic shell plane accepts both Bash and PowerShell
payloads and keeps its catastrophic classifier non-relaxable across Unix/macOS
roots and Windows drive, system, profile, disk, recovery, and permission
operations. macOS and Linux use native harness sandboxes; WSL2 follows the
Linux path. Native Windows Codex selects its elevated sandbox, while native
Windows Claude has no equivalent OS sandbox and therefore moves
high-blast-radius work to WSL2 or a container (ADR-0033). Beyond the
guards, `session-orient` is wired for Codex `SessionStart` too (ADR-0013), so an
interactive Codex session opens with the same orientation digest Claude gets,
and re-orients to it after a compaction. PR-content checks (attribution/emoji,
`gh pr merge` base) are git-guard/CI concerns by design, because git hooks cannot see
PR creation.

Delegation has two engine surfaces. The legacy `delegate-turn --result`
adapter writes immutable `0600` terminal evidence and signals its scoped tmux
waiter, byte-compatible with the ADR-0023 lanes. The schema-v2 lifecycle
(`delegate.rs`, surfaced as `codeflow delegate init|arm|wait` and
`hook delegate-turn --state-dir`) is a transport-neutral state machine over
write-once JSON records in an owner-only state directory outside any Git
worktree: `init` generates task-scoped Claude hook settings binding
SessionStart/UserPromptSubmit/Stop/StopFailure back to the hook; the host,
not the binary, launches the harness and delivers the armed prompt bytes;
ordinary `wait` polling is lock-free file reads with zero tmux involvement,
while every state mutation, including poisoning an interrupted wait after
acceptance, serializes on a single bounded run lock; a duplicate or
digest-mismatched prompt submission is blocked (hook exit 2, run state
preserved); and every true ambiguity (non-startup session source,
mis-correlated terminal event, ambiguous retry, interrupt after acceptance)
fails closed via a durable poison record. The `delegate-roundtrip` doctor
check drives the installed binary through the full synthetic lifecycle at
Fail severity.
Rationale, the full invariant set, and the canonical prompt-boundary amendment:
ADR-0036 and ADR-0037.

Interactive presentation is a separate bounded engine surface
(`codeflow-present`, ADR-0049, ADR-0050, and ADR-0052); the area outgrew this
file and is graduated to [architecture/present.md](architecture/present.md).

Records follow the markdown-truth design (D17): markdown + YAML frontmatter is
the source of truth, the JSONL ledger is the append-only event log, and SQLite
FTS5 is a rebuildable cache, with no database-as-authority and no embeddings. Core
reads through a `RecordStore` trait with a `MarkdownStore` implementation.
CodeFlow writes the flat, independently allocated stable-ID layout
`project-management/{epics/EPC-NNN.md,specs/SPC-NNN.md,tasks/TSK-NNN.md}`.
A shared layout enumerator keeps the store, docs validator, and recall index on
that contract while retaining read-only compatibility for historical nested
epic/task and `TSK-NNN-NNN` records; specs are indexed as project-management
recall sources. Frontmatter owns relationships: tasks point to an epic or carry
a standalone rationale, tasks name direct predecessors, and consuming epics or
tasks link specs.
Recall walks source trees without following directory symlinks and applies
depth/count budgets; encoded path bytes are index identity while lossy paths are
display-only. Ledger compaction syncs the directory after installing the merged
base and again after deleting fragments so crash ordering preserves the base.
Task records carry an `integration_target` and canonical `depends_on`
metadata; the historical `dependencies` spelling is a read alias.
Documentation validation checks the
non-executable structural graph for well-formed IDs, filenames, references,
duplicates, parent/standalone exclusivity, spec readiness, self-edges, and
cycles. The read-only `work start` preflight proves the task and its applicable
graph at the merge-base with the declared target; the CLI, pre-commit hook, and
detached CI share that core check. Scheduling and status mutation remain
Plan/native-harness concerns (ADR-0040, ADR-0046).

### scaffold: `assets/`

`assets/base/` holds the shipped scaffold (AGENTS.md/CLAUDE.md templates, the
`claude/` artifacts, policy.json, git-hook shims, docs and pm templates), and
`assets/docs-portal/` holds the optional portal starter. Each area below is
owned by an accepted decision; the mirrored skills and the CAP-### registry
carry the operating detail.

| Area | Owning ADR(s) | Consequence |
|---|---|---|
| Ownership classes | v2 charter §4.3 | Fully-managed files (agents, skills, hook shims, CI) refresh by hash and 3-way merge from `.codeflow/.baseline/`; managed-region files (AGENTS.md markers, settings.json codeflow-prefixed keys) touch only their region; schema-versioned user config additively gains new keys with their defaults and never mutates a value you set, while the write-once `docs/` seeds are laid once at init and never touched again. `scaffold-manifest.toml` is the update contract |
| Duo orchestration (`cf-model-orchestrator`) | ADR-0030, ADR-0035, ADR-0041, ADR-0056, ADR-0060 | The stage-aware, harness-neutral default for every non-trivial task in standard/full; primaries default to high effort and obtain xhigh on trigger without restarting; linked checkouts live under `.worktrees/`; each actual executor first-verifies its unit, the responsible primary accepts it, and a different lineage reviews it independently |
| Optional estimation (`cf-estimate`) | ADR-0057 | Shipped to standard/full as progressive disclosure (CAP-017); adoption is project-owned `.codeflow/estimate.json`, forecasts and actuals live under the project's existing planning authority, and none of it is scaffold-managed. Minimal receives no method files |
| Design direction (`cf-design`) | ADR-0043, ADR-0051 | Material product, UX, interaction or visual work records a proportionate `DESIGN_INTENT` inside Plan vN; the qualified Claude judgment role leads intent and owns design implementation and fidelity; refinement after selection stays bounded to one named unresolved choice, and no second design database appears |
| Editorial quality (`cf-editorial-review`) | ADR-0032 | Substantial prose loads the mirrored skill rather than expanding the always-loaded contract; truth and policy outrank philosophy, voice and requested tone, and the Claude judgment primary owns the final contextual verdict |
| Task graphs and verification strength | ADR-0040 | A multi-task plan settles one acyclic graph whose guards represent genuine decisions; property tests, targeted mutation testing and architecture fitness checks are earned from risk and oracle evidence, and CodeFlow never becomes a task scheduler |
| Review materiality | ADR-0034 | Substantiated material and systemic findings precede cosmetics, evidence confidence stays distinct from severity, security keeps its CVSS-aligned vocabulary, and out-of-scope material risk is routed without silent scope expansion |
| Critical-path stewardship | ADR-0038, ADR-0017 | The current critical path never licenses weaker quality, testing, security, review, documentation or recovery; escalation is reserved for a true external dependency or an intent-level choice; a gate is the check, not the CI job name, and a completed same-check in a sibling job or local run satisfies it |
| Runtime autonomy and settings | ADR-0025, ADR-0026, ADR-0028, ADR-0039, ADR-0060 | Claude's project settings are a fail-closed sandbox with file secret denies and exact raw model/cloud environment denies; a failed sandboxed command may request an auto-classified unsandboxed retry only for a trusted installed tool that needs host state, and arbitrary bypass stays outside the contract. The plan records the responsible primary separately from the actual executor. Detail: [harness posture](harness-posture.md) |
| Model and harness qualification (`cf-evaluate-model`) | ADR-0027 | A new model, harness, permission profile or material instruction revision is qualified over fresh one-commit disposable repositories in supervised native interactive sessions; no engine model router, headless peer runner, CI model call or general cleanup command is added. Detail: [model and harness upgrades](model-upgrades.md) |
| Binding facts versus durable doctrine | ADR-0039, ADR-0041, ADR-0054 | `current-ensemble.json`, `routing-policy.json` and `harnesses.json` own the fast-changing selectors, triggers and capability catalog while the orchestrator keeps stable role duties; a consuming project maps a stable role to an approved binding ID in `.codeflow/model-selection.json`, and doctor fails closed on a malformed, missing, ineligible, unsupported, drifted or lineage-collapsing override. Interactive Grok Build is a first-class host; Hermes (an outer coordinator, not a native CodeFlow host) normally delegates the whole repository task |
| Layered code verification | ADR-0042 | Project-owned deterministic lanes cover the applicable syntax/style, SCA, source and data flow, taint, secret and architecture rules while the two primary lineages review intent and semantics; a deterministic red result blocks regardless of model agreement, and consuming projects choose their own analyzer or record the residual risk |
| Whole-flow and UI isolation | ADR-0044 | Each material changed journey records one faithful vertical run across its applicable changed boundaries; concurrent UI tasks receive isolated browser state, non-overlapping endpoints, namespaced data, run-scoped artifacts and verified teardown; `codeflow status` reports removable, dirty and unproven resources and never deletes |
| Session review CLI (`codeflow present`) | ADR-0049, ADR-0050, ADR-0052 | Agents author this session's catalog document and the runtime owns chrome and Comment; it is neither a documentation portal nor a clone of the design-exploration board |
| Documentation portal bundle | ADR-0048, ADR-0058 | A separate managed bundle, absent until `codeflow portal setup --path <dir>` adopts it and records its root, release, hashes and ownership in `.codeflow/docs-portal.json`; the exact-pinned Node adapter is the sole author of disposable pages, twins, `llms.txt` and evidence from one clean committed snapshot; drift, collisions and edited retirement stop every portal write; `codeflow validate --portal` re-derives the byte claims without executing project code |
| Utility presentation system | ADR-0053, ADR-0063 | One shared design system of tokens, type roles, altitude grammar, and stage grammar backs both `cf-present` and the portal, with a contract test failing the build on token drift |

Two areas outgrew this file and are graduated, with the pointer left behind:
interactive presentation to [architecture/present.md](architecture/present.md)
and the shared design system to
[architecture/utility-presentation.md](architecture/utility-presentation.md).

### docs: `docs/`

The six-layer knowledge model this file belongs to, plus `docs/plan/v2/` (the
charter, the execution-status tracker, and the Day-0 probe artifacts; the
rebaseline ADRs live in `docs/decisions/`).
