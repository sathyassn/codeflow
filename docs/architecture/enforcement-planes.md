# Enforcement planes

<!-- HOW layer. Graduated area page: the plane detail that outgrew
     docs/architecture.md and docs/adoption.md, with a pointer left behind in
     each. Sources: ADR-0007, ADR-0008, ADR-0016, ADR-0017, ADR-0018,
     ADR-0019, ADR-0020. Runtime posture per harness lives in
     docs/harness-posture.md. -->

## Concept

`.codeflow/policy.json` is the only place a rule is written, and four planes
read it where each can see the work.

No single harness is a required trust anchor. The git client hooks and the
in-session guards give fast local feedback and can be edited;
CI and remote protection are the authoritative perimeter where they are armed.
This
four-plane floor installs from the minimal tier up, and the tiers scale
project management, never enforcement (architecture decision record ADR-0019).
The planes read one source with no inline drift, through thin per-platform CI
wrappers (ADR-0017).

## Architecture

The planes cover different moments of one change, three of them can stop it
at push, and only remote branch protection is a boundary, once the host arms
it.

- Minimal installs the local floor and the CI scaffold, not remote branch
  protection (ADR-0019), so the remote plane is coverage the host still has to
  arm.
- PR-content checks are git-guard and CI by design. A git hook never sees
  `gh pr create` or `gh pr merge`, so attribution and emoji scans and the
  protected-base check live in the Claude layer and in CI, not in the hooks.
- The human override (`CODEFLOW_HUMAN_OVERRIDE=1`) and the integrate token
  apply to the git-hook plane only. The git-guard never trusts them, because an
  agent in a session cannot prove it is a human. In that plane they lift the
  protected-branch commit, merge, push and local ref-update rules; they never
  lift a force push or deletion of a protected branch, or the secret checks.
  A protected branch moves only by a proven fast-forward: a push over a remote
  tip that is not in the local repository is refused as a force push.
- Host attribution has a fourth brake that is not a plane: the shipped Claude
  settings preset turns the host's own injection off at the source
  (`includeCoAuthoredBy`, `attribution`), so the `commit-msg` hook catches only
  what a changed or absent preset lets through.

The planes describe available coverage, not proof that every plane is active.
Verify hook execution, harness trust, policy severity, CI results, and actual
remote rules and bypass permissions.

## Technical

One row per protected action; the columns say which planes see it.

| Protection | git hooks | in-session guard | CI | remote |
|---|---|---|---|---|
| Commit / non-ff merge commit on protected | pre-commit / pre-merge-commit | git-guard | yes | yes |
| FF-merge, `reset --hard`, `branch -D` on protected | reference-transaction | git-guard | none | yes (result unpushable) |
| Push / force-push / delete to protected | pre-push | git-guard | none | yes |
| `gh pr merge` into a protected base | none (hooks cannot see a PR) | git-guard | none | yes |
| Destructive command (`rm -rf /`, `mkfs`, fork bomb) | none | exec-guard (block) | none | none |
| Privilege escalation (`sudo`, `LD_PRELOAD`) | none | preset deny rules, exec-guard (block) | none | none |
| Commit format, no-attribution, no-emoji, secrets | commit-msg / pre-commit | partial | yes | none |
| Host attribution injection (`Co-Authored-By`, "Generated with") | commit-msg | git-guard (PR body) | yes | none |
| Override-token laundering, `--no-verify` bypass | none | git-guard (structural) | none | none |

### git client hooks

Five shims, harness-agnostic: they act on git operations, not on which tool
produced them (ADR-0007).

| Shim | What it holds | Notes |
|---|---|---|
| `pre-commit` | commits on a protected branch; the secret scan | also refuses staged conflict markers, and a commit in the root checkout while it is off its root branch |
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

Three PreToolUse handlers add fast, pre-git feedback.

| Handler | Policy section | Verdict |
|---|---|---|
| `git-guard` | git policy | blocks the git rows above, the PR-content checks, structural override-token laundering, discards of local-only work, and changes to the tracking refs and transport settings that decide policy authority |
| `exec-guard` | the `security` section | destructive commands and privilege escalation block (ADR-0075 D5); outward action families, secret-store reads and interpreter literals refuse at their rule's level |
| `edit-guard` | the action table's enforcement paths | native file edits (Codex `apply_patch`, Grok `write` and `search_replace`) to enforcement paths refuse, including patch move sources and destinations |

| Harness | Wiring | Condition |
|---|---|---|
| Claude Code | `.claude/settings.json` | laid by the scaffold from `--minimal` up |
| Interactive Codex | `.codex/hooks.json`, with edit-guard | byte-compatible PreToolUse payload (ADR-0008); the project's `.codex/` layer must be trusted, and Codex asks again for each new folder or worktree |
| Grok Build | `.grok/hooks/codeflow.json`, with edit-guard | ADR-0008 analog; project hooks load only after `/hooks-trust` or `--trust` |
| Headless `codex exec`, `grok -p` | none | project PreToolUse hooks do not run, so those invocations are not work-session lanes and rely on the git-hook plane |

