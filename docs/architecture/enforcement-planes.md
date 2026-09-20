# Enforcement planes

<!-- HOW layer. Graduated area page: the plane detail that outgrew
     docs/architecture.md and docs/adoption.md, with a pointer left behind in
     each. Sources: ADR-0007, ADR-0008, ADR-0016, ADR-0017, ADR-0018,
     ADR-0019, ADR-0020, the v2 charter. Runtime posture per harness lives in
     docs/harness-posture.md. -->

## Concept

**One policy file arms four planes, so no single harness is a required trust
anchor.** `.codeflow/policy.json` is the only place a rule is written; each of
the four complementary planes reads that one source and acts where it can see
the work, with no inline drift and thin per-platform CI wrappers for
portability (ADR-0017).

```cf-stage
.codeflow/policy.json | one source of truth @accent
->
git client hooks | five shims on git operations
in-session guards | git-guard · exec-guard on PreToolUse
codeflow ci | the same git standards, server-side
remote protection | where the host arms it
->
protected branches | human-merged PRs on evidenced-green checks @positive
caption: local planes are fast feedback, CI and remote protection are the authoritative perimeter
```

Local planes are fast feedback; CI and remote protection are the authoritative
perimeter (the v2 charter, `docs/plan/v2/00-charter.md`, §6.5). This
four-plane floor is the **minimal** tier: it installs from `--minimal` up,
before any of the method or project-management scaffolding, and the tiers scale
project-management, not enforcement (ADR-0019). Which plane catches which
action is the matrix below.

## Architecture

Read one row for a protected action and the columns say which planes see it.
Minimal installs the local floor and the CI scaffold, not remote branch
protection (ADR-0019), so a row's remote column is coverage the host still has
to arm.

