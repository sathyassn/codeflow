# Composition and design system

Read this reference when a primary composition is materially open, or when a
settled direction must become a system the product will reuse. It supplies
working detail, not formats, themes, palettes, component catalogs, or bans. Any
form named here is an example; the product's subject decides.

## Work the subject down to a form

Answer in order, in the product's own vocabulary, before naming any visual
form:

```text
subject: which real objects, data, artefacts and states are in play
  -> governing idea: the one thing a viewer must leave with
  -> user action: the decision or task the composition serves
  -> relationships: how the objects actually relate, and which are material
  -> what must be seen before anything is read
  -> the minimum labels that name what is already visible
  -> what belongs one layer deeper
```

Two checks keep this honest:

- **Removal.** Take the sentences out of the candidate composition. If the
  remaining structure no longer expresses the relationship, the structure was
  furniture and the prose was doing all the work.
- **Wrong first reading.** State the first thing a stranger would conclude from
  the composition. If that conclusion is wrong, or arrives only after reading a
  caption, the form is not encoding the idea.

## Let the relationship choose the form

Ask what kind of relationship dominates, then choose the least elaborate form
that exposes it honestly:

| Dominant relationship | What the form must make visible |
|---|---|
| Sequence or journey | direction, order, and where the user is |
| Comparison | the axes compared and where options actually differ |
| Composition or containment | what is inside what, and at what scale |
| Flow or transformation | what moves, what changes it, and where it lands |
| State or lifecycle | the real states, transitions, and terminal cases |
| Quantity or distribution | magnitude, proportion, and outliers that matter |
| Structure or authority | which parts govern which, and the boundaries |

A subject may have no dominant relationship worth drawing. Ordered prose, a
table, a form, or a plain list is then the correct form, not a failure to
design. Conversely, a familiar container is right whenever it maps to a real
object, boundary, state, or action — the defect is never familiarity, it is
substituting a container for the subject.

For anything spatial, physical, or genuinely dimensional, check whether depth
carries meaning before using it. For anything abstract, prefer disciplined flat
structure. Motion encodes a relationship — direction, accumulation, cause — or
it is garnish; always preserve comprehension without it.

## Research the subject before selecting a form

For a novel or materially open surface, study the subject's own world and the
audience's real context first. References may come from products, physical
materials, editorial systems, environments, tools, or cultural forms relevant
to the brief. Record what is being borrowed — an information rhythm, type
character, palette anchor, density, imagery approach, or motion stance — never
pixels or protected expression. An annotated reference or mood board is
optional evidence, worth making only when visual alignment is genuinely
uncertain and cheaper than building competing surfaces.

## Explore encodings by rendering them

At the open rung, produce two or three candidates that differ in what they
encode, then judge them rendered.

Settle four things before authoring, while the comparison is still cheap to
change. Each of them is expensive to fix once candidates are rendered, and two
of them can invalidate the whole comparison:

- **The question.** Write the question the candidates must settle, then check it
  against the surface's real job. A page whose job is orientation is not settled
  by a lookup question, and a question an ordinary sentence, table, or list
  already answers optimally cannot settle a composition at all — that comparison
  is decided by the question's shape before anything is drawn. Check too that
  every candidate's encoding *can* answer it, and that the answers differ; a
  question all candidates answer identically discriminates nothing.
- **What each candidate encodes.** Declare its primary unit, its primary axis,
  and the relationship it claims to expose. Two candidates sharing all three are
  one candidate in two costumes. Asserting this up front turns sibling
  distinctness into a check, rather than something discovered after both are
  built.
- **What would carry it.** Name the block, component, schema, or platform
  affordance in the product's real technical contract that could express each
  encoding, and what is lost if it cannot. Discovering after selection that the
  contract flattens the chosen encoding leaves a choice between unplanned
  contract work and shipping the encoding without the part that made it win.
- **Whether the question is compound.** A two-part question is usually settled
  best by two encodings, each strong at one part. Decompose it, or state which
  part the direction optimises and what carries the other. Forcing one winner
  across both parts selects a compromise nobody chose.

Then judge them rendered:

- use the product's real content, not lorem or invented figures;
- render at the viewports, modes, and input conditions that could change the
  reading, not only the author's window;
- keep fidelity as low as the question allows — a static frame answers a
  composition question; a prototype is only needed for an interaction question;
- keep the artefacts disposable and outside production code until the direction
  is settled;
- record for each candidate: encoded idea, comprehension effect, layering and
  navigation effect, system and maintenance effect, tradeoff, and the
  implementation seam it would need.

Keep the reader's channel and the machine's channel apart. Where the comparison
publishes a derived answer, digest, or provenance so a harness can verify the
candidate against its source, publish it somewhere the reader does not see.
A candidate that also states the answer in visible prose stops testing its
encoding: the observer reads the sentence, the plain baseline contains the same
sentence, and both the comprehension result and the baseline differential become
void rather than close. Verification integrity and comprehension evidence are
different obligations and must not share a surface.

Then choose in the open: options considered, the one chosen, why it wins, what
it costs, and what would have to be true to revisit it. If only one candidate
survives, say what eliminated the others.

A candidate is not materially different when it is the same content in new
furniture, a palette or typeface change, a familiar template refilled, a
generically generated diagram, or a variant distinguished only by ornament.
Stop exploring once the governing choice is settled; refinement after that is
bounded to named unresolved choices.

## Carry the idea across contexts

