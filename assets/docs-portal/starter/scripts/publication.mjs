import { createHash, randomBytes } from "node:crypto";
import { constants as fsConstants } from "node:fs";
import { lstat, mkdir, open, readdir, rename, rm } from "node:fs/promises";
import path from "node:path";
import { compareDeterministicText, portablePathKey, safeRelative } from "./lib.mjs";

const PUBLICATION_LIVE_PATHS = new Set([".portal/generated", "src/content/docs", "public"]);
const RESERVED_PUBLIC_FILES = new Set(["llms.txt"]);
const RESERVED_PUBLIC_PREFIXES = ["markdown", "media"];
const MAX_PRESERVED_UNKNOWN_BYTES = 64 * 1024 * 1024;
const MAX_PRESERVED_UNKNOWN_FILES = 10_000;
const MAX_PRESERVED_UNKNOWN_DEPTH = 32;
const PUBLICATION_LEASE_MAX_AGE_MS = 30 * 60 * 1000;
const WORKFLOW_LEASE_MAX_AGE_MS = 4 * 60 * 60 * 1000;

export async function withWorkflowLease(portalRoot, action) {
  if (typeof action !== "function") throw new Error("workflow lease action must be a function");
  await assertNoSymlink(portalRoot, ".portal");
  const directory = path.join(portalRoot, ".portal/workflow.lock");
  await mkdir(path.dirname(directory), { recursive: true });
  const now = Date.now();
  const lease = { schema_version: 1, token: randomBytes(24).toString("hex"), created_at_ms: now, heartbeat_at_ms: now };
  try {
    await mkdir(directory, { mode: 0o700 });
  } catch (error) {
    if (error?.code !== "EEXIST") throw error;
    const existing = await readWorkflowLease(directory);
    if (Date.now() - existing.heartbeat_at_ms <= WORKFLOW_LEASE_MAX_AGE_MS) throw new Error("portal build/check workflow already in progress");
    const stale = `${directory}.stale-${process.pid}-${Date.now()}-${randomBytes(8).toString("hex")}`;
    await rename(directory, stale);
    await rm(stale, { recursive: true, force: true });
    await mkdir(directory, { mode: 0o700 });
  }
  await writeDurableJson(path.join(directory, "owner.json"), lease, { exclusive: true });
  let heartbeatError = null;
  const heartbeat = setInterval(() => {
    lease.heartbeat_at_ms = Date.now();
    writeDurableJson(path.join(directory, "owner.json"), lease).catch((error) => { heartbeatError = error; });
  }, 30_000);
  heartbeat.unref();
  try {
    const result = await action();
    if (heartbeatError) throw heartbeatError;
    return result;
  } finally {
    clearInterval(heartbeat);
    const current = await readWorkflowLease(directory).catch(() => null);
    if (current?.token === lease.token) await rm(directory, { recursive: true, force: true });
  }
}

async function readWorkflowLease(directory) {
  const metadata = await lstat(directory);
  if (metadata.isSymbolicLink() || !metadata.isDirectory()) throw new Error("unsafe portal workflow lease");
  const owner = path.join(directory, "owner.json");
  const value = JSON.parse(await readBoundedRegularFile(owner, 4096, "portal workflow lease owner"));
  validatePublicationLease(value);
  return value;
}

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
    const lease = JSON.parse(await readBoundedRegularFile(owner, 4096, "portal publication lease"));
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
  let journalBytes;
  try { journalBytes = await readBoundedRegularFile(journal, 64 * 1024, "portal publication journal"); }
  catch (error) { if (error?.code === "ENOENT") return; throw error; }
  const transaction = JSON.parse(journalBytes);
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

