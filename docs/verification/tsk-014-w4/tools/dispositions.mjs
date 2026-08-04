// Builds dispositions.json from ../tsk-014-w3/registry.json so every `before`
// statement is verbatim rather than retyped. W3 is read only, never written.
import fs from 'node:fs';
import path from 'node:path';

const ROOT = path.resolve(import.meta.dirname, '..');
const W3 = path.resolve(ROOT, '..', 'tsk-014-w3');
const w3 = JSON.parse(fs.readFileSync(path.join(W3, 'registry.json'), 'utf8'));

// Keyed by the W3 render name. Written before the repairs were authored.
const PLAN = {
  'p3-a-convergence-with-source-depth-light-mobile': {
    disposition: 'repaired',
    kind: 'mechanical',
    now: 'cases/p3/a-convergence-with-source-depth',
    what_changed:
      "The off-axis arrival column is given its own measured width and the two identifying labels are laid out inside it, so neither overshoots the viewBox and neither sits on the hatched region's boundary.",
  },
  'p3-a-convergence-with-source-depth-dark-mobile': {
    disposition: 'repaired',
    kind: 'mechanical',
    now: 'cases/p3/a-convergence-with-source-depth',
    what_changed: 'As the light mobile band; the geometry is mode-independent.',
  },
  'p3-b-source-region-history-light-desktop': {
    disposition: 'repaired',
    kind: 'mechanical',
    now: 'cases/p3/b-source-region-history',
    what_changed:
      'The rail viewBox is extended to include the tick labels\' descenders, so the round axis this candidate is read against is no longer clipped.',
  },
  'p3-b-source-region-history-dark-desktop': {
    disposition: 'repaired',
    kind: 'mechanical',
    now: 'cases/p3/b-source-region-history',
    what_changed: 'As the light desktop band; the geometry is mode-independent.',
  },
  'p3-b-source-region-history-light-tablet': {
    disposition: 'repaired',
    kind: 'design decision',
    now: 'cases/p3/b-source-region-history',
    what_changed:
      'The quoted source no longer sits in a fixed-width column with hidden overflow. Quoted lines wrap with a hanging indent that keeps the continuation visibly subordinate to its own line, so every verified character is on the page at every width. The round-mark strip and the file-order reading are unchanged.',
    why_this_and_not_withdrawal:
      "Withdrawing the band would remove the one candidate whose carrier verdict is not `needs-new-block`, and the defect was never the encoding — it was that the page cropped the line its own A6 contract verifies character for character.",
  },
  'p3-b-source-region-history-dark-tablet': {
    disposition: 'repaired',
    kind: 'design decision',
    now: 'cases/p3/b-source-region-history',
    what_changed: 'As the light tablet band; the geometry is mode-independent.',
  },
  'p3-b-source-region-history-light-mobile': {
    disposition: 'repaired',
    kind: 'design decision',
    now: 'cases/p3/b-source-region-history',
    what_changed:
      'As the tablet band, and it matters more here: the W3 render hid up to 430px of a 332px container with no ellipsis, fade or affordance in a still frame. Wrapping removes the crop rather than signposting it. The file-level round rail is bounded to the figure width instead of running off the right edge.',
  },
  'p3-b-source-region-history-dark-mobile': {
    disposition: 'repaired',
    kind: 'design decision',
    now: 'cases/p3/b-source-region-history',
    what_changed: 'As the light mobile band; the geometry is mode-independent.',
  },
  'p2-baseline-light-mobile': {
    disposition: 'repaired',
    kind: 'mechanical, but constrained',
    now: 'baselines/p2',
    what_changed:
      'The plain control wraps within 390px instead of overflowing it. Nothing else changed: no colour was added, no geometry, no figure, and the same stylesheet still serves all five baselines. A control that becomes a design stops being a control.',
  },
  'd1-baseline-light-mobile': {
    disposition: 'repaired',
    kind: 'mechanical, but constrained',
    now: 'baselines/d1',
    what_changed: 'As the p2 baseline, and under the same constraint.',
  },
  'p1-a-critical-path-and-room-light-mobile': {
    disposition: 'replaced',
    kind: 'design decision',
    now: 'cases/p1/a-critical-path-and-room',
    what_changed:
      "The narrow composition is replaced, not adjusted, and was declared in registry.json before it was drawn. Extent no longer rotates with the axis into a shared gutter. Each task's later start is measured horizontally inside that task's own row against one shared wave scale drawn at the head of the figure. Two tasks can no longer occupy one column because no two rows are the same row, and a zero-room cap sits against its own card instead of floating below the last row.",
    why_replacement_not_repair:
      'The defect was the declared geometry, not its execution. A shared gutter measured down the page collides whenever two tasks have overlapping room, which is the normal case, so separating the marks inside it would have preserved a premise that fails on the next dataset.',
  },
  'p1-a-critical-path-and-room-dark-mobile': {
    disposition: 'replaced',
    kind: 'design decision',
    now: 'cases/p1/a-critical-path-and-room',
    what_changed: 'As the light mobile band; the geometry is mode-independent.',
  },
  'p2-a-stopped-provenance-light-mobile': {
    disposition: 'replaced',
    kind: 'design decision',
    now: 'cases/p2/a-stopped-provenance',
    what_changed:
      "The narrow composition is replaced and was declared before drawing. Each claim becomes a bounded lane with its own edge; the rail runs down inside that lane, inset from both edges so a stop can never span the figure's width; the stop is a cross-tick on the rail rather than a rule; and the cause caption sits inside the same lane, indented to the rail, so it can no longer abut the next claim's title.",
    why_replacement_not_repair:
      "A stop drawn as a full-width rule reads as a section divider wherever the rail sits, and moving the caption alone would leave the stop mark still spanning the figure. P2's registered question is attribution, and attribution is what the band lost.",
  },
  'p2-a-stopped-provenance-dark-mobile': {
    disposition: 'replaced',
    kind: 'design decision',
    now: 'cases/p2/a-stopped-provenance',
    what_changed: 'As the light mobile band; the geometry is mode-independent.',
  },
};

