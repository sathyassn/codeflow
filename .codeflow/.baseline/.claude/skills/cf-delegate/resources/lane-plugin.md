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

- **Consult (read-only):** `/codex:review` and `/codex:adversarial-review`:
  the design/diff read and the cross-vendor security red-team.
- **Delegate (write-enabled):** `/codex:rescue`: delegated execution and
  first-round testing; flags: `--background`/`--wait`, `--resume`/`--fresh`,
  `--model`, `--effort`.
- **Multi-round:** `/codex:transfer`: a persistent codex thread for the
  back-and-forth; follow-ups resume it instead of starting fresh.

Collect the peer in-turn through the public foreground or qualified native
fallback; no host `run_in_background` watcher. The Codex-host turn adapter
does not apply here.

Start every delegated plugin prompt with an explicit bounded role, for example
`ROLE: peer. Complete only this bounded assignment. Do not start the top-level
model orchestrator or delegate back to the host lineage (Claude).` Claude
subagents are not Codex; nested duos violate scope.

Read the current Codex primary selector, default effort, and typed internal
routes from
`../../cf-model-orchestrator/resources/current-ensemble.json`, and invoke the
primary directly with that selector and default effort; the receiving primary
alone selects its permitted internal route, so never call a foreign worker
directly. Send the difficulty and triggers with the task. Worker routing,
effort escalation, candidate use, accountability and authored lineage follow
`../../cf-model-orchestrator/resources/capability-routing.md`. With project
model overrides, apply the orchestrator's
[project model overrides](../../cf-model-orchestrator/references/model-overrides.md)
first.

**Output counts as Codex only with a native Codex thread behind it.** Every
plugin exchange must yield the native thread ID, recheckable afterward
through the plugin or the native Codex surface; a generic Claude subagent, an
unverified relay, or any surface that cannot show that thread never counts as
Codex. Record model and effort as *observed* only when the transport exposes
the actual values; otherwise record them as *requested* (a project-level high
default is a fallback, not evidence that the requested turn used it); never
silently upgrade requested to observed. When completion is inferred from
thread state rather than an explicit result, grade it explicitly as inferred
and verify it through the thread before relying on it.

Ask every consult for a closing `VERDICT: approved|changes_requested` line so
the reply is checkable, and branch on it, then re-derive the findings
yourself (see the skill's Guardrails).

## Evidence on this lane

Launch is a created native thread or first output within a bounded window;
silence is not a launch. Provenance and recheck are the native thread ID and
its resumable thread, with model and effort labeled as above. Failure is an
explicit plugin or harness error, then one bounded retry with diagnosis.
