/** Visible excerpts for the calling harness. Gesture UX is unchanged. */

const TEXT_CARRIERS = "p, h1, h2, h3, h4, li, td, th, blockquote, figcaption, text, tspan, span, strong, em, label";
const SVG_PAINT_PROPS = [
  "fill",
  "stroke",
  "stroke-width",
  "stroke-dasharray",
  "stroke-opacity",
  "fill-opacity",
  "opacity",
  "font-family",
  "font-size",
  "font-weight",
  "font-style",
  "letter-spacing",
  "text-anchor",
  "dominant-baseline",
  "color",
];
// The service's bound on an excerpt image, in base64 bytes (24 KiB decoded).
const MAX_JPEG_B64 = 32_768;
const MAX_CROP_WIDTH = 480;
const MAX_CROP_HEIGHT = 360;

export interface ExcerptImage {
  /** JPEG, or PNG for the crop of a review entity (SPC-014 B4). */
  readonly media_type: "image/jpeg" | "image/png";
  readonly data_base64: string;
}

export interface FeedbackExcerpt {
  readonly text?: string;
  readonly image?: ExcerptImage;
}

export function visibleTextOf(element: Element): string {
  if (element.namespaceURI?.includes("svg") || element.querySelector("text, tspan")) {
    const parts = svgTextParts(element);
    if (parts.length) return collapse(parts.join(" "));
  }
  const aria = element.getAttribute("aria-label")?.trim() ?? "";
  const live = collapse(element instanceof HTMLElement ? element.innerText : (element.textContent ?? ""));
  return live || aria;
}

export function quoteFromRange(range: Range): string {
  const fragment = range.cloneContents();
  const svgParts = svgTextParts(fragment);
  if (svgParts.length) return svgParts.join(" ");
  return range.toString();
}

function svgTextParts(root: Element | DocumentFragment): string[] {
  const nodes = root instanceof Element && root.matches("text, tspan")
    ? [root]
    : [...root.querySelectorAll("text, tspan")];
  const carriers = new Set(nodes);
  return nodes.filter((node) => {
    for (let parent = node.parentElement; parent; parent = parent.parentElement) {
      if (carriers.has(parent)) return false;
    }
    return true;
  }).map((node) => collapse(node.textContent ?? "")).filter(Boolean);
}

export function intersectingVisibleText(root: HTMLElement, box: DOMRect): string {
  // Review chrome (note markers, the marquee) lives inside the document root
  // but is not document content — its digits must never enter an excerpt.
  const nodes = [...root.querySelectorAll(TEXT_CARRIERS)].filter((node) => {
    if (node.closest(".cf-marker-layer")) return false;
    const style = getComputedStyle(node);
    if (style.visibility === "hidden" || style.display === "none") return false;
    const rect = node.getBoundingClientRect();
    return rect.width >= 2 && rect.height >= 2 && intersects(rect, box);
  });
  const included = new Set<Element>(nodes);
  const parts: string[] = [];
  for (const node of nodes) {
    // A carrier nested in an included carrier (strong in p, tspan in text,
    // li in li) would repeat its ancestor's textContent verbatim.
    let ancestor = node.parentElement;
    let covered = false;
    while (ancestor && ancestor !== root) {
      if (included.has(ancestor)) {
        covered = true;
        break;
      }
      ancestor = ancestor.parentElement;
    }
    if (covered) continue;
    const text = visibleTextOf(node);
    if (text) parts.push(text);
  }
  return parts.join(" ").slice(0, 4000);
}

/**
 * Rasterize a viewport rectangle of the document.
 *
 * Stages use CSS variables (`fill="var(--cf-text)"`); a naive SVG clone has
 * no :root and paints nothing. Bake computed paints from the live tree, then
 * crop. HTML cannot go through SVG-as-image: browsers skip `foreignObject`
 * in that mode, which is why element/region crops used to ship blank. Paint
 * HTML onto a canvas from live layout instead. Reject near-blank frames.
 */
export async function captureRectJpeg(
  root: HTMLElement,
  box: DOMRect,
  owner: SVGSVGElement | null = null,
  { png = false }: { png?: boolean } = {},
): Promise<ExcerptImage | null> {
  if (box.width < 4 || box.height < 4) return null;
  const svg = owner ?? intersectingSvg(root, box);
  if (svg && containsRect(svg.getBoundingClientRect(), box)) {
    const fromSvg = await rasterizeSvgCrop(svg, box, png);
    if (fromSvg) return fromSvg;
  }
  return rasterizeDomSlice(root, box);
}

