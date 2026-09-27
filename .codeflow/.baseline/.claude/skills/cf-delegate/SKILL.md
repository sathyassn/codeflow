---
name: cf-delegate
description: Delegate to the other vendor's native coding harness under its own subscription auth, with qualified transport, lifecycle evidence and scoped edit access. Use for a specialty pass or genuinely parallel edit handoff. Use cf-consult for a read-only second opinion.
---

# cf-delegate — cross-vendor consult and delegate

Compose native harnesses at the process boundary, each under its own subscription
auth. CodeFlow's gates judge the output, not the author. You own and verify
every returned result.

The delegate is a vendor you are **not**: from Claude Code that is codex; from
codex that is claude. Consulting or delegating to your own vendor is
self-review with extra steps — never label it independent.

## Consult, delegate, or neither

The orchestrator owns required cross-lineage planning/review. Within that flow,
choose the authority this assignment needs:

- **Consult:** an independent critique of a design, diff or question. The peer
  reads, tests and reasons; it edits no source. Use `cf-consult`.
- **Delegate:** an explicitly scoped implementation unit in its own worktree.
  Reserve additional handoffs for independent parallel work or specialty value;
  coordination must earn its cost.
- **Neither:** ordinary local steps need no separate handoff. This does not
  waive the orchestrator's required independent planning or review.

When uncertain about edit authority, consult first; never turn a read-only
assignment into an implicit write grant.

## Transport — preferred lanes, qualified native fallback

```text
Claude Code ──official plugin (preferred) or qualified native client──▶ codex
codex ──durable delegate lifecycle over interactive claude CLI──▶ claude
```

- **Claude Code → codex: prefer the official `codex-plugin-cc` plugin.** It
  wraps the codex app-server — the same interactive engine as the TUI — so a
  delegated task gets codex's full MCP toolset (Playwright verified with 24
  browser tools on codex-cli 0.144.1, 2026-07-11), a resumable thread, and
  in-band approvals. When unavailable or incompatible, use a qualified official
  Codex App/interactive CLI route under the fallback contract below. A missing
  plugin is not proof that Codex itself is unavailable.
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
`--print`), and driving the codex app-server through hand-rolled JSON-RPC.
CodeFlow requires verified native sessions with the task's tools and guards;
it does not infer those capabilities from a process label or terminal host.
Use vendor-supported clients instead of maintaining a competing broker.
Status commands are not work sessions
— `codex login status`, `codex --version`, `codex mcp list`, and the plugin
install/setup steps stay fine.

For an incompatible or unavailable preferred lane, read
[qualified native fallback](resources/native-fallback.md) before choosing
another client. It preserves all five evidence obligations and the effective
safety boundary; it is not permission to route around a security denial.

## Preflight — is the delegate even available

Check the preferred lane, then any qualified native fallback. Additional edit
handoffs are optional; the orchestrator's required independent review is not.

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

If no qualified native route remains, record the unavailable seat and reduced
assurance; never silently substitute your own vendor or claim duo completion.
A missing CLI alone does not rule out a qualified App route. If a route is present
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

Collect peer in-turn via public foreground/qualified native fallback; no host
`run_in_background` watcher. The Codex-host turn adapter does not apply here.

Start every delegated plugin prompt with an explicit bounded role, for example
`ROLE: peer. Complete only this bounded assignment. Do not start the top-level
model orchestrator or delegate back to the host lineage (Claude).` Cross-family
entry always targets the primary at default effort. Only that primary may
dispatch same-family `ROLE: worker` escalation; never call a foreign worker
directly. Claude subagents are not Codex; nested duos violate scope.

Read the current Codex primary selector, default effort, and typed internal
routes from
`../cf-model-orchestrator/resources/current-ensemble.json`. Invoke the primary
directly with that selector and default effort; the receiving primary alone
selects its permitted internal route. Follow the canonical candidate versus
scoped-qualified and actual-execution contract in
`../cf-model-orchestrator/resources/capability-routing.md`: a candidate with
proven native routing may perform bounded non-design work without gaining a
qualification claim. The responsible primary retains scope, integration,
acceptance and the accountable verdict; record the actual executor and
authored lineage for review. If `.codeflow/model-selection.json` is nonempty, first require
`codeflow doctor --check model-bindings` to pass and use only its effective
qualified override for the active harness.

Include difficulty/triggers. The primary applies capability-routing: default
effort is not a ceiling; demanding work gets the strongest capable permitted
reasoning route and direct xhigh when warranted, while substantial bounded routine work
uses a capable permitted route when available. Select worker effort for the
unit. Preserve primary accountability and actual-authored-lineage review; do
not infer economy, qualification, availability or applied selection.

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
codeflow delegate init --run-id run-42 --state-dir "$STATE" \
  --model "$CLAUDE_MODEL" --effort "$CLAUDE_EFFORT"  # prints generated settings.json
tmux new-session -d -s cf-run-42 -x 220 -y 50 -c /path/to/worktree \
  "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1 claude --model $CLAUDE_MODEL --effort $CLAUDE_EFFORT --permission-mode bypassPermissions --settings $STATE/settings.json"
