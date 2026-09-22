---
id: ADR-0067
title: "Written content policy for new text"
status: proposed
date: 2026-09-22
supersedes: []
superseded_by: []
architecture_impact: none
---

# ADR-0067: Written content policy for new text

## Context

ADR-0032 made editorial quality contextual: no word list, no prose linter,
no punctuation blacklist, and a reference that protects legitimate
punctuation from over-correction. Five behaviours still recur in agent
sessions on this repository (EPC-017): dashes used as drama in replies and
documents, slogans and rhetorical framing where plain prose was wanted,
paragraph walls where a short summary and a table were wanted, record
identifiers or acronyms standing as titles, and pull request summaries that
are bullets only. The contextual rule cannot stop them because it excludes
ordinary chat, protects the em dash by name, and governs titles against
slogans but not against bare identifiers. The project already owns one rule
of the needed shape: no emoji in commit subjects or pull request bodies. It
is closed, mechanical and cheap to check.

## Decision

Project policy for new text is extended in the family of the no-emoji rule.
Three closed lists say what is enforced and by whom. Nothing outside them
changes: word choice, punctuation in general and style stay contextual and on
demand under ADR-0032, which this record narrows and does not supersede.

### Mechanical (commit-msg hook and `codeflow ci`)

The characters U+2013 (en dash) and U+2014 (em dash) are absent from:

- commit subjects and bodies, checked by the commit-msg hook;
- pull request bodies, checked by `codeflow ci`;
- lines a change adds under `docs/`, `project-management/`,
  `.claude/skills/`, `.agents/skills/`, `assets/base/agents/skills/` and
  `assets/base/claude/skills/`, checked by `codeflow ci` over the pull
  request range. The `.codeflow/.baseline/` mirrors carry the same bytes and
  need no rule of their own.

The check judges what a change adds, never the tree. Existing bytes are
grandfathered: a dash on an unchanged line passes, and the tree at the head
this record lands on passes without a sweep. A changed line is new text and
carries the policy, so the tree converges edit by edit. `docs/decisions/` is
never rewritten to comply: accepted records are append-only, and only a new
record or a dated Note is new text there. A failing check names the
sanctioned fix: a comma, colon, semicolon, parentheses, or a full stop and a
new sentence; a hyphen (U+002D) inside a compound word; "to" in a range. The
hyphen, the minus sign in code and a dash inside a quoted command or fixture
are not policy characters, and a fixture that must contain one lives outside
the named trees.

### Editorial smell graded by evaluation

Three smells join `cf-editorial-review/references/editorial-smells.md` and
are graded by the evaluation kit and by review, never by a hook:

- a title, heading, navigation label or pull request title whose whole text
  is a stable identifier matching `TSK|EPC|ADR|CAP|SPC-###` (three or more
  digits; ADR ids carry four) or an unexpanded all-caps acronym. The words
  name the subject; the identifier stays in the body, the link or the
  frontmatter. An identifier followed by words is not the smell;
- a summary that is not two to four sentences of plain prose before any
  bullets or table. Bullets and tables follow the prose only where they carry
  facts better than a sentence, and a summary that is bullets only is a
  defect;
- mannered prose in any operator-facing text, not only substantial
  documents: slogans, contrast turns ("not X but Y"), rhetorical triplets,
  dramatic fragments, stacked hedges, colon reveals, self-narration,
  ceremonial framing and paragraph walls. The smells reference carries a
  plain rewrite of each.

### Policy judged by evaluation only

Operator-facing replies carry the whole policy: no policy characters, short
plain prose with bullets or a table where they carry facts, a figure where a
flow or relationship carries the point, and `cf-present` opened or offered
where one surface carries a substantial comparison or review. No hook can
see a reply. The evaluation kit grades replies with blind cases and a faulty
control, and the lifecycle reference states the reply rule once.

### Non-goals

No word list, style linter, acronym glossary or AI-text detector is added.
Existing documents are not rewritten. The over-correction canary stays:
semicolons, colons, headings, bold phrases, domain terms, lists and
contextual emoji remain legitimate choices, and removing them without a
diagnosed defect is still a defect.

## Consequences

- Two characters become a deterministic gate on commits, pull request bodies
  and added lines; the fix is always local and named by the check.
- The editorial reference no longer protects the em dash where policy
  forbids it, and it names identifier-only titles and paragraph walls as
  smells.
- Replies are covered by policy for the first time. Enforcement there is
  evaluation and review, not tooling, so a session that never runs the kit
  relies on the always-loaded contract sentence.
- Existing dashes stay in the tree until the line that holds them is next
  edited, and the contract's own dashes wait for its next managed edit. A
  reviewer must not read an old dash as a violation.
- Fixtures and tests that need a policy character live outside the named
  trees or enter as unchanged bytes. This is a small tax on whoever tests the
  check.

## Alternatives considered

### Sweep the tree once

Rejected. Accepted decision records are append-only, a sweep would touch
hundreds of lines with no behavioural change, and the check on added lines
converges the tree without it.

### Extend the mechanical check to titles, summaries and replies

Rejected. A hook cannot see a reply, and a title or summary rule needs
judgment: an acronym may be the subject's own name, and a summary may
legitimately be one sentence. Grading by evaluation keeps ADR-0032's
contextual stance for everything a pattern cannot decide.

### A word list or style linter

Rejected by ADR-0032 and still rejected. It would reject legitimate
technical voice and add a dependency for a problem that two characters and
three named smells cover.

## Architecture impact

None.
