---
id: ADR-0055
title: Medium-default effort, Astra Codex primary, contained worktrees, Herdr project cwd
date: 2026-09-05
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — duo seats default to medium and escalate on complexity; Codex primary is Astra via app-server then CLI; worktrees live under .worktrees/; Herdr/tmux cwd is the project being worked
---

# ADR-0055 — medium-default effort, Astra Codex primary, contained worktrees

## Context

ADR-0028 defaulted both primaries to high. That over-spends ordinary work. The
operator rule is medium by default; high/xhigh only when the *task* is complex
or difficult (architecture, technical depth, planning, design — new or
existing). Novelty is not a trigger.

Codex's current coding primary is GPT-6 Astra (`gpt-6-astra`), via the official
app-server (Codex App / `codex` CLI talking to the local daemon). Interactive
CLI in Herdr/tmux is the fallback when app-server is unavailable. Third-party
Grok plugins that wrap Codex are not a CodeFlow lane.

Linked worktrees were created as sibling directories and polluted the parent
folder. Git's `.git/worktrees/` is metadata, not the checkout. Checkouts belong
under the owning repository at `.worktrees/<slug>`, gitignored.

Herdr/tmux sessions are rooted at the project folder being worked, not some
other repo's space. Same topic reuses the same tab/session; a new topic gets a
new tab; a finished session's tab is closed.

## Decision

1. **Effort.** Every primary defaults to medium. Escalate to high on a recorded
   complexity/difficulty trigger; to xhigh only on a recorded xhigh trigger
   (stalled high, pair disagreement, long-horizon/cross-cutting security).
2. **Bindings.** Claude primary remains Fable; internals may use Opus at
   medium/high/xhigh; Opus is the recorded fallback if Fable is unavailable.
   Codex primary is `gpt-6-astra`; internals may use Sol/Terra; `gpt-5.6-sol`
   is the recorded fallback if Astra cannot run. Grok primary is `grok-4.6`;
   internals stay on `grok-4.6` at medium/high.
3. **Permissions (production host).** Claude `bypassPermissions` or `auto`;
   Codex `--sandbox danger-full-access --ask-for-approval never` (or always-approve
   equivalent); Grok `--always-approve`. Consults and no-edit review stay
   non-bypass. Git-guard, exec-guard, git hooks, and CI remain the floor.
4. **Codex transport.** Prefer official app-server. From a Grok host, or when
   Codex itself is the host CLI, if the daemon is missing and `codex` CLI is
   present, start interactive CLI via Herdr (tmux degraded). The Claude Code →
   Codex lane remains the official plugin only (ADR-0018, ADR-0023); a missing
   plugin degrades to solo. Never `codex exec`. Do not install third-party
   Grok Codex plugins.
5. **Worktrees.** From the protected root, create
   `git worktree add .worktrees/<slug> -b <branch>`. Ignore `.worktrees/` in
   the managed gitignore. Relative `core.hooksPath=.codeflow/git-hooks` so each
   worktree uses its own shims. Do not store machine-absolute hook paths.
6. **Herdr.** Workspace/tabs are the project being worked. No hijack. Resume the
   same tab for the same topic; new tab only for a new topic; close the tab
   when that work is harvested and done.

## Consequences

- Ordinary work is cheaper; hard work still has an explicit ladder.
- Consumers receive the contract through the scaffold (ensemble, skills,
  gitignore, hooks), not only CodeFlow's dogfood tree.
- Existing sibling worktrees are not migrated automatically.

## Architecture impact

`docs/architecture.md` host/effort/worktree sentences match this ADR. ADR-0028's
high-default and ADR-0025's "never bypass on an ordinary production host" are
amended by this decision; consult/no-edit review is unchanged.

## Note (2026-09-06)

A medium primary that hits a high or xhigh trigger mid-session stays the
orchestrator and spawns same-family workers at that effort. It does not
restart the host session. Judgment, plan approval, and named review stay
with the primary. Opus/Sol/Terra remain bounded/simple routes, not the
high-effort substitute for Fable/Astra. Grok internals may use xhigh.
