// The crop likeness rule the annotation matrix applies (likeness.mjs), held
// to synthetic pictures: a crop of the right rectangle passes; a stripe
// field, a blank crop, a crop of another place and the right ink moved 4 or
// 12 px all fail (TSK-071, G071-1).
import assert from "node:assert/strict";
import { test } from "node:test";
import { LIKENESS_FLOOR, NEARBY_OFFSETS, gridShape, inkGrid, likeness, placed } from "./likeness.mjs";

const PAGE = { width: 640, height: 1000 };
// Rectangles the matrix meets: one line, two lines, a paragraph, a picture
// sized area, a small element and a tall area.
const RECTANGLES = [[40, 60, 480, 22], [40, 60, 480, 44], [40, 104, 480, 78], [60, 60, 272, 156], [60, 40, 120, 40], [40, 60, 480, 300]];

test("a crop of its own rectangle is placed", () => {
  for (const rect of RECTANGLES) {
    const { inside } = judge(rect, window(TEXT, rect, 0, 0), 14);
    assert.ok(inside >= 0.95, `${rect}: ${inside}`);
    assert.equal(verdict(rect, window(TEXT, rect, 0, 0), 14), true, `${rect}`);
  }
});

test("a stripe field is not placed", () => {
  for (const rect of RECTANGLES) {
    for (const period of [4, 6, 8, 12, 16]) {
      for (const vertical of [true, false]) {
        const [, , width, height] = rect;
        const stripes = Uint8Array.from({ length: width * height }, (_, index) => {
          const along = vertical ? index % width : Math.floor(index / width);
          return along % period < period / 2 ? 1 : 0;
        });
        const where = `${rect}, ${vertical ? "vertical" : "horizontal"} stripes of ${period} px: ${JSON.stringify(judge(rect, stripes))}`;
        assert.equal(verdict(rect, stripes), false, where);
        // Clearly under the floor, not at it: on a coarser grid a stripe
        // field lines up with one line of text.
        assert.ok(judge(rect, stripes).inside < LIKENESS_FLOOR - 0.1, where);
      }
    }
  }
});

test("the right ink moved 4 or 12 px is not placed", () => {
  for (const rect of RECTANGLES) {
    for (const shift of [4, 12]) {
      for (const [dx, dy] of [[0, shift], [0, -shift], [shift, 0], [-shift, 0]]) {
        const moved = window(TEXT, rect, dx, dy);
        assert.equal(verdict(rect, moved, 14), false, `${rect}, moved ${dx},${dy}: ${JSON.stringify(judge(rect, moved, 14))}`);
      }
    }
  }
});

test("a blank crop and a crop of another place are not placed", () => {
  for (const rect of RECTANGLES) {
    const [, , width, height] = rect;
    assert.equal(verdict(rect, new Uint8Array(width * height)), false, `${rect}: blank`);
    const elsewhere = window(OTHER, rect, 0, 0);
    assert.equal(verdict(rect, elsewhere, 14), false, `${rect}, another text: ${JSON.stringify(judge(rect, elsewhere, 14))}`);
  }
});

// A page of text-like ink: lines of words, each word stems and bars, with a
// seeded generator so every run draws the same page.
const TEXT = textPage(1);
const OTHER = textPage(7, 9);

function textPage(seed, lineTop = 4) {
  const random = generator(seed);
  const ink = new Uint8Array(PAGE.width * PAGE.height);
  for (let top = lineTop; top < PAGE.height - 16; top += 22) {
    let x = 6 + Math.floor(random() * 30);
    while (x < PAGE.width - 10) {
      const word = 18 + Math.floor(random() * 52);
      for (let column = x; column < Math.min(PAGE.width - 6, x + word); column += 1) {
        const glyph = (column - x) % 7;
        for (let y = top; y < top + 12; y += 1) {
          const stem = glyph < 2 && !(random() < 0.25 && y > top + 8);
          const bar = (y === top + 5 || y === top + 11) && glyph < 5;
          const ascender = y < top + 3 && glyph === 0 && (column >> 3) % 3 === 0;
          if (stem || bar || ascender) ink[y * PAGE.width + column] = 1;
        }
      }
      x += word + 8;
    }
  }
  return ink;
}

function generator(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 4294967296;
  };
}

/** The ink of `rect` moved by dx, dy on the page. */
function window(page, [x, y, width, height], dx, dy) {
  const out = new Uint8Array(width * height);
  for (let row = 0; row < height; row += 1) {
    for (let column = 0; column < width; column += 1) out[row * width + column] = page[(y + dy + row) * PAGE.width + x + dx + column];
  }
  return out;
}

/** RGBA pixels of an ink mask, dark on light, with seeded noise like a JPEG's. */
function pixels(mask, noise) {
  const random = generator(5);
  const out = new Uint8ClampedArray(mask.length * 4);
  for (const [index, ink] of mask.entries()) {
    const value = (ink ? 40 : 248) + Math.round((random() - 0.5) * noise);
    out.set([value, value, value, 255], index * 4);
  }
  return out;
}

function grid(mask, width, height, noise = 0) {
  const { columns, rows } = gridShape(width, height);
  return inkGrid(pixels(mask, noise), width, height, columns, rows);
}

const PAGE_GRIDS = new Map();

/** The crop against the page as the matrix measures it. */
function judge(rect, crop, noise = 0) {
  const [, , width, height] = rect;
  const picture = grid(crop, width, height, noise);
  const page = (dx, dy) => {
    const key = `${rect}:${dx},${dy}`;
    if (!PAGE_GRIDS.has(key)) PAGE_GRIDS.set(key, grid(window(TEXT, rect, dx, dy), width, height));
    return PAGE_GRIDS.get(key);
  };
  return {
    inside: likeness(picture, page(0, 0)),
    outside: likeness(picture, page(0, height + 1)),
    nearby: NEARBY_OFFSETS.map(([dx, dy]) => likeness(picture, page(dx, dy))),
  };
}

function verdict(rect, crop, noise = 0) {
  const { inside, outside, nearby } = judge(rect, crop, noise);
  return placed(inside, outside, nearby);
}
