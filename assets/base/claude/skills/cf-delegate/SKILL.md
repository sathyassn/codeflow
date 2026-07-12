---
name: cf-delegate
description: When and how to consult or delegate to the other vendor's coding CLI under its own subscription auth — read-only second opinions versus full task handoffs, the interactive-only transport lanes (ADR-0018), the edit-access doctrine, and the guardrails. Use when you want an independent second opinion, a specialty pass, or genuinely parallel work handed to a harness of a vendor you are not.
---

# cf-delegate — cross-vendor consult and delegate

One line: **compose harnesses at the process boundary, each under its own
subscription auth; CodeFlow's gates judge the output, never the author.** A
second harness is a colleague you can call, not a dependency you take on — you
still own the result, verify it, and answer for it.

The delegate is a vendor you are **not**: from Claude Code that is codex; from
codex that is claude. Consulting or delegating to your own vendor is
self-review with extra steps — never label it independent.

## Consult, delegate, or neither

Match the tool to the work; most work is *neither*.

- **Neither (the default).** You have the context and the skill — do it
  yourself. A second harness costs a round-trip, quota, and a synthesis step;
  spend that only when it buys something you cannot get alone.
- **Consult — read-only second opinion.** A critique, a design review, a
  correctness pass, an "am I missing something" on code *you* wrote. The
  delegate reads and reasons; it never edits. Value is the *independent*
  vantage: a different model, trained differently, catching what you pattern
  past. Use it exactly where self-review is weakest.
- **Delegate — full task handoff with edit access.** A whole unit of work
  handed off, code included. Reserve for genuinely **parallel** work (you are
  busy on the critical path and this is independent) or **specialty** work
  another harness does better. Not for work you could just do — a handoff you
  have to review line-by-line is slower than doing it yourself.

When unsure, consult before you delegate: a read-only opinion is cheap and
reversible; an edit handoff is neither.

## Transport — interactive-only, one lane per direction (ADR-0018)

```
Claude Code ──codex-plugin-cc plugin──▶ codex
codex ──tmux-driven interactive claude CLI──▶ claude
```

- **Claude Code → codex: the official `codex-plugin-cc` plugin, only.** It
  wraps the codex app-server — the same interactive engine as the TUI — so a
  delegated task gets codex's full MCP toolset (Playwright verified with 24
  browser tools on codex-cli 0.144.1, 2026-07-11), a resumable thread, and
  in-band approvals.
- **codex → claude: the interactive `claude` CLI driven via tmux, only**
  (pattern below). A live round-trip has not been verified here —
  verify-on-install with one scoped read-only consult before relying on it.

**Prohibited at all times** — no exceptions, including batch/pipeline stages:
headless task execution in either direction (`codex exec`, `claude -p` /
`--print`), driving the codex app-server directly (hand-rolled JSON-RPC), and
Claude → codex via tmux. Why: a headless session fires no in-session guards,
carries no full MCP toolset, and resumes nothing; a hand-rolled driver chases
an experimental API the vendor already wraps; one lane per direction is
simpler to verify and harder to misuse. Status commands are not work sessions
— `codex login status`, `codex --version`, `codex mcp list`, and the plugin
install/setup steps stay fine.

## Preflight — is the delegate even available

Delegation is **optional**. Never assume the lane is wired; check your seat's
lane, and degrade legibly when it is not.

- **From Claude Code:** the plugin surface exists (the `/codex:*` commands
  respond; install once from a Claude Code session: `/plugin marketplace add
  openai/codex-plugin-cc` → `/plugin install codex@openai-codex` →
  `/reload-plugins` → `/codex:setup`) and codex is authenticated
  (`codex login status` — exit 0 + "Logged in using ChatGPT"; the same signal
  `codeflow doctor` reports as the `delegates` check).
- **From codex:** `claude` on PATH and `tmux` present.

If the other vendor's CLI is missing, do the work yourself and **say so** —
loudly, never as a silent substitution of your own vendor. If it is present
but unauthenticated (or 401s mid-run), stop and tell the user to run
`codex login` (or log in to `claude`) — **never automate the auth**. One
vendor account per side, the user's own.

## Lane 1 — from Claude Code, through the plugin

The plugin's commands cover both modes:

- **Consult (read-only):** `/codex:review` and `/codex:adversarial-review` —
  the design/diff read and the cross-vendor security red-team.
- **Delegate (write-enabled):** `/codex:rescue` — delegated execution and
  first-round testing; flags: `--background`/`--wait`, `--resume`/`--fresh`,
  `--model`, `--effort`.
- **Multi-round:** `/codex:transfer` — a persistent codex thread for the
  back-and-forth; follow-ups resume it instead of starting fresh.

