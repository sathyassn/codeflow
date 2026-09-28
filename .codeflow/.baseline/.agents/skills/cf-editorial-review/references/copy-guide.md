# Copy guide for every string

This guide says how to write each kind of string CodeFlow ships or replies
with: a title, a lead, a caption, a legend key, a summary, a bullet, a
label, a reply, a skill sentence, an architecture decision record (ADR) and
a pull request (PR) body. Each section gives its rules and one or more
examples that exist word for word in the named source. The policy behind
the rules is ADR-0067, which names the two policy characters, the three
graded smells and the reply rule. When a draft reads badly and the reason is
unclear, diagnose it with `references/editorial-smells.md` and then apply
the section here that owns the string.

The design system kit states the same rules in six lines, so a product that
reads only the kit gets them; the six lines below are the kit's own
(`resources/design-system/README.md`, "Writing rules"):

- Visuals first: a lead sentence above each figure, the acting sentences below.
- Short plain sentences; bullets or a table where they carry facts better
  than prose.
- No em or en dash; use a comma, colon, full stop or hyphen.
- Titles name the subject in words, never a bare identifier.
- Sentence case, except the uppercase mono kicker.
- No slogans, no "not X but Y" turns, no rhetorical triplets.

Every rule below removes words or moves a fact to the carrier that holds it
best. No rule asks for a lead, a heading or an explanation that a short
answer does not need. A source path that starts with `cf-` names a skill
in the skill trees; any other path is relative to the repository root.

## Voice

Plain and calm. Documentation speaks in the third person about the subject;
how-to steps and replies speak to the reader in the second person.

- State what the subject does or what holds. Do not praise it, sell it or
  apologise for it.
- Use the third person in a guide page, a reference, a caption, a
  description and a record: the subject is the actor.
- Use the second person in a how-to step, a prompt on a review surface and a
  reply: the reader is the actor.
- Keep one register through a document. A page does not switch from "the
  adapter derives" to "you will see".
- Invent no personality, experience, feelings, familiarity or slang. Where a
  consuming project documents its own voice, that voice governs its product
  copy; utility copy stays neutral.

Third person, documentation.

Example (source: `cf-docs-portal/resources/design-system/README.md`):
> Products do not import these files as a library; they carry the values and the behaviour this kit demonstrates.

Second person, a prompt to the reader on the review surface.

