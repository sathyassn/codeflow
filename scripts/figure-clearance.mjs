#!/usr/bin/env node
// Clear the guide's authored figure labels for figure rule 8 in every engine.
//
// Usage: node scripts/figure-clearance.mjs <docs-portal dir> [--check] [path filter...]
//
// Each authored declaration under <docs-portal>/figures is rendered in
// Chromium, Firefox and WebKit, at the wide and the narrow width. A label's
// box is the union of its boxes in the three engines, in the composition's
// own units, so the tallest and widest face any engine reports is the one
// cleared. A label must then sit at least CLEARANCE units from every mark it
// does not label (8 px at the smallest 0.9 render scale) and must not
// overprint another label.
//
// A label that fails moves, never its marks: the smallest whole-unit step
// up, down, left or right that clears it. When the step would put it on a
// neighbouring label, that label moves by the same step with it, so a stack
// of lines keeps its pitch. A step that leaves the composition, or that
// cannot clear within MAX_STEP units, is reported and left to a person.
// Layout figures and derived figures are generated, so they are reported,
// not moved. --check reports and writes nothing. The script is idempotent:
// a clear figure is left byte for byte.
import { readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { createRequire } from "node:module";

const CLEARANCE = 9;
const TEXT_GAP = 0.5;
const MAX_STEP = 16;

const args = process.argv.slice(2);
const check = args.includes("--check");
const [rootArg, ...filters] = args.filter((arg) => arg !== "--check");
if (!rootArg) {
  console.error("usage: node scripts/figure-clearance.mjs <docs-portal dir> [--check] [path filter...]");
  process.exit(2);
}
const root = path.resolve(rootArg);
const grammar = await import(pathToFileURL(path.join(root, "scripts/figure-grammar.mjs")).href);
const playwright = createRequire(path.join(root, "package.json"))("@playwright/test");

const styles = path.join(root, "src/styles");
let css = (await Promise.all(["utility-tokens.css", "portal.css", "figure-roles.css", "figure.css"].map((name) => readFile(path.join(styles, name), "utf8")))).join("\n");
for (const [reference, file] of [...css.matchAll(/url\("\.\.\/fonts\/([^"]+\.woff2)"\)/g)]) {
  const data = (await readFile(path.join(styles, "../fonts", file))).toString("base64");
  css = css.replace(reference, `url("data:font/woff2;base64,${data}")`);
}

async function walk(dir) {
  const out = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...await walk(full));
    else if (full.endsWith(".json")) out.push(full);
  }
  return out;
}

// Runs in the page: every label's box and every mark's outline, in the
// visible composition's own units, in draw order.
function measure() {
  const svg = [...document.querySelectorAll("svg.cf-fig-svg")].find((node) => node.getClientRects().length > 0);
  const inverse = svg.getScreenCTM().inverse();
  const toSvg = (x, y) => {
    const point = new DOMPoint(x, y).matrixTransform(inverse);
    return [point.x, point.y];
  };
  const texts = [...svg.querySelectorAll("text")].filter((node) => node.textContent.trim()).map((node) => {
    const rect = node.getBoundingClientRect();
    const [left, top] = toSvg(rect.left, rect.top);
    const [right, bottom] = toSvg(rect.right, rect.bottom);
    return [left, top, right, bottom];
  });
  const DRAWN = ["rect", "circle", "ellipse", "path", "line", "polygon", "polyline"];
  const marks = [];
  for (const group of svg.querySelectorAll("[data-state]")) {
    const nodes = [...group.children].filter((node) => DRAWN.includes(node.tagName.toLowerCase()));
    const outlines = nodes.map((node) => {
      const tag = node.tagName.toLowerCase();
      const style = getComputedStyle(node);
      const half = (parseFloat(style.strokeWidth) || 0) / 2;
      const points = [];
      if (tag === "circle") {
        const cx = node.cx.baseVal.value; const cy = node.cy.baseVal.value; const r = node.r.baseVal.value;
        for (let step = 0; step < 32; step += 1) points.push([cx + r * Math.cos(step * Math.PI / 16), cy + r * Math.sin(step * Math.PI / 16)]);
        points.push([cx, cy]);
      } else if (tag === "rect") {
        const x = node.x.baseVal.value; const y = node.y.baseVal.value; const w = node.width.baseVal.value; const h = node.height.baseVal.value;
        const nx = Math.max(2, Math.ceil(w / 3)); const ny = Math.max(2, Math.ceil(h / 3));
        for (let i = 0; i <= nx; i += 1) { points.push([x + (w * i) / nx, y]); points.push([x + (w * i) / nx, y + h]); }
        for (let j = 0; j <= ny; j += 1) { points.push([x, y + (h * j) / ny]); points.push([x + w, y + (h * j) / ny]); }
      } else {
        let length = 0;
        try { length = node.getTotalLength(); } catch { length = 0; }
        const count = Math.max(2, Math.ceil(length / 2));
        for (let i = 0; i <= count; i += 1) {
          const point = node.getPointAtLength((length * i) / count);
          points.push([point.x, point.y]);
        }
      }
      const filled = style.fill && style.fill !== "none" && (tag === "rect" || tag === "circle");
      const box = node.getBBox();
      return { half, points, area: filled ? [box.x, box.y, box.x + box.width, box.y + box.height] : null };
    });
    marks.push(outlines);
  }
  return { texts, marks };
}

