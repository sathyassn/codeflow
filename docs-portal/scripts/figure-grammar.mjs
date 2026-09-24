// The figure grammar runtime (figure-grammar.md, ADR-0068). One module, shared
// byte for byte by the portal adapter (Node) and the present review surface
// (browser, crates/codeflow-present/web/src/figure-grammar.mjs). It validates a
// figure declaration, draws it with the mark classes of the design kit's
// figure.css, keys the legend from the drawn set, writes the accessible
// description and the table twin, recomposes the narrow variant, and declares
// the thresholds and rule checks the figure gate applies to the render.
//
// No DOM is needed to draw: the output is an HTML string, so the adapter can
// write it into a page and present can place it into a placeholder. Every
// colour, face and rule comes from the kit classes; the module writes no
// colour, no inline style and no author markup.

export const GRAMMAR_VERSION = 1;

export const FAMILIES = Object.freeze(["flow", "structure", "layering", "sequence", "state", "coverage", "extent", "derivation", "graph"]);

// Thresholds are declared once, here and in the token sheet (rule 4 text floor).
export const THRESHOLDS = Object.freeze({
  textFloorPx: 12.5,
  markFloorPx: 9,
  narrowBreakPx: 646,
  elongationMax: 1.5,
  labelClearancePx: 8,
  overprintDepthPx: 1,
  widthChannelPx: 0.5,
  minChannels: 2,
  wideMaxWidth: 720,
  narrowMaxWidth: 368,
});

// The twelve rules, numbered as figure-grammar.md section 2 numbers them.
export const FIGURE_RULES = Object.freeze({
  1: "one governing idea",
  2: "every mark keyed",
  3: "two channels, never hue alone",
  4: "legible at render size",
  5: "narrow recomposes with its own mark set",
  6: "declared facts derived from source",
  7: "no text boxes as the primary form",
  8: "no text over text or over a mark",
  9: "one-sentence caption",
  10: "token-only colour",
  11: "a description",
  12: "a table twin",
});

// The closed mark vocabulary (figure-grammar.md section 4). Each mark names
// the kit class that draws it and the shapes it may take. Part classes for
// heads, caps, crosses and ticks are drawn inside the same state group, so
// they are an overlay channel measured off the render.
const LINE = Object.freeze(["path", "line", "polyline"]);
const ROUND = Object.freeze(["circle"]);
const SOLID = Object.freeze(["rect"]);
export const MARKS = Object.freeze({
  done: { className: "cf-m-done", shapes: LINE, key: "line" },
  todo: { className: "cf-m-todo", shapes: LINE, key: "line" },
  blocked: { className: "cf-m-blocked", shapes: LINE, key: "line" },
  warn: { className: "cf-m-warn", shapes: LINE, key: "line" },
  stop: { className: "cf-m-stop", shapes: LINE, key: "bar-v" },
  limit: { className: "cf-m-limit", shapes: LINE, key: "bar-v" },
  trans: { className: "cf-m-trans", shapes: LINE, key: "line", head: "cf-m-trans-head" },
  return: { className: "cf-m-return", shapes: LINE, key: "line", head: "cf-m-return-head" },
  human: { className: "cf-m-human", shapes: ROUND, key: "ring" },
  node: { className: "cf-m-node", shapes: ROUND, key: "ring" },
  agent: { className: "cf-m-agent", shapes: ROUND, key: "disc" },
  act: { className: "cf-m-act", shapes: ROUND, key: "disc" },
  merge: { className: "cf-m-merge", shapes: Object.freeze(["diamond", "path"]), key: "diamond" },
  cross: { className: "cf-m-cross", shapes: Object.freeze(["cross"]), key: "cross" },
  layer: { className: "cf-m-layer", shapes: SOLID, key: "bar" },
  "layer-remote": { className: "cf-m-layer--remote", shapes: SOLID, key: "bar" },
  used: { className: "cf-m-used", shapes: SOLID, key: "bar" },
  state: { className: "cf-m-state", shapes: SOLID, key: "box" },
  optional: { className: "cf-m-optional", shapes: Object.freeze(["rect", "path", "line", "polyline"]), key: "box" },
  denied: { className: "cf-m-denied", shapes: SOLID, key: "box" },
  cov: { className: "cf-m-cov", shapes: SOLID, key: "cell" },
  part: { className: "cf-m-part", shapes: SOLID, key: "cell" },
  notrun: { className: "cf-m-notrun", shapes: SOLID, key: "cell", hatch: true },
  na: { className: "cf-m-na", shapes: SOLID, key: "cell" },
  nc: { className: "cf-m-nc", shapes: SOLID, key: "cell", cross: "cf-m-nc-cross" },
});

// The rendered channels rule 3 counts. Paint kind is read as two channels,
// the interior and the edge, so a solid disc, a hollow ring, a faint outline
// and a hatch are told apart without reading a hue.
export const CHANNELS = Object.freeze(["interior", "edge", "width", "dash", "shape", "overlay"]);

// The closed set of fact checks (rule 6). Each re-derives one value from a
// committed source so a wrong fact fails even when its anchor exists. The
// portal adapter and the Rust validator implement the same three.
export const FACT_CHECKS = Object.freeze(["contains", "count-items", "json"]);

const DECORATIONS = Object.freeze({ rule: "cf-f-rule", axis: "cf-f-axis", tick: "cf-f-tick" });
const TEXT_STYLES = Object.freeze({ strong: "cf-t--strong", mute: "cf-t--mute", head: "cf-t--head", mono: "cf-t--mono" });
// Every class the drawing writes on a shape, and every class a text adds to
// cf-t: the closed vocabulary present's DOM guard accepts. The part classes
// are the literals markParts and legendKey write; a test pins them.
const PART_CLASSES = Object.freeze(["cf-m-trans-head", "cf-m-state", "cf-m-cap", "cf-m-cross", "cf-m-done", "cf-m-hatchline"]);
export const SHAPE_CLASSES = Object.freeze([...new Set([
  ...Object.values(MARKS).flatMap((mark) => [mark.className, mark.head, mark.cross].filter((value) => value !== undefined)),
  ...Object.values(DECORATIONS),
  ...PART_CLASSES,
])]);
export const TEXT_CLASSES = Object.freeze(Object.values(TEXT_STYLES));
const RECOMPOSE = Object.freeze(["rotate", "stack", "strip", "list"]);
const LIMITS = Object.freeze({ states: 24, draw: 600, facts: 32, twinRows: 64, twinColumns: 8, text: 400, coordinate: 4000 });
const KEBAB = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/;

export class FigureDeclarationError extends Error {
  constructor(problems, context = "figure declaration") {
    super(`${context}: ${problems.join("; ")}`);
    this.problems = problems;
  }
}

// ---------------------------------------------------------------------------
// Declaration validation
// ---------------------------------------------------------------------------

export function validateDeclaration(value, context = "figure declaration") {
  const problems = [];
  const fail = (message) => { problems.push(message); };
  if (!isObject(value) || value.schema_version !== 1 || !isObject(value.figure)) throw new FigureDeclarationError(["expected { schema_version: 1, figure: { ... } }"], context);
  onlyKeys(value, ["schema_version", "figure"], "declaration", fail);
  const figure = value.figure;
  onlyKeys(figure, ["id", "family", "binding", "question", "idea", "title", "kicker", "description", "caption", "states", "facts", "wide", "narrow", "twin", "source", "layout"], "figure", fail);
  if (!KEBAB.test(figure.id ?? "") || figure.id.length > 64) fail("id must be kebab-case, at most 64 characters");
  if (!FAMILIES.includes(figure.family)) fail(`family must be one of ${FAMILIES.join(", ")}`);
  if (!["authored", "derived"].includes(figure.binding)) fail("binding must be authored or derived");
  for (const field of ["question", "idea", "title", "caption"]) text(figure[field], field, fail);
  if (figure.kicker !== undefined) text(figure.kicker, "kicker", fail);
  if (figure.description !== undefined) text(figure.description, "description", fail, 2000);
  if (typeof figure.caption === "string" && !oneSentence(figure.caption)) fail("rule 9: the caption must be exactly one sentence ending in a full stop");
  if (typeof figure.caption === "string" && typeof figure.title === "string" && figure.caption.trim().toLowerCase() === figure.title.trim().toLowerCase()) fail("rule 9: the caption repeats the title");
  const states = Array.isArray(figure.states) ? figure.states : [];
  if (!Array.isArray(figure.states) || states.length === 0 || states.length > LIMITS.states) fail(`states must list 1 to ${LIMITS.states} drawn states`);
  const stateNames = new Map();
  for (const state of states) {
    if (!isObject(state)) { fail("each state must be an object"); continue; }
    onlyKeys(state, ["name", "mark", "means", "channels"], `state ${state.name}`, fail);
    if (!KEBAB.test(state.name ?? "")) fail(`state name ${state.name} must be kebab-case`);
    if (stateNames.has(state.name)) fail(`duplicate state ${state.name}`);
    stateNames.set(state.name, state.mark);
    if (!Object.hasOwn(MARKS, state.mark)) fail(`state ${state.name} draws unknown mark ${state.mark}`);
    text(state.means, `state ${state.name} means`, fail, 120);
    if (state.channels !== undefined && (!Array.isArray(state.channels) || state.channels.some((channel) => !CHANNELS.includes(channel)))) fail(`state ${state.name} channels must name ${CHANNELS.join(", ")}`);
  }
  const facts = Array.isArray(figure.facts) ? figure.facts : [];
  if (!Array.isArray(figure.facts) || facts.length === 0 || facts.length > LIMITS.facts) fail(`rule 6: facts must list 1 to ${LIMITS.facts} facts`);
  facts.forEach((fact, index) => validateFact(fact, index, fail));
  if (figure.binding === "derived") {
    if (!isObject(figure.source)) fail("a derived figure needs source { path, select }");
    else {
      onlyKeys(figure.source, ["path", "select"], "source", fail);
      if (!safePath(figure.source.path) || !figure.source.path.endsWith(".json")) fail("source.path must be a repository-relative JSON file");
      if (!validSelector(figure.source.select)) fail("source.select must be a dotted selector");
    }
    if (!isObject(figure.layout)) fail("a derived figure draws from a layout");
  } else if (figure.source !== undefined) fail("source is forbidden in the authored binding");
  if (figure.layout !== undefined) validateLayout(figure, fail);
  if (figure.layout === undefined) {
    validateComposition(figure.wide, "wide", stateNames, THRESHOLDS.wideMaxWidth, fail);
  } else if (figure.wide !== undefined) fail("a figure with a layout draws its wide composition from the layout");
  if (!isObject(figure.narrow)) fail("rule 5: narrow must declare the recomposition");
  else {
    onlyKeys(figure.narrow, ["recompose", "drops", "marks", "elongation_max", "reason", "width", "height", "draw"], "narrow", fail);
    if (!RECOMPOSE.includes(figure.narrow.recompose)) fail(`narrow.recompose must be one of ${RECOMPOSE.join(", ")}`);
    if (!Array.isArray(figure.narrow.drops) || figure.narrow.drops.some((name) => !stateNames.has(name))) fail("narrow.drops must list declared states");
    if (figure.narrow.marks !== "same" && !(Array.isArray(figure.narrow.marks) && figure.narrow.marks.every((name) => stateNames.has(name)))) fail("narrow.marks must be same or list declared states");
    if (figure.narrow.elongation_max !== undefined) {
      if (typeof figure.narrow.elongation_max !== "number" || figure.narrow.elongation_max < 1 || figure.narrow.elongation_max > 4) fail("narrow.elongation_max must be a number from 1 to 4");
      if (figure.narrow.elongation_max > THRESHOLDS.elongationMax && typeof figure.narrow.reason !== "string") fail("rule 5: a ceiling above the default needs a reason");
    }
    if (figure.layout === undefined) validateComposition(figure.narrow, "narrow", stateNames, THRESHOLDS.narrowMaxWidth, fail);
    else if (figure.narrow.draw !== undefined) fail("a figure with a layout draws its narrow composition from the layout");
  }
  validateTwin(figure.twin, fail);
  if (problems.length) throw new FigureDeclarationError(problems, context);
  return value;
}

