---
name: cf-customize
description: Walk a codeflow-scaffolded project and tailor it to the project — verify the tools its flows need (git and the harness for every flow; for the duo flow the codex-plugin-cc plugin, codex, and its MCP servers; tmux where a codex seat consults claude; the stack's test toolchain) and offer to install or fix what is missing, then fill the project-owned specifics that `codeflow init` leaves generic — docs/product.md, the AGENTS.md/CLAUDE.md project sections, policy.json gate levels, and model pins. Analysis-then-propose — a prioritized report first, then applied interactively on a working branch through a PR. Use after `codeflow init`, or any time the codeflow surface needs tailoring or a `codeflow update` brought new defaults to decide. Never auto-installs a tool and never auto-runs itself — it offers, you confirm.
---

# cf-customize — tailor a scaffolded project to itself

`codeflow init` scaffolds the generic surface; this fills in the project. Init
seeds one-liners, generic templates, and warn-level defaults — the reasoning
about *this* project's purpose, its tools, and where its gates should sit is left
for a thinking session. That is this skill: the reasoning-layer companion to
`init`, not a mechanical command.

Two jobs, run in this order: **Part A** verifies the tools the project's flows
need and offers to fix what is missing; **Part B** fills the project-owned
artifacts still sitting at template defaults.

## How to run it

1. **Analyze first, propose second — the default.** Produce one prioritized
   report of what is missing (Part A) and what is still generic (Part B) *before*
   touching anything. Then walk the fixes interactively, one at a time.
2. **On a working branch.** Do the work on `chore/codeflow-customize` (or another
   `chore/` branch), scoped commits, and land it through a PR like any change —
   never on a protected branch. Push the branch for durability as you go (backup,
   not a merge).
3. **Idempotent.** Safe to run at init *and* to re-run post-init — e.g. after a
   `codeflow update` ships new defaults (like the `security_review` / `dep_audit`
   gate levels) that need a project decision. A section already tailored is left
   alone; only what is still at a default is proposed.

## Part A — flow-aware tool preflight

First decide **which flows this project uses**, then verify each flow's tools:

- **Solo** (`/cf-develop`) — always in play.
- **Duo** (`/cf-model-orchestrator`) — when codex is configured (a `.codex/`
  starter is present, or the orchestrator skill is in the set). The duo is
  driven **from Claude Code** through the `codex-plugin-cc` plugin (ADR-0018);
  from any other harness it is unavailable, so verify its tooling *for* the
  Claude Code seat rather than for this session.
- **Batch** — the pipeline preset; needs the core tools plus whatever stages it
  composes (often the duo stages).

Then verify and **offer** remediation — never install silently.

- **Core (every flow).** git, the `codeflow` binary, and the harness (`claude` /
  `codex`). Read these off **`codeflow doctor`** rather than reinventing them: its
  `claude` check is harness presence, its `delegates` check is codex presence +
  authentication. Lean on that output.
- **Duo flow** (codex configured / `cf-model-orchestrator` in use):
  - **codex driver** — the official **`codex-plugin-cc`** plugin is the **only
    lane** for driving codex from Claude Code (ADR-0018: it wraps the
    app-server, is OpenAI-maintained, and spares a hand-rolled driver; headless
    `codex exec`, direct app-server driving, and tmux-driving codex are all
    prohibited). Check whether it is installed; if not, see the remediation
    rule — the fix runs only from a Claude Code session.
  - **Installed + authenticated** — `codex login status` (exit 0 +
    "Logged in using ChatGPT"; the same signal doctor's `delegates` reports).
  - **Healthy** — `codex doctor` (it diagnoses installation, config, auth, and
    runtime health; flag a damaged state DB or any issue it reports), and
    `codex --version` against the codex-cli version the plugin lane's MCP
    access was last verified on (0.144.1, in the orchestrator's "Tool access —
    verified" note) so that verification cannot rot.
  - **Required MCP servers READY** — `codex mcp list` (configured servers +
    status). For the duo's UI e2e, **Playwright** must be present *with tools*
    — confirm by delegating one browser e2e through the plugin, per the
    orchestrator's verify note. `computer-use` is optional and desktop-only; it
    is not needed — Playwright covers web e2e.
  - **tmux** — needed only for the **codex → claude lane** (a codex seat
    consulting claude by driving the interactive `claude` CLI — see
    `cf-delegate`). It is not a duo driver: the duo runs from Claude Code
    through the plugin, and tmux-driving codex is prohibited (ADR-0018).
- **Stack test toolchain.** The runner the detected stack tests with — cargo /
  npm / pytest / go — aligned with `cf-stack` and what `codeflow test` invokes.
  A missing runner means the test gate cannot run.

**Remediation rule (critical).** For each missing or outdated tool, **print the
exact fix and confirm before running it.** Never silently auto-install:
installing or updating a system tool is privileged and reaches outside the repo,
so it gets the same offer-and-confirm posture codeflow takes for any irreversible
or outward action. The fixes:

- codex-plugin-cc not installed → **from a Claude Code session only**:
  `/plugin marketplace add openai/codex-plugin-cc` → `/plugin install
  codex@openai-codex` → `/reload-plugins` → `/codex:setup`. These are Claude
  Code slash commands — if this session is another harness, do **not** offer
  them; report the gap for the user to fix from a Claude Code session.
- codex present but unauthenticated → `codex login`
- codex behind the pinned version → `codex update`
- tmux absent (and the project has codex seats that consult claude) →
  `brew install tmux` (or the platform's package manager)
- Playwright MCP absent → add a block to `~/.codex/config.toml`, then confirm it
  reads READY with `codex mcp list`:

  ```toml
  [mcp_servers.playwright]
  command = "npx"
  args = ["-y", "@playwright/mcp@latest"]
  ```

If a tool is simply absent and the user declines the fix, **degrade legibly** —
name what the project loses. The duo flow already falls back to solo `/cf-develop`
silently when either half is missing — a non-Claude-Code seat, the plugin
surface, or codex itself (`cf-model-orchestrator`'s degradation); say so, so
declining is an informed choice, not a surprise.

## Part B — project-artifact customization

Analyze what is still at template defaults, then propose filling each. Walk them
interactively — and never invent product facts; the content is the user's, you
draft and confirm.

- **`docs/product.md`** — the WHY layer: purpose, users, scope, non-goals. Init
  seeds only the one-liner; fill the four sections. Non-goals are the
  load-bearing part — `cf-plan` checks new work against them.
- **`AGENTS.md` / `CLAUDE.md`** — the project-owned sections *outside* the
  `codeflow:managed` markers: project-specific instructions, non-goals, stack
  specifics. **Never edit inside the managed block** — `codeflow update` owns it.
- **`.codeflow/policy.json`** — gate levels: harden `dep_audit` /
  `security_review` from `warn` → `block` once the project's scanners and
  allowlists are ready; protected-branch globs; branch prefixes.
- **`.claude/workflows/pipeline.workflow.js`** — per-stage models and whether to
  default to the duo preset. It is user-owned; `codeflow update` never touches it.
- **`.codex/config.toml`** — model pins and MCP servers (adding Playwright for duo
  e2e is the concrete gap surfaced in Part A).
- **`cf-model-orchestrator` model pins** — Claude Fable 5 / codex `gpt-5.6-sol`,
  if the project runs the duo flow (see the orchestrator's Models section).

For each: propose the change, get the user's content, write it, commit in a
scoped unit, and push for durability. `codeflow validate` and `codeflow doctor`
green before reporting done.

## Reminder, not auto-run

cf-customize needs an interactive thinking session, so it is **never auto-run**
(`codeflow init` may be non-interactive or run in CI). The intended surface is a
*reminder*: a `codeflow init` closing hint to run `/cf-customize`, plus an
orient/doctor nudge while `docs/product.md` or the AGENTS.md/CLAUDE.md project
sections are still at template defaults, clearing once they are filled. None of
that is wired into the engine yet — today the user (or an agent reading this
skill) invokes `/cf-customize` by hand. Do **not** propose auto-running it.

Report completion with the prioritized findings, the tool fixes applied or
declined (and what each declined fix costs), the artifacts filled, and the gate
output. Hand off to `cf-ship` to land the PR — a human merges it.