const union = (boxes) => boxes.reduce((a, b) => [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.max(a[2], b[2]), Math.max(a[3], b[3])]);
const shift = (box, [dx, dy]) => [box[0] + dx, box[1] + dy, box[2] + dx, box[3] + dy];
const overlap = (a, b) => Math.min(Math.min(a[2], b[2]) - Math.max(a[0], b[0]), Math.min(a[3], b[3]) - Math.max(a[1], b[1]));

// The nearest a box comes to a mark it does not label, by the probe's own
// measure; negative when the box sits on the mark.
function distance(box, outlines) {
  let nearest = Infinity;
  for (const { half, points, area } of outlines) {
    for (const [x, y] of points) {
      const dx = Math.max(box[0] - x, 0, x - box[2]);
      const dy = Math.max(box[1] - y, 0, y - box[3]);
      nearest = Math.min(nearest, dx === 0 && dy === 0 ? -1 : Math.hypot(dx, dy) - half);
    }
    if (area) {
      const depth = overlap(area, box);
      if (depth > 0) nearest = Math.min(nearest, -depth);
    }
  }
  return nearest;
}

function solve(composition, measured) {
  const labels = composition.draw.map((item, index) => ({ item, index })).filter(({ item }) => item.text !== undefined && item.text.trim());
  const marked = composition.draw.filter((item) => item.state !== undefined);
  if (labels.length !== measured.texts.length || marked.length !== measured.marks.length) throw new Error("the render does not match the declaration's draw list");
  const boxes = measured.texts.map((box) => [...box]);
  const bounds = [0, 0, composition.width, composition.height];
  const inside = (box, original) => box[0] >= Math.min(bounds[0], original[0]) && box[1] >= Math.min(bounds[1], original[1]) && box[2] <= Math.max(bounds[2], original[2]) && box[3] <= Math.max(bounds[3], original[3]);
  const markClear = (t, box) => marked.every((mark, m) => (mark.id !== undefined && (labels[t].item.for ?? []).includes(mark.id)) || distance(box, measured.marks[m]) >= CLEARANCE);
  const textsOn = (t, box, moving) => boxes.map((other, u) => u).filter((u) => !moving.has(u) && u !== t && overlap(box, boxes[u]) > -TEXT_GAP);
  const failing = (t) => !markClear(t, boxes[t]) || textsOn(t, boxes[t], new Set()).length > 0;
  const moves = new Map();
  for (let pass = 0; pass < 4; pass += 1) {
    let changed = false;
    for (let t = 0; t < boxes.length; t += 1) {
      if (!failing(t)) continue;
      let best = null;
      for (let step = 1; step <= MAX_STEP && !best; step += 1) {
        for (const delta of [[0, -step], [0, step], [-step, 0], [step, 0]]) {
          // The label and every label the step would put it on move together;
          // a label it already overprints stays, so the step must clear it.
          const partners = new Set(textsOn(t, boxes[t], new Set()));
          const group = new Set([t]);
          let blocked = false;
          for (let grew = true; grew && !blocked;) {
            grew = false;
            for (const u of [...group]) {
              for (const v of textsOn(u, shift(boxes[u], delta), group)) {
                if (partners.has(v)) { blocked = true; break; }
                group.add(v);
                grew = true;
              }
            }
          }
          const ok = !blocked && [...group].every((u) => markClear(u, shift(boxes[u], delta)) && inside(shift(boxes[u], delta), measured.texts[u]));
          if (ok && (!best || group.size < best.group.size)) best = { delta, group };
        }
      }
      if (!best) continue;
      for (const u of best.group) {
        boxes[u] = shift(boxes[u], best.delta);
        const previous = moves.get(u) ?? [0, 0];
        moves.set(u, [previous[0] + best.delta[0], previous[1] + best.delta[1]]);
      }
      changed = true;
    }
    if (!changed) break;
  }
  const unresolved = labels.filter((_, t) => failing(t)).map(({ item }) => item.text);
  for (const [u, [dx, dy]] of moves) {
    const item = labels[u].item;
    item.x += dx;
    item.y += dy;
  }
  return { moves: [...moves].filter(([, [dx, dy]]) => dx || dy).map(([u, [dx, dy]]) => `"${labels[u].item.text}" ${dx ? `x${dx > 0 ? "+" : ""}${dx}` : ""}${dx && dy ? " " : ""}${dy ? `y${dy > 0 ? "+" : ""}${dy}` : ""}`), unresolved: [...new Set(unresolved)] };
}

