// The last check before a drawn figure enters the page: a closed allowlist of
// the elements, attributes and values the grammar module writes (renderFigure
// in figure-grammar.mjs), keyed by namespace. The module escapes everything it
// writes, so anything outside this list means the module and this guard
// disagree, and the figure is refused rather than inserted. A reference to a
// resource is allowed only as `url(#id)` naming a pattern the same figure
// defines; `aria-labelledby` must name the figure's own title and description.
// Families, shape classes and text styles come from the grammar module itself,
// so the two cannot drift apart; a state named on a mark or legend entry must
// be one its figure declares. Identifiers are held to their character set, not
// a length: the declaration envelope already bounds them.
import { ENTITY_ID, entityLabel, FAMILIES, SHAPE_CLASSES, TEXT_CLASSES } from "./figure-grammar.mjs";

const HTML = "http://www.w3.org/1999/xhtml";
const SVG = "http://www.w3.org/2000/svg";

type Check = RegExp | ((value: string) => boolean);

const NUMBER = /^-?\d+(?:\.\d+)?$/u;
const JS_NUMBER = /^-?\d+(?:\.\d+)?(?:e[+-]?\d+)?$/u;
const ID = /^[A-Za-z0-9_-]+$/u;
const IDS = /^[A-Za-z0-9_-]+(?: [A-Za-z0-9_-]+)*$/u;
const KEBAB = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/u;
const KEBABS = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*(?: [a-z][a-z0-9]*(?:-[a-z0-9]+)*)*$/u;
const DECLARED_STATE = "data-state";
const ENTITY = "data-cf-entity";
// An entity label is already collapsed and cut (SPC-014 B2), and never empty.
const ENTITY_LABEL = (value: string): boolean => value.length > 0 && entityLabel(value) === value && !/[\u0000-\u001f\u007f]/u.test(value);
const PATH_DATA = /^[MLHVCQZz0-9., -]{1,4096}$/u;
// The grammar sets no point count; the declaration envelope bounds it.
const POINTS = /^-?\d+(?:\.\d+)?,-?\d+(?:\.\d+)?(?: -?\d+(?:\.\d+)?,-?\d+(?:\.\d+)?)*$/u;
const VIEW_BOX = /^0 0 \d+(?:\.\d+)? \d+(?:\.\d+)?$/u;
const LOCAL_FILL = "fill";
const json = (shape: "array" | "object") => (value: string): boolean => {
  try {
    const parsed: unknown = JSON.parse(value);
    return shape === "array" ? Array.isArray(parsed) : parsed !== null && typeof parsed === "object" && !Array.isArray(parsed);
  } catch {
    return false;
  }
};
const exactly = (...values: readonly string[]) => (value: string): boolean => values.includes(value);
// cf-t, then each text style at most once.
const textClass = (value: string): boolean => {
  const [base, ...styles] = value.split(" ");
  return base === "cf-t" && new Set(styles).size === styles.length && styles.every((style) => TEXT_CLASSES.includes(style));
};

