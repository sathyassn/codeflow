# codeflow — capabilities

<!-- WHAT layer: the registry of what the system does. Consult before building
     ("does this exist? what does it touch?"). Updated in the same PR that
     ships the work — the discipline is the ship flow, not a blocking gate.
     `validate --docs` enforces referential integrity (each entry's epics[] and
     adrs[] resolve to real files) and a non-empty verified_by on shipped
     entries; it does not verify that the test tags resolve, and it does not
     block an epic from closing.
     Statuses: planned → building → shipped → deprecated (never delete).
     At ~15 entries, graduate to docs/capabilities/CAP-*.md. -->

## CAP-001 — scaffold-init

```yaml
id: CAP-001
name: scaffold-init
area: scaffold
status: shipped
verified_by: ["cargo test scaffold::init", "cargo test scaffold::detect", "codeflow-core tests/scaffold_test.rs"]
epics: [EPC-001]
adrs: []
```

`codeflow init [--minimal|--standard|--full] [--yes]` lays the discipline
layer into any repo: AGENTS.md/CLAUDE.md, git-hook shims, Claude settings and
artifacts, docs knowledge model, CI template, and (full tier)
project-management/. Idempotent, non-destructive, offline (assets embedded
via rust-embed), with bootstrap grace, husky/hooksPath detection, and a
printed per-file report. Re-running at a higher tier is an additive upgrade.

## CAP-002 — scaffold-update

```yaml
id: CAP-002
name: scaffold-update
area: scaffold
status: shipped
verified_by: ["cargo test scaffold::update", "cargo test scaffold::settings_merge", "cargo test scaffold::region", "cargo test scaffold::manifest"]
epics: [EPC-001]
adrs: []
```

`codeflow update` refreshes managed scaffold files by ownership class:
unmodified files are replaced, user-modified files get a 3-way merge from
`.codeflow/.baseline/` (conflicts produce `.new` + report), managed regions
(AGENTS.md markers, settings.json codeflow keys) are surgically updated, and
user-owned schema-versioned files only gain new keys with defaults. Never
clobbers, never silently skips.

## CAP-003 — git-policy-gates

```yaml
id: CAP-003
name: git-policy-gates
area: engine
status: shipped
verified_by: ["cargo test hooks::git_hook", "cargo test hooks::git_guard", "cargo test hooks::policy", "cargo test hooks::standards", "codeflow-cli tests/hooks_cli.rs"]
epics: [EPC-001]
adrs: [ADR-0002, ADR-0006, ADR-0007]
```

Git discipline enforced across four planes reading one config (the `git`
section of `.codeflow/policy.json`). Two give fast local feedback — the git
client hooks (pre-commit secret scan + staged-.env, commit-msg
format/attribution/emoji, pre-merge-commit and reference-transaction
protected-branch merge/ref rules, pre-push branch naming and protected-branch
rules) and the Claude `git-guard` PreToolUse hook (in-session immediacy, plus
the `gh pr merge` and PR-body checks no client hook can see). Two are the
authoritative perimeter — CI re-running the same checks and remote branch
protection (`codeflow remote protect`). Every rule is a policy value,
user-flippable per repo.

## CAP-004 — test-gate

```yaml
id: CAP-004
name: test-gate
area: engine
status: shipped
verified_by: ["cargo test testing::gate", "cargo test testing::config", "codeflow-core tests/integration_testing_setup.rs"]
epics: [EPC-001]
adrs: []
```

`codeflow test [--mode full|quick|essential]` runs the generic test engine
against configured targets (`.codeflow/test-config.json`) or runtime stack
detection (`quick` is an alias for `essential`, the lighter mode). No stack
detected is a loud no-op; with a stack it is a real gate, wired into pre-push
via the `test_gate_on_push` policy and re-run in CI.

## CAP-005 — integrate

```yaml
id: CAP-005
name: integrate
area: engine
status: shipped
verified_by: ["cargo test integrate::", "codeflow-cli tests/cli_flow.rs"]
epics: [EPC-001]
adrs: []
```

