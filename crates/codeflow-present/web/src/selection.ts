import type { ElementSelector, RegionSelector, TextSelector } from "./contracts";
// Explicit .ts: check.mjs loads this module under Node type stripping, which
// does not resolve extensionless relative value imports.
import { intersectingVisibleText, quoteFromRange, visibleTextOf } from "./excerpt.ts";

const CONTEXT_UNITS = 32;
const REGION_SCALE = 1_000_000;
const ANNOTATABLE = [
  "h1", "h2", "h3", "p", "li", "blockquote", "pre", "code", "table", "thead", "tbody", "tr", "th", "td",
  "figure", "figcaption", "img", "video", "audio", "svg", "details", "summary", "article", "aside",
].join(",");

export function annotatableElements(documentRoot: HTMLElement): readonly HTMLElement[] {
  return [...documentRoot.querySelectorAll<HTMLElement>(ANNOTATABLE)]
    .filter((element) => element.closest("[data-cf-block-id]") && !element.closest("button[data-anchor-block]"));
}

/** The element a hover highlight or click pin resolves to — mirrors captureElement. */
export function annotatableAncestor(documentRoot: HTMLElement, target: Element): HTMLElement | null {
  if (target.closest(".cf-marker, .cf-marker-layer")) return null;
  const block = target.closest<HTMLElement>("[data-cf-block-id]");
  if (!block || !documentRoot.contains(block)) return null;
  const element = target.closest<HTMLElement>(ANNOTATABLE) ?? block;
  if (!block.contains(element) || element.closest("button[data-anchor-block]")) return null;
  return element;
}

const TEXTUAL_TAGS = /^(H1|H2|H3|H4|P|LI|PRE|CODE|TD|TH|LABEL|A|EM|STRONG|SMALL|BLOCKQUOTE|SPAN)$/;
const PROSE_SELECTOR = "p, h1, h2, h3, h4, li, pre, td, th, blockquote, figcaption";

/**
 * Words the reviewer can highlight, including SVG `<text>` / `<tspan>` on a
 * stage. Shapes, media, and empty canvas stay non-prose so a box-drag is a
 * region and a click is an element. A blanket "anything in svg/figure is
 * non-prose" rule made stage labels select with no Text chip.
 */
export function isTextualTarget(target: Element): boolean {
  const tag = target.tagName.toUpperCase();
  if (tag === "TEXT" || tag === "TSPAN") return true;
  if ((target.namespaceURI ?? "").includes("svg")) return false;
  if (target.closest("img, video, audio, iframe")) return false;
  if (TEXTUAL_TAGS.test(tag)) return true;
  const reviewRoot = target.closest("[data-cf-review-text-root]");
  if (reviewRoot?.querySelector(PROSE_SELECTOR) && !target.closest("button, svg, figure[role='img']")) {
    return true;
  }
  return Boolean(target.closest(PROSE_SELECTOR) && !target.closest("button"));
}

export interface CapturedTarget {
  readonly blockId: string;
  readonly blockLabel: string;
  readonly selector?: TextSelector;
  readonly element_selector?: ElementSelector;
  readonly region_selector?: RegionSelector;
  readonly summary: string;
  readonly excerptText?: string;
}

export interface Point {
  readonly x: number;
  readonly y: number;
}

export function captureSelection(documentRoot: HTMLElement): CapturedTarget | null {
  const selection = getSelection();
  if (!selection || selection.rangeCount !== 1 || selection.isCollapsed) return null;
  const range = selection.getRangeAt(0);
  const startElement = closestElement(range.startContainer, "[data-cf-review-text-root]");
  const endElement = closestElement(range.endContainer, "[data-cf-review-text-root]");
  if (!startElement || startElement !== endElement || !documentRoot.contains(startElement)) return null;
  const block = startElement.closest<HTMLElement>("[data-cf-block-id]");
  const blockId = block?.dataset.cfBlockId;
  if (!block || !blockId || block.matches(".block--diagram")) return null;

  const exact = quoteFromRange(range);
  if (!exact.trim()) return null;
  // Anchor the actual DOM range, including its occurrence within the block.
  // Rendered formatting and canonical text can differ in whitespace (and HTML
  // blocks may have a canonical title prefix), but never guess through a
  // substantive mismatch or an ambiguous mapping.
  const canonical =
    startElement.getAttribute("data-cf-canonical-text") ?? startElement.textContent ?? "";
  const offsets = canonicalRangeOffsets(startElement, range, canonical);
  if (!offsets) return null;

  return {
    blockId,
    blockLabel: block.dataset.cfBlockLabel ?? blockId,
    selector: selectorFromOffsets(canonical, offsets.start, offsets.end),
    summary: `Text: ${truncate(exact.trim(), 96)}`,
    excerptText: exact.trim(),
  };
}

