# codeflow — architecture

<!-- HOW layer. Updated only inside the ship flow, in the same PR as the code,
     when an ADR declares architecture impact. Link to decisions by ADR id —
     never duplicate their content here.
     When an area outgrows this file, graduate it to docs/architecture/<area>.md
     and leave a one-line pointer behind. -->

## Overview

A two-crate Cargo workspace that builds one binary with the scaffold embedded.
`codeflow-core` is the engine library — all mechanics live here. `codeflow-cli`
is a thin dispatcher: `main.rs` is a clap command surface over 16 subcommands
(`init`, `update`, `hook`, `git-hook`, `orient`, `test`, `validate`, `ci`,
`status`, `integrate`, `doctor`, `policy`, `recall`, `remote`, `epic`, `task`) — most a small handler in `cmd/` that
calls into core, while `init`/`update` dispatch inline in `main.rs` to the
scaffold module; `embedded.rs` embeds `assets/` via rust-embed (debug builds
read `assets/` from disk for instant scaffold iteration). The consuming repo is
its own first consumer, so `assets/` is as much the product as the code.

## Areas

### engine — `crates/codeflow-core` + `crates/codeflow-cli`

Core modules grouped by responsibility:

- **Scaffold** (`scaffold/`): `init`, `update`, manifest, 3-way merge, and the
  ownership classes below; sourced from the rust-embed asset provider.
- **Enforcement** (`hooks/`, `security/`, `git/`, `integrate.rs`, `remote.rs`):
  the `git-guard` and `exec-guard` PreToolUse handlers and git-client hook
  stages, the owner-only `delegate-turn` Stop/StopFailure completion adapter,
  the secret scanner, git conflict detection + CI wait, the flock-guarded
  `integrate` primitive with its gate-context token, and the GitHub remote-
  protect adapter.
- **Records / knowledge** (`models/`, `ledger/`, `workgraph/`, `validate/`,
  `capability.rs`, `recall.rs`, `registry.rs`): frontmatter models, the JSONL
  ledger, the work graph, `validate` (+ the `--docs` referential-integrity
  lint), the capability registry parser, FTS5 recall, and the cross-repo
  registry.
- **Support** (`doctor/`, `settings/`, `status.rs`, `testing/`, `file_lock.rs`,
  `error.rs`): the doctor check table (12 checks — hooks, claude, codex, config,
  permissions, network, delegates, repo-integrity, ci-perimeter, managed-drift,
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
denies the home-dir secret stores (`~/.ssh`, `~/.aws`, `.env`, …) at the OS-sandbox
layer, so unlike the PreToolUse guards it holds even in headless `codex exec`
(ADR-0014); the `gh`/`docker` tool-token stores are deliberately left readable so
those tools can read their own tokens. The profile is the only sandbox
configuration—legacy `sandbox_mode` would shadow it—and also enables broad
public egress, exact loopback for local UI tests, and live search. Private
destinations and arbitrary Unix sockets stay closed; `on-request` escalations
route to automatic review (ADR-0025). Beyond the
guards, `session-orient` is wired for Codex `SessionStart` too (ADR-0013), so an
interactive Codex session opens with — and re-orients after a compaction from —
the same orientation digest Claude gets. PR-content checks (attribution/emoji,
`gh pr merge` base) are git-guard/CI concerns by design — git hooks cannot see
PR creation.

Records follow the markdown-truth design (D17): markdown + YAML frontmatter is
the source of truth, the JSONL ledger is the append-only event log, and SQLite
FTS5 is a rebuildable cache — no database-as-authority, no embeddings. Core
reads through a `RecordStore` trait with a `MarkdownStore` implementation.
Recall walks source trees without following directory symlinks and applies
depth/count budgets; encoded path bytes are index identity while lossy paths are
display-only. Ledger compaction syncs the directory after installing the merged
base and again after deleting fragments so crash ordering preserves the base.

### scaffold — `assets/`

`cf-model-orchestrator` is the stage-aware harness-neutral default for every
non-trivial repository task in standard/full scaffolds, with two
vendor-maintained/native adapters: Claude Code reaches Codex through the
official plugin/app-server integration, while Codex reaches an interactive
Claude CLI through a task-scoped tmux session. Research/analysis, plan/design,
implementation, review/verification, and substantive-doc modes select only the
stages the requested outcome needs. Fixed model-class roles, the versioned
plan/evidence contract, bounded worktree/resource/integration rules, and quality
gates live in the shared skill resource; harness-specific reviewer agents only
deepen that contract (ADR-0023, ADR-0024).

Runtime autonomy is an explicit second layer, not a prose assumption. Claude's
project settings enable a fail-closed sandbox, sandbox-contained Bash autonomy,
web access, local port binding, destructive asks, and secret denies. The
interactive Codex→Claude launch supplies Fable/xhigh, auto mode, and
`classifyAllShell` through CLI settings because Claude intentionally ignores
classifier policy from a repository. Codex's project config selects the guarded
workspace profile, public egress/live search, and auto-reviewed escalations.
`cf-customize` verifies the effective modes, tools, authentication paths, and
live canaries; the binary neither mutates global settings nor authenticates
services (ADR-0025).

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
