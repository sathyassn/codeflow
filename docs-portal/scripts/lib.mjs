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
  commit = "", base, strictTargets, mediaReferences, sourceAnchors = new Map(),
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
    const children = [];
    let cursor = 0;
    for (const match of node.value.matchAll(/\b(?:ADR|EPC|SPC|TSK|CAP)-\d{3,}(?:-\d{3,})?\b/g)) {
      if (!strictId(match[0])) continue;
      if (match.index > cursor) children.push({ type: "text", value: node.value.slice(cursor, match.index) });
      const target = strictTargets.get(match[0]);
      if (!target) children.push({ type: "text", value: match[0] });
      else children.push({ type: "html", value: strictIdPreview(match[0], target, `${sha256(sourcePath).slice(0, 10)}-${previewSequence++}`) });
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

export function validatePortalConfig(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("portal.config.json: expected an object");
  const allowed = new Set(["schema_version", "title", "description", "theme", "repository_url", "repository_root", "release_version", "primitive_tokens", "source_roots", "exclude", "layers", "base"]);
  for (const key of Object.keys(value)) if (!allowed.has(key)) throw new Error(`portal.config.json: unknown key ${key}`);
  if (value.schema_version !== 1) throw new Error("portal.config.json: unsupported schema_version");
  boundedString(value.title, "title", 1, 120);
  boundedString(value.description, "description", 1, 400);
  if (!["signal", "folio"].includes(value.theme)) throw new Error("portal.config.json: theme must be signal or folio");
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
  return value;
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
  instrument: { light: ["#ffffff", "#f5f8f5", "#e8eee9", "#eaf2f9"], dark: ["#121c18", "#182420", "#1e2c26", "#153040"] },
  editorial: { light: ["#fafaf8", "#f1f1ee", "#e4e4df", "#e8eef1"], dark: ["#1f1c1a", "#282422", "#322e2b", "#1e2a30"] },
  ink: { light: ["#fbf6ec", "#f3eadc", "#e8dccb", "#f3e6d6"], dark: ["#241e17", "#2e261e", "#3a3127", "#3a2a1c"] },
});

export function validatePrimitiveTokens(value, theme) {
  if (!value || typeof value !== "object" || Array.isArray(value) || Object.keys(value).sort().join(",") !== "dark,light,schema_version" || value.schema_version !== 1) {
    throw new Error("primitive token import must contain exactly schema_version, light, and dark");
  }
  const skins = theme === "folio" ? ["ink", "instrument", "editorial"] : ["instrument", "editorial", "ink"];
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
  if (host === "bitbucket.org") return `${root}/src/${commit}/${encodedPath}`;
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
