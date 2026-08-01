import { createHash, randomBytes } from "node:crypto";
import { constants as fsConstants } from "node:fs";
import { copyFile, lstat, mkdir, open, readFile, readdir, rename, rm } from "node:fs/promises";
import path from "node:path";
import YAML from "yaml";

export const sha256 = (value) => createHash("sha256").update(value).digest("hex");

export function safeRelative(value, label = "path") {
  if (typeof value !== "string" || !value || value.includes("\\")) throw new Error(`${label}: expected a non-empty POSIX path`);
  const normalized = path.posix.normalize(value);
  if (normalized !== value || normalized.startsWith("/") || normalized === ".." || normalized.startsWith("../")) {
    throw new Error(`${label}: path must stay beneath its root: ${value}`);
  }
  return value;
}

export async function walkMarkdown(root, relative = "") {
  const dir = path.join(root, relative);
  const entries = await readdir(dir, { withFileTypes: true });
  const found = [];
  for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
    const rel = path.posix.join(relative, entry.name);
    if (entry.isSymbolicLink()) throw new Error(`source symlink refused: ${rel}`);
    if (entry.isDirectory()) {
      if ([".git", ".codeflow", "node_modules", "dist", ".portal", ".astro"].includes(entry.name)) continue;
      found.push(...await walkMarkdown(root, rel));
    } else if (entry.isFile() && entry.name.endsWith(".md") && !/^\.env(?:\.|$)/.test(entry.name)) found.push(rel);
  }
  return found;
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
  const heading = body.match(/^#\s+(.+)$/m)?.[1]?.trim();
  return heading || path.posix.basename(sourcePath, ".md").replaceAll("-", " ");
}

