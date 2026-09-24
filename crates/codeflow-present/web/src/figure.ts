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
    const template = document.createElement("template");
    template.innerHTML = renderFigure(declaration, { idPrefix: `cf-present-figure-${++sequence}` });
    const unsafe = unsafeFigureNode(template.content);
    if (unsafe !== null) throw new Error(`the drawn figure carries ${unsafe}`);
    element.querySelector("[data-cf-figure-output]")?.replaceChildren(template.content);
    element.dataset.cfFigureBlock = "ready";
  } catch (error) {
    markFigureFailure(element, `The figure was not drawn: ${error instanceof Error ? error.message : String(error)}`);
  }
}

export function markFigureFailure(element: HTMLElement, message: string): void {
  element.dataset.cfFigureBlock = "failed";
  const status = element.querySelector<HTMLElement>("[data-cf-figure-status]");
  if (!status) return;
  status.classList.remove("sr-only");
  status.textContent = message;
}
