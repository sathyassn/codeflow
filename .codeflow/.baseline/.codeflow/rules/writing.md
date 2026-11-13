# Writing

One reference for everything an agent writes: chat replies, status reports,
summaries, documents, records, commit messages and PR bodies. Managed by
`codeflow update`; the project's own voice rules go in the project section of
`AGENTS.md`.

**Write plainly.** Everything you write, replies and status updates
included, is simple, straightforward, clear and unambiguous, with the detail
the reader needs and no more. Avoid mannered prose, writing that performs
for effect: slogans, "not X but Y" turns, rhetorical triplets, dramatic
fragments, stacked hedges, colon reveals, self-narration, ceremonial framing
and walls of text. State the fact directly, and check what a reader could
take it to mean. At the standard and full tiers,
`.agents/skills/cf-editorial-review/references/editorial-smells.md` lists
each pattern under "Mannered prose" with a plain rewrite.

**Prose length.** Default to short prose and bullets. Write long prose only
when the reader asks for it or the artifact is prose by nature, such as an
essay or a narrative guide.

## Meaning the reader takes

A reader takes from a text whatever it can plausibly mean, not only what the
writer meant. The check below is part of thinking, not a pass after
drafting: choose each word, name, figure and image with its readings in
mind, then read the draft again as the reader. It covers everything that
leaves the session: a reply, a document, a record, a commit message, a PR
body, release notes, a figure, an interface string, and code, where it
covers identifiers, flags, messages, log lines, comments, test names and
example data.

What to check:

- **Connotation.** What a word, phrase, name, image, colour, icon or example
  suggests beyond its literal meaning. `force` suggests damage where the
  flag only skips a prompt; a skull icon suggests danger where the state is
  a disabled option.
- **Ambiguity.** More than one reasonable reading. "The review is blocked"
  can mean the reviewer cannot proceed or the merge is held; say which.
- **Unintended readings.** A reading that is offensive, alarming, blaming,
  misleading, overclaiming, unsafe or legally risky: a message that lets a
  reader think data was lost when nothing was deleted, a name that promises
  a safety the code does not give, a term that carries a slur or an
  association the subject lacks, an unsourced or damaging claim about a
  third party's product. A sourced sentence that names another tool is not
  one.
- **Audience and context.** A reader in another culture or language, a
  non-native speaker, a reader who skims, a screen-reader user who hears
  the words and the description only, and a reader who sees this one line
  alone: a log line, a commit subject, a notification, an error.

How to check, once per draft, briefly:

1. Read it as a reader who does not know the intent. What would they
   conclude, and what would they do next? If either differs from what you
   mean, change the text, not the reader.
2. Read it as a hostile or careless reader. What is the worst thing it could
   be taken to mean, and does the text leave that reading open?
3. Go name by name and claim by claim. Does each name, label, number,
   colour and claim have a second meaning? Does the plainer word serve
   better?

Fix what a plausible reading gets wrong. Prefer the plainer word and the
literal statement over a metaphor. State the boundary where silence lets a
reader fear worse: "nothing was deleted", "the check reports and does not
block". Keep a term that is exact even when it is unfamiliar; one term for
one thing (Words below) still holds, and a qualifier resolves a term the
project uses for several things.

In code, a name or a message is read by someone who sees only it. A new
name uses the ordinary word for what the thing does, with the verb that
names the operation: `delete` deletes, `copy` copies, `kill` stops a
process. A name is a defect when a plausible reader takes it to promise
more than the code does (a `verify` that checks nothing) or to soften what
the code does (a `cleanup` that deletes); a name people already depend on
stays, with that meaning stated where it is used. An error or refusal names
the cause and the fix, says what did and did not happen, and never alarms
or blames the reader; a refusal names the rule and the sanctioned path.
Example data is invented and neutral: no real person, company, address,
credential or in-joke. At the standard and full tiers the copy guide
(`.agents/skills/cf-editorial-review/references/copy-guide.md`) "Names and
code strings" has the rules string by string.

In a figure or an image, the drawing asserts only what its facts carry: no
order, cause, certainty, urgency or completeness the sources do not state;
an arrow means travel or dependency, never decoration. Colour and icons
carry the meaning the audience already gives them for that state, and
nothing else: where red marks an error or a stop for that audience, red
marks nothing ordinary, and a warning sign marks a warning, never a tip. No
state is carried by hue alone, so a colour-blind reader and a screen-reader
user, who gets the description, take the same reading. An image, metaphor or example carries no association the subject
lacks; a person, flag, symbol or culture-bound reference appears only when
it is the subject.

