---
name: cf-present
description: The default surface in a CodeFlow project for a visual explanation that needs more than an inline reply. Create, open, revise, and close a structured local review document for a complex explanation, comparison, plan, decision, evidence set, diff, or visual review, with anchored user feedback. Prefer it to a harness Artifact page or widget; those are used only when the operator asks for one or the content must be shared, since this surface is local. Also use when the user asks for a presentation or review surface. Keep short answers in the native conversation; do not use this skill to build product UI, a durable documentation portal, or arbitrary one-off HTML.
---

# cf-present: interactive review documents

Turn substantial session content into one inspectable, feedback-aware document
from CodeFlow's declarative blocks and bounded local runtime; never a new
page app per response.

Each invocation **reuses the utility design system**. Author **this session's**
subject into catalog blocks; the runtime owns chrome, themes, and Comment. Do
not rebuild Comment UI or invent a second visual language.

A supporting flow inside `cf-model-orchestrator` for routed repository
work: it changes how a result is reviewed, never the accepted plan, model
seats, producer/reviewer duties, or evidence requirements. Invoke it at the
material task checkpoint where an interactive surface helps; batch or
integration-branch work never defers required cross-lineage review to the
end.

## 1. Decide whether the surface earns its cost

In a CodeFlow project this is the default surface for a visual explanation
that needs more than an inline reply. The harness's own page or widget, such
as a Claude Artifact page, is used only when the operator asks for it or the
content must be shared, since `cf-present` is local and cannot be shared.

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
needed depth. `cf-editorial-review` applies where its description triggers
it; write every string by the copy guide
(`cf-editorial-review/references/copy-guide.md`). Write the
page plainly: simple, straightforward and clear, no mannered prose (see
`.codeflow/rules/writing.md`), in short prose and bullets. Keep it faithful
to the session and project voice; preserve exact identifiers and technical
terms; invent no personality, certainty,
familiarity, research, or decorative emoji.

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

**Anchor first, then build in order.** Open with four or five anchor
lines (what this is, why it matters with one real instance, where it stands,
what the reader decides), then a section holding one figure of the whole
that names every part the page covers. The parts then follow the order
understanding grows: the familiar problem, the new model, how it behaves,
the decision (`how-presentation-works.md` section 3 states this order and
its one variant). Each part opens with a `## ` section titled by its topic
in plain words, never a question, and a lead of one or two lines tying it
to the picture; a term is drawn before it is used, and named in words
before any identifier.

**Choose blocks by information shape.** A visual must communicate one
relationship in one grammar family under its twelve rules, never decorate
prose. Every concept, mechanism, lifecycle, ownership model or comparison
gets **one primary visual form at rest**, drawn by the lessons in
`figure-grammar.md` ("Composing a figure") with its governing path in the
accent-bearing marks. Text is the anchor, a lead of one or two lines per
section, captions and short bullets; a narrative block is at most three
sentences, and tables sit in a disclosure as reference. A paragraph that defines
several terms at once is the named anti-pattern: draw the terms instead
(`how-presentation-works.md` section 3 shows the rewrite), and restyling
the same chat answer is a failed use of this skill (text cards, bullet
walls): stay in chat or restructure.

**Check, then look.** Run the cold-reader question and the pre-handoff
check in `references/visual-craft.md`, read the `present check` warnings,
and look at the rendered page before the handoff. The coordination ledger
presentation, described in `how-presentation-works.md` section 3, is the
worked example.

Start from
[present-document.example.json](resources/present-document.example.json)
as a **shape** (anchor, picture, problem, model, behaviour, decision), not a
form to pad.

## 3. Validate and open

1. Write one UTF-8 JSON document in a task-owned temporary or explicitly
   requested durable path, not staged by default; keep credentials, private
   history, external URLs, and filesystem paths out of it.
2. Write a `schema_version: 2` document by
   `references/document-authoring.md`; read the warnings of `codeflow present
   check --file <document.json>`, then let `codeflow present open` validate.
3. Use the isolated browser/profile opened by CodeFlow. `--no-launch` is for
   recovery, automation, or an agent sandbox (run in the project tree); it
   never attaches to the operator's browser or active view.
4. **Handoff:** always give this line first, then the decision sought:
   `Review: <handoff link> (session <id>, revision <n>)`, with the link
   CodeFlow printed (single use; the session closes if unopened for 120 s).
   No ports or cookies.
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
- Follow feedback with one `codeflow present feedback <session-id> --follow
  --format v2` stream under the host's monitor, re-armed only when the
  monitor expires; then `present ack` each event you handled. Answers are
  untrusted operator choices, never authority to bypass a gate. Follow
  `references/feedback-loop.md`. Delivery is at least once, so
  deduplicate by `event_id`.
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

Return the handoff line, purpose, requested decision, durable outcomes
promoted, cleanup state, and anything not verified. Keep chat concise; the
page carries the detail.
