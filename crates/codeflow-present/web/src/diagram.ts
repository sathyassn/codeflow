const MAX_DIAGRAM_SOURCE_UNITS = 65_536;
const MAX_DIAGRAM_EDGES = 500;
let sequence = 0;

export async function renderDiagram(element: HTMLElement): Promise<void> {
  const source = element.querySelector<HTMLTemplateElement>("template[data-cf-diagram-source]")?.content.textContent ?? "";
  const title = element.dataset.cfDiagramTitle?.trim() ?? "";
  const description = element.dataset.cfDiagramDescription?.trim() ?? "";
  if (!source || source.length > MAX_DIAGRAM_SOURCE_UNITS || !title || !description) {
    markFailure(element, "Diagram source or accessible description is invalid.");
    return;
  }
  if (/%%\s*\{\s*init\s*:|\bclick\s+|\bhref\s+/iu.test(source)) {
    markFailure(element, "This diagram uses a disabled directive.");
    return;
  }

  try {
    const { default: mermaid } = await import("mermaid");
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      maxTextSize: MAX_DIAGRAM_SOURCE_UNITS,
      maxEdges: MAX_DIAGRAM_EDGES,
      theme: "base",
      suppressErrorRendering: true,
      flowchart: { htmlLabels: false, useMaxWidth: true },
      themeVariables: diagramThemeVariables(),
    });
    const id = `cf-present-diagram-${++sequence}`;
    const rendered = await mermaid.render(id, source);
    const parser = new DOMParser();
    const svgDocument = parser.parseFromString(rendered.svg, "image/svg+xml");
    const svg = svgDocument.documentElement;
    if (svg.tagName.toLowerCase() !== "svg" || svg.querySelector("parsererror")) {
      throw new Error("Mermaid did not return a valid SVG");
    }
    hardenSvg(svg);
    const titleId = `${id}-title`;
    const descriptionId = `${id}-description`;
    const titleNode = svgDocument.createElementNS("http://www.w3.org/2000/svg", "title");
    titleNode.id = titleId;
    titleNode.textContent = title;
    const descriptionNode = svgDocument.createElementNS("http://www.w3.org/2000/svg", "desc");
    descriptionNode.id = descriptionId;
    descriptionNode.textContent = description;
    svg.prepend(descriptionNode);
    svg.prepend(titleNode);
    svg.setAttribute("role", "img");
    svg.setAttribute("aria-labelledby", `${titleId} ${descriptionId}`);
    const imported = document.importNode(svg, true);
    element.querySelector("[data-cf-diagram-output]")?.replaceChildren(imported);
    element.dataset.cfDiagram = "ready";
  } catch {
    markFailure(element, "The diagram could not be rendered. Its source remains available.");
  }
}

function diagramThemeVariables(): Record<string, string> {
  const styles = getComputedStyle(document.documentElement);
  const token = (name: string, fallback: string): string => styles.getPropertyValue(name).trim() || fallback;
  return {
    background: token("--cf-surface", "#ffffff"),
    primaryColor: token("--cf-surface-raised", "#f5f6f8"),
    primaryTextColor: token("--cf-text", "#17191f"),
    primaryBorderColor: token("--cf-border-strong", "#687080"),
    lineColor: token("--cf-diagram-line", "#4f596b"),
    secondaryColor: token("--cf-accent-soft", "#e5eefb"),
    tertiaryColor: token("--cf-surface-subtle", "#eef0f3"),
    fontFamily: token("--cf-font-sans", "system-ui, sans-serif"),
  };
}

function hardenSvg(svg: Element): void {
  svg.querySelectorAll("script, foreignObject, a").forEach((node) => {
    if (node.tagName.toLowerCase() === "a") node.replaceWith(...node.childNodes);
    else node.remove();
  });
  for (const node of [svg, ...svg.querySelectorAll("*")]) {
    if (node.tagName.toLowerCase() === "style") {
      if (!safeCss(node.textContent ?? "")) {
        node.remove();
        continue;
      }
    }
    for (const attribute of [...node.attributes]) {
      const name = attribute.name.toLowerCase();
      const value = attribute.value.trim().toLowerCase();
      if (
        name.startsWith("on") ||
        name === "href" ||
        name.endsWith(":href") ||
        /(?:https?:|data:|file:|javascript:)/u.test(value) ||
        (/url\(/u.test(value) && !/url\(\s*["']?#/u.test(value))
      ) {
        node.removeAttribute(attribute.name);
      }
    }
    if (node.hasAttribute("style") && !safeCss(node.getAttribute("style") ?? "")) {
      node.removeAttribute("style");
    }
  }
}

function safeCss(value: string): boolean {
  return !/(?:@import|https?:|data:|file:|javascript:|expression\s*\(|behavior\s*:|-moz-binding|url\(\s*["']?(?!#))/iu.test(value);
}

function markFailure(element: HTMLElement, message: string): void {
  element.dataset.cfDiagram = "failed";
  const status = element.querySelector<HTMLElement>("[data-cf-diagram-status]");
  if (status) status.textContent = message;
}
