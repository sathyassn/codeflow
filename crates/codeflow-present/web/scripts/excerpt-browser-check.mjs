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
