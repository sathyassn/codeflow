import { realpath } from "node:fs/promises";
import { TextDecoder } from "node:util";
import { fileURLToPath } from "node:url";
import path from "node:path";
import {
  amendmentHeadings, capabilityFenceRecords, checkoutEquivalentBytes, collectPageIds, committedDirectoryPaths, compareDeterministicText, decorateAltitude, excerptFor, extractPageRelationships,
  findRepositoryRoot, headingAnchors, localRouteFor, parseMarkdown, placeCapabilityTable,
  pinnedSourceUrl as providerSourceUrl, recordFilesFor, referencedIds, renderCapabilityFences, renderPrimitiveTokenCss, rewriteRepositoryMarkdown, safeRelative, sha256, titleFor,
  recoverUnavailableIds, renderStageFences, strictUrlSegment, stripLeadingTitleHeading, validatePageMetadata, validatePortalConfig, validatePrimitiveTokens, withBase,
} from "./lib.mjs";
import { GitSnapshot } from "./git-snapshot.mjs";
import { GENERATOR } from "./generator.mjs";
import { assertEvidenceEnvelope, assertEvidencePageLimits, EVIDENCE_LIMITS } from "./limits.mjs";
import { isReservedPublicPath, publishOwnedCorpus, readBoundedRegularFile, recoverOwnedCorpus } from "./publication.mjs";

const MAX_CONFIG_BYTES = 64 * 1024;
// The canonical capability registry is the one source whose YAML fences are
// identity declarations, so it is also the one page whose fences render as
// generated structure (lib.mjs collectPageIds shares this contract).
const CAPABILITY_REGISTRY = "docs/capabilities.md";
const RECORD_POINTER_TITLE = "Where decisions and work records live";
// Keep arbitrary bounded fork versions inert and each provenance marker on one line.
const renderedGeneratorVersion = escapeHtml(GENERATOR.version).replaceAll("\r", "&#13;").replaceAll("\n", "&#10;");
const MAX_SOURCE_BYTES = 4 * 1024 * 1024;
const MAX_TOTAL_SOURCE_BYTES = 64 * 1024 * 1024;
const MAX_SOURCES = 10_000;
const MAX_RUNTIME_FILE_BYTES = 4 * 1024 * 1024;
const MAX_TOTAL_RUNTIME_BYTES = 64 * 1024 * 1024;
const MAX_PRIMITIVE_TOKEN_BYTES = 16 * 1024;
const MAX_MEDIA_BYTES = 8 * 1024 * 1024;
const MAX_TOTAL_MEDIA_BYTES = 64 * 1024 * 1024;
const MAX_MEDIA_FILES = 1_000;
const MAX_STALE_REASON_BYTES = 512;
const MAX_STALE_STUB_BYTES = 4 * 1024;
// Canonicalize before comparing paths: Windows runners may expose the same
// directory through both long and 8.3 names, which are not lexically relative.
const portalRoot = await realpath(process.cwd());
await recoverOwnedCorpus(portalRoot);
const discoveredRepositoryRoot = await findRepositoryRoot(portalRoot);
const repositoryRoot = await realpath(discoveredRepositoryRoot);
const git = new GitSnapshot(repositoryRoot);
const commit = git.resolveHead();
const repositoryFiles = git.loadInventory(commit);
const repositoryDirectories = committedDirectoryPaths(repositoryFiles);

const portalConfigRelative = safeRelative(path.relative(repositoryRoot, path.join(portalRoot, "portal.config.json")).split(path.sep).join("/"), "portal configuration path");
const configRecord = git.requireRegular(portalConfigRelative, ["100644"], "portal configuration");
const configBytes = git.readBlobs([configRecord], { perObjectBytes: MAX_CONFIG_BYTES, totalBytes: MAX_CONFIG_BYTES, label: "portal configuration" }).get(portalConfigRelative);
const config = validatePortalConfig(JSON.parse(configBytes));
const base = config.base;
const layers = config.layers;
const repoRelative = config.repository_root;
if (typeof repoRelative !== "string" || !repoRelative || repoRelative.includes("\\") || path.isAbsolute(repoRelative)) throw new Error("repository_root: expected a relative POSIX path");
const configuredRepositoryRoot = await realpath(path.resolve(portalRoot, repoRelative));
if (!filesystemPathsEqual(repositoryRoot, configuredRepositoryRoot)) throw new Error("repository_root must resolve to the owning CodeFlow repository");
const adapterRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const selfContainedRuntime = filesystemPathsEqual(path.resolve(adapterRoot), path.resolve(portalRoot));
const runtimeInputs = selfContainedRuntime
  ? [".node-version", "astro.config.mjs", "package.json", "package-lock.json", "scripts", "src/components", "src/content.config.ts", "src/styles", "tsconfig.json"].map((item) => safeRelative(path.relative(repositoryRoot, path.join(portalRoot, item)).split(path.sep).join("/"), "portal runtime input"))
  : [];
const publicRootRelative = safeRelative(path.relative(repositoryRoot, path.join(portalRoot, "public")).split(path.sep).join("/"), "portal public path");
const publicRecords = selfContainedRuntime
  ? git.requireDirectory(publicRootRelative, "portal public directory").filter((record) => {
    const relative = record.path.slice(publicRootRelative.length + 1);
    return relative !== ".codeflow-generated.json" && !isReservedPublicPath(relative);
  })
  : [];