function validateFact(fact, index, fail) {
  const where = `fact ${index + 1}`;
  if (!isObject(fact)) { fail(`${where} must be an object`); return; }
  onlyKeys(fact, ["claim", "source", "derive", "check", "value"], where, fail);
  text(fact.claim, `${where} claim`, fail, 300);
  text(fact.derive, `${where} derive`, fail, 300);
  const source = parseFactSource(fact.source);
  if (source === null) fail(`${where} source must be a repository path with an optional #anchor`);
  if (!isObject(fact.check) || !FACT_CHECKS.includes(fact.check.kind)) { fail(`${where} check.kind must be one of ${FACT_CHECKS.join(", ")}`); return; }
  if (fact.check.kind === "contains") {
    onlyKeys(fact.check, ["kind", "text"], `${where} check`, fail);
    text(fact.check.text, `${where} check.text`, fail, 300);
    if (fact.value !== true) fail(`${where}: a contains check asserts value true`);
  } else if (fact.check.kind === "count-items") {
    onlyKeys(fact.check, ["kind"], `${where} check`, fail);
    if (!Number.isSafeInteger(fact.value) || fact.value < 0) fail(`${where}: a count-items check asserts a whole number`);
  } else {
    onlyKeys(fact.check, ["kind", "select"], `${where} check`, fail);
    if (!validSelector(fact.check.select)) fail(`${where} check.select must be a dotted selector`);
    if (source !== null && (source.anchor !== null || !source.path.endsWith(".json"))) fail(`${where}: a json check reads a JSON file with no anchor`);
    if (fact.value === undefined) fail(`${where}: a json check asserts a value`);
  }
  if (source !== null && fact.check.kind !== "json" && source.path.endsWith(".json")) fail(`${where}: a JSON source takes the json check`);
}

function validateLayout(figure, fail) {
  const layout = figure.layout;
  if (!isObject(layout) || !["extent", "coverage"].includes(layout.kind)) { fail("layout.kind must be extent or coverage"); return; }
  if (layout.kind === "extent") {
    if (figure.family !== "extent") fail("the extent layout draws the extent family");
    onlyKeys(layout, ["kind", "max", "unit", "rows", "limits"], "layout", fail);
    if (!(typeof layout.max === "number" && layout.max > 0 && layout.max <= 1e6)) fail("layout.max must be a positive number");
    if (layout.unit !== undefined) text(layout.unit, "layout.unit", fail, 40);
    if (!Array.isArray(layout.rows) || layout.rows.length < 1 || layout.rows.length > 16) fail("layout.rows must list 1 to 16 rows");
    for (const row of layout.rows ?? []) {
      if (!isObject(row)) { fail("each layout row must be an object"); continue; }
      onlyKeys(row, ["label", "value", "state"], "layout row", fail);
      text(row.label, "layout row label", fail, 60);
      if (!(typeof row.value === "number" || validSelector(row.value))) fail("layout row value must be a number or, in the derived binding, a selector into the source");
      if (typeof row.value === "string" && figure.binding !== "derived") fail("a selector value needs the derived binding");
      if (!figure.states?.some?.((state) => state.name === row.state)) fail(`layout row state ${row.state} is not declared`);
    }
    for (const limit of layout.limits ?? []) {
      if (!isObject(limit)) { fail("each layout limit must be an object"); continue; }
      onlyKeys(limit, ["label", "value", "state"], "layout limit", fail);
      text(limit.label, "layout limit label", fail, 60);
      if (!(typeof limit.value === "number" || validSelector(limit.value))) fail("layout limit value must be a number or selector");
      if (!figure.states?.some?.((state) => state.name === limit.state)) fail(`layout limit state ${limit.state} is not declared`);
    }
  } else {
    if (figure.family !== "coverage") fail("the coverage layout draws the coverage family");
    onlyKeys(layout, ["kind", "columns", "rows"], "layout", fail);
    if (!Array.isArray(layout.columns) || layout.columns.length < 1 || layout.columns.length > 8) fail("layout.columns must list 1 to 8 columns");
    for (const column of layout.columns ?? []) text(column, "layout column", fail, 24);
    if (!Array.isArray(layout.rows) || layout.rows.length < 1 || layout.rows.length > 16) fail("layout.rows must list 1 to 16 rows");
    for (const row of layout.rows ?? []) {
      if (!isObject(row) || !Array.isArray(row.cells) || row.cells.length !== (layout.columns ?? []).length) { fail("each coverage row needs one cell per column"); continue; }
      onlyKeys(row, ["label", "cells"], "coverage row", fail);
      text(row.label, "coverage row label", fail, 40);
      for (const cell of row.cells) if (!figure.states?.some?.((state) => state.name === cell)) fail(`coverage cell ${cell} is not a declared state`);
    }
  }
}

function validateComposition(composition, name, stateNames, maxWidth, fail) {
  if (!isObject(composition)) { fail(`${name} must declare width, height and draw`); return; }
  if (!(Number.isFinite(composition.width) && composition.width >= 120 && composition.width <= maxWidth)) fail(`${name}.width must be 120 to ${maxWidth} units`);
  if (!(Number.isFinite(composition.height) && composition.height >= 40 && composition.height <= 2400)) fail(`${name}.height must be 40 to 2400 units`);
  if (!Array.isArray(composition.draw) || composition.draw.length === 0 || composition.draw.length > LIMITS.draw) { fail(`${name}.draw must list 1 to ${LIMITS.draw} items`); return; }
  const ids = new Set();
  const references = [];
  for (const [index, item] of composition.draw.entries()) {
    const where = `${name}.draw[${index}]`;
    if (!isObject(item)) { fail(`${where} must be an object`); continue; }
    const kinds = ["state", "deco", "text"].filter((key) => item[key] !== undefined);
    if (kinds.length !== 1) { fail(`${where} must be exactly one of a state mark, a decoration or a text`); continue; }
    if (item.id !== undefined) {
      if (!KEBAB.test(item.id) || ids.has(item.id)) fail(`${where} id must be a unique kebab-case name`);
      ids.add(item.id);
    }
    if (kinds[0] === "text") {
      onlyKeys(item, ["text", "x", "y", "anchor", "style", "for"], where, fail);
      text(item.text, `${where} text`, fail, 120);
      coordinates(item, ["x", "y"], where, fail);
      if (item.anchor !== undefined && !["start", "middle", "end"].includes(item.anchor)) fail(`${where} anchor must be start, middle or end`);
      const styles = item.style === undefined ? [] : Array.isArray(item.style) ? item.style : [item.style];
      if (styles.some((style) => !Object.hasOwn(TEXT_STYLES, style)) || new Set(styles).size !== styles.length) fail(`${where} style must be distinct entries among ${Object.keys(TEXT_STYLES).join(", ")}`);
      if (item.for !== undefined) {
        if (!Array.isArray(item.for) || item.for.some((id) => typeof id !== "string")) fail(`${where} for must list mark ids`);
        else references.push(...item.for.map((id) => [where, id]));
      }
      continue;
    }
    if (kinds[0] === "deco") {
      if (!Object.hasOwn(DECORATIONS, item.deco)) fail(`${where} deco must be rule, axis or tick`);
      validateShape(item, ["path", "line", "polyline", "rect"], where, fail, ["deco", "id"]);
      continue;
    }
    if (!stateNames.has(item.state)) { fail(`${where} draws undeclared state ${item.state}`); continue; }
    const mark = MARKS[stateNames.get(item.state)];
    if (mark !== undefined) validateShape(item, mark.shapes, where, fail, ["state", "id"]);
  }
  for (const [where, id] of references) if (!ids.has(id)) fail(`${where} labels unknown mark ${id}`);
}

