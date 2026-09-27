import type { ElementSelector, EntitySelector, RegionSelector, TextSelector } from "./contracts";
// Explicit .ts: check.mjs loads this module under Node type stripping, which
// does not resolve extensionless relative value imports.
import { intersectingVisibleText, quoteFromRange, visibleTextOf } from "./excerpt.ts";

const CONTEXT_UNITS = 32;
const REGION_SCALE = 1_000_000;
const ANNOTATABLE = [
  "h1", "h2", "h3", "p", "li", "blockquote", "pre", "code", "table", "thead", "tbody", "tr", "th", "td",
  "figure", "figcaption", "img", "video", "audio", "svg", "details", "summary", "article", "aside",
].join(",");

/**
 * The review resolver (SPC-014 B3). One resolver serves hover, click, touch,
 * keyboard and saved anchors. For a gesture it returns the first that applies:
 * a `data-cf-for` label (its first entity), the innermost group or entity
 * outside a `none` subtree, a visible shape or element labelled by the
 * nearest labelled ancestor, then the block. Text selections and regions are
 * captured before and on request by the callers. Chrome, `defs`, markers,
 * `title`, `desc` and hidden elements are never targets, and a point within
 * 6 CSS px of a line, polyline or path counts as a hit on it.
 */
const SHAPES = ["g", "path", "line", "polyline", "polygon", "rect", "circle", "ellipse", "text"];
const TARGETS = `${ANNOTATABLE},${SHAPES.join(",")}`;
const ENTITY = "[data-cf-entity], [data-cf-group], [data-cf-target]:not([data-cf-target='none'])";
const NONE = "[data-cf-entity-none], [data-cf-target='none']";
const EXCLUDED = "defs, marker, title, desc, clipPath, mask, pattern, symbol, style, script";
const CHROME = ".cf-marker, .cf-marker-layer, button[data-anchor-block]";
const STROKES = "line, polyline, path";
export const STROKE_PADDING_PX = 6;
const LABEL_LIMIT = 120;

export type ResolvedVia = "label" | "entity" | "shape" | "block";

export interface Resolution {
  readonly block: HTMLElement;
  /** The element the reviewer means: highlighted, anchored and cropped. */
  readonly element: Element;
  readonly via: ResolvedVia;
  readonly label: string;
  readonly entityId?: string;
}

/** Every keyboard stop in document order: entities and annotatable elements that show. */
export function annotatableElements(documentRoot: HTMLElement): readonly Element[] {
  return [...documentRoot.querySelectorAll(`${ANNOTATABLE},${ENTITY}`)]
    .filter((element) => {
      const block = element.closest("[data-cf-block-id]");
      return block && documentRoot.contains(block) && !element.closest(CHROME) && !element.closest(EXCLUDED)
        && !element.closest(NONE) && !insideGroup(element) && isShown(element);
    });
}

/** The element a hover highlight or click pin resolves to. */
export function annotatableAncestor(documentRoot: HTMLElement, target: Element, point?: Point): Element | null {
  return resolveTarget(documentRoot, target, point)?.element ?? null;
}

export function resolveTarget(documentRoot: HTMLElement, raw: Element, point?: Point): Resolution | null {
  if (raw.closest(CHROME)) return null;
  const block = raw.closest<HTMLElement>("[data-cf-block-id]");
  if (!block || !documentRoot.contains(block)) return null;
  // A hit inside a non-rendering subtree stands for the element around it.
  const start = outermost(raw, EXCLUDED)?.parentElement ?? raw;
  if (!block.contains(start)) return blockResolution(block);
  const direct = resolveFrom(block, start);
  if (!point || direct.via === "label" || direct.via === "entity") return direct;
  const weak = direct.via === "block" || (direct.via === "shape" && isSvgRoot(direct.element));
  const stroke = nearestStroke(block, start, point);
  if (!stroke) return direct;
  const viaStroke = resolveFrom(block, stroke);
  if (viaStroke.via === "label" || viaStroke.via === "entity" || weak) return viaStroke;
  return direct;
}

/** "Select enclosing": the entity around this one, then the block. */
export function enclosingTarget(documentRoot: HTMLElement, element: Element): Resolution | null {
  const block = element.closest<HTMLElement>("[data-cf-block-id]");
  if (!block || !documentRoot.contains(block) || element === block) return null;
  const outer = element.parentElement?.closest(ENTITY);
  if (outer && block.contains(outer) && outer !== block && !outer.closest(NONE)) return entityResolution(block, outer);
  return blockResolution(block);
}

