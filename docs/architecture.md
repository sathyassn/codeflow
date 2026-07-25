# codeflow — architecture

<!-- HOW layer. Updated only inside the ship flow, in the same PR as the code,
     when an ADR declares architecture impact. Link to decisions by ADR id —
     never duplicate their content here.
     When an area outgrows this file, graduate it to docs/architecture/<area>.md
     and leave a one-line pointer behind. -->

## Overview

A two-crate Cargo workspace that builds one binary with the scaffold embedded.
`codeflow-core` is the engine library — all mechanics live here. `codeflow-cli`
is a thin dispatcher: `main.rs` is a clap command surface over 17 subcommands
(`init`, `update`, `hook`, `git-hook`, `orient`, `test`, `validate`, `ci`,
`status`, `integrate`, `doctor`, `policy`, `recall`, `remote`, `epic`, `task`,
`delegate`) — most a small handler in `cmd/` that
calls into core, while `init`/`update` dispatch inline in `main.rs` to the
scaffold module; `embedded.rs` embeds `assets/` via rust-embed (debug builds
read `assets/` from disk for instant scaffold iteration). The consuming repo is
its own first consumer, so `assets/` is as much the product as the code.

## Areas

### engine — `crates/codeflow-core` + `crates/codeflow-cli`

Core modules grouped by responsibility:

- **Scaffold** (`scaffold/`): `init`, `update`, manifest, 3-way merge, and the
  ownership classes below; sourced from the rust-embed asset provider.
- **Enforcement** (`hooks/`, `security/`, `git/`, `delegate.rs`,
  `integrate.rs`, `remote.rs`): the `git-guard` and `exec-guard` PreToolUse
  handlers and git-client hook stages, the dual-mode `delegate-turn` adapter
  (legacy `--result` record-and-signal plus the schema-v2 lifecycle backed by
  the transport-neutral `delegate.rs` state machine — ADR-0036), the secret
  scanner, git conflict detection + CI wait, the flock-guarded `integrate`
  primitive with its gate-context token, and the GitHub remote-protect
  adapter.
- **Records / knowledge** (`models/`, `ledger/`, `workgraph/`, `validate/`,
  `capability.rs`, `recall.rs`, `registry.rs`): frontmatter models, the JSONL
  ledger, the work graph, `validate` (+ the `--docs` referential-integrity
  lint, including structural task dependency identity/reference/cycle checks),
  the capability registry parser, FTS5 recall, and the cross-repo registry.
- **Support** (`doctor/`, `settings/`, `status.rs`, `testing/`, `file_lock.rs`,
  `error.rs`): the doctor check table (14 checks — hooks, claude, codex, config,
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
warn-only contract-surface tripwire — ADR-0020, attribution, emoji,
breaking-footer, branch naming) — one source of truth, no
inline drift, and portable across CI
hosts via thin per-platform wrappers (ADR-0017). This four-plane floor is the
**minimal** tier: it installs from `--minimal` up, before any of the method or
project-management scaffolding; the tiers scale project-management, not
enforcement (ADR-0019). CI also carries the
**security-review** plane (ADR-0016): a `security-review` job whose deterministic
floor is `osv-scanner` — stack-agnostic SCA across every lockfile ecosystem, the
universal floor today (per-stack scanners such as `cargo audit` / `pip-audit` /
`govulncheck` / `semgrep` are an optional future extension), with the
`cf-security-reviewer` dual-vendor red-team layered on top. It is gated by the
`security_review` (whole-job umbrella) and `dep_audit` (SCA sub-gate) policy keys
beside `secret_scan`; the advisory blocks when either is `block`. Local planes are
fast feedback; CI + remote protection are the authoritative perimeter (charter §6.5). The
git client plane carries five shims — `pre-commit`, `commit-msg`,
`pre-merge-commit` (non-fast-forward merge commits onto protected),
`reference-transaction` (the harness-agnostic backstop: fast-forward merges,
`reset --hard`, and `branch -D` on protected, git ≥ 2.28), and `pre-push`
(ADR-0007). The in-session guard plane is two handlers — `git-guard` (git
policy) and `exec-guard` (the `security` section: destructive commands block,
privilege escalation warns) — wired for Claude in `.claude/settings.json` and,
through a byte-compatible PreToolUse payload, for an interactive Codex session in
`.codex/hooks.json` (ADR-0008; headless `codex exec` 0.142.5 does not run project
PreToolUse hooks, so headless Codex relies on the git-hook plane). Codex credential
*reads* are guarded too — not only the Bash guards: a `cf-guard` permission profile
in `.codex/config.toml` (selected via `default_permissions`, extending `:workspace`)
denies the home-dir secret stores and high-confidence workspace key material
(`~/.ssh`, `~/.aws`, `.env`, `*.key`, `*.p12`, …) at the OS-sandbox
layer, so unlike the PreToolUse guards it holds even in headless `codex exec`
(ADR-0014); the `gh`/`docker` tool-token stores are deliberately left readable so
those tools can read their own tokens. The profile is the only sandbox
configuration—legacy `sandbox_mode` would shadow it—and also enables broad
public egress, exact loopback for local UI tests, and live search. Private
destinations and arbitrary Unix sockets stay closed; `on-request` escalations
route to a reviewer subagent (or a human when the project/launch setting selects
`approvals_reviewer = "user"`), and the shell keeps Codex's default
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
interactive Codex session opens with — and re-orients after a compaction from —
the same orientation digest Claude gets. PR-content checks (attribution/emoji,
`gh pr merge` base) are git-guard/CI concerns by design — git hooks cannot see
PR creation.

