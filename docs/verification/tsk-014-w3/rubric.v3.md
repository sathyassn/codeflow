# Comparison rubric — version 3, written after the board was rendered

Version 3 governs the W3 board. It **does not replace** `rubric.v2.md`, which is
pinned by its own lock and stays byte-identical, exactly as v2 kept v1. Every
gate in v2 still applies unchanged except where this file says otherwise.

**This rubric pre-registers nothing.** It was written after the v2 renders
existed, by an independent Claude design-primary audit of those renders. What
can be said for it is about method, not priority: A7 and A8 were derived from
defects already found in the rendered board and then implemented as machine
checks rather than fitted to a wanted result — both fail the board as it stands.
Every render they judge was produced before them, and none was redrawn to satisfy
either.

## What changed from version 2, and why

| v2 | What the audit found | v3 |
|---|---|---|
| A1–A6 all passed; the board was declared admissible | Ten of the eighty-one renders clip their own content or draw past their own frame, and every gate still passed | **A7** measures frame containment per render and fails on unregistered clipping |
| v2 line 10 states that `tools/verify.mjs` "asserts its modification time precedes every file in `renders/`" | **No such assertion existed.** The only ordering evidence was the author's own account in `rubric.v2.lock.json`, which contradicts the rubric by saying mtime cannot be re-observed later | The assertion is now real in `verify.mjs`, and **§ "What the ordering evidence is worth"** below states plainly what it does and does not prove |
| `registry.json` was described as written before any candidate and never amended except for marker tokens | Two of one candidate's composition declarations were rewritten, in a single edit, after its renders were inspected; and the registry file carries no lock at all | `registry.json` carries an `amendments` record with both prior wordings verbatim, an `amendments.lock.json` pinning them by digest, and an honest `pre_registration_evidence` block; **A8** requires all three |
| A candidate band that cannot survive a context had no way to be recorded as such | Four composition failures are not mechanical defects and must not be silently repaired by whoever is judging the board | `registry.invalidated_bands` records a band as inadmissible **and** names the failure that made it so; A7 requires both halves |

## A7 — frame containment, and what an invalidated band means

*Every render carries what its page contains.*

A still render is this study's entire evidence channel. It has no interaction, so
`overflow-x: auto` is not an affordance — it is a silent crop, and a line that
stops mid-token with nothing saying it did is worse than a line that is missing,
because the reader believes they have it. The same is true of a label drawn past
its own `viewBox`: the frame removes it and nothing reports the removal.

`tools/render.mjs` measures both per render and writes `checks/frame.json`:

- every HTML element whose `scrollWidth` exceeds its `clientWidth` while its
  computed `overflow-x` is `auto`, `scroll` or `hidden`, with how many pixels are
  hidden and how wide the container is;
- every SVG node whose `getBBox()` extends past its own `viewBox`, with the
  overshoot;
- horizontal overflow of the document itself.

*Expected: none of the three, on any render.*

### An invalidated band is two statements, not one

A band entered in `registry.invalidated_bands` records **both**:

1. **`evidence_status` — inadmissible.** No timed or blind observation gate may
   be recorded from this render. It cannot serve in the comparison.
2. **`failure` — what actually failed, and how badly.** Its `kind`
   (`containment`, `legibility`, `composition`), its `severity` (`minor`,
   `material`, `disqualifying`), the axes it fails on, and what fails in plain
   words.

Both are required and A7 enforces both. **Inadmissible does not mean neutral,
and it does not mean unjudged.** Every band on this board is inadmissible
*because* something failed; not one is inadmissible for a procedural reason. A
record that carried only the first statement would convert a demonstrated
failure into a shrug, which is the failure mode this section exists to prevent.

What an entry does **not** claim is a rubric gate result. G1–G8 need an observer
who authored nothing and read no answer, and the audit read the answer key. That
disqualification is real and is not worked around here — but it does not reach
these findings, because a measured clip and two marks in one position are
properties of the render. No observer, blind or otherwise, can make a clipped
line uncut. Where a failure is invisible to the probe and rests on composition
judgement instead, the entry records `machine_visible: false` and A7 requires its
`kind` to be `composition`, so the two kinds of evidence never blur.

A7 measures containment only. Two marks that fully overlap inside their frame are
inside it, and the machine passes them; the audit found exactly that case twice,
and both are recorded as `machine_visible: false` composition failures.

## A8 — the registration is an account, not an assertion

*Expected: `registry.json` carries a `pre_registration_evidence` object stating
what can and cannot be shown from the files, and an `amendments` array in which
every post-registration change to a registered field appears with its field, its
candidate, when it was made, why, and whether the drawing or the declaration
moved first. A registry that claims no amendments while an amendment is known
fails this gate, and rewording the claim does not satisfy it.*

An amendments array can be emptied as easily as a false claim can be written, and
this study produced both. So the amendments an outside seat established from
evidence are pinned in `amendments.lock.json` by digest over their verbatim
`before` and `after` text, and A8 fails if one is dropped, if its field or
candidate changes, if either side is reworded, or if the recorded `after` is no
longer the field's live value. Later seats may add to the array freely; they may
not remove or alter what is locked.

The rule this exists to enforce: **when a drawing diverges from its
registration, the divergence is recorded — the registration is never edited to
match the drawing.** Editing it silently converts a prediction into a
description, and a description constrains nothing.

## What the ordering evidence is worth

`rubric.v2.md` precedes every candidate file and every render in this working
tree, and `verify.mjs` now asserts it. That assertion is worth exactly this
much:

- it is **filesystem mtime**, which does not survive a fresh Git checkout, so a
  later reader cannot re-observe it. The digest lock is what carries forward,
  and it pins the bytes, not the moment;
- it covers `rubric.v2.md` only. **`registry.json` has no lock and its inode was
  created after most candidate pages had been written**, so the filesystem
  carries no evidence that the registry preceded the candidates;
- and for `a-critical-path-and-room` the question is settled the other way:
  the authoring session's transcript shows both of its composition declarations
  being rewritten in one edit after its renders were inspected. For those two
  fields the registry is **known** to follow the drawing rather than precede it.

So the honest standing is: **the rubric's priority is witnessed in this checkout
and pinned by digest; the registry's priority is asserted by its author, cannot
be proved from the files, and is disproved for two fields.** A reader who was not
present should treat A2, A3 and A4 as checks against a declaration whose priority
they are taking on trust, and should read `registry.json`'s `amendments` array
before weighing any of them.

## Everything else

Unchanged from `rubric.v2.md`: the decision rule, A1–A6, G1–G8, the deterministic
integrity checks, and what the rubric refuses to do. A gate not run is recorded
as *not run*. A gate recorded as **void** is not a weak pass. Fact presence
without idea survival is a fail. The rubric creates no house style, bans no
component, and names no preferred visual form.
