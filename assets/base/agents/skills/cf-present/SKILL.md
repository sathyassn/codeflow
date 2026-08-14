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
use `cf-docs-portal` for durable repository documentation. Neither inherits
this utility's themes or components.

## 2. Shape the information before encoding it

Anchor purpose, audience, decision/action, verified evidence, uncertainty, and
the depth the user needs. Apply `cf-editorial-review` to substantial prose.
Keep language plain, direct, calm, and faithful to the session and project
voice; preserve exact identifiers and technical terms. Do not invent
personality, certainty, familiarity, research, or decorative emoji.

### How to think about presentation (mandatory)

The JSON document is **not** the presentation. The open page is. Block order is
attention order; block **type** chooses how the claim is perceived (figure vs
reading band vs evidence rows vs peer columns). The runtime will not invent a
stage board from a wall of prose.

**Before writing blocks or calling `present open`, load in order:**

1. [resources/how-presentation-works.md](resources/how-presentation-works.md)
   — what the human sees, how to choose instruments, worked good/bad contrast
2. [resources/utility-presentation-system.md](resources/utility-presentation-system.md)
   — system craft, Comment lifecycle, fail-closed anti-patterns
3. [references/visual-craft.md](references/visual-craft.md) — short checklist
4. [references/document-authoring.md](references/document-authoring.md) — fields
   **after** the page walk is settled

Work backward: job → 5‑second picture → one primary carrier → support / prove /
ask → mentally walk the page → only then encode JSON.

**Choose blocks by information shape.** A visual must communicate a
relationship, sequence, comparison, state, evidence, scale, or actual
appearance—not decorate surrounding prose. Give the governing relationship
**one primary visual form at rest**; put explanation around that form.
**Restyling the same chat answer is a failed use** of this skill (including
equal-weight text cards and bullet walls with no structural carrier)—stay in
chat or restructure. Phrase retained for contract checks: restyling the same
chat answer is a failed use of this skill.

Start from
[resources/present-document.example.json](resources/present-document.example.json)
as a **shape** (carrier first), not a form to pad. Use
[assets/config.example.toml](assets/config.example.toml) and
[assets/primitive-tokens.example.json](assets/primitive-tokens.example.json)
only during an explicit `cf-customize` opt-in.

`cf-design` settles **product** experience direction for consuming apps. It does
not define utility themes or present chrome.

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
4. **Handoff to the operator:** lead with the owner-private **bootstrap file
   path / openable link** CodeFlow printed, then session ID, revision, and the
   decision sought. Do not send ports and cookie recipes as the primary path.
5. Do not claim the document was seen or approved until feedback or other
   direct evidence proves it.

## 4. Revise through immutable document versions

- Use `codeflow present list` and `codeflow present show <session-id>` to find
  or inspect a session for the current project. Do not reopen a user-ended
  review without a fresh invitation; start a new session when the work has
  materially changed.
- Use `codeflow present update <session-id> <document.json>` for a meaningful
  content revision. Preserve stable block IDs for conceptually unchanged
  blocks so anchored feedback can be explained across revisions.
- Comment is a single mode (`C` only inside the present session window). Notes
  attach to exact selected text, one semantic element, a dragged visual area, a
  whole block, or the whole document. The notes rail is visible **only while
  Comment mode is on**; queued notes still update the count badge when mode is
  off. Pending numbered marks visible while notes are edited. Never guess a
  moved element or visual region across revisions; unchanged coordinate space
  may re-anchor, otherwise retain it visibly as orphaned feedback.
- Use `codeflow present feedback <session-id> [--follow]` to deliver review
  envelopes to the invoking harness through stdout. The path is harness-agnostic
  (Claude Code, Codex, Grok CLI, or any consumer of the CLI). Deduplicate by
  stable `event_id`; delivery is at least once. This proves a complete envelope
  reached the command consumer, not that a later model acted on it. The harness
  must include the envelope in its active turn and only then resolve it after
  action or an explicit decline.
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
- If close reports that the recorded browser leader disappeared before its
  group or tree could be re-qualified, do not use broad name matching or force
  deletion. Inspect or terminate the reported identity with native OS tools,
  then retry `codeflow present close` so CodeFlow can finish confined cleanup.
- Use `codeflow present clear ... --dry-run` before removal. Clear only eligible
  closed state; never delete active, locked, unrelated, or unverified paths.
- Keep raw feedback and exports untracked unless the user explicitly promotes
  them to a named durable path.

## Completion

Return the session ID, revision, purpose, requested decision, durable outcomes
promoted, cleanup state, and anything not verified. Keep the ordinary chat
handoff concise; the interactive document carries the detail.
