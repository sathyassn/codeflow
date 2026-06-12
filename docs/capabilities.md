# codeflow — capabilities

<!-- WHAT layer: the registry of what the system does. Consult before building
     ("does this exist? what does it touch?"). Updated in the same PR that
     ships the work; a FEAT epic cannot close without its entry at full tier.
     Statuses: planned → building → shipped → deprecated (never delete).
     At ~15 entries, graduate to docs/capabilities/CAP-*.md. -->

## CAP-001 — scaffold-init

```yaml
id: CAP-001
name: scaffold-init
area: scaffold
status: shipped
verified_by: ["cargo test scaffold::init", "cargo test scaffold::detect", "codeflow-cli tests/scaffold_test.rs"]
epics: []
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
epics: []
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
epics: []
adrs: [ADR-0002]
```

Git discipline enforced across three local planes reading one config
(`.codeflow/policy.json.git`): git client hooks (pre-commit secret scan +
staged-.env, commit-msg format/attribution/emoji, pre-push branch naming and
protected-branch rules), the Claude `git-guard` PreToolUse hook (instant
in-session feedback on force-push/hard-reset/delete dodges client hooks can't
see), and CI re-running the same checks as the perimeter. Every rule is a
policy value, user-flippable per repo.

## CAP-004 — test-gate

```yaml
id: CAP-004
name: test-gate
area: engine
status: shipped
verified_by: ["cargo test testing::gate", "cargo test testing::config", "codeflow-cli tests/integration_testing_setup.rs"]
epics: []
adrs: []
```

`codeflow test [--mode full|quick]` runs the generic test engine against
configured targets (`.codeflow/test-config.json`) or runtime stack detection.
No stack detected is a loud no-op; with a stack it is a real gate, wired into
pre-push via the `test_gate_on_push` policy and re-run in CI.

## CAP-005 — integrate

```yaml
id: CAP-005
name: integrate
area: engine
status: shipped
verified_by: ["cargo test integrate::", "codeflow-cli tests/cli_flow.rs"]
epics: []
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
epics: []
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
verified_by: ["cargo test hooks::orient", "cargo test hooks::session_summary", "codeflow-cli tests/hooks_cli.rs"]
epics: []
adrs: []
```

`codeflow orient` generates the session-start digest live (≤30 lines,
pointers not content): product one-liner, branch/worktree state, work and
capability counts, recent ADR titles, gate status, paths to read more. The
`session-summary` SessionEnd hook appends a session record to the ledger —
recall's zero-ceremony corpus. Both are wired through `.claude/settings.json`.

## CAP-008 — remote-protect-doctor

```yaml
id: CAP-008
name: remote-protect-doctor
area: engine
status: shipped
verified_by: ["cargo test remote::", "cargo test doctor::", "codeflow-cli tests/recall_remote_cli.rs"]
epics: []
adrs: [ADR-0002]
```

`codeflow remote protect` applies the policy's `protected_branches` to the
provider (GitHub via `gh api`: require PR + green CI, block force-push and
deletion) with a legible report of anything the plan tier cannot apply.
`codeflow doctor` reports the live enforcement matrix per plane — hooks,
Claude wiring, config, permissions, network — so degradation is always
visible, never silent.
