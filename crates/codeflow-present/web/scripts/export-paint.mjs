import assert from "node:assert/strict";

// A text box must contain ink, not just the page's canvas color. Test every
// line so the last lanes and wrapped text cannot silently disappear.
export function hasTextInk(pixels, imageWidth, box, background) {
  for (let y = Math.ceil(box.y); y < Math.floor(box.y + box.height); y++) {
    for (let x = Math.ceil(box.x); x < Math.floor(box.x + box.width); x++) {
      const offset = (y * imageWidth + x) * 4;
      if (background.some((channel, index) => Math.abs(pixels[offset + index] - channel) > 48)) return true;
    }
  }
  return false;
}

export async function assertStageTextPainted(page, screenshot) {
  const boxes = [];
  for (const handle of await page.locator("iframe[data-cf-export-stage]").elementHandles()) {
    const outer = await handle.boundingBox();
    const frame = await handle.contentFrame();
    const lines = await frame.evaluate(() => {
      const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
      const result = [];
      while (walker.nextNode()) {
        const node = walker.currentNode;
        if (!node.textContent.trim() || node.parentElement.closest("style,script")) continue;
        const range = document.createRange();
        range.selectNodeContents(node);
        for (const rect of range.getClientRects()) {
          if (!rect.width || !rect.height) continue;
          result.push({ x: rect.x, y: rect.y, width: rect.width, height: rect.height, text: node.textContent.slice(0, 60) });
        }
      }
      return result;
    });
    boxes.push(...lines.map((line) => ({ ...line, x: outer.x + line.x, y: outer.y + line.y })));
  }
  const background = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--cf-canvas"));
  // Decode in a separate blank page, outside the export's CSP and sandboxes.
  const decoder = await page.context().newPage();
  try {
    const data = await decoder.evaluate(async ({ encoded, background, boxes }) => {
      const image = new Image();
      image.src = `data:image/png;base64,${encoded}`;
      await image.decode();
      const canvas = document.createElement("canvas");
      canvas.width = image.width; canvas.height = image.height;
      const context = canvas.getContext("2d");
      context.fillStyle = background;
      context.fillRect(0, 0, 1, 1);
      const ground = [...context.getImageData(0, 0, 1, 1).data].slice(0, 3);
      context.drawImage(image, 0, 0);
      return boxes.map((box) => {
        const width = Math.max(1, Math.floor(box.width));
        const height = Math.max(1, Math.floor(box.height));
        return { width, height, pixels: [...context.getImageData(Math.ceil(box.x), Math.ceil(box.y), width, height).data], ground };
      });
    }, { encoded: screenshot.toString("base64"), background, boxes });
    assert.ok(boxes.length, "capture must contain stage text");
    for (const [index, crop] of data.entries()) {
      assert.ok(hasTextInk(crop.pixels, crop.width, { x: 0, y: 0, width: crop.width, height: crop.height }, crop.ground), `stage text did not paint: ${JSON.stringify(boxes[index])}`);
    }
  } finally {
    await decoder.close();
  }
}