const snapshotPaths = [portalConfigRelative, ...runtimeInputs, ...publicRecords.map((record) => record.path), ...config.source_roots, ...(config.primitive_tokens === null ? [] : [config.primitive_tokens])].map((item) => safeRelative(item, "snapshot path"));
git.assertClean(snapshotPaths);
const runtimeRecords = git.recordsForInputs([portalConfigRelative, ...runtimeInputs], "portal runtime input");
const authorityRecords = [...runtimeRecords, ...publicRecords];
const authorityBlobs = git.readBlobs(authorityRecords, { perObjectBytes: MAX_RUNTIME_FILE_BYTES, totalBytes: MAX_TOTAL_RUNTIME_BYTES, label: "portal runtime input" });
await assertWorktreeMatchesCommit(authorityRecords, authorityBlobs, MAX_RUNTIME_FILE_BYTES, MAX_TOTAL_RUNTIME_BYTES, "portal runtime input");
const committedPublicFiles = new Map(publicRecords.map((record) => [record.path.slice(publicRootRelative.length + 1), authorityBlobs.get(record.path)]));
let primitiveTokens = null;
let primitiveTokenEvidence = null;
let primitiveTokenRecord = null;
let primitiveTokenBlob = null;
if (config.primitive_tokens !== null) {
  const tokenRecord = git.requireRegular(config.primitive_tokens, ["100644"], "primitive token import");
  const bytes = git.readBlobs([tokenRecord], { perObjectBytes: MAX_PRIMITIVE_TOKEN_BYTES, totalBytes: MAX_PRIMITIVE_TOKEN_BYTES, label: "primitive token import" }).get(config.primitive_tokens);
  primitiveTokenRecord = tokenRecord;
  primitiveTokenBlob = bytes;
  await assertWorktreeMatchesCommit([tokenRecord], new Map([[tokenRecord.path, bytes]]), MAX_PRIMITIVE_TOKEN_BYTES, MAX_PRIMITIVE_TOKEN_BYTES, "primitive token import");
  primitiveTokens = validatePrimitiveTokens(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)), config.theme);
  primitiveTokenEvidence = { source_path: config.primitive_tokens, source_sha256: sha256(bytes) };
}
const primitiveTokenCss = renderPrimitiveTokenCss(primitiveTokens);
if (primitiveTokenEvidence !== null) Object.assign(primitiveTokenEvidence, { output_path: ".portal/generated/project-tokens.css", output_sha256: sha256(primitiveTokenCss) });

const excludes = (config.exclude ?? []).map((item) => safeRelative(item, "exclude"));
// The records switch (ADR-0064): while it is off, the configured pointer
// folders are not portal pages, claim no route and reach no layer, search
// entry or llms.txt line. One generated pointer page names them instead.
const recordsSwitch = config.records;
const recordPointers = recordsSwitch.enabled ? [] : recordsSwitch.pointers;
const recordPointerRoute = recordPointers.length ? `${recordsSwitch.layer}/records` : null;
const sourceRecords = committedMarkdownSources(config.source_roots, [...excludes, ...recordPointers.map((pointer) => pointer.folder)]);
const sources = sourceRecords.map((record) => record.path);
const sourceBlobs = git.readBlobs(sourceRecords, { perObjectBytes: MAX_SOURCE_BYTES, totalBytes: MAX_TOTAL_SOURCE_BYTES, label: "portal source" });
await assertWorktreeMatchesCommit(sourceRecords, sourceBlobs, MAX_SOURCE_BYTES, MAX_TOTAL_SOURCE_BYTES, "portal source");
const routeOwners = new Map();
const sourceRoutes = new Map();
for (const sourcePath of sources) {
  const layer = chooseLayer(sourcePath, layers);
  const route = `${layer.id}/${localRouteFor(sourcePath, config.source_roots)}`;
  claimRoute(route, sourcePath);
  sourceRoutes.set(sourcePath, route);
}

const recordFolders = recordPointers.map((pointer) => ({
  ...pointer,
  exists: repositoryDirectories.has(pointer.folder),
  files: recordFilesFor(pointer, repositoryFiles),
}));
// An id the guide does not publish still resolves: to its repository file
// where a provider URL is configured, and otherwise to the pointer page that
// names its folder. It is never a dangling link and never a page.
const recordTargets = new Map();
for (const folder of recordFolders) {
  for (const file of folder.files) {
    const href = pinnedSourceUrl(file.path) ?? (recordPointerRoute === null ? null : withBase(base, recordPointerRoute));
    if (href === null) continue;
    recordTargets.set(file.id, { href, title: `${file.path} at ${commit}` });
  }
}

