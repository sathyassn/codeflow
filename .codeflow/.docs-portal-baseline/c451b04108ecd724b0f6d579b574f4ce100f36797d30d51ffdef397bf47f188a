import { execFileSync } from "node:child_process";
import { readFile, realpath, stat } from "node:fs/promises";
import { TextDecoder } from "node:util";
import path from "node:path";
import {
  assertNoSymlink, collectPageIds, excerptFor, extractPageRelationships,
  findRepositoryRoot, linkStrictIds, parseMarkdown, publishOwnedCorpus,
  recoverOwnedCorpus,
  referencedIds, renderSafeMarkdown, safeRelative, sha256, strictId, titleFor,
  validatePortalConfig, walkMarkdown, withBase,
} from "./lib.mjs";

const MAX_CONFIG_BYTES = 64 * 1024;
const MAX_EVIDENCE_BYTES = 8 * 1024 * 1024;
const MAX_SOURCE_BYTES = 4 * 1024 * 1024;
const MAX_TOTAL_SOURCE_BYTES = 64 * 1024 * 1024;
const MAX_SOURCES = 10_000;
const MAX_LAST_GOOD_BYTES = 8 * 1024 * 1024;
const portalRoot = process.cwd();
await recoverOwnedCorpus(portalRoot);
await assertNoSymlink(portalRoot, "portal.config.json");
const configBytes = await readBounded(path.join(portalRoot, "portal.config.json"), MAX_CONFIG_BYTES, "portal configuration");
const config = validatePortalConfig(JSON.parse(configBytes));
const base = config.base;
const layers = config.layers;
const repoRelative = config.repository_root;
if (typeof repoRelative !== "string" || !repoRelative || repoRelative.includes("\\") || path.isAbsolute(repoRelative)) throw new Error("repository_root: expected a relative POSIX path");
const configuredRepositoryRoot = path.resolve(portalRoot, repoRelative);
const discoveredRepositoryRoot = await findRepositoryRoot(portalRoot);
const [repositoryRoot, canonicalDiscoveredRoot] = await Promise.all([realpath(configuredRepositoryRoot), realpath(discoveredRepositoryRoot)]);
if (!filesystemPathsEqual(repositoryRoot, canonicalDiscoveredRoot)) throw new Error("repository_root must resolve to the owning CodeFlow repository");
let commit = "unavailable";
try { commit = execFileSync("git", ["-C", repositoryRoot, "rev-parse", "HEAD"], { encoding: "utf8" }).trim(); } catch {}

const evidencePath = path.join(portalRoot, ".portal/generated/evidence.json");
let previous = { pages: [] };
try { previous = JSON.parse(await readBounded(evidencePath, MAX_EVIDENCE_BYTES, "prior evidence")); } catch {}
const previousBySource = new Map();
const ambiguousPriorSources = new Set();
for (const page of Array.isArray(previous.pages) ? previous.pages : []) {
  if (!isSafePriorPage(page) || ambiguousPriorSources.has(page.source_path)) continue;
  if (previousBySource.has(page.source_path)) {
    previousBySource.delete(page.source_path);
    ambiguousPriorSources.add(page.source_path);
  } else previousBySource.set(page.source_path, page);
}
const previousRendered = new Map();
for (const page of previousBySource.values()) {
  try {
    await assertNoSymlink(portalRoot, page.output_markdown);
    const rendered = await readBounded(path.join(portalRoot, page.output_markdown), MAX_LAST_GOOD_BYTES, "prior rendered page");
    if (sha256(rendered) === page.output_markdown_sha256) previousRendered.set(page.source_path, rendered.toString("utf8"));
  } catch {}
}

const excludes = (config.exclude ?? []).map((item) => safeRelative(item, "exclude"));
const sources = [];
for (const configuredRoot of config.source_roots ?? []) {
  const sourceRoot = safeRelative(configuredRoot, "source_root");
  await assertNoSymlink(repositoryRoot, sourceRoot);
  for (const relative of await walkMarkdown(path.join(repositoryRoot, sourceRoot))) {
    const sourcePath = path.posix.join(sourceRoot, relative);
    if (excludes.some((prefix) => sourcePath === prefix || sourcePath.startsWith(`${prefix}/`))) continue;
    sources.push(sourcePath);
    if (sources.length > MAX_SOURCES) throw new Error(`source count exceeds ${MAX_SOURCES}`);
  }
}
sources.sort();

