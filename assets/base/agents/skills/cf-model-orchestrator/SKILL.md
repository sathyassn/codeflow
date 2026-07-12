---
name: cf-model-orchestrator
description: Decide solo vs duo for planned dev work, then run it. Duo adds codex as a second, independently-trained reviewer alongside the cf-reviewer subagent in the build → review → verify loop, reserved for real blast radius in a codeflow repo — a Tier-3-grade decision (new dependency, schema/API change, module boundary change), anything touching auth, secrets, payments, or a data migration, or a change to a shipped capability's public contract. Use when about to build a feature, change, or fix that already has acceptance criteria and whose stakes go beyond a small, easily self-reviewed edit — not for a trivial fix, a conversational answer, or a docs-only change. Silently degrades to solo /cf-develop when codex is missing or unauthenticated — never blocks or prompts for auth.
---

# cf-model-orchestrator — solo or duo, then drive it

Decide how many independent models guard this work, then run it. This is an axis
**orthogonal** to the weight ladder in `cf-method` (how *much* process): it sets
*who reviews* — the `cf-reviewer` subagent alone (solo), or codex as a second,
independently-trained model alongside it (duo). It never replaces `/cf-develop`;
solo **is** `/cf-develop`.

## Decide: solo or duo

1. The work must already have acceptance criteria (from an epic, task, spec, or
   the prompt). None stated → run the clarity gate in `cf-plan` first; never
   build against a guess.
2. **Preflight codex** (the same check as `cf-delegate` and `codeflow doctor`
   delegates):

   ```sh
   codex login status   # exit 0 + "Logged in using ChatGPT" → duo is available
   ```

   `codex` missing from PATH, or non-zero exit → **duo is unavailable. Silently
   run solo `/cf-develop`** with `cf-reviewer` as the independent pass, and note
   it once in your report. Never prompt for `codex login`, never nag — duo was
   never promised.
3. Available **and** the stakes clear the bar → **duo**. The bar (the skill
   description is the canonical list): a Tier-3-grade change (new dependency,
   schema/API change, module-boundary change), anything touching auth, secrets,
   payments, or a data migration, or a change to a shipped capability's public
   contract. A trivial fix, a docs-only change, or an easily self-reviewed edit
   → solo. On a genuine judgment call, prefer duo.

## The duo loop

Claude is the **primary orchestrator** — it plans, analyses, designs, and drives
the loop. Codex is the **independent second model** — it reviews the plan, then
executes it. The roles are fixed: two independently-trained models guard the work
at the two points self-review is weakest — the plan and the final verify.

1. **Plan (Claude).** Produce a plan with detailed epics/tasks and **acceptance
   criteria that cover the happy path *and* the edge/error cases** — on the
   capability→epic→spec→task spine (`cf-plan`, `cf-method`). The ACs are the
   contract both models sign at plan-align and the rubric at verify.
2. **Plan-align (codex reviews — or plans in parallel and cross-verifies).**
   Hand codex the plan; it critiques it against the ACs, or writes its own and
   you reconcile. Iterate to agreement, **bounded to ≤2 rounds** — no agreement
   → stop and ask the human. Never launder a disagreement into a default.
3. **Execute + first test (codex).** Codex implements the agreed plan on a
   correctly prefixed feature branch in a worktree, and runs the **first round of
   testing, including Playwright-driven UI e2e** through its MCP tool set.
4. **Final verify + review (Claude).** Do the final testing and review yourself.
   **Grade every acceptance criterion** (happy and edge) with file:line or
   command evidence, plus the general test/review requirements, the
   project-specific requirements, and the CI requirements. Spawn `cf-reviewer`
   for the independent read-only pass.
5. **Fix loop.** Feed findings back; codex reworks. Bounded (share the
   `cf-develop`/pipeline rework budget). **All criteria and all gates green is a
   MUST before any push or PR** — no model talks a red gate green.

## Models