const pages = [];
const sourceAnchors = new Map();
const capabilityRows = [];
for (const sourcePath of sources) {
  const bytes = sourceBlobs.get(sourcePath);
  const sourceHash = sha256(bytes);
  try {
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    const { frontmatter, body } = parseMarkdown(text, sourcePath);
    sourceAnchors.set(sourcePath, headingAnchors(body));
    validatePageMetadata(frontmatter, sourcePath);
    const layer = chooseLayer(sourcePath, layers);
    const route = sourceRoutes.get(sourcePath);
    const title = titleFor(frontmatter, body, sourcePath);
    const ids = collectPageIds(frontmatter, text, sourcePath);
    const relationships = extractPageRelationships(frontmatter, text, sourcePath);
    if (ids.length > EVIDENCE_LIMITS.idsPerPage) throw new Error(`${sourcePath}: identity count exceeds ${EVIDENCE_LIMITS.idsPerPage}`);
    if (relationships.length > EVIDENCE_LIMITS.relationshipsPerPage) throw new Error(`${sourcePath}: relationship count exceeds ${EVIDENCE_LIMITS.relationshipsPerPage}`);
    if (sourcePath === CAPABILITY_REGISTRY) capabilityRows.push(...capabilityFenceRecords(body, sourcePath));
    const excerpt = excerptFor(text);
    pages.push({ source_path: sourcePath, source_sha256: sourceHash, built_from_commit: commit, route, layer: layer.id, title, frontmatter, body, ids, unavailable_ids: [], lookup_ids: ids, relationships, backlinks: [], stale: false, searchable: true, excerpt });
  } catch (error) {
    sourceAnchors.delete(sourcePath);
    pages.push(staleStubPage(sourcePath, sourceHash, bytes, error));
  }
}
git.assertClean(snapshotPaths);
await assertWorktreeMatchesCommit(sourceRecords, sourceBlobs, MAX_SOURCE_BYTES, MAX_TOTAL_SOURCE_BYTES, "portal source");
if (primitiveTokenRecord !== null) await assertWorktreeMatchesCommit([primitiveTokenRecord], new Map([[primitiveTokenRecord.path, primitiveTokenBlob]]), MAX_PRIMITIVE_TOKEN_BYTES, MAX_PRIMITIVE_TOKEN_BYTES, "primitive token import");
await assertWorktreeMatchesCommit(authorityRecords, authorityBlobs, MAX_RUNTIME_FILE_BYTES, MAX_TOTAL_RUNTIME_BYTES, "portal runtime input");
if (git.resolveHead() !== commit) throw new Error("repository HEAD changed while the portal snapshot was being read");

const ownerById = new Map();
for (const page of pages) {
  for (const id of page.lookup_ids) {
    if (ownerById.has(id)) throw new Error(`duplicate identity ${id}: ${ownerById.get(id).source_path} and ${page.source_path}`);
    ownerById.set(id, page);
  }
}
for (const page of pages) {
  for (const relationship of page.relationships) {
    const target = ownerById.get(relationship.target);
    // A declared relationship that names a record the guide does not publish
    // resolves exactly like an inline mention of it: to the repository file,
    // or to the pointer page for its folder.
    if (!target && recordTargets.has(relationship.target)) continue;
    if (!target) throw new Error(`${page.source_path}: relationship target does not exist: ${relationship.target}`);
    if (!page.stale && !target.stale) target.backlinks.push({ type: relationship.type, source_route: page.route, source_id: relationship.source_id, target: relationship.target });
  }
}
for (const page of pages) page.backlinks.sort((a, b) => compareDeterministicText(JSON.stringify(a), JSON.stringify(b)));
for (const page of pages) if (page.backlinks.length > EVIDENCE_LIMITS.backlinksPerPage) throw new Error(`${page.source_path}: backlink count exceeds ${EVIDENCE_LIMITS.backlinksPerPage}`);
const previewMetadata = new Map([...ownerById].filter(([, owner]) => !owner.stale).map(([id, owner]) => [id, {
  route: withBase(base, owner.route), title: owner.title,
  status: typeof owner.frontmatter?.status === "string" ? owner.frontmatter.status : owner.status,
  source_path: owner.source_path, stale: owner.stale,
}]));

// Starlight sorts an autogenerated sidebar group alphabetically unless a page
// declares sidebar.order, so every generated page declares its place: the
// landing first, the configured reading order next, the pointer page last.
const sidebarOrder = new Map();
for (const layer of layers) {
  sidebarOrder.set(layer.id, 0);
  const layerPages = orderedLayerPages(layer, pages.filter((page) => page.route.startsWith(`${layer.id}/`)));
  layerPages.forEach((page, index) => sidebarOrder.set(page.route, index + 1));
  if (recordPointerRoute !== null && layer.id === recordsSwitch.layer) sidebarOrder.set(recordPointerRoute, layerPages.length + 1);
}

const renderedPages = [];
const mediaReferences = new Map();
for (const page of pages) {
  const rendered = page.stale ? renderStaleStub(page) : renderPage(page, ownerById, previewMetadata, mediaReferences, sourceAnchors);
  const outputMarkdown = `src/content/docs/${page.route}.md`;
  const twin = `public/markdown/${page.route}.md`;
  renderedPages.push({ route: page.route, rendered });
  page.output_markdown = outputMarkdown;
  page.output_markdown_sha256 = sha256(rendered);
  page.markdown_twin = twin;
  page.markdown_twin_sha256 = sha256(rendered);
  page.snippets = page.stale || !page.excerpt ? [] : [{ start_line: page.excerpt.start, end_line: page.excerpt.end, sha256: sha256(page.excerpt.text) }];
  assertEvidencePageLimits(page, page.source_path);
  page.status = typeof page.frontmatter?.status === "string" ? page.frontmatter.status : null;
  delete page.frontmatter; delete page.body; delete page.excerpt; delete page.layer; delete page.lookup_ids;
}

