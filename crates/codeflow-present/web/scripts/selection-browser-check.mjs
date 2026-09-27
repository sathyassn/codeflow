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
      root.innerHTML = `<section data-cf-block-id="evidence" data-cf-block-digest="digest">
        <style>/* private-style-text */</style>
        <ul><li style="display:grid;grid-template-columns:150px 250px"><strong>Real service</strong><span>Authenticated loopback journey</span></li></ul>
        <p>Visible context<span hidden>hidden-descendant-text</span></p>
        <p style="visibility:hidden">hidden-block-text</p>
      </section>`;
      const documentText = selectionHarness.captureDocument(root)?.excerptText;
      return { first, second, formatted, astral, spacing, styled, mismatch, ambiguous, documentText };
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
    assert.ok(results.documentText.includes("Real service Authenticated loopback journey"));
    assert.ok(results.documentText.includes("Visible context"));
    for (const hidden of ["private-style-text", "hidden-descendant-text", "hidden-block-text"]) {
      assert.ok(!results.documentText.includes(hidden), `Document excerpt exposed ${hidden}`);
    }
  } finally {
    await context.close();
  }
}

/**
 * The resolver rules of SPC-014 B3, one case each, on a stage built to the
 * TSK-118 DOM contract (the service's entity attributes written by hand) and
 * a figure drawn by the grammar module itself.
 */