In review, a reading that is wrong, unsafe, alarming, offensive or a false
promise is a material finding ("Review verdicts" in
`workflow-discipline.md`), whatever its size; a plainer word with no such
reading is a nit. At the standard and full tiers the editorial smells
reference lists the patterns under "Meaning taken".

## Replies and status

- **Outcomes first, in words.** A status report or summary names each item
  by what it achieved or what is at stake, in plain words a reader with no
  context understands. IDs, file names, numbers and branch names follow as
  references, never as the heading or the lead of a bullet.
- **Titles name the subject in words.** An identifier or bare acronym is
  never the whole title; it goes in the body.
- **Status names live-checked facts.** Each changeable fact a report or
  handoff states (a seat, a check, a PR, a line, a task's status, another
  agent's progress) is checked live for that report, or marked unverified
  with the time of its last check; `workflow-discipline.md` "Check
  changeable state live" is the rule.
- A reply or report opens with the result it serves and where the work
  stands, then what would change that and who resolves it, then what the
  reader must decide or do; steps, gates, counts and tooling come last, and
  only where they explain those. This is an order, not a set of headings. A
  design discussion leads with the result in prose; labels belong only in
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
  that buries the anchor in detail fails, however short it is. A pull
  request body opens with the same kind of summary.
- In a reply to the operator, items the operator must act on go once under
  NEED YOUR ATTENTION, after the opening and before the detail. Each starts
  with what is needed (Decide, Do, Confirm, Clarify or Note) and stands on its
  own with the subject, the options and a recommendation. The items are the
  decisions, actions and confirmations only the operator can give, including
  a hard gate that waits on the operator; other work keeps moving. With
  nothing owed there is no heading, and a manufactured ask is a defect. The
  heading never appears in a pull request body, document, commit message,
  outbound draft or machine payload. A project may rename or drop it in its
  own section of `AGENTS.md`.
- A simple answer stays simple: no figure, no headings, no recap, and a
  one-line answer stays one line.
- Durations for agent-delivered work are agentic estimates with stated bases;
  see "Durations" in `workflow-discipline.md`.
- When a reply names a link (a pull request, a served page, a file), give the
  exact link a tool printed or one you verified. Never guess a URL, port, or
  pull request number; state an unknown link as unknown.

## Figures by surface

When a relationship carries the point, the reply or document carries a
figure. Match the form to the surface, as listed below. The figure families
are the same nine the presentation skills use (flow, structure, layering,
sequence, state, coverage, extent, derivation, graph); the medium changes the
marks, not the choice. Draw a figure only when a relationship carries the
point, then draw the family that relationship names, with one idea, every
mark explained and one caption line. The forms by surface:

- Where the current surface renders one and the answer is short, an inline
  figure is the default, such as an inline HTML figure in a desktop harness.
- A figure or explanation that needs more than an inline reply goes to
  `cf-present` (standard and full tiers) by default; say why you opened it.
  Formatting preference alone never opens one.
- The harness's own page or widget, such as a Claude Artifact page, is the
  exception: use it only when the operator asks for it or the content must
  be shared, since `cf-present` is local and cannot be shared.
- On docs-portal pages, use the portal's figure grammar, a declared figure
  block (standard and full tiers).
- Markdown files, PR bodies and records are ASCII surfaces. Use fenced
  ASCII in other Markdown files (READMEs, docs, records, PR bodies), in
  terminal output and on any other plain-text surface, or when unsure what
  the surface renders.
- Never use Mermaid. A simple answer stays simple and carries no figure.

## Shape the deliverable

**Shape the deliverable.** Layer it concept before detail, each layer
complete at its own altitude; condense by layering, never by cutting key
information. Keep presentation proportionate. Bullets for the enumerable,
short prose for the rest, and a figure whose scope fits the explanation
when structure, state, or a decision is materially clearer drawn. Use the
least complicated form that stays complete, not the physically smallest;
complex subjects may need a larger or layered view, with a caption or legend
when useful. Never add decorative or forced diagrams, headings, tables, or
recaps. Before done, take the audience's seat: structured, logical,
progressive? Sloppy work is a defect, not a style. At the standard and full
tiers, to explain, follow the explanation method
(`.agents/skills/cf-present/resources/explanation-method.md`); to write each
string, follow the copy guide
(`.agents/skills/cf-editorial-review/references/copy-guide.md`).

In prose, verified truth, policy, technical meaning, project voice, and accessibility
outrank decoration or fabricated personality: verified truth and policy outrank documented project voice,
audience, medium, task, and requested tone; preserve technical meaning and
never invent personality, experience, feelings, familiarity, or slang. Web
artifacts stay componentized rather than monolithic. At the standard and
full tiers use `cf-design` for material product, UX, UI, interaction, or
visual direction.

Consequential prose gets an editorial read for meaning and evidence
whatever its home: substantial docs, ADRs, proposals, release notes,
operator communications and user-facing copy, combined with the technical
review where one is due. At the standard and full tiers that read is
`cf-editorial-review`. Ordinary PR bodies, records and short replies do not
need it.

## Written content policy

The written content policy (ADR-0067): avoid em and en dashes in prose; use a
comma, colon, full stop or hyphen instead, and keep a dash only where it is
really needed, such as a quoted title or a numeric range in data. This is a
writing guideline that review and evaluation judge. `git.policy_characters` checks
commit messages, PR bodies and added lines under `docs/`,
`project-management/` and skill trees (warn by default; a project may set
block); old lines are exempt. No emoji or AI attribution in commits and PR
bodies. No hook sees a chat reply, so these rules hold there by discipline;
mannered prose is a defect in a reply as much as in a document.
At the standard and full tiers, `cf-editorial-review` judges substantial
prose and the `cf-evaluate-model` evaluations check replies.

## Copy guide

How to write each kind of string. Every rule here removes words or moves a
fact to its best carrier, and none asks a short answer for a lead, a heading
or a sentence about its own format. Each section ends with one example quoted
verbatim from the CodeFlow source it names. At the standard and full tiers,
`cf-editorial-review` holds the smells for diagnosing a draft.

### Voice

Keep the voice calm. Documentation speaks in the third person; how-to
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
When one word names several things in the project, such as `versioning` for
an API and for a map, qualify it at every use. Write numbers as digits with
their units.

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

> Delivery, in the order work moves:

### Captions

A caption is one sentence that states the takeaway. It does not repeat the
title or explain the legend.

Example, from CodeFlow's `docs/verification/evidence/tsk-006/prototype.html`:

> Only dependency-free nodes share a time window; branch tips follow predecessor landings.

### Summaries

A substantive summary follows the summary rule under Replies and status: it
anchors the reader in a few lines before the list or table it leads into. A
short answer is its own summary and takes no lead.

A pull request Summary is one prose paragraph that anchors the reader,
then the details as a list or table, then at most one closing paragraph;
`codeflow ci` checks that shape (`git.pr_summary`).

Example, from CodeFlow's `docs/decisions/ADR-0067-written-content-policy.md`:

> Project policy for new text is extended in the family of the no-emoji rule. Three closed lists say what is enforced and by whom.

### Bullets and tables

Put one fact in each bullet and keep the bullets parallel in form. Use a table
when three or more items share the same fields, with nouns as its headers.

Example, from CodeFlow's `project-management/specs/SPC-013.md`:

> | Rule | Plane | Default | Adjustable |

### Microcopy

Start a label with a verb and name a state with an adjective or a short noun
phrase. An empty state says what to do next, and an error names the cause and
the fix. Use digits, keep a tooltip to one clause, and use no exclamation
marks.

Example, from CodeFlow's `crates/codeflow-present/web/src/chrome.tsx`:

> Select words, click any part, or drag a box; hold Shift to start a box on words.

### Replies

A simple answer stays simple: no figure, no headings, no recap, and a
one-line answer stays one line. A longer reply leads with the outcome in
words, follows the summary rule, carries a figure when a relationship carries
the point, puts tabular facts in a table, and gives the exact link a tool
printed. Never add a sentence about the reply's own format. The example is
a recorded chat reply's opening line, which leads into its table.

Example, from CodeFlow's `docs/verification/tsk-014-w3/baselines/p3/chat.md`:

> Eight rounds of exact review over the qualification harness produced seven findings:
