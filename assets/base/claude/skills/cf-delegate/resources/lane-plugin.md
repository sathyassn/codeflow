# cf-delegate lane: from Claude Code, through the Codex plugin

Load this lane on a Claude host after the common core in `../SKILL.md`. A
Codex host uses [the lifecycle lane](lane-lifecycle.md) instead.

## Preflight

The plugin surface exists (the `/codex:*` commands respond; install once from
a Claude Code session: `/plugin marketplace add openai/codex-plugin-cc` →
`/plugin install codex@openai-codex` → `/reload-plugins` → `/codex:setup`) and
codex is authenticated (`codex login status`: exit 0 + "Logged in using
ChatGPT"; the same signal `codeflow doctor` reports as the `delegates` check).

## Commands, prompt and provenance

The plugin's commands cover both modes:

- **Consult (read-only):** `/codex:review` and `/codex:adversarial-review`:
  the design/diff read and the cross-vendor security red-team.
- **Delegate (write-enabled):** `/codex:rescue`: delegated execution and
  first-round testing; flags: `--background`/`--wait`, `--resume`/`--fresh`,
  `--model`, `--effort`.
- **Multi-round:** `/codex:transfer`: a persistent codex thread for the
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
`../../cf-model-orchestrator/resources/current-ensemble.json`. Invoke the primary
directly with that selector and default effort; the receiving primary alone
selects its permitted internal route. Follow the canonical candidate versus
scoped-qualified and actual-execution contract in
`../../cf-model-orchestrator/resources/capability-routing.md`: a candidate with
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
the actual values; otherwise record them as *requested*; never silently
upgrade requested to observed. When completion is inferred from thread state
rather than an explicit result, grade it explicitly as inferred and verify it
through the thread before relying on it.

Ask every consult for a closing `VERDICT: approved|changes_requested` line so
the reply is checkable, and branch on it, then re-derive the findings
yourself (see the skill's Guardrails).
