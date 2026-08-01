import { execFileSync } from "node:child_process";
import { realpath } from "node:fs/promises";
import { TextDecoder } from "node:util";
import { fileURLToPath } from "node:url";
import path from "node:path";
import {
  amendmentHeadings, collectPageIds, compareDeterministicText, excerptFor, extractPageRelationships,
  findRepositoryRoot, parseMarkdown,
  referencedIds, renderPrimitiveTokenCss, rewriteRepositoryMarkdown, safeRelative, sha256, titleFor,
  validatePortalConfig, validatePrimitiveTokens, withBase,
} from "./lib.mjs";
import { publishOwnedCorpus, readBoundedRegularFile, recoverOwnedCorpus } from "./publication.mjs";

const MAX_CONFIG_BYTES = 64 * 1024;
const MAX_SOURCE_BYTES = 4 * 1024 * 1024;
const MAX_TOTAL_SOURCE_BYTES = 64 * 1024 * 1024;
const MAX_SOURCES = 10_000;
const MAX_GIT_LIST_BYTES = 8 * 1024 * 1024;
const MAX_RUNTIME_FILE_BYTES = 4 * 1024 * 1024;
const MAX_TOTAL_RUNTIME_BYTES = 64 * 1024 * 1024;
const MAX_STALE_HISTORY = 128;
const MAX_PRIMITIVE_TOKEN_BYTES = 16 * 1024;
const MAX_MEDIA_BYTES = 8 * 1024 * 1024;
const MAX_TOTAL_MEDIA_BYTES = 64 * 1024 * 1024;
const MAX_MEDIA_FILES = 1_000;
const portalRoot = process.cwd();
await recoverOwnedCorpus(portalRoot);
const discoveredRepositoryRoot = await findRepositoryRoot(portalRoot);
const repositoryRoot = await realpath(discoveredRepositoryRoot);
const commit = gitText(["rev-parse", "--verify", "HEAD^{commit}"], 1024).trim();
if (!/^[a-f0-9]{40}$/.test(commit)) throw new Error("repository HEAD must resolve to a full Git commit");

const portalConfigRelative = safeRelative(path.relative(repositoryRoot, path.join(portalRoot, "portal.config.json")).split(path.sep).join("/"), "portal configuration path");
assertCommittedFileAt(commit, portalConfigRelative, ["100644"], "portal configuration");
const configBytes = readCommittedBlob(commit, portalConfigRelative, MAX_CONFIG_BYTES, "portal configuration");
const config = validatePortalConfig(JSON.parse(configBytes));
const base = config.base;
const layers = config.layers;
const repoRelative = config.repository_root;
if (typeof repoRelative !== "string" || !repoRelative || repoRelative.includes("\\") || path.isAbsolute(repoRelative)) throw new Error("repository_root: expected a relative POSIX path");
const configuredRepositoryRoot = await realpath(path.resolve(portalRoot, repoRelative));
if (!filesystemPathsEqual(repositoryRoot, configuredRepositoryRoot)) throw new Error("repository_root must resolve to the owning CodeFlow repository");
const adapterRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const runtimeInputs = filesystemPathsEqual(path.resolve(adapterRoot), path.resolve(portalRoot))
  ? ["astro.config.mjs", "package.json", "package-lock.json", "scripts", "src/content.config.ts", "src/styles", "public/portal-preview.js"].map((item) => safeRelative(path.relative(repositoryRoot, path.join(portalRoot, item)).split(path.sep).join("/"), "portal runtime input"))
  : [];