| Protection | git hooks | in-session guard | CI | remote |
|---|---|---|---|---|
| Commit / non-ff merge commit on protected | pre-commit / pre-merge-commit | git-guard | yes | yes |
| FF-merge, `reset --hard`, `branch -D` on protected | reference-transaction | git-guard | none | yes (result unpushable) |
| Push / force-push / delete to protected | pre-push | git-guard | none | yes |
| `gh pr merge` into a protected base | none (hooks can't see a PR) | git-guard | none | yes |
| Destructive command (`rm -rf /`, `mkfs`, fork bomb) | none | exec-guard (block) | none | none |
| Privilege escalation (`sudo`, `LD_PRELOAD`) | none | exec-guard (warn) | none | none |
| Commit format, no-attribution, no-emoji, secrets | commit-msg / pre-commit | partial | yes | none |
| Host attribution injection (`Co-Authored-By`, "Generated with") | settings preset turns it off (`includeCoAuthoredBy`, `attribution`) | git-guard (PR body) | yes (commit-msg) | no |
| Override-token laundering, `--no-verify` bypass | none | git-guard (structural) | none | none |

Two facts the matrix encodes. **PR-content checks are git-guard/CI by design.**
A git hook never sees `gh pr create` or `gh pr merge`, so attribution and emoji
scans and the protected-base check live in the Claude layer and in CI, not in
the hooks. **The human override (`CODEFLOW_HUMAN_OVERRIDE=1`) and the integrate
token apply to the git-hook plane only.** The git-guard never trusts them,
because an agent in a session cannot prove it is a human.

The matrix describes available coverage, not proof that every plane is active.
Verify hook execution, harness trust, policy severity, CI results, and actual
remote rules and bypass permissions. Local checks are required feedback, but
they remain editable.

## Technical

The four planes, then how far each reaches on a harness CodeFlow does not
integrate.

### git client hooks

Five shims, harness-agnostic: they act on git operations, not on which tool
produced them (ADR-0007).

| Shim | What it holds | Notes |
|---|---|---|
| `pre-commit` | commits on a protected branch; the secret scan | also runs the read-only `work start` preflight that the CLI and detached CI share |
| `commit-msg` | commit format, no-attribution, no-emoji | the same checks `codeflow ci` runs server-side |
| `pre-merge-commit` | non-fast-forward merge commits onto protected | |
| `reference-transaction` | fast-forward merges, `reset --hard`, `branch -D` on protected | the harness-agnostic backstop; needs git ≥ 2.28 |
| `pre-push` | push, force-push and delete to protected | reports `codeflow pre-push: BLOCKED` against policy rule `git.push_to_protected` |

`reference-transaction` on older git is absent and protection falls back to the
other planes. With active protection, an unreadable or unevaluable prepared
transaction blocks rather than silently skipping the check. The current input
contract is UTF-8: non-UTF-8 input also blocks, including remote-only
transactions, while valid UTF-8 remote-only input retains its fast path.
Inspect the reported cause and the repository, tool and backend compatibility
before retrying; preserve the work and repair the supported path rather than
disabling the hook or automatically converting repository storage to evade it.

This plane needs no per-harness configuration: a Codex
`git push --force origin main` against protected `main` is refused by the
`pre-push` shim exactly as any agent's would be. Installed files still do not
make these checks unbypassable: local hook files and Git configuration remain
editable.

### in-session guards

Two PreToolUse (Bash) handlers add fast, pre-git feedback.

| Handler | Policy section | Verdict |
|---|---|---|
| `git-guard` | git policy | blocks the git rows above, the PR-content checks, and structural override-token laundering |
| `exec-guard` | the `security` section | destructive commands block; privilege escalation warns |

| Harness | Wiring | Condition |
|---|---|---|
| Claude Code | `.claude/settings.json` | laid by the scaffold from `--minimal` up |
| Interactive Codex | `.codex/hooks.json` | byte-compatible PreToolUse payload (ADR-0008); the project's `.codex/` layer must be trusted |
| Grok Build | `.grok/hooks/codeflow.json` | ADR-0008 analog; project hooks load only after `/hooks-trust` or `--trust` |
| Headless `codex exec`, `grok -p` | none | project PreToolUse hooks do not run, so those invocations are not work-session lanes and rely on the git-hook plane |

The deterministic shell plane accepts both Bash and PowerShell payloads and
keeps its catastrophic classifier non-relaxable across Unix and macOS roots and
Windows drive, system, profile, disk, recovery, and permission operations.
Another harness needs its own qualified event contract before it gets this
plane. Historical headless hook gaps motivated ADR-0018, but they are not a
universal claim about every current harness; the consult, delegate and duo
flows remain interactive-only independently of whether a headless mode can run
hooks.

### CI

The scaffolded CI runs the same git standards through the `codeflow ci` binary,
one source of truth with no inline drift, portable across CI hosts via thin
per-platform wrappers (ADR-0017).

| CI check | What it enforces |
|---|---|
| commit format | the conventional-commit shape |
| subject length | the 50/72 subject-length budget |
| body shape | bullet-only, with opt-in footer trailers and ticket references |
| contract-surface tripwire | warn only (ADR-0020) |
| attribution, emoji | no AI attribution and no emoji in commit content |
| breaking footer, branch naming | the declared footer and branch conventions |

CI also carries the **security-review** plane (ADR-0016): a `security-review`
job whose deterministic floor is `osv-scanner`, stack-agnostic SCA across every
lockfile ecosystem and the universal floor today, with the
`cf-security-reviewer` dual-vendor red-team layered on top. Per-stack scanners
such as `cargo audit`, `pip-audit`, `govulncheck` or `semgrep` are an optional
future extension.

| Gate | Policy key | Behaviour |
|---|---|---|
| security-review job | `security_review` | whole-job umbrella |
| SCA sub-gate | `dep_audit` | the `osv-scanner` floor |
| secret scan | `secret_scan` | the sibling key the git-hook plane reads |

The advisory blocks when either `security_review` or `dep_audit` is `block`.

### remote protection

The GitHub remote-protect adapter (`remote.rs`, surfaced as `codeflow remote`)
arms the host's own branch rules where the host offers them. CI re-runs the
checks and configured remote rules can require their results, so CI becomes a
merge gate when the remote requires its result. The end state the planes exist
for is a protected branch whose PRs are merged by a human on evidenced-green
checks.

### how far each plane reaches

CodeFlow has two kinds of thing: **enforcement** (gates that block) and
**guidance** (instructions and skills that inform). They reach different
distances, so be precise about what a given harness actually gets.

| Layer | Reaches | Depends on |
|---|---|---|
| Git hooks and CI | any agent or human, Claude Code, Codex, a future CLI: conventional-commit format, the secret scan, no-AI-attribution, the branch, push and protected-merge rules, and the test gate | the relevant hooks and CI actually executing |
| In-session guards | Claude, interactive Codex, Grok Build | qualified harness integration and, for Codex and Grok, trust of the project layer |
| Guidance: `AGENTS.md` and the `cf-*` skills | Claude and Codex | the skills ship to `.claude/skills/` and `.agents/skills/`; both read the repo `AGENTS.md` operating contract |
| Workflow runtime (`pipeline.workflow.js`) | Claude Code only | |
| A harness CodeFlow does not integrate, for example Google's Antigravity `agy` | the git-hook plane and CI, because those are harness-agnostic | it does not receive the in-session guards, the skills, or (historically verified on `agy` 1.0.15) the `AGENTS.md` instructions; its reliable coverage is the configured, verified hook and CI plane, not assumed guidance |

The one-line version: CodeFlow shares policy across harness-neutral checks;
native guidance and in-session guards depend on the installed integration.
Missing planes are disclosed without relaxing safety or review duties.

In every mode, verify actual local hook execution and any required CI and
remote rules rather than inferring protection from installed files. The runtime
posture each harness receives is in [harness posture](../harness-posture.md).