function canonicalRangeOffsets(root: HTMLElement, range: Range, canonical: string): { start: number; end: number } | null {
  const compact = (text: string) => text.replace(/\s/gu, "");
  const rendered = compact(reviewText(root));
  const normalized = compact(canonical);
  if (!rendered) return null;
  const base = normalized.indexOf(rendered);
  if (base < 0 || normalized.indexOf(rendered, base + 1) >= 0) return null;

  const prefix = range.cloneRange();
  prefix.selectNodeContents(root);
  prefix.setEnd(range.startContainer, range.startOffset);
  const start = base + compact(reviewText(prefix.cloneContents())).length;
  const length = compact(reviewText(range.cloneContents())).length;
  if (!length) return null;

  // Preserve the server's exact UTF-16 coordinates, including internal
  // whitespace, without confusing astral characters with single code units.
  let unit = 0;
  let first = -1;
  for (let index = 0; index < canonical.length; index += 1) {
    if (/\s/u.test(canonical.charAt(index))) continue;
    if (unit === start) first = index;
    unit += 1;
    if (unit === start + length && first >= 0) return { start: first, end: index + 1 };
  }
  return null;
}

function reviewText(root: Node): string {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const parts: string[] = [];
  while (walker.nextNode()) {
    const text = walker.currentNode;
    if (!text.parentElement?.closest("style, script")) parts.push(text.textContent ?? "");
  }
  return parts.join("");
}

export function captureElement(documentRoot: HTMLElement, rawTarget: Element): CapturedTarget | null {
  const block = rawTarget.closest<HTMLElement>("[data-cf-block-id]");
  if (!block || !documentRoot.contains(block)) return null;
  const element = rawTarget.closest<HTMLElement>(ANNOTATABLE) ?? block;
  if (!block.contains(element) || element.closest("button[data-anchor-block]")) return null;
  const blockId = block.dataset.cfBlockId;
  const blockDigest = block.dataset.cfBlockDigest;
  if (!blockId || !blockDigest) return null;
  const label = accessibleLabel(element);
  return {
    blockId,
    blockLabel: block.dataset.cfBlockLabel ?? blockId,
    element_selector: {
      element_path: elementPath(block, element),
      tag_name: element.tagName.toLowerCase(),
      label,
      block_digest: blockDigest,
    },
    summary: `Element: ${label}`,
    excerptText: label,
  };
}

export function captureRegion(documentRoot: HTMLElement, start: Point, end: Point): CapturedTarget | null {
  const selected = normalizedRect(start, end);
  if (selected.width < 4 || selected.height < 4) return null;
  const blocks = [...documentRoot.querySelectorAll<HTMLElement>("[data-cf-block-id]")];
  const containing = blocks
    .filter((block) => containsRect(block.getBoundingClientRect(), selected))
    .sort((left, right) => rectArea(left.getBoundingClientRect()) - rectArea(right.getBoundingClientRect()))[0];
  const center = document.elementFromPoint(selected.left + selected.width / 2, selected.top + selected.height / 2);
  const attributed = containing ?? center?.closest<HTMLElement>("[data-cf-block-id]") ?? blocks.find((block) => intersects(block.getBoundingClientRect(), selected));
  const blockId = attributed?.dataset.cfBlockId;
  const blockDigest = attributed?.dataset.cfBlockDigest;
  if (!attributed || !blockId || !blockDigest) return null;

  const scope = containing ? "block" : "document";
  const coordinateRect = containing?.getBoundingClientRect() ?? documentRoot.getBoundingClientRect();
  const clipped = intersection(selected, coordinateRect);
  if (!clipped) return null;
  const x = (clipped.left - coordinateRect.left) / coordinateRect.width;
  const y = (clipped.top - coordinateRect.top) / coordinateRect.height;
  const width = clipped.width / coordinateRect.width;
  const height = clipped.height / coordinateRect.height;
  const selector: RegionSelector = {
    scope,
    anchor_id: scope === "block" ? blockId : "document",
    block_digest: blockDigest,
    x_ppm: ppm(x),
    y_ppm: ppm(y),
    width_ppm: Math.max(1, ppm(width)),
    height_ppm: Math.max(1, ppm(height)),
    capture_width_px: Math.max(1, Math.round(coordinateRect.width)),
    capture_height_px: Math.max(1, Math.round(coordinateRect.height)),
  };
  const regionText = intersectingVisibleText(documentRoot, selected);
  return {
    blockId,
    blockLabel: attributed.dataset.cfBlockLabel ?? blockId,
    region_selector: selector,
    summary: `Area: ${Math.round(clipped.width)}×${Math.round(clipped.height)} px in ${scope === "block" ? attributed.dataset.cfBlockLabel ?? blockId : "document"}`,
    ...(regionText ? { excerptText: regionText } : {}),
  };
}

