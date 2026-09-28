# Writing

One reference for everything an agent writes: chat replies, status reports,
summaries, documents, records, commit messages and PR bodies. Managed by
`codeflow update`; the project's own voice rules go in the project section of
`AGENTS.md`.

## Replies and status

- **Outcomes first, in words.** A status report or summary names each item
  by what it achieved or what is at stake, in plain words a reader with no
  context understands. IDs, file names, numbers and branch names follow as
  references, never as the heading or the lead of a bullet.
- **Titles name the subject in words.** An identifier or bare acronym is
  never the whole title; it goes in the body.
- A reply or report opens with the result it serves and where the work
  stands, then what would change that and who resolves it, then what the
  reader must decide or do; steps, gates, counts and tooling come last, and
  only where they explain those. This is an order, not a set of headings: a
  design discussion leads with the result in prose, labels belong only in
  status, readiness or closeout reports the same reader compares, and labels
  forced onto a short answer are a defect.
- A running report on long work opens with the result the work serves and
  where it stands, then what would change it and who resolves it; progress
  lines follow.
- A summary anchors the reader: what this is, why it matters and where it
  stands, in a few lines. It is judgment, not a sentence count or a list of
  banned items; a key number, file name, data point or caveat belongs there
  when it is part of that context, and detail that does not help the reader
  orient comes after it. The details follow as bullets, one point each, in a
  logical order (problem, change, effect, limits, or the order of the flow);
  a table for tabular data and a fenced block for pasted output. A summary
  that buries the anchor in detail fails, however short it is.
- In a reply to the operator, items the operator must act on go once under
  NEED YOUR ATTENTION, after the opening and before the detail. Each starts
  with what is needed (Decide, Do, Confirm, Clarify or Note) and stands on its
  own with the subject, the options and a recommendation. The items are the
  decisions, actions and confirmations only the operator can give, including
  a hard gate that waits on the operator; other work keeps moving. With
  nothing owed there is no heading, and a manufactured ask is a defect. The
  heading never appears in a pull request body, document, commit message,
  outbound draft or machine payload; a project may rename or drop it in its
  own section of `AGENTS.md`.
- A simple answer stays simple: no figure, no headings, no recap, and a
  one-line answer stays one line.
- Durations for agent-delivered work are agentic estimates with stated bases;
  see "Durations" in `workflow-discipline.md`.
- When a reply names a link (a pull request, a served page, a file), give the
  exact link a tool printed or one you verified. Never guess a URL, port, or
  pull request number; state an unknown link as unknown.

## Figures by surface

When the point is a flow, dependency, structure, state change, or
comparison that is clearer drawn, the reply or document carries a figure.
Match the form to the surface:

- A multi-part explanation, comparison, plan or decision that benefits from
  one coherent surface and anchored feedback goes through `cf-present` where
  the harness can show it (standard and full tiers); say why you opened it.
- Where the harness renders one, use an inline HTML figure.
- Use fenced ASCII only on a terminal or other plain-text surface, or when
  unsure what the surface renders.
- Never use Mermaid.

## Shape the deliverable

**Shape the deliverable.** Layer it concept before detail, each layer
complete at its own altitude; condense by layering, never by cutting key
information. Keep presentation proportionate. Bullets for the enumerable,
prose that earns its place, and a figure whose scope fits the explanation
when structure, state, or a decision is materially clearer drawn. Use the
least complicated form that stays complete, not the physically smallest;
complex subjects may need a larger or layered view, with a caption or legend
when useful. Never add decorative or forced diagrams, headings, tables, or
recaps. Before done, take the audience's seat: structured, logical,
progressive? Sloppy work is a defect, not a style.

In prose, verified truth, policy, technical meaning, project voice, and accessibility
outrank decoration or fabricated personality: verified truth and policy outrank documented project voice,
audience, medium, task, and requested tone; preserve technical meaning and
never invent personality, experience, feelings, familiarity, or slang. At the
standard and full tiers use `cf-design` for material product, UX, UI,
interaction, or visual direction and `cf-editorial-review` for substantial
prose.

## Written content policy

