import assert from "node:assert/strict";
import test from "node:test";
import { PAIRS, WIDTHS, runDesignKitCheck } from "./design-kit-check.mjs";

// The rows every run must report, so a control that silently stops being
// driven fails here instead of vanishing from the output.
const COMMON = ["prepaint", "no-hscroll", "display.open", "display.font", "display.size", "display.palette", "display.appearance", "display.selected-pill-border", "display.arrow-keys", "display.esc", "display.remembered", "console-errors"];
const figureRows = (count, prefix = "") => Array.from({ length: count }, (_, index) => ["variant", "min-text", "clearance", "legend", "title-desc-twin"].map((check) => `${prefix}fig${index + 1}.${check}`)).flat();
const PORTAL = [...COMMON, ...figureRows(6), "figures.count", "search.click", "search.esc", "search.slash", "search.ctrl-k", "search.cmd-k", "search.results", "search.one-row-per-section", "search.chip-micro", "search.arrow-keys", "search.enter-follows", "tabs.click-writes-hash", "tabs.arrow-keys", "tabs.back-forward", "tabs.underline", "toc.built", "toc.scroll-spy", "toc.soft-fill", "crumbs", "crumbs.route-seam", "provenance", "nav.group-collapse", "nav.collapse", "nav.peek-hover", "nav.peek-bracket", "toc.collapse", "toc.peek-bracket", "preview.hover", "preview.focus-esc", "footer.cards"];
const PRESENT = [...COMMON, "topbar", "rail.hidden-when-off", "route", ...figureRows(1), "comment.arm-click", "comment.key-c", "verdict.default", ...figureRows(1, "armed."), "gesture.text-float", "composer.open", "composer.cmd-enter-saves", "verdict.first-note", "gesture.element", "gesture.area", "markers.numbered", "markers.survive-resize", "rail.notes", "rail.earlier-feedback", "tools.add-selected-text", "verdict.request-changes", "submit", "esc-ladder"];
const EXPECTED = {
  "portal 1280": { rows: [...PORTAL, "nav.peek-keyboard", "nav.restore", "toc.peek-hover", "grip.drag-clamp", "grip.keyboard", "grip.reset"], notApplicable: [] },
  "portal 375": { rows: [...PORTAL, "nav.sheet", "grip"], notApplicable: ["nav.collapse", "nav.peek-hover", "toc.collapse", "grip"] },
  "present 1280": { rows: PRESENT, notApplicable: [] },
  "present 375": { rows: [...PRESENT, "no-hscroll.armed"], notApplicable: ["route"] },
};

test("both reference pages pass the kit checklist in every skin and mode", { skip: process.platform === "win32", timeout: 900_000 }, async () => {
  const report = await runDesignKitCheck({ log: () => {} });
  assert.deepEqual(report.failures, [], `kit check failures:\n${report.failures.join("\n")}`);
  assert.equal(report.consoleErrors, 0);
  assert.equal(report.runs.length, PAIRS.length * WIDTHS.length * 2);
  for (const run of report.runs) {
    const label = `${run.page} ${run.skin}-${run.theme} ${run.width}px`;
    const expected = EXPECTED[`${run.page} ${run.width}`];
    assert.deepEqual(run.rows.map((row) => row.name).sort(), [...expected.rows].sort(), `${label} reported a different control set`);
    const skipped = run.rows.filter((row) => row.ok === "n/a").map((row) => row.name).sort();
    assert.deepEqual(skipped, [...expected.notApplicable].sort(), `${label} skipped a control that applies at this width`);
  }
  assert.equal(report.motion.length, 2 * WIDTHS.length);
  assert.ok(report.motion.every((probe) => probe.ok && probe.matched), "reduced motion must leave no transition, animation or loop");
});