const snapshotPaths = [portalConfigRelative, ...runtimeInputs, ...config.source_roots, ...(config.primitive_tokens === null ? [] : [config.primitive_tokens])].map((item) => safeRelative(item, "snapshot path"));
assertCleanSnapshot(snapshotPaths);
await assertRuntimeMatchesCommit([portalConfigRelative, ...runtimeInputs]);
let primitiveTokens = null;
let primitiveTokenEvidence = null;
if (config.primitive_tokens !== null) {
  assertCommittedPrimitiveToken(config.primitive_tokens);
  const bytes = readCommittedBlob(commit, config.primitive_tokens, MAX_PRIMITIVE_TOKEN_BYTES, "primitive token import");
  primitiveTokens = validatePrimitiveTokens(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)), config.theme);
  primitiveTokenEvidence = { source_path: config.primitive_tokens, source_sha256: sha256(bytes) };
}
const primitiveTokenCss = renderPrimitiveTokenCss(primitiveTokens);
if (primitiveTokenEvidence !== null) Object.assign(primitiveTokenEvidence, { output_path: ".portal/generated/project-tokens.css", output_sha256: sha256(primitiveTokenCss) });

const excludes = (config.exclude ?? []).map((item) => safeRelative(item, "exclude"));
const sources = committedMarkdownSources(config.source_roots, excludes);
const routeOwners = new Map();
const sourceRoutes = new Map();
for (const sourcePath of sources) {
  const layer = chooseLayer(sourcePath, layers);
  const route = `${layer.id}/${localRouteFor(sourcePath)}`;
  claimRoute(route, sourcePath);
  sourceRoutes.set(sourcePath, route);
}

const pages = [];
let totalSourceBytes = 0;
for (const sourcePath of sources) {
  const bytes = readCommittedBlob(commit, sourcePath, MAX_SOURCE_BYTES, sourcePath);
  totalSourceBytes += bytes.length;
  if (totalSourceBytes > MAX_TOTAL_SOURCE_BYTES) throw new Error(`source corpus exceeds ${MAX_TOTAL_SOURCE_BYTES} bytes`);
  const sourceHash = sha256(bytes);
  let text;
  try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); }
  catch { throw new Error(`${sourcePath}: source is not valid UTF-8`); }
  try {
    const { frontmatter, body } = parseMarkdown(text, sourcePath);
    const layer = chooseLayer(sourcePath, layers);
    const route = sourceRoutes.get(sourcePath);
    const title = titleFor(frontmatter, body, sourcePath);
    const ids = collectPageIds(frontmatter, text, sourcePath);
    const relationships = extractPageRelationships(frontmatter, text, sourcePath);
    const excerpt = excerptFor(text);
    pages.push({ source_path: sourcePath, source_sha256: sourceHash, built_from_commit: commit, route, layer: layer.id, title, frontmatter, body, ids, relationships, backlinks: [], stale: false, searchable: true, excerpt });
  } catch (error) {
    pages.push(findLastGoodPage(sourcePath, sourceHash, error));
  }
}
assertCleanSnapshot(snapshotPaths);
await assertRuntimeMatchesCommit([portalConfigRelative, ...runtimeInputs]);
if (gitText(["rev-parse", "--verify", "HEAD^{commit}"], 1024).trim() !== commit) throw new Error("repository HEAD changed while the portal snapshot was being read");

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
    if (!page.stale && !target.stale) target.backlinks.push({ type: relationship.type, source_route: page.route, source_id: relationship.source_id, target: relationship.target });
  }
}
for (const page of pages) page.backlinks.sort((a, b) => compareDeterministicText(JSON.stringify(a), JSON.stringify(b)));

const renderedPages = [];
const mediaReferences = new Map();
for (const page of pages) {
  const referencedMedia = page.stale ? new Map() : mediaReferences;
  let rendered = renderPage(page, ownerById, referencedMedia);
  if (page.stale) {
    if (referencedMedia.size) throw new Error(`${page.source_path}: stale fallback cannot import ancestor media`);
    rendered = renderStalePage(page, rendered);
  }
  const outputMarkdown = `src/content/docs/${page.route}.md`;
  const twin = `public/markdown/${page.route}.md`;
  renderedPages.push({ route: page.route, rendered });
  page.output_markdown = outputMarkdown;
  page.output_markdown_sha256 = sha256(rendered);
  page.markdown_twin = twin;
  page.markdown_twin_sha256 = sha256(rendered);
  page.snippets = page.stale || !page.excerpt ? [] : [{ start_line: page.excerpt.start, end_line: page.excerpt.end, sha256: sha256(page.excerpt.text) }];
  page.status = typeof page.frontmatter?.status === "string" ? page.frontmatter.status : null;
  delete page.frontmatter; delete page.body; delete page.excerpt; delete page.layer; delete page.stale_reason;
}