const renderedLandings = [];
for (const layer of layers) {
  const layerPages = pages.filter((page) => page.route.startsWith(`${layer.id}/`) && !page.stale);
  const landing = renderLayerLanding(layer, layerPages);
  renderedLandings.push({ path: `${layer.id}/index.md`, rendered: landing });
}
if (recordPointerRoute !== null) renderedLandings.push({ path: `${recordPointerRoute}.md`, rendered: renderRecordPointerPage() });
const renderedIndex = renderIndex(layers);

if (mediaReferences.size > MAX_MEDIA_FILES) throw new Error(`referenced media count exceeds ${MAX_MEDIA_FILES}`);
git.assertClean([...snapshotPaths, ...mediaReferences.keys()]);
const mediaFiles = new Map();
const mediaEvidence = [];
let totalMediaBytes = 0;
const mediaEntries = [...mediaReferences].sort(([left], [right]) => compareDeterministicText(left, right));
const mediaRecords = mediaEntries.map(([sourcePath]) => git.requireRegular(sourcePath, ["100644"], "referenced media"));
const mediaBlobs = git.readBlobs(mediaRecords, { perObjectBytes: MAX_MEDIA_BYTES, totalBytes: MAX_TOTAL_MEDIA_BYTES, label: "referenced media" });
await assertWorktreeMatchesCommit(mediaRecords, mediaBlobs, MAX_MEDIA_BYTES, MAX_TOTAL_MEDIA_BYTES, "referenced media");
for (const [sourcePath, outputPath] of mediaEntries) {
  const bytes = mediaBlobs.get(sourcePath);
  assertRaster(sourcePath, bytes);
  totalMediaBytes += bytes.length;
  if (totalMediaBytes > MAX_TOTAL_MEDIA_BYTES) throw new Error(`referenced media corpus exceeds ${MAX_TOTAL_MEDIA_BYTES} bytes`);
  mediaFiles.set(outputPath, bytes);
  mediaEvidence.push({ source_path: sourcePath, source_sha256: sha256(bytes), output_path: `public/${outputPath}`, output_sha256: sha256(bytes) });
}
git.assertClean([...snapshotPaths, ...mediaReferences.keys()]);
await assertWorktreeMatchesCommit(sourceRecords, sourceBlobs, MAX_SOURCE_BYTES, MAX_TOTAL_SOURCE_BYTES, "portal source");
if (primitiveTokenRecord !== null) await assertWorktreeMatchesCommit([primitiveTokenRecord], new Map([[primitiveTokenRecord.path, primitiveTokenBlob]]), MAX_PRIMITIVE_TOKEN_BYTES, MAX_PRIMITIVE_TOKEN_BYTES, "primitive token import");
await assertWorktreeMatchesCommit(mediaRecords, mediaBlobs, MAX_MEDIA_BYTES, MAX_TOTAL_MEDIA_BYTES, "referenced media");
await assertWorktreeMatchesCommit(authorityRecords, authorityBlobs, MAX_RUNTIME_FILE_BYTES, MAX_TOTAL_RUNTIME_BYTES, "portal runtime input");
if (git.resolveHead() !== commit) throw new Error("repository HEAD changed while referenced media was being read");

const llms = [`# ${escapeMarkdownInline(config.title)}`, "", escapeMarkdownInline(config.description), "", `Repository commit: ${commit}`, ...(config.release_version === null ? [] : [`Release version: ${config.release_version}`]), "", ...pages.filter((page) => !page.stale).map((page) => `- [${escapeMarkdownInline(page.route)}](./markdown/${page.route.split("/").map(strictUrlSegment).join("/")}.md) — ${escapeMarkdownInline(page.source_path)}`), ""].join("\n");
const evidence = {
  schema_version: 1,
  generator: GENERATOR,
  repository: { root: repoRelative, commit, release_version: config.release_version },
  config_sha256: sha256(configBytes),
  primitive_tokens: primitiveTokenEvidence,
  media: mediaEvidence,
  pages,
  llms: { path: "public/llms.txt", sha256: sha256(llms) },
  artifacts: [],
};
const evidenceText = `${JSON.stringify(evidence, null, 2)}\n`;
assertEvidenceEnvelope(pages, evidenceText);
const contentFiles = new Map([...renderedPages.map((page) => [`${page.route}.md`, page.rendered]), ...renderedLandings.map((landing) => [landing.path, landing.rendered]), ["index.md", renderedIndex]]);
const publicFiles = new Map([...committedPublicFiles, ...renderedPages.map((page) => [`markdown/${page.route}.md`, page.rendered]), ...mediaFiles, ["llms.txt", llms]]);
await publishOwnedCorpus(portalRoot, [
  { live: ".portal/generated", files: new Map([["evidence.json", evidenceText], ["project-tokens.css", primitiveTokenCss]]) },
  { live: "src/content/docs", files: contentFiles },
  { live: "public", files: publicFiles, preserveUnknown: false },
]);
console.log(`portal: adapted ${pages.length} source page(s) across ${layers.length} layer(s)`);