export const captureRegionJpeg = captureRectJpeg;

function intersectingSvg(root: HTMLElement, box: DOMRect): SVGSVGElement | null {
  // Marker speech-bubble icons are svgs too; a small region over a saved
  // note must not rasterize the marker instead of the content under it.
  const svg = [...root.querySelectorAll("svg")]
    .filter((node) => !node.closest(".cf-marker-layer"))
    .find((node) => intersects(node.getBoundingClientRect(), box));
  return svg instanceof SVGSVGElement ? svg : null;
}

// The crop is drawn from the same user-space rectangle `userSpaceBox`
// reports, through the drawing's own screen transform (viewBox origin,
// aspect placement and non-uniform scale included), and stretched exactly
// onto the crop, so its pixels are the pixels under the box (T118-2).
async function rasterizeSvgCrop(svg: SVGSVGElement, box: DOMRect, png: boolean): Promise<ExcerptImage | null> {
  const user = userRect(svg, box);
  if (!user || user.width <= 0 || user.height <= 0) return null;
  const clone = svg.cloneNode(true) as SVGSVGElement;
  bakeSvgPaints(svg, clone);
  clone.setAttribute("xmlns", "http://www.w3.org/2000/svg");
  clone.setAttribute("viewBox", `${user.x} ${user.y} ${user.width} ${user.height}`);
  clone.setAttribute("preserveAspectRatio", "none");
  clone.setAttribute("width", String(Math.round(box.width)));
  clone.setAttribute("height", String(Math.round(box.height)));
  clone.removeAttribute("style");
  return canvasFromSvgMarkup(new XMLSerializer().serializeToString(clone), box, groundOf(svg), png);
}

// The colour a drawing sits on: its nearest opaque background, else the page.
function groundOf(element: Element): string {
  for (let node: Element | null = element; node; node = node.parentElement) {
    const color = opaqueColor(getComputedStyle(node).backgroundColor);
    if (color) return color;
  }
  return "#ffffff";
}

/**
 * The thin-stroke padding in CSS px on each axis (SPC-014 B4), held inside
 * the service's containment tolerance of 8 user units on that axis when the
 * drawing is shown below 0.75 px per unit there, so a correct crop is never
 * refused as outside its entity, whatever the aspect placement (T118-3).
 */
export function entityCropPadding(svg: SVGSVGElement, strokePadding: number): { x: number; y: number } {
  const matrix = svg.getScreenCTM();
  const perUnitX = matrix ? Math.hypot(matrix.a, matrix.b) : 1;
  const perUnitY = matrix ? Math.hypot(matrix.c, matrix.d) : 1;
  return {
    x: Math.min(strokePadding, 7.5 * (perUnitX || 1)),
    y: Math.min(strokePadding, 7.5 * (perUnitY || 1)),
  };
}

/** A client rectangle widened on every side, as the entity crop pads thin strokes. */
export function paddedRect(rect: DOMRect, padding: number | { x: number; y: number }): DOMRect {
  const { x, y } = typeof padding === "number" ? { x: padding, y: padding } : padding;
  return new DOMRect(rect.left - x, rect.top - y, rect.width + 2 * x, rect.height + 2 * y);
}

// A client rectangle in an SVG's user space, unrounded.
function userRect(svg: SVGSVGElement, box: DOMRect): { x: number; y: number; width: number; height: number } | null {
  const matrix = svg.getScreenCTM()?.inverse();
  if (!matrix) return null;
  const corners = [[box.left, box.top], [box.right, box.top], [box.left, box.bottom], [box.right, box.bottom]]
    .map(([x, y]) => new DOMPoint(x, y).matrixTransform(matrix));
  const xs = corners.map((point) => point.x);
  const ys = corners.map((point) => point.y);
  const left = Math.min(...xs);
  const top = Math.min(...ys);
  return { x: left, y: top, width: Math.max(...xs) - left, height: Math.max(...ys) - top };
}