`codeflow integrate <branch> [--into <target>]` is the sanctioned local
landing path for protected branches: a flock-guarded rebase → test → ff-merge
primitive. It runs under a gate-context token verified by the git hooks and
git-guard, so a merge commit reaches a protected branch only through
integrate or a PR — raw `git merge` stays blocked.

## CAP-006 — recall-registry

```yaml
id: CAP-006
name: recall-registry
area: engine
status: shipped
verified_by: ["cargo test recall::", "cargo test registry::", "codeflow-cli tests/recall_remote_cli.rs"]
epics: [EPC-001]
adrs: []
```

`codeflow recall [--all] "<query>"` searches project memory — ledger events,
session summaries, ADRs, epics, capabilities — via bundled SQLite FTS5, with
coverage gaps disclosed. The cross-repo view comes from
`~/.codeflow/registry.json`, a flock'd registry upserted on every command run;
a lazy view at query time, not a daemon.

## CAP-007 — orient-session-summary

```yaml
id: CAP-007
name: orient-session-summary
area: engine
status: shipped
verified_by: ["cargo test hooks::orient", "cargo test hooks::session_summary", "codeflow-cli tests/hooks_cli.rs", "codeflow-cli tests/codex_hooks.rs"]
epics: [EPC-001]
adrs: [ADR-0013]
```

`codeflow orient` generates the session-start digest live (≤30 lines,
pointers not content): product one-liner, branch/worktree state, work and
capability counts, recent ADR titles, gate status, paths to read more. The
`session-summary` SessionEnd hook appends a session record to the ledger —
recall's zero-ceremony corpus. Both are wired through `.claude/settings.json`;
the orient digest is also wired for Codex via `.codex/hooks.json` (SessionStart,
all sources — ADR-0013), so an **interactive** Codex session opens with the same
digest and re-orients after a compaction (`source=compact`). One handler serves
both harnesses — plain-text stdout each injects as session context — so there is
no per-harness duplication. Headless `codex exec` does not fire project hooks
(ADR-0008), so this is an interactive-session aid; the `codex_hooks` test pins the
JSON wiring, while live firing rests on Codex's documented hooks contract.

## CAP-008 — remote-protect-doctor

```yaml
id: CAP-008
name: remote-protect-doctor
area: engine
status: shipped
verified_by: ["cargo test remote::", "cargo test doctor::", "codeflow-cli tests/recall_remote_cli.rs"]
epics: [EPC-001]
adrs: [ADR-0002, ADR-0007]
```

`codeflow remote protect` applies the policy's `protected_branches` to the
provider (GitHub via `gh api`: require PR + green CI, block force-push and
deletion) with a legible report of anything the plan tier cannot apply.
`codeflow doctor` runs seven health checks — hooks, Claude wiring, config,
permissions, network, delegates, and repo integrity — so degradation is
always visible, never silent.

## CAP-009 — cross-vendor-delegation

```yaml
id: CAP-009
name: cross-vendor-delegation
area: scaffold
status: shipped
verified_by: ["cargo test doctor::tests::test_check_delegates", "live: codex exec --json consult + resume on registry.rs, thread 019f23e5-7bd7-7842-8a26-009e5a652759 (docs/plan/v2/01-execution-status.md)"]
epics: []
adrs: [ADR-0005]
```

Consult or delegate a unit of work to another vendor's coding CLI at the
process boundary, each under its own subscription auth, with CodeFlow's gates
judging the output author-agnostically (ADR-0005). `codex` is the primary tier
(`codex exec --json` + `resume`, plus the official `codex-plugin-cc` documented
as the interactive tier); Antigravity `agy` is a degraded, opt-in, read-only
consult tier. It ships as Claude artifacts — the `cf-delegate` and `cf-consult`
skills, and an optional `consult` pipeline stage — plus one
deterministic `delegates` doctor check; delegates edit only inside a worktree on
a feature branch, so pre-commit, commit-msg, the test gate, and `cf-reviewer`
constrain them exactly as they do Claude. No engine orchestration code is added.