Delegation has two engine surfaces. The legacy `delegate-turn --result`
adapter writes immutable `0600` terminal evidence and signals its scoped tmux
waiter, byte-compatible with the ADR-0023 lanes. The schema-v2 lifecycle
(`delegate.rs`, surfaced as `codeflow delegate init|arm|wait` and
`hook delegate-turn --state-dir`) is a transport-neutral state machine over
write-once JSON records in an owner-only state directory outside any Git
worktree: `init` generates task-scoped Claude hook settings binding
SessionStart/UserPromptSubmit/Stop/StopFailure back to the hook; the host —
not the binary — launches the harness and delivers the armed prompt bytes;
ordinary `wait` polling is lock-free file reads with zero tmux involvement,
while every state mutation — including poisoning an interrupted wait after
acceptance — serializes on a single bounded run lock; a duplicate or
digest-mismatched prompt submission is blocked (hook exit 2, run state
preserved); and every true ambiguity (non-startup session source,
mis-correlated terminal event, ambiguous retry, interrupt after acceptance)
fails closed via a durable poison record. The `delegate-roundtrip` doctor
check drives the installed binary through the full synthetic lifecycle at
Fail severity.
Rationale, the full invariant set, and the canonical prompt-boundary amendment:
ADR-0036 and ADR-0037.

Records follow the markdown-truth design (D17): markdown + YAML frontmatter is
the source of truth, the JSONL ledger is the append-only event log, and SQLite
FTS5 is a rebuildable cache — no database-as-authority, no embeddings. Core
reads through a `RecordStore` trait with a `MarkdownStore` implementation.
Recall walks source trees without following directory symlinks and applies
depth/count budgets; encoded path bytes are index identity while lossy paths are
display-only. Ledger compaction syncs the directory after installing the merged
base and again after deleting fragments so crash ordering preserves the base.
Task records optionally carry canonical `depends_on` metadata; the historical
`dependencies` spelling is a read alias. Documentation validation checks the
non-executable structural graph for well-formed IDs, filenames, references,
duplicates, self-edges, and cycles. Guard selection, branch readiness, task
completion, and scheduling remain Plan/ledger and native-harness concerns
(ADR-0040).

### scaffold — `assets/`

`cf-model-orchestrator` is the stage-aware harness-neutral default for every
non-trivial repository task in standard/full scaffolds, with two
vendor-maintained/native adapters: Claude Code reaches Codex through the
official plugin/app-server integration, while Codex reaches an interactive
Claude CLI through a task-scoped tmux session. Research/analysis, plan/design,
implementation, review/verification, and substantive-doc modes select only the
stages the requested outcome needs. Claude-led design, capability-routed
producer/cross-lineage-review assignments, evidence-routed effort, explicit
host/peer/worker roles, the versioned plan/evidence contract, bounded
worktree/resource/integration rules, and quality gates live in shared skill
resources; harness-specific reviewer agents only
deepen that contract. Quality includes proportionate design and implementation:
every material new surface maps to a current requirement or evidenced risk,
each producer first-verifies its unit, the other lineage reviews it independently,
and the directly invoked Fable primary owns the integrated design/code quality
verdict without claiming independent review of its own work (ADR-0030,
ADR-0035). Substantial prose loads the
mirrored `cf-editorial-review` skill rather than expanding the always-loaded
contract: truth and policy outrank CodeFlow philosophy, consuming-project voice,
audience/medium/task, and requested tone; both seats protect technical meaning,
and Fable owns the final contextual editorial verdict (ADR-0032).
Multi-task plans additionally settle one acyclic task graph whose evidence
guards represent genuine decisions, not repeated quality gates. Durable task
metadata preserves its structural candidate predecessors, while Plan evidence
selects guarded branches and a material graph mutation forces Plan vN+1.
Verification planning may earn property tests, targeted mutation testing, or a
project-owned architecture fitness check from explicit risk and oracle
evidence; ordinary work and single-task plans incur no such ceremony. These
resources are progressive disclosure, and CodeFlow never becomes a task
scheduler or installs consuming-project test tools for parity (ADR-0040).
Review attention is consequence-led: substantiated material and systemic
findings precede cosmetics, evidence confidence stays distinct from severity,
remediation effort affects sequencing only, security retains its CVSS-aligned
vocabulary, and out-of-scope material risk is routed without silent scope
expansion or issue farming (ADR-0034). Execution additionally distinguishes
materiality from the current critical path: the path is the dependency or
blocker presently controlling the accepted outcome, not a license to weaken
quality, testing, security, review, documentation, or recovery. Clear, safe,
in-scope improvements with bounded validation are normally fixed while context
is warm. Only genuinely uncertain secondary observations are consolidated for
one natural cross-lineage checkpoint, where both primary seats choose fix now,
track once, or drop. A tracked item uses the repository's existing planning
altitude and a deterministic event such as the next touch of that surface, a
named dependency landing, a release/quality gate, or symptom recurrence—never
age alone or vague “later” wording. Blocker navigation uses the same
critical-path model: technical uncertainty is reproduced and tested with a
changed hypothesis, while an outcome-preserving reversible strategy may change
without operator ceremony. Escalation is reserved for a true external
dependency or a choice that changes intent, public contract, scope/authority,
risk tolerance, or an irreversible tradeoff; deterministic and safety gates
are fixed or honored rather than talked around (ADR-0038).

