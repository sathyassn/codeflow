import assert from "node:assert/strict";
import { build } from "esbuild";
import { fileURLToPath } from "node:url";

/** Exercise real DOM ranges: selecting a repeated phrase must retain its occurrence. */
export async function checkSelectionOccurrences(browser) {
  const compiled = await build({
    entryPoints: [fileURLToPath(new URL("../src/selection.ts", import.meta.url))],
    bundle: true,
    write: false,
    format: "iife",
    globalName: "selectionHarness",
    platform: "browser",
  });
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    await page.setContent('<main id="document"></main>');
    await page.addScriptTag({ content: compiled.outputFiles[0].text });
    const results = await page.evaluate(() => {
      const root = document.getElementById("document");
      function capture(html, canonical, selector, start, end) {
        root.innerHTML = '<section data-cf-block-id="review" data-cf-block-label="Review"><div data-cf-review-text-root></div></section>';
        const content = root.querySelector("[data-cf-review-text-root]");
        content.innerHTML = html;
        content.setAttribute("data-cf-canonical-text", canonical);
        const node = content.querySelector(selector).firstChild;
        const range = document.createRange();
        range.setStart(node, start);
        range.setEnd(node, end);
        getSelection().removeAllRanges();
        getSelection().addRange(range);
        return selectionHarness.captureSelection(root)?.selector ?? null;
      }
      const repeated = "First passed. Second passed.";
      const first = capture(`<p>${repeated}</p>`, repeated, "p", 6, 12);
      const second = capture(`<p>${repeated}</p>`, repeated, "p", 21, 27);
      const unicode = "🧭 passed. 🧭 passed.";
      const formatted = capture("<p>🧭 <strong>passed</strong>. 🧭 <em>passed</em>.</p>", unicode, "em", 0, 6);
      const astral = capture(`<p>${unicode}</p>`, unicode, "p", 11, 13);
      const spacing = capture("<svg><text>passed</text><text id='second'>passed</text></svg>", "Stage title passed passed", "#second", 0, 6);
      const styled = capture("<style>@scope (#host) { p { color: red } }</style><p>First passed. Second <em>passed</em>.</p>", repeated, "em", 0, 6);
      const mismatch = capture("<p>First passed. Second passed.</p>", "First passed. Unrelated passed.", "p", 21, 27);
      const ambiguous = capture("<p>passed</p>", "passed passed", "p", 0, 6);
      return { first, second, formatted, astral, spacing, styled, mismatch, ambiguous };
    });
    assert.equal(results.first.start_utf16, 6);
    assert.equal(results.second.start_utf16, 21);
    assert.equal(results.second.end_utf16, 27);
    assert.equal(results.second.exact, "passed");
    assert.equal(results.second.prefix, "First passed. Second ");
    assert.equal(results.second.suffix, ".");
    assert.equal(results.formatted.start_utf16, 14);
    assert.equal(results.formatted.exact, "passed");
    assert.equal(results.astral.start_utf16, 11);
    assert.equal(results.astral.end_utf16, 13);
    assert.equal(results.astral.exact, "🧭");
    assert.equal(results.spacing.start_utf16, 19);
    assert.equal(results.spacing.exact, "passed");
    assert.equal(results.styled.start_utf16, 21);
    assert.equal(results.styled.exact, "passed");
    assert.equal(results.mismatch, null, "Substantive canonical mismatch must not guess an occurrence");
    assert.equal(results.ambiguous, null, "Ambiguous canonical mapping must not guess an occurrence");
  } finally {
    await context.close();
  }
}