function validateShape(item, shapes, where, fail, extraKeys) {
  const shape = item.shape;
  if (!shapes.includes(shape)) { fail(`${where} shape must be one of ${shapes.join(", ")}`); return; }
  const geometry = { path: ["d"], line: ["x1", "y1", "x2", "y2"], polyline: ["points"], rect: ["x", "y", "w", "h", "rx"], circle: ["cx", "cy", "r"], diamond: ["cx", "cy", "r"], cross: ["cx", "cy", "size"] }[shape];
  const parts = ["head", "open", "square", "cap", "cross", "tick"];
  onlyKeys(item, ["shape", ...geometry, ...extraKeys, ...parts], where, fail);
  if (shape === "path") {
    if (typeof item.d !== "string" || item.d.length > 4000 || !/^[MLHVCQZmlhvcqz0-9.,\s-]+$/.test(item.d) || !/^\s*M/.test(item.d)) fail(`${where} d must be an absolute path of M, L, H, V, C, Q and Z commands`);
    else if (/[mlhvcqs]/.test(item.d.replace(/[zZ]/g, ""))) fail(`${where} d must use absolute commands`);
  } else if (shape === "polyline") {
    if (!Array.isArray(item.points) || item.points.length < 2 || item.points.some((point) => !Array.isArray(point) || point.length !== 2 || !point.every(finiteCoordinate))) fail(`${where} points must list at least two [x, y] pairs`);
  } else {
    coordinates(item, geometry.filter((key) => key !== "rx"), where, fail);
    if (item.rx !== undefined && !(Number.isFinite(item.rx) && item.rx >= 0 && item.rx <= 40)) fail(`${where} rx must be 0 to 40`);
  }
  for (const part of ["head", "open", "square"]) if (item[part] !== undefined && !["start", "end", "both"].includes(item[part])) fail(`${where} ${part} must be start, end or both`);
  if (item.cap !== undefined && item.cap !== "end") fail(`${where} cap must be end`);
  if (item.cross !== undefined && item.cross !== true && !(isObject(item.cross) && ["cx", "cy", "size"].every((key) => finiteCoordinate(item.cross[key])))) fail(`${where} cross must be true or { cx, cy, size }`);
  if (item.tick !== undefined && item.tick !== true) fail(`${where} tick must be true`);
}

function validateTwin(twin, fail) {
  if (twin === "facts") return;
  if (!isObject(twin)) { fail("rule 12: twin must be facts or { columns, rows }"); return; }
  onlyKeys(twin, ["columns", "rows"], "twin", fail);
  if (!Array.isArray(twin.columns) || twin.columns.length < 1 || twin.columns.length > LIMITS.twinColumns) fail(`twin.columns must list 1 to ${LIMITS.twinColumns} columns`);
  if (!Array.isArray(twin.rows) || twin.rows.length < 1 || twin.rows.length > LIMITS.twinRows) fail(`rule 12: twin.rows must list 1 to ${LIMITS.twinRows} rows`);
  for (const row of twin.rows ?? []) {
    if (!Array.isArray(row) || row.length !== (twin.columns ?? []).length || row.some((cell) => typeof cell !== "string" || cell.length > 400)) fail("each twin row needs one text cell per column");
  }
  for (const column of twin.columns ?? []) text(column, "twin column", fail, 60);
}

// ---------------------------------------------------------------------------
// Facts (rule 6)
// ---------------------------------------------------------------------------

export function parseFactSource(value) {
  if (typeof value !== "string" || value.length > 512) return null;
  const hash = value.indexOf("#");
  const filePath = hash < 0 ? value : value.slice(0, hash);
  const anchor = hash < 0 ? null : value.slice(hash + 1);
  if (!safePath(filePath)) return null;
  if (anchor !== null && !/^[\p{L}\p{N}_-]+$/u.test(anchor)) return null;
  return { path: filePath, anchor };
}