const SHAPE: Record<string, Check> = { class: exactly(...SHAPE_CLASSES), [LOCAL_FILL]: /^url\(#[A-Za-z0-9_-]+\)$/u };
const coordinates = (...names: string[]): Record<string, Check> => Object.fromEntries(names.map((name) => [name, NUMBER]));

const ALLOWED: Record<string, Record<string, Check>> = {
  [`${HTML} figure`]: {
    class: exactly("cf-fig"),
    "data-cf-figure": exactly(...FAMILIES),
    "data-cf-figure-id": KEBAB,
    "data-cf-binding": exactly("authored", "derived"),
    "data-cf-states": KEBABS,
    "data-cf-elongation-max": JS_NUMBER,
    // Written only when the declaration keeps the wide marks with a reason.
    "data-cf-same-marks": exactly("declared"),
    "data-cf-facts": json("array"),
    "data-cf-values": json("object"),
  },
  [`${HTML} p`]: { class: exactly("cf-fig-title", "cf-fig-description") },
  [`${HTML} span`]: { class: exactly("cf-fig-number", "cf-fig-name", "cf-fig-kicker") },
  [`${HTML} ul`]: { class: exactly("cf-legend"), "aria-label": exactly("Legend") },
  [`${HTML} li`]: { [DECLARED_STATE]: KEBAB, "data-cf-wide": exactly(""), [ENTITY]: ENTITY_ID, "data-cf-entity-label": ENTITY_LABEL },
  [`${HTML} figcaption`]: { class: exactly("cf-fig-caption") },
  [`${HTML} details`]: { class: exactly("cf-fig-details") },
  [`${HTML} summary`]: {},
  [`${HTML} div`]: { class: exactly("cf-twin-scroll") },
  [`${HTML} table`]: {},
  [`${HTML} thead`]: {},
  [`${HTML} tbody`]: {},
  [`${HTML} tr`]: {},
  [`${HTML} th`]: { scope: exactly("col") },
  [`${HTML} td`]: {},
  [`${HTML} code`]: {},
  [`${SVG} svg`]: {
    class: exactly("cf-fig-svg cf-fig-svg--wide", "cf-fig-svg cf-fig-svg--narrow", "cf-key"),
    viewBox: VIEW_BOX,
    role: exactly("img"),
    "aria-labelledby": /^[A-Za-z0-9_-]+-t [A-Za-z0-9_-]+-d$/u,
    "aria-hidden": exactly("true"),
    "data-cf-variant": exactly("wide", "narrow"),
  },
  [`${SVG} title`]: { id: ID },
  [`${SVG} desc`]: { id: ID },
  [`${SVG} defs`]: {},
  [`${SVG} pattern`]: {
    id: ID,
    width: exactly("4.5"),
    height: exactly("4.5"),
    patternUnits: exactly("userSpaceOnUse"),
    patternTransform: exactly("rotate(45)"),
  },
  [`${SVG} g`]: { [DECLARED_STATE]: KEBAB, id: ID, "data-cf-value": JS_NUMBER, [ENTITY]: ENTITY_ID, "data-cf-entity-label": ENTITY_LABEL },
  [`${SVG} text`]: {
    class: textClass,
    ...coordinates("x", "y"),
    "text-anchor": exactly("middle", "end"),
    "data-cf-for": IDS,
  },
  [`${SVG} line`]: { ...SHAPE, ...coordinates("x1", "y1", "x2", "y2") },
  [`${SVG} path`]: { ...SHAPE, d: PATH_DATA },
  [`${SVG} polyline`]: { ...SHAPE, points: POINTS },
  [`${SVG} rect`]: { ...SHAPE, ...coordinates("x", "y", "width", "height", "rx") },
  [`${SVG} circle`]: { ...SHAPE, ...coordinates("cx", "cy", "r") },
};

/** The first thing in `root` the grammar does not write, or null when it all is. */
export function unsafeFigureNode(root: ParentNode): string | null {
  const elements = [...root.querySelectorAll("*")];
  const ids = (namespace: string, name: string) =>
    new Set(elements.filter((node) => node.namespaceURI === namespace && node.localName === name).map((node) => node.id));
  const patterns = ids(SVG, "pattern");
  const labels = new Set([...ids(SVG, "title"), ...ids(SVG, "desc")]);
  const declared = (node: Element) => new Set((node.closest("figure")?.getAttribute("data-cf-states") ?? "").split(" "));
  for (const node of elements) {
    const tag = node.localName;
    const allowed = ALLOWED[`${node.namespaceURI ?? ""} ${tag}`];
    if (allowed === undefined) return `a <${tag}> element`;
    for (const attribute of node.attributes) {
      const name = attribute.localName;
      const check = attribute.namespaceURI === null && Object.hasOwn(allowed, name) ? allowed[name] : undefined;
      if (check === undefined) return `a ${attribute.name} attribute on <${tag}>`;
      const value = attribute.value;
      if (!(typeof check === "function" ? check(value) : check.test(value))) return `an unexpected ${name} value on <${tag}>`;
      if (name === LOCAL_FILL && !patterns.has(value.slice(5, -1))) return `a ${name} reference outside the figure on <${tag}>`;
      if (name === "aria-labelledby" && !value.split(" ").every((id) => labels.has(id))) return `an ${name} reference outside the figure on <${tag}>`;
      if (name === DECLARED_STATE && !declared(node).has(value)) return `a ${name} the figure does not declare on <${tag}>`;
      // A mark entity is the mark's own id; a legend entity names its state.
      if (name === ENTITY && value !== (tag === "li" ? `legend-${node.getAttribute(DECLARED_STATE) ?? ""}` : node.id.endsWith(`-${value}`) ? value : null)) return `a ${name} that names another element on <${tag}>`;
      if (name === ENTITY && !node.hasAttribute("data-cf-entity-label")) return `an entity without its label on <${tag}>`;
    }
  }
  return null;
}