Principle: **each side runs its vendor's latest frontier model at high reasoning
effort** — the cross-vendor, cross-training independence is the whole point, so
never quietly drop either to a cheaper tier. Current pins (as of 2026-07-10;
re-verify at each codex upgrade via the harness-parity canary so they cannot rot
silently):

- **Claude** = Fable 5, `high` (or `xhigh`) effort.
- **Codex** = `gpt-5.6-sol` at `xhigh` — set the pin in `.codex/config.toml`:
  `model = "gpt-5.6-sol"`, `model_reasoning_effort = "xhigh"`.

## Driving codex

Drive codex through the official **`codex-plugin-cc`** (PRIMARY) — the
OpenAI-maintained Claude Code plugin that wraps the codex app-server, so it gives
codex's full tool set (including the Playwright MCP for UI e2e), resumable
sessions, and code-answered approvals — without a hand-rolled driver to keep in
step with the experimental app-server API (OpenAI owns that churn). Install once:
`/plugin marketplace add openai/codex-plugin-cc` → `/plugin install
codex@openai-codex` → `/reload-plugins` → `/codex:setup` (needs `codex login`).
Use its commands for the duo:

- `/codex:review` + `/codex:adversarial-review` — the plan cross-verify and the
  cross-vendor security red-team (both read-only).
- `/codex:rescue` — delegate execution and the first round of testing.
- `/codex:transfer` — a persistent codex thread (`codex resume <id>`) for the
  multi-round back-and-forth.

**Tool access — verified.** Against codex-cli 0.144.1, a delegated `/codex:rescue`
task reports codex's **full MCP tool set** — the `playwright` MCP present with its
24 browser tools (`browser_navigate`, `browser_click`, `browser_snapshot`, …),
alongside codex's other MCP servers — in a resumable multi-turn thread, **not a
headless one-shot**; the sandbox is read-only for a diagnostic and opens to
workspace-write for a fix. Re-confirm on a new codex/plugin version, or if your
own `~/.codex` MCP config differs, by delegating one browser e2e. Only if a
delegated task genuinely can't reach the browser MCP, fall back to the app-server
driver for the e2e step.

**Advanced fallback — drive the app-server directly.** For fully-programmatic
driving without slash commands, or where the plugin cannot be installed, the raw
app-server JSON-RPC protocol, the robustness rules, and a reference driver script
live in
[`resources/codex-app-server-driver.md`](resources/codex-app-server-driver.md).
Degrade to **tmux**-driving only where the app-server itself is unavailable.

## Security — mandatory, cross-vendor

A duo run **must** include a cross-vendor security / red-team pass before any push
or PR (the pipeline `security` stage, driven by the `cf-security-reviewer` agent —
being added in this same batch; reference it by name as the mechanism). Both
models review for the vuln classes; the **joint verdict gates push/PR**, while the
deterministic `codeflow test` / `codeflow validate` gate stays authoritative — a
model verdict never turns a red deterministic gate green. Do not design the
security stage here; just require it and stop.

## Degradation — never give up, but never lie

Two failures, two responses:

- **Absent at the start** (codex not on PATH, or `codex login status` non-zero):
  duo was never promised → **silently degrade to solo `/cf-develop`**, note it
  once, carry on.
- **Mid-flow failure** (a wedged turn, a failed MCP precheck, a crashed session,
  a 401 mid-run): **diagnose, retry within bounds, and escalate to the human** —
  never silently abandon the run, and never quietly finish solo as if the duo
  pass had happened.

## Durability — push the working branch

If a remote is configured, **push the working branch after each committed logical
unit** so work survives a machine failure: `-u` on the first push;
`git push --force-with-lease` (never bare `--force`) when history was rewritten.
This is **backup only** — it never merges and never bypasses a gate (the
pre-commit `secret_scan` gate still protects pushed content). No remote → no-op.

Report completion with the plan-align outcome, the graded criteria (each with
evidence), the security verdict, and the gate output. Hand off to `cf-ship` to
land it — a human merges the PR.
