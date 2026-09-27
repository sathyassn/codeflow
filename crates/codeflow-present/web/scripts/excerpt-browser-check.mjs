import assert from "node:assert/strict";
import { build } from "esbuild";
import { fileURLToPath } from "node:url";

/** Pixel evidence must include the selected extent, not merely be nonblank. */
export async function checkDocumentExcerpts(browser) {
  const compiled = await build({
    entryPoints: [fileURLToPath(new URL("../src/excerpt.ts", import.meta.url))],
    bundle: true, write: false, format: "iife", globalName: "excerptHarness", platform: "browser",
  });
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    await page.setContent(`<style>body{margin:0}#document{width:400px;background:white}section{height:100px}p{margin:0}svg{display:block}</style>
      <main id="document">
        <section data-cf-block-id="first"><p>First revision</p></section>
        <section data-cf-block-id="middle"><svg width="400" height="100"><rect width="400" height="100" fill="rgb(0,160,0)"/></svg></section>
        <section data-cf-block-id="last" style="background:rgb(0,0,220);color:white"><p>Last evidence</p></section>
      </main>`);
    await page.addScriptTag({ content: compiled.outputFiles[0].text });
    const labels = await page.evaluate(() => {
      const fixture = document.createElement("div");
      fixture.innerHTML = '<svg><text y="20"><tspan>Input</tspan></text><text y="40"><tspan>Input</tspan></text><text y="60">Review <tspan>and </tspan><tspan>evidence</tspan></text><tspan>Standalone</tspan></svg>';
      document.body.append(fixture);
      const svg = fixture.firstElementChild;
      const range = document.createRange();
      range.selectNodeContents(svg);
      const mixed = svg.querySelectorAll("text")[2];
      const result = {
        element: excerptHarness.visibleTextOf(svg),
        range: excerptHarness.quoteFromRange(range),
        mixed: excerptHarness.visibleTextOf(mixed),
        region: excerptHarness.intersectingVisibleText(fixture, fixture.getBoundingClientRect()),
      };
      fixture.remove();
      return result;
    });
    assert.equal(labels.element, "Input Input Review and evidence Standalone");
    assert.equal(labels.range, labels.element);
    assert.equal(labels.mixed, "Review and evidence");
    assert.ok(labels.region.startsWith("Input Input Review and evidence"), labels.region);
    const pixels = await page.evaluate(async () => {
      const root = document.getElementById("document");
      const box = root.getBoundingClientRect();
      async function sample(rect, points) {
        const excerpt = await excerptHarness.captureRectJpeg(root, rect);
        if (!excerpt) throw new Error("Expected a bounded image excerpt");
        const image = new Image();
        image.src = `data:image/jpeg;base64,${excerpt.data_base64}`;
        await image.decode();
        const canvas = document.createElement("canvas");
        canvas.width = image.width;
        canvas.height = image.height;
        const ctx = canvas.getContext("2d");
        ctx.drawImage(image, 0, 0);
        return points.map(([x, y]) => [...ctx.getImageData(x, y, 1, 1).data].slice(0, 3));
      }
      return {
        whole: await sample(box, [[200, 150], [200, 250]]),
        crossBlock: await sample(new DOMRect(box.left, box.top + 100, 400, 200), [[200, 50], [200, 150]]),
      };
    });
    for (const [label, [green, blue]] of Object.entries(pixels)) {
      assert.ok(green[1] > 130 && green[0] < 30 && green[2] < 30, `${label} lost the middle SVG surface: ${green}`);
      assert.ok(blue[2] > 190 && blue[0] < 30 && blue[1] < 30, `${label} lost the last block: ${blue}`);
    }
  } finally {
    await context.close();
  }
}

/**
 * Entity crops (SPC-014 B4): a PNG whose pixel size is the entity's bounds
 * plus the thin-stroke padding within 2 px, at 1, 0.5 and 2 px per user unit;
 * never one colour; its user-space box inside the entity widened by the
 * service's 8 unit tolerance. Controls: a crop of the wrong element fails the
 * size rule and a crop of empty ground is refused.
 */