Ask every consult for a closing `VERDICT: approved|changes_requested` line so
the reply is checkable, and branch on it — then re-derive the findings
yourself (see Guardrails).

## Lane 2 — from codex, tmux-driving the interactive claude CLI

The pattern, compactly:

```sh
tmux new-session -d -s delegate -x 220 -y 50 'claude'
tmux send-keys -t delegate -l 'Review src/foo.rs for correctness. Cite line numbers. Read and reason only - edit nothing. End with VERDICT: approved|changes_requested.'
tmux send-keys -t delegate Enter
# poll until the output stabilizes, then read it:
tmux capture-pane -p -J -t delegate -S -200
```

- **Turn detection:** poll `capture-pane` and compare consecutive captures
  (hash them); the turn is done when the pane stops changing and the input
  prompt is back. Bound the wait with a timeout; on expiry capture the pane
  for diagnosis, then `kill-session`.
- **Read-only is posture, not enforcement:** the interactive TUI has no
  sandbox flag, so a consult's read-only contract lives in the prompt ("read
  and reason only; edit nothing"). Say that plainly when you report — the lane
  cannot mechanically prevent an edit; the git-hook plane and review still
  catch what matters.
- **Edit handoff:** start the session with the worktree as its working
  directory (`tmux new-session -d -s delegate -c /path/to/worktree 'claude'`)
  so edits and commits land where the gates guard them.
- **Multiline prompts:** `set-buffer` + `paste-buffer -p`, then a separate
  `send-keys Enter`.
- **Follow-ups:** the pane keeps its context — send the next prompt to the
  same session.

## Edit-access doctrine (delegate tier)

A delegate that edits works **only** inside a worktree on a feature branch —
the same worktree-per-session discipline that binds every agent here. From
Claude Code, scope `/codex:rescue` to the worktree; from codex, start the tmux
claude session in the worktree. Let it commit conventionally.

Never grant edit access on the root checkout or a protected branch. The point
of the doctrine: a delegate's commits pass through **CodeFlow's existing gates
unchanged** — pre-commit secret scan, commit-msg format and no-AI-attribution,
the test gate, and an independent review pass through the primary harness's
normal review gate judge its work exactly as they judge yours. Enforcement is
author-agnostic, so a delegate cannot lower the bar. The git-hook plane is
harness-agnostic by design: the protected-branch merge and ref guards
(`pre-merge-commit`, `reference-transaction`) bind a delegate exactly as they
bind any agent, so it cannot ff-merge, `reset --hard`, or delete a protected
branch. Review the handoff before it ships; a delegate's output is a proposal,
not a merge — a human lands it.

## agy — retired as a delegate tier (no interactive lane)

Google's Antigravity `agy` was a degraded, opt-in read-only tier. Its only
documented drive shape is headless one-shot CLI invocation (non-TTY stdout
drops the final response, so automation had to read a transcript file) — a
shape ADR-0018 prohibits outright. There is no verified interactive lane to
it, so `agy` is **not** a delegate tier; if the user names it, say the
transport rule rules it out. Like any tool that touches the repo, `agy`
remains bound by the harness-agnostic git-hook plane and CI.

**Experimental — binding the CodeFlow guards to `agy`** (for when `agy` is
someone's *harness*, not a delegate): its hook dialect differs (an
`allow_tool` JSON contract, hooks that always exit 0), so the exit-2 block
that stops a bad command under Claude/Codex is at best *advisory* there. To
opt in for feedback anyway, create `~/.gemini/config/hooks.json` with
`{"hooks":{"PreToolUse":[{"matcher":"^Bash$","hooks":[{"type":"command","command":"codeflow hook git-guard"},{"type":"command","command":"codeflow hook exec-guard"}]}]}}`
— experimental and unverified on macOS; the authoritative protection stays the
git-hook plane, CI, and an independent review before anything lands.

## Guardrails

- **Never automate vendor auth.** The user runs `codex login` (or logs in to
  `claude`) by hand, on a single account per side. Degrade legibly on missing
  or 401 — never paper over it.
- **Every delegate prompt carries caps.** Explicit file scope, a step budget,
  and "surface ambiguity as a blocker, never guess." Unbounded handoffs are how
  quota and scope both blow out.
- **Synthesize, never paste.** A delegate's finding is an input, not your
  conclusion. Re-derive it, cite the file:line or command output yourself, and
  say plainly where you agree and disagree before reporting it as fact. An
  unverified delegate claim is an unverifiable claim — a defect by this repo's
  own rule.
- **Stay within ToS.** This process-boundary composition is sanctioned (OpenAI
  ships the plugin itself); the single-account, manual-auth, degrade-on-401
  posture is what keeps it there.