/** A client rectangle in the user space of an SVG, at most three decimals (SPC-014 I2). */
export function userSpaceBox(svg: SVGSVGElement, box: DOMRect): { x: number; y: number; width: number; height: number } | null {
  const user = userRect(svg, box);
  if (!user) return null;
  const round = (value: number): number => Math.round(value * 1000) / 1000;
  return { x: round(user.x), y: round(user.y), width: round(user.width), height: round(user.height) };
}

async function rasterizeDomSlice(root: HTMLElement, box: DOMRect): Promise<ExcerptImage | null> {
  const scale = Math.min(MAX_CROP_WIDTH / box.width, MAX_CROP_HEIGHT / box.height, 1);
  const width = Math.max(1, Math.round(box.width * scale));
  const height = Math.max(1, Math.round(box.height * scale));
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;

  const source = bestHtmlSource(root, box);
  ctx.fillStyle = opaqueColor(getComputedStyle(document.documentElement).backgroundColor)
    || opaqueColor(getComputedStyle(source).backgroundColor)
    || "#ffffff";
  ctx.fillRect(0, 0, width, height);
  const reference = ctx.getImageData(0, 0, 1, 1).data;

  const painted = await paintElementTree(ctx, source, box, scale);
  if (!painted) return null;
  return imageFromCanvas(ctx, canvas, reference, false);
}

function bestHtmlSource(root: HTMLElement, box: DOMRect): HTMLElement {
  const blocks = [...root.querySelectorAll<HTMLElement>("[data-cf-block-id]")];
  const hit = blocks
    .filter((block) => containsRect(block.getBoundingClientRect(), box))
    .sort((left, right) => rectArea(left.getBoundingClientRect()) - rectArea(right.getBoundingClientRect()))[0];
  return hit ?? root;
}

async function paintElementTree(
  ctx: CanvasRenderingContext2D,
  source: HTMLElement,
  box: DOMRect,
  scale: number,
): Promise<number> {
  let painted = 0;
  const elements: HTMLElement[] = [];
  const collect = (node: Element): void => {
    if (node.classList.contains("cf-marker-layer")) return;
    if (node instanceof HTMLElement) {
      const rect = node.getBoundingClientRect();
      if (intersects(rect, box) && rect.width > 0 && rect.height > 0) elements.push(node);
    }
    for (const child of node.children) collect(child);
  };
  collect(source);

  for (const element of elements) {
    const style = getComputedStyle(element);
    if (style.visibility === "hidden" || style.display === "none") continue;
    const rect = element.getBoundingClientRect();
    const x = (rect.left - box.left) * scale;
    const y = (rect.top - box.top) * scale;
    const w = rect.width * scale;
    const h = rect.height * scale;
    const bg = style.backgroundColor;
    if (!isTransparent(bg)) {
      ctx.fillStyle = bg;
      const radius = (parseFloat(style.borderTopLeftRadius) || 0) * scale;
      if (radius > 1) {
        roundedRect(ctx, x, y, w, h, Math.min(radius, w / 2, h / 2));
        ctx.fill();
      } else {
        ctx.fillRect(x, y, w, h);
      }
      painted += 1;
    }
    const borderWidth = parseFloat(style.borderTopWidth) || 0;
    if (borderWidth > 0 && !isTransparent(style.borderTopColor)) {
      ctx.strokeStyle = style.borderTopColor;
      ctx.lineWidth = Math.max(1, borderWidth * scale);
      ctx.strokeRect(x + 0.5, y + 0.5, Math.max(0, w - 1), Math.max(0, h - 1));
      painted += 1;
    }
    // Raster content: backgrounds and text alone leave an <img> excerpt blank.
    // CSP restricts img-src to self/data:/blob:, so drawImage cannot taint.
    if (element instanceof HTMLImageElement && element.complete && element.naturalWidth > 0) {
      ctx.drawImage(element, x, y, w, h);
      painted += 1;
    }
  }

  for (const svg of source.querySelectorAll<SVGSVGElement>("svg")) {
    if (svg.closest(".cf-marker-layer") || svg.parentElement?.closest("svg")) continue;
    const rect = svg.getBoundingClientRect();
    if (!intersects(rect, box) || rect.width <= 0 || rect.height <= 0) continue;
    const clone = svg.cloneNode(true) as SVGSVGElement;
    bakeSvgPaints(svg, clone);
    clone.setAttribute("xmlns", "http://www.w3.org/2000/svg");
    clone.setAttribute("width", String(rect.width));
    clone.setAttribute("height", String(rect.height));
    const url = URL.createObjectURL(new Blob([new XMLSerializer().serializeToString(clone)], { type: "image/svg+xml" }));
    try {
      const image = await loadImage(url);
      ctx.drawImage(image, (rect.left - box.left) * scale, (rect.top - box.top) * scale, rect.width * scale, rect.height * scale);
      painted += 1;
    } catch {
      // Keep real text/layout context if an unsupported SVG cannot be painted.
    } finally {
      URL.revokeObjectURL(url);
    }
  }

  const walker = document.createTreeWalker(source, NodeFilter.SHOW_TEXT);
  let node: Node | null;
  while ((node = walker.nextNode())) {
    if (!(node instanceof Text) || !node.parentElement) continue;
    const parent = node.parentElement;
    if (parent.closest(".cf-marker-layer, style, script")) continue;
    if (parent.closest("svg") && !parent.closest("foreignObject")) continue;
    const style = getComputedStyle(parent);
    if (style.visibility === "hidden" || style.display === "none" || Number(style.opacity) === 0) continue;
    if (clippedAway(parent, source)) continue;
    painted += paintTextNode(ctx, node, style, box, scale);
  }
  return painted;
}

