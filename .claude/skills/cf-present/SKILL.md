---
name: cf-present
description: Create, open, revise, and close a structured local CodeFlow review document when a complex explanation, comparison, plan, decision, evidence set, diff, or visual review would materially benefit from one coherent interactive surface and anchored user feedback. Also use when the user asks for a presentation or review surface. Keep short or linearly explained answers in the native conversation; do not use this skill to build product UI, a durable documentation portal, or arbitrary one-off HTML.
---

# cf-present: interactive review documents

Turn substantial session content into one inspectable, feedback-aware document
from CodeFlow's declarative blocks and bounded local runtime; never a new
page application per response.

Each invocation **reuses the utility design system**. Author **this session's**
subject into catalog blocks; the runtime owns chrome, themes, and Comment. Do
not rebuild Comment UI or invent a second visual language.

A supporting flow inside `cf-model-orchestrator` for non-trivial repository
work: it changes how a result is reviewed, never the accepted plan, model
seats, producer/reviewer duties, or evidence requirements. Invoke it at the
material task checkpoint where an interactive surface helps; batch or
integration-branch work never defers required cross-lineage review to the
end.

## 1. Decide whether the surface earns its cost

Use `cf-present` when at least one is true:

- relationships, state, sequence, alternatives, or evidence are materially
  easier to inspect together than in chat;
- the user needs anchored comments or an explicit review verdict;
- a substantial implementation/design review needs code, diffs, diagrams,
  status, and decisions in one revisioned surface;
- the user asks for the interactive presentation explicitly.

Stay in chat for a short answer, a small list, one simple figure, or a status
update that needs no interaction. Do not turn formatting preference into a
runtime session. Use `cf-design` for a consuming product's design direction
and `cf-docs-portal` for durable repository documentation; product UI never
inherits this utility's themes or components; portal shares craft, not present
chrome or Comment.

## 2. Shape the information before encoding it

Anchor purpose, audience, decision/action, verified evidence, uncertainty, and
needed depth. Apply `cf-editorial-review` to substantial prose and write copy
by `cf-editorial-review/references/copy-guide.md`; invent no personality,
certainty, familiarity, research, or decorative emoji.

### How to think (mandatory)

Block order is attention order; block **type** sets how a claim is
perceived.
**Before writing blocks or calling `present open`, load in order:**

1. [explanation-method](resources/explanation-method.md): reader,
   altitude, carrier, draft, check
2. [how-presentation-works](resources/how-presentation-works.md): what
   the human sees
3. [utility-presentation-system](resources/utility-presentation-system.md):
   craft, Comment lifecycle, anti-patterns
4. [figure-grammar](resources/figure-grammar.md): families, rules
5. [design-system kit](resources/design-system/README.md)
6. [visual-craft](references/visual-craft.md): checklist
7. [document-authoring](references/document-authoring.md): fields last

**Choose blocks by information shape.** A visual must communicate one
relationship in one grammar family under its twelve rules, never decorate
prose. Give the governing relationship **one primary visual form at rest**,
short plain prose around it; restyling the same chat answer is a failed use
of this skill (text cards, bullet walls): stay in chat or restructure.

Start from
[present-document.example.json](resources/present-document.example.json)
as a **shape** (carrier first), not a form to pad.

## 3. Validate and open

1. Write one UTF-8 JSON document in a task-owned temporary or explicitly
   requested durable path, not staged by default; keep credentials, private
   history, external URLs, and filesystem paths out of it.
2. Validate against `.codeflow/schemas/present/document-v1.schema.json`, then
   let `codeflow present open <document.json>` perform authoritative semantic
   and byte-bound validation (schema alone cannot enforce unique IDs, byte
   limits, or cross-field invariants).
3. Use the isolated browser/profile opened by CodeFlow. `--no-launch` is an
   explicit recovery or automation path, not permission to attach to the
   operator's browser or active view.
4. **Handoff:** lead with the owner-private **bootstrap file path / openable
   link** CodeFlow printed, then session ID, revision, and the decision
   sought, never ports and cookie recipes as the primary path.
5. Do not claim the document was seen or approved until feedback or other
   direct evidence proves it.

## 4. Revise through immutable document versions

- `codeflow present list` / `codeflow present show <session-id>` find and
  inspect this project's sessions. Do not reopen a user-ended review
  without a fresh invitation; start a new session when the work changed
  materially.
- Use `codeflow present update <session-id> <document.json>` for a meaningful
  content revision. Preserve stable block IDs for conceptually unchanged
  blocks so anchored feedback can be explained across revisions.
- Comment is a single mode (`C` only inside the present session window). Notes
  attach to exact selected text, one semantic element, a dragged visual area, a
  whole block, or the whole document. The notes rail shows **only while
  Comment mode is on**; queued notes keep the count badge; pending
  numbered marks visible while notes are edited. Never guess a moved element or
  region across revisions; unchanged coordinate space may re-anchor, else
  retain it visibly as orphaned feedback.
- Deliver review envelopes with `codeflow present feedback <session-id>
  [--follow]`: stdout, harness-agnostic. Deduplicate by stable `event_id`;
  delivery is at least once and proves a complete envelope reached the
  command consumer, not that a later model acted on it. The harness includes the
  envelope in its active turn before resolving.
- After action or an intentional decline, use
  `codeflow present resolve <session-id> <event-id> --event-version <n>
  --status addressed|dismissed` with the current version from the review
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
  `codeflow present export <session-id> --out <path>`; choose the utility
  theme and light/dark/system mode, never as product design authority.
- Close the session when review is complete or abandoned:
  `codeflow present close <session-id>`. Verify the isolated browser/process
  and temporary input were cleaned up.
- If close reports the recorded browser leader disappeared before its group or
  tree re-qualified, do not use broad name matching or force deletion: inspect
  or terminate the reported identity with native OS tools, then retry
  `codeflow present close` for confined cleanup.
- Run `codeflow present clear ... --dry-run` before removal; clear only
  eligible closed state, never active, locked, unrelated, or unverified paths.
- Keep raw feedback and exports untracked unless the user explicitly promotes
  them to a named durable path.

## Completion

Return the session ID, revision, purpose, requested decision, durable outcomes
promoted, cleanup state, and anything not verified. Keep the chat handoff
concise.
