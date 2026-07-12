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
verified_by: ["cargo test scaffold::init", "cargo test scaffold::detect", "codeflow-core tests/scaffold_test.rs", "codeflow-cli tests/tier_floor_e2e.rs"]
epics: [EPC-001]
adrs: [ADR-0019]
```

`codeflow init [--minimal|--standard|--full] [--yes]` lays the discipline
layer into any repo. Enforcement is the floor; the tiers scale project-management
(ADR-0019): `--minimal` installs the complete four-plane enforcement floor (all
five git-hook shims, the CI check, the in-session `git-guard`/`exec-guard` +
orient/summary hooks in `.claude/settings.json` and the `.codex/` starter, the
armed `policy.json`, `.gitignore`, and a lean `AGENTS.md` + `CLAUDE.md`);
`--standard` adds the method (Claude/agent skills, reviewer agents, the pipeline)
and the six-layer docs spine and full contract; `--full` adds
project-management/. Idempotent, non-destructive, offline (assets embedded via
rust-embed), with bootstrap grace, husky/hooksPath detection, and a printed
per-file report. Re-running at a higher tier is an additive upgrade.

## CAP-002 — scaffold-update

```yaml
id: CAP-002
name: scaffold-update
area: scaffold
status: shipped
verified_by: ["cargo test scaffold::update", "cargo test scaffold::settings_merge", "cargo test scaffold::region", "cargo test scaffold::manifest", "codeflow-cli tests/tier_floor_e2e.rs"]
epics: [EPC-001]
adrs: [ADR-0011, ADR-0019]
```

`codeflow update` refreshes managed scaffold files by ownership class:
unmodified files are replaced, user-modified files get a 3-way merge from
`.codeflow/.baseline/` (conflicts produce `.new` + report), managed regions
(AGENTS.md markers, settings.json codeflow keys) are surgically updated, and
user-owned schema-versioned files only gain new keys with defaults. It also
installs any manifest entry that is in-tier but missing on disk — so a file that
became in-tier since the last install (e.g. an old `--minimal` repo gaining the
enforcement floor under ADR-0019) is reconciled into place and recorded, not just
refreshed. Never clobbers, never silently skips.

## CAP-003 — git-policy-gates

```yaml
id: CAP-003
name: git-policy-gates
area: engine
status: shipped
verified_by: ["cargo test hooks::git_hook", "cargo test hooks::git_guard", "cargo test hooks::policy", "cargo test hooks::policy_schema", "cargo test hooks::standards", "codeflow-cli tests/hooks_cli.rs", "codeflow-cli tests/policy_cli.rs"]
epics: [EPC-001]
adrs: [ADR-0002, ADR-0006, ADR-0007, ADR-0017]
```

Git discipline enforced across four planes reading one config (the `git`
section of `.codeflow/policy.json`). Two give fast local feedback — the git
client hooks (pre-commit secret scan + staged-.env, commit-msg
format/attribution/emoji, pre-merge-commit and reference-transaction
protected-branch merge/ref rules, pre-push branch naming and protected-branch
rules) and the Claude `git-guard` PreToolUse hook (in-session immediacy, plus
the `gh pr merge` and PR-body checks no client hook can see). Two are the
authoritative perimeter — CI, which re-runs the same checks through the
`codeflow ci` binary (the same Rust functions the hooks call, so no inline
drift, portable across CI hosts via thin GitHub/GitLab/Bitbucket/generic
wrappers — ADR-0017), and remote branch protection (`codeflow remote protect`).
Every rule is a policy value, user-flippable per repo. The policy file is
discoverable and strict from the binary alone: `codeflow policy explain`
renders every key's type, default, and valid values from a schema registry
drift-guarded against the policy struct; `codeflow policy show` prints the
effective values and their source; and a present-but-invalid file fails
loudly — naming each offending key, its value, and the valid set — at the
commit-msg hook, `codeflow ci`, and `codeflow validate`, instead of silently
reverting every key to the built-in defaults.

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

`codeflow test [--mode full|quick|essential] [--strict]` runs the generic test
engine against configured targets (`.codeflow/test-config.json`) or runtime stack
detection (`quick` is an alias for `essential`, the lighter mode). No stack
detected is a loud no-op (exit 0) so the bootstrap/early-setup path stays green;
`--strict` escalates that no-op to a non-zero exit for scripted/unattended callers
(CI, the pipeline verify gate) where "ran nothing" must not read as a pass. With a
stack it is a real gate, wired into pre-push via the `test_gate_on_push` policy and
re-run in CI.

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
`codeflow doctor` runs eleven health checks — hooks, Claude wiring, codex wiring, config,
permissions, network, delegates, repo integrity, CI perimeter, managed-region
drift, and test config — so degradation is always visible, never silent.

## CAP-009 — cross-vendor-delegation

```yaml
id: CAP-009
name: cross-vendor-delegation
area: scaffold
status: shipped
verified_by: ["cargo test doctor::tests::test_check_delegates", "live: codex-plugin-cc MCP + resumable thread verified 2026-07-11, codex-cli 0.144.1 (ADR-0018)"]
epics: []
adrs: [ADR-0005, ADR-0018]
```

Consult or delegate a unit of work to another vendor's coding CLI at the
process boundary, each under its own subscription auth, with CodeFlow's gates
judging the output author-agnostically (ADR-0005). Transport is
interactive-only per ADR-0018, one lane per direction: from Claude Code the
official `codex-plugin-cc` plugin (wrapping the codex app-server); from codex
the interactive `claude` CLI driven via tmux. Headless task execution
(`codex exec`, `claude -p`) is prohibited; the earlier headless tier and the
Antigravity `agy` delegate tier (headless-only) are retired. It ships as the
`cf-delegate` and `cf-consult` skills (mirrored to `.agents/skills`), and an
optional `consult` pipeline stage — plus one deterministic `delegates` doctor
check; delegates edit only inside a worktree on a feature branch, so
pre-commit, commit-msg, the test gate, and the independent review pass
constrain them exactly as they do the orchestrating harness. No engine
orchestration code is added.

## CAP-010 — duo-model-orchestration

```yaml
id: CAP-010
name: duo-model-orchestration
area: scaffold
status: shipped
verified_by: ["codeflow-core tests/manifest_consistency.rs", "cargo test doctor::tests::test_check_delegates"]
epics: []
adrs: [ADR-0015, ADR-0018]
```

`/cf-model-orchestrator` drives higher-stakes planned work as a Claude+codex
duo: Claude plans with explicit acceptance criteria, codex (through the
official `codex-plugin-cc` plugin — the only sanctioned lane, ADR-0018)
reviews the plan then executes and
first-tests it including UI-driven e2e, Claude does the final verification
grading every criterion, and the two iterate a bounded fix loop. Ships as the
`cf-model-orchestrator` skill (mirrored across `.claude/skills`, `.agents/skills`,
and the `assets/base` source) and an opt-in `duo` pipeline preset with a
`plan-align` convergence gate — never the default, so trivial work pays no duo
tax. Driven from a Claude Code seat only; silently degrades to solo
`/cf-develop` when either half of the duo is unavailable at flow start (the
symmetric seat gate, ADR-0018); no engine orchestration code is added, and the
authoritative gate stays
server-side CI plus a human-merged PR (ADR-0006).

## CAP-011 — security-redteam-review

```yaml
id: CAP-011
name: security-redteam-review
area: engine
status: shipped
verified_by: ["cargo test hooks::policy", "codeflow-core tests/manifest_consistency.rs"]
epics: []
adrs: [ADR-0016]
```

The duo develop flow's mandatory security / red-team stage, bound at three
planes that copy the git-rules model. Deterministic floor: the CI
`security-review` job runs `osv-scanner` (stack-agnostic SCA over every lockfile
ecosystem — the universal floor today; per-stack scanners are a future
extension), gated by the `security_review` (whole-job umbrella) and `dep_audit`
(SCA sub-gate) policy keys beside `secret_scan`, with the advisory blocking when
either is `block`. Model layer: the `cf-security-reviewer` agent runs a
dual-vendor adversarial red-team (Claude defender lens + codex assume-breach
attacker, ADR-0005) across seven axes mapped to OWASP Top 10:2025 / OWASP LLM
Top 10:2025 / CWE Top 25 (2025), emitting structured `SecurityFinding` /
`SecurityVerdict` output; the pipeline `security` stage sets its verdict from the
severity+confidence block rule. The deterministic floor hard-blocks CI only
when `security_review` or `dep_audit` is hardened to `block` (the shipped
default is warn); secrets via gitleaks always block, and a High+ severity
filter is future work alongside the per-stack scanners. Model-reasoned findings
warn locally and force bounded rework, with the human merger as the backstop
for judgment a machine cannot adjudicate (ADR-0007).

## CAP-012 — scaffold-customize

```yaml
id: CAP-012
name: scaffold-customize
area: scaffold
status: shipped
verified_by: ["codeflow-core tests/manifest_consistency.rs"]
epics: []
adrs: []
```

`/cf-customize` is the post-init tailoring walk-through: after `codeflow init`
(or a `codeflow update` that ships new defaults to decide), it runs a
flow-aware tool preflight — verifying the tools needed by each flow the
project actually uses (git and the harness for every flow; the codex driver,
its MCP servers, and tmux for the duo flow; the stack's test toolchain) —
then fills the project-owned artifacts still sitting at template defaults
(`docs/product.md`, the AGENTS.md/CLAUDE.md project sections, policy.json gate
levels, model pins). Analysis-then-propose: one prioritized report first, then
fixes applied interactively on a working branch through a PR; it never
auto-installs a tool. Ships as the `cf-customize` skill, mirrored across
`.claude/skills`, `.agents/skills`, and the `assets/base` scaffold source.