export async function checkResolverRules(browser) {
  const { readFile } = await import("node:fs/promises");
  const { renderFigure } = await import("../src/figure-grammar.mjs");
  const compiled = await build({
    entryPoints: [fileURLToPath(new URL("../src/selection.ts", import.meta.url))],
    bundle: true, write: false, format: "iife", globalName: "resolverHarness", platform: "browser",
  });
  const framed = JSON.parse(await readFile(new URL("../../tests/fixtures/contract-v2/documents/v2-framed.json", import.meta.url), "utf8"));
  const landing = framed.blocks.find((block) => block.id === "landing").declaration;
  const drawn = renderFigure(landing, { idPrefix: "cf-present-figure-1", number: 1 });
  const kit = await readFile(new URL("../src/figure.css", import.meta.url), "utf8");
  const tokens = ":root{--cf-fig-font:sans-serif;--cf-fig-mono:monospace;--cf-fig-ground:#fff;--cf-fig-rule:#ccc;--cf-fig-line:#222;--cf-fig-line-mid:#555;--cf-fig-line-soft:#888;--cf-fig-fill:#eee;--cf-fig-hatch:#999;--cf-fig-accent:#1a5fb4;--cf-fig-warn:#b5651d;--cf-fig-stop:#b00020;--cf-fig-na-alpha:0.5}";
  const context = await browser.newContext({ viewport: { width: 1000, height: 1400 } });
  try {
    const page = await context.newPage();
    await page.setContent(`<!doctype html><style>${tokens}${kit}body{margin:0}main{width:900px}svg{display:block}text{font:14px sans-serif}</style>
      <main id="document">${STAGE_FIXTURE}
        <section data-cf-block-id="landing" data-cf-block-label="Two landing paths" data-cf-block-digest="${"b".repeat(64)}">
          <div class="figure-block" data-cf-figure-block="ready" data-cf-figure-number="1"><div data-cf-figure-output>${drawn}</div></div>
        </section>
        <div class="cf-marker-layer"><button class="cf-marker" id="marker">1</button></div>
      </main>`);
    await page.addScriptTag({ content: compiled.outputFiles[0].text });
    const result = await page.evaluate(() => {
      const root = document.getElementById("document");
      const h = resolverHarness;
      const summary = (resolution) => resolution && ({
        via: resolution.via,
        element: resolution.element.id || resolution.element.getAttribute("data-cf-entity") || resolution.element.getAttribute("data-cf-block-id") || resolution.element.localName,
        entity: resolution.entityId ?? null,
        label: resolution.label,
      });
      const at = (id) => summary(h.resolveTarget(root, document.getElementById(id)));
      const byEntity = (block, id) => document.querySelector(`[data-cf-block-id="${block}"] [data-cf-entity="${id}"]:not(.cf-fig-svg--narrow *)`);
      // A point off a thin stroke: the drawing under it is the svg itself.
      const offLine = (element, distance) => {
        const box = element.getBoundingClientRect();
        const x = box.left + box.width / 2;
        const y = box.top + box.height / 2 - distance;
        const under = document.elementFromPoint(x, y);
        return { under: under?.localName, resolved: summary(h.resolveTarget(root, under, { x, y })) };
      };
      const line = document.getElementById("submit-line");
      const m1 = byEntity("landing", "m1");
      const candidates = h.annotatableElements(root);
      const order = (id) => candidates.findIndex((element) => element.id === id || element.getAttribute("data-cf-entity") === id);
      const port = document.getElementById("port");
      const enclosing = [h.enclosingTarget(root, port), h.enclosingTarget(root, document.getElementById("service"))].map(summary);
      return {
        groupText: at("browser-text"),
        groupRect: at("browser-rect"),
        forLabel: at("submit-label"),
        near: offLine(line, 4),
        far: offLine(line, 7),
        plainNear: offLine(document.getElementById("plain-line"), 5),
        innermost: at("port"),
        targetText: at("service-text"),
        labelledGroup: at("node-a-rect"),
        ancestorLabel: at("aria-rect"),
        bareRect: at("bare-rect"),
        noneOnly: at("none-rect"),
        noneInEntity: at("ledger-none-rect"),
        markerPath: at("marker-path"),
        titleNode: at("stage-title"),
        chrome: h.resolveTarget(root, document.getElementById("marker")),
        legend: summary(h.resolveTarget(root, document.querySelector("[data-cf-entity='legend-1']"))),
        frameTitle: at("frame-title"),
        frameTitleTextual: h.isTextualTarget(document.getElementById("frame-title")),
        figureNode: summary(h.resolveTarget(root, byEntity("landing", "m15").firstElementChild)),
        figureLabel: summary(h.resolveTarget(root, [...document.querySelectorAll("[data-cf-block-id='landing'] .cf-fig-svg--wide text")].find((text) => text.textContent === "Human merge"))),
        figureLine: offLine(m1, 5),
        figureLegend: summary(h.resolveTarget(root, document.querySelector("[data-cf-block-id='landing'] li[data-state='human'] svg"))),
        figureTitleLine: summary(h.resolveTarget(root, document.querySelector("[data-cf-block-id='landing'] .cf-fig-title"))),
        figureTitleTextual: h.isTextualTarget(document.querySelector("[data-cf-block-id='landing'] .cf-fig-name")),
        keyboard: {
          order: ["browser", "submit-edge", "service", "service-port", "box"].map(order),
          hidden: order("hidden-rect"),
          narrowHidden: candidates.some((element) => element.closest(".cf-fig-svg--narrow")),
          wideShown: candidates.some((element) => element.getAttribute("data-cf-entity") === "m15"),
          insideGroup: candidates.some((element) => element.id === "browser-rect"),
          insideNone: candidates.some((element) => element.id === "none-rect"),
        },
        enclosing,
        top: h.enclosingTarget(root, document.querySelector("[data-cf-block-id='answer-flow']")),
        computed: ["authored", "named", "aria", "own", "bare"].map((id) => at(`computed-${id}`)),
        captured: h.captureElement(root, document.getElementById("port")),
        capturedFigure: h.captureElement(root, byEntity("landing", "m15")),
        collapse: [h.collapseLabel("  a \n\t b  "), h.collapseLabel("x".repeat(130)).length, [...h.collapseLabel("🧭".repeat(130))].length],
        distance: [h.segmentDistance({ x: 5, y: 3 }, { x: 0, y: 0 }, { x: 10, y: 0 }), h.segmentDistance({ x: 14, y: 3 }, { x: 0, y: 0 }, { x: 10, y: 0 })],
      };
    });
    const entity = (via, id, label, element = id) => ({ via, element, entity: id, label });
    // Step 3: the innermost group, not its parts.
    assert.deepEqual(result.groupText, entity("entity", "browser", "Browser page"));
    assert.deepEqual(result.groupRect, entity("entity", "browser", "Browser page"));
    // Step 2: a data-cf-for label resolves to its first entity.
    assert.deepEqual(result.forLabel, entity("label", "submit-edge", "submit", "submit-line"));
    // Thin strokes: 4 px off a 1.5 px line hits it; 7 px does not.
    assert.equal(result.near.under, "svg");
    assert.deepEqual(result.near.resolved, entity("entity", "submit-edge", "submit", "submit-line"));
    assert.notEqual(result.far.resolved.entity, "submit-edge");
    assert.equal(result.far.resolved.element, "stage");
    assert.equal(result.plainNear.resolved.element, "plain-line", "a plain thin line is a visible shape within 6 px");
    // Targets nest: the innermost wins, select enclosing climbs, then the block.
    assert.deepEqual(result.innermost, entity("entity", "service-port", "Port", "port"));
    assert.deepEqual(result.enclosing, [entity("entity", "service", "Service"), { via: "block", element: "answer-flow", entity: null, label: "How an answer reaches the agent" }]);
    assert.equal(result.top, null);
    assert.deepEqual(result.targetText, entity("entity", "service", "Service"));
    // Step 4: a shape in a labelled group resolves to the group; an explicit
    // ancestor label names a shape; else the block label, never a tag name.
    assert.deepEqual(result.labelledGroup, { via: "shape", element: "node-a", entity: null, label: "Node A" });
    assert.deepEqual(result.ancestorLabel, { via: "shape", element: "aria-rect", entity: null, label: "Answer flow" });
    assert.deepEqual(result.bareRect, { via: "shape", element: "bare-rect", entity: null, label: "How an answer reaches the agent" });
    // none: the nearest target ancestor, else the block.
    assert.deepEqual(result.noneOnly, { via: "block", element: "answer-flow", entity: null, label: "How an answer reaches the agent" });
    assert.deepEqual(result.noneInEntity, entity("entity", "ledger", "Ledger"));
    // Never defs, markers, title or chrome.
    assert.equal(result.markerPath.element, "second-svg");
    assert.equal(result.titleNode.element, "stage");
    assert.equal(result.chrome, null);
    // Framing: the legend entry is an entity; the title line is an element
    // target labelled by its text, never a text selection target.
    assert.deepEqual(result.legend, entity("entity", "legend-1", "a request"));
    assert.deepEqual(result.frameTitle, { via: "shape", element: "frame-title", entity: null, label: "Figure 2 · How an answer reaches the agent" });
    assert.equal(result.frameTitleTextual, false);
    // The drawn figure: marks, labels, thin paths and legend entries.
    assert.deepEqual(result.figureNode, { via: "entity", element: "cf-present-figure-1-landing-paths-wide-m15", entity: "m15", label: "Human merge" });
    assert.deepEqual(result.figureLabel, { via: "label", element: "cf-present-figure-1-landing-paths-wide-m15", entity: "m15", label: "Human merge" });
    assert.equal(result.figureLine.resolved.entity, "m1");
    assert.equal(result.figureLine.resolved.label, "Travel completed");
    assert.deepEqual(result.figureLegend, { via: "entity", element: "legend-human", entity: "legend-human", label: "Human decision" });
    // The label is the line as shown: the kit sets the kicker in capitals.
    assert.equal(result.figureTitleLine.label, "Figure 1 · Two landing paths from a task branch to main TWO LANDING PATHS");
    assert.equal(result.figureTitleTextual, false);
    // Keyboard: entities in document order; hidden, grouped and none parts are no stops.
    assert.ok(result.keyboard.order.every((index, position, all) => index >= 0 && (position === 0 || index > all[position - 1])), JSON.stringify(result.keyboard));
    assert.equal(result.keyboard.hidden, -1);
    assert.equal(result.keyboard.narrowHidden, false);
    assert.equal(result.keyboard.wideShown, true);
    assert.equal(result.keyboard.insideGroup, false);
    assert.equal(result.keyboard.insideNone, false);
    // B2 label order when the service wrote no label.
    assert.deepEqual(result.computed.map((item) => item.label), ["Authored label", "sub mit", "Aria name", "Own words", "computed-bare"]);
    // The capture: element path plus the entity selector (no crop yet).
    assert.deepEqual(result.captured.entity_selector, { entity_id: "service-port", label: "Port", block_digest: "a".repeat(64) });
    assert.equal(result.captured.element_selector.tag_name, "rect");
    assert.equal(result.captured.summary, "Element: Port");
    assert.deepEqual(result.capturedFigure.entity_selector, { entity_id: "m15", label: "Human merge", block_digest: "b".repeat(64), variant: "wide" });
    assert.deepEqual(result.collapse, ["a b", 120, 120]);
    assert.deepEqual(result.distance, [3, 5]);
    process.stdout.write("resolver rules passed: label, group, innermost target, none, thin stroke at 4 and 7 px, labelled group, block label, exclusions, framing, figure marks, keyboard order, select enclosing, B2 label order\n");
  } finally {
    await context.close();
  }
}

