// The authority contract for everything a shared subject source declares.
//
// A shared source under `shared/` is repository-controlled data, not trusted
// code, and everything it names — a file to quote from, a revision to read, a
// path inside a diff — reaches an interpreter, the filesystem or `git`. This
// module is the single place where such a value is qualified before it does.
//
// Three separate boundaries live here:
//
//   1. `evaluateSharedSource` runs a shared source in a fresh node:vm context
//      with no Node globals and code generation disabled, bounded by a wall
//      clock, and returns a JSON snapshot taken inside that same bounded
//      context. Nothing the source defines ever executes in the host realm.
//   2. `resolveInsideRoot` / `readInsideRoot` admit only a canonical regular
//      file inside one explicitly named root. There is no fallback root: a
//      repository source is never satisfied by a study file and a study source
//      is never satisfied by a repository file.
//   3. `isQualifiedRevision` and `literalPathspec` qualify the two values that
//      reach `git` arguments, so neither can become an option and neither can
//      widen a pathspec beyond the literal path it names.
//
// What is bounded and what is not, stated exactly: the vm timeout bounds
// synchronous execution, so an infinite loop fails the run instead of hanging
// it. Memory is not bounded, and a context is not a security sandbox against a
// determined V8 escape — it is a boundary against the accidental and the
// careless, which is what a repository-controlled study source is.

import { readFile, realpath, stat } from "node:fs/promises";
import path, { resolve } from "node:path";
import { createContext, Script } from "node:vm";
import { isContained } from "./path-containment.mjs";

export const SHARED_SOURCE_TIMEOUT_MS = 5_000;

// A snapshot helper installed before the source runs, holding a pristine
// `JSON.stringify` in its closure and pinned to a non-writable, non-
// configurable global, so a source cannot replace what reads it back out.
const SNAPSHOT_SETUP = `(() => {
  const stringify = JSON.stringify;
  Object.defineProperty(globalThis, "__cfSnapshot", {
    value: (name) => stringify(globalThis.window[name] ?? null),
    writable: false, configurable: false, enumerable: false,
  });
})();`;

/**
 * Evaluate a shared subject source and return a plain JSON snapshot of the
 * global it defines. Functions, Maps and every other live reference are dropped
 * by construction: callers recompute from the raw facts instead.
 */
export function evaluateSharedSource(code, globalName, options = {}) {
  const timeout = options.timeoutMs ?? SHARED_SOURCE_TIMEOUT_MS;
  const filename = options.filename ?? "shared-source";
  if (typeof code !== "string") return { ok: false, reason: "source is not text" };

  let context;
  try {
    context = createContext({ window: {} }, {
      name: filename,
      codeGeneration: { strings: false, wasm: false },
    });
    new Script(SNAPSHOT_SETUP, { filename: "source-authority:snapshot" }).runInContext(context, { timeout });
  } catch (error) {
    return { ok: false, reason: `the bounded context could not be prepared (${describe(error)})` };
  }

  try {
    // A vm context cannot load a module at all without --experimental-vm-modules,
    // so `import()` never yields one. The explicit callback makes that a normal
    // refusal rather than an internal loader error; a source that leaves the
    // refusal unattended fails the run instead of continuing quietly.
    new Script(code, {
      filename,
      importModuleDynamically: () => { throw new Error("a shared source may not import modules"); },
    }).runInContext(context, { timeout, displayErrors: false });
  } catch (error) {
    return { ok: false, reason: `did not evaluate inside the bounded context (${describe(error)})` };
  }

  let json;
  try {
    json = new Script(`__cfSnapshot(${JSON.stringify(globalName)})`, { filename: "source-authority:snapshot" })
      .runInContext(context, { timeout });
  } catch (error) {
    return { ok: false, reason: `window.${globalName} could not be snapshotted (${describe(error)})` };
  }
  if (typeof json !== "string") return { ok: false, reason: `did not define window.${globalName}` };

  let value;
  try { value = JSON.parse(json); } catch { return { ok: false, reason: `window.${globalName} is not JSON data` }; }
  if (value === null || typeof value !== "object") return { ok: false, reason: `did not define window.${globalName}` };
  return { ok: true, value };
}

const describe = (error) => `${error?.name ?? "Error"}: ${String(error?.message ?? error).split("\n")[0]}`;

// ---------------------------------------------------------- declared paths

/**
 * Pure lexical qualification of a path a shared source declares. It runs before
 * any I/O, so a rejected path is never opened, and before any `git` call, so a
 * rejected path never becomes an option or a pathspec with magic.
 */
export function classifyDeclaredPath(rel) {
  if (typeof rel !== "string" || rel === "") return no("is not a non-empty string");
  if (/[\u0000-\u001f]/u.test(rel)) return no("contains a control character");
  if (rel.startsWith("-")) return no("starts with '-', which git would read as an option");
  if (rel.startsWith(":")) return no("starts with ':', which git would read as pathspec magic");
  if (rel.includes("\\")) return no("contains a backslash");
  if (path.posix.isAbsolute(rel) || /^[A-Za-z]:/u.test(rel)) return no("is an absolute path");
  const segments = rel.split("/");
  if (segments.some((segment) => segment === "")) return no("has an empty path segment");
  if (segments.some((segment) => segment === "..")) return no("walks upward through '..'");
  if (segments.some((segment) => segment === ".")) return no("contains a '.' segment");
  return { ok: true };
}

