# Copy guide for every string

This guide says how to write each kind of string CodeFlow ships or replies
with, one section per string. Each section gives its rules and examples
that exist word for word in a named source.

The design system kit states the same rules in six lines, so a product that
reads only the kit still gets them. The six lines below are the kit's own
(`cf-docs-portal/resources/design-system/README.md`, "Writing rules"):

- Visuals first: a lead sentence above each figure, the acting sentences below.
- Short plain sentences; bullets or a table where they carry facts better
  than prose.
- No em or en dash; use a comma, colon, full stop or hyphen.
- Titles name the subject in words, never a bare identifier.
- Sentence case, except the uppercase mono kicker.
- No slogans, no "not X but Y" turns, no rhetorical triplets.

Every rule below removes words or moves a fact to the carrier that holds it
best. No rule asks for a lead, a heading or an explanation that a short
answer does not need. The policy behind the rules is the architecture
decision record (ADR) on written content, ADR-0067. It names the two policy
characters, the three graded smells and the reply rule. When a draft
reads badly and the reason is unclear, diagnose it with
`references/editorial-smells.md`, then apply the section that owns the
string. A source path that starts with `cf-` names a skill in the skill
trees; any other path names a file in the CodeFlow repository.

## Voice

Every string speaks plainly and calmly. A how-to step, a prompt on a review
surface and a reply address the reader, a skill file instructs the agent,
and every other string is written in the third person.

- State what the subject does or what holds. Do not praise it, sell it or
  apologise for it.
- Use the third person in a guide page, a reference page, a caption, a
  description and a record; a skill file takes the imperative (see Skill
  prose).
- Use the second person in a how-to step, a prompt on a review surface and a
  reply: the reader is the actor.
- Keep one register through a document. A page does not switch from "the
  adapter derives" to "you will see".
- Invent no personality, experience, feelings, familiarity or slang. Where a
  consuming project documents its own voice, that voice governs its product
  copy; utility copy stays neutral.

Example, the third person in documentation (source: `cf-docs-portal/resources/design-system/README.md`):
> Products do not import these files as a library; they carry the values and the behaviour this kit demonstrates.

