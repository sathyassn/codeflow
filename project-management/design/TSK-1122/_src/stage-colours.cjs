// Keep the approved 55% mix until the rendered probe establishes a feasible replacement.
const STAGE_LINE_PERCENT = 55;
const STAGE_LINE_FILL = `color-mix(in srgb,var(--accent) ${STAGE_LINE_PERCENT}%,var(--surface))`;
const STAGE_SEPARATION_FLOOR = 15; // Euclidean OKLab distance ×100, as in the kit's dataviz validator.

module.exports = { STAGE_LINE_PERCENT, STAGE_LINE_FILL, STAGE_SEPARATION_FLOOR };
