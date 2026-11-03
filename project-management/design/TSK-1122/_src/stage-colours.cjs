// The Coordinator selected 75% after the rendered probe; rebuilt-page verification is still required.
const STAGE_LINE_PERCENT = 75;
const STAGE_LINE_FILL = `color-mix(in srgb,var(--accent) ${STAGE_LINE_PERCENT}%,var(--surface))`;
const STAGE_SEPARATION_FLOOR = 15; // Euclidean OKLab distance ×100, as in the kit's dataviz validator.

module.exports = { STAGE_LINE_PERCENT, STAGE_LINE_FILL, STAGE_SEPARATION_FLOOR };
