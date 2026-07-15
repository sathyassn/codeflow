---
name: cf-customize
description: Tailor a Codeflow scaffold to its project. Verify the core harness and test tools plus both interactive duo lanes—Claude Code to Codex through the official plugin, and Codex to Claude through tmux—along with task-specific MCP/UI tools. Then propose project-owned docs, policy, workflow, and model-selection changes. Use after init or when an update brings defaults to decide. Analyze before editing; never auto-install tools.
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
- **Duo** (`/cf-model-orchestrator`) — host-neutral when both native
  interactive seats are available: Claude Code → Codex through the official
  plugin, or Codex → Claude through interactive Claude CLI + tmux (ADR-0023).
- **Batch** — the pipeline preset; single-vendor by design, even when its
  assurance stages resemble parts of the duo.

Then verify and **offer** remediation — never install silently.

- **Core (every flow).** git, the `codeflow` binary, the active harness, and the
  stack test toolchain. Read inspectable health from **`codeflow doctor`**; its
  `delegates` check covers both CLIs, Codex auth/MCP, the Claude plugin/MCP, and
  tmux. Retain live interactive canaries because status commands cannot prove a
  native TTY session and its tools work.
- **Duo flow** (codex configured / `cf-model-orchestrator` in use):
  - **Claude-host lane** — `codex login status`, `codex mcp list`, the enabled
    `codex@openai-codex` plugin, and a scoped `/codex:setup`/tool canary. The
    plugin is the only sanctioned Claude → Codex transport; never use headless
    `codex exec`, a hand-rolled app-server driver, or tmux-driving Codex.
  - **Codex-host lane** — `claude`, `tmux`, and `claude mcp list`, followed by
    an authenticated interactive TTY canary and a task-scoped tmux round trip
    using Stop/StopFailure hook completion. Never use `claude -p` or pane
    stability as the work protocol.
  - **Task tools** — Playwright or an equivalent browser driver for web UI;
    Computer Use or a surface-specific driver for native/mobile/desktop UI;
    the project's format, lint, test, coverage, and security tools. Prove tool
    access through the actual peer lane, not only by listing configuration.
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
- outdated Codex or Claude CLI → present the vendor-supported update command
  after checking current release guidance; never rely on a version pinned here
- tmux absent →
  `brew install tmux` (or the platform's package manager)
- Playwright MCP absent → add a block to `~/.codex/config.toml`, then confirm it
  reads READY with `codex mcp list`:

  ```toml
  [mcp_servers.playwright]
  command = "npx"
  args = ["-y", "@playwright/mcp@latest"]
  ```

If a tool is absent and the user declines the fix, **degrade legibly**—name the
missing lane/evidence and use solo `/cf-develop`. Never label the run duo or
quietly replace the missing vendor with another instance of the host model.

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
  allowlists are ready; protected-branch globs; branch prefixes. **Footer
  policy** — the commit body is `-` bullets + a `BREAKING CHANGE:` footer only by
  default; every other trailer blocks until opted in. Decide per class: (a)
  **track tickets in commits?** if yes, set `commit_ticket_keys` (e.g.
  `["Refs","Closes"]`) to *allow* them, then `commit_ticket_required`
  (`warn`/`block`) if every commit must *carry* one, and `commit_ticket_pattern`
  (e.g. `^PROJ-\d+$`) to constrain the ID format; (b) **DCO / sign-off?** add the
  token to `commit_required_footers` (e.g. `["Signed-off-by"]`) to require it on
  every commit; (c) any other trailer a workflow needs → add it to
  `commit_footer_tokens` to allow it. Leave every list empty for the strict
  default — an agent fills any slot you open, so open only what you mean.
- **`.claude/workflows/pipeline.workflow.js`** — per-stage model selection and
  the honest single-vendor assurance preset. It is user-owned; `codeflow update`
  never touches it.
- **Harness model/MCP config** — choose the strongest supported model and high
  reasoning available to each seat, and configure the MCP/UI tools the project
  needs. Record selected versions as run evidence; keep fast-aging model names
  out of shared skills and doctrine.

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
