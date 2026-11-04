// No mix alone meets both floors (75% fails separation, 65% fails contrast on the fill), so the coord/catalog Coordinator chose
// a 65% fill for separation and a 1 px accent outline for the 3:1 boundary, on 2026-11-03, after the operator's approval.
const STAGE_LINE_PERCENT = 65;
const STAGE_LINE_FILL = `color-mix(in srgb,var(--accent) ${STAGE_LINE_PERCENT}%,var(--surface))`;
const STAGE_SEPARATION_FLOOR = 15; // Euclidean OKLab distance ×100, as in the kit's dataviz validator.

module.exports = { STAGE_LINE_PERCENT, STAGE_LINE_FILL, STAGE_SEPARATION_FLOOR };