// The anchor of each ATX heading outside fenced code, slugged the way the
// portal slugs a rendered heading. Duplicates take -1, -2 and so on.
export function markdownSections(text) {
  const lines = String(text).replace(/^﻿/, "").replace(/\r\n?/g, "\n").split("\n");
  const headings = [];
  const seen = new Map();
  let fence = null;
  lines.forEach((line, index) => {
    const marker = line.match(/^ {0,3}(`{3,}|~{3,})(.*)$/);
    if (fence !== null) {
      if (marker && marker[1][0] === fence.char && marker[1].length >= fence.length && marker[2].trim() === "") fence = null;
      return;
    }
    if (marker && !(marker[1][0] === "`" && marker[2].includes("`"))) { fence = { char: marker[1][0], length: marker[1].length }; return; }
    const heading = line.match(/^ {0,3}(#{1,6})(?:[ \t]+(.*?))?[ \t]*$/);
    if (!heading) return;
    const level = heading[1].length;
    const raw = (heading[2] ?? "").replace(/[ \t]+#+[ \t]*$/, "").replace(/^#+$/, "");
    const base = slugHeading(raw);
    const count = seen.get(base) ?? 0;
    seen.set(base, count + 1);
    headings.push({ line: index, level, text: raw, anchor: count === 0 ? base : `${base}-${count}` });
  });
  return headings.map((heading, position) => {
    const next = headings.slice(position + 1).find((candidate) => candidate.level <= heading.level);
    return { ...heading, body: lines.slice(heading.line + 1, next === undefined ? lines.length : next.line).join("\n") };
  });
}

export function slugHeading(raw) {
  const visible = String(raw)
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[`*_~]/g, (character) => (character === "_" ? "_" : ""))
    .replace(/<[^>]*>/g, "");
  return [...visible.trim().toLowerCase()].filter((character) => /[\p{L}\p{N}\p{M}_\- ]/u.test(character)).join("").replace(/ /g, "-");
}

// One value per fact, re-derived from the committed bytes of its source.
export function deriveFact(fact, readSource) {
  const source = parseFactSource(fact.source);
  if (source === null) throw new Error(`fact source is invalid: ${fact.source}`);
  const bytes = readSource(source.path);
  if (bytes === null || bytes === undefined) throw new Error(`fact source does not exist: ${source.path}`);
  const text = typeof bytes === "string" ? bytes : new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  if (fact.check.kind === "json") return selectJson(JSON.parse(text), fact.check.select, source.path);
  let scope = text.replace(/^﻿/, "").replace(/\r\n?/g, "\n");
  if (source.anchor !== null) {
    const section = markdownSections(scope).find((candidate) => candidate.anchor === source.anchor);
    if (section === undefined) throw new Error(`fact anchor does not exist: ${fact.source}`);
    scope = section.body;
  }
  if (fact.check.kind === "contains") return scope.includes(fact.check.text);
  return scope.split("\n").filter((line) => /^(?:[-*+]|\d{1,9}[.)])[ \t]/.test(line)).length;
}

export function selectJson(value, selector, where = "source") {
  let cursor = value;
  for (const segment of selector.split(".")) {
    if (Array.isArray(cursor) && /^\d+$/.test(segment)) cursor = cursor[Number(segment)];
    else if (isObject(cursor) && Object.hasOwn(cursor, segment)) cursor = cursor[segment];
    else throw new Error(`${where}: selector ${selector} does not resolve`);
    if (cursor === undefined) throw new Error(`${where}: selector ${selector} does not resolve`);
  }
  return cursor;
}

export function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (isObject(value)) return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(",")}}`;
  return JSON.stringify(value);
}

// Every fact re-derived and compared with the value the figure asserts. The
// result is what the evidence manifest records and the validator re-checks.
export function checkFacts(figure, readSource) {
  return figure.facts.map((fact) => {
    let derived;
    let error = null;
    try { derived = deriveFact(fact, readSource); } catch (caught) { error = caught.message; derived = null; }
    const matches = error === null && canonicalJson(derived) === canonicalJson(fact.value);
    return { claim: fact.claim, source: fact.source, check: fact.check, drawn: fact.value, derived, matches, error };
  });
}

// The derived binding: the data model a layout draws from, read out of the
// named source. Row values that are selectors resolve inside source.select.
export function bindDerivedData(figure, readSource) {
  if (figure.binding !== "derived") return null;
  const bytes = readSource(figure.source.path);
  if (bytes === null || bytes === undefined) throw new Error(`derived source does not exist: ${figure.source.path}`);
  const text = typeof bytes === "string" ? bytes : new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  const model = selectJson(JSON.parse(text), figure.source.select, figure.source.path);
  const values = {};
  for (const entry of [...(figure.layout.rows ?? []), ...(figure.layout.limits ?? [])]) {
    if (typeof entry.value !== "string") continue;
    const value = selectJson(model, entry.value, `${figure.source.path} ${figure.source.select}`);
    if (typeof value !== "number" || !Number.isFinite(value)) throw new Error(`derived value ${entry.value} is not a number`);
    values[entry.value] = value;
  }
  return { source: { path: figure.source.path, select: figure.source.select }, derived: values };
}

// ---------------------------------------------------------------------------
// Scales and layouts
// ---------------------------------------------------------------------------

export function linearScale(domain, range) {
  const [d0, d1] = domain;
  const [r0, r1] = range;
  const span = d1 - d0;
  const scale = (value) => (span === 0 ? r0 : r0 + ((value - d0) / span) * (r1 - r0));
  scale.invert = (position) => (r1 === r0 ? d0 : d0 + ((position - r0) / (r1 - r0)) * span);
  scale.domain = domain;
  scale.range = range;
  return scale;
}

export function bandScale(count, [start, end], padding = 0) {
  const step = count <= 0 ? 0 : (end - start) / count;
  return (index) => start + step * index + step * padding;
}

// Space between a label and a mark it does not label, in drawing units: the
// rule 8 clearance at the smallest scale a composition renders at.
const LABEL_GAP = 12;

function layoutCompositions(figure, bound) {
  const value = (entry) => (typeof entry.value === "number" ? entry.value : bound.derived[entry.value]);
  if (figure.layout.kind === "extent") {
    const rows = figure.layout.rows.map((row) => ({ ...row, number: value(row) }));
    const limits = (figure.layout.limits ?? []).map((limit) => ({ ...limit, number: value(limit) }));
    const unit = figure.layout.unit ? ` ${figure.layout.unit}` : "";
    const drawnValues = {};
    const compose = (width, narrow) => {
      const labelWidth = narrow ? 0 : 150;
      const left = labelWidth;
      const right = width - 40;
      const scale = linearScale([0, figure.layout.max], [left, right]);
      const draw = [];
      const rowHeight = narrow ? 58 : 40;
      const top = 30;
      rows.forEach((row, index) => {
        const y = top + index * rowHeight + (narrow ? 22 : 0);
        const labelY = narrow ? y - 8 : y + 10;
        const length = scale(row.number) - left;
        const id = `row-${index}`;
        draw.push({ text: `${row.label}`, x: narrow ? left : 0, y: labelY, style: "strong", for: [id] });
        draw.push({ state: row.state, shape: "rect", x: left, y, w: round(length), h: 12, rx: 3, id, value: row.number });
        if (typeof row.value === "string") drawnValues[row.value] = round(scale.invert(left + round(length)), 0.0001);
        // The value sits past the bar and clear of any limit line it would
        // otherwise touch, so it never reads as that limit's label.
        const valueText = `${formatNumber(row.number)}${unit}`;
        const valueWidth = valueText.length * 8.5;
        let valueX = left + length + LABEL_GAP;
        for (const limitX of limits.map((limit) => scale(limit.number)).sort((a, b) => a - b)) {
          if (limitX > valueX - LABEL_GAP && limitX < valueX + valueWidth + LABEL_GAP) valueX = limitX + LABEL_GAP;
        }
        draw.push({ text: valueText, x: round(valueX), y: y + 11, style: ["mono", "mute"], for: [id] });
      });
      const axisY = top + rows.length * rowHeight + (narrow ? 22 : 4);
      draw.push({ deco: "axis", shape: "line", x1: left, y1: axisY, x2: right, y2: axisY });
      limits.forEach((limit, index) => {
        const x = round(scale(limit.number));
        const id = `limit-${index}`;
        draw.push({ state: limit.state, shape: "line", x1: x, y1: top - 6, x2: x, y2: axisY, id, value: limit.number });
        if (typeof limit.value === "string") drawnValues[limit.value] = round(scale.invert(x), 0.0001);
        draw.push({ text: `${limit.label} ${formatNumber(limit.number)}`, x, y: axisY + 22 + (narrow ? index * 20 : 0), anchor: narrow ? "end" : "middle", style: "mute", for: [id] });
      });
      const height = axisY + 30 + (narrow ? Math.max(0, limits.length - 1) * 20 : 0);
      // The scale travels with the composition, so the gate can read each
      // rendered mark back through it (rule 6).
      return { width, height, draw, scale: { domain: scale.domain, range: scale.range } };
    };
    const wide = compose(THRESHOLDS.wideMaxWidth, false);
    const narrow = compose(360, true);
    return { wide, narrow, drawnValues };
  }
  const { columns, rows } = figure.layout;
  const cell = 18;
  const wideDraw = [];
  const labelWidth = 240;
  const colStep = Math.min(80, (THRESHOLDS.wideMaxWidth - labelWidth) / columns.length);
  columns.forEach((column, index) => wideDraw.push({ text: column, x: round(labelWidth + colStep * index + colStep / 2), y: 24, anchor: "middle", style: "head" }));
  rows.forEach((row, rowIndex) => {
    const y = 48 + rowIndex * 36;
    wideDraw.push({ text: row.label, x: 0, y: y + 14 });
    row.cells.forEach((state, columnIndex) => wideDraw.push({ state, shape: "rect", x: round(labelWidth + colStep * columnIndex + colStep / 2 - cell / 2), y, w: cell, h: cell, rx: 2 }));
  });
  const narrowDraw = [];
  rows.forEach((row, rowIndex) => {
    const y = 20 + rowIndex * 64;
    narrowDraw.push({ text: row.label, x: 0, y, style: "strong" });
    const step = Math.min(88, 360 / columns.length);
    row.cells.forEach((state, columnIndex) => {
      const x = columnIndex * step;
      narrowDraw.push({ state, shape: "rect", x, y: y + 14, w: 16, h: 16, rx: 2 });
      narrowDraw.push({ text: columns[columnIndex], x: x + 22, y: y + 27, style: "mute" });
    });
  });
  return {
    wide: { width: THRESHOLDS.wideMaxWidth, height: 48 + rows.length * 36 + 4, draw: wideDraw },
    narrow: { width: 360, height: 20 + rows.length * 64, draw: narrowDraw },
    drawnValues: null,
  };
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

// The figure as HTML: kicker, the wide and the narrow SVG, the legend keyed
// from the drawn set, the caption and the table twin. `bound` is the derived
// data model, or null for an authored figure.
// The two compositions a figure draws and, for a layout, the values its
// marks encode, read back through the scale that placed them.
export function composeFigure(declaration, bound = null) {
  const figure = declaration.figure;
  if (figure.layout === undefined) return { wide: figure.wide, narrow: figure.narrow, drawnValues: null };
  if (figure.binding === "derived" && bound === null) throw new Error(`${figure.id}: a derived figure needs its bound data`);
  const composed = layoutCompositions(figure, bound ?? { derived: {} });
  return { wide: composed.wide, narrow: { ...figure.narrow, ...composed.narrow }, drawnValues: composed.drawnValues };
}

export function renderFigure(declaration, { idPrefix = "cf-fig", bound = null, facts = null } = {}) {
  const figure = declaration.figure;
  const prefix = `${sanitizeId(idPrefix)}-${figure.id}`;
  const { wide, narrow, drawnValues } = composeFigure(declaration, bound);
  const states = new Map(figure.states.map((state) => [state.name, state]));
  const wideDrawn = drawnStates(wide);
  const narrowDrawn = drawnStates(narrow);
  const declared = figure.states.map((state) => state.name);
  const union = new Set([...wideDrawn, ...narrowDrawn]);
  const unkeyed = declared.filter((name) => !union.has(name));
  if (unkeyed.length) throw new Error(`${figure.id}: rule 2: declared states are never drawn: ${unkeyed.join(", ")}`);
  const dropped = declared.filter((name) => wideDrawn.has(name) && !narrowDrawn.has(name));
  if (dropped.join(" ") !== [...figure.narrow.drops].sort((a, b) => declared.indexOf(a) - declared.indexOf(b)).join(" ")) {
    throw new Error(`${figure.id}: rule 5: the narrow composition drops ${dropped.join(", ") || "nothing"} but declares drops ${figure.narrow.drops.join(", ") || "none"}`);
  }
  const description = describe(figure);
  const title = figure.title;
  const factValues = (facts ?? figure.facts.map((fact) => ({ claim: fact.claim, drawn: fact.value }))).map((fact) => ({ claim: fact.claim, value: fact.drawn }));
  const attributes = [
    ["class", "cf-fig"],
    ["data-cf-figure", figure.family],
    ["data-cf-figure-id", figure.id],
    ["data-cf-binding", figure.binding],
    ["data-cf-states", declared.join(" ")],
    ["data-cf-elongation-max", String(figure.narrow.elongation_max ?? THRESHOLDS.elongationMax)],
    ["data-cf-facts", JSON.stringify(factValues)],
    ...(drawnValues === null ? [] : [["data-cf-values", JSON.stringify(drawnValues)]]),
  ].map(([name, value]) => `${name}="${escapeAttribute(value)}"`).join(" ");
  const svg = (composition, variant) => {
    const id = `${prefix}-${variant}`;
    const defs = composition.draw.some((item) => item.state !== undefined && states.get(item.state).mark === "notrun")
      ? `<defs><pattern id="${id}-hatch" width="4.5" height="4.5" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><line class="cf-m-hatchline" x1="0" y1="0" x2="0" y2="4.5"/></pattern></defs>`
      : "";
    const body = composition.draw.map((item) => drawItem(item, states, `${id}-hatch`, `${id}-`)).join("");
    return `<svg class="cf-fig-svg cf-fig-svg--${variant}" viewBox="0 0 ${num(composition.width)} ${num(composition.height)}" role="img" aria-labelledby="${id}-t ${id}-d" data-cf-variant="${variant}"><title id="${id}-t">${escapeText(title)}</title><desc id="${id}-d">${escapeText(description)}</desc>${defs}${body}</svg>`;
  };
  const legend = declared.map((name) => {
    const state = states.get(name);
    const wideOnly = wideDrawn.has(name) && !narrowDrawn.has(name);
    const sample = [...wide.draw, ...narrow.draw].find((item) => item.state === name);
    return `<li data-state="${escapeAttribute(name)}"${wideOnly ? " data-cf-wide" : ""}>${legendKey(state, sample, `${prefix}-key-${name}`)}${escapeText(state.means)}</li>`;
  }).join("");
  const kicker = figure.kicker ?? title;
  return `<figure ${attributes}><span class="cf-fig-kicker">Figure · ${escapeText(kicker)}</span>${svg(wide, "wide")}${svg(narrow, "narrow")}<ul class="cf-legend" aria-label="Legend">${legend}</ul><figcaption class="cf-fig-caption">${escapeText(figure.caption)}</figcaption>${twinTable(figure, factValues)}</figure>`;
}

// The accessible description names every state and states every fact, so it
// is generated from the declaration and follows the drawn set.
export function describe(figure) {
  const lead = figure.description ?? figure.idea;
  const key = figure.states.map((state) => `${state.means} (${state.name})`).join("; ");
  const facts = figure.facts.map((fact) => fact.claim).join("; ");
  return `${lead} Key: ${key}. Facts: ${facts}.`;
}

function drawnStates(composition) {
  return new Set(composition.draw.filter((item) => item.state !== undefined).map((item) => item.state));
}

function drawItem(item, states, hatchId, idPrefix) {
  if (item.text !== undefined) {
    const styles = item.style === undefined ? [] : Array.isArray(item.style) ? item.style : [item.style];
    const classes = ["cf-t", ...styles.map((style) => TEXT_STYLES[style])].join(" ");
    const anchor = item.anchor && item.anchor !== "start" ? ` text-anchor="${item.anchor}"` : "";
    const labels = item.for?.length ? ` data-cf-for="${escapeAttribute(item.for.map((id) => `${idPrefix}${id}`).join(" "))}"` : "";
    return `<text class="${classes}" x="${num(item.x)}" y="${num(item.y)}"${anchor}${labels}>${escapeText(item.text)}</text>`;
  }
  if (item.deco !== undefined) return shapeElement(item, DECORATIONS[item.deco], "");
  const state = states.get(item.state);
  const mark = MARKS[state.mark];
  if (!mark.shapes.includes(item.shape)) throw new Error(`state ${item.state} draws the ${state.mark} mark, which takes ${mark.shapes.join(", ")}, not ${item.shape}`);
  const id = item.id === undefined ? "" : ` id="${escapeAttribute(`${idPrefix}${item.id}`)}"`;
  const extra = mark.hatch ? ` fill="url(#${hatchId})"` : "";
  const value = item.value === undefined ? "" : ` data-cf-value="${escapeAttribute(String(item.value))}"`;
  const primary = shapeElement(item, mark.className, extra);
  const parts = markParts(item, state.mark, mark);
  return `<g data-state="${escapeAttribute(item.state)}"${id}${value}>${primary}${parts}</g>`;
}

function shapeElement(item, className, extra) {
  const cls = `class="${className}"`;
  switch (item.shape) {
    case "path": return `<path ${cls} d="${normalizePath(item.d)}"${extra}/>`;
    case "line": return `<line ${cls} x1="${num(item.x1)}" y1="${num(item.y1)}" x2="${num(item.x2)}" y2="${num(item.y2)}"${extra}/>`;
    case "polyline": return `<polyline ${cls} points="${item.points.map(([x, y]) => `${num(x)},${num(y)}`).join(" ")}"${extra}/>`;
    case "rect": return `<rect ${cls} x="${num(item.x)}" y="${num(item.y)}" width="${num(item.w)}" height="${num(item.h)}"${item.rx === undefined ? "" : ` rx="${num(item.rx)}"`}${extra}/>`;
    case "circle": return `<circle ${cls} cx="${num(item.cx)}" cy="${num(item.cy)}" r="${num(item.r)}"${extra}/>`;
    case "diamond": {
      const { cx, cy, r } = item;
      return `<path ${cls} d="M${num(cx)} ${num(cy - r)}L${num(cx + r)} ${num(cy)}L${num(cx)} ${num(cy + r)}L${num(cx - r)} ${num(cy)}Z"${extra}/>`;
    }
    case "cross": return `<path ${cls} d="${crossPath(item.cx, item.cy, item.size)}"${extra}/>`;
    default: throw new Error(`unknown shape ${item.shape}`);
  }
}

function markParts(item, markName, mark) {
  const parts = [];
  const ends = (which) => (which === "both" ? ["start", "end"] : which === undefined ? [] : [which]);
  const terminals = LINE.includes(item.shape) ? lineTerminals(item) : null;
  for (const end of ends(item.head)) {
    if (terminals === null) throw new Error(`a head needs a line mark, not ${item.shape}`);
    parts.push(`<path class="${mark.head ?? "cf-m-trans-head"}" d="${headPath(terminals[end])}"/>`);
  }
  for (const end of ends(item.open)) {
    if (terminals === null) throw new Error(`an open head needs a line mark, not ${item.shape}`);
    parts.push(`<path class="cf-m-state" d="${headPath(terminals[end], 11, 5.5)}"/>`);
  }
  for (const end of ends(item.square)) {
    if (terminals === null) throw new Error(`a square end needs a line mark, not ${item.shape}`);
    const { x, y } = terminals[end];
    parts.push(`<rect class="cf-m-state" x="${num(x - 5)}" y="${num(y - 5)}" width="10" height="10"/>`);
  }
  if (item.cap === "end") {
    if (item.shape !== "rect") throw new Error("a cap closes a bar");
    const { x, y, w, h } = item;
    const rx = Math.min(item.rx ?? 0, h / 2, 6);
    const start = x + w - 14;
    parts.push(`<path class="cf-m-cap" d="M${num(start)} ${num(y)}H${num(x + w - rx)}Q${num(x + w)} ${num(y)} ${num(x + w)} ${num(y + rx)}V${num(y + h - rx)}Q${num(x + w)} ${num(y + h)} ${num(x + w - rx)} ${num(y + h)}H${num(start)}Z"/>`);
  }
  const crossClass = mark.cross ?? "cf-m-cross";
  if (item.cross !== undefined || mark.cross !== undefined) {
    const spec = isObject(item.cross) ? item.cross : defaultCross(item, markName);
    parts.push(`<path class="${crossClass}" d="${crossPath(spec.cx, spec.cy, spec.size)}"/>`);
  }
  if (item.tick === true) {
    if (item.shape !== "circle") throw new Error("a tick sits inside a ring");
    const { cx, cy } = item;
    parts.push(`<path class="cf-m-done" d="M${num(cx - 6)} ${num(cy)}L${num(cx - 2)} ${num(cy + 5)}L${num(cx + 7)} ${num(cy - 5)}"/>`);
  }
  return parts.join("");
}

function defaultCross(item, markName) {
  if (item.shape === "rect") {
    const size = markName === "nc" ? Math.min(item.w, item.h) * 0.88 : 12;
    return { cx: item.x + item.w / 2, cy: item.y + item.h / 2, size };
  }
  if (LINE.includes(item.shape)) {
    const points = pathPoints(item);
    const middle = points[Math.floor(points.length / 2)] ?? points[0];
    const previous = points[Math.max(0, Math.floor(points.length / 2) - 1)];
    return { cx: (middle.x + previous.x) / 2, cy: (middle.y + previous.y) / 2, size: 12 };
  }
  if (item.shape === "circle") return { cx: item.cx, cy: item.cy, size: 12 };
  throw new Error(`no default cross for ${item.shape}`);
}

function crossPath(cx, cy, size) {
  const half = size / 2;
  return `M${num(cx - half)} ${num(cy - half)}L${num(cx + half)} ${num(cy + half)}M${num(cx + half)} ${num(cy - half)}L${num(cx - half)} ${num(cy + half)}`;
}

function headPath({ x, y, dx, dy }, length = 11, half = 5.5) {
  const norm = Math.hypot(dx, dy) || 1;
  const ux = dx / norm;
  const uy = dy / norm;
  const bx = x - ux * length;
  const by = y - uy * length;
  return `M${num(x)} ${num(y)}L${num(bx - uy * half)} ${num(by + ux * half)}L${num(bx + uy * half)} ${num(by - ux * half)}Z`;
}

// The start and end of a line mark with the direction of travel at each.
function lineTerminals(item) {
  if (item.shape === "line") {
    const dx = item.x2 - item.x1;
    const dy = item.y2 - item.y1;
    return { start: { x: item.x1, y: item.y1, dx: -dx, dy: -dy }, end: { x: item.x2, y: item.y2, dx, dy } };
  }
  if (item.shape === "polyline") {
    const [a, b] = item.points;
    const [c, d] = item.points.slice(-2);
    return { start: { x: a[0], y: a[1], dx: a[0] - b[0], dy: a[1] - b[1] }, end: { x: d[0], y: d[1], dx: d[0] - c[0], dy: d[1] - c[1] } };
  }
  const segments = parsePath(item.d);
  const first = segments[0];
  const last = segments.at(-1);
  return {
    start: { x: first.from.x, y: first.from.y, dx: first.from.x - first.startControl.x, dy: first.from.y - first.startControl.y },
    end: { x: last.to.x, y: last.to.y, dx: last.to.x - last.endControl.x, dy: last.to.y - last.endControl.y },
  };
}

function pathPoints(item) {
  if (item.shape === "line") return [{ x: item.x1, y: item.y1 }, { x: item.x2, y: item.y2 }];
  if (item.shape === "polyline") return item.points.map(([x, y]) => ({ x, y }));
  const segments = parsePath(item.d);
  return [segments[0].from, ...segments.map((segment) => segment.to)];
}

// Absolute M, L, H, V, C, Q and Z only: enough for every family, small
// enough to read the direction of travel at each end without a DOM.
export function parsePath(d) {
  const tokens = String(d).match(/[MLHVCQZ]|-?\d*\.?\d+(?:e-?\d+)?/gi) ?? [];
  const segments = [];
  let index = 0;
  let command = null;
  let current = { x: 0, y: 0 };
  let start = { x: 0, y: 0 };
  const number = () => {
    const value = Number(tokens[index++]);
    if (!Number.isFinite(value)) throw new Error(`invalid path data: ${d}`);
    return value;
  };
  while (index < tokens.length) {
    if (/^[MLHVCQZ]$/i.test(tokens[index])) command = tokens[index++].toUpperCase();
    if (command === null) throw new Error(`path data must start with a command: ${d}`);
    const from = { ...current };
    if (command === "M") {
      current = { x: number(), y: number() };
      start = { ...current };
      command = "L";
      continue;
    }
    if (command === "Z") {
      if (current.x !== start.x || current.y !== start.y) segments.push({ from, to: { ...start }, startControl: { ...start }, endControl: from });
      current = { ...start };
      command = null;
      continue;
    }
    let startControl;
    let endControl;
    if (command === "L") current = { x: number(), y: number() };
    else if (command === "H") current = { x: number(), y: current.y };
    else if (command === "V") current = { x: current.x, y: number() };
    else if (command === "C") {
      const c1 = { x: number(), y: number() };
      const c2 = { x: number(), y: number() };
      current = { x: number(), y: number() };
      startControl = same(c1, from) ? c2 : c1;
      endControl = same(c2, current) ? c1 : c2;
    } else if (command === "Q") {
      const c = { x: number(), y: number() };
      current = { x: number(), y: number() };
      startControl = c;
      endControl = c;
    } else throw new Error(`unsupported path command ${command}`);
    segments.push({ from, to: { ...current }, startControl: startControl ?? { ...current }, endControl: endControl ?? from });
  }
  if (!segments.length) throw new Error(`path draws nothing: ${d}`);
  return segments;
}

function legendKey(state, sample, id) {
  const mark = MARKS[state.mark];
  const cls = mark.className;
  const svg = (inner) => `<svg class="cf-key" viewBox="0 0 28 16" aria-hidden="true">${inner}</svg>`;
  const hasPart = (part) => sample?.[part] !== undefined;
  switch (mark.key) {
    case "line": {
      const head = hasPart("head") ? `<path class="${mark.head ?? "cf-m-trans-head"}" d="${headPath({ x: 26, y: 8, dx: 1, dy: 0 }, 9, 4.5)}"/>` : "";
      const open = hasPart("open") ? `<path class="cf-m-state" d="${headPath({ x: 26, y: 8, dx: 1, dy: 0 }, 9, 4.5)}"/>` : "";
      const square = hasPart("square") ? `<rect class="cf-m-state" x="19" y="4" width="8" height="8"/>` : "";
      const cross = hasPart("cross") ? `<path class="cf-m-cross" d="${crossPath(14, 8, 9)}"/>` : "";
      return svg(`<path class="${cls}" d="M3 8H${head || open || square ? 20 : 25}"/>${head}${open}${square}${cross}`);
    }
    case "bar-v": return svg(`<line class="${cls}" x1="14" y1="1" x2="14" y2="15"/>`);
    case "ring": return svg(`<circle class="${cls}" cx="14" cy="8" r="6"/>${hasPart("tick") ? `<path class="cf-m-done" d="M10.5 8L13 11L18 5"/>` : ""}`);
    case "disc": return svg(`<circle class="${cls}" cx="14" cy="8" r="6.5"/>`);
    case "diamond": return svg(`<path class="${cls}" d="M14 1.5L20.5 8L14 14.5L7.5 8Z"/>`);
    case "cross": return svg(`<path class="${cls}" d="${crossPath(14, 8, 10)}"/>`);
    case "bar": return svg(`<rect class="${cls}" x="2" y="3" width="24" height="10" rx="3"/>${hasPart("cap") ? `<path class="cf-m-cap" d="M19 3H23Q26 3 26 6V10Q26 13 23 13H19Z"/>` : ""}${hasPart("cross") ? `<path class="cf-m-cross" d="${crossPath(14, 8, 9)}"/>` : ""}`);
    case "box": return svg(`<rect class="${cls}" x="2" y="2" width="24" height="12" rx="3"/>${mark === MARKS.denied || hasPart("cross") ? `<path class="cf-m-cross" d="${crossPath(14, 8, 9)}"/>` : ""}`);
    case "cell": {
      const hatch = mark.hatch ? `<defs><pattern id="${id}" width="4.5" height="4.5" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><line class="cf-m-hatchline" x1="0" y1="0" x2="0" y2="4.5"/></pattern></defs>` : "";
      const fill = mark.hatch ? ` fill="url(#${id})"` : "";
      const cross = mark.cross ? `<path class="${mark.cross}" d="${crossPath(14, 8, 10.5)}"/>` : "";
      return svg(`${hatch}<rect class="${cls}" x="8" y="2" width="12" height="12" rx="2"${fill}/>${cross}`);
    }
    default: throw new Error(`no legend key for ${state.mark}`);
  }
}

function twinTable(figure, factValues) {
  let columns;
  let rows;
  if (figure.twin === "facts") {
    columns = ["Fact", "Source", "Value"];
    rows = figure.facts.map((fact, index) => [fact.claim, `\`${fact.source}\``, typeof factValues[index].value === "string" ? factValues[index].value : JSON.stringify(factValues[index].value)]);
  } else {
    ({ columns, rows } = figure.twin);
  }
  const head = columns.map((column) => `<th scope="col">${inlineText(column)}</th>`).join("");
  const body = rows.map((row) => `<tr>${row.map((cell) => `<td>${inlineText(cell)}</td>`).join("")}</tr>`).join("");
  return `<details class="cf-twin"><summary>Table twin</summary><div class="cf-twin-scroll"><table><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table></div></details>`;
}