async function publishOwnedCorpusLocked(portalRoot, groups, { faultAt = null, simulateCrash = false, testHooks = {} } = {}, lease) {
  await assertPublicationLease(portalRoot, lease);
  if (!testHooks || typeof testHooks !== "object" || Array.isArray(testHooks)
    || Object.keys(testHooks).some((key) => key !== "afterPreservedOpen")
    || testHooks.afterPreservedOpen !== undefined && typeof testHooks.afterPreservedOpen !== "function") {
    throw new Error("publication testHooks are invalid");
  }
  if (!Array.isArray(groups) || groups.length < 1 || groups.length > 3 || new Set(groups.map((group) => group.live)).size !== groups.length) {
    throw new Error("generated corpus must contain 1 to 3 unique live groups");
  }
  const nonce = `${process.pid}-${Date.now()}-${randomBytes(12).toString("hex")}`;
  const stageRoot = `.portal/.publish-stage-${nonce}`;
  const backupRoot = `.portal/.publish-backup-${nonce}`;
  const transactionGroups = [];
  const preparedGroups = [];
  try {
    for (const [index, group] of groups.entries()) {
      if (!PUBLICATION_LIVE_PATHS.has(group.live)) throw new Error(`unsupported generated corpus path: ${group.live}`);
      const planned = new Map();
      const plannedKeys = new Set();
      for (const [relative, content] of group.files) {
        const safe = safeRelative(relative, "generated file");
        const portable = portablePathKey(safe, "generated file");
        if (planned.has(safe) || plannedKeys.has(portable)) throw new Error(`duplicate or case-colliding generated file: ${safe}`);
        plannedKeys.add(portable);
        planned.set(safe, content);
      }
      const live = path.join(portalRoot, group.live);
      const stageRelative = `${stageRoot}/${index}`;
      const backupRelative = `${backupRoot}/${index}`;
      transactionGroups.push({ live: group.live, stage: stageRelative, backup: backupRelative, had_live: await exists(live) });
      preparedGroups.push({ logicalLive: group.live, live, stage: path.join(portalRoot, stageRelative), planned, preserveUnknown: group.preserveUnknown === true });
    }
    const transaction = { schema_version: 1, phase: "preparing", stage_root: stageRoot, backup_root: backupRoot, groups: transactionGroups };
    validateTransaction(transaction);
    const journal = path.join(portalRoot, ".portal/publish-transaction.json");
    await mkdir(path.dirname(journal), { recursive: true });
    await writeDurableJson(journal, transaction, { exclusive: true });
    maybeFault("after-journal", faultAt);
    for (const [index, group] of preparedGroups.entries()) {
      await refreshPublicationLease(portalRoot, lease);
      await prepareOwnedStage(group.logicalLive, group.live, group.stage, group.planned, group.preserveUnknown, testHooks);
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

async function prepareOwnedStage(logicalLive, live, stage, planned, preserveUnknown, testHooks) {
  const { inventory } = await inspectOwnedDirectory(live, preserveUnknown, logicalLive === "public");
  const owned = new Set(inventory ?? []);
  await rm(stage, { recursive: true, force: true });
  await mkdir(stage, { recursive: true });
  let preservedBytes = 0;
  for (const relative of await walkFiles(live)) {
    if (relative === ".codeflow-generated.json" || owned.has(relative)) continue;
    if (logicalLive === "public" && isReservedPublicPath(relative)) continue;
    if (logicalLive === "public" && planned.has(relative)) continue;
    if (!preserveUnknown) throw new Error(`refusing uncommitted portal file: ${path.join(live, relative)}`);
    if (planned.has(relative)) throw new Error(`refusing to overwrite unowned generated path: ${path.join(live, relative)}`);
    const source = path.join(live, relative);
    const remaining = MAX_PRESERVED_UNKNOWN_BYTES - preservedBytes;
    const bytes = await readBoundedRegularFile(source, remaining, "preserved unknown portal file", {
      afterOpen: testHooks.afterPreservedOpen === undefined ? undefined : () => testHooks.afterPreservedOpen(source),
    });
    preservedBytes += bytes.length;
    const destination = path.join(stage, relative);
    await assertNoSymlink(live, path.posix.dirname(relative));
    await assertNoSymlink(stage, path.posix.dirname(relative));
    await mkdir(path.dirname(destination), { recursive: true });
    await writeText(destination, bytes);
  }
  for (const [relative, content] of planned) await writeText(path.join(stage, relative), content);
  await writeText(path.join(stage, ".codeflow-generated.json"), `${JSON.stringify({ schema_version: 1, files: [...planned.keys()].sort(compareDeterministicText) }, null, 2)}\n`);
}

export function isReservedPublicPath(relative) {
  const key = portablePathKey(relative, "public file");
  return [...RESERVED_PUBLIC_FILES].some((reserved) => key === portablePathKey(reserved, "reserved public file"))
    || RESERVED_PUBLIC_PREFIXES.some((prefix) => {
      const prefixKey = portablePathKey(prefix, "reserved public prefix");
      return key === prefixKey || key.startsWith(`${prefixKey}/`);
    });
}

async function inspectOwnedDirectory(dir, preserveUnknown, allowPlannedExisting = false) {
  const marker = path.join(dir, ".codeflow-generated.json");
  let inventory = null;
  try {
    const rootMetadata = await lstat(dir);
    if (rootMetadata.isSymbolicLink() || !rootMetadata.isDirectory()) throw new Error(`generated corpus root is not a regular directory: ${dir}`);
    const entries = await readdir(dir);
    if (!preserveUnknown && !allowPlannedExisting && entries.length && !entries.includes(".codeflow-generated.json")) throw new Error(`refusing to manage unowned generated directory: ${dir}`);
    if (entries.includes(".codeflow-generated.json")) {
      const parsed = JSON.parse(await readBoundedRegularFile(marker, 1024 * 1024, "generated ownership inventory"));
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed) || Object.keys(parsed).sort().join(",") !== "files,schema_version" || parsed.schema_version !== 1 || !Array.isArray(parsed.files) || parsed.files.length > MAX_PRESERVED_UNKNOWN_FILES) throw new Error(`invalid generated ownership inventory: ${marker}`);
      inventory = parsed.files.map((file) => safeRelative(file, "generated file"));
      if (new Set(inventory.map((file) => portablePathKey(file, "generated file"))).size !== inventory.length) throw new Error(`duplicate or case-colliding generated ownership entry: ${marker}`);
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
  for (const entry of entries.sort((a, b) => compareDeterministicText(a.name, b.name))) {
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
  if (relative === ".") return;
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

export async function assertToolOutputRoots(root, relatives) {
  if (!Array.isArray(relatives) || !relatives.length) throw new Error("tool output roots must be a non-empty array");
  for (const relative of relatives) {
    const safe = safeRelative(relative, "tool output root");
    await assertNoSymlink(root, safe);
    const output = path.join(root, safe);
    let metadata;
    try { metadata = await lstat(output); }
    catch (error) { if (error?.code === "ENOENT") continue; throw error; }
    if (!metadata.isDirectory()) throw new Error(`tool output root is not a regular directory: ${safe}`);
    const pending = [output];
    let entriesSeen = 0;
    while (pending.length) {
      const directory = pending.pop();
      for (const entry of await readdir(directory, { withFileTypes: true })) {
        entriesSeen += 1;
        if (entriesSeen > 50_000) throw new Error(`tool output root contains too many entries: ${safe}`);
        const candidate = path.join(directory, entry.name);
        if (entry.isSymbolicLink()) throw new Error(`symlink refused in tool output root: ${path.relative(root, candidate)}`);
        if (entry.isDirectory()) pending.push(candidate);
        else if (!entry.isFile()) throw new Error(`non-regular entry refused in tool output root: ${path.relative(root, candidate)}`);
      }
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
  const state = { count: 0, bytes: 0, portableKeys: new Set() };
  const files = [];
  async function visit(relative = "") {
    const depth = relative ? relative.split("/").length : 0;
    if (depth > maximumDepth) throw new Error(`built artifact depth exceeds ${maximumDepth}: ${root}`);
    const entries = await readdir(path.join(root, relative), { withFileTypes: true });
    for (const entry of entries.sort((a, b) => compareDeterministicText(a.name, b.name))) {
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
        const artifactPath = `dist/${safeRelative(next, "built artifact")}`;
        const portable = portablePathKey(artifactPath, "built artifact");
        if (state.portableKeys.has(portable)) throw new Error(`built artifact path collides case-insensitively: ${artifactPath}`);
        state.portableKeys.add(portable);
        files.push({ path: artifactPath, sha256: result.sha256 });
        if (onProgress && state.count % 128 === 0) await onProgress();
      } else throw new Error(`non-regular built artifact refused: ${next}`);
    }
  }
  await visit();
  return files;
}

export function assertExpectedPageArtifacts(pages, artifacts) {
  if (!Array.isArray(pages) || !Array.isArray(artifacts)) throw new Error("page artifact evidence must be arrays");
  const paths = new Set(artifacts.map((artifact) => safeRelative(artifact?.path, "built artifact")));
  for (const page of pages) {
    const route = safeRelative(page?.route, "page route");
    const expected = route === "index" ? "dist/index.html" : `dist/${route}/index.html`;
    if (!paths.has(expected)) throw new Error(`built artifacts omit the exact page route: ${expected}`);
  }
}

async function hashRegularFile(file, maximumBytes) {
  const { handle, opened } = await openStableRegularFile(file, maximumBytes, "built artifact");
  try {
    const hash = createHash("sha256");
    let bytes = 0;
    for await (const chunk of handle.createReadStream({ autoClose: false })) {
      bytes += chunk.length;
      if (bytes > maximumBytes) throw new Error(`built artifact exceeds ${maximumBytes} bytes: ${file}`);
      hash.update(chunk);
    }
    await assertStableRegularFile(file, handle, opened, bytes, "built artifact");
    return { bytes, sha256: hash.digest("hex") };
  } finally { await handle.close(); }
}

export async function readBoundedRegularFile(file, maximumBytes, label = "file", testHooks = {}) {
  if (!Number.isSafeInteger(maximumBytes) || maximumBytes < 0) throw new Error("maximumBytes: expected a non-negative safe integer");
  if (testHooks.afterOpen !== undefined && typeof testHooks.afterOpen !== "function") throw new Error("afterOpen: expected a function");
  const { handle, opened } = await openStableRegularFile(file, maximumBytes, label);
  try {
    if (testHooks.afterOpen) await testHooks.afterOpen();
    const chunks = [];
    let bytes = 0;
    let position = 0;
    while (true) {
      const remaining = maximumBytes - bytes;
      const buffer = Buffer.allocUnsafe(Math.min(64 * 1024, remaining + 1));
      const result = await handle.read(buffer, 0, buffer.length, position);
      if (result.bytesRead === 0) break;
      bytes += result.bytesRead;
      if (bytes > maximumBytes) throw new Error(`${label} exceeds ${maximumBytes} bytes: ${file}`);
      chunks.push(buffer.subarray(0, result.bytesRead));
      position += result.bytesRead;
    }
    await assertStableRegularFile(file, handle, opened, bytes, label);
    return Buffer.concat(chunks, bytes);
  } finally { await handle.close(); }
}

async function openStableRegularFile(file, maximumBytes, label) {
  const flags = fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW ?? 0);
  const handle = await open(file, flags);
  try {
    const opened = await handle.stat({ bigint: true });
    const linked = await lstat(file, { bigint: true });
    if (!opened.isFile() || linked.isSymbolicLink() || !linked.isFile() || !sameFile(opened, linked) || opened.size > BigInt(maximumBytes)) {
      throw new Error(`${label} exceeds ${maximumBytes} bytes or is not one stable regular file: ${file}`);
    }
    return { handle, opened };
  } catch (error) {
    await handle.close();
    throw error;
  }
}

async function assertStableRegularFile(file, handle, opened, bytes, label) {
  const [after, linked] = await Promise.all([handle.stat({ bigint: true }), lstat(file, { bigint: true })]);
  if (!after.isFile() || linked.isSymbolicLink() || !linked.isFile() || !sameFile(opened, after) || !sameFile(after, linked)
    || after.size !== BigInt(bytes) || opened.size !== after.size || opened.mtimeNs !== after.mtimeNs || opened.ctimeNs !== after.ctimeNs) {
    throw new Error(`${label} changed while it was being read: ${file}`);
  }
}

function sameFile(left, right) {
  return left.dev === right.dev && left.ino === right.ino;
}