const renderedLandings = [];
for (const layer of layers) {
  const layerPages = pages.filter((page) => page.route.startsWith(`${layer.id}/`) && !page.stale);
  const landing = renderLayerLanding(layer, layerPages);
  renderedLandings.push({ path: `${layer.id}/index.md`, rendered: landing });
}
const renderedIndex = renderIndex(layers, pages);

if (mediaReferences.size > MAX_MEDIA_FILES) throw new Error(`referenced media count exceeds ${MAX_MEDIA_FILES}`);
assertCleanSnapshot([...snapshotPaths, ...mediaReferences.keys()]);
const mediaFiles = new Map();
const mediaEvidence = [];
let totalMediaBytes = 0;
for (const [sourcePath, outputPath] of [...mediaReferences].sort(([left], [right]) => compareDeterministicText(left, right))) {
  assertCommittedFile(sourcePath, ["100644"], "referenced media");
  const bytes = readCommittedBlob(commit, sourcePath, MAX_MEDIA_BYTES, `referenced media ${sourcePath}`);
  assertRasterSignature(sourcePath, bytes);
  totalMediaBytes += bytes.length;
  if (totalMediaBytes > MAX_TOTAL_MEDIA_BYTES) throw new Error(`referenced media corpus exceeds ${MAX_TOTAL_MEDIA_BYTES} bytes`);
  mediaFiles.set(outputPath, bytes);
  mediaEvidence.push({ source_path: sourcePath, source_sha256: sha256(bytes), output_path: `public/${outputPath}`, output_sha256: sha256(bytes) });
}
assertCleanSnapshot([...snapshotPaths, ...mediaReferences.keys()]);
await assertRuntimeMatchesCommit([portalConfigRelative, ...runtimeInputs]);
if (gitText(["rev-parse", "--verify", "HEAD^{commit}"], 1024).trim() !== commit) throw new Error("repository HEAD changed while referenced media was being read");