const entries = w3.invalidated_bands.map((band) => {
  const plan = PLAN[band.render];
  if (!plan) throw new Error(`no disposition planned for ${band.render}`);
  return {
    w3_render: band.render,
    w3_band: band.band,
    before: {
      evidence_status: band.evidence_status,
      failure_kind: band.failure.kind,
      failure_severity: band.failure.severity,
      failing_axes: band.failure.failing_axes,
      what_failed_verbatim: band.failure.what_fails,
      machine_visible: band.machine_visible,
      found_by: band.found_by,
    },
    after: plan,
    w3_untouched:
      '../tsk-014-w3 is append-only evidence and is not edited by this study. The W3 render, its registration and its failure record stay exactly as the audit left them.',
  };
});

const out = {
  study: 'tsk-014-w4',
  source_of_truth_for_before: '../tsk-014-w3/registry.json → invalidated_bands',
  note:
    'One entry per W3 invalidated band. `before` is copied verbatim from the W3 registry so the audit history cannot drift; `after` is the disposition this study authored. A11 fails when a band has no entry, or when an entry names a W4 artefact that does not exist.',
  independent_recheck:
    'checks/w3-recheck.json re-measured all fourteen W3 bands with a corrected containment probe (client rects on both sides, in CSS pixels; the W3 harness compared getBBox against viewBox.baseVal, and an intermediate formulation in this study compared through getCTM — both mix coordinate systems). Under the corrected probe every one of the ten machine-visible W3 findings is confirmed and none is an artefact. The remaining four are the p1-a and p2-a mobile bands, which W3 itself recorded as machine_visible:false composition failures; the probe agrees they are inside their frames, which is what W3 said.',
  counts: {
    total: entries.length,
    repaired: entries.filter((e) => e.after.disposition === 'repaired').length,
    replaced: entries.filter((e) => e.after.disposition === 'replaced').length,
    withdrawn: entries.filter((e) => e.after.disposition === 'withdrawn').length,
  },
  entries,
};

fs.writeFileSync(path.join(ROOT, 'dispositions.json'), `${JSON.stringify(out, null, 2)}\n`);
console.log(`dispositions: ${JSON.stringify(out.counts)}`);
