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
encode, then judge them rendered:

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

Then choose in the open: options considered, the one chosen, why it wins, what
it costs, and what would have to be true to revisit it. If only one candidate
survives, say what eliminated the others.

A candidate is not materially different when it is the same content in new
furniture, a palette or typeface change, a familiar template refilled, a
generically generated diagram, or a variant distinguished only by ornament.
Stop exploring once the governing choice is settled; refinement after that is
bounded to named unresolved choices.

## Earn the system after the direction is settled

Recurrence evidence comes before construction. A layer is earned when the
product already repeats the pattern, when the accepted lifetime makes change
likely, or when several surfaces must stay coherent. Build in this order, and
stop where the evidence stops:

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