function resolveFrom(block: HTMLElement, element: Element): Resolution {
  const none = element.closest(NONE);
  if (none && block.contains(none)) {
    const outer = none.parentElement?.closest(ENTITY);
    return outer && block.contains(outer) && outer !== block ? entityResolution(block, outer) : blockResolution(block);
  }
  const label = element.closest("[data-cf-for]");
  if (label && block.contains(label)) {
    const first = (label.getAttribute("data-cf-for") ?? "").split(/\s+/u).find(Boolean);
    const entity = first ? entityNamed(block, first) : null;
    if (entity) return { ...entityResolution(block, entity), via: "label" };
  }
  const entity = element.closest(ENTITY);
  if (entity && block.contains(entity) && entity !== block) return entityResolution(block, entity);
  const shape = element.closest(TARGETS);
  if (!shape || shape === block || !block.contains(shape)) return blockResolution(block);
  const own = ownLabel(block, shape);
  if (own) return { block, element: shape, via: "shape", label: own };
  // A shape inside a labelled group resolves to the nearest labelled group.
  for (let group = shape.parentElement?.closest("g"); group && block.contains(group); group = group.parentElement?.closest("g")) {
    const named = ownLabel(block, group);
    if (named) return { block, element: group, via: "shape", label: named };
  }
  // Other ancestors lend only an explicit label: the text of a whole drawing
  // or frame would name everything in it.
  for (let ancestor = shape.parentElement; ancestor && ancestor !== block; ancestor = ancestor.parentElement) {
    const named = ownLabel(block, ancestor, false);
    if (named) return { block, element: shape, via: "shape", label: named };
  }
  return { block, element: shape, via: "shape", label: blockLabel(block) };
}

function entityResolution(block: HTMLElement, entity: Element): Resolution {
  const id = entityIdOf(entity);
  return { block, element: entity, via: "entity", label: entityLabelOf(block, entity, id), ...(id ? { entityId: id } : {}) };
}

function blockResolution(block: HTMLElement): Resolution {
  return { block, element: block, via: "block", label: blockLabel(block) };
}

function blockLabel(block: HTMLElement): string {
  return block.dataset.cfBlockLabel ?? block.dataset.cfBlockId ?? "Block";
}

function entityIdOf(entity: Element): string | undefined {
  return entity.getAttribute("data-cf-entity") ?? entity.getAttribute("data-cf-group") ?? entity.getAttribute("data-cf-target") ?? undefined;
}

// A `data-cf-for` token names an entity id, or for a drawn figure the mark's
// element id (the grammar prefixes it); either way the entity is the element
// that carries the entity attributes.
function entityNamed(block: HTMLElement, token: string): Element | null {
  const escaped = CSS.escape(token);
  const byEntity = block.querySelector(`[data-cf-entity="${escaped}"], [data-cf-group="${escaped}"], [data-cf-target="${escaped}"]`);
  if (byEntity) return byEntity;
  const byId = block.querySelector(`[id="${escaped}"]`);
  return byId?.matches(ENTITY) ? byId : null;
}

/** The label of B2: the service's label when it sent one, else the same order. */
function entityLabelOf(block: HTMLElement, entity: Element, id: string | undefined): string {
  const served = entity.getAttribute("data-cf-entity-label");
  if (served) return collapseLabel(served);
  return ownLabel(block, entity) || collapseLabel(id ?? "") || blockLabel(block);
}

// B2 order without the id fallback: data-cf-label, the labels that name the
// element, aria-label, then its own visible text. Never a tag name.
function ownLabel(block: HTMLElement, element: Element, withText = true): string {
  const authored = element.getAttribute("data-cf-label");
  if (authored?.trim()) return collapseLabel(authored);
  const names = [entityIdOf(element), element.id || undefined].filter((value): value is string => Boolean(value));
  if (names.length) {
    const labels = [...block.querySelectorAll("[data-cf-for]")]
      .filter((label) => (label.getAttribute("data-cf-for") ?? "").split(/\s+/u).some((token) => names.includes(token)))
      .map((label) => label.textContent ?? "");
    const joined = collapseLabel(labels.join(" "));
    if (joined) return joined;
  }
  const aria = element.getAttribute("aria-label");
  if (aria?.trim()) return collapseLabel(aria);
  if (element instanceof HTMLImageElement && element.alt.trim()) return collapseLabel(element.alt);
  if (element === block || !withText) return "";
  return collapseLabel(visibleTextOf(element));
}

