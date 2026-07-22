---
name: cf-delegate
description: Consult or delegate to the other vendor's native coding harness under its own subscription auth. Covers read-only opinions, full task handoffs, the two interactive-only transport lanes, deterministic reverse-lane completion, edit-access doctrine, and guardrails. Use for an independent second opinion, specialty pass, or genuinely parallel work (CodeFlow ADR-0023).
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

## Transport — interactive-only, one lane per direction (CodeFlow ADR-0023)

```text
Claude Code ──codex-plugin-cc plugin──▶ codex
codex ──tmux-driven interactive claude CLI──▶ claude
```

- **Claude Code → codex: the official `codex-plugin-cc` plugin, only.** It
  wraps the codex app-server — the same interactive engine as the TUI — so a
  delegated task gets codex's full MCP toolset (Playwright verified with 24
  browser tools on codex-cli 0.144.1, 2026-07-11), a resumable thread, and
  in-band approvals.
- **codex → claude: the interactive `claude` CLI driven via tmux, only**
  (pattern below). The account and interactive response must be verified with a
  scoped TTY canary; verify the full tmux + completion-signal round trip on
  install and whenever the Claude CLI or hook configuration changes.

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
- **From codex:** `claude` on PATH, `tmux` present, `claude mcp list`
  succeeds, and a short interactive TTY canary gets an authenticated response.
  Do not infer authentication from a status subcommand when it conflicts with
  a working interactive session.

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

Invoke GPT-5.6 Sol or the strongest supported successor Codex coding seat
directly at high effort by default. Escalate the primary to xhigh only for
capability-sensitive or long-horizon work, material ambiguity, cross-cutting
architecture/security, unresolved disagreement, or failed/stalled high work.
Codex may use bounded Sol-class medium/high workers only through verified native
routing; the invoked primary retains the task, implementation, verification,
and verdict.

Ask every consult for a closing `VERDICT: approved|changes_requested` line so
the reply is checkable, and branch on it — then re-derive the findings
yourself (see Guardrails).

## Lane 2 — from codex, tmux-driving the interactive claude CLI

The transport pattern, compactly:

```sh
tmux new-session -d -s delegate -x 220 -y 50 -c /path/to/worktree 'claude --model fable --effort high --permission-mode auto --settings /path/to/task-settings.json'
tmux send-keys -t delegate -l 'Review src/foo.rs for correctness. Cite line numbers. Read and reason only - edit nothing. End with VERDICT: approved|changes_requested.'
tmux send-keys -t delegate Enter
# The task-scoped Stop/StopFailure hooks signal completion. Read the pane only
# for bounded diagnostics or to collect interactive output after that signal.
tmux capture-pane -p -J -t delegate -S -200
```

- **Turn detection:** use a task-scoped Claude `Stop` hook as the primary
  completion signal and a `StopFailure` hook as the failure signal. Give the
  run a unique id and owner-only status path; on `Stop`, persist the hook
  input's `last_assistant_message` with the run id and signal the matching
  `tmux wait-for` channel. On `StopFailure`, persist only the structured
  error type/message needed for diagnosis and signal failure. Bound every wait
  with a timeout and clean up the task session. Claude's hook contract fires
  `Stop` once after a completed turn and explicitly provides
  `last_assistant_message`; do not scrape the transcript or treat a visually
  stable pane as proof of completion. See
  <https://code.claude.com/docs/en/hooks#stop> and
  <https://code.claude.com/docs/en/hooks#stopfailure>.
  Use the shipped [turn-completion adapter](resources/claude-turn-completion.md)
  for the exact settings, launch, wait, result-validation, and cleanup
  contract; do not improvise a different parser or signal protocol.
- **Pane access:** capture only the dedicated task pane, after the completion
  signal or on bounded failure diagnosis. Never enumerate or capture unrelated
  tmux sessions; they may contain secrets or other users' work.
- **Effective autonomy is layered:** invoke the latest available Fable-class
  model directly at high by default, or xhigh for capability-sensitive,
  long-horizon, materially ambiguous, cross-cutting architecture/security,
  unresolved-disagreement, or failed/stalled-high work. Fable owns the native
  session and may use Opus medium for bounded deterministic tool/UI/MCP evidence
  collection or Opus high for ambiguous/multi-step tool operation; Fable
  interprets the evidence and owns the judgment. Launch with the selected
  effort, `--permission-mode auto`, and task-scoped settings containing
  `autoMode.classifyAllShell: true`. Require the effective
  project settings to keep the OS sandbox enabled, set
  `sandbox.failIfUnavailable: true`, auto-allow sandboxed Bash, and permit an
  auto-classified unsandboxed retry only for a trusted installed tool that
  requires host state. Arbitrary unsandboxed commands remain out of bounds.
  This preserves native tools, MCP servers, and broad public-network research
  while keeping secret stores, private-network access, destructive operations,
  and privilege changes behind explicit controls. Never use bypass mode on an
  ordinary host. See <https://code.claude.com/docs/en/permission-modes> and
  <https://code.claude.com/docs/en/sandboxing>.
- **Read-only consults:** keep "read and reason only; edit nothing" in the
  prompt, record the worktree state before launch, and compare the diff after
  completion. Auto mode enables useful inspection and test tools; it does not
  silently turn a consult into an edit handoff. If stronger write isolation is
  required, use a separate read-only checkout or filesystem boundary.
- **Edit handoff:** start a separate auto-mode session with explicit write
  authority and the worktree as its working directory so edits and commits land
  where the gates guard them. Never reuse a consult session as an implicit
  write grant.
- **Test-running review:** use a separate auto-mode interactive session with
  the same fail-closed sandbox. Instruct Claude to edit no source files and
  require a clean before/after worktree-diff comparison. If a fix is needed,
  return it to the task's approved producer.
- **Multiline prompts:** `set-buffer` + `paste-buffer -p`, then a separate
  `send-keys Enter`.
- **Interactive prompts:** classifier escalations, ambiguity, and other user
  questions are handled in the same dedicated session. They never authorize a
  write silently, and a visible question never substitutes for the terminal
  hook result.
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
shape CodeFlow ADR-0023 prohibits outright. There is no verified interactive lane to
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
