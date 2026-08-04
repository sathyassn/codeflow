# Observer state

## Nothing on this board has been observed

The gates in `rubric.v2.md` sections G1–G8 require someone who authored none of these
candidates and has read no answer. No such session has looked at the W3 renders. **G1–G8
are not run**, and no composition here has been shown to communicate.

What *had* been established was different: that section A of the rubric passed, and that a
comprehension result taken from this board would therefore be capable of meaning something.

**An independent Claude design-primary audit has since narrowed that.** A1–A6 do pass. But
they passed while ten of the eighty-one renders clipped their own content or drew past their
own frame, while the registry's account of its own amendments was false, and while four
further bands lost their governing idea in ways no machine check can see. `rubric.v3.md` now
governs, adding **A7** (frame containment) and **A8** (registration honesty), and both pass
against a board that has not been redrawn.

So the standing is: **the board is admissible for the bands that are not registered
invalidated, and not admissible for the fourteen that are.** Those fourteen are listed in
`registry.invalidated_bands`, and no observation gate may be recorded for any of them.

**Inadmissible is not neutral.** Each of the fourteen carries a second, separate statement:
the failure that made it inadmissible, with its kind, its severity and the axes it fails on.
Six are disqualifying, six material, two minor. None is inadmissible for a procedural
reason. What none of them is, is a recorded G1–G8 result — the audit read the answer key, so
it cannot supply one — but the failures do not depend on a blind observer: a line clipped by
its container is clipped whoever looks, and two marks drawn at the same position stay there.

## Who has looked, and what that disqualifies

- **The author cannot observe.** Every judgement in this directory is the author's own,
  made with the answer key in hand. This session authored all eleven candidates, inspected
  every render at every width and mode, and corrected what it found. Those corrections are
  evidence about the drawings; none of them is a gate.
- **Any session that reads `answer-key.md` is disqualified from G1–G4 for this board**, on
  the same terms as every pass in W2. A reader who knows the answer cannot produce a
  five-second or thirty-second reading. This includes an integrity reviewer who must read
  the key to check that the verifier recomputes the right values — that is the correct
  thing for such a reviewer to do, and it costs them the timed gates.
- **The independent Claude design-primary audit has looked, and read the key.** It ran the
  render, self-test and verify tools; inspected every one of the eighty-one renders — twenty
  individually at full page, six more as native-resolution crops, the remainder across four
  contact sheets — read the registry, the shared sources, the candidate markup and the answer
  key; and recorded what it found in `registry.amendments`, `registry.invalidated_bands` and
  `rubric.v3.md`. That costs it G1–G4 for this board, which is the correct cost for that role.
  It **authored no candidate and redrew nothing**, deliberately: the seat judging a board
  cannot repair the drawings it is judging without becoming their author.
- **The W2 records are not carried forward.** The Codex blind observation and the Claude
  design judgment under `../tsk-014-w2/observations/` are about the W2 board. They remain
  append-only evidence about what that board made readable and which defects it had. They
  are not observations of anything in this directory, and neither is rewritten into a pass.

## What has been run

Deterministic only, by `tools/render.mjs` and `tools/verify.mjs`:

- **A1 answer-channel separation** — the visible text captured off every candidate render
  is checked against that case's registered forbidden phrases and patterns, and against the
  derived-answer payload itself.
- **A2 declared distinctness** — no two siblings share both primary unit and primary axis;
  none shares all three declared fields.
- **A3 the intermediate context** — the tablet width falls strictly between every
  candidate's own two declared breakpoints, and each page really declares a media query at
  both.
- **A4 carrier honesty** — every referenced block type exists in the live document schema,
  every carrier claim is quoted verbatim from the source that imposes it, and every
  non-native verdict states its material loss.
- **A5 motion** — every render probed under both `prefers-reduced-motion` settings for a
  running animation, a declared animation name or a non-zero transition duration.
- **A6 source binding** — every sibling renders from its case's one shared source; every
  quotation verbatim; every commit, diff line, frontmatter value, contract-table cell and
  declared path checked against the repository.
- Plus axe, network and file-containment isolation, console and page errors, render
  coverage, task-owned process lifecycle, and checksums over every authored source and
  every render.

A clean pass says the study is internally consistent, accessible to the automated checks,
reproducible, and admissible. **It says nothing about whether a composition communicates.**

## What the deterministic pass explicitly cannot do

- It cannot judge a paraphrase A1 was not registered to catch. The registered leak tests
  narrow the surface; the distinction between a fact the encoding is built out of and a
  sentence that performs the derivation is still a judgement, and the author made it.
- It cannot tell whether the governing idea survives a narrow context. It can only prove
  that a different composition is declared and rendered there. Whether that composition
  carries the idea is G8, and G8 needs an observer.
- It cannot substitute for the operator. The direction is operator-owned, and agreement
  between model seats has never been that decision.

## What remains, in order

1. **The author, or the operator, disposes of the fourteen failed bands.** Six are
   mechanically fixable — clipped labels, descenders outside a viewBox, two baselines that
   overflow the viewport. Eight are design decisions, not repairs: P1-A and P2-A at mobile,
   and P3-B's quoted source at every width in both modes. Each is either redrawn and
   re-rendered, or withdrawn from the board for that context with the loss stated. The audit
   redrew none of them on purpose; deciding between those two outcomes is not the judging
   seat's call.
2. A fresh session that has authored nothing here and read no answer runs G1–G8 against the
   still-valid PNGs, the questions from `registry.json`, and `rubric.v3.md` — and nothing
   else. It records **nothing** for an invalidated band.
3. Operator selection from the corrected board, including the balance between authored
   composition and any earned reusable primitive.

Independent Claude design judgment (formerly step 2 in this list) has been given, and is
recorded in `registry.amendments`, `registry.invalidated_bands` and `rubric.v3.md`.

Until step 3, `hypotheses.md` stays a hypothesis file and no shared visual primitive is
extracted from this study.