/** SPC-014 B2: whitespace runs collapse to one space; cut to 120 characters. */
export function collapseLabel(value: string): string {
  return [...value.replace(/[\t\n\f\r ]+/gu, " ").replace(/^ | $/gu, "")].slice(0, LABEL_LIMIT).join("");
}

function outermost(element: Element, selector: string): Element | null {
  let found: Element | null = null;
  for (let node = element.closest(selector); node; node = node.parentElement?.closest(selector) ?? null) found = node;
  return found;
}

function insideGroup(element: Element): boolean {
  return Boolean(element.parentElement?.closest("[data-cf-group]"));
}

function isSvgRoot(element: Element): boolean {
  return element.localName === "svg" && element.namespaceURI === "http://www.w3.org/2000/svg";
}

export function isShown(element: Element): boolean {
  if (element.closest(EXCLUDED) || element.getClientRects().length === 0) return false;
  const style = getComputedStyle(element);
  return style.display !== "none" && style.visibility !== "hidden";
}

// The nearest line, polyline or path of the SVG under the point, measured to
// its segments in screen space, when it lies within the stroke padding.
function nearestStroke(block: HTMLElement, raw: Element, point: Point): Element | null {
  const svg = raw.closest("svg");
  if (!svg || !block.contains(svg)) return null;
  let best: { element: Element; distance: number } | null = null;
  for (const candidate of svg.querySelectorAll<SVGGeometryElement>(STROKES)) {
    if (candidate.closest(".cf-key") || candidate.closest(NONE) || !isShown(candidate)) continue;
    const box = candidate.getBoundingClientRect();
    if (point.x < box.left - STROKE_PADDING_PX || point.x > box.right + STROKE_PADDING_PX
      || point.y < box.top - STROKE_PADDING_PX || point.y > box.bottom + STROKE_PADDING_PX) continue;
    const distance = distanceToOutline(candidate, point);
    if (distance <= STROKE_PADDING_PX && (!best || distance < best.distance)) best = { element: candidate, distance };
  }
  return best?.element ?? null;
}

function distanceToOutline(element: SVGGeometryElement, point: Point): number {
  const matrix = element.getScreenCTM();
  if (!matrix) return Infinity;
  const screen = (x: number, y: number): Point => ({ x: matrix.a * x + matrix.c * y + matrix.e, y: matrix.b * x + matrix.d * y + matrix.f });
  let points: Point[];
  if (element instanceof SVGLineElement) {
    points = [screen(element.x1.baseVal.value, element.y1.baseVal.value), screen(element.x2.baseVal.value, element.y2.baseVal.value)];
  } else if (element instanceof SVGPolylineElement) {
    points = [...element.points].map((item) => screen(item.x, item.y));
  } else {
    // Curves are followed at about 2 CSS px steps, at most 400 samples.
    const length = element.getTotalLength();
    const scale = Math.hypot(matrix.a, matrix.b) || 1;
    const count = Math.min(400, Math.max(2, Math.ceil((length * scale) / 2)));
    points = Array.from({ length: count + 1 }, (_, step) => {
      const at = element.getPointAtLength((length * step) / count);
      return screen(at.x, at.y);
    });
  }
  let nearest = Infinity;
  for (let index = 1; index < points.length; index += 1) {
    nearest = Math.min(nearest, segmentDistance(point, points[index - 1]!, points[index]!));
  }
  return points.length === 1 ? Math.hypot(point.x - points[0]!.x, point.y - points[0]!.y) : nearest;
}

export function segmentDistance(point: Point, from: Point, to: Point): number {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const length = dx * dx + dy * dy;
  const t = length === 0 ? 0 : Math.max(0, Math.min(1, ((point.x - from.x) * dx + (point.y - from.y) * dy) / length));
  return Math.hypot(point.x - (from.x + t * dx), point.y - (from.y + t * dy));
}