const routeOwners = new Map();
const pages = [];
let totalSourceBytes = 0;
for (const sourcePath of sources) {
  await assertNoSymlink(repositoryRoot, sourcePath);
  const sourceFile = path.join(repositoryRoot, sourcePath);
  const metadata = await stat(sourceFile);
  if (metadata.size > MAX_SOURCE_BYTES) throw new Error(`${sourcePath}: source exceeds ${MAX_SOURCE_BYTES} bytes`);
  totalSourceBytes += metadata.size;
  if (totalSourceBytes > MAX_TOTAL_SOURCE_BYTES) throw new Error(`source corpus exceeds ${MAX_TOTAL_SOURCE_BYTES} bytes`);
  const bytes = await readFile(sourceFile);
  const sourceHash = sha256(bytes);
  let text;
  try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); }
  catch { throw new Error(`${sourcePath}: source is not valid UTF-8`); }
  try {
    const { frontmatter, body } = parseMarkdown(text, sourcePath);
    const layer = chooseLayer(sourcePath, layers);
    const localRoute = localRouteFor(sourcePath);
    const route = `${layer.id}/${localRoute}`;
    claimRoute(route, sourcePath);
    const title = titleFor(frontmatter, body, sourcePath);
    const ids = collectPageIds(frontmatter, text, sourcePath);
    const relationships = extractPageRelationships(frontmatter, text, sourcePath);
    const excerpt = excerptFor(text);
    pages.push({ source_path: sourcePath, source_sha256: sourceHash, route, layer: layer.id, title, frontmatter, body, ids, relationships, backlinks: [], stale: false, searchable: true, excerpt });
  } catch (error) {
    const prior = previousBySource.get(sourcePath);
    const rendered = previousRendered.get(sourcePath);
    if (!prior || !rendered) throw error;
    claimRoute(prior.route, sourcePath);
    pages.push({ ...prior, source_sha256: sourceHash, stale: true, searchable: false, rendered, rendered_is_stale: prior.stale === true, snippets: [], backlinks: prior.backlinks ?? [], stale_reason: String(error.message ?? error) });
  }
}

const ownerById = new Map();
for (const page of pages) {
  for (const id of page.ids) {
    if (ownerById.has(id)) throw new Error(`duplicate identity ${id}: ${ownerById.get(id).source_path} and ${page.source_path}`);
    ownerById.set(id, page);
  }
}
for (const page of pages) {
  for (const relationship of page.relationships) {
    const target = ownerById.get(relationship.target);
    if (!target) throw new Error(`${page.source_path}: relationship target does not exist: ${relationship.target}`);
    if (!target.stale) target.backlinks.push({ type: relationship.type, source_route: page.route, source_id: relationship.source_id, target: relationship.target });
  }
}
for (const page of pages) page.backlinks.sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));

const renderedPages = [];
for (const page of pages) {
  let rendered = page.rendered;
  if (!page.stale) rendered = renderPage(page, ownerById);
  else if (!page.rendered_is_stale) rendered = rendered.replace(/^---\n/, `---\npagefind: false\n`).replace(/\n---\n/, `\n---\n\n<div class="portal-stale" data-pagefind-ignore="all">\n\n> **Stale rendering:** ${escapeText(page.stale_reason)}. This page is excluded from search until its source is valid again.\n\n`).concat("\n</div>\n");
  const outputMarkdown = `src/content/docs/${page.route}.md`;
  const twin = `public/markdown/${page.route}.md`;
  renderedPages.push({ route: page.route, rendered });
  page.output_markdown = outputMarkdown;
  page.output_markdown_sha256 = sha256(rendered);
  page.markdown_twin = twin;
  page.markdown_twin_sha256 = sha256(rendered);
  page.snippets = page.stale || !page.excerpt ? [] : [{ start_line: page.excerpt.start, end_line: page.excerpt.end, sha256: sha256(page.excerpt.text) }];
  delete page.frontmatter; delete page.body; delete page.excerpt; delete page.rendered; delete page.rendered_is_stale; delete page.layer; delete page.title; delete page.stale_reason;
}

const renderedLandings = [];
for (const layer of layers) {
  const layerPages = pages.filter((page) => page.route.startsWith(`${layer.id}/`) && !page.stale);
  const landing = `---\ntitle: ${JSON.stringify(layer.label)}\ndescription: ${JSON.stringify(layer.description)}\n---\n\n# ${layer.label}\n\n${layer.description}\n\n${layerPages.map((page) => `- [${page.route.split("/").pop().replaceAll("-", " ")}](${withBase(base, page.route)})`).join("\n")}\n`;
  renderedLandings.push({ path: `${layer.id}/index.md`, rendered: landing });
}
const renderedIndex = renderIndex(layers, pages);

