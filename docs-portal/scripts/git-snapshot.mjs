import { execFileSync } from "node:child_process";
import { TextDecoder } from "node:util";
import { compareDeterministicText, portablePathKey, safeRelative } from "./lib.mjs";
import { hardenedChildEnvironment } from "./process-environment.mjs";

const MAX_GIT_TREE_BYTES = 64 * 1024 * 1024;
const MAX_GIT_STATUS_BYTES = 8 * 1024 * 1024;
const MAX_REPOSITORY_FILES = 100_000;
const MAX_PATHS_PER_STATUS = 64;
const MAX_STATUS_PATHSPEC_UTF16_UNITS = 8 * 1024;
const GIT_TIMEOUT_MS = 30_000;
const REGULAR_MODES = new Set(["100644", "100755"]);
const FULL_OBJECT_ID = /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/;

export class GitSnapshot {
  constructor(repositoryRoot, { onCommand = () => {} } = {}) {
    this.repositoryRoot = repositoryRoot;
    this.onCommand = onCommand;
    this.inventory = null;
  }

  resolveHead() {
    const commit = this.text(["rev-parse", "--verify", "HEAD^{commit}"], 1024, "repository HEAD").trim();
    if (!FULL_OBJECT_ID.test(commit)) throw new Error("repository HEAD must resolve to a full Git commit");
    return commit;
  }

  loadInventory(commit) {
    if (!FULL_OBJECT_ID.test(commit)) throw new Error("Git inventory requires a full commit ID");
    const listing = this.text(["ls-tree", "-r", "-z", "--full-tree", commit], MAX_GIT_TREE_BYTES, "repository tree");
    const inventory = new Map();
    const portable = new Set();
    for (const raw of listing.split("\0")) {
      if (!raw) continue;
      const separator = raw.indexOf("\t");
      if (separator < 0) throw new Error("Git repository tree contains an invalid record");
      const [mode, type, oid] = raw.slice(0, separator).split(" ");
      const file = safeRelative(raw.slice(separator + 1), "committed repository path");
      if (!FULL_OBJECT_ID.test(oid ?? "") || !["blob", "commit"].includes(type)) throw new Error(`unsupported Git tree entry: ${file}`);
      const key = portablePathKey(file, "committed repository path");
      if (portable.has(key)) throw new Error(`committed repository has a portable path collision: ${file}`);
      portable.add(key);
      inventory.set(file, { path: file, mode, type, oid });
      if (inventory.size > MAX_REPOSITORY_FILES) throw new Error(`repository file count exceeds ${MAX_REPOSITORY_FILES}`);
    }
    this.inventory = inventory;
    return inventory;
  }

  requireRegular(pathText, allowedModes, label) {
    const file = safeRelative(pathText, label);
    const record = this.#inventory().get(file);
    if (!record || record.type !== "blob" || !REGULAR_MODES.has(record.mode) || !allowedModes.includes(record.mode)) {
      throw new Error(`${label} must be one committed regular file with an allowed mode: ${file}`);
    }
    return record;
  }