Example, the second person in a prompt to the reader (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Approve the figure for the guide, or mark what should change.

## Sentences

A sentence carries one idea in the active voice and the present tense, with
a named subject, in about 25 words or fewer.

- Put the subject first and let it act: "the hook blocks the push", not
  "the push is blocked".
- Write the present tense for what holds now and the past tense only for
  what happened once, such as a recorded event.
- Split a sentence that carries two ideas. A second idea takes a second
  sentence.
- Use a colon to introduce a list or the explanation of the clause before
  it. Use a semicolon to join two parallel clauses that belong to one idea.
  Neither replaces a full stop between two ideas.
- Drop hedges, ceremony and self-narration: "the tests pass", not "it is
  worth noting that the tests pass".
- Use a hyphen inside a compound word and "to" for a range. Never use an em
  or en dash.

Example, a colon that introduces an explanation (source: `cf-method/references/workflow-lifecycle.md`):
> A simple answer stays simple: no figure, no headings, no recap, and a one-line answer stays one line.

Example, a semicolon that joins two parallel clauses (source: `cf-docs-portal/resources/explanation-method.md`):
> The medium changes the form of a figure; the family stays the one the relationship names.

## Words

A word is the exact one: the name the tool prints, one term per thing, and
a digit where a number is measured.

- Write an identifier, a command, a flag, a path or a value exactly as the
  tool prints it, in a code span. Never paraphrase a command.
- Expand an acronym once, at first use, with the acronym in parentheses;
  use the acronym after that. An acronym the whole audience reads daily
  (CI, URL) needs no expansion.
- Use one term for one thing through a document and its neighbours. Pick
  the term the source uses; do not vary it for style.
- Write a measurement, a value with a unit and an interface count as
  digits, with a space before the unit: "12.5 px", "720 units", "30
  minutes". A small count in running prose may be a word.
- Prefer the short common word when it is exact: "use", not "utilise";
  "before", not "prior to".

Example, an acronym expanded once (source: `docs/decisions/README.md`):
> architecture decision record (ADR)

Example, a command as the tool prints it (source: `cf-ship/references/pr-evidence.md`):
> codeflow test --mode essential --strict

Example, digits with units (source: `cf-docs-portal/resources/utility-presentation-system.md`):
> Text 12.5 px or larger; every inner mark 9 px or larger

## Titles and headings

A title or heading is a noun phrase in words that names its subject.

- Name the subject in words. A bare identifier (`TSK-066`, `ADR-0032`) or an
  unexpanded acronym is never the whole title; it goes in the body, the link
  or the frontmatter. An identifier followed by words is fine.
- Name the outcome in a how-to heading: what the reader has when the section
  is done.
- Use sentence case: capitalise the first word and proper nouns only. The
  uppercase mono kicker is the one exception.
- Keep a heading literal and findable: no question, slogan, riddle or
  clever phrase. A noun clause that names a thing ("What each plane
  checks") is not a question. A question heading asks the reader something
  and ends in a question mark.
- End a heading without punctuation.

Example, a figure title (source: `cf-docs-portal/resources/figure-grammar-specimens.md`):
> Four enforcement planes along the life of one change

Example, a how-to heading that names the outcome (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> Start a task in a worktree

Example, a section heading in sentence case (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> Commit message limits

## Leads

A lead is the one sentence above a carrier that says what the reader is
looking at.

- Write a lead only above a carrier: a figure, a table or a fenced block. A
  plain answer takes no lead.
- A summary that opens a list or a table is its lead, and it may take the
  one to three sentences a summary allows.
- Say what the carrier shows and why it is here, in one sentence.
- Never restate the caption, the title or the legend.
- Never describe the carrier's form: not "the figure below shows".

Example, a lead above a figure (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> Four planes check the same policy at different moments.

Example, a lead above a figure (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> A blocked change has one route: fix the cause, then continue.

## Captions

A caption is the one sentence under a figure or screenshot that states the
takeaway.

- Write exactly one sentence, ending in a full stop.
- State what the reader takes from the figure or screenshot, as a fact
  about the subject.
- Never repeat the title, the lead or a legend key. A caption that names the
  marks explains the legend; the legend already does that.
- Name a boundary when the figure has one: the plane that is not armed, the
  path not taken.

Example, a caption that names a boundary (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> A red check returns the change to editing; there is no route from red to merged.

Example, a caption that states the takeaway (source: `cf-docs-portal/resources/figure-grammar-specimens.md`):
> Both landing paths end at a human merge behind green checks.

## Legend keys and descriptions

A legend key names the state its mark stands for, and the description names
every state and every drawn fact.

- Write a key as a noun phrase: the thing the mark stands for, with no
  article, no main verb, and no instruction to the reader. "Gate
  that stops travel", not "This marks where a gate stops you" or "CI runs
  this step".
- Give each state one key and each key one state. Two states with one
  meaning are one state.
- Write the description for a listener: every state, every drawn fact and
  the reading order, in plain sentences that name the marks by their key.
- Keep the description free of the caption's judgement; it says what is
  drawn, not what to conclude.

Example, a key (source: `cf-docs-portal/resources/figure-grammar-specimens.md`):
> Gate that stops travel

Example, a key with its qualifier (source: `cf-docs-portal/resources/figure-grammar-specimens.md`):
> Managed copy, never edited

Example, a description sentence that names the marks (source: `cf-docs-portal/resources/figure-grammar-specimens.md`):
> Repository Markdown sources, a filled source pin, feed the adapter, a transform diamond.

## Summaries

A summary gives context only: what this is and why it exists, in one to
three short sentences, before the bullets or table that carry the details.

- Open these with a summary: a closeout, a pull request body, a report, and
  a reply that leads into a list or table. An ADR context has its own
  shape, in the last section.
- Write one to three short sentences a reader with no context understands.
- Keep every detail out of it: no mechanism, file name, identifier, number,
  rule list or caveat. A count may be spelled out in words.
- Put the details after it as bullets, one point each, in a logical order;
  a table for tabular data; a fenced block for pasted output.
- Judge a summary by what it carries. A short summary that already holds
  the details fails, and bullets or a table alone where a summary should
  open fails.
- A short answer is its own summary and takes no lead. In the words of the
  lifecycle reply rule: "A simple answer stays simple: no figure, no
  headings, no recap, and a one-line answer stays one line."

Example, two sentences of context before a table (source: `cf-docs-portal/resources/explanation-method.md`):
> Work through five stages in order. Each stage answers one question and produces one output.

Example, the rule as the pull request evidence reference states it (source: `cf-ship/references/pr-evidence.md`):
> Write it as one to three short sentences.

## Bullets and tables

A bullet carries one fact, and a table carries items that share fields.

- Write one fact, decision or step per bullet. Split a bullet that runs to
  a paragraph, and turn a paragraph that lists into bullets.
- Keep the bullets parallel: each starts the same way (a noun, a verb or a
  full sentence) and carries the same kind of fact.
- Order bullets by the order of the flow or by importance, most important
  first. Number them only for a sequence.
- Use a table when three or more items share the same fields. Give each
  column a noun header and each cell a fragment or a value, not a sentence.
- Put pasted output in a fenced block, never in a table cell or a bullet.

Example, a bullet with one fact (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> Wrong branch is the failure to prevent.

Example, its parallel neighbour (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> New work starts off the current tip unless the task pins a base.

Example, the bullet rule for a pull request (source: `.github/pull_request_template.md`):
> One bullet per logical change, most important first

Example, noun headers (source: `cf-docs-portal/resources/design-system/README.md`):
> | Check | How |

## Microcopy

Microcopy is the text on a control, a state, an empty state, an error or a
tooltip.

- Start an action label with the verb and name its object: "Add selected
  text", "Submit review". Sentence case.
- Name a state with an adjective or a participle, not a verb phrase: "Not
  run", "armed", "collapsed".
- Make an empty state say what to do next, in one sentence the reader can
  act on.
- Give an error its cause and its fix in one or two sentences. Name what
  to change; never blame the reader.
- Write counts as digits: "3 results", "0 notes".
- Keep a tooltip to one clause, with the shortcut in parentheses when there
  is one.
- Use no exclamation mark, joke or mascot voice anywhere in an interface.

Example, a verb-first label (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Add selected text

Example, a verb-first label (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Pick element

Example, a verb-first label (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Select area

Example, a verb-first label (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Submit review

Example, an adjective state (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Not run

Example, an empty state's heading (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Nothing noted yet

Example, the empty state's sentence that says what to do (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Select words, click a figure part, or drag a box.

Example, an error that names the fix (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Write the required change before you submit.

Example, a no-results message with the next step (source: `cf-docs-portal/resources/design-system/chrome.js`):
> Try a command name or an ID.

Example, a one-clause tooltip with its shortcut (source: `cf-docs-portal/resources/design-system/chrome.js`):
> Comment mode (C)

## Replies

A reply is sized to its question. The rules below say when it grows a
summary, a figure or a table.

- Keep a simple answer simple. In the words of the lifecycle reply rule:
  "A simple answer stays simple: no figure, no headings, no recap, and a
  one-line answer stays one line."
- For anything longer, follow the summary rule: one to three short
  sentences of context, then the details as bullets or a table.
- Carry a figure when a relationship carries the point, and draw the family
  that relationship names, with a legend and one caption line.
- Match the figure's form to the surface. Use an inline HTML figure where
  the harness renders one, a `cf-present` page when it needs a full page or
  anchored review, and fenced ASCII on a terminal or other plain-text
  surface, or when unsure what the surface renders. Never use Mermaid.
- Put tabular facts in a table and pasted output in a fenced block.
- Give the exact link a tool printed or one you verified. Never guess a
  URL, a port or a pull request number; say an unknown link is unknown.
- Never write a sentence about the reply's own format, length or structure.
- Use no em or en dash, and no emoji unless the project's voice documents
  one.

Example, the exception itself (source: `cf-method/references/workflow-lifecycle.md`):
> A simple answer stays simple: no figure, no headings, no recap, and a one-line answer stays one line.

Example, a plain-text figure's legend line (source: `cf-docs-portal/resources/figure-grammar-specimens.md`):
> Legend: = travelled  - - not yet  o agent  <> agent merge  (H) human  | stop

Example, a plain-text figure's caption line (source: `cf-docs-portal/resources/figure-grammar-specimens.md`):
> Caption: Both landing paths end at a human merge behind green checks.

Example, the link rule (source: `cf-method/references/workflow-lifecycle.md`):
> Never guess a URL, port, or pull request number; state an unknown link as unknown.

## Skill prose

Skill prose follows the sentence and word rules in the imperative register.

- Write a rule as an instruction to the agent: the verb first, then the
  object, then the condition. "Review the artifact in context", not "the
  artifact should be reviewed".
- Name the actor when it is not the reader: "the builder raises the
  budget", "the operator merges".
- State the rule, then the reason or the boundary in a second sentence when
  one is needed. Do not stack conditions in one sentence.
- Use the exact identifiers, commands and paths the tools use, in code
  spans.
- Write no slogan, motto, contrast turn or rhetorical triplet.
- Use no em or en dash. The skill trees are policy surfaces.

Example, a rule as an instruction (source: `cf-editorial-review/SKILL.md`):
> Review the artifact in its real project, audience, medium, and task context.

Example, a boundary in one sentence (source: `cf-editorial-review/SKILL.md`):
> Never trade precision for fluency.

## ADR and PR shapes

An ADR and a pull request (PR) body each have a fixed shape, given below
section by section.

- ADR context: two to five sentences naming the constraint that forced the
  decision, not the history that led to it.
- ADR decision: one paragraph stated as fact, in the present tense, with
  no hedge and no alternatives inside it. The alternatives take their own
  section.
- ADR consequences: what gets easier, what gets harder and what is ruled
  out, with the cost named plainly.
- PR summary: one to three short sentences of context, then the changes as
  bullets, one per logical change, most important first.
- PR testing: the tested revision and command, the gate's summary lines in a
  fenced block, the measured coverage with its scope, and what was not
  tested, named.
- PR tables and blocks: a table for tabular data such as coverage or a test
  matrix, and a fenced block for pasted output. A fenced ASCII figure
  carries a relationship when one is the point.

Example, a context sentence that names the constraint (source: `docs/decisions/ADR-0058-explicit-portal-runtime-ownership.md`):
> Projects need their edits preserved without an implicit promise that arbitrary forks can continue receiving safe upstream merges.

Example, a decision stated as fact (source: `docs/decisions/ADR-0064-portal-as-a-guide-to-the-project-as-it-stands.md`):
> The portal explains the project as it stands: what it is, what it does, how to adopt it, how it is built and how it is operated.

Example, a consequence with its cost (source: `docs/decisions/ADR-0067-written-content-policy.md`):
> This is a small tax on whoever tests the check.

Example, the PR evidence rule (source: `cf-ship/references/pr-evidence.md`):
> Tables carry tabular data and fenced blocks carry pasted output.