export async function checkEntityCrops(browser) {
  const compiled = await build({
    entryPoints: [fileURLToPath(new URL("../src/excerpt.ts", import.meta.url))],
    bundle: true, write: false, format: "iife", globalName: "cropHarness", platform: "browser",
  });
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    // One drawing at three scales: 200 x 100 user units shown at 200, 100 and 400 px wide.
    const drawing = (id, width) => `<svg id="${id}" viewBox="0 0 200 100" width="${width}" height="${width / 2}" style="display:block;background:#fff">
      <rect class="node" x="20" y="20" width="60" height="40" fill="#1a5fb4"/>
      <line class="edge" x1="90" y1="40" x2="170" y2="40" stroke="#222" stroke-width="1.5"/>
    </svg>`;
    await page.setContent(`<style>body{margin:0;background:#fff}</style><main id="document">${drawing("one", 200)}${drawing("half", 100)}${drawing("double", 400)}</main>`);
    await page.addScriptTag({ content: compiled.outputFiles[0].text });
    const cells = await page.evaluate(async () => {
      const root = document.getElementById("document");
      const h = cropHarness;
      async function crop(svg, element) {
        const rect = element.getBoundingClientRect();
        const padding = h.entityCropPadding(svg, 6);
        const box = h.paddedRect(rect, padding);
        const image = await h.captureRectJpeg(root, box, svg, { png: true });
        if (!image) return { image: null };
        const decoded = new Image();
        decoded.src = `data:${image.media_type};base64,${image.data_base64}`;
        await decoded.decode();
        return {
          image: { media_type: image.media_type, width: decoded.width, height: decoded.height },
          expected: { width: rect.width + 2 * padding, height: rect.height + 2 * padding },
          userBox: h.userSpaceBox(svg, box),
        };
      }
      const out = {};
      for (const id of ["one", "half", "double"]) {
        const svg = document.getElementById(id);
        out[id] = { node: await crop(svg, svg.querySelector(".node")), edge: await crop(svg, svg.querySelector(".edge")) };
      }
      // Controls: the whole drawing is the wrong size for the node; empty ground is one colour.
      const one = document.getElementById("one");
      out.wrong = await crop(one, one);
      const blank = one.getBoundingClientRect();
      out.blank = await h.captureRectJpeg(root, new DOMRect(blank.left + 2, blank.top + 70, 60, 25), one, { png: true });
      return out;
    });
    const bounds = { node: { x: 20, y: 20, width: 60, height: 40 }, edge: { x: 90, y: 40, width: 80, height: 0 } };
    const sizeMatches = (cell) => Math.abs(cell.image.width - cell.expected.width) <= 2 && Math.abs(cell.image.height - cell.expected.height) <= 2;
    for (const scale of ["one", "half", "double"]) {
      for (const part of ["node", "edge"]) {
        const cell = cells[scale][part];
        const label = `${scale} ${part}`;
        assert.ok(cell.image, `${label}: no crop`);
        assert.equal(cell.image.media_type, "image/png", label);
        assert.ok(sizeMatches(cell), `${label}: ${JSON.stringify(cell)}`);
        const entity = bounds[part];
        const box = cell.userBox;
        assert.ok(box.x >= entity.x - 8 && box.y >= entity.y - 8
          && box.x + box.width <= entity.x + entity.width + 8
          && box.y + box.height <= entity.y + entity.height + 8, `${label}: ${JSON.stringify(box)} is outside the tolerance`);
      }
    }
    assert.ok(cells.wrong.image && !sizeMatches({ ...cells.wrong, expected: cells.one.node.expected }), "the wrong-element control passed the size rule");
    assert.equal(cells.blank, null, "a crop of empty ground was not refused");
    process.stdout.write("entity crops passed: PNG at 1, 0.5 and 2 px per unit within 2 px, inside the 8 unit tolerance, wrong-size and blank controls\n");
  } finally {
    await context.close();
  }
}
