---
name: cf-delegate
description: Consult or delegate to the other vendor's native coding harness under its own subscription auth. Covers full task handoffs, the two interactive-only transport lanes, the durable reverse-lane turn lifecycle, edit-access doctrine, and guardrails. Use for a specialty pass or genuinely parallel edit handoff (CodeFlow ADR-0023). Do not use for a read-only second opinion (cf-consult).
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
codex ──durable delegate lifecycle over interactive claude CLI──▶ claude
```

- **Claude Code → codex: the official `codex-plugin-cc` plugin, only.** It
  wraps the codex app-server — the same interactive engine as the TUI — so a
  delegated task gets codex's full MCP toolset (Playwright verified with 24
  browser tools on codex-cli 0.144.1, 2026-07-11), a resumable thread, and
  in-band approvals.
- **codex → claude: the interactive `claude` CLI driven through CodeFlow's
  schema-v2 delegate lifecycle, only** (CodeFlow ADR-0036; pattern below). The
  lifecycle owns startup, acceptance, and terminal correlation. When
  `HERDR_ENV=1`, load `cf-herdr` and run that Claude process in a named Herdr
  tab; tmux is the degraded TTY host when Herdr is unavailable. Herdr
  `idle`/`done` is not turn completion. The account and interactive response
  must be verified with a scoped TTY canary; verify the full lifecycle round
  trip on install and whenever the Claude CLI or hook configuration changes.

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
- **From codex:** `claude` on PATH, `claude mcp list` succeeds, and a short
  interactive TTY canary gets an authenticated response. When `HERDR_ENV=1`,
  require `herdr` and load `cf-herdr`; `tmux` is not required. Outside Herdr,
  require `tmux`. `codeflow doctor` may still flag missing tmux; that is the
  degraded-path preflight, not a Herdr blocker. Do not infer authentication
  from a status subcommand when it conflicts with a working interactive
  session.

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

Start every delegated plugin prompt with an explicit bounded role, for example
`ROLE: peer. Complete only this bounded assignment. Do not start the top-level
model orchestrator or delegate back to the host lineage (Claude).` Use `worker` instead of `peer`
only for a primary-owned subtask. A generic Claude subagent is not a Codex
delegate, and a native Codex thread that recursively starts another duo has
violated the assignment rather than completed it.

Read the current Codex primary selector, default effort, and permitted
worker classes from
`../cf-model-orchestrator/resources/current-ensemble.json`. Invoke the primary
directly with that selector and default effort; workers take escalation. Any worker requires observed native
routing; the invoked primary retains the task, implementation, verification,
and verdict. If `.codeflow/model-selection.json` is nonempty, first require
`codeflow doctor --check model-bindings` to pass and use only its effective
qualified override for the active harness.

Include difficulty/triggers. The primary applies capability-routing:
default effort is not a ceiling; demanding work gets qualified high/xhigh
workers, direct xhigh when warranted. Preserve proportionate routine work,
primary approval, and cross-lineage review.

**Output counts as Codex only with a native Codex thread behind it.** Every
plugin exchange must yield the native thread ID, recheckable afterward
through the plugin or the native Codex surface. A generic Claude subagent, an
unverified relay, or any surface that cannot show that thread never counts as
Codex. Record model and effort as *observed* only when the transport exposes
the actual values; otherwise record them as *requested* — never silently
upgrade requested to observed. When completion is inferred from thread state
rather than an explicit result, grade it explicitly as inferred and verify it
through the thread before relying on it.

Ask every consult for a closing `VERDICT: approved|changes_requested` line so
the reply is checkable, and branch on it — then re-derive the findings
yourself (see Guardrails).

## Lane 2 — from codex, the durable lifecycle over the interactive claude CLI

CodeFlow's schema-v2 lifecycle proves what a terminal signal alone cannot:
the session started cleanly, the delivered prompt was accepted as the armed
turn, and the terminal event belongs to that turn. The sequence, compactly:

```sh
# Read the managed defaults, then any doctor-validated project override.
CLAUDE_MODEL="<claude-primary native selector>"
CLAUDE_EFFORT="<default effort>"
codeflow delegate init --run-id run-42 --state-dir "$STATE"  # prints generated settings.json
tmux new-session -d -s cf-run-42 -x 220 -y 50 -c /path/to/worktree \
  "claude --model $CLAUDE_MODEL --effort $CLAUDE_EFFORT --permission-mode bypassPermissions --settings $STATE/settings.json"
