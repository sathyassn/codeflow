---
name: cf-delegate
description: Delegate to the other vendor's native coding harness under its own subscription auth, with qualified transport, lifecycle evidence and scoped edit access. Use for a specialty pass or genuinely parallel edit handoff. Use cf-consult for a read-only second opinion.
---

# cf-delegate: cross-vendor consult and delegate

Compose native harnesses at the process boundary, each under its own subscription
auth. CodeFlow's gates judge the output, not the author. You own and verify
every returned result. The delegate is a family you are **not**: from
Claude Code that is Codex or Grok; from Codex, Claude or Grok; from Grok,
Claude or Codex. Consulting or delegating to your own family is self-review
with extra steps; never label it independent.

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

## Transport

Which seat runs the other family, its fallbacks and its launch flags are
[cross-family transport](../cf-model-orchestrator/resources/routing/transport.md),
the one statement of the rule; this skill does not restate it. The seat
always runs interactive, under the task's tools and guards.

**Prohibited at all times**, no exceptions, including batch/pipeline stages:
headless task execution in any direction (`codex exec`, `claude -p` /
`--print`), and driving the codex app-server through hand-rolled JSON-RPC.
Status commands are not work sessions: `codex login status`,
`codex --version`, `codex mcp list`, and the plugin install/setup steps stay
fine.

For an incompatible or unavailable preferred lane, read
[qualified native fallback](resources/native-fallback.md) before choosing
another client. It preserves all five evidence obligations and the effective
safety boundary; it is not permission to route around a security denial.

## Preflight: is the delegate even available

Check the preferred seat, then each fallback in the transport order: the
callee's CLI and an authenticated interactive canary, a reachable Herdr
server (`herdr status server`), and for Codex the app-server daemon
(`codex app-server daemon version`). A missing plugin or a missing
`HERDR_ENV` is not proof that a seat is unavailable.

If no qualified native route remains, record the unavailable seat and reduced
assurance; never silently substitute your own vendor or claim duo completion.
A missing CLI alone does not rule out a qualified App route. If a route is present
but unauthenticated (or 401s mid-run), stop and tell the user to run
`codex login` (or log in to `claude`); **never automate the auth**. One
vendor account per side, the user's own.

## Lanes: load the one in use

Read this core, then only the lane for the seat you are calling:

- **A Codex or Grok seat (any host):** `cf-herdr` creates the tab, launches
  the seat, delivers the prompt and harvests the reply; its review and
  harvest reference states this lane's evidence.
- **A Claude seat, from a Codex or Grok host:** [the lifecycle lane](resources/lane-lifecycle.md)
  holds the seat preflight, the Herdr host, pane access, effective
  autonomy, consult and edit sessions, and cleanup; before launch it sends
  you to the turn adapter for `codeflow delegate init`, the sibling
  Stop-hook preflight, exact-byte delivery and turn detection. A Claude
  host does not load it.
- **Only when the Codex plugin fallback is in use, on a Claude Code host:**
  [the plugin fallback](resources/lane-plugin.md) holds the `/codex:*`
  commands and their native thread provenance.

## Dispatch and return

A dispatch names the task id, branch, exact revision, worktree, role and
where to return, and nothing the task record already says. Check the model
line (selector and effort) before every send. Where the Codex seat exposes
a setting that turns off its rate-limit prompt to switch models, keep it
off, so a switch never happens silently mid-task. A return names the model
that produced it.

## Evidence contract, every lane

Output counts as the other lineage only under
[admissible cross-lineage evidence](../cf-model-orchestrator/resources/routing/evidence.md):
native runtime provenance, a relay is transport and never author. Every
qualified native route (a Codex or Grok seat in Herdr, the schema-v2
delegate lifecycle for a Claude seat, or a recorded fallback) meets one
five-obligation evidence contract:

1. **Launch**: verify the delegated task started through a native session
   artifact: a confirmed started turn in a Codex or Grok seat, or a
   lifecycle ready record for a Claude seat;
2. **Provenance**: native runtime provenance only: a native Codex thread ID
   (or Grok session id) with source-labeled model/effort; the lifecycle's
   session/digest/prompt binding for a Claude seat;
3. **Return**: verify the returned unit, scoped worktree diff, and cited
   evidence; a relay's idle or completion signal is evidence of neither;
4. **Failure**: a legible bounded failure (stable exit state, durable
   poison, or explicit harness error), never silent substitution or
   completion inferred from silence;
5. **Recheck**: evidence recheckable through the native surface after the
   fact: the resumable Codex thread or Grok session, the durable state
   records until cleanup for a Claude seat.

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
  ambiguity blocks; never guess. Send only necessary minimized data to an
  approved processor; route qualification is not data authority. An
  already-authorized scoped handoff needs no new approval; the lead verifies
  effects and claims.
- **Plain briefs.** Write each delegate prompt plainly: simple,
  straightforward and clear, no mannered prose (see
  `.codeflow/rules/writing.md`).
- **Synthesize, never paste.** A finding is input, not conclusion: re-derive and
  cite it, state agreement/disagreement; unverified remains unverifiable.
- **Stay within ToS.** Each seat is the vendor's own interactive client
  under the user's own login (OpenAI ships a plugin that composes the same
  way); the single-account, manual-auth, degrade-on-401 posture is what
  keeps it there.