export function renderSafeMarkdown(body) {
  let fence = null;
  let comment = false;
  return body.split("\n").map((line) => {
    const marker = line.match(/^\s{0,3}(`{3,}|~{3,})/);
    if (marker) { fence = fence === null ? marker[1][0] : fence === marker[1][0] ? null : fence; return line; }
    if (fence !== null) return line;
    let visible = "";
    for (let cursor = 0; cursor < line.length;) {
      if (comment) {
        const end = line.indexOf("-->", cursor);
        if (end < 0) return visible;
        comment = false; cursor = end + 3; continue;
      }
      const start = line.indexOf("<!--", cursor);
      if (start < 0) { visible += line.slice(cursor); break; }
      visible += line.slice(cursor, start); comment = true; cursor = start + 4;
    }
    const spans = visible.split(/(`+[^`]*`+)/g);
    return spans.map((span, index) => index % 2 ? span : span.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;")).join("");
  }).join("\n");
}

export function linkStrictIds(markdown, routesById) {
  let fence = null;
  return markdown.split("\n").map((line) => {
    const marker = line.match(/^\s{0,3}(`{3,}|~{3,})/);
    if (marker) { fence = fence === null ? marker[1][0] : fence === marker[1][0] ? null : fence; return line; }
    if (fence !== null) return line;
    return line.split(/(`+[^`]*`+)/g).map((span, index) => {
      if (index % 2) return span;
      return span.replace(/(?<![\w/\[])\b((?:ADR|EPC|SPC|TSK|CAP)-\d{3,}(?:-\d{3,})?)\b(?![\]])/g, (match, id) => routesById.has(id) ? `[${id}](${routesById.get(id)})` : match);
    }).join("");
  }).join("\n");
}

export function referencedIds(markdown) {
  const found = new Set();
  let fence = null;
  let comment = false;
  for (const line of markdown.split("\n")) {
    const marker = line.match(/^\s{0,3}(`{3,}|~{3,})/);
    if (marker) { fence = fence === null ? marker[1][0] : fence === marker[1][0] ? null : fence; continue; }
    if (fence !== null) continue;
    let visible = "";
    for (let cursor = 0; cursor < line.length;) {
      if (comment) {
        const end = line.indexOf("-->", cursor);
        if (end < 0) break;
        comment = false; cursor = end + 3; continue;
      }
      const start = line.indexOf("<!--", cursor);
      if (start < 0) { visible += line.slice(cursor); break; }
      visible += line.slice(cursor, start); comment = true; cursor = start + 4;
    }
    visible.split(/(`+[^`]*`+)/g).forEach((span, index) => {
      if (index % 2) return;
      for (const match of span.matchAll(/\b(?:ADR|EPC|SPC|TSK|CAP)-\d{3,}(?:-\d{3,})?\b/g)) found.add(match[0]);
    });
  }
  return [...found];
}

const PUBLICATION_LIVE_PATHS = new Set([".portal/generated", "src/content/docs", "public"]);
const MAX_PRESERVED_UNKNOWN_BYTES = 64 * 1024 * 1024;
const MAX_PRESERVED_UNKNOWN_FILES = 10_000;
const MAX_PRESERVED_UNKNOWN_DEPTH = 32;
const PUBLICATION_LEASE_MAX_AGE_MS = 30 * 60 * 1000;

async function acquirePublicationLease(portalRoot) {
  await assertNoSymlink(portalRoot, ".portal");
  const portalDirectory = path.join(portalRoot, ".portal");
  const lockDirectory = path.join(portalDirectory, "publish.lock");
  await mkdir(portalDirectory, { recursive: true });
  await assertNoSymlink(portalRoot, ".portal");
  for (let attempt = 0; attempt < 4; attempt += 1) {
    const now = Date.now();
    const lease = {
      schema_version: 1,
      token: randomBytes(24).toString("hex"),
      created_at_ms: now,
      heartbeat_at_ms: now,
    };
    try {
      await mkdir(lockDirectory, { mode: 0o700 });
      try {
        await writeDurableJson(path.join(lockDirectory, "owner.json"), lease, { exclusive: true });
        await syncDirectory(portalDirectory);
        return lease;
      } catch (error) {
        await rm(lockDirectory, { recursive: true, force: true });
        throw error;
      }
    } catch (error) {
      if (error?.code !== "EEXIST") throw error;
      const existing = await readPublicationLease(lockDirectory);
      const heartbeat = existing?.heartbeat_at_ms ?? (await lstat(lockDirectory)).mtimeMs;
      if (Date.now() - heartbeat <= PUBLICATION_LEASE_MAX_AGE_MS) {
        throw new Error("portal publication already in progress");
      }
      const staleDirectory = path.join(portalDirectory, `.stale-publish-lock-${process.pid}-${Date.now()}-${randomBytes(8).toString("hex")}`);
      try { await rename(lockDirectory, staleDirectory); }
      catch (renameError) {
        if (["ENOENT", "EEXIST", "ENOTEMPTY"].includes(renameError?.code)) continue;
        throw renameError;
      }
      await rm(staleDirectory, { recursive: true, force: true });
      await syncDirectory(portalDirectory);
    }
  }
  throw new Error("could not acquire portal publication lease");
}

async function readPublicationLease(lockDirectory) {
  const metadata = await lstat(lockDirectory);
  if (metadata.isSymbolicLink() || !metadata.isDirectory()) throw new Error(`unsafe portal publication lock: ${lockDirectory}`);
  const owner = path.join(lockDirectory, "owner.json");
  try {
    const ownerMetadata = await lstat(owner);
    if (ownerMetadata.isSymbolicLink() || !ownerMetadata.isFile() || ownerMetadata.size > 4096) throw new Error(`unsafe portal publication lease: ${owner}`);
    const lease = JSON.parse(await readFile(owner, "utf8"));
    validatePublicationLease(lease);
    return lease;
  } catch (error) {
    if (error?.code === "ENOENT") return null;
    throw error;
  }
}

function validatePublicationLease(lease) {
  if (!lease || typeof lease !== "object" || Array.isArray(lease) || Object.keys(lease).sort().join(",") !== "created_at_ms,heartbeat_at_ms,schema_version,token" || lease.schema_version !== 1 || typeof lease.token !== "string" || !/^[a-f0-9]{48}$/.test(lease.token) || !Number.isSafeInteger(lease.created_at_ms) || !Number.isSafeInteger(lease.heartbeat_at_ms) || lease.created_at_ms < 0 || lease.heartbeat_at_ms < lease.created_at_ms) {
    throw new Error("invalid portal publication lease");
  }
}

async function assertPublicationLease(portalRoot, lease) {
  validatePublicationLease(lease);
  const current = await readPublicationLease(path.join(portalRoot, ".portal/publish.lock"));
  if (!current || current.token !== lease.token) throw new Error("portal publication lease was lost");
  if (Date.now() - current.heartbeat_at_ms > PUBLICATION_LEASE_MAX_AGE_MS) throw new Error("portal publication lease expired");
}

async function refreshPublicationLease(portalRoot, lease) {
  await assertPublicationLease(portalRoot, lease);
  lease.heartbeat_at_ms = Date.now();
  await writeDurableJson(path.join(portalRoot, ".portal/publish.lock/owner.json"), lease);
}

async function releasePublicationLease(portalRoot, lease) {
  const lockDirectory = path.join(portalRoot, ".portal/publish.lock");
  let current;
  try { current = await readPublicationLease(lockDirectory); }
  catch (error) { if (error?.code === "ENOENT") return; throw error; }
  if (!current || current.token !== lease.token) return;
  await rm(lockDirectory, { recursive: true, force: true });
  await syncDirectory(path.dirname(lockDirectory));
}

export async function recoverOwnedCorpus(portalRoot) {
  await withPublicationLeaseLocked(portalRoot, async () => {});
}

export async function withPublicationLease(portalRoot, action) {
  if (typeof action !== "function") throw new Error("publication lease action must be a function");
  return withPublicationLeaseLocked(portalRoot, (_lease, refresh) => action({ refresh }));
}

async function withPublicationLeaseLocked(portalRoot, action) {
  const lease = await acquirePublicationLease(portalRoot);
  try {
    await recoverOwnedCorpusLocked(portalRoot, lease);
    return await action(lease, () => refreshPublicationLease(portalRoot, lease));
  } finally { await releasePublicationLease(portalRoot, lease); }
}

async function recoverOwnedCorpusLocked(portalRoot, lease) {
  await assertPublicationLease(portalRoot, lease);
  const journalRelative = ".portal/publish-transaction.json";
  const journal = path.join(portalRoot, journalRelative);
  await assertNoSymlink(portalRoot, ".portal");
  try {
    const metadata = await lstat(journal);
    if (metadata.isSymbolicLink() || !metadata.isFile() || metadata.size > 64 * 1024) throw new Error(`unsafe portal publication journal: ${journal}`);
  } catch (error) {
    if (error?.code === "ENOENT") return;
    throw error;
  }
  const transaction = JSON.parse(await readFile(journal, "utf8"));
  validateTransaction(transaction);
  if (transaction.phase === "committed") {
    await rm(path.join(portalRoot, transaction.stage_root), { recursive: true, force: true });
    await rm(path.join(portalRoot, transaction.backup_root), { recursive: true, force: true });
    await rm(journal, { force: true });
    return;
  }
  for (const group of [...transaction.groups].reverse()) {
    const live = path.join(portalRoot, group.live);
    const stage = path.join(portalRoot, group.stage);
    const backup = path.join(portalRoot, group.backup);
    if (await exists(backup)) {
      await rm(live, { recursive: true, force: true });
      await mkdir(path.dirname(live), { recursive: true });
      await rename(backup, live);
    } else if (!group.had_live) await rm(live, { recursive: true, force: true });
    await rm(stage, { recursive: true, force: true });
  }
  await rm(path.join(portalRoot, transaction.stage_root), { recursive: true, force: true });
  await rm(path.join(portalRoot, transaction.backup_root), { recursive: true, force: true });
  await rm(journal, { force: true });
}

export async function publishOwnedCorpus(portalRoot, groups, options = {}) {
  return withPublicationLeaseLocked(portalRoot, (lease) => publishOwnedCorpusLocked(portalRoot, groups, options, lease));
}

async function publishOwnedCorpusLocked(portalRoot, groups, { faultAt = null, simulateCrash = false } = {}, lease) {
  await assertPublicationLease(portalRoot, lease);
  if (!Array.isArray(groups) || groups.length < 1 || groups.length > 3 || new Set(groups.map((group) => group.live)).size !== groups.length) {
    throw new Error("generated corpus must contain 1 to 3 unique live groups");
  }
  const nonce = `${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  const stageRoot = `.portal/.publish-stage-${nonce}`;
  const backupRoot = `.portal/.publish-backup-${nonce}`;
  const transactionGroups = [];
  const preparedGroups = [];
  try {
    for (const [index, group] of groups.entries()) {
      if (!PUBLICATION_LIVE_PATHS.has(group.live)) throw new Error(`unsupported generated corpus path: ${group.live}`);
      const planned = new Map();
      for (const [relative, content] of group.files) {
        const safe = safeRelative(relative, "generated file");
        if (planned.has(safe)) throw new Error(`duplicate generated file: ${safe}`);
        planned.set(safe, content);
      }
      const live = path.join(portalRoot, group.live);
      const stageRelative = `${stageRoot}/${index}`;
      const backupRelative = `${backupRoot}/${index}`;
      transactionGroups.push({ live: group.live, stage: stageRelative, backup: backupRelative, had_live: await exists(live) });
      preparedGroups.push({ live, stage: path.join(portalRoot, stageRelative), planned, preserveUnknown: group.preserveUnknown === true });
    }
    const transaction = { schema_version: 1, phase: "preparing", stage_root: stageRoot, backup_root: backupRoot, groups: transactionGroups };
    validateTransaction(transaction);
    const journal = path.join(portalRoot, ".portal/publish-transaction.json");
    await mkdir(path.dirname(journal), { recursive: true });
    await writeDurableJson(journal, transaction, { exclusive: true });
    maybeFault("after-journal", faultAt);
    for (const [index, group] of preparedGroups.entries()) {
      await refreshPublicationLease(portalRoot, lease);
      await prepareOwnedStage(group.live, group.stage, group.planned, group.preserveUnknown);
      maybeFault(`after-prepare-${index}`, faultAt);
    }
    transaction.phase = "prepared";
    await writeDurableJson(journal, transaction);
    maybeFault("after-prepared", faultAt);
    for (const [index, group] of transactionGroups.entries()) {
      await refreshPublicationLease(portalRoot, lease);
      const live = path.join(portalRoot, group.live);
      const stage = path.join(portalRoot, group.stage);
      const backup = path.join(portalRoot, group.backup);
      await assertNoSymlink(portalRoot, path.posix.dirname(group.live));
      await assertNoSymlink(portalRoot, path.posix.dirname(group.stage));
      await assertNoSymlink(portalRoot, path.posix.dirname(group.backup));
      await mkdir(path.dirname(backup), { recursive: true });
      if (group.had_live) await rename(live, backup);
      maybeFault(`after-backup-${index}`, faultAt);
      await mkdir(path.dirname(live), { recursive: true });
      await rename(stage, live);
      maybeFault(`after-publish-${index}`, faultAt);
    }
    transaction.phase = "committed";
    await writeDurableJson(journal, transaction);
    maybeFault("after-commit", faultAt);
    await rm(path.join(portalRoot, backupRoot), { recursive: true, force: true });
    await rm(path.join(portalRoot, stageRoot), { recursive: true, force: true });
    await rm(journal, { force: true });
  } catch (error) {
    if (!(simulateCrash && error?.code === "CODEFLOW_SIMULATED_CRASH")) {
      await recoverOwnedCorpusLocked(portalRoot, lease);
      await rm(path.join(portalRoot, stageRoot), { recursive: true, force: true });
      await rm(path.join(portalRoot, backupRoot), { recursive: true, force: true });
    }
    throw error;
  }
}

async function prepareOwnedStage(live, stage, planned, preserveUnknown) {
  const { inventory } = await inspectOwnedDirectory(live, preserveUnknown);
  const owned = new Set(inventory ?? []);
  await rm(stage, { recursive: true, force: true });
  await mkdir(stage, { recursive: true });
  let preservedBytes = 0;
  for (const relative of await walkFiles(live)) {
    if (relative === ".codeflow-generated.json" || owned.has(relative)) continue;
    if (planned.has(relative)) throw new Error(`refusing to overwrite unowned generated path: ${path.join(live, relative)}`);
    const source = path.join(live, relative);
    const metadata = await lstat(source);
    if (!metadata.isFile()) throw new Error(`non-regular file refused in generated corpus: ${source}`);
    preservedBytes += metadata.size;
    if (preservedBytes > MAX_PRESERVED_UNKNOWN_BYTES) throw new Error(`preserved unknown corpus exceeds ${MAX_PRESERVED_UNKNOWN_BYTES} bytes: ${live}`);
    const destination = path.join(stage, relative);
    await assertNoSymlink(live, path.posix.dirname(relative));
    await assertNoSymlink(stage, path.posix.dirname(relative));
    await mkdir(path.dirname(destination), { recursive: true });
    await copyFile(source, destination);
  }
  for (const [relative, content] of planned) await writeText(path.join(stage, relative), content);
  await writeText(path.join(stage, ".codeflow-generated.json"), `${JSON.stringify({ schema_version: 1, files: [...planned.keys()].sort() }, null, 2)}\n`);
}

async function inspectOwnedDirectory(dir, preserveUnknown) {
  const marker = path.join(dir, ".codeflow-generated.json");
  let inventory = null;
  try {
    const entries = await readdir(dir);
    if (!preserveUnknown && entries.length && !entries.includes(".codeflow-generated.json")) throw new Error(`refusing to manage unowned generated directory: ${dir}`);
    if (entries.includes(".codeflow-generated.json")) {
      const parsed = JSON.parse(await readFile(marker, "utf8"));
      if (parsed.schema_version !== 1 || !Array.isArray(parsed.files)) throw new Error(`invalid generated ownership inventory: ${marker}`);
      inventory = parsed.files.map((file) => safeRelative(file, "generated file"));
      if (new Set(inventory).size !== inventory.length) throw new Error(`duplicate generated ownership entry: ${marker}`);
    }
  } catch (error) { if (error?.code !== "ENOENT") throw error; }
  return { inventory };
}

async function walkFiles(root, relative = "", state = { count: 0 }) {
  const depth = relative ? relative.split("/").length : 0;
  if (depth > MAX_PRESERVED_UNKNOWN_DEPTH) throw new Error(`generated corpus depth exceeds ${MAX_PRESERVED_UNKNOWN_DEPTH}: ${root}`);
  let entries;
  try { entries = await readdir(path.join(root, relative), { withFileTypes: true }); }
  catch (error) { if (error?.code === "ENOENT") return []; throw error; }
  const files = [];
  for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
    const next = path.posix.join(relative, entry.name);
    if (entry.isSymbolicLink()) throw new Error(`symlink refused in generated corpus: ${path.join(root, next)}`);
    if (entry.isDirectory()) files.push(...await walkFiles(root, next, state));
    else if (entry.isFile()) {
      state.count += 1;
      if (state.count > MAX_PRESERVED_UNKNOWN_FILES) throw new Error(`generated corpus file count exceeds ${MAX_PRESERVED_UNKNOWN_FILES}: ${root}`);
      files.push(next);
    } else throw new Error(`non-regular entry refused in generated corpus: ${path.join(root, next)}`);
  }
  return files;
}

async function exists(target) {
  try { await lstat(target); return true; }
  catch (error) { if (error?.code === "ENOENT") return false; throw error; }
}

function validateTransaction(transaction) {
  const keys = Object.keys(transaction).sort().join(",");
  if (keys !== "backup_root,groups,phase,schema_version,stage_root" || transaction.schema_version !== 1 || !["preparing", "prepared", "committed"].includes(transaction.phase) || !Array.isArray(transaction.groups) || transaction.groups.length < 1 || transaction.groups.length > 3) {
    throw new Error("invalid portal publication journal");
  }
  const rootPattern = /^\.portal\/\.publish-(stage|backup)-[a-zA-Z0-9-]+$/;
  for (const [label, value, expected] of [["stage_root", transaction.stage_root, "stage"], ["backup_root", transaction.backup_root, "backup"]]) {
    safeRelative(value, label);
    const match = value.match(rootPattern);
    if (!match || match[1] !== expected) throw new Error(`invalid portal publication ${label}`);
  }
  const lives = new Set();
  transaction.groups.forEach((group, index) => {
    if (!group || typeof group !== "object" || Object.keys(group).sort().join(",") !== "backup,had_live,live,stage" || !PUBLICATION_LIVE_PATHS.has(group.live) || typeof group.had_live !== "boolean" || lives.has(group.live)) throw new Error("invalid portal publication group");
    lives.add(group.live);
    if (group.stage !== `${transaction.stage_root}/${index}` || group.backup !== `${transaction.backup_root}/${index}`) throw new Error("portal publication group is not bound to its transaction root");
    safeRelative(group.stage, "stage"); safeRelative(group.backup, "backup");
  });
}

function maybeFault(boundary, requested) {
  if (boundary !== requested) return;
  const error = new Error(`injected publication failure at ${boundary}`);
  error.code = "CODEFLOW_SIMULATED_CRASH";
  throw error;
}

async function writeDurableJson(file, value, { exclusive = false } = {}) {
  const encoded = `${JSON.stringify(value, null, 2)}\n`;
  if (exclusive) {
    const handle = await open(file, "wx", 0o600);
    try { await handle.writeFile(encoded, "utf8"); await handle.sync(); }
    finally { await handle.close(); }
    await syncDirectory(path.dirname(file));
    return;
  }
  const temporary = `${file}.${process.pid}.${Date.now()}.tmp`;
  try {
    const handle = await open(temporary, "wx", 0o600);
    try {
      await handle.writeFile(encoded, "utf8");
      await handle.sync();
    } finally { await handle.close(); }
    await rename(temporary, file);
    await syncDirectory(path.dirname(file));
  } catch (error) {
    await rm(temporary, { force: true });
    throw error;
  }
}

async function syncDirectory(directory) {
  if (process.platform === "win32") return;
  try { const handle = await open(directory, "r"); try { await handle.sync(); } finally { await handle.close(); } }
  catch (error) { if (!["EINVAL", "ENOTSUP", "EISDIR"].includes(error?.code)) throw error; }
}

export async function writeText(file, text) {
  await mkdir(path.dirname(file), { recursive: true });
  const temporary = path.join(path.dirname(file), `.${path.basename(file)}.${process.pid}.${Date.now()}.tmp`);
  try {
    const handle = await open(temporary, "wx", 0o600);
    try { await handle.writeFile(text, "utf8"); await handle.sync(); }
    finally { await handle.close(); }
    await rename(temporary, file);
    await syncDirectory(path.dirname(file));
  } catch (error) {
    await rm(temporary, { force: true });
    throw error;
  }
}

export async function assertNoSymlink(root, relative) {
  let current = root;
  try {
    if ((await lstat(current)).isSymbolicLink()) throw new Error(`symlink refused: ${current}`);
  } catch (error) { if (error?.code !== "ENOENT") throw error; }
  for (const part of safeRelative(relative).split("/")) {
    if (part === ".") continue;
    current = path.join(current, part);
    try {
      if ((await lstat(current)).isSymbolicLink()) throw new Error(`symlink refused: ${relative}`);
    } catch (error) {
      if (error?.code !== "ENOENT") throw error;
    }
  }
}

export async function collectBuiltArtifacts(root, limits = {}) {
  const maximumFiles = limits.maximumFiles ?? 50_000;
  const maximumDepth = limits.maximumDepth ?? 32;
  const maximumFileBytes = limits.maximumFileBytes ?? 64 * 1024 * 1024;
  const maximumTotalBytes = limits.maximumTotalBytes ?? 512 * 1024 * 1024;
  const onProgress = limits.onProgress;
  for (const [label, value] of Object.entries({ maximumFiles, maximumDepth, maximumFileBytes, maximumTotalBytes })) {
    if (!Number.isSafeInteger(value) || value < 1) throw new Error(`${label}: expected a positive safe integer`);
  }
  if (onProgress !== undefined && typeof onProgress !== "function") throw new Error("onProgress: expected a function");
  const rootMetadata = await lstat(root);
  if (rootMetadata.isSymbolicLink() || !rootMetadata.isDirectory()) throw new Error(`built artifact root is not a regular directory: ${root}`);
  const state = { count: 0, bytes: 0 };
  const files = [];
  async function visit(relative = "") {
    const depth = relative ? relative.split("/").length : 0;
    if (depth > maximumDepth) throw new Error(`built artifact depth exceeds ${maximumDepth}: ${root}`);
    const entries = await readdir(path.join(root, relative), { withFileTypes: true });
    for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
      const next = path.posix.join(relative, entry.name);
      if (entry.isSymbolicLink()) throw new Error(`built artifact symlink refused: ${next}`);
      if (entry.isDirectory()) { if (onProgress) await onProgress(); await visit(next); }
      else if (entry.isFile()) {
        state.count += 1;
        if (state.count > maximumFiles) throw new Error(`built artifact count exceeds ${maximumFiles}: ${root}`);
        const file = path.join(root, next);
        const result = await hashRegularFile(file, maximumFileBytes);
        state.bytes += result.bytes;
        if (state.bytes > maximumTotalBytes) throw new Error(`built artifact corpus exceeds ${maximumTotalBytes} bytes: ${root}`);
        files.push({ path: `dist/${next}`, sha256: result.sha256 });
        if (onProgress && state.count % 128 === 0) await onProgress();
      } else throw new Error(`non-regular built artifact refused: ${next}`);
    }
  }
  await visit();
  return files;
}

async function hashRegularFile(file, maximumBytes) {
  const flags = fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW ?? 0);
  const handle = await open(file, flags);
  try {
    const metadata = await handle.stat();
    if (!metadata.isFile() || metadata.size > maximumBytes) throw new Error(`built artifact exceeds ${maximumBytes} bytes or is not a regular file: ${file}`);
    const hash = createHash("sha256");
    for await (const chunk of handle.createReadStream({ autoClose: false })) hash.update(chunk);
    return { bytes: metadata.size, sha256: hash.digest("hex") };
  } finally { await handle.close(); }
}

export function strictId(value) {
  return /^(?:ADR|EPC|SPC|TSK|CAP)-\d{3,}(?:-\d{3,})?$/.test(value);
}

export function collectPageIds(frontmatter, text, sourcePath) {
  text = normalizeMarkdown(text);
  const declared = [];
  if (typeof frontmatter.id === "string" && strictId(frontmatter.id)) declared.push(frontmatter.id);
  if (sourcePath !== "docs/capabilities.md") {
    if (declared.length) return declared;
    const filenameId = path.posix.basename(sourcePath, ".md").toUpperCase();
    return strictId(filenameId) ? [filenameId] : [];
  }
  let fence = null;
  let yamlFence = false;
  for (const line of text.split("\n")) {
    const marker = line.match(/^\s{0,3}(`{3,}|~{3,})([^`]*)$/);
    if (marker) {
      const token = marker[1][0];
      if (fence === null) { fence = token; yamlFence = /^ya?ml\s*$/i.test(marker[2].trim()); }
      else if (fence === token) { fence = null; yamlFence = false; }
      continue;
    }
    if (fence !== null && !yamlFence) continue;
    if (fence === null) continue;
    const match = line.match(/^\s*-?\s*id:\s*((?:ADR|EPC|SPC|TSK|CAP)-\d{3,}(?:-\d{3,})?)\s*$/);
    if (match) declared.push(match[1]);
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

export function extractRelationships(frontmatter) {
  return relationshipFields.flatMap(([field, kind]) => {
    const value = frontmatter[field];
    const values = Array.isArray(value) ? value : value == null ? [] : [value];
    return values.filter((item) => typeof item === "string" && strictId(item)).map((target) => ({ type: kind, target }));
  });
}

export function extractPageRelationships(frontmatter, text, sourcePath) {
  const sourceId = typeof frontmatter.id === "string" && strictId(frontmatter.id) ? frontmatter.id : null;
  const relationships = extractRelationships(frontmatter).map((relationship) => ({ ...relationship, source_id: sourceId }));
  if (sourcePath !== "docs/capabilities.md") return relationships;
  for (const block of yamlFences(text)) {
    const record = YAML.parse(block);
    if (record && typeof record === "object" && !Array.isArray(record) && strictId(record.id ?? "")) {
      relationships.push(...extractRelationships(record).map((relationship) => ({ ...relationship, source_id: record.id })));
    }
  }
  return relationships;
}

function yamlFences(text) {
  text = normalizeMarkdown(text);
  const blocks = [];
  let fence = null;
  let yaml = false;
  let lines = [];
  for (const line of text.split("\n")) {
    const marker = line.match(/^\s{0,3}(`{3,}|~{3,})([^`]*)$/);
    if (marker) {
      const token = marker[1][0];
      if (fence === null) { fence = token; yaml = /^ya?ml\s*$/i.test(marker[2].trim()); lines = []; }
      else if (fence === token) { if (yaml) blocks.push(lines.join("\n")); fence = null; yaml = false; lines = []; }
      continue;
    }
    if (fence !== null && yaml) lines.push(line);
  }
  return blocks;
}

export function excerptFor(text) {
  text = normalizeMarkdown(text);
  const lines = text.split("\n");
  let inFrontmatter = text.startsWith("---\n");
  let inComment = false;
  let fence = null;
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (index === 0 && inFrontmatter) continue;
    if (inFrontmatter) { if (line === "---") inFrontmatter = false; continue; }
    const fenceMatch = line.match(/^\s{0,3}(`{3,}|~{3,})/);
    if (fenceMatch) { fence = fence === null ? fenceMatch[1][0] : fence === fenceMatch[1][0] ? null : fence; continue; }
    if (fence !== null) continue;
    let visible = line;
    if (inComment) {
      const end = visible.indexOf("-->");
      if (end < 0) continue;
      inComment = false; visible = visible.slice(end + 3);
    }
    while (visible.includes("<!--")) {
      const start = visible.indexOf("<!--"); const end = visible.indexOf("-->", start + 4);
      if (end < 0) { visible = visible.slice(0, start); inComment = true; break; }
      visible = visible.slice(0, start) + visible.slice(end + 3);
    }
    const trimmed = visible.trim();
    if (!trimmed || /^#{1,6}\s/.test(trimmed)) continue;
    const selected = [line];
    for (let cursor = index + 1; cursor < lines.length && selected.length < 3; cursor += 1) {
      if (!lines[cursor].trim()) break;
      selected.push(lines[cursor]);
    }
    return { start: index + 1, end: index + selected.length, text: selected.join("\n") };
  }
  return null;
}

export function validateBase(value) {
  if (typeof value !== "string" || !value.startsWith("/") || !value.endsWith("/") || value.includes("\\") || value.includes("?") || value.includes("#")) {
    throw new Error("base: expected an absolute URL path ending in /");
  }
  const parts = value.split("/").filter(Boolean);
  if (parts.some((part) => part === "." || part === "..")) throw new Error("base: traversal is not allowed");
  return value === "/" ? "/" : `/${parts.join("/")}/`;
}

export function withBase(base, route) {
  const safeRoute = safeRelative(route, "route");
  return `${validateBase(base)}${safeRoute}/`.replace(/^\/\//, "/");
}

export function validatePortalConfig(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("portal.config.json: expected an object");
  const allowed = new Set(["schema_version", "title", "description", "theme", "repository_url", "repository_root", "source_roots", "exclude", "layers", "base"]);
  for (const key of Object.keys(value)) if (!allowed.has(key)) throw new Error(`portal.config.json: unknown key ${key}`);
  if (value.schema_version !== 1) throw new Error("portal.config.json: unsupported schema_version");
  boundedString(value.title, "title", 1, 120);
  boundedString(value.description, "description", 1, 400);
  if (!["signal", "folio"].includes(value.theme)) throw new Error("portal.config.json: theme must be signal or folio");
  if (value.repository_url !== null) {
    boundedString(value.repository_url, "repository_url", 1, 2048);
    const url = new URL(value.repository_url);
    if (url.protocol !== "https:" || url.username || url.password || url.search || url.hash) throw new Error("repository_url: expected an HTTPS repository URL without credentials, query, or fragment");
  }
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

function boundedString(value, label, min, max) {
  if (typeof value !== "string" || value.length < min || value.length > max) throw new Error(`${label}: expected ${min} to ${max} characters`);
}

function validatePathArray(value, label, min, max) {
  if (!Array.isArray(value) || value.length < min || value.length > max) throw new Error(`${label}: expected ${min} to ${max} paths`);
  const paths = value.map((item) => safeRelative(item, label));
  if (new Set(paths).size !== paths.length) throw new Error(`${label}: duplicate path`);
}