# consult / no-edit review (keep auto):
# claude --model $CLAUDE_MODEL --effort $CLAUDE_EFFORT --permission-mode auto --settings $STATE/settings.json
codeflow delegate wait --run-id run-42 --state-dir "$STATE" --until ready --timeout-seconds 120
codeflow delegate arm --run-id run-42 --state-dir "$STATE" --turn-id turn-1 --prompt-file "$P"
tmux load-buffer -b cf-run-42-turn-1 "$P"; tmux paste-buffer -p -b cf-run-42-turn-1 -t cf-run-42
sleep 0.3  # bounded TUI input-settle; this is not completion detection
tmux send-keys -t cf-run-42 Enter
codeflow delegate wait --run-id run-42 --state-dir "$STATE" --until accepted --turn-id turn-1 --timeout-seconds 120
codeflow delegate wait --run-id run-42 --state-dir "$STATE" --until terminal --turn-id turn-1 --timeout-seconds 3600
```

When `HERDR_ENV=1`, start Claude in the named Herdr tab per `cf-herdr`
(include `--settings` and the selected model/effort; production
`bypassPermissions`, consult auto).
Deliver the armed file with `herdr pane send-text` then Enter as that skill
names; do not `tmux load-buffer` into a Herdr pane. Lifecycle waits stay the
completion signal.

- **Turn detection is the lifecycle, not the pane.** `init` creates an
  owner-only state directory outside every Git worktree and generates the
  task settings that wire `SessionStart`, `UserPromptSubmit`, `Stop`, and
  `StopFailure` to `codeflow hook delegate-turn --state-dir`. The generated
  settings file is **immutable** — every later call revalidates it against
  exactly (run id, state-dir spelling) and rejects any difference. Arming
  records the SHA-256 of canonical UTF-8 prompt bytes with internal LF line
  endings, no terminal line break, and no other control characters; `arm` rejects
  empty or other noncanonical input before durable turn state is created.
  Normalize once before arming, then
  deliver that same file exactly
  (buffer paste, a bounded 300 ms input-settle, then one separate Enter);
  acceptance and terminal records bind
  session and `prompt_id`. Waits are bounded with stable exit states —
  `0` observed (a completed terminal prints the result JSON), `10` failed
  terminal, `11` poison/unsafe, `124` timeout, `130` interrupt. Restarts,
  mis-correlated events, and interrupted waits after acceptance poison the
  run; recovery is a new run id in a fresh state directory. Turns are
  sequential — one outstanding armed turn per run; arm the next turn id in
  the same session after each terminal result. Use the shipped
  [turn lifecycle adapter](resources/claude-turn-completion.md) for the exact
  contract; do not improvise a different parser or signal protocol, and do
  not scrape transcripts or treat a visually stable pane as completion.
- **Sibling Stop-hook preflight.** Before delivery, enumerate the effective
  Stop-hook set from every source the session loads (user/project/local
  settings, enabled plugins, task settings). Reject any sibling Stop hook
  whose nonblocking behavior you do not deterministically know. The one
  currently known-safe sibling is the official Codex plugin's
  `stop-review-gate-hook.mjs`, and only when the operator confirms its
  effective `stopReviewGate` is off through the plugin's own surface.
  CodeFlow never reads or infers plugin-private state; an unknown or
  unverified sibling fails the preflight.
- **Pane access is diagnosis-only.** Capture only the dedicated task pane,
  and only for bounded diagnosis when a wait times out or a result is
  malformed, or to answer an explicit in-turn dialog. Never enumerate or
  capture unrelated tmux sessions; they may contain secrets or other users'
  work. If acceptance times out and the dedicated pane explicitly shows the
  paste attachment still waiting in the input editor, send Enter once more and
  re-wait once. Never issue blind or repeated Enter retries.
- **Effective autonomy is layered:** invoke the Claude primary with the selector
  and default effort from
  `../cf-model-orchestrator/resources/current-ensemble.json`. The primary owns
  the native session, internal worker routing, interpretation, and judgment.
  Launch with default effort and the generated task settings; workers take
  escalation. Make
  `autoMode.classifyAllShell` effective at user scope (Claude ignores it at
  project scope, and repeated `--settings` flags are not a supported merge
  contract; the generated task file carries only the lifecycle hooks). On the
  degraded tmux path, use production `--permission-mode bypassPermissions` (consult: auto) and keep the OS sandbox
  enabled with `sandbox.failIfUnavailable: true`, auto-allow sandboxed Bash,
  and permit an auto-classified unsandboxed retry only for a trusted
  installed tool that requires host state. When `HERDR_ENV=1`, native flags
  come from `cf-herdr` (production bypass; consult auto). That is not a
  write grant and not hook-trust bypass. Never
  `--dangerously-skip-permissions` unless the operator named it, and never
  `--dangerously-bypass-hook-trust`. Consults still edit nothing. See
  <https://code.claude.com/docs/en/permission-modes> and
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
- **Interactive prompts:** classifier escalations, ambiguity, and other user
  questions are handled in the same dedicated session. They never authorize a
  write silently, and a visible dialog never substitutes for the terminal
  lifecycle result.
- **Follow-ups:** the session keeps its context — arm the next turn and
  deliver to the same pane.
- **Cleanup:** after harvesting the bounded result and the evidence
  verification needs, kill the task session and remove the state directory
  and private prompt files.
- **Legacy:** `codeflow hook delegate-turn --result` remains only as
  byte-compatible compatibility for existing callers until a later major
  release; the two hook modes are mutually exclusive and never fall back to
  one another. New work always uses the lifecycle.

## Evidence contract — five obligations, both lanes

The two adapters stay distinct — the plugin lane is vendor-owned forward
transport; the lifecycle lane is CodeFlow-owned reverse protocol — but every
delegated exchange meets the same five obligations:

1. **Launch.** Verify the delegated session actually started: `wait --until
   ready` on the reverse lane; a created native thread or first output within
   a bounded window on the forward lane. Silence is not a launch.
2. **Provenance.** Attribute output only with native runtime provenance: the
   schema-v2 records binding session, digest, and `prompt_id` on the reverse
   lane; the native Codex thread ID plus model/effort labeled by source
   (`observed` when exposed, otherwise `requested`) on the forward lane. A
   relay is transport, not author.
3. **Return.** Verify the returned work itself — the bounded terminal message
   or thread result, the scoped worktree diff, and the cited evidence,
   re-derived by you.
4. **Failure.** Failure is legible and bounded: stable exit states, durable
   poison, or an explicit plugin/harness error, then one bounded retry with
   diagnosis — never silent substitution, never completion inferred from
   silence.
5. **Recheck.** Evidence stays recheckable after the fact: durable state
   records until cleanup on the reverse lane; the resumable native thread on
   the forward lane. Record model/effort as observed only when the transport
   exposes actual values, otherwise as requested — never silently upgrade —
   and grade inferred completion explicitly as inferred.

## Edit-access doctrine (delegate tier)

A delegate that edits works **only** inside a worktree on a feature branch —
the same worktree-per-session discipline that binds every agent here. From
Claude Code, scope `/codex:rescue` to the worktree; from codex, start the
lifecycle claude session in the worktree. Let it commit conventionally.

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