Decide which viewports, input modes, and platforms are applicable, then treat
the governing idea — not the fact inventory — as the thing that must survive
each one. The two are routinely confused: an encoding can keep every fact in a
narrow context and still lose its point.

Watch the encodings whose meaning lives in something a smaller context takes
away — simultaneous comparison across a wide axis, position along that axis,
reserved or deliberately empty space, or a drawn relationship between distant
elements. A matrix whose finding is the shape of its emptiness says nothing when
only one column fits. Bars whose meaning is their position say nothing once they
are all full width with a label. Neither is fixed by scrolling or by a caption.

Test at the sizes where the composition actually changes, which usually means an
intermediate one and not only the narrowest and widest — that middle context is
where a composition first stops fitting and is the one most often skipped.
Where a single composition cannot carry the idea across an applicable context,
declare a second composition for that context and render it too. A declared
alternate composition is a design decision; a compressed copy of the wide one is
the absence of a decision, and compressing a large layout is not designing a
small one.

## Earn the system after the direction is settled

Recurrence evidence comes before construction. A layer is earned when the
product already repeats the pattern, when the accepted lifetime makes change
likely, or when several surfaces must stay coherent.

A completed comparison is worth mining, because a rejected candidate can carry a
transferable primitive that would otherwise be thrown away with it. Read the
rejected work for those primitives deliberately.

What that mining produces is a **hypothesis, not recurrence evidence**. Two
candidates reaching the same encoding is a reason to look, and nothing more,
because the usual explanations are not recurrence at all:

- **One hand's habit.** The same author or the same study reusing a motif across
  its own candidates is authorial style. It says nothing about the product.
- **Alternatives that never coexist.** Candidates competing to settle one
  decision are mutually exclusive by construction. Four of them converging still
  ships one surface, not four, so nothing has to stay coherent with anything.
- **Speculative work outranking real work.** Candidates are cheap and
  provisional; accepted surfaces are neither. Convergence among discarded
  artefacts never outweighs a pattern the shipped product actually repeats.

So test the hypothesis before it earns anything. Did the candidates come from
genuinely independent subjects, cases, or authors, or from one hand and one
sitting? Will the surfaces carrying it actually coexist, and for a lifetime that
makes shared change likely? Does an accepted product surface have a real reuse
need for it now? Convergence that survives all three is worth recording as a
tested primitive. Convergence that does not is a note for later, and the right
outcome is often to implement it once, locally, and wait.

Convergence alone never earns a system layer. The recurrence rule below is
unchanged and still decides, the evidence still stops where it stops, and a
speculative primitive is not promoted past it. If a layer is later earned, take
the primitive and the state vocabulary it needs, named by meaning — never the
subject-specific forms built on it, because flattening several of those into one
generic component to make the reuse look tidier destroys the layer where the
product's design value actually lives (item 5 below).

Build in this order, and stop where the evidence stops:

1. **Semantic tokens.** Name by meaning and role, not by value or by the first
   surface that used them. One consumer is not a token layer.
2. **Modes and preferences.** Carry the decided appearance modes through every
   token, asset, and motion path; verify the preference source, override, and
   persistence rather than asserting them.
3. **Accessible primitives.** Focus, labelling, keyboard and pointer
   interaction, contrast, status, and error association built once against the
   project's target, so application components inherit them.
4. **Recurring components.** Only for patterns the product genuinely repeats,
   with the states they will actually be used in.
5. **Application-specific components.** Where the subject has structure no
   generic control expresses. These are usually where the product's real design
   value lives; do not flatten them into generic containers to reuse a library.
6. **Content and copy states.** Every applicable navigation, action, guidance,
   validation, empty, loading, error, disabled, success, destructive, and
   recovery case, in the project's voice.
7. **Responsive and platform behavior.** Reconsider the composition for each
   size and input mode in scope; compressing a large layout is not a small
   layout.
8. **Internationalization and localization.** Text expansion, writing
   direction, formatting, sorting, and locale-correct content, verified in a
   real variant when the product ships more than one.
9. **Implementation fidelity.** The shipped surfaces use the designed
   components; one-off styling does not accumulate beside the system.

Signals the system is being over-built: abstractions with a single consumer,
tokens that encode one surface's values, a component library preceding the
second surface, or configuration invented for a need no requirement states.
Signals it is under-built: the same decision re-made per surface, duplicated
state or styling logic, modes handled per component, or copy states discovered
during review.

## Across platforms

The altitude is the same everywhere; the vocabulary is not. Respect each
platform's native navigation, input, gesture, density, notification, and
accessibility conventions, and treat a cross-platform sameness argument and a
novelty argument with equal suspicion — both must be justified by the product,
not by convenience or fashion.

The evidence vocabulary changes with the platform too, and this is where
cross-platform work most often degrades into assertion. Each applicable platform
carries its own conventions and its own way of showing they were met: what the
platform defines as an accessible name, focus order, target size, text scaling,
safe area, or gesture affordance, and how that is actually observed there. A
browser render is evidence about the browser. It is not evidence about a native
mobile, tablet, or desktop surface, and describing a platform convention is not
evidence of conforming to it.

So state, per applicable platform, the conventions in force and how the surface
will be observed on it. Where a platform is out of scope, record an evidenced
`N/A`. Where it is in scope but could not be exercised, record it as unverified
and say so plainly — an unverified platform is a known gap, and inferring its
behavior from another platform's render converts a gap into a false claim.
