// Generated wrapper: registry.json, byte for byte, assigned to one global so
// board.html can read it with no network call. tools/verify.mjs proves the
// payload between the first newline and the trailing semicolon is identical to
// registry.json. Never edit this file; regenerate it from the registry.
window.__w3registry =
{
  "study": "tsk-014-w3",
  "supersedes_board": "docs/verification/tsk-014-w2",
  "states_note": "Each state names the drawn channel that carries it with no colour at all, and tools/verify.mjs requires that token to be in the page. Post-registration changes to these tokens and to any other registered field are recorded in `amendments` below, not summarised here.",
  "note": "The declarations here were written to precede authoring, and tools/verify.mjs enforces every field against the repository and against the rendered pages. What that priority is actually worth is stated in `pre_registration_evidence` rather than asserted. A field that cannot be checked is marked so explicitly.",

  "pre_registration_evidence": {
    "claim": "The rubric and this registry were written before any W3 candidate existed.",
    "what_the_files_can_show": "rubric.v2.md's modification time precedes every candidate page and every render in this working tree, and tools/verify.mjs now asserts that (it did not when the v2 board was produced — see rubric.v3.md). rubric.v2.md is additionally pinned by digest in rubric.v2.lock.json.",
    "what_the_files_cannot_show": "Nothing about this file. registry.json carries no lock, and its inode was created at 06:23:21 on 2026-08-04 — after seven of the eleven candidate pages had last been written, and after board.html, answer-key.md, observer-state.md, NEXT.md, content-inventory.md and hypotheses.md already existed. An atomic rewrite destroys the original creation time, so the filesystem carries no evidence that this registry preceded the candidates.",
    "what_the_session_transcript_shows": "That at least one candidate's encoding declarations were rewritten after its renders were inspected. The authoring session's Claude transcript carries the Edit event that changed both of a-critical-path-and-room's composition declarations, and both prior wordings are recorded verbatim in `amendments`. A transcript is outside this directory and outside version control, so it is not a durable witness either — but it is why two earlier claims are withdrawn below rather than repeated: that only the technique token changed, and that the prior wording was unrecoverable. Both appear in this file only as withdrawals.",
    "standing": "The rubric's priority is witnessed in this checkout and pinned by digest. This registry's priority is the author's account, cannot be proved from the files, and is known to be false for at least two fields. A2, A3 and A4 are checks against a declaration whose priority a later reader is taking on trust, and for a-critical-path-and-room's two composition bands they are checks against a declaration written to match its own drawing. Read `amendments` before weighing any of them.",
    "found_by": "independent Claude design-primary audit, 2026-08-04, from filesystem birth/modification times captured before a fresh checkout could destroy them; extended after the host recovered the prior declaration wording from the authoring session transcript",
    "durability_note": "The mtime evidence above is recorded here because it does not survive a Git checkout, and the transcript evidence is recorded here because the transcript is not part of this repository at all. After this directory is committed or copied, this file is the only remaining witness to either."
  },

  "amendments": [
    {
      "id": "p1a-intermediate-composition",
      "candidate": "a-critical-path-and-room",
      "field": "intermediate_composition",
      "when": "during authoring, in the same edit that changed narrow_composition, after the candidate's renders had been inspected",
      "what_moved_first": "the drawing",
      "before": "the wave axis stays horizontal but the room bars stack under their task rather than beside it, so position still reads and the bar length still compares",
      "after": "the single field splits into two registers against the same horizontal wave axis, separated by a drawn divider: the unbroken run above it, everything that can start later below it with its track. Position and extent both survive and the page stops being one field.",
      "what_happened": "The registered intermediate band was room bars stacked under their own task on an unchanged horizontal axis. What was drawn instead splits the field into two registers separated by a drawn divider — the unbroken run above, everything with a later start below. The field was then rewritten to describe the drawing.",
      "why_it_matters": "A3 is only a constraint on a composition if the declaration precedes it. Rewritten to match the drawing, this field describes rather than constrains, so A3's pass for this candidate's intermediate band means less than A3's pass elsewhere.",
      "recorded_by": "independent Claude design-primary audit, 2026-08-04, correcting its own earlier and false claim that the prior wording was unrecoverable",
      "prior_wording_provenance": "Recovered from the authoring session's Claude transcript (~/.claude/projects/…/604c82de-9861-4d19-920e-8f69b2db575c.jsonl, the Edit event near line 1190) by the host, and supplied to this session by the operator. This session could not read that file itself — the path is denied to it by permission settings — so the `before` string is second-hand. It is corroborated three ways from artefacts this session can read: the `after` string matches registry.json byte for byte; the drawn intermediate band implements the `after` description (a register divider labelled 'the run' / 'and what can start later') and not the `before` one; and the `before` string appears in no document in this directory."
    },
    {
      "id": "p1a-narrow-composition",
      "candidate": "a-critical-path-and-room",
      "field": "narrow_composition",
      "when": "during authoring, in the same edit that changed intermediate_composition, after the candidate's mobile render had been inspected",
      "what_moved_first": "the drawing",
      "before": "the wave axis rotates to vertical: waves become rows down the page, the link runs down the page through them, and room extends rightward from each task. Position still encodes wave; it does not become full-width boxes with a wave number printed on them.",
      "after": "the axis rotates. Waves become rows down the page and the run descends through them, and extent turns with the axis: a later start is measured down the same rows, in a gutter of its own, with a tick at each wave line it crosses. Position still encodes wave; it does not become full-width boxes with a wave number printed on them.",
      "what_happened": "The registered narrow band had room extending rightward from each task after the axis rotated. What was drawn instead measures a later start downward, in one shared gutter, ticked at each wave line it crosses. The field was then rewritten to describe the drawing. The change is not cosmetic: rightward extent per task and downward extent in a shared gutter are different geometries, and the shared gutter is the direct cause of the collision recorded against this candidate's mobile band in invalidated_bands.",
      "why_it_matters": "A3 is only a constraint on a composition if the declaration precedes it. Rewritten to match the drawing, this field describes rather than constrains — and the drawing it was rewritten to describe does not work, so the rewrite removed the one declaration that could have caught it.",
      "recorded_by": "independent Claude design-primary audit, 2026-08-04, correcting its own earlier and false claim that the prior wording was unrecoverable",
      "prior_wording_provenance": "Same source and same limitation as p1a-intermediate-composition. Corroborated from artefacts this session can read: the `after` string matches registry.json byte for byte; the drawn narrow band measures extent down a shared gutter with a tick at each crossed wave line, exactly as `after` says and not as `before` says; the phrase 'room extends rightward' appears nowhere in the drawing or in any document here.",
      "supersedes_claim": "Two earlier claims are withdrawn, not reworded. First, the original `states_note` said only technique tokens changed and that no primary unit, axis or encoded relationship was touched after registration: the second half remains true and is separately checkable, the first half was false. Second, this audit's own first correction said the prior wording was unrecoverable and that only one field had changed: both were false. Two fields changed, in one edit, and the prior wording was recoverable from the session transcript."
    },
    {
      "id": "non-colour-marker-tokens",
      "candidate": "several",
      "field": "states[].non_colour_marker",
      "when": "during authoring",
      "what_moved_first": "the drawing",
      "what_happened": "A pre-authoring guess named an SVG property where the realised composition used a CSS one or a named class, and the tokens were corrected to match.",
      "why_it_matters": "The token is what verify.mjs looks for in the page, so a corrected token makes the check pass by construction. The check proves the declared channel is present in the markup; it does not prove the channel was chosen before the drawing.",
      "recorded_by": "the study's author, restated here without the scope claim that accompanied it"
    },
    {
      "id": "d1c-straddle-divergence",
      "candidate": "c-write-authority-map",
      "field": "encoded_relationship",
      "when": "not amended — recorded here as an unreconciled divergence",
      "what_moved_first": "neither; the divergence was never noticed",
      "what_happened": "The registration says the one path whose authority is split is 'shown straddling the perimeter'. The page draws it wholly inside the editable region with a miniature internal divider (`cases/d1/c-write-authority-map/index.html`, the straddle block is appended only when `band.id === \"yours\"`). The perimeter itself is drawn further down the page, after the second band.",
      "why_it_matters": "This is the failure A8 exists to prevent, caught in the other direction: the declaration was left alone and the drawing does something else, and no gate compares them. It is recorded rather than fixed, because changing either one is a design decision belonging to the author or the operator, not to the seat judging the board.",
      "recorded_by": "independent Claude design-primary audit, 2026-08-04"
    },
    {
      "id": "p2a-unkeyed-mark",
      "candidate": "a-stopped-provenance",
      "field": "states",
      "when": "not amended — recorded here as an unreconciled divergence",
      "what_moved_first": "the drawing",
      "what_happened": "The coverage strip draws five cell states — `executed`, `bounded`, `notrun`, `na` and `notclaimed`. The registry declares four states for this candidate and the page's legend keys four marks; `notclaimed`, drawn as a cross on the Chromium claim's second lane, appears in neither. The figure's own `<desc>` — which README.md names as the accessible equivalent — also lists four.",
      "why_it_matters": "An information-bearing mark with no key is unreadable by construction, and the accessible description is incomplete on all six renders of this candidate in both modes and all three widths. A4 checks carrier verdicts and A2 checks distinctness; nothing checks that every mark the page draws is declared and keyed.",
      "recorded_by": "independent Claude design-primary audit, 2026-08-04"
    }
  ],

  "invalidated_bands_note": "Every entry here is inadmissible as comparison evidence AND carries a demonstrated failure. 'Invalidated' is not neutral and does not mean 'unjudged': each band names what fails, on which axis, and how badly. What no entry claims is a rubric gate result — G1 to G8 need an observer who authored nothing and read no answer, and this audit read the answer key. A measured clip and a collision do not need that observer to be true.",

  "invalidated_bands": [
    {
      "render": "p3-a-convergence-with-source-depth-light-mobile",
      "band": "mobile",
      "evidence_status": "inadmissible — no timed or blind observation gate may be recorded from this render",
      "failure": {
        "demonstrated": true,
        "kind": "containment",
        "severity": "material",
        "failing_axes": [
          "fact presence"
        ],
        "what_fails": "The off-axis arrival's label is drawn past the figure's viewBox: 'Playwright 1.55' overshoots by 8.8px and is cut by the frame, and 'Chrome 150' sits on the hatched region's dashed boundary. The identity of the external cause is the payload of the 'not a round' annotation, so what is lost is the answer to the second half of P3's question.",
        "needs_a_blind_observer_to_confirm": false,
        "why_not": "The failure is a measured or directly observable property of the render — clipped content, a mark drawn past its frame, two marks occupying one position. A blind observer adds nothing to it, and an author's or judge's knowledge of the answer cannot make a clipped line uncut."
      },
      "machine_visible": true,
      "fix_is": "mechanical — the label needs measuring and containing inside its region",
      "disposition": "recorded and handed back. The audit redrew nothing: the seat judging a board cannot repair the drawings it is judging without becoming their author, and the operator has not yet chosen between them.",
      "found_by": "host-reported, independently confirmed and corrected by the audit"
    },
    {
      "render": "p3-a-convergence-with-source-depth-dark-mobile",
      "band": "mobile",
      "evidence_status": "inadmissible — no timed or blind observation gate may be recorded from this render",
      "failure": {
        "demonstrated": true,
        "kind": "containment",
        "severity": "material",
        "failing_axes": [
          "fact presence"
        ],
        "what_fails": "The same clipping as the light mobile render; the geometry is mode-independent.",
        "needs_a_blind_observer_to_confirm": false,
        "why_not": "The failure is a measured or directly observable property of the render — clipped content, a mark drawn past its frame, two marks occupying one position. A blind observer adds nothing to it, and an author's or judge's knowledge of the answer cannot make a clipped line uncut."
      },
      "machine_visible": true,
      "fix_is": "mechanical — as the light mobile render",
      "disposition": "recorded and handed back. The audit redrew nothing: the seat judging a board cannot repair the drawings it is judging without becoming their author, and the operator has not yet chosen between them.",
      "found_by": "host-reported, independently confirmed and corrected by the audit"
    },
    {
      "render": "p3-b-source-region-history-light-desktop",
      "band": "desktop",
      "evidence_status": "inadmissible — no timed or blind observation gate may be recorded from this render",
      "failure": {
        "demonstrated": true,
        "kind": "legibility",
        "severity": "minor",
        "failing_axes": [
          "fact presence"
        ],
        "what_fails": "Ten rail tick labels overshoot their viewBox by 1.8px vertically and lose their descenders. Small, but it is the round axis this candidate's whole encoding is read against.",
        "needs_a_blind_observer_to_confirm": false,
        "why_not": "The failure is a measured or directly observable property of the render — clipped content, a mark drawn past its frame, two marks occupying one position. A blind observer adds nothing to it, and an author's or judge's knowledge of the answer cannot make a clipped line uncut."
      },
      "machine_visible": true,
      "fix_is": "mechanical — the viewBox needs to include the label descenders",
      "disposition": "recorded and handed back. The audit redrew nothing: the seat judging a board cannot repair the drawings it is judging without becoming their author, and the operator has not yet chosen between them.",
      "found_by": "independent Claude design-primary audit, 2026-08-04"
    },
    {
      "render": "p3-b-source-region-history-dark-desktop",
      "band": "desktop",
      "evidence_status": "inadmissible — no timed or blind observation gate may be recorded from this render",
      "failure": {
        "demonstrated": true,
        "kind": "legibility",
        "severity": "minor",
        "failing_axes": [
          "fact presence"
        ],
        "what_fails": "Ten rail tick labels overshoot their viewBox by 1.8px vertically and lose their descenders, exactly as in the light render; the geometry is mode-independent.",
        "needs_a_blind_observer_to_confirm": false,
        "why_not": "The failure is a measured or directly observable property of the render — clipped content, a mark drawn past its frame, two marks occupying one position. A blind observer adds nothing to it, and an author's or judge's knowledge of the answer cannot make a clipped line uncut."
      },
      "machine_visible": true,
      "fix_is": "mechanical — as the light desktop render",
      "disposition": "recorded and handed back. The audit redrew nothing: the seat judging a board cannot repair the drawings it is judging without becoming their author, and the operator has not yet chosen between them.",
      "found_by": "independent Claude design-primary audit, 2026-08-04"
    },
    {
      "render": "p3-b-source-region-history-light-tablet",
      "band": "tablet",
      "evidence_status": "inadmissible — no timed or blind observation gate may be recorded from this render",
      "failure": {
        "demonstrated": true,
        "kind": "containment",
        "severity": "material",
        "failing_axes": [
          "fact presence"
        ],
        "what_fails": "Two quoted-source blocks silently hide 7px and 29px of their own content, and the rail tick labels overshoot as at desktop.",
        "needs_a_blind_observer_to_confirm": false,
        "why_not": "The failure is a measured or directly observable property of the render — clipped content, a mark drawn past its frame, two marks occupying one position. A blind observer adds nothing to it, and an author's or judge's knowledge of the answer cannot make a clipped line uncut."
      },
      "machine_visible": true,
      "fix_is": "a design decision — the candidate quotes real source in a column that cannot hold it, and choosing between wrapping, shrinking, and an explicit continuation marker changes what the candidate is",
      "disposition": "recorded and handed back. The audit redrew nothing: the seat judging a board cannot repair the drawings it is judging without becoming their author, and the operator has not yet chosen between them.",
      "found_by": "independent Claude design-primary audit, 2026-08-04"
    },
    {
      "render": "p3-b-source-region-history-dark-tablet",
      "band": "tablet",
      "evidence_status": "inadmissible — no timed or blind observation gate may be recorded from this render",
      "failure": {
        "demonstrated": true,
        "kind": "containment",
        "severity": "material",
        "failing_axes": [
          "fact presence"
        ],
        "what_fails": "Two quoted-source blocks silently hide 7px and 29px of their own content and the rail tick labels overshoot, exactly as in the light render; the geometry is mode-independent.",
        "needs_a_blind_observer_to_confirm": false,
        "why_not": "The failure is a measured or directly observable property of the render — clipped content, a mark drawn past its frame, two marks occupying one position. A blind observer adds nothing to it, and an author's or judge's knowledge of the answer cannot make a clipped line uncut."
      },
      "machine_visible": true,
      "fix_is": "a design decision — as the light tablet render",
      "disposition": "recorded and handed back. The audit redrew nothing: the seat judging a board cannot repair the drawings it is judging without becoming their author, and the operator has not yet chosen between them.",
      "found_by": "independent Claude design-primary audit, 2026-08-04"
    },
    {
      "render": "p3-b-source-region-history-light-mobile",
      "band": "mobile",
      "evidence_status": "inadmissible — no timed or blind observation gate may be recorded from this render",
      "failure": {
        "demonstrated": true,
        "kind": "containment",
        "severity": "disqualifying",
        "failing_axes": [
          "fact presence",
          "idea survival"
        ],
        "what_fails": "Thirteen quoted-source blocks silently hide up to 430px of a 332px container — more than half of most quoted lines is not on the page, with no ellipsis, fade or affordance in a still frame — and the file-level round rail itself hides 133px of 360 and runs off the right edge. The candidate's A6 contract is that every quoted diff line matches `git show` character for character; the render does not carry the line it verifies.",
        "correction_of_a_prior_report": "The host reported this as text being unreadable at the rendered size. It is not: the type is 11px, small but legible. The defect is silent truncation, and it is present at desktop and tablet too.",
        "needs_a_blind_observer_to_confirm": false,
        "why_not": "The failure is a measured or directly observable property of the render — clipped content, a mark drawn past its frame, two marks occupying one position. A blind observer adds nothing to it, and an author's or judge's knowledge of the answer cannot make a clipped line uncut."
      },
      "machine_visible": true,
      "fix_is": "a design decision — as the tablet band, and more acutely",
      "disposition": "recorded and handed back. The audit redrew nothing: the seat judging a board cannot repair the drawings it is judging without becoming their author, and the operator has not yet chosen between them.",
      "found_by": "host-reported, independently confirmed and corrected by the audit"
    },
    {
      "render": "p3-b-source-region-history-dark-mobile",
      "band": "mobile",
      "evidence_status": "inadmissible — no timed or blind observation gate may be recorded from this render",
      "failure": {
        "demonstrated": true,
        "kind": "containment",
        "severity": "disqualifying",
        "failing_axes": [
          "fact presence",
          "idea survival"
        ],
        "what_fails": "Thirteen quoted-source blocks hide up to 430px of a 332px container and the file-level round rail hides 133px of 360, exactly as in the light render; the geometry is mode-independent.",
        "needs_a_blind_observer_to_confirm": false,
        "why_not": "The failure is a measured or directly observable property of the render — clipped content, a mark drawn past its frame, two marks occupying one position. A blind observer adds nothing to it, and an author's or judge's knowledge of the answer cannot make a clipped line uncut."
      },
      "machine_visible": true,
      "fix_is": "a design decision — as the light mobile render",
      "disposition": "recorded and handed back. The audit redrew nothing: the seat judging a board cannot repair the drawings it is judging without becoming their author, and the operator has not yet chosen between them.",
      "found_by": "host-reported, independently confirmed and corrected by the audit"
    },
    {
      "render": "p2-baseline-light-mobile",
      "band": "mobile",
      "evidence_status": "inadmissible — no timed or blind observation gate may be recorded from this render",
      "failure": {
        "demonstrated": true,
        "kind": "containment",
        "severity": "material",
        "failing_axes": [
          "fact presence"
        ],
        "what_fails": "The plain baseline overflows the document horizontally at 390px (475px of content in a 390px viewport), so part of the control is off the page.",
        "affects": "G3, not a candidate. The baselines are the differential's control; a control the reader cannot see whole makes the comparison unfair in the baseline's favour as well as the candidate's.",
        "needs_a_blind_observer_to_confirm": false,
        "why_not": "The failure is a measured or directly observable property of the render — clipped content, a mark drawn past its frame, two marks occupying one position. A blind observer adds nothing to it, and an author's or judge's knowledge of the answer cannot make a clipped line uncut."
      },
      "machine_visible": true,
      "fix_is": "mechanical, but constrained — the baseline must become legible without becoming a design",
      "disposition": "recorded and handed back. The audit redrew nothing: the seat judging a board cannot repair the drawings it is judging without becoming their author, and the operator has not yet chosen between them.",
      "found_by": "independent Claude design-primary audit, 2026-08-04; not previously reported by anyone"
    },
    {
      "render": "d1-baseline-light-mobile",
      "band": "mobile",
      "evidence_status": "inadmissible — no timed or blind observation gate may be recorded from this render",
      "failure": {
        "demonstrated": true,
        "kind": "containment",
        "severity": "material",
        "failing_axes": [
          "fact presence"
        ],
        "what_fails": "The plain baseline overflows the document horizontally at 390px — 477px of content in a 390px viewport — so part of the control is off the page.",
        "affects": "G3, not a candidate — as p2-baseline.",
        "needs_a_blind_observer_to_confirm": false,
        "why_not": "The failure is a measured or directly observable property of the render — clipped content, a mark drawn past its frame, two marks occupying one position. A blind observer adds nothing to it, and an author's or judge's knowledge of the answer cannot make a clipped line uncut."
      },
      "machine_visible": true,
      "fix_is": "mechanical, but constrained — as p2-baseline",
      "disposition": "recorded and handed back. The audit redrew nothing: the seat judging a board cannot repair the drawings it is judging without becoming their author, and the operator has not yet chosen between them.",
      "found_by": "independent Claude design-primary audit, 2026-08-04; not previously reported by anyone"
    },
    {
      "render": "p1-a-critical-path-and-room-light-mobile",
      "band": "mobile",
      "evidence_status": "inadmissible — no timed or blind observation gate may be recorded from this render",
      "failure": {
        "demonstrated": true,
        "kind": "composition",
        "severity": "disqualifying",
        "failing_axes": [
          "fact presence",
          "idea survival"
        ],
        "what_fails": "Everything is inside the frame, so no containment probe sees this. All room tracks share one gutter at a single x: TSK-013 (room 2, boundary 1 to 3) and TSK-009 (room 1, boundary 2 to 3) are drawn at the same x and merge into one continuous column, so the reader cannot recover which task has how much room. Three different caps land on boundary 3 — TSK-013's limit, TSK-009's limit, and TSK-007's zero-room cap — and render as a single mark. The remaining zero-room caps are occluded by the tracks or orphaned: TSK-010's sits alone below the last row, attached to nothing.",
        "why_this_band_matters": "The shared gutter is exactly what narrow_composition was rewritten to describe after this render was inspected (see amendments/p1a-narrow-composition). The declaration that could have caught this was replaced by a description of it.",
        "needs_a_blind_observer_to_confirm": false,
        "why_not": "The failure is a measured or directly observable property of the render — clipped content, a mark drawn past its frame, two marks occupying one position. A blind observer adds nothing to it, and an author's or judge's knowledge of the answer cannot make a clipped line uncut."
      },
      "machine_visible": false,
      "fix_is": "a design decision — the tracks need separating or the extent needs re-encoding, and either changes the candidate",
      "disposition": "recorded and handed back. The audit redrew nothing: the seat judging a board cannot repair the drawings it is judging without becoming their author, and the operator has not yet chosen between them.",
      "found_by": "independent Claude design-primary audit, 2026-08-04; not previously reported by anyone"
    },
    {
      "render": "p1-a-critical-path-and-room-dark-mobile",
      "band": "mobile",
      "evidence_status": "inadmissible — no timed or blind observation gate may be recorded from this render",
      "failure": {
        "demonstrated": true,
        "kind": "composition",
        "severity": "disqualifying",
        "failing_axes": [
          "fact presence",
          "idea survival"
        ],
        "what_fails": "The room tracks merge into one continuous column, three caps collapse onto boundary 3, and TSK-010's zero-room cap is orphaned below the last row, exactly as in the light render; the geometry is mode-independent.",
        "needs_a_blind_observer_to_confirm": false,
        "why_not": "The failure is a measured or directly observable property of the render — clipped content, a mark drawn past its frame, two marks occupying one position. A blind observer adds nothing to it, and an author's or judge's knowledge of the answer cannot make a clipped line uncut."
      },
      "machine_visible": false,
      "fix_is": "a design decision — as the light mobile render",
      "disposition": "recorded and handed back. The audit redrew nothing: the seat judging a board cannot repair the drawings it is judging without becoming their author, and the operator has not yet chosen between them.",
      "found_by": "independent Claude design-primary audit, 2026-08-04; not previously reported by anyone"
    },
    {
      "render": "p2-a-stopped-provenance-light-mobile",
      "band": "mobile",
      "evidence_status": "inadmissible — no timed or blind observation gate may be recorded from this render",
      "failure": {
        "demonstrated": true,
        "kind": "composition",
        "severity": "disqualifying",
        "failing_axes": [
          "idea survival"
        ],
        "what_fails": "Everything is inside the frame. Each stop is drawn as a long horizontal rule running from the figure's left edge across roughly 40% of empty width to the rail, where it reads as a section divider rather than as a stop on a rail; and each stop's cause caption is left-aligned at the far edge, directly abutting the NEXT claim's title with no separation, so the cause attaches visually to the wrong claim.",
        "why_this_band_matters": "P2's registered question is the attribution of causes to absences. Attribution is the part this band loses.",
        "needs_a_blind_observer_to_confirm": false,
        "why_not": "The failure is a measured or directly observable property of the render — clipped content, a mark drawn past its frame, two marks occupying one position. A blind observer adds nothing to it, and an author's or judge's knowledge of the answer cannot make a clipped line uncut."
      },
      "machine_visible": false,
      "fix_is": "a design decision — the stop mark and its caption need re-placing against the rotated rail",
      "disposition": "recorded and handed back. The audit redrew nothing: the seat judging a board cannot repair the drawings it is judging without becoming their author, and the operator has not yet chosen between them.",
      "found_by": "independent Claude design-primary audit, 2026-08-04; not previously reported by anyone"
    },
    {
      "render": "p2-a-stopped-provenance-dark-mobile",
      "band": "mobile",
      "evidence_status": "inadmissible — no timed or blind observation gate may be recorded from this render",
      "failure": {
        "demonstrated": true,
        "kind": "composition",
        "severity": "disqualifying",
        "failing_axes": [
          "idea survival"
        ],
        "what_fails": "Each stop reads as a section divider across empty width and each cause caption abuts the next claim's title, exactly as in the light render; the geometry is mode-independent.",
        "needs_a_blind_observer_to_confirm": false,
        "why_not": "The failure is a measured or directly observable property of the render — clipped content, a mark drawn past its frame, two marks occupying one position. A blind observer adds nothing to it, and an author's or judge's knowledge of the answer cannot make a clipped line uncut."
      },
      "machine_visible": false,
      "fix_is": "a design decision — as the light mobile render",
      "disposition": "recorded and handed back. The audit redrew nothing: the seat judging a board cannot repair the drawings it is judging without becoming their author, and the operator has not yet chosen between them.",
      "found_by": "independent Claude design-primary audit, 2026-08-04; not previously reported by anyone"
    }
  ],

  "contexts": {
    "note": "The tablet width is not a convention: every candidate declares the two widths at which its own composition changes, and the check below requires the intermediate render to land strictly between them, so the tablet frame always shows a composition neither of the other two frames shows.",
    "viewports": {
      "desktop": { "width": 1440, "height": 900 },
      "tablet": { "width": 900, "height": 1200 },
      "mobile": { "width": 390, "height": 844 }
    },
    "rule": "for every candidate, narrow_breakpoint < tablet.width < wide_breakpoint, and the page must declare a max-width media query at each of its two breakpoints",
    "modes": ["light", "dark"],
    "motion": {
      "policy": "none-authored",
      "why": "No candidate encodes a relationship in movement, and the one carrier that could host these compositions forbids the CSS that motion and its reduction would need. Motion is therefore proven absent at runtime rather than asserted, and no reduced-motion screenshot is taken: a duplicate still of a motionless page is evidence about the screenshot, not about motion.",
      "evidence": "checks/motion.json — per render, document.getAnimations() and a computed-style sweep for a running animation or a non-zero transition duration, captured under both prefers-reduced-motion settings"
    }
  },

  "carrier_contracts": {
    "cf-present": {
      "surface": "the CodeFlow presentation utility (SPC-004)",
      "schema": "assets/base/present/schemas/document-v1.schema.json",
      "closed_block_catalogue": true,
      "escape_block": "html",
      "facts": [
        {
          "id": "escape-is-the-only-free-geometry",
          "statement": "Of the typed blocks only `diagram` and `html` can carry drawn geometry. `diagram` is Mermaid restricted to seven kinds, so it cannot place a mark at a computed position, reserve empty space, hatch a region, or size a bar to a quantity.",
          "checked_by": "schema $defs enumeration and the diagram block's kind enum"
        },
        {
          "id": "escape-forbids-at-rules",
          "statement": "Inline CSS inside the sandboxed HTML block may not contain `@`, so a composition placed there has no media query: no responsive recomposition, no prefers-color-scheme, and no prefers-reduced-motion.",
          "source": "crates/codeflow-present/src/safe_html.rs",
          "quote": "lower.contains('@')"
        },
        {
          "id": "escape-forbids-script-and-smil",
          "statement": "Script is refused by the validator and again by the sandbox CSP, and the SMIL animation elements are on the forbidden-element list, so the block cannot resize itself or animate.",
          "source": "crates/codeflow-present/src/service.rs",
          "quote": "default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src data:;"
        },
        {
          "id": "escape-is-a-fixed-frame",
          "statement": "The block is rendered into a sandboxed iframe, and the review stylesheet constrains only its inline size, so it keeps the HTML default replaced-element box rather than growing to its content.",
          "source": "crates/codeflow-present/web/src/styles.css",
          "quote": "#cf-present-document iframe, #cf-present-document img, #cf-present-document video { max-inline-size: 100%; }"
        },
        {
          "id": "escape-is-rationed",
          "statement": "The contract permits the escape block as a bounded v1 concession, not as an authoring pattern for the utility's own explanations.",
          "source": "project-management/specs/SPC-004.md",
          "quote": "V1 permits one session-local sandboxed HTML escape block with no network, credentials, review-chrome access, or durable execution."
        }
      ],
      "verdict_meanings": {
        "native": "expressible in typed blocks with the governing relationship intact",
        "escape-with-loss": "expressible only through the sandboxed HTML escape block, losing at least one thing the encoding depends on",
        "needs-new-block": "not expressible today; selecting it commits to a new typed block whose renderer owns mode and responsive behaviour"
      }
    },
    "docs-portal": {
      "surface": "the CodeFlow documentation portal (SPC-005)",
      "generator": "docs-portal/scripts/lib.mjs",
      "facts": [
        {
          "id": "source-html-is-escaped",
          "statement": "Raw HTML written in a repository Markdown source is escaped, so a portal composition can never be authored in the content it publishes — only emitted by the generator.",
          "source": "docs-portal/scripts/lib.mjs",
          "quote": "else parent.children[index] = { type: \"html\", value: escapeGeneratedHtml(node.value) };"
        },
        {
          "id": "no-component-runtime",
          "statement": "The portal installs no MDX integration, so there is no component layer in content either; the generator's emitted markup plus one portal stylesheet is the whole composition surface.",
          "source": "docs-portal/package.json",
          "checked_by": "structural: @astrojs/mdx must be absent from dependencies and devDependencies",
          "quote": "\"@astrojs/starlight\": \"0.41.6\""
        },
        {
          "id": "styling-is-real-css",
          "statement": "Unlike the presentation escape block, the portal's composition lives in a real stylesheet, so media queries, colour-scheme handling and reduced motion are all available.",
          "source": "docs-portal/astro.config.mjs",
          "quote": "customCss: [\"./src/styles/portal.css\", \"./.portal/generated/project-tokens.css\"]"
        }
      ],
      "verdict_meanings": {
        "needs-generator-work": "the composition is expressible with no contract change; the cost is a new emitter and stylesheet work",
        "needs-source-model": "the composition additionally needs a fact the source graph does not carry today, so a source-model change comes with it"
      }
    }
  },

  "cases": [
    {
      "id": "p1",
      "surface": "cf-present",
      "subject": "the unfinished EPC-005 task graph",
      "surface_job": "An operator is shown a plan inside a review document and must decide where the schedule risk is before approving or redirecting it. The job is judgement about consequence, not lookup of a field.",
      "question": "If exactly one unfinished EPC-005 task slipped by one wave, which tasks would move the finish and which would not?",
      "question_fitness": {
        "matches_job": "the operator's real decision is which task to protect",
        "answerable_by_each_encoding": true,
        "discriminates": "a candidate that shows only order, or only dependency incidence, cannot separate a task with room from one without"
      },
      "compound": { "is_compound": false },
      "shared_source": "shared/plan-model.js",
      "forbidden_phrases": [
        "moves the finish",
        "move the finish",
        "sets the finish",
        "critical chain",
        "longest chain",
        "longest path",
        "can wait",
        "could wait",
        "start today"
      ],
      "forbidden_patterns": [
        { "pattern": "TSK-\\d{3}\\s*(?:→|->|>)\\s*TSK-\\d{3}", "why": "the chain written out as a run of arrows states the relationship the geometry exists to show" },
        { "pattern": "TSK-\\d{3}[^\\n]{0,60}\\b(?:one|two|three|1|2|3)\\s+waves?\\s+of\\s+room", "why": "attaching a room quantity to a named task in prose answers the question in a sentence" },
        { "pattern": "\\b(?:four|4)[- ]task\\b", "why": "names the length of the chain the reader is asked to find" }
      ],
      "candidates": ["a-critical-path-and-room", "b-paths-to-the-sink"]
    },
    {
      "id": "p2",
      "surface": "cf-present",
      "subject": "the TSK-007 qualification evidence at exact candidate 82671551",
      "surface_job": "A reviewer is shown an evidence set and must judge what it does not cover before accepting it. The job is to see the shape of what is missing, and why each piece is missing, without the absences being smoothed away.",
      "question": "Which platforms carry executed evidence for the exact candidate, and for each platform that does not, what kind of thing stopped it?",
      "question_fitness": {
        "matches_job": "acceptance turns on the causes of the gaps, not on the count of passes",
        "answerable_by_each_encoding": true,
        "discriminates": "an encoding that renders absence as a blank cell can answer the first half and not the second"
      },
      "compound": {
        "is_compound": true,
        "parts": ["platform coverage", "cause of each absence"],
        "handling": "explicit-optimisation",
        "why_not_decomposed": "the two parts are one judgement for the reviewer — coverage is only interpretable through the causes — so both candidates carry both, and each declares which part its axis optimises"
      },
      "shared_source": "shared/tsk007-facts.js",
      "forbidden_phrases": [
        "only macOS",
        "macOS arm64 only",
        "one platform deep",
        "blocked by authorization",
        "rather than by a test",
        "rather than a test failure",
        "four of six",
        "two of six"
      ],
      "forbidden_patterns": [
        { "pattern": "\\b(?:four|4|six|6)\\s+of\\s+(?:four|4|six|6)\\b", "why": "a counted summary of how many claims stop short is the answer, not a label" },
        { "pattern": "no\\s+(?:other\\s+)?platform[^\\n]{0,40}(?:evidence|run|executed)", "why": "states the coverage finding in prose" }
      ],
      "candidates": ["a-stopped-provenance", "b-absence-by-cause"]
    },
    {
      "id": "p3",
      "surface": "cf-present",
      "subject": "the eight rounds of exact review over the TSK-007 qualification harness",
      "surface_job": "A reader is shown a review that did not converge in one pass and must tell a fix that failed from a fix that was overtaken by something the review did not control. The job is attribution.",
      "question": "Which findings took more than one round to close, and which of those was re-opened by something that did not come from the review itself?",
      "question_fitness": {
        "matches_job": "attribution decides whether the harness or its environment is the thing to fix",
        "answerable_by_each_encoding": true,
        "discriminates": "an encoding with no ordered round axis cannot separate a re-opening from an ordinary later edit"
      },
      "compound": { "is_compound": false },
      "shared_source": "shared/p3-convergence.js",
      "forbidden_phrases": [
        "outside the harness rather than",
        "needed more than one round",
        "took more than one round",
        "three findings",
        "re-opened from outside",
        "reopened from outside",
        "not caused by the previous fix"
      ],
      "forbidden_patterns": [
        { "pattern": "F\\d[^\\n]{0,30}F\\d[^\\n]{0,30}F\\d", "why": "the answer set enumerated in one line of prose" },
        { "pattern": "F\\d[^\\n]{0,60}outside", "why": "names which finding the external cause belongs to instead of placing it" }
      ],
      "candidates": ["a-convergence-with-source-depth", "b-source-region-history"]
    },
    {
      "id": "d1",
      "surface": "docs-portal",
      "subject": "the six layers this repository organises its knowledge into",
      "surface_job": "The portal's Orient layer brings a reader who has never seen this repository into it. The job is to leave with a usable model of where knowledge lives and what may be done to it — not to look up one fact. A numbered list wins a lookup question by construction, which is why the W2 registration was withdrawn.",
      "question": "Where does this repository keep each kind of knowledge, and how is each kind allowed to change?",
      "question_fitness": {
        "matches_job": "orientation is exactly the pairing of location with the discipline that governs it",
        "answerable_by_each_encoding": true,
        "discriminates": "a plain list of six layers gives location and nothing about who may write them"
      },
      "compound": {
        "is_compound": true,
        "parts": ["where each kind of knowledge lives", "how each kind may change"],
        "handling": "explicit-optimisation",
        "why_not_decomposed": "orientation fails if the two are learned separately; the reader needs one model, so each candidate declares which part its axis optimises and how it carries the other"
      },
      "shared_source": "shared/repo-layers.js",
      "forbidden_phrases": [
        "may be edited by hand",
        "you may edit",
        "you can edit",
        "must not be edited",
        "cannot be edited",
        "everything else is generated",
        "in short"
      ],
      "forbidden_patterns": [
        { "pattern": "(?:three|four|five|six|3|4|5|6)\\s+of\\s+(?:the\\s+)?six\\s+layers", "why": "a counted summary of the partition is the answer" },
        { "pattern": "the\\s+answer\\s+is", "why": "an explicit answer sentence" }
      ],
      "candidates": ["a-layers-by-change-rate", "b-the-spine", "c-write-authority-map"]
    },
    {
      "id": "d2",
      "surface": "docs-portal",
      "subject": "SPC-004 as a portal record page, with its decision lineage and its evidence",
      "surface_job": "A record page must let a reader trust or distrust the record in front of them: which accepted decision still governs it and how far that decision reaches, and which of its claims are actually backed. Both are needed to judge the record, and they are different shapes of question.",
      "question": "Which decision governs this record now, how much of it does that decision actually reach, and which of its claims has nothing behind it?",
      "question_fitness": {
        "matches_job": "trust in a record is exactly authority plus evidence",
        "answerable_by_each_encoding": true,
        "discriminates": "a two-column table answers neither: partial reach collapses to a link, and deliberate absence becomes an empty cell indistinguishable from an omission"
      },
      "compound": {
        "is_compound": true,
        "parts": ["reach of the governing decision", "presence or deliberate absence of evidence"],
        "handling": "decomposed",
        "why_decomposed": "the two parts want opposite geometry — reach wants extent drawn across the record, evidence wants adjacency at each claim — and forcing one candidate to win both selects a compromise nobody chose. The pair is registered as a decomposition: neither is the other's defeated sibling, and the settlement may take one, the other, or the composition of both."
      },
      "shared_source": "shared/record-authority.js",
      "forbidden_phrases": [
        "only partially",
        "governs it partially",
        "has no evidence",
        "is not backed",
        "unbacked",
        "nothing behind it",
        "the central claim"
      ],
      "forbidden_patterns": [
        { "pattern": "ADR-\\d{4}[^\\n]{0,40}(?:partial|only)", "why": "states the reach finding instead of drawing it" },
        { "pattern": "CLM-\\d[^\\n]{0,40}(?:absent|missing|no evidence)", "why": "names the unbacked claim in prose" }
      ],
      "candidates": ["a-decision-reach", "b-evidence-margin"]
    }
  ],

  "candidates": [
    {
      "id": "a-critical-path-and-room",
      "case": "p1",
      "primary_unit": "an unfinished task",
      "primary_axis": "wave position — the earliest wave the task can occupy, left to right",
      "encoded_relationship": "consequence of delay: one unbroken line threads the tasks it passes through, and every task carries a cap on the same wave scale — set out along a ticked track where a later start exists, flush on the task's own wave line where none does, so having none is drawn rather than left out",
      "optimises": "n/a",
      "carries_other_by": "n/a",
      "breakpoints": { "wide": 1080, "narrow": 640 },
      "intermediate_composition": "the single field splits into two registers against the same horizontal wave axis, separated by a drawn divider: the unbroken run above it, everything that can start later below it with its track. Position and extent both survive and the page stops being one field.",
      "narrow_composition": "the axis rotates. Waves become rows down the page and the run descends through them, and extent turns with the axis: a later start is measured down the same rows, in a gutter of its own, with a tick at each wave line it crosses. Position still encodes wave; it does not become full-width boxes with a wave number printed on them.",
      "states": [
        { "name": "on the unbroken run", "non_colour_marker": "stroke-width" },
        { "name": "could start later", "non_colour_marker": "stroke-dasharray" },
        { "name": "could not start later", "non_colour_marker": "room-cap" },
        { "name": "nothing open blocks it today", "non_colour_marker": "now-tab" }
      ],
      "carrier": {
        "surface": "cf-present",
        "native_blocks_considered": ["table", "status", "diagram"],
        "verdict": "needs-new-block",
        "loss": "The encoding is position on an axis plus a drawn link plus a length-proportional bar. `table` and `status` keep every fact and lose all three. `diagram` can draw the link but cannot place a task at a computed wave position or size a bar to a quantity. The sandboxed escape block can draw it, but with no media query it cannot switch to either declared alternate composition and cannot follow the document's mode, and with no script it cannot size its own frame — so the escape carries the desktop composition only.",
        "loss_is_material": true
      },
      "motion": "none"
    },
    {
      "id": "b-paths-to-the-sink",
      "case": "p1",
      "primary_unit": "a whole path through the unfinished graph to its sink",
      "primary_axis": "path length in tasks",
      "encoded_relationship": "dominance and overlap: every route to the finish is one object, sorted by how long it is, and a task's exposure is how many routes it lies on and whether the longest is among them",
      "optimises": "n/a",
      "carries_other_by": "n/a",
      "breakpoints": { "wide": 1080, "narrow": 640 },
      "intermediate_composition": "paths keep their full length and sorted order; the task columns they share compress to a narrower shared gutter",
      "narrow_composition": "each path becomes its own row of linked tasks with its length rendered as the row's extent, so the comparison between paths is still a comparison of drawn extents rather than of numbers",
      "states": [
        { "name": "longest route", "non_colour_marker": "stroke-width" },
        { "name": "shorter route", "non_colour_marker": "stroke-dasharray" },
        { "name": "shared task", "non_colour_marker": "share-edge" }
      ],
      "carrier": {
        "surface": "cf-present",
        "native_blocks_considered": ["table", "tree", "diagram"],
        "verdict": "needs-new-block",
        "loss": "`tree` is the closest native shape and is wrong: these routes share tasks, so a tree duplicates them and destroys the overlap that is the point. `diagram` draws a graph but not a set of routes sorted by extent. The escape block draws it, and loses recomposition, mode and self-sizing as above.",
        "loss_is_material": true
      },
      "motion": "none"
    },
    {
      "id": "a-stopped-provenance",
      "case": "p2",
      "primary_unit": "a claim and the rail its evidence travels along",
      "primary_axis": "derivation stage, from what produced the evidence to what accepted it",
      "encoded_relationship": "distance travelled and where it stopped: a filled run up to the stopping node, then a reserved open remainder that is drawn rather than omitted, with each rail head carrying the platform coverage that claim actually has",
      "optimises": "cause of each absence — the stopping point sits at the stage that failed, so the kind of stop is positional",
      "carries_other_by": "an explicit five-platform coverage strip at each rail head, so coverage is read at a glance instead of scanned out of prose",
      "breakpoints": { "wide": 1080, "narrow": 640 },
      "intermediate_composition": "rails keep their horizontal stage axis and the coverage strip moves above the rail head rather than beside it",
      "narrow_composition": "rails rotate to vertical and run down the page; stage position, the stopping node and the reserved remainder all survive as vertical extent, and the coverage strip becomes the rail's head row",
      "states": [
        { "name": "travelled", "non_colour_marker": "stroke-width" },
        { "name": "not travelled", "non_colour_marker": "stroke-dasharray" },
        { "name": "stop", "non_colour_marker": "stroke-linecap" },
        { "name": "no run at all", "non_colour_marker": "url(#p2a-hatch" }
      ],
      "carrier": {
        "surface": "cf-present",
        "native_blocks_considered": ["status", "table", "comparison"],
        "verdict": "needs-new-block",
        "loss": "`status` has exactly the right state vocabulary — pass, fail, pending, not_run — and no geometry at all, so the distance a claim travelled before stopping disappears entirely. A table renders the untravelled remainder as an empty cell, which is the one thing this encoding exists to avoid. The escape block draws it and loses recomposition, mode and self-sizing.",
        "loss_is_material": true
      },
      "motion": "none"
    },
    {
      "id": "b-absence-by-cause",
      "case": "p2",
      "primary_unit": "a single missing piece of evidence",
      "primary_axis": "the kind of thing that caused it to be missing",
      "encoded_relationship": "classification with magnitude: the absences are grouped by cause and sized by how much of the evidence set each cause accounts for, with the executed evidence drawn as the small complement it is",
      "optimises": "cause of each absence — cause is the axis itself",
      "carries_other_by": "each absence keeps its platform as a mark inside its group, so the platform pattern is recoverable by reading down a group",
      "breakpoints": { "wide": 1080, "narrow": 620 },
      "intermediate_composition": "cause groups keep their proportional widths and wrap to two rows rather than compressing to unreadable slivers",
      "narrow_composition": "cause groups become full-width bands stacked down the page whose height is proportional to their share, so magnitude is still drawn and the complement is still visibly small",
      "states": [
        { "name": "a run that happened", "non_colour_marker": "tile-have" },
        { "name": "a place with no run", "non_colour_marker": "tile-gap" },
        { "name": "one block, one cause", "non_colour_marker": "grp-bracket" }
      ],
      "carrier": {
        "surface": "cf-present",
        "native_blocks_considered": ["comparison", "table", "status"],
        "verdict": "needs-new-block",
        "loss": "The magnitude of each cause is drawn as area; no typed block renders a proportion. Flattened into a comparison the encoding becomes three lists and the reader loses the fact that one cause dominates. The escape block draws it and loses recomposition, mode and self-sizing.",
        "loss_is_material": true
      },
      "motion": "none"
    },
    {
      "id": "a-convergence-with-source-depth",
      "case": "p3",
      "primary_unit": "a finding",
      "primary_axis": "review round, ordered",
      "encoded_relationship": "state over an ordered axis with back-reference: a re-opening is drawn as an arc returning to the round it re-opened, and a cause that did not come from a round arrives from a column that is deliberately off the axis, with the exact source region that round changed available as depth inside the finding's own row",
      "optimises": "n/a",
      "carries_other_by": "n/a",
      "breakpoints": { "wide": 1100, "narrow": 680 },
      "intermediate_composition": "the round axis keeps every round column and the off-axis column keeps its position; the row-level source depth moves from beside the row to beneath it",
      "narrow_composition": "rounds become a compressed tick axis under each finding's own name, so a finding keeps its own ordered axis rather than the page keeping one shared axis; the arcs stay drawn within that per-finding axis, and the off-axis arrival keeps a reserved position outside the ticks",
      "states": [
        { "name": "self-caused re-opening", "non_colour_marker": "stroke-dasharray" },
        { "name": "external re-opening", "non_colour_marker": "stroke-width" },
        { "name": "not a round", "non_colour_marker": "url(#p3a-hatch" },
        { "name": "extended, not re-opened", "non_colour_marker": "cell-ext" }
      ],
      "carrier": {
        "surface": "cf-present",
        "native_blocks_considered": ["table", "diagram", "disclosure", "diff"],
        "verdict": "needs-new-block",
        "loss": "The arcs and the off-axis region are the encoding. A table flattens both. A Mermaid state diagram can draw transitions but not against a shared ordered round axis, and cannot reserve a region that is explicitly not on that axis. `disclosure` plus `diff` reproduces the row-level depth faithfully and nothing else. The escape block draws the whole thing and loses recomposition, mode and self-sizing — and this candidate's narrow composition is a different composition, not a scaled one, so that loss is at its most expensive here.",
        "loss_is_material": true
      },
      "motion": "none"
    },
    {
      "id": "b-source-region-history",
      "case": "p3",
      "primary_unit": "a region of source code",
      "primary_axis": "position in the file",
      "encoded_relationship": "accumulation in place: each region carries the rounds that rewrote it as marks in file order, so a region rewritten three times is visibly denser than one settled at the first attempt",
      "optimises": "n/a",
      "carries_other_by": "n/a",
      "breakpoints": { "wide": 1080, "narrow": 660 },
      "intermediate_composition": "the source column narrows and the round marks move from a margin rail to a strip directly above each region's first line",
      "narrow_composition": "each region becomes a card whose head is the round-mark strip and whose body is the quoted lines; file order is preserved as page order and the density comparison survives as strip length",
      "states": [
        { "name": "settled first time", "non_colour_marker": "tick-once" },
        { "name": "rewritten again", "non_colour_marker": "tick-again" },
        { "name": "cause from outside", "non_colour_marker": "tick-outside" }
      ],
      "carrier": {
        "surface": "cf-present",
        "native_blocks_considered": ["diff", "code", "disclosure"],
        "verdict": "escape-with-loss",
        "loss": "`diff` renders the lines faithfully and carries no round attribution at all, so the accumulation — the only thing this candidate encodes — is lost. The escape block can render code with a margin rail, so this is the one candidate the current contract could host at all; it still loses recomposition, mode and self-sizing, which for a code surface means the block cannot follow the document's syntax theme.",
        "loss_is_material": true
      },
      "motion": "none"
    },
    {
      "id": "a-layers-by-change-rate",
      "case": "d1",
      "primary_unit": "a layer of the repository",
      "primary_axis": "how often the layer changes, from rarely to continuously",
      "encoded_relationship": "stratification: the layers are ordered by volatility, and each layer's write authority is a mark on it, so the reader sees that the parts that change most are the parts a human writes least",
      "optimises": "how each kind may change — volatility is the axis",
      "carries_other_by": "each stratum names its own home paths in place, so location is read off the same object",
      "breakpoints": { "wide": 1080, "narrow": 660 },
      "intermediate_composition": "strata keep their proportional bands and their home paths move from a right column into the band itself",
      "narrow_composition": "strata become full-width bands down the page whose height still encodes position on the volatility axis, so the ordering and the spacing between rarely and continuously survive rather than becoming six equal rows",
      "states": [
        { "name": "human-owned", "non_colour_marker": "border-style" },
        { "name": "tool-maintained", "non_colour_marker": "repeating-linear-gradient" },
        { "name": "append-only", "non_colour_marker": "border-top-width" },
        { "name": "generated", "non_colour_marker": "text-decoration" }
      ],
      "carrier": {
        "surface": "docs-portal",
        "verdict": "needs-source-model",
        "loss": "Nothing in the composition is beyond a generator emitter and the portal stylesheet. What is missing is the fact: no frontmatter or portal config records a layer's change rate or its write authority, so the Orient page would need a declared layer model in the source graph before it could be generated rather than hand-written.",
        "loss_is_material": true
      },
      "motion": "none"
    },
    {
      "id": "b-the-spine",
      "case": "d1",
      "primary_unit": "a link in the traceability spine",
      "primary_axis": "direction along the spine, downward from capability to ledger",
      "encoded_relationship": "derivation: one continuous descent that every layer hangs off at the point it enters, so the reader learns that a why-question is answered by walking the spine upward from wherever they are",
      "optimises": "where each kind of knowledge lives — location is position on the spine",
      "carries_other_by": "each hanging layer carries its change discipline as the shape of its attachment, so a layer that only ever grows attaches differently from one a human rewrites",
      "breakpoints": { "wide": 1080, "narrow": 700 },
      "intermediate_composition": "the spine stays vertical and the layers alternate sides instead of all hanging right, so the descent keeps its length rather than compressing",
      "narrow_composition": "the spine stays vertical and full-length and the layers hang beneath their own link rather than beside it, so the descent and the attachment points survive",
      "states": [
        { "name": "on the spine", "non_colour_marker": "rail::before" },
        { "name": "hangs off it", "non_colour_marker": "clasp" },
        { "name": "grows only", "non_colour_marker": "grow-only" }
      ],
      "carrier": {
        "surface": "docs-portal",
        "verdict": "needs-generator-work",
        "loss": "The spine is already declared in the operating contract and the relationships it draws are the ones the portal already derives for its cross-links, so this needs an emitter and stylesheet work and no new source fact.",
        "loss_is_material": false
      },
      "motion": "none"
    },
    {
      "id": "c-write-authority-map",
      "case": "d1",
      "primary_unit": "a real path in the repository",
      "primary_axis": "who may write it",
      "encoded_relationship": "partition with a boundary drawn through it: the paths are grouped by write authority and a single perimeter separates what a contributor edits from what refuses an edit, with the one path whose authority is split shown straddling the perimeter",
      "optimises": "how each kind may change — authority is the axis",
      "carries_other_by": "the units are the paths themselves, so location is not a label on the encoding, it is the encoding",
      "breakpoints": { "wide": 1120, "narrow": 700 },
      "intermediate_composition": "authority groups keep both sides of the perimeter visible and wrap within their side rather than re-cutting into a single column",
      "narrow_composition": "the perimeter becomes a horizontal division the page scrolls through, with every group keeping its side of it and the split path still drawn across the division rather than assigned to one side",
      "states": [
        { "name": "editable", "non_colour_marker": "border-style" },
        { "name": "refuses an edit", "non_colour_marker": "repeating-linear-gradient" },
        { "name": "split authority", "non_colour_marker": "straddle" }
      ],
      "carrier": {
        "surface": "docs-portal",
        "verdict": "needs-source-model",
        "loss": "Same shape as the stratification candidate: the drawing is ordinary generator and CSS work, but write authority per path is not a fact the source graph carries, so it would have to be declared before it could be generated.",
        "loss_is_material": true
      },
      "motion": "none"
    },
    {
      "id": "a-decision-reach",
      "case": "d2",
      "primary_unit": "a claim in the record",
      "primary_axis": "the extent of the record each governing decision reaches",
      "encoded_relationship": "partial supersession as extent: each decision is drawn as a span covering exactly the claims it governs, so a decision that reaches two mechanisms and leaves the rest standing is visibly shorter than the record it is attached to",
      "optimises": "reach of the governing decision",
      "carries_other_by": "each claim keeps a compact evidence mark on its own row, so the evidence state is present without competing with the spans",
      "breakpoints": { "wide": 1080, "narrow": 680 },
      "intermediate_composition": "the spans keep their measured extent and move from a wide left gutter to a narrower one, with decision labels moving above their span",
      "narrow_composition": "spans rotate to run horizontally above the claims they cover, keeping measured extent as width; a decision covering two of eight claims is still visibly a fragment rather than a link",
      "states": [
        { "name": "governs", "non_colour_marker": "stroke-width" },
        { "name": "reaches only a named mechanism", "non_colour_marker": "stroke-dasharray" },
        { "name": "no decision reaches here", "non_colour_marker": "reach-skip" }
      ],
      "carrier": {
        "surface": "docs-portal",
        "verdict": "needs-source-model",
        "loss": "A supersession's scope lives in ADR prose today — the sentence that names which mechanisms it replaces. Drawing reach as extent needs that scope structured against the claims it covers, which is a source-model change on top of the generator work.",
        "loss_is_material": true
      },
      "motion": "none"
    },
    {
      "id": "b-evidence-margin",
      "case": "d2",
      "primary_unit": "a claim in the record",
      "primary_axis": "reading order down the record",
      "encoded_relationship": "adjacency with reserved absence: every claim has an evidence position at its own eye level, and a claim whose evidence does not exist keeps that position, reserved and visibly unfilled, so a deliberate absence cannot be mistaken for a claim nobody got to",
      "optimises": "presence or deliberate absence of evidence",
      "carries_other_by": "each claim carries the id of the decision governing it as a small fixed mark, so authority is present without extent being drawn",
      "breakpoints": { "wide": 1080, "narrow": 680 },
      "intermediate_composition": "the margin narrows but stays a margin — the reserved position remains beside its claim rather than moving under it",
      "narrow_composition": "the margin becomes a reserved band beneath each claim, drawn with its own edge and kept at full height when empty, so absence stays a shape on the page and never becomes a missing row or a blank table cell",
      "states": [
        { "name": "evidence present", "non_colour_marker": "border-style" },
        { "name": "evidence bounded", "non_colour_marker": "dashed var(--bnd)" },
        { "name": "reserved and empty", "non_colour_marker": "repeating-linear-gradient" }
      ],
      "carrier": {
        "surface": "docs-portal",
        "verdict": "needs-source-model",
        "loss": "The adjacency is generator and CSS work. The fact that is missing is a claim-level evidence link: the record carries no structured pointer from a claim to the artefact backing it, and reserving a position for an absence requires the source to say that the absence is deliberate rather than unrecorded.",
        "loss_is_material": true
      },
      "motion": "none"
    }
  ],

  "convergence_hypotheses": [
    {
      "id": "absence-as-a-positive-state",
      "observed_in": ["a-stopped-provenance", "b-absence-by-cause", "a-convergence-with-source-depth", "c-write-authority-map", "b-evidence-margin"],
      "status": "hypothesis",
      "not_promoted_because": [
        "one hand, one sitting: every candidate here was authored by the same session for the same study, which is authorial habit rather than product recurrence",
        "the candidates within a case are mutually exclusive by construction; at most one ships, so nothing has to stay coherent with anything",
        "no accepted CodeFlow product surface repeats the pattern today: the presentation contract's nearest state vocabulary is the status block's not_run, which carries no geometry, and the portal ships no absence treatment at all"
      ],
      "test_before_promotion": "a real reuse need in two accepted surfaces that will coexist, not a fifth candidate agreeing with the first four",
      "must_not": "be extracted into a shared visual primitive, token, or component by this study"
    }
  ]
}
;
