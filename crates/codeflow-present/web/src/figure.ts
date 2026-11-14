// The figure block: the service renders a placeholder carrying the
// declaration, and this module draws it with the grammar module the docs
// portal runs. `figure-grammar.mjs` is a byte copy of
// docs-portal/scripts/figure-grammar.mjs, pinned by a test, because the
// esbuild entry reaches only web/src.
import { renderFigure, validateDeclaration } from "./figure-grammar.mjs";
import { unsafeFigureNode } from "./figure-guard";

let sequence = 0;

export function renderFigureBlock(element: HTMLElement): void {
  try {
    const declaration: unknown = JSON.parse(element.dataset.cfFigureDeclaration ?? "");
    validateDeclaration(declaration);
    const number = figureNumber(element.dataset.cfFigureNumber);
    const template = document.createElement("template");
    template.innerHTML = renderFigure(declaration, { idPrefix: `cf-present-figure-${++sequence}`, number });
    const unsafe = unsafeFigureNode(template.content);
    if (unsafe !== null) throw new Error(`the drawn figure carries ${unsafe}`);
    // The service owns the block text. Match every rendered occurrence to
    // its declared UTF-16 position, including repeated wide/narrow labels.
    const root = element.closest("[data-cf-review-text-root]");
    const canonical = root?.getAttribute("data-cf-canonical-text") ?? "";
    const figure = template.content.querySelector("figure");
    if (!figure) throw new Error("the figure has no text container");
    const title = figure.querySelector(".cf-fig-name")?.textContent ?? "";
    const caption = figure.querySelector(".cf-fig-caption")?.textContent ?? "";
    let prefix = title;
    if (caption) {
      if (prefix && !prefix.endsWith("\n")) prefix += "\n";
      prefix += caption;
    }
    const words: Element[] = [...figure.querySelectorAll("svg[data-cf-variant] text")];
    for (const entry of figure.querySelectorAll(".cf-legend li")) {
      // Keep the legend's drawing outside its text root: a drag starting on
      // a key still draws a region; a click on its words finds the entity.
      const span = document.createElement("span");
      for (const child of [...entry.childNodes]) if (child.nodeType === Node.TEXT_NODE) span.append(child);
      entry.append(span);
      words.push(span);
    }
    for (const word of words) {
      const text = word.textContent ?? "";
      if (!text) continue;
      if (prefix && !prefix.endsWith("\n")) prefix += "\n";
      const base = prefix.length;
      prefix += text;
      word.setAttribute("data-cf-review-text-root", "");
      word.setAttribute("data-cf-review-text-base", String(base));
    }
    if (prefix !== canonical) throw new Error("the figure text differs from the service review text");
    element.querySelector("[data-cf-figure-output]")?.replaceChildren(template.content);
    element.dataset.cfFigureBlock = "ready";
  } catch (error) {
    markFigureFailure(element, `The figure was not drawn: ${error instanceof Error ? error.message : String(error)}`);
  }
}

// The number the service gives the figure in its document (SPC-014 B5), or
// null for a document that numbers no figures. A malformed number is a
// server and client disagreement, so the figure is refused, not unnumbered.
function figureNumber(value: string | undefined): number | null {
  if (value === undefined) return null;
  if (!/^[1-9][0-9]{0,5}$/u.test(value)) throw new Error(`the figure number ${JSON.stringify(value)} is not a positive whole number`);
  return Number(value);
}

export function markFigureFailure(element: HTMLElement, message: string): void {
  element.dataset.cfFigureBlock = "failed";
  const status = element.querySelector<HTMLElement>("[data-cf-figure-status]");
  if (!status) return;
  status.classList.remove("sr-only");
  status.textContent = message;
}
