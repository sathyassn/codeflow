# Observer state

## Who authored what, and what that disqualifies

**This session authored everything in this directory** except the seed copies it
started from, and `registry.lock.json` records exactly which files those were:
44 files byte-identical to their W4 or W3 counterparts at the moment the rubric
and registration were pinned, and none authored yet.

**An author cannot observe his own candidates.** G1 through G12 in `rubric.v5.md`
require someone who authored nothing here and has read no answer. No such session
has looked at anything in this directory.

**This session has also read the answer.** It wrote `answer-key.md`, read every
shared subject source and every candidate's markup, and derived the key from the
same objects the candidates render. That is the correct thing for an author to do
and it costs the timed gates permanently, for this board, for this session.

### The gates this session is ineligible to run

| Gate | Why |
|---|---|
| G1 governing idea (five seconds) | authored the compositions; knows the intended idea |
| G2 task question (thirty seconds) | wrote the answer key |
| G3 plain-baseline differential | same |
| G4 form match | declared the relationship each artefact claims to encode |
| G5 primary-form inventory | authored every composition on every surface |
| G6 interchangeability | declared the distinctness it would be testing |
| G7 no-box | author |
| G8 idea survival across contexts | authored both the wide and the alternate compositions |
| G9 expressive reach | wrote the `reach` and `cannot_express` fields it would be checking against |
| G10 extension cost | wrote the `extension_path` fields |
| G11 channel legibility in context | chose the channels, and knows what each mark means before looking at it |
| G12 the answer is correct | wrote the key |

All twelve. There is no gate on this board this session may record a result for.

## What the observer's evidence actually was, exactly

This matters more on W5 than it did on W4, because W5 exists to answer findings
this session **could not read**.

The independent answer-blind observer **completed exactly two turns**. Turn 1
inspected the twelve light-wide candidates. Turn 2 inspected the case and portal
light-middle, light-narrow and dark-wide renders; contract light-narrow; contract
1, 2, 4 and 5 dark-wide; and the four available baselines. It did **not** inspect
contract light-middle, contract/3 dark-wide, or any contract dark-middle or
dark-narrow corner — those corners were in the bundle and were not looked at.

A third turn was drafted and armed and **never accepted**. It produced no report,
no ranking, no direction disposition and no selection. There is no observer
ranking of the directions in existence, and nothing on this board cites one.

**This session did not read the observer's own reports.** They live in a
transcript this session's permission settings deny; reads were attempted and
refused. Every finding in `registry.json` is transcribed from the operator's
enumeration in the orchestration prompts — the operator's wording, not the
observer's. Where that enumeration is silent, this board is silent and says so.

The one observer-adjacent fact this session established firsthand is the
anonymised mapping, by SHA-256 of every bundle render against
`../tsk-014-w4/renders/`: 90 of 96 match exactly, and the six that do not are the
p1 and p3 plain baselines W4 never rendered. That method and result are in
`registry.evidence_provenance.anonymised_mapping`.

## A correction that happened before any artefact existed

A first draft of `rubric.v5.md` and `registry.json` claimed three completed
observer turns, cited the drafted third turn as the source of per-direction
dispositions, and recorded fifteen planned repairs as completed
`disposition: repaired` outcomes — including a claim that context coverage had
already been repaired, before a single render existed.

Orchestration caught it and corrected it **before any W5 candidate artefact was
authored**. No drawing, render, check or report ever rested on it. The false
draft is not preserved as evidence; `registry.correction_history` is the
disclosure that replaces it, and gates **A21** and **A22** are the standing form
of the correction — they make the same class of error fail a check rather than
depend on a reader noticing.

## What *has* been established, and by whom

Deterministically, by `tools/render.mjs` and `tools/verify.mjs`, over 90 renders,
twice with identical results:

- **A1** answer-channel separation; **A5** motion absent under both
  reduced-motion settings; **A7** frame containment; **A10** one byte-identical
  shell across all 90 renders; **A12** every drawn state keyed; **A13** no figure
  text below the legibility floor;
- **A15** every drawn state mark at or above the 9px mark floor, on the dimension
  that actually carries the information, measured inside the element that names
  the state rather than around it — 2054 marks across 90 renders, smallest 9.3px;
- **A15, rendered and bound** — for each of the four material distinctions, in
  all six contexts: evidence selected only by an explicit artefact-to-page-and-
  distinction binding; every registered `channel_a`/`channel_b` tied to one named
  measured channel and to an exact per-state token map, with the registry prose
  quoted verbatim and compared character for character; every divergence covered
  by a matching `authorises` entry in the named amendment; **every pair of states
  separated by at least two of the NAMED channels**; and every token identical
  between the light and dark render of the same viewport, which is what proves no
  token carries hue. Recorded per render in `checks/channels.json`, bound in
  `amendments.json` A-10;
- **A16** the complete 15 × 6 width-by-mode matrix, no gap;
- **A17** a plain-chat, plain-Markdown and plain-HTML control for all six
  scorable surfaces;
- **A18** the traceability spine drawn in the contract's declared order, five
  distinct nodes, all seven authority kinds named — the three things W4 got
  wrong;
- **A19** every narrow band inside the declared elongation ceiling, with the
  gate's own weakness recorded;
- **A20** the reused recipe's second binding varying across five instances;
- **A21** intent separated from outcome on all 40 registered dispositions, none
  claiming an outcome this session may not claim;
- **A22** every finding citing a completed turn, the uncompleted turn cited
  nowhere, and the reading boundary declared;
- plus zero axe violations, zero requests off the study's own origin, zero
  console or page errors, zero text overprints, and a task-owned headless
  lifecycle with no headed launch.

**A clean pass says the board is internally consistent, accessible to the
automated checks, reproducible and admissible. It says nothing about whether a
composition communicates.** On the previous board a clean pass shipped a figure
that stated a repository fact incorrectly, which is why A18 exists.

## What the deterministic pass explicitly cannot do

- It cannot judge a paraphrase A1 was not registered to catch.
- It cannot tell whether a governing idea survives a narrow context. It proves
  only that a different composition is declared and rendered there. Whether that
  composition carries the idea is G8.
- It cannot tell whether two marks that are keyed separately look alike at the
  size they render. That is G11, and it is new precisely because A12 and A14 both
  passed a vocabulary the observer misread in every inspected context.
- **Distinctness is not decodability.** A15 now proves that every pair of states
  is encoded differently on at least two non-hue channels. It does not prove that
  a reader can tell an upright rule pattern from a diagonal hatch, or a dash from
  a dot, at the size and in the mode they render. That is exactly G11, and the
  stronger A15 makes it more important rather than less: the board has more
  encoded distinctions to misread than it had before.
- It did not catch its own absence, twice. A15's rendered half was missing
  entirely, and its first replacement then selected evidence with a predicate
  that was always true and proved differences on channels the registry never
  named. Every run reported green through both. Independent review found both;
  no gate on this board did, and neither did this session.
- The collision gate compares text with text. A label printed on a *mark*, and a
  label merely too close to be read as belonging to its own row, are still human.
- **It cannot substitute for the operator.** The direction is operator-owned, and
  agreement between model seats has never been that decision.