// Figure and table framing (SPC-014 B5): the title line, legend, caption and
// Details sit outside a block's review text; they take element notes
// labelled by their text, never text selections.
const FRAME = ".cf-frame-title, .cf-frame-caption, .cf-frame-details, .cf-legend, .cf-fig-title, .cf-fig-caption, .cf-fig-details";
const TEXTUAL_TAGS = /^(H1|H2|H3|H4|P|LI|PRE|CODE|TD|TH|LABEL|A|EM|STRONG|SMALL|BLOCKQUOTE|SPAN)$/;
const PROSE_SELECTOR = "p, h1, h2, h3, h4, li, pre, td, th, blockquote, figcaption";

/**
 * Words the reviewer can highlight, including SVG `<text>` / `<tspan>` on a
 * stage. Shapes, media, and empty canvas stay non-prose so a box-drag is a
 * region and a click is an element. A blanket "anything in svg/figure is
 * non-prose" rule made stage labels select with no Text chip.
 */
export function isTextualTarget(target: Element): boolean {
  if (target.closest(FRAME)) return false;
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
  /** The entity anchor without its crop box, which the crop adds. */
  readonly entity_selector?: Omit<EntitySelector, "crop_box">;
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
  if (closestElement(range.startContainer, FRAME) || closestElement(range.endContainer, FRAME)) return null;
  const block = startElement.closest<HTMLElement>("[data-cf-block-id]");
  const blockId = block?.dataset.cfBlockId;
  if (!block || !blockId) return null;

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

/** An element note on whatever the resolver picks under a gesture. */
export function captureElement(documentRoot: HTMLElement, rawTarget: Element, point?: Point): CapturedTarget | null {
  const resolution = resolveTarget(documentRoot, rawTarget, point);
  return resolution ? captureResolution(resolution) : null;
}

export function captureResolution(resolution: Resolution): CapturedTarget | null {
  const { block, element } = resolution;
  const blockId = block.dataset.cfBlockId;
  const blockDigest = block.dataset.cfBlockDigest;
  if (!blockId || !blockDigest) return null;
  // HTML prose keeps its full visible text as the element label, as in v1;
  // a drawing part carries the resolver's label.
  const prose = resolution.via === "shape" && element instanceof HTMLElement ? truncate(visibleTextOf(element), 2048) : "";
  const label = prose || resolution.label;
  const variant = element.closest("svg[data-cf-variant]")?.getAttribute("data-cf-variant");
  return {
    blockId,
    blockLabel: block.dataset.cfBlockLabel ?? blockId,
    element_selector: {
      element_path: elementPath(block, element),
      tag_name: element.localName,
      label,
      block_digest: blockDigest,
    },
    ...(resolution.entityId ? {
      entity_selector: {
        entity_id: resolution.entityId,
        label: resolution.label,
        block_digest: blockDigest,
        ...(variant === "wide" || variant === "narrow" ? { variant } : {}),
      },
    } : {}),
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

export function resolveElement(documentRoot: HTMLElement, blockId: string, selector: ElementSelector): Element | null {
  const block = documentRoot.querySelector<HTMLElement>(`[data-cf-block-id="${CSS.escape(blockId)}"]`);
  if (!block || block.dataset.cfBlockDigest !== selector.block_digest) return null;
  if (selector.element_path === ":scope") return block;
  const element = block.querySelector(`:scope > ${selector.element_path}`);
  return element?.localName === selector.tag_name ? element : null;
}

/** The shown element of an entity: a figure draws each mark in two compositions, one visible at a time. */
export function resolveEntity(documentRoot: HTMLElement, blockId: string, entityId: string): Element | null {
  const block = documentRoot.querySelector<HTMLElement>(`[data-cf-block-id="${CSS.escape(blockId)}"]`);
  if (!block) return null;
  const escaped = CSS.escape(entityId);
  return [...block.querySelectorAll(`[data-cf-entity="${escaped}"], [data-cf-group="${escaped}"], [data-cf-target="${escaped}"]`)].find(isShown) ?? null;
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

function elementPath(block: HTMLElement, element: Element): string {
  if (block === element) return ":scope";
  const segments: string[] = [];
  let current: Element | null = element;
  while (current && current !== block) {
    const parent: Element | null = current.parentElement;
    if (!parent) return ":scope";
    const tag = current.localName;
    const peers = [...parent.children].filter((child) => child.localName === tag);
    segments.unshift(`${tag}:nth-of-type(${peers.indexOf(current) + 1})`);
    current = parent;
  }
  return segments.join(" > ");
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
