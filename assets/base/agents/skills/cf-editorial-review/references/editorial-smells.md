# Contextual editorial smells

Use these as diagnostic prompts, not a checklist or blacklist. A pattern is a
problem only when it weakens this artifact in its actual context. The one
exception is project policy: the characters and shapes ADR-0067 names are
defects in new text wherever they appear.

## Credibility

- Praise or agreement substitutes for analysis, evidence, or a clear decision.
- Certainty exceeds the evidence, or a limitation is buried after the claim.
- Routine work is inflated with grand claims, superlatives, or ceremonial
  framing.
- The text invents personal experience, emotion, intimacy, personality, or
  colloquial language the author or project never established.
- Chatbot openers or closers ("great question", "I hope this helps", "let me
  know if") substitute for the answer.
- Stock puffery ("pivotal", "evolving landscape", "testament to") inflates
  routine work.
- Vague attribution ("experts believe", "industry reports suggest") names no
  source.
- Contrast crutches ("not just X, but Y") add a frame without a fact.

## Meaning and structure

- The reader lacks the subject, stakes, prior decision, or requested action.
- A summary repeats headings instead of giving the outcome.
- A summary is not two to four sentences of plain prose before its bullets or
  table; bullets alone, or a table alone, is not a summary.
- Paragraphs mix decisions, evidence, instructions, and caveats without a
  usable order.
- Compression removes a necessary qualifier; expansion adds no new context.
- Terminology drifts across sections or replaces an exact domain term with a
  smoother but less precise synonym.

## Delivery

- Sentence fragments, excessive parentheticals, or uniform sentence shapes
  make the text harder to follow.
- Lists split a single thought into fragments, or prose hides genuinely
  enumerable material.
- A reply or summary runs as a paragraph wall: one block of many sentences
  where two to four sentences of plain prose and then bullets or a table
  carried the facts.
- Headings, bold text, tables, or callouts compete for attention instead of
  exposing hierarchy.
- Titles, navigation, or action labels hide the actual subject or action behind
  a riddle, slogan, or clever phrase unsupported by the product voice.
- A title, heading, navigation label, or pull request title is a bare stable
  identifier (`TSK-066`, `ADR-0032`, `CAP-003`) or an unexpanded all-caps
  acronym. The words name the subject; the identifier stays in the body, the
  link, or the frontmatter. An identifier followed by words is fine.
- Emoji, jokes, slogans, or rhetorical flourishes conflict with the medium,
  policy, gravity, or documented voice.
- Interface words and visual treatment imply conflicting levels of urgency,
  trust, playfulness, or certainty.

## Mannered prose

Mannered prose performs instead of informing. It is a defect in any
operator-facing text, a chat reply included, not only in substantial
documents. Each pattern below pairs a mannered example with its plain
rewrite; the rewrite keeps the fact and drops the performance.

| Pattern | Mannered | Plain |
|---|---|---|
| Slogan | "Evidence over vibes." | "Every claim cites its evidence." |
| Contrast turn | "This is not a linter, it is a discipline." | "The check reads two characters; review reads meaning." |
| Rhetorical triplet | "Fast, safe, and boring." | "The check runs in under a second and edits nothing." |
| Dramatic fragment | "One rule. No exceptions." | "The rule has no exceptions." |
| Stacked hedges | "It might perhaps be worth possibly considering a cap." | "A 31 KiB cap keeps the file loadable." |
| Colon reveal | "The result: nothing changed." | "Nothing changed." |
| Self-narration | "Let me walk you through what I did next." | "Next, the hook was installed." |
| Ceremonial framing | "It is worth noting that the tests pass." | "The tests pass." |
| Dash as drama | a pause marked with U+2014 before the point | a comma, a colon, or a new sentence |
| Paragraph wall | eight sentences in one block carrying three facts | two sentences, then a three-row table |

## False positives to avoid

Do not rewrite merely because text uses an em dash on a line that predates
ADR-0067, or a semicolon, colon, heading, bold phrase, technical term,
transition, list, or occasional emoji. Preserve such choices when they are
grammatically sound and useful for this audience and medium. Where policy
forbids it (new text in commit messages, pull request bodies, tracked
documents, planning records, the skill trees, and operator-facing replies),
the em or en dash is not a protected choice: replace it on the line being
written and leave unchanged lines alone. Do not replace an established project
voice with generic corporate prose, forced informality, or a model's preferred
cadence.