const llms = [`# ${config.title}`, "", config.description, "", `Repository commit: ${commit}`, "", ...pages.filter((page) => !page.stale).map((page) => `- [${page.route}](./markdown/${page.route}.md) — ${page.source_path}`), ""].join("\n");
const evidence = {
  schema_version: 1,
  generator: { name: "@codeflow/docs-portal", version: "1.0.0" },
  repository: { root: repoRelative, commit },
  config_sha256: sha256(configBytes),
  pages,
  llms: { path: "public/llms.txt", sha256: sha256(llms) },
  artifacts: [],
};
const contentFiles = new Map([...renderedPages.map((page) => [`${page.route}.md`, page.rendered]), ...renderedLandings.map((landing) => [landing.path, landing.rendered]), ["index.md", renderedIndex]]);
const publicFiles = new Map([...renderedPages.map((page) => [`markdown/${page.route}.md`, page.rendered]), ["llms.txt", llms]]);
await publishOwnedCorpus(portalRoot, [
  { live: ".portal/generated", files: new Map([["evidence.json", `${JSON.stringify(evidence, null, 2)}\n`]]) },
  { live: "src/content/docs", files: contentFiles },
  { live: "public", files: publicFiles, preserveUnknown: true },
]);
console.log(`portal: adapted ${pages.length} source page(s) across ${layers.length} layer(s)`);

function chooseLayer(sourcePath, definitions) {
  return definitions.find((layer) => (layer.paths ?? []).includes(sourcePath) || (layer.prefixes ?? []).some((prefix) => sourcePath === prefix || sourcePath.startsWith(`${prefix}/`))) ?? definitions.find((layer) => layer.fallback);
}

function claimRoute(route, sourcePath) {
  const output = `${route}.md`;
  const reserved = new Set(["index.md", "404.md", ...layers.map((layer) => `${layer.id}/index.md`)]);
  if (reserved.has(output)) throw new Error(`reserved generated route: ${sourcePath} -> ${route}`);
  const key = route.toLocaleLowerCase("en-US");
  if (routeOwners.has(key)) throw new Error(`route collision: ${sourcePath} and ${routeOwners.get(key)} -> ${route}`);
  routeOwners.set(key, sourcePath);
}