function chooseLayer(sourcePath, definitions) {
  return definitions.find((layer) => (layer.paths ?? []).includes(sourcePath) || (layer.prefixes ?? []).some((prefix) => sourcePath === prefix || sourcePath.startsWith(`${prefix}/`))) ?? definitions.find((layer) => layer.fallback);
}

function claimRoute(route, sourcePath) {
  const output = `${route}.md`;
  const reserved = new Set(["index.md", "404.md", ...layers.map((layer) => `${layer.id}/index.md`), ...(recordPointerRoute === null ? [] : [`${recordPointerRoute}.md`])]);
  if (reserved.has(output)) throw new Error(`reserved generated route: ${sourcePath} -> ${route}`);
  const key = route.normalize("NFC").toLowerCase();
  if (routeOwners.has(key)) throw new Error(`route collision: ${sourcePath} and ${routeOwners.get(key)} -> ${route}`);
  routeOwners.set(key, sourcePath);
}

// The forty character hash wrapped onto a second line at 390 pixels and led
// every page. The linked line now carries one short pin and puts the full
// hash in the link title. With no provider link, because no repository URL is
// configured or its host has no known blob layout, there is no title to carry
// it, so the line keeps its own built-from segment with the full hash.
function sourceLink(sourcePath) {
  const label = `<code>${escapeHtml(sourcePath)}</code> at <code>${escapeHtml(commit.slice(0, 12))}</code>`;
  const href = pinnedSourceUrl(sourcePath);
  if (href === null) return label;
  return `<a href="${escapeHtml(href)}" title="${escapeHtml(`${sourcePath} at ${commit}`)}">${label}</a>`;
}

function builtFromSegment(sourcePath) {
  return pinnedSourceUrl(sourcePath) === null ? ` · built from <code>${escapeHtml(commit)}</code>` : "";
}

// The registry's YAML fences are the machine-read authority. The page shows
// them as one generated summary table and one compact definition table per
// capability, never as a block of raw YAML.
function renderCapabilityRegistry(markdown, sourcePath) {
  return renderCapabilityFences(placeCapabilityTable(markdown, capabilitiesTableMarkdown(capabilityRows)), escapeMarkdownCell, sourcePath);
}

// The hand written registry table carried seven columns and two long id
// lists, so it ran past the reading measure at 1280. The generated summary
// keeps the four fields a reader scans; every other declared field waits in
// the capability's own definition table further down the page.
function capabilitiesTableMarkdown(rows) {
  if (!rows.length) return null;
  const head = "| Capability | Name | Area | Status |\n|---|---|---|---|";
  const body = rows.map((row) => {
    const field = (name) => escapeMarkdownCell(row.lookup.get(name) ?? "not declared");
    return `| ${escapeMarkdownCell(row.id)} | ${field("name")} | ${field("area")} | ${field("status")} |`;
  });
  return `${head}\n${body.join("\n")}`;
}

function pinnedSourceUrl(sourcePath, target = "file") {
  return providerSourceUrl(config.repository_url, commit, sourcePath, target);
}

function renderPage(page, routesById, previews, referencedMedia, anchorsBySource) {
  const status = typeof page.frontmatter.status === "string" ? page.frontmatter.status : null;
  const amendments = page.source_path.startsWith("docs/decisions/") ? amendmentHeadings(page.body) : [];
  const relationships = page.relationships.map((relation) => {
    const target = routesById.get(relation.target);
    const label = target && target.stale ? `${relation.target} (stale)` : relation.target;
    const href = target ? withBase(base, target.route) : recordTargets.get(relation.target).href;
    return `- ${relation.source_id ? `\`${relation.source_id}\` · ` : ""}**${relation.type.replaceAll("_", " ")}** → [${label}](${href})`;
  }).join("\n");
  const backlinks = page.backlinks.map((backlink) => `- **${backlink.type.replaceAll("_", " ")}** ← [${backlink.source_id ?? backlink.source_route}](${withBase(base, backlink.source_route)})`).join("\n");
  const referenced = referencedIds(page.body).filter((id) => routesById.has(id) && !page.ids.includes(id));
  const sourceMarkdown = renderStageFences(rewriteRepositoryMarkdown(stripLeadingTitleHeading(page.body, page.title), {
    sourcePath: page.source_path,
    sourceRoutes,
    repositoryFiles,
    repositoryDirectories,
    pinnedSourceUrl,
    commit,
    base,
    strictTargets: previews,
    mediaReferences: referencedMedia,
    sourceAnchors: anchorsBySource,
    recordTargets,
    selfRoute: withBase(base, page.route),
  }), page.source_path);
  const safeBody = decorateAltitude(page.source_path === CAPABILITY_REGISTRY ? renderCapabilityRegistry(sourceMarkdown, page.source_path) : sourceMarkdown);
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
    return `- [${id}${owner.stale ? " (stale)" : ""}](${withBase(base, owner.route)})`;
  }).join("\n")}`);
  if (backlinks) context.push(`### Inverse links\n\n${backlinks}`);
  const recordContext = context.length ? `\n\n---\n\n${context.join("\n\n")}` : "";
  const release = config.release_version === null ? "" : ` · release <code>${escapeHtml(config.release_version)}</code>`;
  return `---\ntitle: ${JSON.stringify(page.title)}\ndescription: ${JSON.stringify(typeof page.frontmatter.description === "string" ? page.frontmatter.description : `Repository source: ${page.source_path}`)}\nslug: ${JSON.stringify(page.route)}\n${sidebarFrontmatter(page.route)}---\n\n${provenanceMarker(page)}\n<div class="portal-provenance">Source ${sourceLink(page.source_path)}${builtFromSegment(page.source_path)}${release} · portal <code>${renderedGeneratorVersion}</code></div>${snippetMarker}\n\n<div data-pagefind-body data-codeflow-search-root="${escapeHtml(page.route)}">\n\n${safeBody}${recordContext}\n\n</div>\n`;
}