export function captureDocument(documentRoot: HTMLElement): CapturedTarget | null {
  const block = documentRoot.querySelector<HTMLElement>("[data-cf-block-id]");
  const blockId = block?.dataset.cfBlockId;
  const blockDigest = block?.dataset.cfBlockDigest;
  const rect = documentRoot.getBoundingClientRect();
  if (!block || !blockId || !blockDigest || rect.width <= 0 || rect.height <= 0) return null;
  return {
    blockId,
    // The server validates the note's block identity and label together. The
    // region selector carries document scope; the note still retains the
    // first concrete block as its immutable digest authority.
    blockLabel: block.dataset.cfBlockLabel ?? blockId,
    region_selector: {
      scope: "document",
      anchor_id: "document",
      block_digest: blockDigest,
      x_ppm: 0,
      y_ppm: 0,
      width_ppm: REGION_SCALE,
      height_ppm: REGION_SCALE,
      capture_width_px: Math.round(rect.width),
      capture_height_px: Math.round(rect.height),
    },
    summary: "Whole document",
    excerptText: intersectingVisibleText(documentRoot, rect),
  };
}

export function resolveElement(documentRoot: HTMLElement, blockId: string, selector: ElementSelector): HTMLElement | null {
  const block = documentRoot.querySelector<HTMLElement>(`[data-cf-block-id="${CSS.escape(blockId)}"]`);
  if (!block || block.dataset.cfBlockDigest !== selector.block_digest) return null;
  if (selector.element_path === ":scope") return block;
  const element = block.querySelector<HTMLElement>(`:scope > ${selector.element_path}`);
  return element?.tagName.toLowerCase() === selector.tag_name ? element : null;
}

export function resolveRegion(documentRoot: HTMLElement, selector: RegionSelector): DOMRect | null {
  const anchor = selector.scope === "document"
    ? documentRoot
    : documentRoot.querySelector<HTMLElement>(`[data-cf-block-id="${CSS.escape(selector.anchor_id)}"]`);
  if (!anchor || anchor.getBoundingClientRect().width <= 0 || anchor.getBoundingClientRect().height <= 0) return null;
  const rect = anchor.getBoundingClientRect();
  return new DOMRect(
    rect.left + rect.width * selector.x_ppm / REGION_SCALE,
    rect.top + rect.height * selector.y_ppm / REGION_SCALE,
    rect.width * selector.width_ppm / REGION_SCALE,
    rect.height * selector.height_ppm / REGION_SCALE,
  );
}

export function selectorFromOffsets(text: string, start: number, end: number): TextSelector {
  if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start < 0 || end <= start || end > text.length) {
    throw new RangeError("Selection offsets are outside canonical review text");
  }
  return {
    start_utf16: start,
    end_utf16: end,
    exact: text.slice(start, end),
    prefix: text.slice(Math.max(0, start - CONTEXT_UNITS), start),
    suffix: text.slice(end, end + CONTEXT_UNITS),
  };
}

function elementPath(block: HTMLElement, element: HTMLElement): string {
  if (block === element) return ":scope";
  const segments: string[] = [];
  let current: HTMLElement | null = element;
  while (current && current !== block) {
    const parent: HTMLElement | null = current.parentElement;
    if (!parent) return ":scope";
    const tag = current.tagName.toLowerCase();
    const peers = [...parent.children].filter((child) => child.tagName === current?.tagName);
    segments.unshift(`${tag}:nth-of-type(${peers.indexOf(current) + 1})`);
    current = parent;
  }
  return segments.join(" > ");
}

function accessibleLabel(element: HTMLElement): string {
  const visible = visibleTextOf(element);
  if (visible) return truncate(visible, 2048);
  const aria = element.getAttribute("aria-label")
    ?? (element instanceof HTMLImageElement ? element.alt : "")
    ?? element.closest("figure[role='img'], [role='img'], figure")?.getAttribute("aria-label")
    ?? "";
  return truncate(aria.replace(/\s+/g, " ").trim() || element.tagName.toLowerCase(), 2048);
}

function normalizedRect(start: Point, end: Point): DOMRect {
  const left = Math.min(start.x, end.x);
  const top = Math.min(start.y, end.y);
  return new DOMRect(left, top, Math.abs(end.x - start.x), Math.abs(end.y - start.y));
}

function containsRect(outer: DOMRect, inner: DOMRect): boolean {
  return inner.left >= outer.left && inner.top >= outer.top && inner.right <= outer.right && inner.bottom <= outer.bottom;
}

function intersects(left: DOMRect, right: DOMRect): boolean {
  return left.left < right.right && left.right > right.left && left.top < right.bottom && left.bottom > right.top;
}

function intersection(left: DOMRect, right: DOMRect): DOMRect | null {
  const x = Math.max(left.left, right.left);
  const y = Math.max(left.top, right.top);
  const edgeX = Math.min(left.right, right.right);
  const edgeY = Math.min(left.bottom, right.bottom);
  return edgeX > x && edgeY > y ? new DOMRect(x, y, edgeX - x, edgeY - y) : null;
}

function rectArea(rect: DOMRect): number {
  return rect.width * rect.height;
}

function ppm(value: number): number {
  return Math.max(0, Math.min(REGION_SCALE, Math.round(value * REGION_SCALE)));
}

function truncate(value: string, limit: number): string {
  return value.length <= limit ? value : `${value.slice(0, Math.max(0, limit - 1))}…`;
}

function closestElement(node: Node, selector: string): HTMLElement | null {
  const element = node instanceof Element ? node : node.parentElement;
  return element?.closest<HTMLElement>(selector) ?? null;
}