const no = (reason) => ({ ok: false, reason });

/** The literal, root-relative pathspec form of an already-qualified path. */
export function literalPathspec(rel) {
  return `:(literal,top)${rel}`;
}

// -------------------------------------------------------------- revisions

/**
 * The only shape a shared source may name as a revision: an abbreviated or full
 * hex object id. Nothing else — no ref name, no range, no option, no revision
 * expression — reaches a `git` argument.
 */
export const QUALIFIED_REVISION = /^[0-9a-f]{7,40}$/u;
export const isQualifiedRevision = (value) => typeof value === "string" && QUALIFIED_REVISION.test(value);

// ------------------------------------------------------------ file access

/**
 * Resolve a declared path inside exactly one root: lexically contained, then
 * canonically contained after every symlink is followed, and a regular file.
 * Every rejection carries its reason; nothing falls back to another root and
 * nothing degrades to empty content.
 */
export async function resolveInsideRoot(root, rel) {
  const lexical = classifyDeclaredPath(rel);
  if (!lexical.ok) return lexical;

  let canonicalRoot;
  try { canonicalRoot = await realpath(root); }
  catch (error) { return no(`its root ${root} could not be resolved (${describe(error)})`); }

  const candidate = resolve(canonicalRoot, rel);
  if (!isContained(canonicalRoot, candidate, path)) return no("resolves outside its root");

  let canonical;
  try { canonical = await realpath(candidate); }
  catch (error) { return no(`could not be resolved on disk (${describe(error)})`); }
  if (!isContained(canonicalRoot, canonical, path)) {
    return no(`resolves through a link to ${canonical}, outside its root`);
  }

  let stats;
  try { stats = await stat(canonical); }
  catch (error) { return no(`could not be inspected (${describe(error)})`); }
  if (!stats.isFile()) return no("is not a regular file");

  return { ok: true, path: canonical };
}

/** Read a declared path under `resolveInsideRoot`. Empty content is a failure. */
export async function readInsideRoot(root, rel) {
  const resolved = await resolveInsideRoot(root, rel);
  if (!resolved.ok) return resolved;
  let raw;
  try { raw = await readFile(resolved.path, "utf8"); }
  catch (error) { return no(`could not be read (${describe(error)})`); }
  if (raw.trim() === "") return no("is empty");
  return { ok: true, raw, path: resolved.path };
}

// ------------------------------------------------------- self-test tables
// Small explicit tables so the negative behaviour is checked as pure logic,
// including the Windows spellings this study's live runs cannot exercise.

export const DECLARED_PATH_CASES = [
  { value: "AGENTS.md", ok: true, why: "an ordinary repository file" },
  { value: "docs/decisions/ADR-0052-x.md", ok: true, why: "a nested repository file" },
  { value: "", ok: false, why: "empty" },
  { value: null, ok: false, why: "not a string" },
  { value: "/etc/hosts", ok: false, why: "absolute POSIX path" },
  { value: "C:/Windows/win.ini", ok: false, why: "absolute Windows path" },
  { value: "..\\..\\AGENTS.md", ok: false, why: "Windows separator" },
  { value: "../../../etc/hosts", ok: false, why: "upward traversal" },
  { value: "docs/../../AGENTS.md", ok: false, why: "traversal in the middle" },
  { value: "./AGENTS.md", ok: false, why: "a '.' segment" },
  { value: "docs//product.md", ok: false, why: "an empty segment" },
  { value: "-o/tmp/x", ok: false, why: "option-like" },
  { value: "--output=/tmp/x", ok: false, why: "an explicit git option" },
  { value: ":(glob)**", ok: false, why: "pathspec glob magic" },
  { value: ":!AGENTS.md", ok: false, why: "pathspec exclude magic" },
  { value: ":/AGENTS.md", ok: false, why: "pathspec top magic" },
  { value: "AGENTS\u0000.md", ok: false, why: "a NUL byte" },
];

export const REVISION_CASES = [
  { value: "86582dc0", ok: true, why: "an abbreviated object id" },
  { value: "826715510440df53fc999e68d77429d7b49a6d55", ok: true, why: "a full object id" },
  { value: "86582dc", ok: true, why: "exactly seven hex digits, git's default abbreviation" },
  { value: "86582d", ok: false, why: "shorter than seven hex digits" },
  { value: "826715510440df53fc999e68d77429d7b49a6d55a", ok: false, why: "longer than a full object id" },
  { value: "86582dcg", ok: false, why: "a non-hex character" },
  { value: "HEAD", ok: false, why: "a ref name, not an object id" },
  { value: "main~3", ok: false, why: "a revision expression" },
  { value: "86582dc0..a47b62c3", ok: false, why: "a range" },
  { value: "--output=/tmp/x", ok: false, why: "an option that makes git write a file" },
  { value: "--all", ok: false, why: "an option" },
  { value: "-1", ok: false, why: "a short option" },
  { value: "86582DC0", ok: false, why: "upper case, which git log would not echo back" },
  { value: "", ok: false, why: "empty" },
  { value: null, ok: false, why: "not a string" },
];