function renderStaleStub(page) {
  const rendered = `---\ntitle: ${JSON.stringify(page.title)}\ndescription: ${JSON.stringify(`Source failed to build: ${page.source_path}`)}\nslug: ${JSON.stringify(page.route)}\n${sidebarFrontmatter(page.route)}pagefind: false\n---\n\n${provenanceMarker(page)}\n<div class="portal-provenance">Source ${sourceLink(page.source_path)}${builtFromSegment(page.source_path)} · portal <code>${renderedGeneratorVersion}</code></div>\n\n<div class="portal-stale" data-pagefind-ignore="all">\n\n> **Source unavailable:** This page's source failed to build at commit <code>${commit.slice(0, 12)}</code>. Fix <code>${escapeHtml(page.source_path)}</code> and rebuild; the previous version of this page is not shown.\n\n<details><summary>Build diagnostic</summary>\n\n${escapeMarkdownInline(page.stale_reason)}\n\n</details>\n\n</div>\n`;
  if (Buffer.byteLength(rendered) > MAX_STALE_STUB_BYTES) throw new Error(`${page.source_path}: stale stub exceeds ${MAX_STALE_STUB_BYTES} bytes`);
  return rendered;
}

function provenanceMarker(page) {
  return `<!-- codeflow-page-provenance source_sha256=${page.source_sha256} built_from_commit=${commit} portal_version=${renderedGeneratorVersion} release_version=${config.release_version ?? "none"} -->`;
}

// A landing says what the layer is for and then shows the order to read it
// in: the configured pages first, in their configured order, then everything
// the prefixes swept in. One line per page, never a bullet list of files.
function renderLayerLanding(layer, layerPages) {
  const preface = `---\ntitle: ${JSON.stringify(layer.label)}\ndescription: ${JSON.stringify(layer.description)}\nslug: ${JSON.stringify(layer.id)}\n${sidebarFrontmatter(layer.id)}---\n\n${escapeMarkdownInline(layer.description)}\n`;
  const entries = orderedLayerPages(layer, layerPages).map((page) => ({ title: page.title, route: page.route }));
  if (recordPointerRoute !== null && layer.id === recordsSwitch.layer) entries.push({ title: RECORD_POINTER_TITLE, route: recordPointerRoute });
  if (!entries.length) return `${preface}\nNo current sources in this layer.\n`;
  const first = entries[0];
  return `${preface}\n${readingOrderFigure(layer, entries)}\n\nStart with [${escapeMarkdownInline(first.title)}](${withBase(base, first.route)}); the sidebar follows the same order and nests a page's sub-pages beneath it.\n`;
}

// Rendered defect: reading order follows the layer configuration, so a
// configured first page is first even when its route sorts last.
function orderedLayerPages(layer, layerPages) {
  const rank = new Map((layer.paths ?? []).map((item, index) => [item, index]));
  return [...layerPages].sort((left, right) => {
    const leftRank = rank.has(left.source_path) ? rank.get(left.source_path) : Number.MAX_SAFE_INTEGER;
    const rightRank = rank.has(right.source_path) ? rank.get(right.source_path) : Number.MAX_SAFE_INTEGER;
    return leftRank !== rightRank ? leftRank - rightRank : compareDeterministicText(left.route, right.route);
  });
}

function sidebarFrontmatter(route) {
  if (!sidebarOrder.has(route)) throw new Error(`no sidebar order for route ${route}`);
  return `sidebar:\n  order: ${sidebarOrder.get(route)}\n`;
}

function readingOrderFigure(layer, entries) {
  const items = entries.map((entry, index) => `<li><a href="${escapeHtml(withBase(base, entry.route))}"><span class="n">${String(index + 1).padStart(2, "0")}</span><span class="t">${escapeHtml(entry.title)}</span></a></li>`).join("");
  return `<figure class="portal-reading-order"><ol>${items}</ol><figcaption>${escapeHtml(`Reading order for ${layer.label}: ${entries.length} page(s)`)}</figcaption></figure>`;
}

function escapeMarkdownCell(value) {
  return escapeMarkdownInline(value).replaceAll("|", "\\|");
}