function localRouteFor(sourcePath) {
  const route = sourcePath.replace(/^docs\//, "").replace(/^project-management\//, "").replace(/\.md$/, "");
  const parts = route.split("/");
  if (parts.at(-1) === "index") parts.pop();
  if (!parts.length || parts.at(-1) === "404") throw new Error(`source claims a reserved generated route: ${sourcePath}`);
  return parts.join("/");
}

function sourceLink(sourcePath) {
  const label = `<code>${escapeHtml(sourcePath)}</code> at <code>${escapeHtml(commit.slice(0, 12))}</code>`;
  if (typeof config.repository_url !== "string" || !/^https:\/\//.test(config.repository_url) || commit === "unavailable") return label;
  const encodedPath = sourcePath.split("/").map(encodeURIComponent).join("/");
  const href = `${config.repository_url.replace(/\/$/, "")}/blob/${commit}/${encodedPath}`;
  return `<a href="${escapeHtml(href)}">${label}</a>`;
}

function renderPage(page, routesById) {
  const status = typeof page.frontmatter.status === "string" ? page.frontmatter.status : null;
  const amendments = [...page.body.matchAll(/^##\s+(Note|Update|Correction)\b[^\n]*/gmi)].map((match) => match[0].replace(/^##\s+/, ""));
  const relationships = page.relationships.map((relation) => `- ${relation.source_id ? `\`${relation.source_id}\` · ` : ""}**${relation.type.replaceAll("_", " ")}** → [${relation.target}](${withBase(base, routesById.get(relation.target).route)})`).join("\n") || "- None declared.";
  const backlinks = page.backlinks.map((backlink) => `- **${backlink.type.replaceAll("_", " ")}** ← [${backlink.source_id ?? backlink.source_route}](${withBase(base, backlink.source_route)})`).join("\n") || "- None.";
  const referenced = referencedIds(page.body).filter((id) => routesById.has(id) && !page.ids.includes(id));
  const safeBody = linkStrictIds(renderSafeMarkdown(page.body), new Map([...routesById].map(([id, owner]) => [id, withBase(base, owner.route)])));
  const excerptBody = page.excerpt ? renderSafeMarkdown(page.excerpt.text).split("\n").map((line) => `> ${line}`).join("\n") : "";
  const excerpt = page.excerpt ? `\n<!-- codeflow-source-snippet sha256=${sha256(page.excerpt.text)} lines=${page.excerpt.start}-${page.excerpt.end} -->\n> **Source excerpt, lines ${page.excerpt.start}–${page.excerpt.end}:**\n>\n${excerptBody}\n` : "";
  const referencedLinks = referenced.length ? referenced.map((id) => `- [${id}](${withBase(base, routesById.get(id).route)})`).join("\n") : "- None.";
  return `---\ntitle: ${JSON.stringify(page.title)}\ndescription: ${JSON.stringify(typeof page.frontmatter.description === "string" ? page.frontmatter.description : `Repository source: ${page.source_path}`)}\n---\n\n<div class="portal-provenance">Source ${sourceLink(page.source_path)} · portal 1.0.0</div>\n${excerpt}\n${safeBody}\n\n---\n\n## Record context\n\n${status ? `- **Status:** ${status}\n` : ""}- **Identity:** ${page.ids.length ? page.ids.map((id) => `\`${id}\``).join(", ") : "No stable record ID"}\n- **Currentness:** ${amendments.length ? `Original record plus ${amendments.map((item) => `**${item}**`).join(", ")}` : "No amendment heading declared"}\n\n### Declared relationships\n\n${relationships}\n\n### Referenced records\n\n${referencedLinks}\n\n### Inverse links\n\n${backlinks}\n`;
}

function renderIndex(definitions, allPages) {
  const steps = definitions.map((layer, index) => `<li><a href="${withBase(base, layer.id)}"><span>${String(index + 1).padStart(2, "0")}</span><strong>${escapeHtml(layer.label)}</strong><small>${escapeHtml(layer.description)}</small><em>${allPages.filter((page) => page.route.startsWith(`${layer.id}/`) && !page.stale).length} sources</em></a></li>`).join("\n");
  return `---\ntitle: ${JSON.stringify(config.title)}\ndescription: ${JSON.stringify(config.description)}\ntemplate: splash\nhero:\n  tagline: ${JSON.stringify(config.description)}\n---\n\n<ul class="portal-journey">\n${steps}\n</ul>\n\n<p class="portal-version">Repository <code>${commit.slice(0, 12)}</code> · portal <code>1.0.0</code></p>\n`;
}

function isSafePriorPage(page) {
  if (!page || typeof page !== "object" || typeof page.source_path !== "string" || typeof page.route !== "string" || typeof page.output_markdown !== "string" || typeof page.output_markdown_sha256 !== "string") return false;
  try {
    safeRelative(page.source_path, "prior source");
    safeRelative(page.route, "prior route");
    safeRelative(page.output_markdown, "prior output");
    const relationships = Array.isArray(page.relationships) && page.relationships.every((item) => item && typeof item === "object" && ["epic", "spec", "depends_on", "capability", "decision", "related", "superseded_by"].includes(item.type) && strictId(item.target) && (item.source_id === null || item.source_id === undefined || strictId(item.source_id)));
    const backlinks = Array.isArray(page.backlinks) && page.backlinks.every((item) => item && typeof item === "object" && typeof item.source_route === "string" && safeRelative(item.source_route, "prior backlink") && strictId(item.target) && (item.source_id === null || item.source_id === undefined || strictId(item.source_id)));
    return page.output_markdown === `src/content/docs/${page.route}.md` && /^[a-f0-9]{64}$/.test(page.output_markdown_sha256) && Array.isArray(page.ids) && page.ids.every(strictId) && relationships && backlinks;
  } catch { return false; }
}

async function readBounded(file, limit, label) {
  const metadata = await stat(file);
  if (!metadata.isFile() || metadata.size > limit) throw new Error(`${label} exceeds ${limit} bytes or is not a file`);
  return readFile(file);
}

function escapeHtml(value) {
  return String(value).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

function escapeText(value) {
  return String(value).replace(/[\r\n]+/g, " ").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
}

function filesystemPathsEqual(left, right) {
  return process.platform === "win32" ? left.toLocaleLowerCase("en-US") === right.toLocaleLowerCase("en-US") : left === right;
}