const files = (await walk(path.join(root, "figures"))).filter((file) => !filters.length || filters.some((filter) => file.includes(filter))).sort();
const engines = await Promise.all(["chromium", "firefox", "webkit"].map(async (name) => {
  const browser = await playwright[name].launch({ headless: true });
  return { browser, page: await browser.newPage() };
}));
let unresolvedCount = 0;
let movedCount = 0;
try {
  for (const file of files) {
    const text = await readFile(file, "utf8");
    const declaration = JSON.parse(text);
    const name = path.relative(path.join(root, "figures"), file);
    if (declaration.figure.binding === "derived" || declaration.figure.layout !== undefined) continue;
    const lines = [];
    for (const [variant, width] of [["wide", 1280], ["narrow", 390]]) {
      const html = grammar.renderFigure(declaration, { idPrefix: "f" });
      const measured = [];
      for (const { page } of engines) {
        await page.setViewportSize({ width, height: 900 });
        await page.setContent(`<!doctype html><html data-theme="light" data-cfp-skin="graphite"><head><style>${css} body{margin:0;background:var(--cf-canvas);font-family:var(--cf-font-sans)} main{box-sizing:content-box;max-width:720px;margin:0 auto;padding:0 16px}</style></head><body><main>${html}</main></body></html>`);
        await page.evaluate(async () => { await Promise.all([...document.fonts].map((face) => face.load())); await document.fonts.ready; });
        measured.push(await page.evaluate(measure));
      }
      const merged = { texts: measured[0].texts.map((_, i) => union(measured.map((m) => m.texts[i]))), marks: measured[0].marks };
      const { moves, unresolved } = solve(declaration.figure[variant], merged);
      if (moves.length) lines.push(`  ${variant}: ${moves.join("; ")}`);
      if (unresolved.length) lines.push(`  ${variant}: UNRESOLVED ${unresolved.map((label) => `"${label}"`).join(", ")}`);
      movedCount += moves.length;
      unresolvedCount += unresolved.length;
    }
    if (!lines.length) continue;
    console.log(name);
    for (const line of lines) console.log(line);
    const output = `${JSON.stringify(declaration, null, 2)}\n`;
    if (!check && output !== text) await writeFile(file, output);
  }
} finally {
  await Promise.all(engines.map(({ browser }) => browser.close()));
}
console.log(`${check ? "would move" : "moved"} ${movedCount} labels; ${unresolvedCount} unresolved`);
process.exit(check && (movedCount || unresolvedCount) ? 1 : unresolvedCount ? 1 : 0);