function escapeMarkdownInline(value) {
  return escapeHtml(String(value).replace(/[\r\n\t]+/g, " "))
    .replace(/([\\`*_[\]{}()#+.!])/g, "\\$1")
    .replaceAll(":", "&#58;");
}

// The home page is the reading path itself: one step per configured layer,
// in configured order, each carrying the label and the sentence the
// configuration gives it. The stage grammar stacks the steps at 390.
function renderIndex(definitions) {
  const lead = `This guide reads in ${definitions.length} steps, from ${definitions[0].label} to ${definitions.at(-1).label}.`;
  const acting = `Start with [${escapeMarkdownInline(definitions[0].label)}](${withBase(base, definitions[0].id)}); every step is also a section of the sidebar.`;
  return `---\ntitle: ${JSON.stringify(config.title)}\ndescription: ${JSON.stringify(config.description)}\nslug: "index"\ntemplate: splash\nhero:\n  tagline: ${JSON.stringify(config.description)}\n---\n\n${escapeMarkdownInline(lead)}\n\n${readingPathFigure(definitions)}\n\n${acting}\n\n<p class="portal-version">Repository <code>${commit}</code>${config.release_version === null ? "" : ` · release <code>${escapeHtml(config.release_version)}</code>`} · portal <code>${renderedGeneratorVersion}</code></p>\n`;
}

function readingPathFigure(definitions) {
  const steps = definitions.map((layer, index) => {
    const role = index === 0 ? "accent" : "neutral";
    return `<ul class="portal-stage-group"><li class="portal-stage-node" data-role="${role}"><a href="${escapeHtml(withBase(base, layer.id))}"><span class="k">${escapeHtml(layer.label)}</span><span class="s">${escapeHtml(layer.description)}</span></a></li></ul>`;
  }).join(`<div class="portal-stage-arrow" aria-hidden="true"></div>`);
  return `<figure class="portal-stage portal-reading-path"><div class="portal-stage-flow">${steps}</div><figcaption>The reading path this portal is configured for</figcaption></figure>`;
}

// One generated page for the records the guide does not publish: the folder,
// what it holds, how many it holds at this commit, and where it lives. It
// lists folders, never files.
function renderRecordPointerPage() {
  const rows = recordFolders.map((folder) => {
    const count = folder.exists && folder.files.length ? String(folder.files.length) : "none yet";
    const href = folder.exists ? pinnedSourceUrl(folder.folder, "directory") : null;
    const location = href === null ? `\`${escapeMarkdownCell(folder.folder)}\`` : `[${escapeMarkdownCell(folder.folder)}](${href})`;
    return `| \`${escapeMarkdownCell(folder.folder)}\` | ${escapeMarkdownCell(folder.purpose)} | ${count} | ${location} |`;
  });
  const total = recordFolders.reduce((sum, folder) => sum + folder.files.length, 0);
  const lead = `The guide has no page for a decision or a work record. ${total} of them sit in ${recordFolders.length} repository folders at the commit this portal was built from.`;
  const prefixes = [...new Set(recordFolders.map((folder) => folder.id_prefix))];
  const closing = `Pages in this guide cite these records by id (${prefixes.join(", ")}), and each id links to its file in the repository.`;
  return `---\ntitle: ${JSON.stringify(RECORD_POINTER_TITLE)}\ndescription: ${JSON.stringify("The repository folders that hold the decisions, epics, tasks and specs this guide cites.")}\nslug: ${JSON.stringify(recordPointerRoute)}\n${sidebarFrontmatter(recordPointerRoute)}---\n\n${escapeMarkdownInline(lead)}\n\n| Folder | Purpose | Count | Repository |\n|---|---|---|---|\n${rows.join("\n")}\n\n${escapeMarkdownInline(closing)}\n`;
}

function staleStubPage(sourcePath, sourceHash, bytes, error) {
  const layer = chooseLayer(sourcePath, layers);
  let text = "";
  try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); } catch {}
  const unavailableIds = recoverUnavailableIds(text, sourcePath);
  return {
    source_path: sourcePath,
    source_sha256: sourceHash,
    built_from_commit: commit,
    route: sourceRoutes.get(sourcePath),
    layer: layer.id,
    title: path.posix.basename(sourcePath, ".md").replaceAll("-", " "),
    frontmatter: {},
    body: "",
    ids: [],
    unavailable_ids: unavailableIds,
    lookup_ids: unavailableIds,
    relationships: [],
    backlinks: [],
    stale: true,
    searchable: false,
    excerpt: null,
    stale_reason: boundedDiagnostic(error),
  };
}

function boundedDiagnostic(error) {
  const clean = String(error?.message ?? error).replace(/[\u0000-\u001f\u007f]+/g, " ").replace(/\s+/g, " ").trim() || "source parsing failed";
  let output = clean;
  while (Buffer.byteLength(output) > MAX_STALE_REASON_BYTES) output = output.slice(0, -1);
  return output;
}