The written content policy (ADR-0067): avoid em and en dashes in prose; use a
comma, colon, full stop or hyphen instead, and keep a dash only where it is
really needed, such as a quoted title or a numeric range in data. This is a
writing guideline that review and evaluation judge. `git.policy_characters` checks
commit messages, PR bodies and added lines under `docs/`,
`project-management/` and skill trees (warn by default; a project may set
block); old lines are exempt. Write plainly: no slogans, no "not X but Y"
turns, no rhetorical triplets or dramatic fragments, no walls of text. No
emoji or AI attribution in commits and PR bodies. No hook sees a chat reply,
so these rules hold there by discipline. At the standard and full tiers,
`cf-editorial-review` judges substantial prose and the `cf-evaluate-model`
evaluations check replies.

## Copy guide

How to write each kind of string. Every rule here removes words or moves a
fact to its best carrier, and none asks a short answer for a lead, a heading
or a sentence about its own format. Each section ends with one example quoted
verbatim from the CodeFlow source it names. At the standard and full tiers,
`cf-editorial-review` holds the smells for diagnosing a draft.

### Voice

Write in a plain, calm voice. Documentation speaks in the third person; how-to
steps and replies address the reader directly or use the imperative.

Example, from CodeFlow's `assets/base/agents/skills/cf-editorial-review/references/editorial-smells.md`:

> Every claim cites its evidence.

### Sentences

Give each sentence one idea, a named subject, the active voice and the present
tense, in about 25 words or fewer. A colon introduces what it announces; a
semicolon joins two closely related clauses.

Example, from CodeFlow's `assets/base/agents/skills/cf-editorial-review/references/editorial-smells.md`:

> The check runs in under a second and edits nothing.

### Words

Write identifiers, commands and paths exactly, in code format. Expand an
acronym once, where it first appears. Use one term for one thing throughout.
Write numbers as digits with their units.

Example, from CodeFlow's `assets/base/rules/workflow-discipline.md`:

> Keep the whole `AGENTS.md` under 32 KiB so Codex reads the project section in full; `codeflow doctor` warns past it.

### Titles and headings

Use a noun phrase in words, in sentence case. A how-to heading names the
outcome the reader reaches. Identifiers go in the body, as the title rule
under Replies and status says.

Example, from CodeFlow's `docs/adoption.md`:

> Install the binary

### Leads

A lead tells the reader what they are looking at before a figure, table or
list. It never repeats the caption.

Example, from CodeFlow's `assets/base/AGENTS.md.tmpl`:

> Each always rule is one line with a pointer to its full text, and the table below names the rule and the reference for the moment you are about to act.

### Captions

A caption is one sentence that states the takeaway. It does not repeat the
title or explain the legend.

Example, from CodeFlow's `docs/verification/evidence/tsk-006/prototype.html`:

> Only dependency-free nodes share a time window; branch tips follow predecessor landings.

### Summaries

A substantive summary follows the summary rule under Replies and status: it
anchors the reader in a few lines before the list or table it leads into. A
short answer is its own summary and takes no lead.

Example, from CodeFlow's `docs/decisions/ADR-0067-written-content-policy.md`:

> Project policy for new text is extended in the family of the no-emoji rule. Three closed lists say what is enforced and by whom.

### Bullets and tables

Put one fact in each bullet and keep the bullets parallel in form. Use a table
when three or more items share the same fields, with nouns as its headers.

Example, from CodeFlow's `project-management/specs/SPC-013.md`:

> | Path | Owner | Later editors, in landing order | Rule |

### Microcopy

Start a label with a verb and name a state with an adjective or a short noun
phrase. An empty state says what to do next, and an error names the cause and
the fix. Use digits, keep a tooltip to one clause, and use no exclamation
marks.

Example, from CodeFlow's `crates/codeflow-present/web/src/chrome.tsx`:

> Select words, click a figure, or drag a box on the stage or empty canvas.

### Replies

A simple answer stays simple: no figure, no headings, no recap, and a
one-line answer stays one line. A longer reply leads with the outcome in
words, follows the summary rule, carries a figure when a relationship carries
the point, puts tabular facts in a table, and gives the exact link a tool
printed. Never add a sentence about the reply's own format. The example is
a recorded chat reply's opening line, which leads into its table.

Example, from CodeFlow's `docs/verification/tsk-014-w3/baselines/p3/chat.md`:

> Eight rounds of exact review over the qualification harness produced seven findings:
