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
const MAX_JPEG_B64 = 32_768;
const MAX_CROP_WIDTH = 480;
const MAX_CROP_HEIGHT = 360;

export interface ExcerptImage {
  readonly media_type: "image/jpeg";
  readonly data_base64: string;
}

export interface FeedbackExcerpt {
  readonly text?: string;
  readonly image?: ExcerptImage;
}

export function visibleTextOf(element: Element): string {
  if (element.namespaceURI?.includes("svg") || element.querySelector("text, tspan")) {
    const parts = [...element.querySelectorAll("text, tspan")]
      .map((node) => (node.textContent ?? "").replace(/\s+/g, " ").trim())
      .filter(Boolean);
    if (parts.length) return collapse(parts.join(" "));
  }
  const aria = element.getAttribute("aria-label")?.trim() ?? "";
  const live = collapse(element instanceof HTMLElement ? element.innerText : (element.textContent ?? ""));
  return live || aria;
}

export function quoteFromRange(range: Range): string {
  const fragment = range.cloneContents();
  const svgParts = [...fragment.querySelectorAll("text, tspan")]
    .map((node) => (node.textContent ?? "").replace(/\s+/g, " ").trim())
    .filter(Boolean);
  if (svgParts.length) return svgParts.join(" ");
  return range.toString();
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
  return uniqueJoin(parts).slice(0, 4000);
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
export async function captureRectJpeg(root: HTMLElement, box: DOMRect): Promise<ExcerptImage | null> {
  if (box.width < 4 || box.height < 4) return null;
  const svg = intersectingSvg(root, box);
  if (svg && containsRect(svg.getBoundingClientRect(), box)) {
    const fromSvg = await rasterizeSvgCrop(svg, box);
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

async function rasterizeSvgCrop(svg: SVGSVGElement, box: DOMRect): Promise<ExcerptImage | null> {
  const svgRect = svg.getBoundingClientRect();
  const clone = svg.cloneNode(true) as SVGSVGElement;
  bakeSvgPaints(svg, clone);
  const vb = svg.viewBox.baseVal;
  const userW = vb && vb.width > 0 ? vb.width : svgRect.width;
  const userH = vb && vb.height > 0 ? vb.height : svgRect.height;
  const sx = userW / Math.max(1, svgRect.width);
  const sy = userH / Math.max(1, svgRect.height);
  clone.setAttribute("xmlns", "http://www.w3.org/2000/svg");
  clone.setAttribute("viewBox", `${(box.left - svgRect.left) * sx} ${(box.top - svgRect.top) * sy} ${Math.max(1, box.width * sx)} ${Math.max(1, box.height * sy)}`);
  clone.setAttribute("width", String(Math.round(box.width)));
  clone.setAttribute("height", String(Math.round(box.height)));
  return canvasFromSvgMarkup(new XMLSerializer().serializeToString(clone), box);
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

  const painted = await paintElementTree(ctx, source, box, scale);
  if (!painted) return null;
  return jpegFromCanvas(ctx, canvas);
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
    painted += paintTextNode(ctx, node, style, box, scale);
  }
  return painted;
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

async function canvasFromSvgMarkup(markup: string, box: DOMRect): Promise<ExcerptImage | null> {
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
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, width, height);
    ctx.drawImage(image, 0, 0, width, height);
    return jpegFromCanvas(ctx, canvas);
  } catch {
    return null;
  } finally {
    URL.revokeObjectURL(url);
  }
}

function jpegFromCanvas(ctx: CanvasRenderingContext2D, canvas: HTMLCanvasElement): ExcerptImage | null {
  try {
    if (isMostlyBlank(ctx, canvas.width, canvas.height)) return null;
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

function isMostlyBlank(ctx: CanvasRenderingContext2D, width: number, height: number): boolean {
  const sample = ctx.getImageData(0, 0, width, height).data;
  let painted = 0;
  let seen = 0;
  for (let i = 0; i < sample.length; i += 64) {
    seen += 1;
    if ((sample[i] ?? 255) < 248 || (sample[i + 1] ?? 255) < 248 || (sample[i + 2] ?? 255) < 248) painted += 1;
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

function uniqueJoin(parts: string[]): string {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const part of parts) {
    if (seen.has(part)) continue;
    seen.add(part);
    out.push(part);
  }
  return out.join(" ");
}