  requireDirectory(pathText, label) {
    const directory = safeRelative(pathText, label);
    if (this.#inventory().has(directory)) throw new Error(`${label} must be a committed directory, not a file: ${directory}`);
    const records = [...this.#inventory().values()].filter((record) => record.path.startsWith(`${directory}/`));
    if (!records.length) throw new Error(`${label} must be a non-empty committed directory: ${directory}`);
    return records.sort((left, right) => compareDeterministicText(left.path, right.path));
  }

  recordsForInputs(inputs, label) {
    const records = new Map();
    for (const input of inputs) {
      const safe = safeRelative(input, label);
      const exact = this.#inventory().get(safe);
      const selected = exact ? [exact] : this.requireDirectory(safe, label);
      for (const record of selected) records.set(record.path, record);
    }
    if (!records.size) throw new Error(`${label} set is empty`);
    return [...records.values()].sort((left, right) => compareDeterministicText(left.path, right.path));
  }

  readBlobs(records, { perObjectBytes, totalBytes, label }) {
    const unique = new Map();
    for (const record of records) {
      if (!record || record.type !== "blob" || !REGULAR_MODES.has(record.mode) || !FULL_OBJECT_ID.test(record.oid ?? "")) {
        throw new Error(`${label} contains an invalid committed object record`);
      }
      unique.set(record.oid, record);
    }
    if (unique.size === 0) return new Map();
    const ordered = [...unique.values()].sort((left, right) => compareDeterministicText(left.oid, right.oid));
    const framingBytes = ordered.length * 96 + 1;
    const output = this.bytes(["cat-file", "--batch"], totalBytes + framingBytes, label, `${ordered.map((record) => record.oid).join("\n")}\n`);
    const byOid = new Map();
    let offset = 0;
    let aggregate = 0;
    for (const expected of ordered) {
      const lineEnd = output.indexOf(0x0a, offset);
      if (lineEnd < 0) throw new Error(`${label}: Git batch response ended before its header`);
      const header = output.subarray(offset, lineEnd).toString("ascii").split(" ");
      if (header.length !== 3 || header[0] !== expected.oid || header[1] !== "blob" || !/^\d+$/.test(header[2])) {
        throw new Error(`${label}: Git batch returned an invalid or missing object`);
      }
      const size = Number(header[2]);
      if (!Number.isSafeInteger(size) || size > perObjectBytes) throw new Error(`${label} object exceeds ${perObjectBytes} bytes`);
      aggregate += size;
      if (aggregate > totalBytes) throw new Error(`${label} corpus exceeds ${totalBytes} bytes`);
      const start = lineEnd + 1;
      const end = start + size;
      if (end >= output.length || output[end] !== 0x0a) throw new Error(`${label}: Git batch response has invalid framing`);
      byOid.set(expected.oid, output.subarray(start, end));
      offset = end + 1;
    }
    if (offset !== output.length) throw new Error(`${label}: Git batch returned unclaimed bytes`);
    return new Map(records.map((record) => [record.path, Buffer.from(byOid.get(record.oid))]));
  }

  assertClean(paths) {
    const configured = new Set(paths.map((item) => safeRelative(item, "snapshot path")));
    const watched = minimalRoots([...configured]);
    const pathspecs = watched.map((item) => `:(top,literal)${item}`);
    let remainingBytes = MAX_GIT_STATUS_BYTES;
    // Git status is not atomic across batches. The adapter therefore repeats
    // byte-for-byte input checks and the HEAD check around publication.
    for (const batch of boundedPathspecBatches(pathspecs)) {
      const status = this.text(["status", "--porcelain=v1", "-z", "--untracked-files=all", "--ignored=matching", "--", ...batch], remainingBytes, "repository status");
      remainingBytes -= Buffer.byteLength(status);
      for (const { code, path: dirty } of parsePorcelainRecords(status)) {
        if (!watched.some((item) => dirty === item || dirty.startsWith(`${item}/`))) continue;
        if (this.#exemptLitter(code, dirty, configured)) continue;
        throw new Error(`configured portal input must match HEAD exactly: ${dirty}`);
      }
    }
  }

  // Harness and tool litter such as .claude/.cc-writes or __pycache__ may sit
  // inside a watched root without being portal input. Exempt one only when Git
  // reports it ignored, which is untracked and matched by the repository's
  // .gitignore, and it is neither a configured input nor a file committed at
  // HEAD. A tracked change stays dirty even when a pattern would match it, and
  // an untracked file the repository does not ignore stays dirty.
  #exemptLitter(code, dirty, configured) {
    if (code !== "!!") return false;
    if (configured.has(dirty)) return false;
    if (!(this.inventory instanceof Map)) return false;
    if (this.inventory.has(dirty)) return false;
    return ![...configured].some((item) => item.startsWith(`${dirty}/`));
  }

  text(args, maximumBytes, label) {
    return new TextDecoder("utf-8", { fatal: true }).decode(this.bytes(args, maximumBytes, label));
  }

  bytes(args, maximumBytes, label, input = undefined) {
    this.onCommand(args);
    try {
      return execFileSync("git", [
        "-c", "core.fsmonitor=false",
        "-c", "core.pager=cat",
        "-c", "pager.status=false",
        "-C", this.repositoryRoot,
        ...args,
      ], {
        encoding: null,
        env: hardenedGitEnvironment(),
        input,
        maxBuffer: maximumBytes + 1,
        timeout: GIT_TIMEOUT_MS,
        windowsHide: true,
      });
    } catch (error) {
      const detail = String(error.stderr ?? error.message ?? error).trim().slice(0, 2048);
      throw new Error(`Git snapshot ${label} failed (${args[0]}): ${detail || "no diagnostic"}`);
    }
  }

  #inventory() {
    if (!(this.inventory instanceof Map)) throw new Error("Git repository inventory is not loaded");
    return this.inventory;
  }
}

function minimalRoots(paths) {
  const roots = [...new Set(paths)].sort((left, right) => left.length - right.length || compareDeterministicText(left, right));
  return roots.filter((candidate, index) => !roots.slice(0, index).some((root) => candidate === root || candidate.startsWith(`${root}/`)));
}

export function boundedPathspecBatches(pathspecs) {
  const batches = [];
  let batch = [];
  let units = 0;
  for (const pathspec of pathspecs) {
    const nextUnits = pathspec.length + 1;
    if (nextUnits > MAX_STATUS_PATHSPEC_UTF16_UNITS) throw new Error("snapshot path is too long for a bounded Git status command");
    if (batch.length && (batch.length >= MAX_PATHS_PER_STATUS || units + nextUnits > MAX_STATUS_PATHSPEC_UTF16_UNITS)) {
      batches.push(batch);
      batch = [];
      units = 0;
    }
    batch.push(pathspec);
    units += nextUnits;
  }
  if (batch.length) batches.push(batch);
  return batches;
}

function parsePorcelainRecords(status) {
  const records = status.split("\0");
  const parsed = [];
  for (let index = 0; index < records.length; index += 1) {
    const record = records[index];
    if (!record) continue;
    if (record.length < 4 || record[2] !== " ") throw new Error("Git status returned an invalid porcelain record");
    const code = record.slice(0, 2);
    parsed.push({ code, path: statusPath(record.slice(3)) });
    if (["R", "C"].includes(record[0])) {
      index += 1;
      if (!records[index]) throw new Error("Git status rename record is incomplete");
      parsed.push({ code, path: statusPath(records[index]) });
    }
  }
  return parsed;
}

function statusPath(value) {
  const stripped = value.endsWith("/") ? value.slice(0, -1) : value;
  return safeRelative(stripped, "Git status path");
}

export function hardenedGitEnvironment(source = process.env) {
  return {
    ...hardenedChildEnvironment(source),
    GIT_CONFIG_GLOBAL: process.platform === "win32" ? "NUL" : "/dev/null",
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_NO_LAZY_FETCH: "1",
    GIT_NO_REPLACE_OBJECTS: "1",
    GIT_OPTIONAL_LOCKS: "0",
    GIT_PAGER: "cat",
    GIT_TERMINAL_PROMPT: "0",
    LC_ALL: "C",
    PAGER: "cat",
  };
}