function escapeHtml(value) {
  return String(value).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

function filesystemPathsEqual(left, right) {
  return process.platform === "win32" ? left.toLowerCase() === right.toLowerCase() : left === right;
}

function committedMarkdownSources(configuredRoots, excluded) {
  const sources = new Map();
  const keys = new Set();
  for (const root of configuredRoots) {
    for (const record of git.requireDirectory(root, "source_root")) {
      const sourcePath = record.path;
      if (!sourcePath.endsWith(".md") || /^\.env(?:\.|$)/.test(path.posix.basename(sourcePath)) || excluded.some((prefix) => sourcePath === prefix || sourcePath.startsWith(`${prefix}/`))) continue;
      if (!["100644", "100755"].includes(record.mode)) throw new Error(`committed source is not a regular file: ${sourcePath}`);
      const key = sourcePath.normalize("NFC").toLowerCase();
      if (keys.has(key) && !sources.has(sourcePath)) throw new Error(`committed source path collides case-insensitively: ${sourcePath}`);
      keys.add(key);
      sources.set(sourcePath, record);
      if (sources.size > MAX_SOURCES) throw new Error(`source count exceeds ${MAX_SOURCES}`);
    }
  }
  if (!sources.size) throw new Error("configured source roots contain no publishable Markdown files");
  return [...sources.values()].sort((left, right) => compareDeterministicText(left.path, right.path));
}

async function assertWorktreeMatchesCommit(records, committedBlobs, perFileBytes, totalLimit, label) {
  let totalBytes = 0;
  for (const record of records) {
    const committed = committedBlobs.get(record.path);
    const worktree = await readBoundedRegularFile(path.join(repositoryRoot, record.path), perFileBytes, `${label} ${record.path}`);
    totalBytes += committed.length;
    if (totalBytes > totalLimit) throw new Error(`${label} inputs exceed ${totalLimit} bytes`);
    // Git may materialize committed LF text as CRLF on Windows. Accept only
    // that reversible text transformation; committed blobs remain authority.
    if (!checkoutEquivalentBytes(committed, worktree)) throw new Error(`${label} does not match ${commit}: ${record.path}`);
  }
}

function assertRaster(sourcePath, bytes) {
  const extension = path.posix.extname(sourcePath).toLowerCase();
  const ascii = (start, end) => bytes.subarray(start, end).toString("ascii");
  const valid = extension === ".png" ? bytes.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))
    : [".jpg", ".jpeg"].includes(extension) ? bytes.length >= 4 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff
      : extension === ".gif" ? ["GIF87a", "GIF89a"].includes(ascii(0, 6))
        : extension === ".webp" ? ascii(0, 4) === "RIFF" && ascii(8, 12) === "WEBP"
          : false;
  if (!valid) throw new Error(`referenced media bytes do not match the approved raster type: ${sourcePath}`);
  const { width, height } = rasterDimensions(sourcePath, bytes);
  const pixels = width * height;
  if (!Number.isSafeInteger(width) || !Number.isSafeInteger(height) || width < 1 || height < 1 || width > 8192 || height > 8192 || pixels > 33_554_432) {
    throw new Error(`referenced media dimensions exceed the portal limit: ${sourcePath}`);
  }
}

function rasterDimensions(sourcePath, bytes) {
  const extension = path.posix.extname(sourcePath).toLowerCase();
  if (extension === ".png") {
    if (bytes.length < 24 || bytes.readUInt32BE(8) !== 13 || bytes.subarray(12, 16).toString("ascii") !== "IHDR") throw new Error(`referenced PNG header is truncated or invalid: ${sourcePath}`);
    return { width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
  }
  if (extension === ".gif") {
    if (bytes.length < 10) throw new Error(`referenced GIF header is truncated: ${sourcePath}`);
    return { width: bytes.readUInt16LE(6), height: bytes.readUInt16LE(8) };
  }
  if (extension === ".webp") {
    if (bytes.length < 30) throw new Error(`referenced WebP header is truncated: ${sourcePath}`);
    const kind = bytes.subarray(12, 16).toString("ascii");
    if (kind === "VP8X") return { width: 1 + readUInt24LE(bytes, 24), height: 1 + readUInt24LE(bytes, 27) };
    if (kind === "VP8L" && bytes[20] === 0x2f) {
      return {
        width: 1 + bytes[21] + ((bytes[22] & 0x3f) << 8),
        height: 1 + (bytes[22] >> 6) + (bytes[23] << 2) + ((bytes[24] & 0x0f) << 10),
      };
    }
    if (kind === "VP8 " && bytes[23] === 0x9d && bytes[24] === 0x01 && bytes[25] === 0x2a) {
      return { width: bytes.readUInt16LE(26) & 0x3fff, height: bytes.readUInt16LE(28) & 0x3fff };
    }
    throw new Error(`referenced WebP dimensions are missing or invalid: ${sourcePath}`);
  }
  if ([".jpg", ".jpeg"].includes(extension)) return jpegDimensions(sourcePath, bytes);
  throw new Error(`referenced media type has no bounded dimension parser: ${sourcePath}`);
}

function jpegDimensions(sourcePath, bytes) {
  let offset = 2;
  const startOfFrame = new Set([0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7, 0xc9, 0xca, 0xcb, 0xcd, 0xce, 0xcf]);
  while (offset < bytes.length) {
    while (offset < bytes.length && bytes[offset] === 0xff) offset += 1;
    if (offset >= bytes.length) break;
    const marker = bytes[offset++];
    if (marker === 0xd9 || marker === 0xda) break;
    if (marker === 0x01 || marker >= 0xd0 && marker <= 0xd7) continue;
    if (offset + 2 > bytes.length) break;
    const length = bytes.readUInt16BE(offset);
    if (length < 2 || offset + length > bytes.length) break;
    if (startOfFrame.has(marker)) {
      if (length < 7) break;
      return { width: bytes.readUInt16BE(offset + 5), height: bytes.readUInt16BE(offset + 3) };
    }
    offset += length;
  }
  throw new Error(`referenced JPEG dimensions are missing or invalid: ${sourcePath}`);
}

function readUInt24LE(bytes, offset) {
  return bytes[offset] + (bytes[offset + 1] << 8) + (bytes[offset + 2] << 16);
}
