import { createHash } from "node:crypto";
import { lstat } from "node:fs/promises";
import path from "node:path";
import { TextDecoder } from "node:util";
import GithubSlugger from "github-slugger";
import remarkGfm from "remark-gfm";
import remarkParse from "remark-parse";
import remarkStringify from "remark-stringify";
import { unified } from "unified";
import YAML from "yaml";
import { slugHeading } from "./figure-grammar.mjs";
import { ALTITUDE_PANELS, DERIVED_LOOKUPS, PAGE_CLASS_REASONS, PANEL_CARRIER_ALTERNATES } from "./page-classes.mjs";

export const sha256 = (value) => createHash("sha256").update(value).digest("hex");

export function checkoutEquivalentBytes(committed, worktree) {
  if (committed.equals(worktree)) return true;
  const decoder = new TextDecoder("utf-8", { fatal: true });
  try {
    const canonical = (bytes) => {
      const text = decoder.decode(bytes);
      const normalized = text.replace(/\r\n/g, "\n");
      return normalized.includes("\r") ? null : normalized;
    };
    const committedText = canonical(committed);
    return committedText !== null && committedText === canonical(worktree);
  } catch {
    return false;
  }
}

// JavaScript's relational string comparison is specified over UTF-16 code
// units. Keep evidence ordering independent of the host locale and ICU build.
export function compareDeterministicText(left, right) {
  left = String(left);
  right = String(right);
  return left < right ? -1 : left > right ? 1 : 0;
}