// Text a clipping box hides from sight, such as a screen-reader label (a
// diff line's "Added:"), is not painted over the words that show.
function clippedAway(element: Element, source: Element): boolean {
  for (let node: Element | null = element; node && node !== source.parentElement; node = node.parentElement) {
    const rect = node.getBoundingClientRect();
    if ((rect.width <= 1 || rect.height <= 1) && getComputedStyle(node).overflow !== "visible") return true;
  }
  return false;
}

function paintTextNode(
  ctx: CanvasRenderingContext2D,
  node: Text,
  style: CSSStyleDeclaration,
  box: DOMRect,
  scale: number,
): number {
  const value = node.data;
  if (!collapse(value)) return 0;
  const probe = document.createRange();
  probe.selectNodeContents(node);
  if (![...probe.getClientRects()].some((rect) => intersects(rect, box))) return 0;

  ctx.save();
  ctx.beginPath();
  ctx.rect(0, 0, ctx.canvas.width, ctx.canvas.height);
  ctx.clip();
  ctx.fillStyle = style.color || "#111111";
  const fontSize = Math.max(1, (parseFloat(style.fontSize) || 16) * scale);
  ctx.font = `${style.fontStyle} ${style.fontWeight} ${fontSize}px ${style.fontFamily}`;
  ctx.textBaseline = "top";
  ctx.textAlign = "left";

  let painted = 0;
  let index = 0;
  while (index < value.length) {
    probe.setStart(node, index);
    probe.setEnd(node, Math.min(value.length, index + 1));
    const lineTop = probe.getBoundingClientRect().top;
    let end = index + 1;
    while (end < value.length) {
      probe.setStart(node, index);
      probe.setEnd(node, end + 1);
      if (Math.abs(probe.getBoundingClientRect().top - lineTop) > 1.5) break;
      end += 1;
    }
    probe.setStart(node, index);
    probe.setEnd(node, end);
    const lineRect = probe.getBoundingClientRect();
    if (intersects(lineRect, box) && lineRect.width > 0.5 && lineRect.height > 0.5) {
      ctx.fillText(value.slice(index, end), (lineRect.left - box.left) * scale, (lineRect.top - box.top) * scale);
      painted += 1;
    }
    index = end;
  }
  ctx.restore();
  return painted;
}