Agent sessions are judged by the landed policy. The guards read
`.codeflow/policy.json` and project settings from the configured remote's
default branch and the declared target, taking the stricter level key by key,
so a local checkout, commit, rebase or stash cannot relax them. When the remote
HEAD is not set, as after `git init`, `git remote add` and `git push -u`,
every existing `main` and `master` tracking ref contributes, stricter wins, so
a fetch that adds one cannot weaken them; a custom default branch needs the
operator's `git remote set-head <remote> --auto`, which `doctor` names, and a
dangling remote HEAD refuses. They read
`HEAD` only when there is no remote or the remote has no tracking refs yet,
and the working copy only on an unborn `HEAD`; every refusal and `doctor`
name the source. Ref plumbing on `refs/remotes`, fetch or pull into an
explicit tracking destination or from another source, remote identity
changes, writes to transport configuration, the global Git config files and
the common Git directory's refs and config cannot replace that authority.
Fetch, pull and remote update compare each selected remote's effective URL
with its configured URL. Once a tracking ref exists, missing authority
refuses the call and names `git fetch`, or the operator's
`git remote set-head <remote> --auto`. `doctor` and orient report the source
and any local policy drift; they do not detect earlier movement of a
tracking ref.

Under the shipped defaults, the guards refuse the wrapped, flag-led and
interpreter forms of privilege escalation, package or gist publishing,
release and tag changes, repository or account changes, secret-store reads
and user-level persistence. Commands inside opaque child programs are not
inspected. Ordinary builds, a task-branch push and a single-file restore
stay ordinary work.

A refusal names the policy rule and the operator's route. Project relief is
that rule's existing level, such as `security.privilege_escalation`,
`security.outward_actions`, `security.secret_reads` or
`git.discard_uncommitted`, landed through a reviewed change, together with
any native deny that still applies; the native presets and the guards are
separate checks, and the agent never edits enforcement files to clear its
own refusal. `security.headless_peer_runs` is the only relief for a headless
peer run; `security.headless_opt_in` is ignored with a warning and removed
by `codeflow update`. The `security.dangerous_commands` floor cannot be
lowered.

Contract-3 git shims exit 1 when the `codeflow` binary is missing or older,
printing the installer and `codeflow update`, so install the new binary
before running `codeflow update`. A harness wrapper is
`codeflow hook <name> --contract 3`, then a fallback that exits 2 whenever
the hook fails and always ends with a line saying it blocked, since Codex
treats exit 2 with an empty stderr as a failed hook and lets the call
through. A policy refusal prints the guard's message first; a missing
binary also prints the installer and `codeflow update`; a binary too old to
know `--contract` prints its own usage error, and one that knows the flag
names the install step for a contract it does not support. The wrapper
never calls the binary twice and carries no `$`: Grok expands `$name` and
`${...}` in a hook command itself and skips, failing open, a hook whose
variable is unset (issue 29). Grok shows only the first stderr line of a
denying hook, so a guard refusing a Grok call also writes Grok's deny
decision with the whole refusal on stdout. `codeflow doctor --check grok`
names a CodeFlow hook command Grok would skip. When the shipped exec-guard
handler (command, timeout and environment) is bound where Grok's shell
tool hits it, matched as Grok matches, doctor judges a fixed canary with
the handler `codeflow hook exec-guard --contract 3` runs, in its own
process under the catastrophic-command floor alone, reading no policy,
repository, working directory or environment and recording no refusal,
and expects exit 2, a reason and Grok's deny answer. Doctor executes
nothing for the check, so no hook text, hook environment, `codeflow` on
PATH or swapped binary, any of which the repository could plant, answers
for it: a customised handler is reported unverified, and where PATH
resolves `codeflow` is reported without being run. The canary does not
exercise a shell, the command-line parsing, the `codeflow` on PATH or
Grok's own hook call; a live session's hook lines prove those.
The wrappers need a POSIX shell (macOS, Linux, WSL or Git Bash); native
PowerShell as the hook runner is unsupported.

The deterministic shell plane accepts both Bash and PowerShell payloads and
keeps its catastrophic classifier non-relaxable across Unix and macOS roots and
Windows drive, system, profile, disk, recovery, and permission operations.
Another harness needs its own qualified event contract before it gets this
plane. The consult, delegate and duo flows are interactive only, whether or not a
headless mode can run hooks (ADR-0018).

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

CI also carries the **security-review** job (ADR-0016), whose deterministic floor is `osv-scanner`, stack-agnostic software
composition analysis (SCA) across every
lockfile ecosystem and the universal floor today, with the
`cf-security-reviewer` dual-vendor red-team layered on top. Per-stack scanners
such as `cargo audit`, `pip-audit`, `govulncheck` or `semgrep` are an optional
future extension.

| Gate | Policy key | Behavior |
|---|---|---|
| security-review job | `security_review` | whole-job umbrella |
| SCA sub-gate | `dep_audit` | the `osv-scanner` floor |
| secret scan | `secret_scan` | the sibling key the git-hook plane reads |

The job blocks when either `security_review` or `dep_audit` is `block`; otherwise it warns.

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
| A harness CodeFlow does not integrate, for example Google's Antigravity `agy` | the git-hook plane and CI, because those are harness-agnostic | it does not receive the in-session guards, the skills, or the `AGENTS.md` instructions (verified on `agy` 1.0.15); its reliable coverage is the configured, verified hook and CI plane |

The one-line version: CodeFlow shares policy across harness-neutral checks;
native guidance and in-session guards depend on the installed integration.
Missing planes are disclosed without relaxing safety or review duties.

In every mode, verify actual local hook execution and any required CI and
remote rules rather than inferring protection from installed files. The runtime
posture each harness receives is in [harness posture](../harness-posture.md).
