---
name: cf-present
description: Create, open, revise, and close a structured local CodeFlow review document when a complex explanation, comparison, plan, decision, evidence set, diff, or visual review would materially benefit from one coherent interactive surface and anchored user feedback. Also use when the user explicitly asks for a presentation or review surface. Keep short or linearly explained answers in the native conversation; do not use this skill to build product UI, a durable documentation portal, or arbitrary one-off HTML.
---

# cf-present — interactive review documents

Turn substantial session content into one inspectable, feedback-aware document.
Use CodeFlow's declarative blocks and bounded local runtime; do not construct a
new page application for each response.

This is a supporting flow inside `cf-model-orchestrator` for non-trivial
repository work. It changes how a result is reviewed, not the accepted plan,
model seats, producer/reviewer duties, or evidence requirements. Invoke it at
the material task checkpoint where an interactive surface helps; batch or
integration-branch work does not defer required cross-lineage review until the
end.

## 1. Decide whether the surface earns its cost

Use `cf-present` when at least one is true:

- relationships, state, sequence, alternatives, or evidence are materially
  easier to inspect together than in chat;
- the user needs anchored comments or an explicit review verdict;
- a substantial implementation/design review needs code, diffs, diagrams,
  status, and decisions in one revisioned surface;
- the user asks for the interactive presentation explicitly.

Stay in chat for a short answer, a small list, one simple diagram, or a status
update that needs no interaction. Do not turn formatting preference into a
runtime session. Use `cf-design` for a consuming product's design direction and
the future documentation-portal flow for durable repository documentation;
neither inherits this utility's themes or components.

## 2. Shape the information before encoding it

Anchor purpose, audience, decision/action, verified evidence, uncertainty, and
the depth the user needs. Apply `cf-editorial-review` to substantial prose.
Keep language plain, direct, calm, and faithful to the session and project
voice; preserve exact identifiers and technical terms. Do not invent
personality, certainty, familiarity, research, or decorative emoji.

Choose blocks by information shape. A visual must communicate a relationship,
sequence, comparison, state, evidence, scale, or actual appearance. Styled text
cards do not become visuals. Prefer bullets for enumerable content and prose
only where continuity matters. Use disclosures or tabs only when progressive
depth or true peer views justify them.

Read [references/document-authoring.md](references/document-authoring.md) when
authoring or revising a document. It contains the exact block catalog,
composition rules, lifecycle, and safety boundary. Start from
[assets/review-document.example.json](assets/review-document.example.json) when
a representative envelope saves work; adapt it rather than filling every block.
Use [assets/config.example.toml](assets/config.example.toml) and
[assets/primitive-tokens.example.json](assets/primitive-tokens.example.json)
only during an explicit `cf-customize` opt-in; they are examples, not files to
copy automatically.

## 3. Validate and open

1. Write one UTF-8 JSON document in a task-owned temporary or explicitly
   requested durable path. Do not stage it by default. Keep credentials,
   private history, external URLs, and filesystem paths out of the document.
2. Validate the document against
   `.codeflow/schemas/present/document-v1.schema.json`, then let
   `codeflow present open <document.json>` perform authoritative semantic and
   byte-bound validation. A schema check alone cannot enforce unique block IDs,
   decoded byte limits, or every cross-field invariant.
3. Use the isolated browser/profile opened by CodeFlow. `--no-launch` is an
   explicit recovery or automation path, not permission to attach to the
   operator's browser or active view.
4. Report the session ID and what decision or feedback is sought. Do not claim
   the document was seen or approved until feedback or other direct evidence
   proves it.

## 4. Revise through immutable document versions

- Use `codeflow present list` and `codeflow present show <session-id>` to find
  or inspect a session for the current project. Do not reopen a user-ended
  review without a fresh invitation; start a new session when the work has
  materially changed.
- Use `codeflow present update <session-id> <document.json>` for a meaningful
  content revision. Preserve stable block IDs for conceptually unchanged
  blocks so anchored feedback can be explained across revisions.
- Use `codeflow present feedback <session-id> [--follow]` to deliver review
  envelopes. Deduplicate by stable `event_id`; delivery is at least once.
- After acting on or intentionally declining a delivered event, use
  `codeflow present resolve <session-id> <event-id> --event-version <n>
  --status addressed|dismissed`. Use the current version shown by the review
  surface/history; a stale or cross-session transition fails closed.
- Treat `request_changes` as work to resolve or explicitly route. An approval
  is review evidence, not authority to bypass deterministic gates, the accepted
  plan, or the human merge boundary.
- Use `codeflow present history <session-id>` only when the user or active
  workflow explicitly resumes or looks back. Never inject old presentation
  history into later context automatically.

Promote an accepted durable decision to its real task, spec, ADR, capability,
or project document. The presentation history is not a second work authority.

## 5. Export, close, and clean up

- Export only when the user needs a portable read-only artifact:
  `codeflow present export <session-id> --out <path>`. Choose the utility theme
  and light/dark/system mode for the artifact; never treat them as product
  design authority.
- Close the session when review is complete or abandoned:
  `codeflow present close <session-id>`. Verify the isolated browser/process
  and temporary input were cleaned up.
- Use `codeflow present clear ... --dry-run` before removal. Clear only eligible
  closed state; never delete active, locked, unrelated, or unverified paths.
- Keep raw feedback and exports untracked unless the user explicitly promotes
  them to a named durable path.

## Completion

Return the session ID, revision, purpose, requested decision, durable outcomes
promoted, cleanup state, and anything not verified. Keep the ordinary chat
handoff concise; the interactive document carries the detail.