// The answer-flow stage of the contract-v2 fixture, with the attributes the
// service adds while scoping (data-cf-entity, data-cf-entity-label,
// data-cf-entity-none) written by hand, plus the cases the fixture lacks.
const STAGE_FIXTURE = `<section data-cf-block-id="answer-flow" data-cf-block-label="How an answer reaches the agent" data-cf-block-digest="${"a".repeat(64)}">
  <figure class="cf-frame" data-cf-frame="figure" data-cf-number="2">
    <p class="cf-frame-title" id="frame-title"><span class="cf-frame-number">Figure 2</span> · <span class="cf-frame-name">How an answer reaches the agent</span></p>
    <div data-cf-review-text-root data-cf-canonical-text="Page submit Service Ledger">
      <div class="cf-stage-host">
        <svg id="stage" viewBox="0 0 400 160" width="400" height="160" aria-label="Answer flow">
          <title id="stage-title">Answer flow</title>
          <g id="browser" data-cf-group="browser" data-cf-label="Browser page" data-cf-entity="browser" data-cf-entity-label="Browser page"><rect id="browser-rect" x="10" y="30" width="100" height="50" fill="#ddd" stroke="#333"/><text id="browser-text" x="40" y="60">Page</text></g>
          <line id="submit-line" data-cf-target="submit-edge" data-cf-entity="submit-edge" data-cf-entity-label="submit" x1="110" y1="55" x2="190" y2="55" stroke="#333" stroke-width="1.5"/>
          <text id="submit-label" data-cf-for="submit-edge" x="128" y="40">submit</text>
          <g id="service" data-cf-target="service" data-cf-entity="service" data-cf-entity-label="Service"><rect x="190" y="30" width="100" height="50" fill="#eee" stroke="#333"/><text id="service-text" x="215" y="60">Service</text><rect id="port" data-cf-target="service-port" data-cf-entity="service-port" data-cf-entity-label="Port" x="276" y="48" width="14" height="14" fill="#999"/></g>
          <rect id="box" data-cf-target="box" data-cf-entity="box" data-cf-entity-label="box" x="300" y="100" width="80" height="40" fill="#ccc"/>
          <g id="node-a" aria-label="Node A"><rect id="node-a-rect" x="10" y="100" width="40" height="40" fill="#bbb"/></g>
          <rect id="aria-rect" x="60" y="100" width="30" height="40" fill="#bbb"/>
          <g data-cf-target="none" data-cf-entity-none><rect id="none-rect" x="100" y="100" width="30" height="40" fill="#aaa"/></g>
          <g data-cf-target="ledger" data-cf-entity="ledger" data-cf-entity-label="Ledger"><rect x="140" y="95" width="60" height="50" fill="#ddd"/><g data-cf-target="none" data-cf-entity-none><rect id="ledger-none-rect" x="150" y="105" width="20" height="20" fill="#888"/></g></g>
          <line id="plain-line" x1="210" y1="120" x2="290" y2="120" stroke="#333" stroke-width="1"/>
          <rect id="hidden-rect" data-cf-target="hidden-rect" data-cf-entity="hidden-rect" data-cf-entity-label="Hidden" x="0" y="0" width="5" height="5" style="display:none"/>
        </svg>
        <svg id="second-svg" viewBox="0 0 200 60" width="200" height="60">
          <defs><marker id="arrow" markerWidth="8" markerHeight="8" refX="8" refY="4" orient="auto"><path id="marker-path" d="M0 0L8 4L0 8Z"/></marker></defs>
          <rect id="bare-rect" x="10" y="10" width="40" height="40" fill="#bbb"/>
          <rect id="computed-authored" data-cf-target="computed-authored" data-cf-label="  Authored
            label " x="60" y="10" width="10" height="10"/>
          <rect id="computed-named" data-cf-target="computed-named" x="80" y="10" width="10" height="10"/>
          <text data-cf-for="computed-named" x="80" y="40">sub</text><text data-cf-for="other computed-named" x="100" y="40">mit</text>
          <rect id="computed-aria" data-cf-target="computed-aria" aria-label="Aria name" x="120" y="10" width="10" height="10"/>
          <g id="computed-own" data-cf-target="computed-own"><text x="140" y="20">Own words</text></g>
          <rect id="computed-bare" data-cf-target="computed-bare" x="180" y="10" width="10" height="10"/>
        </svg>
      </div>
    </div>
    <ul class="cf-legend" aria-label="Legend"><li data-cf-entity="legend-1" data-cf-entity-label="a request">Solid line: a request</li></ul>
    <figcaption class="cf-frame-caption">An answer goes from the page to the service and is appended to the ledger before the agent reads it.</figcaption>
  </figure>
</section>`;
