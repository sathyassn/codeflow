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
  schema-v2 delegate lifecycle, only** (CodeFlow ADR-0036). A Codex host
  follows its host and canary rules in
  [the lifecycle lane](resources/lane-lifecycle.md).

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

Check the preferred lane, then any qualified native fallback.

- **From Claude Code:** the plugin surface and Codex authentication, as
  [the plugin lane](resources/lane-plugin.md) lists them.
- **From codex:** the `claude` CLI, its TTY host and an authenticated
  interactive canary, as [the lifecycle lane](resources/lane-lifecycle.md)
  lists them.

If no qualified native route remains, record the unavailable seat and reduced
assurance; never silently substitute your own vendor or claim duo completion.
A missing CLI alone does not rule out a qualified App route. If a route is present
but unauthenticated (or 401s mid-run), stop and tell the user to run
`codex login` (or log in to `claude`) — **never automate the auth**. One
vendor account per side, the user's own.

## Lanes: load the one in use

Read this core, then only the lane for the host you are on:

- **From Claude Code (Claude host):** [the plugin lane](resources/lane-plugin.md)
  holds the `/codex:*` commands, the bounded role prompt, selector and effort,
  native thread provenance, and the consult verdict line.
- **From codex (Codex host):** [the lifecycle lane](resources/lane-lifecycle.md)
  holds `codeflow delegate init`, exact-byte delivery, turn detection, the
  sibling Stop-hook preflight, pane access, effective autonomy, consult and
  edit sessions, and cleanup. A Claude host does not load it.

## Evidence contract, both lanes

Every delegated exchange meets the one five-obligation evidence contract in
[capability-routing's admissible evidence](../cf-model-orchestrator/resources/routing/evidence.md):
launch, provenance, return, failure and recheck. Each lane file states what
satisfies them on that lane; re-derive the returned work yourself.

## Edit access

Before any write-enabled handoff, read [edit access](resources/edit-access.md):
a delegate edits only inside a worktree on a feature branch, never on the root
checkout or a protected branch, and its commits pass CodeFlow's gates
unchanged.

## agy — retired as a delegate tier (no interactive lane)

`agy` has no verified interactive lane, only headless one-shot CLI use, which
CodeFlow ADR-0023 prohibits, so it is **not** a delegate tier; if the user
names it, say the transport rule rules it out. It stays bound by the
harness-agnostic git-hook plane and CI. For its history and the experimental
guard binding when `agy` is someone's harness, read
[agy notes](resources/agy.md).

## Guardrails

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
