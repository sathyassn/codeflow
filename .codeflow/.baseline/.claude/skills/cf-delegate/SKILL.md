---
name: cf-delegate
description: Delegate to the other vendor's native coding harness under its own subscription auth, with qualified transport, lifecycle evidence and scoped edit access. Use for a specialty pass or genuinely parallel edit handoff. Use cf-consult for a read-only second opinion.
---

# cf-delegate — cross-vendor consult and delegate

Compose native harnesses at the process boundary, each under its own subscription
auth. CodeFlow's gates judge the output, not the author. You own and verify
every returned result. The delegate is a vendor you are **not**: from Claude
Code that is codex; from codex that is claude. Consulting or delegating to
your own vendor is self-review with extra steps; never label it independent.

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
  wraps the codex app-server, so a delegated task gets codex's full MCP
  toolset, a resumable thread, and in-band approvals. When unavailable or
  incompatible, use a qualified official Codex App/interactive CLI route under
  the fallback contract below; a missing plugin is not proof that Codex itself
  is unavailable.
- **codex → claude: the interactive `claude` CLI driven through CodeFlow's
  schema-v2 delegate lifecycle, only** (CodeFlow ADR-0036). A Codex host
  follows its host and canary rules in
  [the lifecycle lane](resources/lane-lifecycle.md).

**Prohibited at all times** — no exceptions, including batch/pipeline stages:
headless task execution in either direction (`codex exec`, `claude -p` /
`--print`), and driving the codex app-server through hand-rolled JSON-RPC.
CodeFlow requires verified native sessions with the task's tools and guards.
Status commands are not work sessions: `codex login status`,
`codex --version`, `codex mcp list`, and the plugin install/setup steps stay
fine.

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

## Dispatch and return

A dispatch names the task id, branch, exact revision, worktree, role and
where to return, and nothing the task record already says. Check the model
line (selector and effort) before every send. Where the Codex seat exposes
a setting that turns off its rate-limit prompt to switch models, keep it
off, so a switch never happens silently mid-task. A return names the model
that produced it.

## Evidence contract, both lanes

Output counts as the other lineage only under
[admissible cross-lineage evidence](../cf-model-orchestrator/resources/routing/evidence.md):
native runtime provenance, a relay is transport and never author. Every
qualified native route (preferred plugin, official client fallback, or
schema-v2 delegate lifecycle) meets one five-obligation evidence contract:

1. **Launch**: verify the delegated task started through a native session
   artifact: a Codex thread forward or a lifecycle ready record reverse;
2. **Provenance**: native runtime provenance only: a native Codex thread ID
   with source-labeled model/effort forward; the lifecycle's
   session/digest/prompt binding reverse;
3. **Return**: verify the returned unit, scoped worktree diff, and cited
   evidence; a relay's idle or completion signal is evidence of neither;
4. **Failure**: a legible bounded failure (stable exit state, durable
   poison, or explicit harness error), never silent substitution or
   completion inferred from silence;
5. **Recheck**: evidence recheckable through the native surface after the
   fact: the resumable Codex thread forward, the durable state records until
   cleanup reverse.

Record model/effort as observed only when the transport exposes actual
values; otherwise label them requested, and never silently upgrade
requested to observed. Grade inferred completion explicitly as inferred.
Each lane file states what satisfies the obligations on that lane;
re-derive the returned work yourself.

## Edit access

Before any write-enabled handoff, read [edit access](resources/edit-access.md):
a delegate edits only inside a worktree on a feature branch, never on the root
checkout or a protected branch, and its commits pass CodeFlow's gates
unchanged.

`agy` is not a delegate tier (headless only, so the transport rule rules it
out); when `agy` is someone's harness, read [agy notes](resources/agy.md) for
the experimental guard binding.

## Guardrails

- **Every delegate prompt narrows authority and data.** Name purpose, permitted
  actions/files/resources/data/processors/destinations/effects and step budget;
  ambiguity blocks—never guess. Send only necessary minimized data to an
  approved processor; route qualification is not data authority. An
  already-authorized scoped handoff needs no new approval; the lead verifies
  effects and claims.
- **Plain briefs.** Write each delegate prompt plainly: simple,
  straightforward and clear, no mannered prose (see
  `.codeflow/rules/writing.md`).
- **Synthesize, never paste.** A finding is input, not conclusion: re-derive and
  cite it, state agreement/disagreement; unverified remains unverifiable.
- **Stay within ToS.** This process-boundary composition is sanctioned (OpenAI
  ships the plugin itself); the single-account, manual-auth, degrade-on-401
  posture is what keeps it there.