Example (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Approve the figure for the guide, or mark what should change.

## Sentences

One idea per sentence, in the active voice and the present tense, with a
named subject, in about 25 words or fewer.

- Put the subject first and let it act: "the hook blocks the push", not
  "the push is blocked".
- Write the present tense for what holds now and the past tense only for
  what happened once, such as a recorded event.
- Split a sentence that carries two ideas. A second idea takes a second
  sentence.
- Use a colon to introduce a list or the explanation of the clause before
  it. Use a semicolon to join two clauses that share one subject or draw one
  contrast. Neither replaces a full stop between two ideas.
- Drop hedges, ceremony and self-narration: "the tests pass", not "it is
  worth noting that the tests pass".
- Use a hyphen inside a compound word and "to" for a range. Never use an em
  or en dash.

A colon that introduces an explanation.

Example (source: `cf-method/references/workflow-lifecycle.md`):
> A simple answer stays simple: no figure, no headings, no recap, and a one-line answer stays one line.

A semicolon that joins two clauses about one subject.

Example (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> Every rule has at least one plane; the secret scan is the one rule every plane enforces.

## Words

Exact names, one term per thing, digits with units.

- Write an identifier, a command, a flag, a path or a value exactly as the
  tool prints it, in a code span. Never paraphrase a command.
- Expand an acronym once, at first use, with the acronym in parentheses;
  use the acronym after that. An acronym the whole audience reads daily
  (CI, URL) needs no expansion.
- Use one term for one thing through a document and its neighbours. Pick
  the term the source uses; do not vary it for style.
- Write numbers as digits, with a space before the unit: "12.5 px", "720
  units", "30 minutes". Spell out a count only inside a summary, where
  digits are kept out.
- Prefer the short common word when it is exact: "use", not "utilise";
  "before", not "prior to".

An acronym expanded once.

Example (source: `docs/decisions/README.md`):
> architecture decision record (ADR)

A command as the tool prints it.

Example (source: `cf-ship/references/pr-evidence.md`):
> codeflow test --mode essential --strict

Digits with units.

Example (source: `cf-docs-portal/resources/utility-presentation-system.md`):
> Text 12.5 px or larger; every inner mark 9 px or larger

## Titles and headings

A noun phrase in words that names the subject; sentence case; identifiers in
the body.

- Name the subject in words. A bare identifier (`TSK-066`, `ADR-0032`) or an
  unexpanded acronym is never the whole title; it goes in the body, the link
  or the frontmatter. An identifier followed by words is fine.
- Name the outcome in a how-to heading: what the reader has when the section
  is done.
- Use sentence case: capitalise the first word and proper nouns only. The
  uppercase mono kicker is the one exception.
- Keep a heading literal and findable. No question, slogan, riddle or
  clever phrase.
- End a heading without punctuation.

A figure title.

Example (source: `cf-docs-portal/resources/figure-grammar-specimens.md`):
> Four enforcement planes along the life of one change

A how-to heading that names the outcome.

Example (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> Start a task in a worktree

A section heading in sentence case.

Example (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> What each plane checks

## Leads

One sentence above a carrier that says what the reader is looking at.

- Write a lead only above a carrier: a figure, a table or a fenced block. A
  plain answer takes no lead.
- Say what the carrier shows and why it is here, in one sentence.
- Never restate the caption, the title or the legend. The lead orients; the
  caption concludes.
- Never describe the carrier's form: not "the figure below shows".

Example (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> Four planes check the same policy at different moments.

Example (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> A blocked change has one route: fix the cause, then continue.

## Captions

One sentence that states the takeaway.

- Write exactly one sentence, ending in a full stop.
- State what the reader takes from the figure or screenshot, as a fact
  about the subject.
- Never repeat the title, the lead or a legend key. A caption that names the
  marks explains the legend; the legend already does that.
- Name a boundary when the figure has one: the plane that is not armed, the
  path not taken.

Example (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> Identity, intent match and currency are checked in that order, and a mismatch stops the task before any edit.

Example (source: `cf-docs-portal/resources/figure-grammar-specimens.md`):
> Both landing paths end at a human merge behind green checks.

## Legend keys and descriptions

A key is a noun phrase that names the state; the description names every
state and every drawn fact.

- Write a key as a noun phrase: the thing the mark stands for, with no
  article and no verb clause about the reader. "Gate that stops travel",
  not "This marks where a gate stops you".
- Give each state one key and each key one state. Two states with one
  meaning are one state.
- Write the description for a listener: every state, every drawn fact and
  the reading order, in plain sentences that name the marks by their key.
- Keep the description free of the caption's judgement; it says what is
  drawn, not what to conclude.

A key.

Example (source: `cf-docs-portal/resources/figure-grammar-specimens.md`):
> Gate that stops travel

A key with its qualifier.

Example (source: `cf-docs-portal/resources/figure-grammar-specimens.md`):
> Plane covers this moment, remote and required

A description sentence that names the marks.

Example (source: `cf-docs-portal/resources/figure-grammar-specimens.md`):
> Repository Markdown sources, a filled source pin, feed the adapter, a transform diamond.

## Summaries

A summary gives context only: what this is and why it exists, in one to
three short sentences, before the bullets or table that carry the details.

- Open a closeout, a pull request body, a report and a reply that leads
  into a list or table with a summary. An ADR context has its own shape,
  in the last section.
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

The page lede: two sentences of context above the Concept figure, with the details in the sections that follow.

Example (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> A change reaches main by one of two paths. A person makes the last step on both.

The pull request rule.

Example (source: `cf-ship/references/pr-evidence.md`):
> Write it as one to three short sentences.

## Bullets and tables

One fact per bullet in parallel form; a table when three or more items
share fields; noun headers.

- Write one fact, decision or step per bullet. A bullet that runs to a
  paragraph is prose in disguise; a paragraph that lists is bullets in
  disguise.
- Keep the bullets parallel: each starts the same way (a noun, a verb or a
  full sentence) and carries the same kind of fact.
- Order bullets by the order of the flow or by importance, most important
  first. Number them only for a sequence.
- Use a table when three or more items share the same fields. Give each
  column a noun header and each cell a fragment or a value, not a sentence.
- Put pasted output in a fenced block, never in a table cell or a bullet.

Two parallel bullets, one fact each.

Example (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> Wrong branch is the failure to prevent.

Example (source: `cf-docs-portal/resources/design-system/portal.reference.html`):
> New work starts off the current tip unless the task pins a base.

The bullet rule for a pull request.

Example (source: `.github/pull_request_template.md`):
> One bullet per logical change, most important first

Noun headers.

Example (source: `cf-docs-portal/resources/design-system/README.md`):
> | File | What it is |

## Microcopy

Verb-first labels, adjective states, empty states that say what to do,
errors with cause and fix, digits, one-clause tooltips, no exclamation
marks.

- Start an action label with the verb and name its object: "Add selected
  text", "Submit review". Sentence case.
- Name a state with an adjective or a participle, not a verb phrase: "Not
  run", "armed", "collapsed".
- Make an empty state say what to do next, in one sentence the reader can
  act on. A bare "nothing here" leaves the reader stranded.
- Give an error its cause and its fix in one or two sentences. Name what
  to change; never blame the reader.
- Write counts as digits: "3 results", "0 notes".
- Keep a tooltip to one clause, with the shortcut in parentheses when there
  is one.
- Use no exclamation mark, joke or mascot voice anywhere in an interface.

Verb-first labels.

Example (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Add selected text

Example (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Pick element

Example (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Select area

Example (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Submit review

An adjective state.

Example (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Not run

An empty state: its heading, then the sentence that says what to do.

Example (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Nothing noted yet

Example (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Select words, click a figure part, or drag a box.

An error that names the fix.

Example (source: `cf-docs-portal/resources/design-system/present.reference.html`):
> Write the required change before you submit.

A no-results message with the next step.

Example (source: `cf-docs-portal/resources/design-system/chrome.js`):
> Try a command name or an ID.

A one-clause tooltip with its shortcut, set by the runtime on the Comment button.

Example (source: `cf-docs-portal/resources/design-system/chrome.js`):
> Comment mode (C)

## Replies

A simple answer stays simple; a longer reply opens with a summary, carries
a figure when a relationship is the point, a table for tabular facts and the
exact link a tool printed.

- In the words of the lifecycle reply rule: "A simple answer stays simple:
  no figure, no headings, no recap, and a one-line answer stays one line."
- For anything longer, follow the summary rule: one to three sentences of
  context, then the details as bullets or a table.
- Carry a figure when the point is a flow, dependency, structure, state
  change or comparison that is clearer drawn. Draw the family the
  relationship names, with a legend and one caption line.
- Match the figure's form to the surface: an inline HTML figure where the
  harness renders one, a `cf-present` page when it needs a full page or
  anchored review, fenced ASCII on a terminal or other plain-text surface.
  Never Mermaid.
- Put tabular facts in a table and pasted output in a fenced block.
- Give the exact link a tool printed or one you verified. Never guess a
  URL, a port or a pull request number; say an unknown link is unknown.
- Never write a sentence about the reply's own format, length or structure.
- Use no em or en dash, and no emoji unless the project's voice documents
  one.

The exception itself.

Example (source: `cf-method/references/workflow-lifecycle.md`):
> A simple answer stays simple: no figure, no headings, no recap, and a one-line answer stays one line.

A plain-text figure's legend line and caption line.

Example (source: `cf-docs-portal/resources/figure-grammar-specimens.md`):
> Legend: = travelled  - - not yet  o agent  <> agent merge  (H) human  | stop

Example (source: `cf-docs-portal/resources/figure-grammar-specimens.md`):
> Caption: Both landing paths end at a human merge behind green checks.

The link rule.

Example (source: `cf-method/references/workflow-lifecycle.md`):
> Never guess a URL, port, or pull request number; state an unknown link as unknown.

## Skill prose

The sentence and word rules in the imperative register; no slogans.

- Write a rule as an instruction to the agent: the verb first, then the
  object, then the condition. "Review the artifact in context", not "the
  artifact should be reviewed".
- Name the actor when it is not the reader: "the builder raises the
  budget", "the operator merges".
- State the rule, then the reason or the boundary in a second sentence when
  one is needed. Do not stack conditions in one sentence.
- Use the exact identifiers, commands and paths the tools use, in code
  spans.
- Write no slogan, motto, contrast turn or rhetorical triplet. A skill
  states what to do; it does not motivate.
- Use no em or en dash; the skill trees are policy surfaces.

Example (source: `cf-editorial-review/SKILL.md`):
> Review the artifact in its real project, audience, medium, and task context.

Example (source: `cf-editorial-review/SKILL.md`):
> Never trade precision for fluency.

## ADR and PR shapes

An ADR states the constraint, the decision as fact and the consequences with
their cost; a PR body opens with a summary and carries its evidence.

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
  matrix, a fenced block for pasted output, a fenced ASCII figure when a
  relationship carries the point.

A context sentence that names the constraint.

Example (source: `docs/decisions/ADR-0058-explicit-portal-runtime-ownership.md`):
> Projects need their edits preserved without an implicit promise that arbitrary forks can continue receiving safe upstream merges.

A decision stated as fact.

Example (source: `docs/decisions/ADR-0063-utility-presentation-doctrine-home.md`):
> The normative doctrine is one shared resource file, byte-identical in `cf-present` and `cf-docs-portal`, with profile-specific rules confined to each skill's `references/visual-craft.md`.

A consequence with its cost.

Example (source: `docs/decisions/ADR-0067-written-content-policy.md`):
> This is a small tax on whoever tests the check.

The PR evidence rule.

Example (source: `cf-ship/references/pr-evidence.md`):
> Tables carry tabular data and fenced blocks carry pasted output.