async function canvasFromSvgMarkup(markup: string, box: DOMRect, ground: string, png: boolean): Promise<ExcerptImage | null> {
  const blob = new Blob([markup], { type: "image/svg+xml;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  try {
    const image = await loadImage(url);
    const scale = Math.min(MAX_CROP_WIDTH / box.width, MAX_CROP_HEIGHT / box.height, 1);
    const width = Math.max(1, Math.round(box.width * scale));
    const height = Math.max(1, Math.round(box.height * scale));
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    if (!ctx) return null;
    ctx.fillStyle = ground;
    ctx.fillRect(0, 0, width, height);
    const reference = ctx.getImageData(0, 0, 1, 1).data;
    ctx.drawImage(image, 0, 0, width, height);
    return imageFromCanvas(ctx, canvas, reference, png);
  } catch {
    return null;
  } finally {
    URL.revokeObjectURL(url);
  }
}

// A PNG keeps a drawing's flat colours and thin strokes exact; when it is
// over the bound, the JPEG ladder below still yields evidence.
function imageFromCanvas(ctx: CanvasRenderingContext2D, canvas: HTMLCanvasElement, ground: Uint8ClampedArray, png: boolean): ExcerptImage | null {
  try {
    if (isOneColour(ctx, canvas.width, canvas.height, ground)) return null;
    if (png) {
      const data_base64 = canvas.toDataURL("image/png").split(",", 2)[1] ?? "";
      if (data_base64 && data_base64.length <= MAX_JPEG_B64) return { media_type: "image/png", data_base64 };
    }
    for (const quality of [0.82, 0.7, 0.55, 0.4]) {
      const dataUrl = canvas.toDataURL("image/jpeg", quality);
      const data_base64 = dataUrl.split(",", 2)[1] ?? "";
      if (data_base64 && data_base64.length <= MAX_JPEG_B64) {
        return { media_type: "image/jpeg", data_base64 };
      }
    }
    return null;
  } catch {
    // A tainted or unreadable canvas degrades to "no image", never to a
    // rejected saveComposer promise that strands the note in the composer.
    return null;
  }
}

function bakeSvgPaints(source: SVGElement, clone: SVGElement): void {
  const srcNodes = [source, ...source.querySelectorAll<SVGElement>("*")];
  const dstNodes = [clone, ...clone.querySelectorAll<SVGElement>("*")];
  for (let i = 0; i < srcNodes.length && i < dstNodes.length; i += 1) {
    const src = srcNodes[i];
    const dst = dstNodes[i];
    if (!src || !dst) continue;
    const computed = getComputedStyle(src);
    for (const prop of SVG_PAINT_PROPS) {
      const value = computed.getPropertyValue(prop).trim();
      if (value) dst.setAttribute(prop, value);
    }
  }
}

// A crop that is all ground, in either mode, carries no evidence: fewer than
// 1% of the sampled pixels (and fewer than four) differ from the ground.
function isOneColour(ctx: CanvasRenderingContext2D, width: number, height: number, ground: Uint8ClampedArray): boolean {
  const sample = ctx.getImageData(0, 0, width, height).data;
  let painted = 0;
  let seen = 0;
  for (let i = 0; i < sample.length; i += 64) {
    seen += 1;
    if ([0, 1, 2].some((channel) => Math.abs((sample[i + channel] ?? 0) - (ground[channel] ?? 0)) > 8)) painted += 1;
  }
  return painted < Math.max(4, seen * 0.01);
}

function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("excerpt image failed"));
    image.src = url;
  });
}

function roundedRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number,
): void {
  ctx.beginPath();
  ctx.moveTo(x + radius, y);
  ctx.arcTo(x + width, y, x + width, y + height, radius);
  ctx.arcTo(x + width, y + height, x, y + height, radius);
  ctx.arcTo(x, y + height, x, y, radius);
  ctx.arcTo(x, y, x + width, y, radius);
  ctx.closePath();
}

function isTransparent(color: string): boolean {
  if (!color || color === "transparent") return true;
  const rgb = color.match(/^rgba?\((.+)\)$/u);
  if (rgb) {
    const parts = (rgb[1] ?? "").split(",").map((part) => part.trim());
    if (parts.length >= 4 && parseFloat(parts[3] ?? "") === 0) return true;
  }
  // Only the space-syntax alpha ("rgb(r g b / 0)") remains; a comma test
  // here would match the blue channel of every opaque rgb(r, g, 0).
  return /\/\s*0(?:\.0+)?\s*\)/u.test(color);
}

function opaqueColor(color: string): string | null {
  return isTransparent(color) ? null : color;
}

function containsRect(outer: DOMRect, inner: DOMRect): boolean {
  return inner.left >= outer.left && inner.right <= outer.right && inner.top >= outer.top && inner.bottom <= outer.bottom;
}

function intersects(left: DOMRect, right: DOMRect): boolean {
  return left.left < right.right && left.right > right.left && left.top < right.bottom && left.bottom > right.top;
}

function rectArea(rect: DOMRect): number {
  return rect.width * rect.height;
}

function collapse(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}
