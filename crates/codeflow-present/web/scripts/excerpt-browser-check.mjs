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
    // A drawing whose viewBox starts at -100,0 and is letterboxed into a
    // square, and one stretched to 2 px per unit across and 0.5 down (T118-2,
    // T118-3). Each crop of the red node must carry red. The blue node sits
    // where a crop that ignored the origin and the letterbox would land.
    const placed = `<svg id="placed" viewBox="-100 0 200 100" width="400" height="400" style="display:block;background:#fff">
      <rect class="node" x="-80" y="20" width="60" height="40" fill="#e01b24"/>
      <rect x="0" y="20" width="100" height="60" fill="#1a5fb4"/>
    </svg>`;
    const stretched = `<svg id="stretched" viewBox="0 0 200 100" width="400" height="50" preserveAspectRatio="none" style="display:block;background:#fff">
      <rect class="node" x="20" y="20" width="60" height="40" fill="#e01b24"/>
      <rect x="120" y="20" width="60" height="40" fill="#1a5fb4"/>
    </svg>`;
    await page.setContent(`<style>body{margin:0;background:#fff}</style><main id="document">${drawing("one", 200)}${drawing("half", 100)}${drawing("double", 400)}${placed}${stretched}</main>`);
    await page.addScriptTag({ content: compiled.outputFiles[0].text });
    const cells = await page.evaluate(async () => {
      const root = document.getElementById("document");
      const h = cropHarness;
      async function crop(svg, element, sampleCentre = false) {
        const rect = element.getBoundingClientRect();
        const padding = h.entityCropPadding(svg, 6);
        const box = h.paddedRect(rect, padding);
        const image = await h.captureRectJpeg(root, box, svg, { png: true });
        if (!image) return { image: null };
        const decoded = new Image();
        decoded.src = `data:${image.media_type};base64,${image.data_base64}`;
        await decoded.decode();
        let centre = null;
        if (sampleCentre) {
          const canvas = document.createElement("canvas");
          canvas.width = decoded.width;
          canvas.height = decoded.height;
          const context = canvas.getContext("2d");
          context.drawImage(decoded, 0, 0);
          centre = [...context.getImageData(Math.floor(decoded.width / 2), Math.floor(decoded.height / 2), 1, 1).data.slice(0, 3)];
        }
        return {
          centre,
          image: { media_type: image.media_type, width: decoded.width, height: decoded.height },
          expected: { width: rect.width + 2 * padding.x, height: rect.height + 2 * padding.y },
          userBox: h.userSpaceBox(svg, box),
        };
      }
      const out = {};
      for (const id of ["one", "half", "double"]) {
        const svg = document.getElementById(id);
        out[id] = { node: await crop(svg, svg.querySelector(".node")), edge: await crop(svg, svg.querySelector(".edge")) };
      }
      for (const id of ["placed", "stretched"]) {
        const svg = document.getElementById(id);
        out[id] = { node: await crop(svg, svg.querySelector(".node"), true) };
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
    const placedBounds = { placed: { x: -80, y: 20, width: 60, height: 40 }, stretched: bounds.node };
    for (const id of ["placed", "stretched"]) {
      const cell = cells[id].node;
      assert.ok(cell.image && sizeMatches(cell), `${id}: ${JSON.stringify(cell)}`);
      const [red, green, blue] = cell.centre;
      assert.ok(red > 190 && green < 60 && blue < 60, `${id}: the crop carries ${cell.centre}, not the red node`);
      const entity = placedBounds[id];
      const box = cell.userBox;
      assert.ok(box.x >= entity.x - 8 && box.y >= entity.y - 8
        && box.x + box.width <= entity.x + entity.width + 8
        && box.y + box.height <= entity.y + entity.height + 8, `${id}: ${JSON.stringify(box)} is outside the tolerance`);
    }
    assert.ok(cells.wrong.image && !sizeMatches({ ...cells.wrong, expected: cells.one.node.expected }), "the wrong-element control passed the size rule");
    assert.equal(cells.blank, null, "a crop of empty ground was not refused");
    process.stdout.write("entity crops passed: PNG at 1, 0.5 and 2 px per unit within 2 px, inside the 8 unit tolerance, the right pixels under an offset letterboxed viewBox and an uneven stretch, wrong-size and blank controls\n");
  } finally {
    await context.close();
  }
}

/**
 * Crops fitted to the review body limit (QA defect 3): 100 notes, each with
 * a noisy crop at the capture bound, are fitted under 256 KiB by the page's
 * own re-encoder. Every note and its body survive; crops are made smaller or
 * left out, and the counts say which.
 */
export async function checkCropBudget(browser) {
  const compiled = await build({
    entryPoints: [fileURLToPath(new URL("../src/budget.ts", import.meta.url))],
    bundle: true, write: false, format: "iife", globalName: "budgetHarness", platform: "browser",
  });
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    await page.setContent("<main></main>");
    await page.addScriptTag({ content: compiled.outputFiles[0].text });
    const result = await page.evaluate(async () => {
      const cap = 262_144;
      const canvas = document.createElement("canvas");
      const draw = context => {
        const pixels = context.createImageData(canvas.width, canvas.height);
        // Blocky noise: 4 px cells of random colour, hard on JPEG and PNG alike.
        for (let y = 0; y < canvas.height; y += 4) {
          for (let x = 0; x < canvas.width; x += 4) {
            context.fillStyle = `rgb(${Math.random() * 256 | 0},${Math.random() * 256 | 0},${Math.random() * 256 | 0})`;
            context.fillRect(x, y, 4, 4);
          }
        }
        void pixels;
      };
      const crop = (index) => {
        // Noise is the worst case for JPEG; the capture keeps a crop under 32 KiB of base64.
        for (const [width, height] of [[480, 360], [320, 240], [200, 150], [120, 90], [80, 60]]) {
          canvas.width = width;
          canvas.height = height;
          draw(canvas.getContext("2d"));
          const type = index % 10 === 0 ? "image/png" : "image/jpeg";
          const data = canvas.toDataURL(type, 0.82).split(",", 2)[1];
          if (data.length <= 32_768) return { media_type: type, data_base64: data };
        }
        throw new Error("no crop under the capture bound");
      };
      const notes = Array.from({ length: 100 }, (_, index) => ({
        client_id: `note-${index}`,
        block_id: "block",
        block_label: "Block",
        kind: "comment",
        body: `Note ${index} keeps its words.`,
        region_selector: { scope: "block", anchor_id: "block", block_digest: "d".repeat(64), x_ppm: 0, y_ppm: 0, width_ppm: 1, height_ppm: 1, capture_width_px: 1, capture_height_px: 1 },
        excerpt: { text: `quote ${index}`, image: crop(index) },
      }));
      const size = (list) => new TextEncoder().encode(JSON.stringify({ event_id: "019f9b53-a341-7fa7-84c2-5f198ceea001", session_id: "019f9b53-a341-7fa7-84c2-5f198ceea002", revision: 1, verdict: "approve_with_notes", notes: list })).byteLength;
      const before = size(notes);
      const fitted = await budgetHarness.fitCrops(notes, cap, size);
      const decodable = await Promise.all(fitted.notes.filter((note) => note.excerpt?.image).map(async (note) => {
        const image = new Image();
        image.src = `data:${note.excerpt.image.media_type};base64,${note.excerpt.image.data_base64}`;
        await image.decode();
        return Math.max(image.width, image.height);
      }));
      return {
        before,
        after: size(fitted.notes),
        cap,
        count: fitted.notes.length,
        bodies: fitted.notes.every((note, index) => note.body === notes[index].body && note.region_selector && note.excerpt?.text === `quote ${index}`),
        kept: decodable.length,
        smallest: Math.min(...decodable),
        reduced: fitted.reduced,
        dropped: fitted.dropped,
        small: await budgetHarness.fitCrops(notes.slice(0, 2), cap, size).then((small) => small.reduced + small.dropped),
      };
    });
    assert.ok(result.before > result.cap, `the fixture fits already: ${JSON.stringify(result)}`);
    assert.ok(result.after <= result.cap, `the fitted review is over the cap: ${JSON.stringify(result)}`);
    assert.equal(result.count, 100);
    assert.equal(result.bodies, true, "a note lost its body, selector or quote");
    assert.ok(result.reduced > 0 && result.reduced + result.dropped === 100, JSON.stringify(result));
    assert.ok(result.kept > 0, JSON.stringify(result));
    assert.equal(result.small, 0, "a review under the cap was changed");
    process.stdout.write(`crop budget passed: 100 image notes from ${result.before} to ${result.after} bytes under ${result.cap}; ${result.reduced} crops made smaller, ${result.dropped} left out, every note kept\n`);
  } finally {
    await context.close();
  }
}
