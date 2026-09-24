// The figure block: the service renders a placeholder carrying the
// declaration, and this module draws it with the grammar module the docs
// portal runs. `figure-grammar.mjs` is a byte copy of
// docs-portal/scripts/figure-grammar.mjs, pinned by a test, because the
// esbuild entry reaches only web/src.
import { renderFigure, validateDeclaration } from "./figure-grammar.mjs";

// The grammar module escapes every declared string and number it writes, so
// its output carries only these elements. Anything else means the module and
// this check disagree, and the figure is refused rather than inserted.
const DRAWN_ELEMENTS = new Set([
  "figure", "span", "svg", "title", "desc", "defs", "pattern", "g", "text", "line", "path", "polyline", "rect", "circle",
  "ul", "li", "figcaption", "details", "summary", "div", "table", "thead", "tbody", "tr", "th", "td", "code",
]);
let sequence = 0;

export function renderFigureBlock(element: HTMLElement): void {
  try {
    const declaration: unknown = JSON.parse(element.dataset.cfFigureDeclaration ?? "");
    validateDeclaration(declaration);
    const template = document.createElement("template");
    template.innerHTML = renderFigure(declaration, { idPrefix: `cf-present-figure-${++sequence}` });
    const unsafe = unsafeNode(template.content);
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

function unsafeNode(root: DocumentFragment): string | null {
  for (const node of root.querySelectorAll("*")) {
    const tag = node.localName;
    if (!DRAWN_ELEMENTS.has(tag)) return `a <${tag}> element`;
    for (const attribute of node.attributes) {
      const name = attribute.name.toLowerCase();
      const value = attribute.value.trim().toLowerCase();
      if (name.startsWith("on") || name === "style" || name.endsWith("href")) return `a ${name} attribute`;
      if (value.includes("url(") && !/^url\(#[a-z0-9_.:-]+\)$/u.test(value)) return `a resource reference in ${name}`;
    }
  }
  return null;
}
