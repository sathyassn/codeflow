import type { TextSelector } from "./contracts";

const CONTEXT_UNITS = 32;

export function captureSelection(documentRoot: HTMLElement): {
  blockId: string;
  blockLabel: string;
  selector: TextSelector;
} | null {
  const selection = getSelection();
  if (!selection || selection.rangeCount !== 1 || selection.isCollapsed) return null;
  const range = selection.getRangeAt(0);
  const startElement = closestElement(range.startContainer, "[data-cf-review-text-root]");
  const endElement = closestElement(range.endContainer, "[data-cf-review-text-root]");
  if (!startElement || startElement !== endElement || !documentRoot.contains(startElement)) {
    return null;
  }
  const block = startElement.closest<HTMLElement>("[data-cf-block-id]");
  const blockId = block?.dataset.cfBlockId;
  if (!block || !blockId) return null;
  if (block.matches(".block--diagram")) return null;

  const before = document.createRange();
  before.selectNodeContents(startElement);
  before.setEnd(range.startContainer, range.startOffset);
  const start = before.toString().length;
  const exact = range.toString();
  const canonical = startElement.textContent ?? "";
  const end = start + exact.length;
  if (!exact || canonical.slice(start, end) !== exact) return null;

  return {
    blockId,
    blockLabel: block.dataset.cfBlockLabel ?? blockId,
    selector: selectorFromOffsets(canonical, start, end),
  };
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

function closestElement(node: Node, selector: string): HTMLElement | null {
  const element = node instanceof Element ? node : node.parentElement;
  return element?.closest<HTMLElement>(selector) ?? null;
}