const llms = [`# ${escapeMarkdownInline(config.title)}`, "", escapeMarkdownInline(config.description), "", `Repository commit: ${commit}`, ...(config.release_version === null ? [] : [`Release version: ${config.release_version}`]), "", ...pages.filter((page) => !page.stale).map((page) => `- [${escapeMarkdownInline(page.route)}](./markdown/${page.route}.md) — ${escapeMarkdownInline(page.source_path)}`), ""].join("\n");
const evidence = {
  schema_version: 1,
  generator: { name: "@codeflow/docs-portal", version: "1.0.0" },
  repository: { root: repoRelative, commit, release_version: config.release_version },
  config_sha256: sha256(configBytes),
  primitive_tokens: primitiveTokenEvidence,
  media: mediaEvidence,
  pages,
  llms: { path: "public/llms.txt", sha256: sha256(llms) },
  artifacts: [],
};
const contentFiles = new Map([...renderedPages.map((page) => [`${page.route}.md`, page.rendered]), ...renderedLandings.map((landing) => [landing.path, landing.rendered]), ["index.md", renderedIndex]]);
const publicFiles = new Map([...renderedPages.map((page) => [`markdown/${page.route}.md`, page.rendered]), ...mediaFiles, ["llms.txt", llms]]);
await publishOwnedCorpus(portalRoot, [
  { live: ".portal/generated", files: new Map([["evidence.json", `${JSON.stringify(evidence, null, 2)}\n`], ["project-tokens.css", primitiveTokenCss]]) },
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
  const key = route.normalize("NFC").toLowerCase();
  if (routeOwners.has(key)) throw new Error(`route collision: ${sourcePath} and ${routeOwners.get(key)} -> ${route}`);
  routeOwners.set(key, sourcePath);
}

function localRouteFor(sourcePath) {
  const route = sourcePath.replace(/^docs\//, "").replace(/^project-management\//, "").replace(/\.md$/, "");
  const parts = route.split("/");
  if (parts.at(-1) === "index") parts.pop();
  if (!parts.length || parts.at(-1) === "404") throw new Error(`source claims a reserved generated route: ${sourcePath}`);
  return parts.map(strictUrlSegment).join("/");
}

function sourceLink(sourcePath) {
  const label = `<code>${escapeHtml(sourcePath)}</code> at <code>${escapeHtml(commit.slice(0, 12))}</code>`;
  const href = pinnedSourceUrl(sourcePath);
  if (href === null) return label;
  return `<a href="${escapeHtml(href)}">${label}</a>`;
}

function pinnedSourceUrl(sourcePath) {
  if (typeof config.repository_url !== "string") return null;
  const repository = new URL(config.repository_url);
  const root = config.repository_url.replace(/\/$/, "").replace(/\.git$/, "");
  const encodedPath = sourcePath.split("/").map(strictUrlSegment).join("/");
  if (repository.hostname.toLowerCase() === "github.com") return `${root}/blob/${commit}/${encodedPath}`;
  if (repository.hostname.toLowerCase() === "gitlab.com") return `${root}/-/blob/${commit}/${encodedPath}`;
  if (repository.hostname.toLowerCase() === "bitbucket.org") return `${root}/src/${commit}/${encodedPath}`;
  return null;
}

function renderPage(page, routesById, referencedMedia) {
  const status = typeof page.frontmatter.status === "string" ? page.frontmatter.status : null;
  const amendments = page.source_path.startsWith("docs/decisions/") ? amendmentHeadings(page.body) : [];
  const relationships = page.relationships.map((relation) => {
    const target = routesById.get(relation.target);
    const label = `${relation.target}${target.stale ? " — stale" : ""}`;
    return `- ${relation.source_id ? `\`${relation.source_id}\` · ` : ""}**${relation.type.replaceAll("_", " ")}** → [${label}](${withBase(base, target.route)})`;
  }).join("\n");
  const backlinks = page.backlinks.map((backlink) => `- **${backlink.type.replaceAll("_", " ")}** ← [${backlink.source_id ?? backlink.source_route}](${withBase(base, backlink.source_route)})`).join("\n");
  const referenced = referencedIds(page.body).filter((id) => routesById.has(id) && !page.ids.includes(id));
  const previews = new Map([...routesById].map(([id, owner]) => [id, {
    route: withBase(base, owner.route), title: owner.title,
    status: typeof owner.frontmatter?.status === "string" ? owner.frontmatter.status : owner.status,
    source_path: owner.source_path, stale: owner.stale,
  }]));
  const safeBody = rewriteRepositoryMarkdown(page.body, { sourcePath: page.source_path, sourceRoutes, base, strictTargets: previews, mediaReferences: referencedMedia });
  const snippetMarker = page.excerpt ? `\n<!-- codeflow-source-snippet sha256=${sha256(page.excerpt.text)} lines=${page.excerpt.start}-${page.excerpt.end} -->` : "";
  const context = [];
  const facts = [];
  if (status) facts.push(`- **Status:** ${escapeMarkdownInline(status)}`);
  if (typeof page.frontmatter.date === "string") facts.push(`- **Decision date:** ${escapeMarkdownInline(page.frontmatter.date)}`);
  if (page.ids.length) facts.push(`- **Identity:** ${page.ids.map((id) => `\`${id}\``).join(", ")}`);
  if (amendments.length) facts.push(`- **Amendments:** ${amendments.map((item) => `**${escapeMarkdownInline(item)}**`).join(", ")}`);
  if (facts.length) context.push(`## Record context\n\n${facts.join("\n")}`);
  if (relationships) context.push(`### Declared relationships\n\n${relationships}`);
  if (referenced.length) context.push(`### Referenced records\n\n${referenced.map((id) => {
    const owner = routesById.get(id);
    return `- [${id}${owner.stale ? " — stale" : ""}](${withBase(base, owner.route)})`;
  }).join("\n")}`);
  if (backlinks) context.push(`### Inverse links\n\n${backlinks}`);
  const recordContext = context.length ? `\n\n---\n\n${context.join("\n\n")}` : "";
  const release = config.release_version === null ? "" : ` · release <code>${escapeHtml(config.release_version)}</code>`;
  return `---\ntitle: ${JSON.stringify(page.title)}\ndescription: ${JSON.stringify(typeof page.frontmatter.description === "string" ? page.frontmatter.description : `Repository source: ${page.source_path}`)}\n---\n\n${provenanceMarker(page)}\n<div class="portal-provenance">Source ${sourceLink(page.source_path)} · built from <code>${commit}</code>${release} · portal <code>1.0.0</code></div>${snippetMarker}\n\n<div data-pagefind-body data-codeflow-search-root="${escapeHtml(page.route)}">\n\n${safeBody}${recordContext}\n\n</div>\n`;
}

function renderStalePage(page, priorRendered) {
  let body = priorRendered
    .replace(/\n<!-- codeflow-page-provenance[^\n]* -->\n/g, "\n")
    .replace(/\n<div class="portal-provenance">[^\n]*<\/div>\n/g, "\n")
    .replace(/\n<div class="portal-stale" data-pagefind-ignore="all">\n\n> \*\*Stale rendering:\*\*[^\n]*\n\n([\s\S]*)\n<\/div>\n$/, "\n$1\n")
    .replace(/<div data-pagefind-body data-codeflow-search-root="[^"]+">/, "<div>")
    .replace(/^---\n(?:pagefind: false\n)?/, "---\npagefind: false\n");
  const lastGoodMarker = `<!-- codeflow-last-good-provenance source_sha256=${page.last_good_source_sha256} built_from_commit=${page.last_good_commit} -->`;
  const opening = `\n---\n\n${provenanceMarker(page)}\n${lastGoodMarker}\n<div class="portal-provenance">Current source ${sourceLink(page.source_path)} · snapshot <code>${commit}</code> · last good source <code>${escapeHtml(page.last_good_commit)}</code></div>\n\n<div class="portal-stale" data-pagefind-ignore="all">\n\n> **Stale rendering:** ${escapeMarkdownInline(page.stale_reason)}. This page is excluded from search until its source is valid again.\n\n`;
  body = body.replace(/\n---\n/, opening);
  return `${body.trimEnd()}\n\n</div>\n`;
}

function provenanceMarker(page) {
  return `<!-- codeflow-page-provenance source_sha256=${page.source_sha256} built_from_commit=${commit} portal_version=1.0.0 release_version=${config.release_version ?? "none"} -->`;
}

function renderLayerLanding(layer, layerPages) {
  const preface = `---\ntitle: ${JSON.stringify(layer.label)}\ndescription: ${JSON.stringify(layer.description)}\n---\n\n# ${escapeMarkdownInline(layer.label)}\n\n${escapeMarkdownInline(layer.description)}\n`;
  if (layer.id === "records") {
    const groups = [["Epics", "EPC-"], ["Specifications", "SPC-"], ["Tasks", "TSK-"]];
    const sections = groups.map(([label, prefix]) => {
      const records = layerPages.filter((page) => page.ids.some((id) => id.startsWith(prefix)));
      if (!records.length) return "";
      return `## ${label}\n\n| Record | Status | Dependencies |\n|---|---|---|\n${records.map((page) => `| [${escapeMarkdownCell(page.title)}](${withBase(base, page.route)}) | ${escapeMarkdownCell(page.status ?? "Not declared")} | ${page.relationships.filter((item) => item.type === "depends_on").map((item) => `\`${item.target}\``).join(", ") || "—"} |`).join("\n")}`;
    }).filter(Boolean);
    return `${preface}\n${sections.join("\n\n")}\n`;
  }
  if (layer.id === "system") {
    const decisions = layerPages.filter((page) => page.ids.some((id) => id.startsWith("ADR-")));
    const foundations = layerPages.filter((page) => !page.ids.some((id) => id.startsWith("ADR-")));
    const lineage = decisions.flatMap((page) => page.relationships.filter((item) => item.type === "superseded_by").map((item) => `- [${escapeMarkdownInline(page.title)}](${withBase(base, page.route)}) → [${item.target}](${withBase(base, ownerById.get(item.target).route)})`));
    return `${preface}\n${foundations.length ? `## Foundations\n\n${pageList(foundations)}\n\n` : ""}## Decisions\n\n${pageList(decisions)}${lineage.length ? `\n\n## Decision lineage\n\n${lineage.join("\n")}` : ""}\n`;
  }
  return `${preface}\n${pageList(layerPages)}\n`;
}

function pageList(items) {
  return items.map((page) => `- [${escapeMarkdownInline(page.title)}](${withBase(base, page.route)})`).join("\n") || "No current sources in this layer.";
}

function escapeMarkdownCell(value) {
  return escapeMarkdownInline(value).replaceAll("|", "\\|");
}

function escapeMarkdownInline(value) {
  return escapeHtml(String(value).replace(/[\r\n\t]+/g, " "))
    .replace(/([\\`*_[\]{}()#+.!])/g, "\\$1")
    .replaceAll(":", "&#58;");
}

function strictUrlSegment(value) {
  return encodeURIComponent(value).replace(/[!'()*]/g, (character) => `%${character.charCodeAt(0).toString(16).toUpperCase()}`);
}

function renderIndex(definitions, allPages) {
  const steps = definitions.map((layer, index) => `<li><a href="${withBase(base, layer.id)}"><span>${String(index + 1).padStart(2, "0")}</span><strong>${escapeHtml(layer.label)}</strong><small>${escapeHtml(layer.description)}</small><em>${allPages.filter((page) => page.route.startsWith(`${layer.id}/`) && !page.stale).length} sources</em></a></li>`).join("\n");
  return `---\ntitle: ${JSON.stringify(config.title)}\ndescription: ${JSON.stringify(config.description)}\ntemplate: splash\nhero:\n  tagline: ${JSON.stringify(config.description)}\n---\n\n<ul class="portal-journey">\n${steps}\n</ul>\n\n<p class="portal-version">Repository <code>${commit}</code>${config.release_version === null ? "" : ` · release <code>${escapeHtml(config.release_version)}</code>`} · portal <code>1.0.0</code></p>\n`;
}

function findLastGoodPage(sourcePath, currentSourceHash, currentError) {
  let history;
  try {
    history = gitText(["rev-list", `--max-count=${MAX_STALE_HISTORY + 1}`, commit, "--", gitLiteralPathspec(sourcePath)], (MAX_STALE_HISTORY + 1) * 41)
      .trim().split("\n").filter((candidate) => candidate && candidate !== commit);
  } catch { throw currentError; }
  for (const ancestor of history.slice(0, MAX_STALE_HISTORY)) {
    if (!/^[a-f0-9]{40}$/.test(ancestor)) continue;
    try {
      const bytes = readCommittedBlob(ancestor, sourcePath, MAX_SOURCE_BYTES, `last-good source ${sourcePath}`);
      const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
      const { frontmatter, body } = parseMarkdown(text, sourcePath);
      const layer = chooseLayer(sourcePath, layers);
      return {
        source_path: sourcePath,
        source_sha256: currentSourceHash,
        built_from_commit: commit,
        route: sourceRoutes.get(sourcePath),
        layer: layer.id,
        title: titleFor(frontmatter, body, sourcePath),
        frontmatter,
        body,
        ids: collectPageIds(frontmatter, text, sourcePath),
        relationships: extractPageRelationships(frontmatter, text, sourcePath),
        backlinks: [],
        stale: true,
        searchable: false,
        excerpt: null,
        stale_reason: String(currentError.message ?? currentError),
        last_good_commit: ancestor,
        last_good_source_sha256: sha256(bytes),
      };
    } catch {}
  }
  throw currentError;
}

function escapeHtml(value) {
  return String(value).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

function filesystemPathsEqual(left, right) {
  return process.platform === "win32" ? left.toLowerCase() === right.toLowerCase() : left === right;
}

function committedMarkdownSources(configuredRoots, excluded) {
  const roots = configuredRoots.map((item) => safeRelative(item, "source_root"));
  const listing = gitText(["ls-tree", "-r", "-z", commit, "--", ...roots.map((root) => gitLiteralPathspec(root, true))], MAX_GIT_LIST_BYTES);
  const sources = [];
  const keys = new Set();
  for (const record of listing.split("\0")) {
    if (!record) continue;
    const separator = record.indexOf("\t");
    if (separator < 0) throw new Error("git ls-tree returned an invalid source record");
    const [mode, type] = record.slice(0, separator).split(" ");
    const sourcePath = safeRelative(record.slice(separator + 1), "committed source");
    if (type !== "blob" || !["100644", "100755"].includes(mode)) throw new Error(`committed source is not a regular file: ${sourcePath}`);
    if (!sourcePath.endsWith(".md") || /^\.env(?:\.|$)/.test(path.posix.basename(sourcePath)) || excluded.some((prefix) => sourcePath === prefix || sourcePath.startsWith(`${prefix}/`))) continue;
    const key = sourcePath.normalize("NFC").toLowerCase();
    if (keys.has(key)) throw new Error(`committed source path collides case-insensitively: ${sourcePath}`);
    keys.add(key);
    sources.push(sourcePath);
    if (sources.length > MAX_SOURCES) throw new Error(`source count exceeds ${MAX_SOURCES}`);
  }
  return sources.sort(compareDeterministicText);
}

async function assertRuntimeMatchesCommit(inputs) {
  const records = committedRegularFiles(commit, inputs, "portal runtime input");
  let totalBytes = 0;
  for (const record of records) {
    const committed = readCommittedBlob(commit, record.path, MAX_RUNTIME_FILE_BYTES, "portal runtime input");
    const worktree = await readBoundedRegularFile(path.join(repositoryRoot, record.path), MAX_RUNTIME_FILE_BYTES, `portal runtime input ${record.path}`);
    totalBytes += committed.length;
    if (totalBytes > MAX_TOTAL_RUNTIME_BYTES) throw new Error(`portal runtime inputs exceed ${MAX_TOTAL_RUNTIME_BYTES} bytes`);
    if (!committed.equals(worktree)) throw new Error(`portal runtime input does not match ${commit}: ${record.path}`);
  }
}

function committedRegularFiles(commitish, inputs, label) {
  const listing = gitText(["ls-tree", "-r", "-z", commitish, "--", ...inputs.map((input) => gitObjectPathspec(commitish, input))], MAX_GIT_LIST_BYTES);
  const records = [];
  const keys = new Set();
  for (const raw of listing.split("\0")) {
    if (!raw) continue;
    const separator = raw.indexOf("\t");
    if (separator < 0) throw new Error(`${label} has an invalid Git tree record`);
    const [mode, type] = raw.slice(0, separator).split(" ");
    const file = safeRelative(raw.slice(separator + 1), label);
    if (type !== "blob" || !["100644", "100755"].includes(mode)) throw new Error(`${label} is not a regular Git file: ${file}`);
    const key = file.normalize("NFC").toLowerCase();
    if (keys.has(key)) throw new Error(`${label} has a portable path collision: ${file}`);
    keys.add(key);
    records.push({ path: file, mode });
  }
  if (!records.length) throw new Error(`${label} set is empty at ${commitish}`);
  return records.sort((left, right) => compareDeterministicText(left.path, right.path));
}

function assertCommittedPrimitiveToken(sourcePath) {
  assertCommittedFile(sourcePath, ["100644"], "primitive token import");
}

function assertCommittedFile(sourcePath, allowedModes, label) {
  assertCommittedFileAt(commit, sourcePath, allowedModes, label);
}

function assertCommittedFileAt(commitish, sourcePath, allowedModes, label) {
  const listing = gitText(["ls-tree", "-z", commitish, "--", gitLiteralPathspec(sourcePath)], 4096);
  const records = listing.split("\0").filter(Boolean);
  if (records.length !== 1) throw new Error(`${label} must be one committed regular file: ${sourcePath}`);
  const separator = records[0].indexOf("\t");
  if (separator < 0) throw new Error(`${label} has an invalid Git tree record: ${sourcePath}`);
  const [mode, type] = records[0].slice(0, separator).split(" ");
  const actual = safeRelative(records[0].slice(separator + 1), label);
  if (actual !== sourcePath || type !== "blob" || !allowedModes.includes(mode)) {
    throw new Error(`${label} must be a committed regular file with an allowed mode: ${sourcePath}`);
  }
}

function readCommittedBlob(commitish, sourcePath, maximumBytes, label) {
  assertCommittedFileAt(commitish, sourcePath, ["100644", "100755"], label);
  const bytes = gitBytes(["cat-file", "blob", `${commitish}:${sourcePath}`], maximumBytes, label);
  if (bytes.length > maximumBytes) throw new Error(`${label} exceeds ${maximumBytes} bytes`);
  return bytes;
}

function assertRasterSignature(sourcePath, bytes) {
  const extension = path.posix.extname(sourcePath).toLowerCase();
  const ascii = (start, end) => bytes.subarray(start, end).toString("ascii");
  const valid = extension === ".png" ? bytes.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))
    : [".jpg", ".jpeg"].includes(extension) ? bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff
      : extension === ".gif" ? ["GIF87a", "GIF89a"].includes(ascii(0, 6))
        : extension === ".webp" ? ascii(0, 4) === "RIFF" && ascii(8, 12) === "WEBP"
          : extension === ".avif" ? ascii(4, 8) === "ftyp" && ["avif", "avis"].includes(ascii(8, 12))
            : false;
  if (!valid) throw new Error(`referenced media bytes do not match the approved raster type: ${sourcePath}`);
}

function assertCleanSnapshot(paths) {
  const status = gitText(["status", "--porcelain=v1", "-z", "--untracked-files=all", "--ignored=matching", "--", ...paths.map((input) => gitObjectPathspec(commit, input))], MAX_GIT_LIST_BYTES);
  if (status.length) throw new Error("configured portal sources must match HEAD exactly; commit or remove staged, modified, deleted, untracked, and ignored source-root changes");
}

function gitLiteralPathspec(value, tree = false) {
  const safe = safeRelative(value, "Git snapshot path");
  return `:(top,literal)${safe}${tree ? "/" : ""}`;
}

function gitObjectPathspec(commitish, value) {
  const safe = safeRelative(value, "Git snapshot path");
  let type;
  try { type = gitText(["cat-file", "-t", `${commitish}:${safe}`], 64).trim(); }
  catch { return gitLiteralPathspec(safe, true); }
  if (!['blob', 'tree'].includes(type)) throw new Error(`Git snapshot path has an unsupported object type: ${safe}`);
  return gitLiteralPathspec(safe, type === "tree");
}

function gitText(args, maximumBytes) {
  return new TextDecoder("utf-8", { fatal: true }).decode(gitBytes(args, maximumBytes, `Git ${args[0]}`));
}

function gitBytes(args, maximumBytes, label) {
  try {
    const output = execFileSync("git", ["-C", repositoryRoot, ...args], { encoding: "buffer", maxBuffer: maximumBytes + 1 });
    if (output.length > maximumBytes) throw new Error(`${label} exceeds ${maximumBytes} bytes`);
    return output;
  } catch (error) {
    throw new Error(`Git snapshot command failed (${args[0]}): ${String(error.stderr ?? error.message ?? error).trim()}`);
  }
}