# For consult/no-edit, use the same launch with --permission-mode auto.
codeflow delegate wait --run-id run-42 --state-dir "$STATE" --until ready --timeout-seconds 120
codeflow delegate arm --run-id run-42 --state-dir "$STATE" --turn-id turn-1 --prompt-file "$P"
tmux load-buffer -b cf-run-42-turn-1 "$P"; tmux paste-buffer -p -b cf-run-42-turn-1 -t cf-run-42
sleep 0.3  # bounded input-settle; not completion detection
# Only if the input shows a "[Pasted text" attachment:
tmux send-keys -l -t cf-run-42 'Carry out the pasted instructions.'; sleep 0.3
tmux send-keys -t cf-run-42 Enter
codeflow delegate wait --run-id run-42 --state-dir "$STATE" --until accepted --turn-id turn-1 --timeout-seconds 120
codeflow delegate wait --run-id run-42 --state-dir "$STATE" --until terminal --turn-id turn-1 --timeout-seconds 3600
```

When `HERDR_ENV=1`, use the named Herdr tab per `cf-herdr` (launch-local task
environment, `--settings`, model/effort, production `bypassPermissions`,
consult auto); deliver the armed file with `herdr pane send-text` as that
skill names, never `tmux load-buffer`. Lifecycle waits stay the completion
signal.

- **Turn detection is the lifecycle, not the pane.** `init` creates owner-only
  state outside every Git worktree and wires `SessionStart`, `UserPromptSubmit`, `Stop`, and
  `StopFailure` to `codeflow hook delegate-turn --state-dir`. The generated
  settings file is **immutable** and bound to run id and state-dir spelling;
  mismatches are rejected. Arming records the SHA-256 of canonical UTF-8 prompt bytes with internal LF line
  endings, no terminal line break, and no other control characters; `arm` rejects
  empty or other noncanonical input before durable turn state is created.
  Normalize once before arming, then
  deliver that same file exactly
  (buffer paste, a bounded 300 ms input-settle, the adapter's fixed
  sentence only when the input shows a paste attachment, then one Enter);
  acceptance and terminal records bind
  session and `prompt_id`. Waits are bounded with stable exit states
  (listed in the adapter). Restarts,
  mis-correlated events, and interrupted waits after acceptance poison the
  run; recovery is a new run id in a fresh state directory. Turns are
  sequential — one outstanding armed turn per run; arm a new id in the same
  session after each terminal result. On this Codex host lane, use the shipped
  [turn lifecycle adapter](resources/claude-turn-completion.md) for exact
  mechanics; never improvise a parser, scrape transcripts, or use pane
  stability as completion.
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
  malformed, to answer an explicit in-turn dialog, or once after a paste to
  see whether the input shows a paste attachment. Never enumerate or
  capture unrelated tmux sessions; they may contain secrets or other users'
  work. If acceptance times out and the pane shows the prompt still waiting,
  send Enter once more and re-wait once; never blind or repeated Enter.
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
  return it to the task's responsible primary and designated executor.
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
   (`observed` when exposed, otherwise `requested`) on the forward lane. On
   the reverse lane the terminal `wait` result already carries `provenance`:
   the thread (the Claude session), and model and effort as `requested` at
   `init` and `observed` at session start, each `unknown` when not given.
   Cite that record; do not record these by hand. A relay is transport, not
   author.
3. **Return.** Verify the returned work itself — the bounded terminal message
   or thread result, the scoped worktree diff, and the cited evidence,
   re-derived by you.
4. **Failure.** Failure is legible and bounded: stable exit states, durable
   poison, or an explicit plugin/harness error, then one bounded retry with
   diagnosis — never silent substitution, never completion inferred from
   silence.
5. **Recheck.** Evidence stays recheckable after the fact: durable state
   records until cleanup on the reverse lane; the resumable native thread on
   the forward lane. On the forward lane, record model/effort as observed
   only when the transport exposes actual values, otherwise as requested
   (never silently upgrade), and grade inferred completion explicitly as
   inferred. On the reverse lane the lifecycle records them.

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

`agy` has no verified interactive lane, only headless one-shot CLI use, which
CodeFlow ADR-0023 prohibits, so it is **not** a delegate tier; if the user
names it, say the transport rule rules it out. It stays bound by the
harness-agnostic git-hook plane and CI. For its history and the experimental
guard binding when `agy` is someone's harness, read
[agy notes](resources/agy.md).

## Guardrails

- **Never automate vendor auth.** The user logs in manually, one account per
  side; degrade legibly on missing/401.
- **Every delegate prompt narrows authority and data.** Name purpose, permitted
  actions/files/resources/data/processors/destinations/effects and step budget;
  ambiguity blocks—never guess. Send only necessary minimized data to an
  approved processor; route qualification is not data authority. An
  already-authorized scoped handoff needs no new approval; the lead verifies
  effects and claims.
- **Synthesize, never paste.** A finding is input, not conclusion: re-derive and
  cite it, state agreement/disagreement; unverified remains unverifiable.
- **Stay within ToS.** This process-boundary composition is sanctioned (OpenAI
  ships the plugin itself); the single-account, manual-auth, degrade-on-401
  posture is what keeps it there.