Runtime autonomy is an explicit second layer, not a prose assumption. Claude's
project settings enable a fail-closed sandbox, sandbox-contained Bash autonomy,
web access, local port binding, asks for common high-risk source-control forms,
protected-branch hook backstops for grammar gaps, file
secret denies, and exact raw model/cloud environment-variable denies. A failed
sandboxed command may request an auto-classified unsandboxed retry only for a
trusted installed tool requiring host state; arbitrary bypass remains outside
the contract. The interactive Codex→Claude launch supplies the current
ensemble's Claude selector and effort, auto mode, and `classifyAllShell`
through CLI settings because Claude intentionally ignores classifier policy
from a repository. Codex's project config selects the guarded workspace
profile, public egress/live search, auto-reviewed escalations, and the current
primary-seat fallback. Official plugin turns still pass the ensemble-selected
model and effort explicitly and retain the observed binding.
`cf-customize` verifies the effective modes, tools, authentication paths, and
live canaries; the binary neither mutates global settings nor authenticates
services. Cross-model callers invoke both primaries directly using the concrete
selectors, default/escalation efforts, and permitted worker classes in the
current ensemble record. Native internal workers may reduce mechanical cost,
but cannot replace the primary seats' judgments, implementation, verification,
or approvals (ADR-0025, ADR-0026, ADR-0028, ADR-0039).

`cf-evaluate-model` is the deliberate maintenance path for a new model, harness,
permission profile, or material instruction revision (ADR-0027). Stable hard
requirements link canonical source markers to behavioral regression/capability
cases. A standard-library tool validates that traceability, materializes exact
overlays into fresh one-commit disposable repositories, removes grader material
before the subject session starts, keeps case identity and evaluator state
outside an opaque neutral subject path, recomputes expected-versus-observed
outcomes, compares a candidate with a pinned baseline, and cleans only an
explicitly marked run root. Subject trials remain supervised native interactive
Codex or Claude sessions with their configured tools; no engine model router,
headless peer runner, CI model call, or general-purpose cleanup command is added.

Fast-changing binding facts are isolated from durable orchestration doctrine
(ADR-0039). `current-ensemble.json` owns concrete primary selectors, effort
policy, permitted worker classes, and escalation triggers.
`harnesses.json` is a qualification catalog, not executable provider
configuration: every entry must prove the universal native-session,
provenance/tool, scoped-work, bounded-failure, permission, recheck, and git
capabilities. `packs.json` composes existing eval cases for diagnosis only.
Approved full results can produce compact user-owned records under
`~/.codeflow/qualified-bindings/`; doctor checks their structure and observable
harness/settings drift without launching models or routing work.

`assets/base/` holds the shipped scaffold (AGENTS.md/CLAUDE.md templates, the
`claude/` artifacts, policy.json, git-hook shims, docs and pm templates); the
engine manages it by three ownership classes (charter §4.3): **fully-managed**
files (agents, skills, hook shims, CI) refresh by hash and 3-way merge from
`.codeflow/.baseline/`; **managed-region** files (AGENTS.md markers,
settings.json codeflow-prefixed keys) touch only their region; and **user-owned**
files, which split by how `update` treats them — schema-versioned config
(`policy.json`, `project.toml`) *additively gains* new keys with their defaults,
reported and never mutating a value you set, while the write-once doc seeds (all
of `docs/`) are seeded once at init and never touched again — yours to edit and
own. `scaffold-manifest.toml` is the update contract.

### docs — `docs/`

The six-layer knowledge model this file belongs to, plus `docs/plan/v2/` (the
charter, the execution-status tracker, and the Day-0 probe artifacts; the
rebaseline ADRs live in `docs/decisions/`).