// Backticks mark code in a twin cell; everything else is text.
function inlineText(value) {
  return String(value).split(/(`[^`]+`)/).map((part) => (part.startsWith("`") && part.endsWith("`") && part.length > 2 ? `<code>${escapeText(part.slice(1, -1))}</code>` : escapeText(part))).join("");
}

// ---------------------------------------------------------------------------
// The gate: in-page probe and rule evaluation
// ---------------------------------------------------------------------------

// Runs in the page. Self-contained on purpose: Playwright serialises it, and
// present's browser checks call the same function. Every token is read off
// the render; data-state only says which state a mark claims to be.
export function probeFigures(options) {
  const clearance = options?.clearance ?? 8;
  const shown = (element) => element.getClientRects().length > 0 && getComputedStyle(element).display !== "none" && getComputedStyle(element).visibility !== "hidden";
  const DRAWN = ["rect", "circle", "ellipse", "path", "line", "polygon", "polyline"];
  const round = (value, step = 0.1) => Number((Math.round(value / step) * step).toFixed(4));
  // Paint is read twice and never as a hue: the interior (none, ground,
  // solid, pattern, faint) and the edge (none, a ground-coloured halo, faint,
  // solid). A fill in the figure's own ground colour is a hollow mark.
  const clear = (value) => !value || value === "none" || /rgba\([^)]*,\s*0\)$/.test(value) || value === "transparent";
  const interiorOf = (node, ground) => {
    const style = getComputedStyle(node);
    if (clear(style.fill)) return "none";
    if (style.fill.startsWith("url(")) return "pattern";
    if (style.fill === ground) return "ground";
    return parseFloat(style.fillOpacity) < 1 || parseFloat(style.opacity) < 1 ? "faint" : "solid";
  };
  const edgeOf = (node, ground) => {
    const style = getComputedStyle(node);
    if (clear(style.stroke) || !(parseFloat(style.strokeWidth) > 0)) return "none";
    if (style.stroke === ground) return "ground";
    return parseFloat(style.strokeOpacity) < 1 || parseFloat(style.opacity) < 1 ? "faint" : "solid";
  };
  const widthOf = (node) => {
    const style = getComputedStyle(node);
    return clear(style.stroke) ? 0 : round(parseFloat(style.strokeWidth) || 0, 0.05);
  };
  const dashOf = (node) => {
    const style = getComputedStyle(node);
    if (clear(style.stroke)) return "no-edge";
    const raw = (style.strokeDasharray || "").trim();
    if (!raw || raw === "none") return "solid";
    const values = raw.split(/[\s,]+/).map((value) => parseFloat(value)).filter((value) => Number.isFinite(value) && value > 0);
    if (!values.length) return "solid";
    const unit = Math.min(...values);
    return `dash:${values.map((value) => Math.round((value / unit) * 2) / 2).join("-")}`;
  };
  const shapeOf = (node) => {
    const tag = node.tagName.toLowerCase();
    if (tag !== "path") return tag;
    const d = node.getAttribute("d") || "";
    const closed = /z\s*$/i.test(d.trim());
    return `path:${/[CcSsQqTtAa]/.test(d) ? "curved" : "straight"}${closed ? ":closed" : ""}`;
  };
  const scaleOf = (node) => {
    const matrix = node.getScreenCTM();
    return matrix ? Math.hypot(matrix.a, matrix.b) : 1;
  };
  const infoDimension = (node) => {
    const tag = node.tagName.toLowerCase();
    const rect = node.getBoundingClientRect();
    if (tag === "circle" || tag === "ellipse" || tag === "rect") return Math.min(rect.width, rect.height);
    const d = node.getAttribute("d") || "";
    const closed = tag === "polygon" || (tag === "path" && /z\s*$/i.test(d.trim()));
    // A closed shape reads by its smaller extent. A cross is two strokes, so
    // its bounding box is the cross itself, never the cell around it.
    if (closed || (tag === "path" && /M[^M]*M/.test(d))) return Math.min(rect.width, rect.height) || Math.max(rect.width, rect.height);
    try { return node.getTotalLength() * scaleOf(node); } catch { return Math.max(rect.width, rect.height); }
  };
  const literal = (value) => {
    if (value === null) return false;
    const trimmed = value.trim().toLowerCase();
    if (!trimmed || ["none", "transparent", "currentcolor", "inherit"].includes(trimmed)) return false;
    if (/^url\(#[^)]+\)$/.test(trimmed)) return false;
    if (/^var\(--cf-fig-[a-z-]+\)$/.test(trimmed)) return false;
    return true;
  };
  const samplesOf = (node) => {
    const tag = node.tagName.toLowerCase();
    const matrix = node.getScreenCTM();
    if (!matrix) return [];
    const map = (x, y) => ({ x: matrix.a * x + matrix.c * y + matrix.e, y: matrix.b * x + matrix.d * y + matrix.f });
    const half = (parseFloat(getComputedStyle(node).strokeWidth) || 0) / 2 * scaleOf(node);
    const points = [];
    if (tag === "circle") {
      const cx = node.cx.baseVal.value; const cy = node.cy.baseVal.value; const r = node.r.baseVal.value;
      for (let step = 0; step < 32; step += 1) points.push(map(cx + r * Math.cos(step * Math.PI / 16), cy + r * Math.sin(step * Math.PI / 16)));
      points.push(map(cx, cy));
    } else if (tag === "rect") {
      const x = node.x.baseVal.value; const y = node.y.baseVal.value; const w = node.width.baseVal.value; const h = node.height.baseVal.value;
      const nx = Math.max(2, Math.ceil(w / 3)); const ny = Math.max(2, Math.ceil(h / 3));
      for (let i = 0; i <= nx; i += 1) { points.push(map(x + (w * i) / nx, y)); points.push(map(x + (w * i) / nx, y + h)); }
      for (let j = 0; j <= ny; j += 1) { points.push(map(x, y + (h * j) / ny)); points.push(map(x + w, y + (h * j) / ny)); }
    } else {
      let length = 0;
      try { length = node.getTotalLength(); } catch { length = 0; }
      const count = Math.max(2, Math.ceil(length / 2));
      for (let i = 0; i <= count; i += 1) {
        const point = node.getPointAtLength((length * i) / count);
        points.push(map(point.x, point.y));
      }
    }
    return points.map((point) => ({ ...point, half }));
  };
  const filledArea = (node) => {
    const tag = node.tagName.toLowerCase();
    const fill = getComputedStyle(node).fill;
    if (!fill || fill === "none") return null;
    if (tag === "rect" || tag === "circle") return node.getBoundingClientRect();
    return null;
  };
  // A mark's box in its SVG's own coordinates, through every transform on
  // the mark and its ancestors, attribute or CSS: what the reader sees,
  // expressed in the units the composition's scale speaks.
  const boxInSvg = (node, svg) => {
    let box = null;
    try { box = node.getBBox(); } catch { return null; }
    const outer = svg.getScreenCTM();
    const inner = node.getScreenCTM();
    if (!box || !outer || !inner) return null;
    const matrix = outer.inverse().multiply(inner);
    const corners = [[box.x, box.y], [box.x + box.width, box.y], [box.x, box.y + box.height], [box.x + box.width, box.y + box.height]]
      .map(([x, y]) => [matrix.a * x + matrix.c * y + matrix.e, matrix.b * x + matrix.d * y + matrix.f]);
    const xs = corners.map(([x]) => x);
    const ys = corners.map(([, y]) => y);
    return [Math.min(...xs), Math.min(...ys), Math.max(...xs) - Math.min(...xs), Math.max(...ys) - Math.min(...ys)];
  };
  // The grammar draws no transform on a mark or any group above it.
  const transformedUpTo = (node, svg) => {
    for (let element = node; element && element !== svg; element = element.parentElement) {
      if (element.hasAttribute("transform") || getComputedStyle(element).transform !== "none") return true;
    }
    return false;
  };
  const figures = [];
  for (const figure of document.querySelectorAll("figure.cf-fig")) {
    const ground = getComputedStyle(figure).backgroundColor;
    const route = figure.closest("[data-altitude]")?.getAttribute("data-altitude") ?? null;
    const svgs = [...figure.querySelectorAll("svg.cf-fig-svg")];
    const visible = svgs.filter(shown);
    const record = {
      id: figure.getAttribute("data-cf-figure-id") ?? "",
      family: figure.getAttribute("data-cf-figure") ?? "",
      binding: figure.getAttribute("data-cf-binding") ?? "",
      panel: route,
      companion: figure.closest("[data-cf-companion]")?.getAttribute("data-cf-companion") ?? null,
      declaredStates: (figure.getAttribute("data-cf-states") ?? "").split(/\s+/).filter(Boolean),
      elongationMax: Number(figure.getAttribute("data-cf-elongation-max") ?? "1.5"),
      facts: (() => { try { return JSON.parse(figure.getAttribute("data-cf-facts") ?? "[]"); } catch { return null; } })(),
      values: (() => { try { return JSON.parse(figure.getAttribute("data-cf-values") ?? "null"); } catch { return "invalid"; } })(),
      variants: svgs.map((svg) => ({ variant: svg.getAttribute("data-cf-variant"), shown: shown(svg), height: svg.getBoundingClientRect().height, width: svg.getBoundingClientRect().width })),
      visibleVariant: visible.length === 1 ? visible[0].getAttribute("data-cf-variant") : null,
      visibleCount: visible.length,
      legend: [...figure.querySelectorAll(".cf-legend li")].filter(shown).map((item) => ({ state: item.getAttribute("data-state"), text: item.textContent.trim() })),
      caption: [...figure.querySelectorAll("figcaption")].map((caption) => caption.textContent.trim()),
      twinRows: figure.querySelectorAll("details.cf-twin table tbody tr").length,
      twinCount: figure.querySelectorAll("details.cf-twin").length,
      literalColours: [],
      title: "",
      description: "",
      drawn: [],
      states: {},
      marks: [],
      texts: [],
      geometry: [],
      collisions: [],
      signatures: {},
    };
    for (const element of figure.querySelectorAll("*")) {
      for (const attribute of ["fill", "stroke", "stop-color", "color", "flood-color", "lighting-color"]) {
        if (literal(element.getAttribute(attribute))) record.literalColours.push(`${element.tagName.toLowerCase()} ${attribute}=${element.getAttribute(attribute)}`);
      }
      const style = element.getAttribute("style");
      if (style && /(?:#[0-9a-f]{3,8}\b|\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\()/i.test(style)) record.literalColours.push(`${element.tagName.toLowerCase()} style=${style}`);
    }
    for (const svg of svgs) {
      const signature = [];
      for (const group of svg.querySelectorAll("[data-state]")) {
        const drawn = [...group.querySelectorAll("*")].filter((node) => DRAWN.includes(node.tagName.toLowerCase()));
        const nodes = group.tagName.toLowerCase() === "g" ? drawn : [group];
        for (const node of nodes) {
          let box = null;
          try { box = node.getBBox(); } catch { box = null; }
          signature.push(`${group.getAttribute("data-state")}:${shapeOf(node)}:${box ? `${Math.round(box.width)}x${Math.round(box.height)}` : "0x0"}`);
        }
      }
      record.signatures[svg.getAttribute("data-cf-variant")] = signature.sort();
    }
    const svg = visible[0];
    if (svg) {
      record.title = svg.querySelector("title")?.textContent.trim() ?? "";
      record.description = svg.querySelector("desc")?.textContent.trim() ?? "";
      const labelled = new Map();
      for (const text of svg.querySelectorAll("text")) {
        if (!text.textContent.trim()) continue;
        const fontSize = parseFloat(getComputedStyle(text).fontSize) || 0;
        const rect = text.getBoundingClientRect();
        record.texts.push({ text: text.textContent.trim().slice(0, 60), full: text.textContent.trim().slice(0, 400), size: round(fontSize * scaleOf(text), 0.01), box: [rect.left, rect.top, rect.right, rect.bottom], labels: (text.getAttribute("data-cf-for") ?? "").split(/\s+/).filter(Boolean) });
      }
      for (const group of svg.querySelectorAll("[data-state]")) {
        const state = group.getAttribute("data-state");
        const drawn = group.tagName.toLowerCase() === "g" ? [...group.children].filter((node) => DRAWN.includes(node.tagName.toLowerCase())) : [group];
        if (!drawn.length) continue;
        const [primary, ...extras] = drawn;
        const token = {
          interior: interiorOf(primary, ground),
          edge: edgeOf(primary, ground),
          width: widthOf(primary),
          dash: dashOf(primary),
          shape: shapeOf(primary),
          overlay: extras.map((node) => `${shapeOf(node)}:${interiorOf(node, ground)}/${edgeOf(node, ground)}`).sort().join("+") || "none",
        };
        const entry = record.states[state] ?? { interior: new Set(), edge: new Set(), width: new Set(), dash: new Set(), shape: new Set(), overlay: new Set() };
        for (const key of Object.keys(token)) entry[key].add(token[key]);
        record.states[state] = entry;
        record.drawn.push(state);
        if (group.id) {
          const box = boxInSvg(primary, svg);
          if (box) record.geometry.push({ id: group.id, tag: primary.tagName.toLowerCase(), box: box.map((value) => round(value, 0.001)), transformed: transformedUpTo(primary, svg) });
        }
        for (const node of drawn) {
          record.marks.push({ state, tag: node.tagName.toLowerCase(), size: round(infoDimension(node), 0.1) });
          labelled.set(node, group.id || null);
        }
      }
      // Rule 8: no text over text, and text clear of every mark it does not label.
      for (let i = 0; i < record.texts.length; i += 1) {
        for (let j = i + 1; j < record.texts.length; j += 1) {
          const a = record.texts[i].box; const b = record.texts[j].box;
          const depth = Math.min(Math.min(a[2], b[2]) - Math.max(a[0], b[0]), Math.min(a[3], b[3]) - Math.max(a[1], b[1]));
          if (depth >= 1) record.collisions.push({ kind: "text", a: record.texts[i].text, b: record.texts[j].text, depth: round(depth) });
        }
      }
      const textNodes = [...svg.querySelectorAll("text")].filter((text) => text.textContent.trim());
      const group = (node) => node.closest("[data-state]")?.getAttribute("data-state") ?? node.tagName.toLowerCase();
      for (const [node, groupId] of labelled) {
        const samples = samplesOf(node);
        const area = filledArea(node);
        textNodes.forEach((text, index) => {
          const label = record.texts[index];
          if (groupId && label.labels.includes(groupId)) return;
          const [left, top, right, bottom] = label.box;
          let nearest = Infinity;
          let inside = 0;
          for (const point of samples) {
            const dx = Math.max(left - point.x, 0, point.x - right);
            const dy = Math.max(top - point.y, 0, point.y - bottom);
            const distance = Math.hypot(dx, dy) - point.half;
            nearest = Math.min(nearest, distance);
            if (dx === 0 && dy === 0) inside = Math.max(inside, Math.min(point.x - left, right - point.x, point.y - top, bottom - point.y) + point.half);
          }
          if (area) {
            const depth = Math.min(Math.min(area.right, right) - Math.max(area.left, left), Math.min(area.bottom, bottom) - Math.max(area.top, top));
            if (depth > 0) { inside = Math.max(inside, depth); nearest = Math.min(nearest, -depth); }
          }
          if (inside >= 1) record.collisions.push({ kind: "overprint", a: label.text, b: group(node), depth: round(inside) });
          else if (nearest < clearance) record.collisions.push({ kind: "clearance", a: label.text, b: group(node), distance: round(nearest) });
        });
      }
      for (const [state, entry] of Object.entries(record.states)) record.states[state] = Object.fromEntries(Object.entries(entry).map(([key, values]) => [key, [...values].sort()]));
      record.drawn = [...new Set(record.drawn)].sort();
      record.boxedText = [...svg.querySelectorAll("[data-state]")].length > 0 && [...svg.querySelectorAll("[data-state]")].every((group) => {
        const primary = group.tagName.toLowerCase() === "g" ? [...group.children].find((node) => DRAWN.includes(node.tagName.toLowerCase())) : group;
        return primary?.tagName.toLowerCase() === "rect" && group.querySelectorAll("*").length <= 1;
      });
    }
    figures.push(record);
  }
  return figures;
}

// Rule failures for one figure from a wide and a narrow observation, each in
// light and dark. The result names the rule by number, so a report reads as
// "the page, the altitude and the rule".
// `composed` is composeFigure() of the pinned declaration with the committed
// values; given it, rule 6 reads every value-bearing mark back off the render.
export function figureRuleFailures({ wide, narrow, wideDark = null, narrowDark = null, evidence = null, composed = null }) {
  const failures = [];
  const add = (rule, message) => failures.push({ rule, name: FIGURE_RULES[rule], message });
  const renders = [["wide", wide], ["narrow", narrow], ["wide dark", wideDark], ["narrow dark", narrowDark]].filter(([, record]) => record);
  if (!wide.family || !FAMILIES.includes(wide.family)) add(1, `the figure declares no grammar family (${wide.family || "none"})`);
  if (wide.visibleVariant !== "wide") add(5, `at the wide width the ${wide.visibleVariant ?? "no single"} composition shows`);
  if (narrow.visibleVariant !== "narrow") add(5, `at the narrow width the ${narrow.visibleVariant ?? "no single"} composition shows`);
  for (const [label, record] of renders) {
    const declared = [...record.declaredStates].sort();
    const legend = [...new Set(record.legend.map((item) => item.state))].sort();
    const drawn = record.drawn;
    if (drawn.join(" ") !== legend.join(" ")) add(2, `${label}: drawn states ${drawn.join(", ") || "none"} but the legend keys ${legend.join(", ") || "none"}`);
    const extra = drawn.filter((state) => !declared.includes(state));
    if (extra.length) add(2, `${label}: drawn states ${extra.join(", ")} are not declared`);
    if (record.literalColours.length) add(10, `${label}: literal colour ${record.literalColours.slice(0, 3).join("; ")}`);
    if (!record.title) add(11, `${label}: the SVG has no title`);
    if (!record.description) add(11, `${label}: the SVG has no description`);
    const means = record.legend.map((item) => item.text.toLowerCase());
    const unnamed = means.filter((text) => !record.description.toLowerCase().includes(text));
    if (record.description && unnamed.length) add(11, `${label}: the description does not name ${unnamed.join(", ")}`);
    if (record.twinCount !== 1 || record.twinRows < 1) add(12, `${label}: expected one table twin with at least one row`);
    if (record.caption.length !== 1 || !oneSentence(record.caption[0] ?? "")) add(9, `${label}: expected one one-sentence caption, found ${record.caption.length}`);
    for (const text of record.texts) if (text.size < THRESHOLDS.textFloorPx) add(4, `${label}: text "${text.text}" renders at ${text.size}px, under ${THRESHOLDS.textFloorPx}px`);
    for (const mark of record.marks) if (mark.size < THRESHOLDS.markFloorPx) add(4, `${label}: a ${mark.state} ${mark.tag} renders at ${mark.size}px, under the ${THRESHOLDS.markFloorPx}px mark floor`);
    for (const collision of record.collisions) {
      if (collision.kind === "text") add(8, `${label}: text "${collision.a}" overprints "${collision.b}" by ${collision.depth}px`);
      else if (collision.kind === "overprint") add(8, `${label}: text "${collision.a}" overprints a ${collision.b} mark by ${collision.depth}px`);
      else add(8, `${label}: text "${collision.a}" sits ${collision.distance}px from a ${collision.b} mark it does not label, under ${THRESHOLDS.labelClearancePx}px`);
    }
    if (record.boxedText) add(7, `${label}: every mark is a box with nothing drawn between them`);
    for (const [a, b] of pairs(Object.keys(record.states))) {
      const differing = channelDifferences(record.states[a], record.states[b]);
      if (differing.length < THRESHOLDS.minChannels) add(3, `${label}: states ${a} and ${b} differ on ${differing.length ? differing.join(", ") : "no channel"}, need ${THRESHOLDS.minChannels}`);
    }
  }
  for (const [light, dark, label] of [[wide, wideDark, "wide"], [narrow, narrowDark, "narrow"]]) {
    if (!dark) continue;
    if (JSON.stringify(light.states) !== JSON.stringify(dark.states)) add(3, `${label}: the non-hue tokens of a state differ between light and dark`);
  }
  const wideHeight = wide.variants.find((variant) => variant.variant === "wide")?.height ?? 0;
  const narrowHeight = narrow.variants.find((variant) => variant.variant === "narrow")?.height ?? 0;
  if (wideHeight > 0 && narrowHeight > wide.elongationMax * wideHeight + 0.5) add(5, `the narrow render is ${round(narrowHeight / wideHeight, 0.01)} times the wide height, over the ${wide.elongationMax} ceiling`);
  // Each set is read where its composition shows: a hidden SVG has no
  // geometry, so the wide set comes from the wide render and the narrow set
  // from the narrow one.
  const wideSet = wide.signatures.wide ?? [];
  const narrowSet = narrow.signatures.narrow ?? [];
  if (!narrowSet.length) add(5, "the figure declares no narrow composition");
  else if (wideSet.join("|") === narrowSet.join("|")) add(5, "the narrow mark set is a permutation of the wide set, a reflow and not a recomposition");
  if (wide.facts === null) add(6, "the drawn facts are unreadable");
  if (evidence) {
    const recorded = evidence.facts ?? [];
    const drawn = wide.facts ?? [];
    if (recorded.length !== drawn.length) add(6, `the page draws ${drawn.length} facts and the evidence records ${recorded.length}`);
    recorded.forEach((fact, index) => {
      if (!fact.matches) add(6, `fact "${fact.claim}" re-derives as ${JSON.stringify(fact.derived)} from ${fact.source}, the figure asserts ${JSON.stringify(fact.drawn)}`);
      if (drawn[index] && canonicalJson(drawn[index].value) !== canonicalJson(fact.drawn)) add(6, `fact "${fact.claim}" is drawn as ${JSON.stringify(drawn[index].value)} and recorded as ${JSON.stringify(fact.drawn)}`);
    });
    if (evidence.data && canonicalJson(wide.values) !== canonicalJson(evidence.data.drawn)) add(6, `the drawn values ${JSON.stringify(wide.values)} differ from the recorded drawn values ${JSON.stringify(evidence.data.drawn)}`);
    if (evidence.data && !drawnValuesMatch(evidence.data.drawn, evidence.data.derived)) add(6, "the drawn values differ from the values derived from the source");
  }
  if (composed) {
    for (const [label, record] of renders) {
      const variant = label.startsWith("wide") ? "wide" : "narrow";
      for (const message of valueReadback(composed[variant], record, variant)) add(6, `${label}: ${message}`);
    }
  }
  return failures;
}

// Rule 6 on the render itself: each mark that encodes a value is measured
// where it is drawn and read back through its composition's scale, and the
// text that labels it must state the committed value. The attributes a figure
// carries about its own values are never the evidence here.
function valueReadback(composition, record, variant) {
  const scale = composition?.scale;
  if (!scale) return [];
  const failures = [];
  const [d0, d1] = scale.domain;
  const [r0, r1] = scale.range;
  const read = (position) => (r1 === r0 ? d0 : d0 + ((position - r0) / (r1 - r0)) * (d1 - d0));
  const texts = (predicate) => composition.draw.filter((item) => item.text !== undefined && predicate(item)).map((item) => String(item.text).trim()).sort();
  for (const item of composition.draw) {
    if (item.value === undefined || item.id === undefined) continue;
    const mark = (record.geometry ?? []).find((candidate) => candidate.id.endsWith(`-${variant}-${item.id}`));
    if (!mark) { failures.push(`the ${item.id} mark for ${formatNumber(item.value)} is not drawn`); continue; }
    if (mark.transformed) failures.push(`the ${item.id} mark is transformed, and the grammar draws no transform`);
    const [x, , width] = mark.box;
    const bar = mark.tag === "rect";
    if (bar && Math.abs(x - r0) > 0.5) failures.push(`the ${item.id} bar starts at ${round(x)}, not at the scale origin ${round(r0)}`);
    const shown = read(bar ? x + width : x + width / 2);
    if (Math.abs(shown - item.value) > DRAWN_VALUE_TOLERANCE * Math.max(1, Math.abs(item.value))) {
      failures.push(`the ${item.id} mark reads ${round(shown)} on its scale and the committed value is ${formatNumber(item.value)}`);
    }
    const expected = texts((text) => text.for?.includes(item.id));
    const labels = record.texts.filter((text) => text.labels.includes(mark.id)).map((text) => text.full).sort();
    if (canonicalJson(labels) !== canonicalJson(expected)) failures.push(`the ${item.id} mark is labelled ${JSON.stringify(labels)}, not ${JSON.stringify(expected)}`);
  }
  return failures;
}

// A drawn value is read back through the scale from a position rounded to
// the drawing grid, so it may differ from the derived value by that rounding
// and no more. The validator applies the same bound.
export const DRAWN_VALUE_TOLERANCE = 1e-3;
export function drawnValuesMatch(drawn, derived) {
  if (!isObject(drawn) || !isObject(derived)) return false;
  const keys = Object.keys(derived).sort();
  if (Object.keys(drawn).sort().join("\u0000") !== keys.join("\u0000")) return false;
  return keys.every((key) => typeof drawn[key] === "number" && typeof derived[key] === "number"
    && Math.abs(drawn[key] - derived[key]) <= DRAWN_VALUE_TOLERANCE * Math.max(1, Math.abs(derived[key])));
}

export function channelDifferences(a, b) {
  const differing = [];
  const same = (key) => JSON.stringify(a[key]) === JSON.stringify(b[key]);
  if (!same("interior")) differing.push("interior");
  if (!same("edge")) differing.push("edge");
  const widthsA = a.width ?? [];
  const widthsB = b.width ?? [];
  const widthGap = Math.min(...widthsA.flatMap((x) => widthsB.map((y) => Math.abs(x - y))));
  if (widthsA.length && widthsB.length && widthGap >= THRESHOLDS.widthChannelPx) differing.push("width");
  if (!same("dash")) differing.push("dash");
  if (!same("shape")) differing.push("shape");
  if (!same("overlay")) differing.push("overlay");
  return differing;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

export function oneSentence(value) {
  const text = String(value).trim();
  if (!/[.!?]$/.test(text)) return false;
  const inner = text.slice(0, -1);
  return !/[.!?](?:\s|$)/.test(inner.replace(/\b(?:e\.g|i\.e|etc|vs)\./gi, ""));
}

function pairs(items) {
  const result = [];
  for (let i = 0; i < items.length; i += 1) for (let j = i + 1; j < items.length; j += 1) result.push([items[i], items[j]]);
  return result;
}

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function onlyKeys(value, allowed, where, fail) {
  for (const key of Object.keys(value)) if (!allowed.includes(key) && !key.startsWith("__")) fail(`${where}: unknown key ${key}`);
}

function text(value, label, fail, max = 400) {
  if (typeof value !== "string" || !value.trim() || value.length > max || /[\u0000-\u0008\u000b-\u001f\u007f]/.test(value)) fail(`${label} must be 1 to ${max} characters of text`);
  else if (/[\u2013\u2014]/.test(value)) fail(`${label} carries an em or en dash`);
}

function coordinates(item, keys, where, fail) {
  for (const key of keys) if (!finiteCoordinate(item[key])) fail(`${where} ${key} must be a coordinate`);
}

function finiteCoordinate(value) {
  return Number.isFinite(value) && Math.abs(value) <= LIMITS.coordinate;
}

function safePath(value) {
  return typeof value === "string" && value.length > 0 && value.length <= 512 && !value.startsWith("/") && !value.includes("\\") && !/[\u0000-\u001f\u007f]/.test(value) && value.split("/").every((part) => part && part !== "." && part !== "..");
}

function validSelector(value) {
  return typeof value === "string" && /^[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)*$/.test(value) && value.length <= 200;
}

function same(a, b) {
  return a.x === b.x && a.y === b.y;
}

function round(value, step = 0.01) {
  return Number((Math.round(value / step) * step).toFixed(6));
}

function num(value) {
  return String(Math.round(value * 100) / 100);
}

function formatNumber(value) {
  return Number.isInteger(value) ? String(value) : String(Math.round(value * 100) / 100);
}

function normalizePath(d) {
  return String(d).replace(/\s+/g, " ").trim();
}

function sanitizeId(value) {
  return String(value).replace(/[^A-Za-z0-9_-]/g, "-");
}

export function escapeText(value) {
  return String(value).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
}

function escapeAttribute(value) {
  return escapeText(value).replaceAll('"', "&quot;");
}