export function safeRelative(value, label = "path") {
  if (typeof value !== "string" || !value || value.includes("\\")) throw new Error(`${label}: expected a non-empty POSIX path`);
  const normalized = path.posix.normalize(value);
  if (normalized !== value || normalized === "." || normalized.startsWith("/") || normalized === ".." || normalized.startsWith("../")) {
    throw new Error(`${label}: path must stay beneath its root: ${value}`);
  }
  for (const segment of value.split("/")) {
    const stem = segment.split(".", 1)[0].toUpperCase();
    if (segment !== segment.normalize("NFC") || segment.length > 255 || Buffer.byteLength(segment, "utf8") > 255 || /[\u0000-\u001f\u007f<>:"|?*]/.test(segment) || /[ .]$/.test(segment) || /^(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])$/.test(stem)) {
      throw new Error(`${label}: path is not portable across macOS, Linux, Windows, and WSL: ${value}`);
    }
  }
  return value;
}

export function portablePathKey(value, label = "path") {
  return safeRelative(value, label).normalize("NFC").toLowerCase();
}

export async function findRepositoryRoot(start) {
  let current = path.resolve(start);
  while (true) {
    try {
      const marker = await lstat(path.join(current, ".codeflow/project.toml"));
      if (marker.isFile()) return current;
    } catch (error) { if (error?.code !== "ENOENT") throw error; }
    const parent = path.dirname(current);
    if (parent === current) throw new Error("repository root not found: .codeflow/project.toml is absent");
    current = parent;
  }
}

export function parseMarkdown(text, sourcePath) {
  text = normalizeMarkdown(text);
  if (!text.startsWith("---\n")) return { frontmatter: {}, body: text };
  const end = text.indexOf("\n---\n", 4);
  if (end < 0) throw new Error(`${sourcePath}: unclosed YAML frontmatter`);
  const frontmatter = YAML.parse(text.slice(4, end)) ?? {};
  if (typeof frontmatter !== "object" || Array.isArray(frontmatter)) throw new Error(`${sourcePath}: frontmatter must be a mapping`);
  return { frontmatter, body: text.slice(end + 5) };
}

export function normalizeMarkdown(text) {
  return text.replace(/^\uFEFF/, "").replace(/\r\n?/g, "\n");
}

export function titleFor(frontmatter, body, sourcePath) {
  if (typeof frontmatter.title === "string" && frontmatter.title.trim()) return frontmatter.title.trim();
  const heading = markdownNodes(markdownTree(body), "heading")
    .find((node) => node.depth === 1 && visibleNodeText(node).trim())?.children;
  const headingText = heading ? visibleNodeText({ children: heading }).trim() : "";
  const derived = headingText || path.posix.basename(sourcePath, ".md").replaceAll("-", " ");
  if (!derived || derived.length > 256) throw new Error(`${sourcePath}: derived title is invalid`);
  return derived;
}

export function validatePageMetadata(frontmatter, sourcePath) {
  for (const [field, maximum] of [["title", 256], ["description", 400], ["status", 128]]) {
    if (!Object.hasOwn(frontmatter, field)) continue;
    const value = frontmatter[field];
    if (typeof value !== "string" || !value.trim() || value.length > maximum) throw new Error(`${sourcePath}: declared ${field} is invalid`);
  }
  return frontmatter;
}

export function rewriteRepositoryMarkdown(body, {
  sourcePath, sourceRoutes, repositoryFiles = new Map(), repositoryDirectories = new Set(), pinnedSourceUrl = () => null,
  commit = "", base, strictTargets, mediaReferences, sourceAnchors = new Map(), recordTargets = new Map(), selfRoute = null,
}) {
  const tree = markdownTree(body);
  const referenceKinds = new Map();
  visitMarkdown(tree, (node) => {
    if (!["linkReference", "imageReference"].includes(node.type)) return;
    const kind = node.type === "imageReference" ? "image" : "link";
    const prior = referenceKinds.get(node.identifier);
    if (prior && prior !== kind) throw new Error(`${sourcePath}: reference ${node.identifier} is used as both a link and an image`);
    referenceKinds.set(node.identifier, kind);
  });
  const definitionResolutions = new Map();
  visitMarkdown(tree, (node) => {
    if (node.type !== "definition") return;
    const kind = referenceKinds.get(node.identifier);
    if (kind) definitionResolutions.set(node.identifier, resolveRepositoryUrl(node.url, { sourcePath, sourceRoutes, repositoryFiles, repositoryDirectories, pinnedSourceUrl, base, mediaReferences, kind, sourceAnchors }));
  });
  let previewSequence = 0;
  visitMarkdown(tree, (node, parent, index, ancestors) => {
    if (node.type === "html") {
      if (/^<!--[\s\S]*-->$/.test(node.value.trim())) parent.children.splice(index, 1);
      else parent.children[index] = { type: "html", value: escapeGeneratedHtml(node.value) };
      return;
    }
    if (["linkReference", "imageReference"].includes(node.type)) {
      const resolved = definitionResolutions.get(node.identifier);
      if (resolved?.sourceReference) parent.children[index] = sourceReferenceNode(node, resolved.sourceReference, commit);
      return;
    }
    if (["link", "image", "definition"].includes(node.type)) {
      const kind = node.type === "definition" ? referenceKinds.get(node.identifier) : node.type;
      if (!kind) {
        if (unsafeUrl(node.url)) throw new Error(`${sourcePath}: unsafe Markdown URL scheme`);
        return;
      }
      const resolved = node.type === "definition"
        ? definitionResolutions.get(node.identifier)
        : resolveRepositoryUrl(node.url, { sourcePath, sourceRoutes, repositoryFiles, repositoryDirectories, pinnedSourceUrl, base, mediaReferences, kind, sourceAnchors });
      if (resolved.sourceReference) {
        if (node.type === "definition") parent.children.splice(index, 1);
        else parent.children[index] = sourceReferenceNode(node, resolved.sourceReference, commit);
      } else node.url = resolved.url;
      return;
    }
    if (node.type !== "text" || ancestors.some((ancestor) => ["link", "linkReference", "definition", "code", "inlineCode", "html"].includes(ancestor.type))) return;
    // A preview tooltip inside a heading would join the heading text, so the
    // slugged id and the "On this page" entry would both swallow the page
    // title and source path it carries. Headings keep the plain link.
    const inHeading = ancestors.some((ancestor) => ancestor.type === "heading");
    const children = [];
    let cursor = 0;
    for (const match of node.value.matchAll(/\b(?:ADR|EPC|SPC|TSK|CAP)-\d{3,}(?:-\d{3,})?\b/g)) {
      if (!strictId(match[0])) continue;
      if (match.index > cursor) children.push({ type: "text", value: node.value.slice(cursor, match.index) });
      const target = strictTargets.get(match[0]);
      const record = target ? null : recordTargets.get(match[0]);
      if (target && inHeading && target.route === selfRoute) children.push({ type: "text", value: match[0] });
      else if (target && inHeading) children.push({ type: "html", value: `<a href="${escapeGeneratedHtml(target.route)}">${match[0]}</a>` });
      else if (target) children.push({ type: "html", value: strictIdPreview(match[0], target, `${sha256(sourcePath).slice(0, 10)}-${previewSequence++}`) });
      else if (record) children.push({ type: "html", value: recordReferenceLink(match[0], record) });
      else children.push({ type: "text", value: match[0] });
      cursor = match.index + match[0].length;
    }
    if (!children.length) return;
    if (cursor < node.value.length) children.push({ type: "text", value: node.value.slice(cursor) });
    parent.children.splice(index, 1, ...children);
  });
  return stringifyMarkdown(tree);
}

function escapeGeneratedHtml(value) {
  return String(value).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

// The Starlight shell already renders the page title; a leading depth-1
// heading that repeats it — allowing initial-letter case and a record-ID prefix such as
// "ADR-0053 — <title>" — would render the title twice (the ID stays visible
// in Record context and provenance). Any other heading is author content.
export function stripLeadingTitleHeading(markdown, title) {
  const tree = markdownTree(markdown);
  const first = tree.children[0];
  if (!first || first.type !== "heading" || first.depth !== 1) return markdown;
  const text = visibleNodeText(first).trim();
  const unprefixed = text.replace(/^(?:ADR|EPC|SPC|TSK|CAP)-\d+(?:-\d+)?\s*[—–:-]\s*/, "");
  // Only sentence-initial case is presentation; internal case may name an API.
  const matches = text === title || unprefixed === title || (unprefixed.slice(1) === title.slice(1)
    && unprefixed.slice(0, 1).toLowerCase() === title.slice(0, 1).toLowerCase());
  if (!matches) return markdown;
  tree.children.shift();
  return stringifyMarkdown(tree);
}

// Subject-led stage figures (utility presentation system, ADR-0053). A
// `cf-stage` fence authors a labeled left-to-right flow in plain text; the
// adapter renders it into generated HTML that uses only --cf-* tokens, so
// sources never hand-author portal markup and raw HTML stays escaped.
//
//   node line     NAME | sublabel @role     (role: accent|positive|warn|danger)
//   `->` line     next stage (nodes between arrows are parallel)
//   caption: …    one mono caption under the figure
//
// Invalid grammar fails the page loudly (bounded stale stub), never silently.
const STAGE_ROLES = new Set(["accent", "positive", "warn", "danger", "neutral"]);
const STAGE_LIMITS = { stages: 6, nodesPerStage: 5, name: 64, sublabel: 96, caption: 160 };

export function parseStageFence(source, context = "cf-stage") {
  const stages = [[]];
  let caption = null;
  for (const raw of String(source).split("\n")) {
    const line = raw.trim();
    if (!line) continue;
    if (line === "->") {
      if (!stages.at(-1).length) throw new Error(`${context}: '->' must follow at least one node`);
      if (stages.length >= STAGE_LIMITS.stages) throw new Error(`${context}: stage count exceeds ${STAGE_LIMITS.stages}`);
      stages.push([]);
      continue;
    }
    if (line.startsWith("caption:")) {
      if (caption !== null) throw new Error(`${context}: only one caption is allowed`);
      caption = line.slice("caption:".length).trim();
      if (!caption || caption.length > STAGE_LIMITS.caption) throw new Error(`${context}: caption must be 1 to ${STAGE_LIMITS.caption} characters`);
      continue;
    }
    const match = line.match(/^(.*?)(?:\|(.*?))?(?:@([a-z]+))?$/);
    const name = match[1].trim();
    const sublabel = (match[2] ?? "").trim();
    const role = (match[3] ?? "neutral").trim();
    if (!name || name.length > STAGE_LIMITS.name) throw new Error(`${context}: node name must be 1 to ${STAGE_LIMITS.name} characters`);
    if (sublabel.length > STAGE_LIMITS.sublabel) throw new Error(`${context}: node sublabel exceeds ${STAGE_LIMITS.sublabel} characters`);
    if (!STAGE_ROLES.has(role)) throw new Error(`${context}: unknown node role @${role}`);
    if (stages.at(-1).length >= STAGE_LIMITS.nodesPerStage) throw new Error(`${context}: node count per stage exceeds ${STAGE_LIMITS.nodesPerStage}`);
    stages.at(-1).push({ name, sublabel, role });
  }
  if (stages.length < 2) throw new Error(`${context}: a stage figure needs at least two '->'-separated stages`);
  if (!stages.at(-1).length) throw new Error(`${context}: the final stage needs at least one node`);
  return { stages, caption };
}

export function renderStageFigure({ stages, caption }) {
  const groups = stages.map((nodes) => {
    const items = nodes.map((node) => {
      const sub = node.sublabel ? `<span class="s">${escapeGeneratedHtml(node.sublabel)}</span>` : "";
      return `<li class="portal-stage-node" data-role="${node.role}"><span class="k">${escapeGeneratedHtml(node.name)}</span>${sub}</li>`;
    }).join("");
    return `<ul class="portal-stage-group">${items}</ul>`;
  }).join(`<div class="portal-stage-arrow" aria-hidden="true"></div>`);
  const figcaption = caption === null ? "" : `<figcaption>${escapeGeneratedHtml(caption)}</figcaption>`;
  return `<figure class="portal-stage"><div class="portal-stage-flow">${groups}</div>${figcaption}</figure>`;
}

export function renderStageFences(markdown, context = "cf-stage") {
  const tree = markdownTree(markdown);
  let changed = false;
  visitMarkdown(tree, (node, parent, index) => {
    if (node.type !== "code" || node.lang !== "cf-stage") return;
    parent.children[index] = { type: "html", value: renderStageFigure(parseStageFence(node.value, context)) };
    changed = true;
  });
  return changed ? stringifyMarkdown(tree) : markdown;
}

// Capability registry fences (ADR-0064). The canonical registry declares one
// YAML block per capability. The guide renders those blocks as structure: a
// generated summary table at the top of the page and a compact definition
// table where each fence stood, never a raw code block a reader must parse.
const CAPABILITY_FIELD_LIMIT = 16;
const CAPABILITY_VALUE_LIMIT = 1024;

export function capabilityFenceRecords(markdown, sourcePath = "capability registry") {
  const records = [];
  for (const node of markdownNodes(markdownTree(markdown), "code")) {
    const record = capabilityRecord(node, sourcePath);
    if (record !== null) records.push(record);
  }
  return records;
}

export function renderCapabilityFences(markdown, escapeCell, sourcePath = "capability registry") {
  const tree = markdownTree(markdown);
  let changed = false;
  visitMarkdown(tree, (node, parent, index) => {
    if (node.type !== "code") return;
    const record = capabilityRecord(node, sourcePath);
    if (record === null) return;
    const rows = record.fields.map(([field, value]) => `| ${escapeCell(field)} | ${escapeCell(value)} |`);
    parent.children[index] = { type: "html", value: `<div class="portal-definition">\n\n| Field | Value |\n|---|---|\n${rows.join("\n")}\n\n</div>` };
    changed = true;
  });
  return changed ? stringifyMarkdown(tree) : markdown;
}

// The generated summary replaces the hand written registry table when the
// source carries one, and otherwise opens the page.
export function placeCapabilityTable(markdown, generated) {
  if (generated === null) return markdown;
  const tree = markdownTree(markdown);
  const replacement = markdownTree(generated).children;
  const index = tree.children.findIndex((node) => node.type === "table" && visibleNodeText(node.children?.[0]?.children?.[0] ?? {}).trim().toLowerCase() === "capability");
  tree.children.splice(index < 0 ? 0 : index, index < 0 ? 0 : 1, ...replacement);
  return stringifyMarkdown(tree);
}

function capabilityRecord(node, sourcePath) {
  if (node.type !== "code" || !/^ya?ml$/i.test(node.lang ?? "")) return null;
  let parsed;
  try { parsed = YAML.parse(node.value); } catch { return null; }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
  const id = parsed.id;
  if (typeof id !== "string" || !/^CAP-\d{3,}$/.test(id)) return null;
  const entries = Object.entries(parsed);
  if (entries.length > CAPABILITY_FIELD_LIMIT) throw new Error(`${sourcePath}: capability ${id} declares more than ${CAPABILITY_FIELD_LIMIT} fields`);
  const fields = entries.map(([field, value]) => [field, capabilityValueText(value)]);
  if (fields.some(([field, value]) => field.length > 64 || value.length > CAPABILITY_VALUE_LIMIT)) {
    throw new Error(`${sourcePath}: capability ${id} declares a field beyond the rendered bounds`);
  }
  return { id, fields, lookup: new Map(fields) };
}

function capabilityValueText(value) {
  if (Array.isArray(value)) return value.map((item) => capabilityValueText(item)).join(", ");
  if (value === null || value === undefined) return "none";
  if (typeof value === "object") return Object.entries(value).map(([key, item]) => `${key}: ${capabilityValueText(item)}`).join("; ");
  return String(value).replace(/\s+/g, " ").trim();
}

const ALTITUDE_LAYERS = ["concept", "architecture", "technical"];

// Utility presentation system altitude grammar (ADR-0053): when a source
// authors the depth-2 sections Concept / Architecture / Technical, wrap each
// section in a tabpanel and emit a real tablist — the portal runtime
// (portal-tabs.js) shows exactly one panel at a time, keys the panel to the
// URL hash, and drives arrow-key selection. Without JavaScript the panels
// stack in document order. Sources that do not follow the grammar pass
// through unchanged; the markup is generated, never author-controlled.
export function decorateAltitude(markdown) {
  const tree = markdownTree(markdown);
  const slugger = new GithubSlugger();
  const sections = [];
  for (let index = 0; index < tree.children.length; index += 1) {
    const node = tree.children[index];
    if (node.type !== "heading") continue;
    const slug = slugger.slug(visibleNodeText(node));
    if (node.depth !== 2) continue;
    const label = visibleNodeText(node).trim().toLowerCase();
    if (ALTITUDE_LAYERS.includes(label)) sections.push({ index, label, slug });
  }
  if (sections.length < 2 || new Set(sections.map((section) => section.label)).size !== sections.length) return markdown;
  for (const section of sections) {
    section.end = tree.children.length;
    for (let index = section.index + 1; index < tree.children.length; index += 1) {
      const node = tree.children[index];
      if (node.type === "heading" && node.depth === 2) { section.end = index; break; }
    }
  }
  // Splice from the last boundary to the first so every position stays valid
  // in the pristine index space computed above.
  for (let cursor = sections.length - 1; cursor >= 0; cursor -= 1) {
    const section = sections[cursor];
    tree.children.splice(section.end, 0, { type: "html", value: "</section>" });
    tree.children.splice(section.index, 0, {
      type: "html",
      value: `<section class="portal-altitude" role="tabpanel" id="portal-panel-${section.label}" aria-labelledby="portal-tab-${section.label}" data-altitude="${section.label}">`,
    });
  }
  const tabs = sections.map((section, index) => {
    const title = `${section.label[0].toUpperCase()}${section.label.slice(1)}`;
    const selected = index === 0 ? ` aria-selected="true"` : ` aria-selected="false" tabindex="-1"`;
    return `<button type="button" role="tab" id="portal-tab-${section.label}" aria-controls="portal-panel-${section.label}"${selected} data-anchor="${escapeGeneratedHtml(section.slug)}">${title}</button>`;
  }).join("\n");
  tree.children.splice(sections[0].index, 0, { type: "html", value: `<div class="portal-altitude-tabs" role="tablist" aria-label="Altitude">\n${tabs}\n</div>` });
  return stringifyMarkdown(tree);
}

export function referencedIds(markdown) {
  const found = new Set();
  for (const node of markdownNodes(markdownTree(markdown), "text")) {
    for (const match of node.value.matchAll(/\b(?:ADR|EPC|SPC|TSK|CAP)-\d{3,}(?:-\d{3,})?\b/g)) {
      if (strictId(match[0])) found.add(match[0]);
    }
  }
  return [...found];
}

function markdownTree(markdown) {
  return unified().use(remarkParse).use(remarkGfm).parse(normalizeMarkdown(markdown));
}

function stringifyMarkdown(tree) {
  return unified().use(remarkStringify, { bullet: "-", fences: true, listItemIndent: "one" }).use(remarkGfm).stringify(tree).trimEnd();
}

function visitMarkdown(node, callback, parent = null, index = -1, ancestors = []) {
  if (parent) callback(node, parent, index, ancestors);
  if (!Array.isArray(node.children)) return;
  for (let cursor = node.children.length - 1; cursor >= 0; cursor -= 1) visitMarkdown(node.children[cursor], callback, node, cursor, [...ancestors, node]);
}

function markdownNodes(root, type) {
  const found = [];
  function visit(node) {
    if (node.type === type) found.push(node);
    if (Array.isArray(node.children)) node.children.forEach(visit);
  }
  visit(root);
  return found;
}

function visibleNodeText(node) {
  if (node.type === "text" || node.type === "inlineCode") return node.value;
  if (node.type === "image") return node.alt ?? "";
  if (!Array.isArray(node.children)) return "";
  return node.children.map(visibleNodeText).join("");
}

function unsafeUrl(value) {
  const normalized = String(value).replace(/&#(?:x0*3a|0*58);|&colon;/gi, ":").replace(/[\u0000-\u0020\u007f]+/g, "").toLowerCase();
  return /^(?:javascript|data|file|vbscript):/.test(normalized);
}

function resolveRepositoryUrl(value, { sourcePath, sourceRoutes, repositoryFiles, repositoryDirectories, pinnedSourceUrl, base, mediaReferences, kind, sourceAnchors }) {
  if (unsafeUrl(value)) throw new Error(`${sourcePath}: unsafe Markdown URL scheme`);
  if (/^(?:https?:|mailto:)/i.test(value)) {
    if (kind === "image") throw new Error(`${sourcePath}: remote images are not imported: ${value}`);
    return { url: value };
  }
  if (value.startsWith("#")) {
    assertPortalFragment(value, sourcePath, sourceAnchors);
    return { url: value };
  }
  if (/^[a-z][a-z0-9+.-]*:/i.test(value) || value.startsWith("//") || value.startsWith("/")) throw new Error(`${sourcePath}: unsupported Markdown URL: ${value}`);
  const match = String(value).match(/^([^?#]*)(\?[^#]*)?(#.*)?$/);
  if (!match || !match[1]) throw new Error(`${sourcePath}: invalid repository-relative Markdown URL: ${value}`);
  let decoded;
  try { decoded = decodeURIComponent(match[1]); } catch { throw new Error(`${sourcePath}: malformed percent-encoding in Markdown URL: ${value}`); }
  if (decoded.includes("\\") || /[\u0000-\u001f\u007f]/.test(decoded)) throw new Error(`${sourcePath}: invalid repository-relative Markdown URL: ${value}`);
  const resolved = path.posix.normalize(path.posix.join(path.posix.dirname(sourcePath), decoded));
  const safe = safeRelative(resolved, `${sourcePath} Markdown target`);
  const suffix = `${match[2] ?? ""}${match[3] ?? ""}`;
  if (sourceRoutes.has(safe)) {
    if (match[3]) assertPortalFragment(match[3], safe, sourceAnchors);
    return { url: `${withBase(base, sourceRoutes.get(safe))}${suffix}` };
  }
  if (kind === "link" && repositoryFiles.has(safe)) {
    const href = pinnedSourceUrl(safe, "file");
    return href === null ? { sourceReference: safe } : { url: `${href}${suffix}` };
  }
  const directory = safe.endsWith("/") ? safe.slice(0, -1) : safe;
  if (kind === "link" && repositoryDirectories.has(directory)) {
    const href = pinnedSourceUrl(directory, "directory");
    return href === null ? { sourceReference: directory } : { url: `${href}${suffix}` };
  }
  if (/\.(?:md|mdx)$/i.test(safe) || kind === "link" && !path.posix.extname(safe)) throw new Error(`${sourcePath}: repository document target does not exist: ${safe}`);
  const extension = path.posix.extname(safe).toLowerCase();
  const mediaExtensions = new Set([".gif", ".jpeg", ".jpg", ".png", ".webp"]);
  if (!mediaExtensions.has(extension)) {
    throw new Error(`${sourcePath}: unsupported local media type: ${safe}`);
  }
  const mediaRoute = `media/${sha256(safe).slice(0, 16)}-${path.posix.basename(safe)}`;
  mediaReferences.set(safe, mediaRoute);
  const encodedMediaRoute = mediaRoute.split("/").map(strictUrlSegment).join("/");
  return { url: `${base}${encodedMediaRoute}${suffix}` };
}

function assertPortalFragment(fragment, targetPath, sourceAnchors) {
  let decoded;
  try { decoded = decodeURIComponent(fragment.slice(1)); }
  catch { throw new Error(`${targetPath}: malformed Markdown fragment: ${fragment}`); }
  if (!decoded || /[\u0000-\u001f\u007f]/.test(decoded) || !sourceAnchors.get(targetPath)?.has(decoded)) {
    throw new Error(`${targetPath}: Markdown fragment does not match a rendered heading: ${fragment}`);
  }
}

export function headingAnchors(markdown) {
  const slugger = new GithubSlugger();
  return new Set(markdownNodes(markdownTree(markdown), "heading").map((node) => slugger.slug(visibleNodeText(node))));
}

function sourceReferenceNode(node, sourcePath, commit) {
  const label = visibleNodeText(node).trim() || sourcePath;
  const revision = /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(commit) ? commit.slice(0, 12) : "pinned commit";
  return {
    type: "html",
    value: `<span class="portal-source-reference">${escapeGeneratedHtml(label)} (<code>${escapeGeneratedHtml(sourcePath)}</code> at <code>${revision}</code>)</span>`,
  };
}

// An id whose record is not a portal source is never a dangling link: it
// resolves to the repository file at the built commit, or to the pointer page
// for its folder when the portal has no provider URL to pin.
function recordReferenceLink(id, record) {
  return `<a class="portal-record-link" href="${escapeGeneratedHtml(record.href)}" title="${escapeGeneratedHtml(record.title)}">${id}</a>`;
}

function strictIdPreview(id, target, suffix) {
  const statusText = target.status;
  const status = typeof statusText === "string" && statusText ? `<span>Status: ${escapeGeneratedHtml(statusText)}</span>` : "";
  const previewId = `portal-preview-${suffix}`;
  return `<span class="portal-id-preview"><a href="${escapeGeneratedHtml(target.route)}" aria-describedby="${previewId}">${id}</a><span id="${previewId}" role="tooltip"><strong>${escapeGeneratedHtml(target.title)}</strong>${status}<span>Source: <code>${escapeGeneratedHtml(target.source_path)}</code></span></span></span>`;
}

export function amendmentHeadings(markdown) {
  const headings = [];
  for (const node of markdownNodes(markdownTree(markdown), "heading")) {
    if (node.depth !== 2) continue;
    const match = visibleNodeText(node).trim().match(/^((?:Note|Update|Correction)\b.*\b\d{4}-\d{2}-\d{2}\b.*)$/i);
    if (match) headings.push(match[1].trim());
  }
  return headings;
}

// Record files are found by the id their filename declares, so a template, a
// README or any other note in the folder is counted by nobody and linked by
// nobody. The scan reads the committed inventory, never the working tree.
export function recordFilesFor(pointer, repositoryFiles) {
  const prefix = pointer.id_prefix;
  const pattern = prefix === "TSK"
    ? /^(TSK-\d{3,}(?:-\d{3,})?)(?:-[^/]*)?\.md$/
    : new RegExp(`^(${prefix}-\\d{3,})(?:-[^/]*)?\\.md$`);
  const found = new Map();
  for (const sourcePath of repositoryFiles.keys()) {
    if (!sourcePath.startsWith(`${pointer.folder}/`)) continue;
    const match = path.posix.basename(sourcePath).match(pattern);
    if (!match || !strictId(match[1]) || found.has(match[1])) continue;
    found.set(match[1], { id: match[1], path: sourcePath });
  }
  return [...found.values()].sort((left, right) => compareDeterministicText(left.id, right.id));
}

export function strictId(value) {
  return /^(?:(?:ADR|EPC|SPC|CAP)-\d{3,}|TSK-\d{3,}(?:-\d{3,})?)$/.test(value);
}

export function recoverUnavailableIds(text, sourcePath) {
  const normalized = String(text).replace(/^\uFEFF/, "").replace(/\r\n?/g, "\n");
  const recovered = [];
  for (const line of normalized.split("\n")) {
    const match = line.match(/^[ \t]*id:[ \t]*(.*)$/);
    if (!match) continue;
    const value = match[1].trimStart();
    let candidate;
    if (value.startsWith("\"") || value.startsWith("'")) {
      const quote = value[0];
      const end = value.indexOf(quote, 1);
      if (end < 0) continue;
      const trailing = value.slice(end + 1).trim();
      if (trailing && !trailing.startsWith("#")) continue;
      candidate = value.slice(1, end);
    } else {
      const token = value.match(/^(\S+)(.*)$/);
      if (!token) continue;
      const trailing = token[2].trim();
      if (trailing && !trailing.startsWith("#")) continue;
      candidate = token[1];
    }
    if (strictId(candidate) && !recovered.includes(candidate)) recovered.push(candidate);
  }
  if (sourcePath === "docs/capabilities.md") return [...recovered].sort(compareDeterministicText);
  const filenameId = path.posix.basename(sourcePath, ".md").toUpperCase();
  if (strictId(filenameId)) return [filenameId];
  return recovered.length ? [recovered[0]] : [];
}

export function collectPageIds(frontmatter, text, sourcePath) {
  const declared = [];
  if (Object.hasOwn(frontmatter, "id")) {
    if (typeof frontmatter.id !== "string" || !strictId(frontmatter.id)) throw new Error(`${sourcePath}: declared id is invalid`);
    declared.push(frontmatter.id);
  }
  if (sourcePath !== "docs/capabilities.md") {
    if (declared.length) return declared;
    const filenameId = path.posix.basename(sourcePath, ".md").toUpperCase();
    return strictId(filenameId) ? [filenameId] : [];
  }
  for (const block of yamlFences(text)) {
    const record = YAML.parse(block);
    if (!record || typeof record !== "object" || Array.isArray(record) || !Object.hasOwn(record, "id")) continue;
    if (typeof record.id !== "string" || !strictId(record.id)) throw new Error(`${sourcePath}: declared capability id is invalid`);
    declared.push(record.id);
  }
  const duplicates = declared.filter((id, index) => declared.indexOf(id) !== index);
  if (duplicates.length) throw new Error(`${sourcePath}: duplicate identity declaration ${[...new Set(duplicates)].join(", ")}`);
  return [...declared].sort();
}

const relationshipFields = [
  ["epic_id", "epic"], ["epics", "epic"], ["specs", "spec"],
  ["depends_on", "depends_on"], ["capabilities", "capability"],
  ["adrs", "decision"], ["related", "related"], ["superseded_by", "superseded_by"],
];

export function extractRelationships(frontmatter, sourcePath = "frontmatter") {
  return relationshipFields.flatMap(([field, kind]) => {
    if (!Object.hasOwn(frontmatter, field)) return [];
    const value = frontmatter[field];
    if (value === null) return [];
    const values = Array.isArray(value) ? value : [value];
    if (values.some((item) => typeof item !== "string" || !strictId(item))) throw new Error(`${sourcePath}: declared ${field} relationship is invalid`);
    return values.map((target) => ({ type: kind, target }));
  });
}

export function extractPageRelationships(frontmatter, text, sourcePath) {
  const sourceId = typeof frontmatter.id === "string" && strictId(frontmatter.id) ? frontmatter.id : null;
  const relationships = extractRelationships(frontmatter, sourcePath).map((relationship) => ({ ...relationship, source_id: sourceId }));
  if (sourcePath !== "docs/capabilities.md") return relationships;
  for (const block of yamlFences(text)) {
    const record = YAML.parse(block);
    if (record && typeof record === "object" && !Array.isArray(record) && strictId(record.id ?? "")) {
      relationships.push(...extractRelationships(record, sourcePath).map((relationship) => ({ ...relationship, source_id: record.id })));
    }
  }
  return relationships;
}

function yamlFences(text) {
  return markdownNodes(markdownTree(text), "code")
    .filter((node) => /^ya?ml$/i.test(node.lang ?? ""))
    .map((node) => node.value);
}

export function excerptFor(text) {
  text = normalizeMarkdown(text);
  const { body } = parseMarkdown(text, "excerpt source");
  const bodyStart = text.slice(0, text.length - body.length).split("\n").length - 1;
  const paragraph = markdownNodes(markdownTree(body), "paragraph").find((node) => visibleNodeText(node).trim());
  if (!paragraph?.position) return null;
  const start = bodyStart + paragraph.position.start.line;
  const end = Math.min(bodyStart + paragraph.position.end.line, start + 2);
  return { start, end, text: text.split("\n").slice(start - 1, end).join("\n") };
}

export function validateBase(value) {
  if (typeof value !== "string" || value.length > 256 || !/^\/(?:[A-Za-z0-9._~-]+\/)*$/.test(value)) {
    throw new Error("base: expected a canonical absolute URL path of RFC 3986 unreserved segments ending in /");
  }
  if (value.split("/").some((part) => part === "." || part === "..")) throw new Error("base: traversal is not allowed");
  return value;
}

export function withBase(base, route) {
  const safeRoute = safeRelative(route, "route");
  const encodedRoute = safeRoute.split("/").map(strictUrlSegment).join("/");
  return `${validateBase(base)}${encodedRoute}/`.replace(/^\/\//, "/");
}

// The themes a configuration may name: the three skins, then the two earlier
// names kept as aliases (signal is graphite, folio is sage). The Rust
// validator holds the same list and a parity test compares them.
export const PORTAL_THEMES = Object.freeze(["graphite", "slate", "sage", "signal", "folio"]);

export function validatePortalConfig(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("portal.config.json: expected an object");
  const allowed = new Set(["schema_version", "title", "description", "theme", "repository_url", "repository_root", "release_version", "primitive_tokens", "source_roots", "exclude", "layers", "records", "page_carriers", "page_classes", "figures", "base"]);
  for (const key of Object.keys(value)) if (!allowed.has(key)) throw new Error(`portal.config.json: unknown key ${key}`);
  if (value.schema_version !== 1) throw new Error("portal.config.json: unsupported schema_version");
  boundedString(value.title, "title", 1, 120);
  boundedString(value.description, "description", 1, 400);
  if (!PORTAL_THEMES.includes(value.theme)) throw new Error("portal.config.json: theme must be graphite, slate or sage (signal and folio remain aliases)");
  if (value.repository_url !== null) {
    boundedString(value.repository_url, "repository_url", 1, 2048);
    if (!validRepositoryUrl(value.repository_url)) throw new Error("repository_url: expected an HTTPS repository URL with an ASCII or punycode host and without credentials, query, or fragment");
  }
  if (value.release_version !== null && (typeof value.release_version !== "string" || !/^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$/.test(value.release_version))) throw new Error("release_version: expected a bounded printable version identifier or null");
  if (value.primitive_tokens !== null) safeRelative(value.primitive_tokens, "primitive_tokens");
  boundedString(value.repository_root, "repository_root", 1, 256);
  validateBase(value.base);
  validatePathArray(value.source_roots, "source_roots", 1, 32);
  validatePathArray(value.exclude, "exclude", 0, 128);
  if (!Array.isArray(value.layers) || value.layers.length < 3 || value.layers.length > 12) throw new Error("portal.config.json: layers must define 3 to 12 information layers");
  const ids = new Set(); let fallback = 0;
  for (const layer of value.layers) {
    if (!layer || typeof layer !== "object" || Array.isArray(layer)) throw new Error("portal.config.json: each layer must be an object");
    const layerAllowed = new Set(["id", "label", "description", "paths", "prefixes", "fallback"]);
    for (const key of Object.keys(layer)) if (!layerAllowed.has(key)) throw new Error(`portal.config.json: unknown layer key ${key}`);
    if (typeof layer.id !== "string" || !/^[a-z][a-z0-9-]*$/.test(layer.id)) throw new Error("portal.config.json: each layer needs a safe id");
    boundedString(layer.label, "layer label", 1, 80);
    boundedString(layer.description, "layer description", 1, 300);
    if (ids.has(layer.id)) throw new Error(`portal.config.json: duplicate layer ${layer.id}`);
    ids.add(layer.id);
    validatePathArray(layer.paths ?? [], `layer ${layer.id} paths`, 0, 128);
    validatePathArray(layer.prefixes ?? [], `layer ${layer.id} prefixes`, 0, 128);
    if (layer.fallback !== undefined && typeof layer.fallback !== "boolean") throw new Error(`portal.config.json: layer ${layer.id} fallback must be boolean`);
    if (layer.fallback === true) fallback += 1;
  }
  if (fallback !== 1) throw new Error("portal.config.json: exactly one layer must be the fallback");
  value.records = validateRecordsSwitch(value.records, value.layers);
  value.page_carriers = validatePageCarriers(value.page_carriers);
  value.page_classes = validatePageClasses(value.page_classes);
  value.figures = validateFigureBindings(value.figures);
  return value;
}

// A page leaves the explanatory class only by a declaration here, naming its
// source or a source prefix. Pass-through takes one reason from a closed set;
// the no-relationship reason also records the judgment in a note. Neither
// class drops a route or changes the bytes of its source.
function validatePageClasses(value) {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > 256) throw new Error("portal.config.json: page_classes must be an array of at most 256 entries");
  const keys = new Set();
  for (const entry of value) {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) throw new Error("portal.config.json: each page_classes entry must be an object");
    for (const key of Object.keys(entry)) if (!["source", "prefix", "class", "reason", "note", "derive"].includes(key)) throw new Error(`portal.config.json: unknown page_classes key ${key}`);
    const named = ["source", "prefix"].filter((key) => entry[key] !== undefined);
    if (named.length !== 1) throw new Error("portal.config.json: each page_classes entry names exactly one source or prefix");
    const target = portablePathKey(entry[named[0]], `page_classes ${named[0]}`);
    if (keys.has(target)) throw new Error(`portal.config.json: duplicate page_classes entry ${entry[named[0]]}`);
    keys.add(target);
    if (!["illustrated", "pass-through", "derived-lookup"].includes(entry.class)) throw new Error(`portal.config.json: page_classes ${entry[named[0]]} class must be illustrated, pass-through or derived-lookup`);
    if (entry.class === "pass-through") {
      if (!PAGE_CLASS_REASONS.includes(entry.reason)) throw new Error(`portal.config.json: pass-through ${entry[named[0]]} needs a reason from ${PAGE_CLASS_REASONS.join(", ")}`);
      if (entry.reason === "no-relationship") boundedString(entry.note, `page_classes ${entry[named[0]]} note`, 1, 300);
      else if (entry.note !== undefined) boundedString(entry.note, `page_classes ${entry[named[0]]} note`, 1, 300);
    } else if (entry.reason !== undefined || entry.note !== undefined) throw new Error(`portal.config.json: only a pass-through entry carries a reason: ${entry[named[0]]}`);
    if (entry.class === "derived-lookup") {
      if (!DERIVED_LOOKUPS.includes(entry.derive) || entry.source === undefined) throw new Error(`portal.config.json: derived-lookup ${entry[named[0]]} names one source and derive ${DERIVED_LOOKUPS.join(" or ")}`);
    } else if (entry.derive !== undefined) throw new Error(`portal.config.json: only a derived-lookup entry names derive: ${entry[named[0]]}`);
  }
  return value;
}

// Every figure is bound to its page here, by route and by an altitude panel
// (optionally narrowed to a section anchor inside that panel) or a section
// anchor of an illustrated source, never by a marker inside a source. The
// declaration file is a committed repository input.
function validateFigureBindings(value) {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > 512) throw new Error("portal.config.json: figures must be an array of at most 512 bindings");
  const seen = new Set();
  for (const binding of value) {
    if (!binding || typeof binding !== "object" || Array.isArray(binding)) throw new Error("portal.config.json: each figures binding must be an object");
    for (const key of Object.keys(binding)) if (!["declaration", "route", "panel", "anchor"].includes(key)) throw new Error(`portal.config.json: unknown figures key ${key}`);
    safeRelative(binding.declaration, "figures declaration");
    if (!binding.declaration.endsWith(".json")) throw new Error(`portal.config.json: figure declaration ${binding.declaration} must be a JSON file`);
    safeRelative(binding.route, "figures route");
    if (binding.panel !== undefined && !ALTITUDE_PANELS.includes(binding.panel)) throw new Error(`portal.config.json: figure ${binding.declaration} panel must be one of ${ALTITUDE_PANELS.join(", ")}`);
    if (binding.anchor !== undefined && (typeof binding.anchor !== "string" || !/^[\p{L}\p{N}_-]{1,200}$/u.test(binding.anchor))) throw new Error(`portal.config.json: figure ${binding.declaration} anchor must be a heading slug`);
    const key = `${binding.route}\u0000${binding.declaration}`;
    if (seen.has(key)) throw new Error(`portal.config.json: figure ${binding.declaration} is bound to ${binding.route} twice`);
    seen.add(key);
  }
  return value;
}

// A page whose subject is its own carrier declares that here, per source. The
// panels, and the alternates each one accepts, are the composition gate's own
// table, so a configuration can never invent a carrier the rules do not know
// or quietly let a page off the carrier its altitude calls for.
function validatePageCarriers(value) {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > 64) throw new Error("portal.config.json: page_carriers must be an array of at most 64 entries");
  const panels = Object.keys(PANEL_CARRIER_ALTERNATES);
  const sources = new Set();
  for (const entry of value) {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) throw new Error("portal.config.json: each page_carriers entry must be an object");
    const allowed = new Set(["source", ...panels]);
    for (const key of Object.keys(entry)) if (!allowed.has(key)) throw new Error(`portal.config.json: unknown page_carriers key ${key}`);
    const source = portablePathKey(entry.source, "page_carriers source");
    if (sources.has(source)) throw new Error(`portal.config.json: duplicate page_carriers entry ${entry.source}`);
    sources.add(source);
    const declared = panels.filter((panel) => entry[panel] !== undefined);
    if (declared.length === 0) throw new Error(`portal.config.json: page_carriers entry ${entry.source} declares no panel carrier`);
    for (const panel of declared) {
      const alternates = Object.keys(PANEL_CARRIER_ALTERNATES[panel]);
      if (!alternates.includes(entry[panel])) throw new Error(`portal.config.json: ${entry.source} ${panel} carrier must be one of ${alternates.join(", ")}`);
    }
  }
  return value;
}

// The records switch (ADR-0064). Off, the configured pointer folders leave the
// page set entirely and one generated page points at the folders instead. On,
// a project takes the lookup form for those sources and steps outside the
// guide doctrine, so it declares no pointers.
export const RECORD_ID_PREFIXES = Object.freeze(["ADR", "CAP", "EPC", "SPC", "TSK"]);

export function validateRecordsSwitch(value, layers) {
  if (value === undefined || value === null) return { enabled: false, layer: null, pointers: [] };
  if (typeof value !== "object" || Array.isArray(value)) throw new Error("portal.config.json: records must be an object");
  for (const key of Object.keys(value)) if (!["enabled", "layer", "pointers"].includes(key)) throw new Error(`portal.config.json: unknown records key ${key}`);
  if (typeof value.enabled !== "boolean") throw new Error("portal.config.json: records.enabled must be boolean");
  const pointers = value.pointers ?? [];
  if (!Array.isArray(pointers) || pointers.length > 16) throw new Error("portal.config.json: records.pointers must be 0 to 16 folders");
  const folders = new Set();
  for (const pointer of pointers) {
    if (!pointer || typeof pointer !== "object" || Array.isArray(pointer)) throw new Error("portal.config.json: each records pointer must be an object");
    for (const key of Object.keys(pointer)) if (!["folder", "id_prefix", "purpose"].includes(key)) throw new Error(`portal.config.json: unknown records pointer key ${key}`);
    safeRelative(pointer.folder, "records pointer folder");
    if (!RECORD_ID_PREFIXES.includes(pointer.id_prefix)) throw new Error(`portal.config.json: records pointer id_prefix must be one of ${RECORD_ID_PREFIXES.join(", ")}`);
    boundedString(pointer.purpose, "records pointer purpose", 1, 200);
    const key = portablePathKey(pointer.folder, "records pointer folder");
    if (folders.has(key)) throw new Error(`portal.config.json: duplicate records pointer folder ${pointer.folder}`);
    folders.add(key);
  }
  const layer = value.layer ?? null;
  if (layer !== null && !layers.some((item) => item.id === layer)) throw new Error(`portal.config.json: records.layer must name a configured layer: ${layer}`);
  if (pointers.length && value.enabled) throw new Error("portal.config.json: records pointers describe folders the portal does not publish, so they need records.enabled false");
  if (pointers.length && layer === null) throw new Error("portal.config.json: records pointers need records.layer to place their pointer page");
  if (!value.enabled) {
    for (const item of layers) {
      const owned = [...(item.paths ?? []), ...(item.prefixes ?? [])];
      if (owned.length && owned.every((entry) => pointers.some((pointer) => entry === pointer.folder || entry.startsWith(`${pointer.folder}/`)))) {
        throw new Error(`portal.config.json: layer ${item.id} publishes only record folders while records are disabled`);
      }
    }
  }
  return { enabled: value.enabled, layer, pointers: pointers.map((pointer) => ({ folder: pointer.folder, id_prefix: pointer.id_prefix, purpose: pointer.purpose })) };
}

export function validRepositoryUrl(value) {
  if (typeof value !== "string" || value.length < 1 || value.length > 2048 || /[\s\\]/u.test(value) || !value.startsWith("https://")) return false;
  const rawAuthority = value.slice("https://".length).split("/", 1)[0];
  if (!rawAuthority || !/^[\x21-\x7e]+$/.test(rawAuthority) || rawAuthority.includes("@") || rawAuthority.includes("%")) return false;
  if (rawAuthority.startsWith("[")) {
    const close = rawAuthority.indexOf("]");
    const suffix = close < 0 ? "invalid" : rawAuthority.slice(close + 1);
    if (close < 2 || suffix && !validPortSuffix(suffix)) return false;
  } else {
    const parts = rawAuthority.split(":");
    if (parts.length > 2 || parts.length === 2 && !validPortSuffix(`:${parts[1]}`)) return false;
  }
  let url;
  try { url = new URL(value); } catch { return false; }
  if (url.protocol !== "https:" || url.username || url.password || url.search || url.hash) return false;
  const host = url.hostname;
  if (host.startsWith("[")) return /^\[(?=.*:)[0-9a-f:.]+\]$/i.test(host);
  return host.length <= 253 && host.split(".").every((label) => /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/i.test(label));
}

function validPortSuffix(value) {
  return /^:\d{1,5}$/.test(value) && Number(value.slice(1)) <= 65_535;
}

// Reader-selectable skins, not only the initial config theme. Browser tests
// bind these validation backgrounds to the actual utility CSS.
export const PORTAL_ACCENT_BACKGROUNDS = Object.freeze({
  graphite: { light: ["#ffffff", "#ffffff", "#ececec", "#dcefec"], dark: ["#232323", "#2b2b2b", "#303030", "#1e3c39"] },
  slate: { light: ["#f8fafc", "#ffffff", "#e2e7ed", "#f5e3d9"], dark: ["#1a2028", "#222a34", "#262e39", "#3e2b20"] },
  sage: { light: ["#fafbf8", "#ffffff", "#e8ece6", "#dcece5"], dark: ["#1d2320", "#252c28", "#29312c", "#213b32"] },
});

export function portalSkinOrder(theme) {
  const initial = ({ signal: "graphite", folio: "sage" })[theme] ?? theme;
  return [initial, ...Object.keys(PORTAL_ACCENT_BACKGROUNDS).filter((skin) => skin !== initial)];
}

export function validatePrimitiveTokens(value, theme) {
  if (!value || typeof value !== "object" || Array.isArray(value) || Object.keys(value).sort().join(",") !== "dark,light,schema_version" || value.schema_version !== 1) {
    throw new Error("primitive token import must contain exactly schema_version, light, and dark");
  }
  const skins = portalSkinOrder(theme);
  for (const mode of ["light", "dark"]) {
    const record = value[mode];
    if (!record || typeof record !== "object" || Array.isArray(record) || Object.keys(record).sort().join(",") !== "accent" || !/^#[a-fA-F0-9]{6}$/.test(record.accent ?? "")) {
      throw new Error(`primitive token ${mode} mode must contain exactly one six-digit accent color`);
    }
    for (const skin of skins) {
      for (const background of PORTAL_ACCENT_BACKGROUNDS[skin][mode]) {
        if (contrastRatio(record.accent, background) < 4.5) throw new Error(`primitive token ${mode} accent does not meet 4.5:1 contrast against the ${skin} portal surface or selected background`);
      }
    }
  }
  return value;
}

export function renderPrimitiveTokenCss(tokens) {
  if (tokens === null) return "/* No project primitive-token influence configured. */\n";
  const declarations = (accent) => `--cf-accent: ${accent}; --cf-accent-strong: ${accent}; --sl-color-accent: var(--cf-accent);`;
  return `:root, :root[data-cfp-skin] { ${declarations(tokens.light.accent)} }\n:root[data-theme="dark"], :root[data-theme="dark"][data-cfp-skin] { ${declarations(tokens.dark.accent)} }\n`;
}

export function localRouteFor(sourcePath, configuredRoots) {
  const matchingRoots = configuredRoots.filter((root) => sourcePath.startsWith(`${root}/`))
    .sort((left, right) => right.length - left.length || compareDeterministicText(left, right));
  if (!matchingRoots.length) throw new Error(`source does not belong to a configured source root: ${sourcePath}`);
  const route = sourcePath.slice(matchingRoots[0].length + 1).replace(/\.md$/, "");
  const parts = route.split("/");
  if (parts.at(-1) === "index") parts.pop();
  if (!parts.length || parts.at(-1) === "404") throw new Error(`source claims a reserved generated route: ${sourcePath}`);
  return safeRelative(parts.join("/"), "route");
}

export function strictUrlSegment(value) {
  return encodeURIComponent(value).replace(/[!'()*]/g, (character) => `%${character.charCodeAt(0).toString(16).toUpperCase()}`);
}

export function committedDirectoryPaths(repositoryFiles) {
  if (!(repositoryFiles instanceof Map)) throw new Error("repository inventory must be a Map");
  const directories = new Set();
  for (const sourcePath of repositoryFiles.keys()) {
    let directory = path.posix.dirname(safeRelative(sourcePath, "committed repository path"));
    while (directory !== ".") {
      directories.add(directory);
      directory = path.posix.dirname(directory);
    }
  }
  return directories;
}

export function pinnedSourceUrl(repositoryUrl, commit, sourcePath, target = "file") {
  if (typeof repositoryUrl !== "string") return null;
  if (!["file", "directory"].includes(target)) throw new Error(`unsupported repository source target: ${target}`);
  const repository = new URL(repositoryUrl);
  const root = repositoryUrl.replace(/\/$/, "").replace(/\.git$/, "");
  const encodedPath = sourcePath.split("/").map(strictUrlSegment).join("/");
  const host = repository.hostname.toLowerCase();
  if (host === "github.com") return `${root}/${target === "directory" ? "tree" : "blob"}/${commit}/${encodedPath}`;
  if (host === "gitlab.com") return `${root}/-/${target === "directory" ? "tree" : "blob"}/${commit}/${encodedPath}`;
  if (host === "bitbucket.org") return `${root}/src/${commit}/${encodedPath}${target === "directory" ? "/" : ""}`;
  return null;
}

function contrastRatio(left, right) {
  const luminance = (hex) => {
    const channels = [1, 3, 5].map((offset) => Number.parseInt(hex.slice(offset, offset + 2), 16) / 255).map((channel) => channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4);
    return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
  };
  const [bright, dark] = [luminance(left), luminance(right)].sort((a, b) => b - a);
  return (bright + 0.05) / (dark + 0.05);
}

function boundedString(value, label, min, max) {
  if (typeof value !== "string" || value.length < min || value.length > max) throw new Error(`${label}: expected ${min} to ${max} characters`);
}

function validatePathArray(value, label, min, max) {
  if (!Array.isArray(value) || value.length < min || value.length > max) throw new Error(`${label}: expected ${min} to ${max} paths`);
  const paths = value.map((item) => safeRelative(item, label));
  if (new Set(paths.map((item) => portablePathKey(item, label))).size !== paths.length) throw new Error(`${label}: duplicate or case-colliding path`);
}

// An illustrated or pass-through source renders as it is. Its region is the
// body after the frontmatter, less one leading level-one heading that repeats
// the page title (the shell already renders the title). The validator
// re-derives the same start from the committed bytes, so this rule is kept to
// raw lines: blank lines, one ATX heading, blank lines.
export function asIsRegionStart(body, title) {
  const lines = body.split("\n");
  let index = 0;
  let offset = 0;
  const advanceBlank = () => {
    while (index < lines.length - 1 && lines[index].trim() === "") { offset += lines[index].length + 1; index += 1; }
  };
  advanceBlank();
  const heading = (lines[index] ?? "").match(/^ {0,3}#[ \t]+(.*?)(?:[ \t]+#+)?[ \t]*$/);
  if (!heading || index >= lines.length - 1 || !asIsTitleMatches(heading[1], title)) return 0;
  offset += lines[index].length + 1;
  index += 1;
  advanceBlank();
  return offset;
}

// Whether an as-is region still carries a level-one heading once the title
// the adapter drops is gone: a second title, a title under a leading comment,
// or deliberate h1 sections. The site's Markdown step then renders every
// heading in the region one level lower (h6 stays h6), so the page title is
// the only h1 and the source keeps its structure; the bytes never change.
export function asIsHeadingsDemoted(source) {
  return markdownNodes(markdownTree(source), "heading").some((node) => node.depth === 1);
}

export function asIsTitleMatches(raw, title) {
  const visible = String(raw).replace(/[`*_]/g, "").trim();
  const wanted = String(title).replace(/[`*_]/g, "").trim();
  const unprefixed = visible.replace(/^(?:ADR|EPC|SPC|TSK|CAP)-\d+(?:-\d+)?\s*[\u2014\u2013:-]\s*/, "");
  return visible === wanted || unprefixed === wanted
    || (unprefixed.slice(1) === wanted.slice(1) && unprefixed.slice(0, 1).toLowerCase() === wanted.slice(0, 1).toLowerCase());
}

// The destinations an as-is region's links resolve to in the portal. The
// bytes stay as the source wrote them; the site's Markdown step reads this
// map and points each link at its route, its pinned file or its fragment,
// exactly as a composed page's links resolve.
export function resolveAsIsLinks(markdown, options) {
  if (/<!--\s*codeflow-/i.test(markdown)) throw new Error(`${options.sourcePath}: an as-is source may not carry a codeflow marker comment`);
  const tree = markdownTree(markdown);
  const referenceKinds = new Map();
  visitMarkdown(tree, (node) => {
    if (!["linkReference", "imageReference"].includes(node.type)) return;
    const kind = node.type === "imageReference" ? "image" : "link";
    const prior = referenceKinds.get(node.identifier);
    if (prior && prior !== kind) throw new Error(`${options.sourcePath}: reference ${node.identifier} is used as both a link and an image`);
    referenceKinds.set(node.identifier, kind);
  });
  const links = {};
  visitMarkdown(tree, (node) => {
    if (!["link", "image", "definition"].includes(node.type)) return;
    const kind = node.type === "definition" ? referenceKinds.get(node.identifier) : node.type;
    if (kind === undefined) {
      if (unsafeUrl(node.url)) throw new Error(`${options.sourcePath}: unsafe Markdown URL scheme`);
      return;
    }
    const key = `${kind}:${node.url}`;
    const resolved = Object.hasOwn(links, key) ? links[key] : resolveRepositoryUrl(node.url, { ...options, kind });
    links[key] = resolved.sourceReference ? { code: resolved.sourceReference } : resolved.url !== undefined ? { url: resolved.url } : resolved;
    if (node.type === "definition") links[`reference:${node.identifier}`] = links[key];
  });
  return links;
}

// A figure bound to an altitude panel sits directly under that panel's
// heading. The heading must exist: a binding to a panel the source does not
// author is a configuration error, never a silent drop.
// Each block is a companion string placed directly under its panel heading,
// or { value, heading } placed directly under the one heading inside that
// panel whose slug equals the slug of `heading`, the source text of the
// anchored heading the adapter already proved lies inside the panel.
export function insertPanelFigures(markdown, blocksByPanel, sourcePath) {
  if (!blocksByPanel.size) return markdown;
  const tree = markdownTree(markdown);
  const found = new Map();
  tree.children.forEach((node, index) => {
    if (node.type !== "heading" || node.depth !== 2) return;
    const label = visibleNodeText(node).trim().toLowerCase();
    if (blocksByPanel.has(label) && !found.has(label)) found.set(label, index);
  });
  for (const panel of blocksByPanel.keys()) {
    if (!found.has(panel)) throw new Error(`${sourcePath}: a figure is bound to the ${panel} panel, but the source has no "## ${panel[0].toUpperCase()}${panel.slice(1)}" section`);
  }
  const inserts = [];
  for (const [panel, index] of found) {
    const next = tree.children.findIndex((node, position) => position > index && node.type === "heading" && node.depth <= 2);
    const end = next === -1 ? tree.children.length : next;
    const atPanel = [];
    for (const block of blocksByPanel.get(panel)) {
      if (typeof block === "string") { atPanel.push(block); continue; }
      const want = slugHeading(block.heading);
      const matches = [];
      for (let position = index + 1; position < end; position += 1) {
        const node = tree.children[position];
        if (node.type === "heading" && node.depth > 2 && slugHeading(visibleNodeText(node)) === want) matches.push(position);
      }
      if (matches.length !== 1) throw new Error(`${sourcePath}: a figure is bound to the section "${block.heading}" of the ${panel} panel, which names ${matches.length} headings there, not one`);
      inserts.push({ at: matches[0], values: [block.value] });
    }
    if (atPanel.length) inserts.push({ at: index, values: atPanel });
  }
  const merged = new Map();
  for (const insert of inserts) merged.set(insert.at, [...(merged.get(insert.at) ?? []), ...insert.values]);
  for (const [at, values] of [...merged].sort((left, right) => right[0] - left[0])) {
    tree.children.splice(at + 1, 0, ...values.map((value) => ({ type: "html", value })));
  }
  return stringifyMarkdown(tree);
}

// The top-level raw HTML blocks of a generated page, in order. An as-is page
// uses this to prove its source and companion markers each parse as their own
// block, so no open fence or raw HTML block in the source can swallow them.
export function topLevelHtmlBlocks(markdown) {
  return markdownTree(markdown).children.filter((node) => node.type === "html").map((node) => node.value.trim());
}
